//! NSS key database (`key4.db`) decryption for Firefox 58+ profiles.
//!
//! 1. `metaData(id='password')`: `item1` is the global salt, `item2` an encrypted
//!    blob that must decrypt to `"password-check"` with the primary password.
//! 2. `nssPrivate` rows whose `a102` is the well-known CKA_ID hold the logins
//!    key(s) in `a11`. Profiles migrated across versions may hold both a 3DES
//!    key (24 bytes) and an AES-256 key (32 bytes) under the same id.
//! 3. Each blob is either PBES2 (PBKDF2-HMAC-SHA256 + AES-256-CBC, Firefox 75+)
//!    or NSS's legacy pbeWithSha1AndTripleDES-CBC.
//!
//! key3.db (Firefox < 58) is not supported; users fall back to CSV export.

use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use hmac::{Hmac, Mac};
use rusqlite::{Connection, OptionalExtension};
use sha1::{Digest, Sha1};
use sha2::Sha256;
use zeroize::Zeroizing;

use super::der::{self, TAG_INTEGER, TAG_OCTET_STRING, TAG_OID, TAG_SEQUENCE};
use crate::error::{Error, Result};

pub const OID_PBES2: &str = "1.2.840.113549.1.5.13";
pub const OID_PBKDF2: &str = "1.2.840.113549.1.5.12";
pub const OID_HMAC_SHA256: &str = "1.2.840.113549.2.9";
pub const OID_HMAC_SHA1: &str = "1.2.840.113549.2.7";
pub const OID_AES256_CBC: &str = "2.16.840.1.101.3.4.1.42";
pub const OID_DES_EDE3_CBC: &str = "1.2.840.113549.3.7";
pub const OID_PBE_SHA1_3DES: &str = "1.2.840.113549.1.12.5.1.3";

/// CKA_ID NSS uses for the logins key.
pub const LOGINS_KEY_ID: [u8; 16] = [0xF8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];

/// Which cipher protected a blob (for logging which path was taken).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PbeKind {
    Pbes2Aes256,
    LegacyTripleDes,
}

/// Decrypted logins keys. Zeroized on drop.
pub struct NssKeys {
    keys: Vec<Zeroizing<Vec<u8>>>,
    pub check_kind: PbeKind,
}

impl std::fmt::Debug for NssKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NssKeys").field("count", &self.keys.len()).field("check_kind", &self.check_kind).finish()
    }
}

impl NssKeys {
    /// A key of exactly `len` bytes, falling back to the first one long enough.
    pub fn key(&self, len: usize) -> Option<&[u8]> {
        self.keys
            .iter()
            .find(|k| k.len() == len)
            .or_else(|| self.keys.iter().find(|k| k.len() >= len))
            .map(|k| &k[..len])
    }

    pub fn lengths(&self) -> Vec<usize> {
        self.keys.iter().map(|k| k.len()).collect()
    }
}

fn crypto(msg: impl Into<String>) -> Error {
    Error::Crypto(msg.into())
}

/// Decrypts an NSS PBE blob: `SEQUENCE { AlgorithmIdentifier, OCTET STRING ciphertext }`.
pub fn decrypt_pbe(blob: &[u8], global_salt: &[u8], password: &[u8]) -> Result<(Zeroizing<Vec<u8>>, PbeKind)> {
    let (outer, _) = der::expect(blob, TAG_SEQUENCE)?;
    let (alg, rest) = der::expect(outer, TAG_SEQUENCE)?;
    let (ciphertext, _) = der::expect(rest, TAG_OCTET_STRING)?;
    let (oid, params) = der::expect(alg, TAG_OID)?;
    match der::oid_to_string(oid)?.as_str() {
        OID_PBES2 => Ok((decrypt_pbes2(params, ciphertext, global_salt, password)?, PbeKind::Pbes2Aes256)),
        OID_PBE_SHA1_3DES => Ok((decrypt_legacy(params, ciphertext, global_salt, password)?, PbeKind::LegacyTripleDes)),
        other => Err(Error::Unsupported(format!("NSS PBE algorithm {other}"))),
    }
}

