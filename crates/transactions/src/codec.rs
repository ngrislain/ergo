use crate::{Error, Result};
use ciborium::value::Value;
use serde::{Serialize, de::DeserializeOwned};

/// RFC 8949 core deterministic CBOR, with finite floats and normalized negative zero.
/// Numeric byte strings (including typed arrays) remain byte-for-byte unchanged.
pub fn canonical_cbor<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut value = Value::serialized(value).map_err(|e| Error::Encoding(e.to_string()))?;
    normalize(&mut value)?;
    encode(&value)
}

fn encode(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    ciborium::into_writer(value, &mut bytes).map_err(|e| Error::Encoding(e.to_string()))?;
    Ok(bytes)
}

fn normalize(value: &mut Value) -> Result<()> {
    match value {
        Value::Float(f) => {
            if !f.is_finite() {
                return Err(Error::Encoding("non-finite CBOR float".into()));
            }
            if *f == 0.0 {
                *f = 0.0;
            }
        }
        Value::Array(values) => {
            for value in values {
                normalize(value)?;
            }
        }
        Value::Map(entries) => {
            let mut ordered = Vec::with_capacity(entries.len());
            for (mut key, mut value) in std::mem::take(entries) {
                normalize(&mut key)?;
                normalize(&mut value)?;
                ordered.push((encode(&key)?, key, value));
            }
            // Core deterministic order is bytewise lexicographic, not RFC 7049 length-first.
            ordered.sort_by(|a, b| a.0.cmp(&b.0));
            if ordered.windows(2).any(|pair| pair[0].0 == pair[1].0) {
                return Err(Error::Encoding("duplicate canonical CBOR map key".into()));
            }
            *entries = ordered.into_iter().map(|(_, k, v)| (k, v)).collect();
        }
        Value::Tag(_, value) => normalize(value)?,
        _ => {}
    }
    Ok(())
}

pub(crate) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let mut reader = std::io::Cursor::new(bytes);
    let value = ciborium::from_reader(&mut reader).map_err(|e| Error::Encoding(e.to_string()))?;
    if reader.position() != bytes.len() as u64 {
        return Err(Error::Encoding("trailing bytes after CBOR value".into()));
    }
    Ok(value)
}
