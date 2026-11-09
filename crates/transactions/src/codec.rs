use crate::{Error, Result};
use serde::{Serialize, de::DeserializeOwned};

#[path = "encoding.rs"]
mod encoding;
pub(crate) use encoding::header_bytes;

/// RFC 8949 core deterministic CBOR, with finite floats and normalized negative zero.
/// Numeric byte strings (including typed arrays) remain byte-for-byte unchanged.
pub fn canonical_cbor<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    encoding::encode(value).map_err(|e| Error::Encoding(e.to_string()))
}

pub(crate) fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    let mut reader = std::io::Cursor::new(bytes);
    let value = ciborium::from_reader(&mut reader).map_err(|e| Error::Encoding(e.to_string()))?;
    if reader.position() != bytes.len() as u64 {
        return Err(Error::Encoding("trailing bytes after CBOR value".into()));
    }
    Ok(value)
}