fn decrypt_pbes2(params: &[u8], ciphertext: &[u8], global_salt: &[u8], password: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    // params: SEQUENCE { SEQUENCE { OID pbkdf2, SEQUENCE {salt, iter, keylen, prf} }, SEQUENCE { OID cipher, OCTET iv } }
    let (p, _) = der::expect(params, TAG_SEQUENCE)?;
    let (kdf, rest) = der::expect(p, TAG_SEQUENCE)?;
    let (enc, _) = der::expect(rest, TAG_SEQUENCE)?;

    let (kdf_oid, kdf_params) = der::expect(kdf, TAG_OID)?;
    if der::oid_to_string(kdf_oid)? != OID_PBKDF2 {
        return Err(Error::Unsupported("PBES2 without PBKDF2".into()));
    }
    let (kp, _) = der::expect(kdf_params, TAG_SEQUENCE)?;
    let fields = der::children(kp)?;
    let salt = fields.first().filter(|t| t.tag == TAG_OCTET_STRING).ok_or_else(|| crypto("PBKDF2 salt"))?.value;
    let iterations =
        der::integer(fields.get(1).filter(|t| t.tag == TAG_INTEGER).ok_or_else(|| crypto("PBKDF2 iterations"))?.value)?;
    let mut key_len = 32usize;
    let mut prf = OID_HMAC_SHA1.to_string();
    for f in &fields[2..] {
        match f.tag {
            TAG_INTEGER => key_len = der::integer(f.value)? as usize,
            TAG_SEQUENCE => {
                let (o, _) = der::expect(f.value, TAG_OID)?;
                prf = der::oid_to_string(o)?;
            }
            _ => {}
        }
    }
    if iterations == 0 || iterations > 10_000_000 || key_len != 32 {
        return Err(crypto("unexpected PBKDF2 parameters"));
    }

    let (cipher_oid, cipher_params) = der::expect(enc, TAG_OID)?;
    if der::oid_to_string(cipher_oid)? != OID_AES256_CBC {
        return Err(Error::Unsupported("PBES2 cipher other than AES-256-CBC".into()));
    }
    let (iv_raw, _) = der::expect(cipher_params, TAG_OCTET_STRING)?;
    // NSS stores a 14-byte IV; the real IV is it prefixed with the DER header 04 0E.
    let iv: Vec<u8> = match iv_raw.len() {
        14 => [&[0x04, 0x0E][..], iv_raw].concat(),
        16 => iv_raw.to_vec(),
        n => return Err(crypto(format!("unexpected IV length {n}"))),
    };

    let hashed = Zeroizing::new(Sha1::new().chain_update(global_salt).chain_update(password).finalize().to_vec());
    let mut key = Zeroizing::new([0u8; 32]);
    match prf.as_str() {
        OID_HMAC_SHA256 => pbkdf2::pbkdf2_hmac::<Sha256>(&hashed, salt, iterations as u32, &mut *key),
        OID_HMAC_SHA1 => pbkdf2::pbkdf2_hmac::<Sha1>(&hashed, salt, iterations as u32, &mut *key),
        other => return Err(Error::Unsupported(format!("PBKDF2 PRF {other}"))),
    }
    aes256_cbc_decrypt(&key[..], &iv, ciphertext)
}

fn decrypt_legacy(params: &[u8], ciphertext: &[u8], global_salt: &[u8], password: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let (p, _) = der::expect(params, TAG_SEQUENCE)?;
    let (entry_salt, _) = der::expect(p, TAG_OCTET_STRING)?;
    type HmacSha1 = Hmac<Sha1>;
    let hp = Sha1::new().chain_update(global_salt).chain_update(password).finalize();
    let mut pes = entry_salt.to_vec();
    pes.resize(20.max(entry_salt.len()), 0);
    let chp = Sha1::new().chain_update(hp).chain_update(entry_salt).finalize();
    let mac = |data: &[&[u8]]| -> Result<Vec<u8>> {
        let mut m = HmacSha1::new_from_slice(&chp).map_err(|_| crypto("hmac key"))?;
        for d in data {
            m.update(d);
        }
        Ok(m.finalize().into_bytes().to_vec())
    };
    let k1 = mac(&[&pes, entry_salt])?;
    let tk = mac(&[&pes])?;
    let k2 = mac(&[&tk, entry_salt])?;
    let k = Zeroizing::new([k1, k2].concat());
    let key = &k[..24];
    let iv = &k[k.len() - 8..];
    tdes_cbc_decrypt(key, iv, ciphertext)
}

