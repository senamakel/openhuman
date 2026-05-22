//! Hand-rolled protobuf wire-format primitives.
//!
//! Only varints, length-delimited bytes, and the two fixed-width forms
//! are supported — that's everything the yuanbao protocol uses. Kept
//! separate from `proto.rs` so the latter stays under 500 lines and
//! reads as a "schema" file.

use std::sync::atomic::{AtomicU32, Ordering};

use super::errors::YuanbaoError;

/// Global per-process sequence number for ConnMsg head.seq_no.
static SEQ: AtomicU32 = AtomicU32::new(1);

pub fn next_seq_no() -> u32 {
    SEQ.fetch_add(1, Ordering::Relaxed)
}

pub const WT_VARINT: u8 = 0;
pub const WT_LEN: u8 = 2;

// ─── Varint ─────────────────────────────────────────────────────────

pub fn encode_varint(mut value: u64, buf: &mut Vec<u8>) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
}

pub fn decode_varint(data: &[u8], pos: usize) -> Result<(u64, usize), YuanbaoError> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    let mut i = pos;
    loop {
        if i >= data.len() {
            return Err(YuanbaoError::ProtoDecode("truncated varint".into()));
        }
        let byte = data[i];
        value |= ((byte & 0x7F) as u64) << shift;
        i += 1;
        if byte & 0x80 == 0 {
            return Ok((value, i - pos));
        }
        shift += 7;
        if shift >= 64 {
            return Err(YuanbaoError::ProtoDecode("varint too long".into()));
        }
    }
}

// ─── Field encoders ────────────────────────────────────────────────

pub fn encode_field_varint(field: u32, value: u64, buf: &mut Vec<u8>) {
    encode_varint(((field as u64) << 3) | WT_VARINT as u64, buf);
    encode_varint(value, buf);
}

pub fn encode_field_bytes(field: u32, data: &[u8], buf: &mut Vec<u8>) {
    encode_varint(((field as u64) << 3) | WT_LEN as u64, buf);
    encode_varint(data.len() as u64, buf);
    buf.extend_from_slice(data);
}

pub fn encode_field_string(field: u32, s: &str, buf: &mut Vec<u8>) {
    encode_field_bytes(field, s.as_bytes(), buf);
}

// ─── Field parsing ──────────────────────────────────────────────────

#[derive(Debug)]
pub enum FieldValue {
    Varint(u64),
    Bytes(Vec<u8>),
    Fixed32(u32),
    Fixed64(u64),
}

pub fn parse_fields(data: &[u8]) -> Result<Vec<(u32, FieldValue)>, YuanbaoError> {
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let (tag, n) = decode_varint(data, pos)?;
        pos += n;
        let field = (tag >> 3) as u32;
        let wire = (tag & 0x07) as u8;
        match wire {
            WT_VARINT => {
                let (v, n) = decode_varint(data, pos)?;
                pos += n;
                out.push((field, FieldValue::Varint(v)));
            }
            WT_LEN => {
                let (len, n) = decode_varint(data, pos)?;
                pos += n;
                let end = pos + len as usize;
                if end > data.len() {
                    return Err(YuanbaoError::ProtoDecode(format!(
                        "truncated len field {field}: need {len} have {}",
                        data.len() - pos
                    )));
                }
                out.push((field, FieldValue::Bytes(data[pos..end].to_vec())));
                pos = end;
            }
            1 => {
                if pos + 8 > data.len() {
                    return Err(YuanbaoError::ProtoDecode("truncated fixed64".into()));
                }
                let v = u64::from_le_bytes(data[pos..pos + 8].try_into().unwrap());
                pos += 8;
                out.push((field, FieldValue::Fixed64(v)));
            }
            5 => {
                if pos + 4 > data.len() {
                    return Err(YuanbaoError::ProtoDecode("truncated fixed32".into()));
                }
                let v = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap());
                pos += 4;
                out.push((field, FieldValue::Fixed32(v)));
            }
            other => {
                return Err(YuanbaoError::ProtoDecode(format!(
                    "unsupported wire type {other} at field {field}"
                )));
            }
        }
    }
    Ok(out)
}

pub fn get_string(fields: &[(u32, FieldValue)], num: u32) -> String {
    for (n, v) in fields {
        if *n == num {
            if let FieldValue::Bytes(b) = v {
                return String::from_utf8_lossy(b).into_owned();
            }
        }
    }
    String::new()
}

pub fn get_varint(fields: &[(u32, FieldValue)], num: u32) -> u64 {
    for (n, v) in fields {
        if *n == num {
            if let FieldValue::Varint(x) = v {
                return *x;
            }
        }
    }
    0
}

pub fn get_bytes(fields: &[(u32, FieldValue)], num: u32) -> Vec<u8> {
    for (n, v) in fields {
        if *n == num {
            if let FieldValue::Bytes(b) = v {
                return b.clone();
            }
        }
    }
    Vec::new()
}

pub fn get_repeated_bytes(fields: &[(u32, FieldValue)], num: u32) -> Vec<Vec<u8>> {
    fields
        .iter()
        .filter_map(|(n, v)| match v {
            FieldValue::Bytes(b) if *n == num => Some(b.clone()),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip() {
        for &v in &[0u64, 1, 127, 128, 300, 16384, u32::MAX as u64, u64::MAX] {
            let mut buf = Vec::new();
            encode_varint(v, &mut buf);
            let (got, n) = decode_varint(&buf, 0).unwrap();
            assert_eq!(got, v, "varint roundtrip failed for {v}");
            assert_eq!(n, buf.len());
        }
    }

    #[test]
    fn varint_truncated_errors() {
        let buf = vec![0x80, 0x80]; // continuation bit set but no end
        assert!(decode_varint(&buf, 0).is_err());
    }

    #[test]
    fn field_roundtrip() {
        let mut buf = Vec::new();
        encode_field_varint(1, 42, &mut buf);
        encode_field_string(2, "hello", &mut buf);
        encode_field_bytes(3, b"\x00\x01\x02", &mut buf);

        let fields = parse_fields(&buf).unwrap();
        assert_eq!(get_varint(&fields, 1), 42);
        assert_eq!(get_string(&fields, 2), "hello");
        assert_eq!(get_bytes(&fields, 3), vec![0, 1, 2]);
    }

    #[test]
    fn unknown_field_skipped_gracefully() {
        let mut buf = Vec::new();
        encode_field_varint(99, 123, &mut buf);
        encode_field_string(1, "wanted", &mut buf);
        let fields = parse_fields(&buf).unwrap();
        assert_eq!(get_string(&fields, 1), "wanted");
        assert_eq!(get_string(&fields, 2), ""); // missing field returns default
    }

    #[test]
    fn seq_numbers_are_monotonic() {
        let a = next_seq_no();
        let b = next_seq_no();
        assert!(b > a);
    }
}
