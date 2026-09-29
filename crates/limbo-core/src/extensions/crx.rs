//! CRX3 parsing and verification.
//!
//! Layout: `"Cr24"`, u32 LE version (3), u32 LE header length, a `CrxFileHeader`
//! protobuf, then the ZIP archive. The extension ID is the first 16 bytes of
//! SHA-256 over a developer RSA public key, spelled with the letters a–p; the
//! key's signature covers `"CRX3 SignedData\0" ‖ u32 LE len ‖ signed_header_data ‖ archive`.
//! A package is accepted only if the ID matches and that signature verifies.

use rsa::pkcs8::DecodePublicKey;
use rsa::{Pkcs1v15Sign, RsaPublicKey};
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

const MAGIC: &[u8; 4] = b"Cr24";
const SIGNATURE_CONTEXT: &[u8] = b"CRX3 SignedData\x00";

#[derive(Debug)]
pub struct VerifiedCrx<'a> {
    pub id: String,
    /// The ZIP archive (points into the input).
    pub archive: &'a [u8],
    /// SubjectPublicKeyInfo DER of the developer key.
    pub public_key: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Proof {
    public_key: Vec<u8>,
    signature: Vec<u8>,
}

fn bad(msg: &str) -> Error {
    Error::Format(format!("CRX: {msg}"))
}

// --- protobuf (just enough) -------------------------------------------------

fn varint(data: &[u8]) -> Result<(u64, &[u8])> {
    let mut v = 0u64;
    for (i, &b) in data.iter().enumerate().take(10) {
        v |= ((b & 0x7f) as u64) << (7 * i);
        if b & 0x80 == 0 {
            return Ok((v, &data[i + 1..]));
        }
    }
    Err(bad("bad varint"))
}

/// Iterates `(field number, wire type, payload)`; payload is the bytes for
/// length-delimited fields and empty otherwise.
fn fields(mut data: &[u8]) -> Result<Vec<(u64, u8, &[u8])>> {
    let mut out = Vec::new();
    while !data.is_empty() {
        let (key, rest) = varint(data)?;
        let (field, wire) = (key >> 3, (key & 7) as u8);
        data = match wire {
            0 => varint(rest)?.1,
            1 => rest.get(8..).ok_or_else(|| bad("truncated fixed64"))?,
            5 => rest.get(4..).ok_or_else(|| bad("truncated fixed32"))?,
            2 => {
                let (len, rest) = varint(rest)?;
                let len = usize::try_from(len).map_err(|_| bad("length"))?;
                if rest.len() < len {
                    return Err(bad("truncated field"));
                }
                out.push((field, wire, &rest[..len]));
                &rest[len..]
            }
            _ => return Err(bad("unsupported wire type")),
        };
    }
    Ok(out)
}

fn parse_proof(data: &[u8]) -> Result<Proof> {
    let mut p = Proof { public_key: Vec::new(), signature: Vec::new() };
    for (f, _, v) in fields(data)? {
        match f {
            1 => p.public_key = v.to_vec(),
            2 => p.signature = v.to_vec(),
            _ => {}
        }
    }
    Ok(p)
}

/// Extension ID for a 16-byte hash prefix.
pub fn id_from_hash(hash: &[u8]) -> String {
    hash.iter().take(16).flat_map(|b| [b >> 4, b & 0xf]).map(|n| (b'a' + n) as char).collect()
}

/// Extension ID for a SubjectPublicKeyInfo DER key.
pub fn id_from_public_key(spki: &[u8]) -> String {
    id_from_hash(&Sha256::digest(spki))
}

/// Parses and verifies a CRX3 package. `expected_id` guards against a store
/// serving a different extension than the one the user chose.
pub fn verify<'a>(data: &'a [u8], expected_id: Option<&str>) -> Result<VerifiedCrx<'a>> {
    if data.len() < 12 || &data[..4] != MAGIC {
        return Err(bad("not a CRX file"));
    }
    let version = u32::from_le_bytes(data[4..8].try_into().unwrap());
    match version {
        3 => {}
        2 => return Err(Error::Unsupported("CRX2 packages are no longer accepted".into())),
        v => return Err(bad(&format!("unknown version {v}"))),
    }
    let header_len = u32::from_le_bytes(data[8..12].try_into().unwrap()) as usize;
    let header = data.get(12..12 + header_len).ok_or_else(|| bad("truncated header"))?;
    let archive = &data[12 + header_len..];

    let mut rsa_proofs = Vec::new();
    let mut signed_header_data: Option<&[u8]> = None;
    for (f, wire, v) in fields(header)? {
        match (f, wire) {
            (2, 2) => rsa_proofs.push(parse_proof(v)?),
            (10000, 2) => signed_header_data = Some(v),
            _ => {}
        }
    }
    let signed = signed_header_data.ok_or_else(|| bad("missing signed header data"))?;
    let crx_id = fields(signed)?
        .into_iter()
        .find(|(f, w, _)| *f == 1 && *w == 2)
        .map(|(_, _, v)| v)
        .filter(|v| v.len() == 16)
        .ok_or_else(|| bad("missing crx_id"))?;
    let id = id_from_hash(crx_id);
    if let Some(expected) = expected_id {
        if !expected.eq_ignore_ascii_case(&id) {
            return Err(bad(&format!("package is for {id}, expected {expected}")));
        }
    }

    let mut message = Vec::with_capacity(SIGNATURE_CONTEXT.len() + 4 + signed.len());
    message.extend_from_slice(SIGNATURE_CONTEXT);
    message.extend_from_slice(&(signed.len() as u32).to_le_bytes());
    message.extend_from_slice(signed);
    let mut hasher = Sha256::new();
    hasher.update(&message);
    hasher.update(archive);
    let digest = hasher.finalize();

    let developer = rsa_proofs
        .iter()
        .find(|p| Sha256::digest(&p.public_key)[..16] == *crx_id)
        .ok_or_else(|| bad("no developer key matches the extension ID"))?;
    let key = RsaPublicKey::from_public_key_der(&developer.public_key).map_err(|_| bad("invalid RSA public key"))?;
    key.verify(Pkcs1v15Sign::new::<Sha256>(), &digest, &developer.signature)
        .map_err(|_| bad("signature does not verify"))?;

    if !archive.starts_with(b"PK\x03\x04") {
        return Err(bad("payload is not a ZIP archive"));
    }
    Ok(VerifiedCrx { id, archive, public_key: developer.public_key.clone() })
}