pub fn aes256_cbc_decrypt(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let dec = cbc::Decryptor::<aes::Aes256>::new_from_slices(key, iv).map_err(|_| crypto("AES key/IV size"))?;
    dec.decrypt_padded_vec_mut::<Pkcs7>(ciphertext).map(Zeroizing::new).map_err(|_| crypto("AES padding"))
}

pub fn tdes_cbc_decrypt(key: &[u8], iv: &[u8], ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let dec = cbc::Decryptor::<des::TdesEde3>::new_from_slices(key, iv).map_err(|_| crypto("3DES key/IV size"))?;
    dec.decrypt_padded_vec_mut::<Pkcs7>(ciphertext).map(Zeroizing::new).map_err(|_| crypto("3DES padding"))
}

/// Unlocks `key4.db` with the primary password (use `""` when none is set).
pub fn unlock(conn: &Connection, primary_password: &str) -> Result<NssKeys> {
    let (global_salt, check): (Vec<u8>, Vec<u8>) = conn
        .query_row("SELECT item1, item2 FROM metaData WHERE id = 'password'", [], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
        .ok_or_else(|| Error::Format("key4.db has no password entry".into()))?;
    let password = primary_password.as_bytes();
    let (plain, check_kind) = match decrypt_pbe(&check, &global_salt, password) {
        Ok(v) => v,
        // A padding failure is what a wrong password looks like.
        Err(Error::Crypto(_)) => return Err(Error::WrongPrimaryPassword),
        Err(e) => return Err(e),
    };
    if &plain[..] != b"password-check" {
        return Err(Error::WrongPrimaryPassword);
    }

    let mut stmt = conn.prepare("SELECT a11, a102 FROM nssPrivate")?;
    type Row = (Option<Vec<u8>>, Option<Vec<u8>>);
    let rows: Vec<Row> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<_, _>>()?;
    let mut keys = Vec::new();
    let any_tagged = rows.iter().any(|(_, id)| id.as_deref() == Some(&LOGINS_KEY_ID[..]));
    for (a11, a102) in rows {
        let Some(a11) = a11 else { continue };
        if any_tagged && a102.as_deref() != Some(&LOGINS_KEY_ID[..]) {
            continue;
        }
        match decrypt_pbe(&a11, &global_salt, password) {
            Ok((k, _)) if k.len() >= 24 => keys.push(k),
            Ok(_) => log::warn!("key4.db: skipping a short key"),
            Err(e) => log::warn!("key4.db: a key failed to decrypt: {e}"),
        }
    }
    if keys.is_empty() {
        return Err(Error::Format("key4.db holds no usable logins key".into()));
    }
    Ok(NssKeys { keys, check_kind })
}

/// Decrypts one `encryptedUsername`/`encryptedPassword` value from `logins.json`.
pub fn decrypt_login_value(keys: &NssKeys, b64: &str) -> Result<Zeroizing<String>> {
    use base64::Engine;
    let blob = Zeroizing::new(
        base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|_| crypto("login value isn't base64"))?,
    );
    // SEQUENCE { OCTET STRING keyId, SEQUENCE { OID cipher, OCTET STRING iv }, OCTET STRING ciphertext }
    let (seq, _) = der::expect(&blob, TAG_SEQUENCE)?;
    let (_key_id, rest) = der::expect(seq, TAG_OCTET_STRING)?;
    let (alg, rest) = der::expect(rest, TAG_SEQUENCE)?;
    let (ciphertext, _) = der::expect(rest, TAG_OCTET_STRING)?;
    let (oid, params) = der::expect(alg, TAG_OID)?;
    let (iv, _) = der::expect(params, TAG_OCTET_STRING)?;
    let plain = match der::oid_to_string(oid)?.as_str() {
        OID_DES_EDE3_CBC => tdes_cbc_decrypt(keys.key(24).ok_or_else(|| crypto("no 3DES key"))?, iv, ciphertext)?,
        OID_AES256_CBC => aes256_cbc_decrypt(keys.key(32).ok_or_else(|| crypto("no AES key"))?, iv, ciphertext)?,
        other => return Err(Error::Unsupported(format!("login cipher {other}"))),
    };
    let s = std::str::from_utf8(&plain).map_err(|_| crypto("login value isn't UTF-8"))?;
    Ok(Zeroizing::new(s.to_string()))
}

