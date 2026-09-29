//! Just enough DER to read NSS's `key4.db` and `logins.json` blobs.

use crate::error::{Error, Result};

pub const TAG_INTEGER: u8 = 0x02;
pub const TAG_OCTET_STRING: u8 = 0x04;
pub const TAG_NULL: u8 = 0x05;
pub const TAG_OID: u8 = 0x06;
pub const TAG_SEQUENCE: u8 = 0x30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tlv<'a> {
    pub tag: u8,
    pub value: &'a [u8],
}

fn err(msg: &str) -> Error {
    Error::Format(format!("DER: {msg}"))
}

/// Reads one TLV from the front of `data`; returns it and the remaining bytes.
pub fn read(data: &[u8]) -> Result<(Tlv<'_>, &[u8])> {
    let (&tag, rest) = data.split_first().ok_or_else(|| err("unexpected end"))?;
    let (&len0, mut rest) = rest.split_first().ok_or_else(|| err("missing length"))?;
    let len = if len0 & 0x80 == 0 {
        len0 as usize
    } else {
        let n = (len0 & 0x7f) as usize;
        if n == 0 || n > 4 || rest.len() < n {
            return Err(err("bad long-form length"));
        }
        let mut len = 0usize;
        for &b in &rest[..n] {
            len = (len << 8) | b as usize;
        }
        rest = &rest[n..];
        len
    };
    if rest.len() < len {
        return Err(err("length past end"));
    }
    Ok((Tlv { tag, value: &rest[..len] }, &rest[len..]))
}

/// Reads one TLV and checks its tag.
pub fn expect(data: &[u8], tag: u8) -> Result<(&[u8], &[u8])> {
    let (tlv, rest) = read(data)?;
    if tlv.tag != tag {
        return Err(err(&format!("expected tag {tag:#04x}, got {:#04x}", tlv.tag)));
    }
    Ok((tlv.value, rest))
}

/// All TLVs inside a constructed value.
pub fn children(mut data: &[u8]) -> Result<Vec<Tlv<'_>>> {
    let mut out = Vec::new();
    while !data.is_empty() {
        let (tlv, rest) = read(data)?;
        out.push(tlv);
        data = rest;
    }
    Ok(out)
}

/// Decodes an OID to dotted form.
pub fn oid_to_string(bytes: &[u8]) -> Result<String> {
    let (&first, rest) = bytes.split_first().ok_or_else(|| err("empty OID"))?;
    let mut parts = vec![(first / 40) as u64, (first % 40) as u64];
    let mut acc: u64 = 0;
    for &b in rest {
        acc = (acc << 7) | (b & 0x7f) as u64;
        if b & 0x80 == 0 {
            parts.push(acc);
            acc = 0;
        }
    }
    Ok(parts.iter().map(u64::to_string).collect::<Vec<_>>().join("."))
}

/// Unsigned INTEGER value (up to 64 bits).
pub fn integer(bytes: &[u8]) -> Result<u64> {
    let bytes = match bytes {
        [0, rest @ ..] if !rest.is_empty() => rest,
        b => b,
    };
    if bytes.len() > 8 {
        return Err(err("integer too large"));
    }
    Ok(bytes.iter().fold(0u64, |acc, &b| (acc << 8) | b as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oids() {
        // 1.2.840.113549.1.5.13 (PBES2)
        assert_eq!(oid_to_string(&[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x05, 0x0d]).unwrap(), "1.2.840.113549.1.5.13");
        // 2.16.840.1.101.3.4.1.42 (aes256-CBC)
        assert_eq!(oid_to_string(&[0x60, 0x86, 0x48, 0x01, 0x65, 0x03, 0x04, 0x01, 0x2a]).unwrap(), "2.16.840.1.101.3.4.1.42");
    }

    #[test]
    fn lengths_and_errors() {
        let (tlv, rest) = read(&[0x04, 0x02, 0xAA, 0xBB, 0x05, 0x00]).unwrap();
        assert_eq!(tlv, Tlv { tag: 4, value: &[0xAA, 0xBB] });
        assert_eq!(rest, &[0x05, 0x00]);
        let mut long = vec![0x04, 0x81, 0x80];
        long.extend(std::iter::repeat_n(7u8, 0x80));
        assert_eq!(read(&long).unwrap().0.value.len(), 128);
        assert!(read(&[0x04, 0x05, 0x01]).is_err());
        assert!(read(&[0x04]).is_err());
        assert!(expect(&[0x02, 0x01, 0x01], TAG_SEQUENCE).is_err());
        assert_eq!(integer(&[0x00, 0x80]).unwrap(), 128);
        assert_eq!(integer(&[0x27, 0x10]).unwrap(), 10000);
    }
}