#[cfg(test)]
pub(crate) mod testutil {
    use rsa::pkcs1::DecodeRsaPrivateKey;
    use rsa::pkcs8::EncodePublicKey;
    use rsa::{Pkcs1v15Sign, RsaPrivateKey};
    use sha2::{Digest, Sha256};

    pub fn key(name: &str) -> RsaPrivateKey {
        let path = format!("{}/tests/fixtures/crx/{name}.pkcs1.der", env!("CARGO_MANIFEST_DIR"));
        RsaPrivateKey::from_pkcs1_der(&std::fs::read(path).unwrap()).unwrap()
    }

    pub fn spki(key: &RsaPrivateKey) -> Vec<u8> {
        key.to_public_key().to_public_key_der().unwrap().as_bytes().to_vec()
    }

    fn pb_bytes(field: u64, v: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut put = |mut x: u64| loop {
            let b = (x & 0x7f) as u8;
            x >>= 7;
            if x == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        };
        put((field << 3) | 2);
        put(v.len() as u64);
        out.extend_from_slice(v);
        out
    }

    /// Builds a CRX3 for `archive`, signed by `signer`, claiming the ID of `id_key`.
    pub fn build(archive: &[u8], signer: &RsaPrivateKey, id_key: &RsaPrivateKey) -> Vec<u8> {
        let crx_id = &Sha256::digest(spki(id_key))[..16];
        let signed = pb_bytes(1, crx_id);
        let mut h = Sha256::new();
        h.update(b"CRX3 SignedData\x00");
        h.update((signed.len() as u32).to_le_bytes());
        h.update(&signed);
        h.update(archive);
        let sig = signer.sign(Pkcs1v15Sign::new::<Sha256>(), &h.finalize()).unwrap();
        let proof = [pb_bytes(1, &spki(signer)), pb_bytes(2, &sig)].concat();
        let header = [pb_bytes(2, &proof), pb_bytes(10000, &signed)].concat();
        let mut out = b"Cr24".to_vec();
        out.extend(3u32.to_le_bytes());
        out.extend((header.len() as u32).to_le_bytes());
        out.extend(header);
        out.extend_from_slice(archive);
        out
    }

    pub fn zip_with(files: &[(&str, &str)]) -> Vec<u8> {
        use std::io::Write;
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut buf);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            for (name, body) in files {
                z.start_file(*name, opts).unwrap();
                z.write_all(body.as_bytes()).unwrap();
            }
            z.finish().unwrap();
        }
        buf.into_inner()
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    fn archive() -> Vec<u8> {
        zip_with(&[("manifest.json", r#"{"name":"Test","version":"1.0","manifest_version":3}"#)])
    }

    #[test]
    fn id_alphabet() {
        assert_eq!(id_from_hash(&[0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0, 0, 0, 0, 0, 0, 0, 0xff]), "abcdefghijklmnopaaaaaaaaaaaaaapp");
    }

    #[test]
    fn valid_package() {
        let k = key("test-key");
        let a = archive();
        let crx = build(&a, &k, &k);
        let expected = id_from_public_key(&spki(&k));
        let v = verify(&crx, Some(&expected)).unwrap();
        assert_eq!(v.id, expected);
        assert_eq!(v.archive, &a[..]);
        assert!(super::super::is_valid_id(&v.id));
        assert!(verify(&crx, None).is_ok());
    }

    #[test]
    fn wrong_expected_id() {
        let k = key("test-key");
        let crx = build(&archive(), &k, &k);
        let other = id_from_public_key(&spki(&key("other-key")));
        assert!(verify(&crx, Some(&other)).is_err());
    }

    #[test]
    fn tampered_archive() {
        let k = key("test-key");
        let mut crx = build(&archive(), &k, &k);
        let last = crx.len() - 30;
        crx[last] ^= 0xff;
        assert!(verify(&crx, None).is_err());
    }

    #[test]
    fn signed_by_a_key_that_doesnt_own_the_id() {
        // Signed by other-key but claiming test-key's ID: no proof matches the ID.
        let crx = build(&archive(), &key("other-key"), &key("test-key"));
        assert!(verify(&crx, None).is_err());
    }

    #[test]
    fn garbage_and_crx2() {
        assert!(verify(b"PK\x03\x04not a crx", None).is_err());
        let mut v2 = b"Cr24".to_vec();
        v2.extend(2u32.to_le_bytes());
        v2.extend([0u8; 16]);
        assert!(matches!(verify(&v2, None), Err(Error::Unsupported(_))));
        let mut short = b"Cr24".to_vec();
        short.extend(3u32.to_le_bytes());
        short.extend(1000u32.to_le_bytes());
        assert!(verify(&short, None).is_err());
    }
}