#[cfg(test)]
pub(crate) mod testutil {
    //! Encryption counterparts used to build synthetic key4.db fixtures in tests.
    use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
    use sha1::{Digest, Sha1};
    use sha2::Sha256;

    fn len_bytes(n: usize) -> Vec<u8> {
        if n < 0x80 {
            vec![n as u8]
        } else if n < 0x100 {
            vec![0x81, n as u8]
        } else {
            vec![0x82, (n >> 8) as u8, n as u8]
        }
    }

    pub fn tlv(tag: u8, value: &[u8]) -> Vec<u8> {
        let mut v = vec![tag];
        v.extend(len_bytes(value.len()));
        v.extend_from_slice(value);
        v
    }

    pub fn oid(dotted: &str) -> Vec<u8> {
        let parts: Vec<u64> = dotted.split('.').map(|p| p.parse().unwrap()).collect();
        let mut body = vec![(parts[0] * 40 + parts[1]) as u8];
        for &p in &parts[2..] {
            let mut stack = vec![(p & 0x7f) as u8];
            let mut v = p >> 7;
            while v > 0 {
                stack.push(((v & 0x7f) as u8) | 0x80);
                v >>= 7;
            }
            stack.reverse();
            body.extend(stack);
        }
        tlv(0x06, &body)
    }

    pub fn aes_encrypt(key: &[u8], iv: &[u8], plain: &[u8]) -> Vec<u8> {
        cbc::Encryptor::<aes::Aes256>::new_from_slices(key, iv).unwrap().encrypt_padded_vec_mut::<Pkcs7>(plain)
    }

    pub fn tdes_encrypt(key: &[u8], iv: &[u8], plain: &[u8]) -> Vec<u8> {
        cbc::Encryptor::<des::TdesEde3>::new_from_slices(key, iv).unwrap().encrypt_padded_vec_mut::<Pkcs7>(plain)
    }

    /// PBES2 blob as NSS writes it (14-byte IV).
    pub fn pbes2_blob(
        global_salt: &[u8],
        password: &[u8],
        entry_salt: &[u8],
        iv14: &[u8; 14],
        plain: &[u8],
    ) -> Vec<u8> {
        let iterations: u32 = 10_000;
        let hashed = Sha1::new().chain_update(global_salt).chain_update(password).finalize();
        let mut key = [0u8; 32];
        pbkdf2::pbkdf2_hmac::<Sha256>(&hashed, entry_salt, iterations, &mut key);
        let iv: Vec<u8> = [&[0x04, 0x0E][..], iv14].concat();
        let ct = aes_encrypt(&key, &iv, plain);
        let prf = tlv(0x30, &[oid(super::OID_HMAC_SHA256), vec![0x05, 0x00]].concat());
        let kdf_params =
            tlv(0x30, &[tlv(0x04, entry_salt), tlv(0x02, &[0x27, 0x10]), tlv(0x02, &[0x20]), prf].concat());
        let kdf = tlv(0x30, &[oid(super::OID_PBKDF2), kdf_params].concat());
        let enc = tlv(0x30, &[oid(super::OID_AES256_CBC), tlv(0x04, iv14)].concat());
        let alg = tlv(0x30, &[oid(super::OID_PBES2), tlv(0x30, &[kdf, enc].concat())].concat());
        tlv(0x30, &[alg, tlv(0x04, &ct)].concat())
    }

    /// A logins.json value (base64) encrypted with AES-256-CBC.
    pub fn login_value_aes(key: &[u8], iv: &[u8; 16], plain: &str) -> String {
        use base64::Engine;
        let ct = aes_encrypt(key, iv, plain.as_bytes());
        let alg = tlv(0x30, &[oid(super::OID_AES256_CBC), tlv(0x04, iv)].concat());
        let der = tlv(0x30, &[tlv(0x04, &super::LOGINS_KEY_ID), alg, tlv(0x04, &ct)].concat());
        base64::engine::general_purpose::STANDARD.encode(der)
    }

    /// A logins.json value (base64) encrypted with 3DES-CBC.
    pub fn login_value_3des(key: &[u8], iv: &[u8; 8], plain: &str) -> String {
        use base64::Engine;
        let ct = tdes_encrypt(key, iv, plain.as_bytes());
        let alg = tlv(0x30, &[oid(super::OID_DES_EDE3_CBC), tlv(0x04, iv)].concat());
        let der = tlv(0x30, &[tlv(0x04, &super::LOGINS_KEY_ID), alg, tlv(0x04, &ct)].concat());
        base64::engine::general_purpose::STANDARD.encode(der)
    }

    /// Creates a synthetic key4.db holding `logins_key`.
    pub fn make_key4(path: &std::path::Path, primary_password: &str, logins_key: &[u8]) {
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE metaData (id PRIMARY KEY UNIQUE ON CONFLICT REPLACE, item1, item2);
             CREATE TABLE nssPrivate (id PRIMARY KEY UNIQUE ON CONFLICT ABORT, a0, a1, a2, a3, a10, a11, a12, a102);",
        )
        .unwrap();
        let global_salt = b"0123456789abcdefghij";
        let pw = primary_password.as_bytes();
        let check = pbes2_blob(global_salt, pw, &[7u8; 32], &[1u8; 14], b"password-check");
        conn.execute(
            "INSERT INTO metaData(id, item1, item2) VALUES ('password', ?1, ?2)",
            rusqlite::params![&global_salt[..], check],
        )
        .unwrap();
        let wrapped = pbes2_blob(global_salt, pw, &[9u8; 32], &[2u8; 14], logins_key);
        conn.execute(
            "INSERT INTO nssPrivate(id, a11, a102) VALUES (1, ?1, ?2)",
            rusqlite::params![wrapped, &super::LOGINS_KEY_ID[..]],
        )
        .unwrap();
        // An unrelated private key that must be ignored.
        conn.execute("INSERT INTO nssPrivate(id, a11, a102) VALUES (2, x'00', x'01')", []).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::*;
    use super::*;

    #[test]
    fn pbes2_roundtrip() {
        let blob = pbes2_blob(b"salt", b"pw", &[3u8; 32], &[5u8; 14], b"password-check");
        let (plain, kind) = decrypt_pbe(&blob, b"salt", b"pw").unwrap();
        assert_eq!(&plain[..], b"password-check");
        assert_eq!(kind, PbeKind::Pbes2Aes256);
        assert!(decrypt_pbe(&blob, b"salt", b"wrong").is_err());
    }

    #[test]
    fn unlock_synthetic_profile() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key4.db");
        let key = [0x42u8; 32];
        make_key4(&path, "test-primary", &key);
        let conn = Connection::open(&path).unwrap();
        assert!(matches!(unlock(&conn, ""), Err(Error::WrongPrimaryPassword)));
        let keys = unlock(&conn, "test-primary").unwrap();
        assert_eq!(keys.lengths(), vec![32]);
        let v = login_value_aes(&key, &[9u8; 16], "hunter2 ✓");
        assert_eq!(&*decrypt_login_value(&keys, &v).unwrap(), "hunter2 ✓");
        // A 3DES value uses the first 24 bytes.
        let v = login_value_3des(&key[..24], &[1u8; 8], "legacy");
        assert_eq!(&*decrypt_login_value(&keys, &v).unwrap(), "legacy");
        assert!(decrypt_login_value(&keys, "not base64!").is_err());
    }

    #[test]
    fn empty_primary_password() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key4.db");
        make_key4(&path, "", &[0x11u8; 24]);
        let conn = Connection::open(&path).unwrap();
        let keys = unlock(&conn, "").unwrap();
        assert_eq!(keys.lengths(), vec![24]);
        assert!(keys.key(32).is_none());
    }
}
