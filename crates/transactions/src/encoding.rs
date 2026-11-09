//! Deterministic CBOR without an intermediate, decoded copy of the document.
//! Arrays write directly to the output; maps retain encoded entries only, since
//! their keys must be sorted before writing. Scalar encoding stays with Ciborium.
use ciborium::value::Error;
use serde::{Serialize, ser};

type Result<T = ()> = std::result::Result<T, Error>;

#[derive(Default)]
struct Encoder {
    bytes: Vec<u8>,
}

pub(super) fn encode<T: ?Sized + Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut encoder = Encoder::default();
    value.serialize(&mut encoder)?;
    Ok(encoder.bytes)
}

fn error(message: impl std::fmt::Display) -> Error {
    ser::Error::custom(message)
}

pub(crate) fn header_bytes(major: u8, value: u64) -> ([u8; 9], usize) {
    let mut bytes = [0; 9];
    let prefix = major << 5;
    let size = match value {
        0..=23 => {
            bytes[0] = prefix | value as u8;
            1
        }
        24..=0xff => {
            bytes[0] = prefix | 24;
            bytes[1] = value as u8;
            2
        }
        0x100..=0xffff => {
            bytes[0] = prefix | 25;
            bytes[1..3].copy_from_slice(&(value as u16).to_be_bytes());
            3
        }
        0x10000..=0xffffffff => {
            bytes[0] = prefix | 26;
            bytes[1..5].copy_from_slice(&(value as u32).to_be_bytes());
            5
        }
        _ => {
            bytes[0] = prefix | 27;
            bytes[1..9].copy_from_slice(&value.to_be_bytes());
            9
        }
    };
    (bytes, size)
}

impl Encoder {
    fn scalar<T: ?Sized + Serialize>(&mut self, value: &T) -> Result {
        ciborium::into_writer(value, &mut self.bytes).map_err(error)
    }

    fn header(&mut self, major: u8, value: u64) {
        let (bytes, size) = header_bytes(major, value);
        self.bytes.extend_from_slice(&bytes[..size]);
    }

    fn sequence(&mut self, length: Option<usize>) -> Sequence<'_> {
        let start = self.bytes.len();
        if let Some(length) = length {
            self.header(4, length as u64);
        } else {
            // Unknown-length iterators still get a definite, shortest header.
            // Patch it once at the end instead of collecting their elements.
            self.bytes.extend_from_slice(&[0; 9]);
        }
        Sequence {
            encoder: self,
            start,
            length,
            count: 0,
        }
    }

    fn map(&mut self) -> Map<'_> {
        Map {
            encoder: self,
            entries: Vec::new(),
            key: None,
        }
    }
}

macro_rules! scalars {
    ($($method:ident($ty:ty)),* $(,)?) => {$(
        fn $method(self, value: $ty) -> Result {
            self.scalar(&value)
        }
    )*};
}

impl<'a> ser::Serializer for &'a mut Encoder {
    type Ok = ();
    type Error = Error;
    type SerializeSeq = Sequence<'a>;
    type SerializeTuple = Sequence<'a>;
    type SerializeTupleStruct = Sequence<'a>;
    type SerializeTupleVariant = Variant<'a>;
    type SerializeMap = Map<'a>;
    type SerializeStruct = Map<'a>;
    type SerializeStructVariant = Map<'a>;

    scalars! {
        serialize_bool(bool),
        serialize_i8(i8), serialize_i16(i16), serialize_i32(i32), serialize_i64(i64), serialize_i128(i128),
        serialize_u8(u8), serialize_u16(u16), serialize_u32(u32), serialize_u64(u64), serialize_u128(u128),
        serialize_char(char), serialize_str(&str),
    }

    fn serialize_f32(self, value: f32) -> Result {
        self.serialize_f64(value.into())
    }

    fn serialize_f64(self, value: f64) -> Result {
        if !value.is_finite() {
            return Err(error("non-finite CBOR float"));
        }
        self.scalar(&if value == 0.0 { 0.0 } else { value })
    }

    fn serialize_bytes(self, value: &[u8]) -> Result {
        self.header(2, value.len() as u64);
        self.bytes.extend_from_slice(value);
        Ok(())
    }

    fn serialize_none(self) -> Result {
        self.serialize_unit()
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result {
        self.bytes.push(0xf6);
        Ok(())
    }

    fn serialize_unit_struct(self, _: &'static str) -> Result {
        self.serialize_unit()
    }

    fn serialize_unit_variant(self, _: &'static str, _: u32, variant: &'static str) -> Result {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _: &'static str, value: &T) -> Result {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        name: &'static str,
        _: u32,
        variant: &'static str,
        value: &T,
    ) -> Result {
        if name != "@@TAG@@" || variant != "@@UNTAGGED@@" {
            self.header(5, 1);
            self.scalar(variant)?;
        }
        value.serialize(self)
    }

    fn serialize_seq(self, length: Option<usize>) -> Result<Self::SerializeSeq> {
        Ok(self.sequence(length))
    }

    fn serialize_tuple(self, length: usize) -> Result<Self::SerializeTuple> {
        self.serialize_seq(Some(length))
    }

    fn serialize_tuple_struct(
        self,
        _: &'static str,
        length: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        self.serialize_seq(Some(length))
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        _: u32,
        variant: &'static str,
        length: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        if name == "@@TAG@@" && variant == "@@TAGGED@@" {
            if length != 2 {
                return Err(error("invalid tag input"));
            }
            Ok(Variant::Tag {
                encoder: self,
                fields: 0,
            })
        } else {
            self.header(5, 1);
            self.scalar(variant)?;
            Ok(Variant::Tuple(self.sequence(Some(length))))
        }
    }

    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap> {
        Ok(self.map())
    }

    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeStruct> {
        Ok(self.map())
    }

    fn serialize_struct_variant(
        self,
        _: &'static str,
        _: u32,
        variant: &'static str,
        _: usize,
    ) -> Result<Self::SerializeStructVariant> {
        self.header(5, 1);
        self.scalar(variant)?;
        Ok(self.map())
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

struct Sequence<'a> {
    encoder: &'a mut Encoder,
    start: usize,
    length: Option<usize>,
    count: usize,
}

impl Sequence<'_> {
    fn element<T: ?Sized + Serialize>(&mut self, value: &T) -> Result {
        value.serialize(&mut *self.encoder)?;
        self.count += 1;
        Ok(())
    }

    fn finish(self) -> Result {
        if let Some(length) = self.length {
            if length != self.count {
                return Err(error("CBOR sequence length mismatch"));
            }
        } else {
            let mut header = Encoder::default();
            header.header(4, self.count as u64);
            let size = header.bytes.len();
            let bytes = &mut self.encoder.bytes;
            bytes[self.start..self.start + size].copy_from_slice(&header.bytes);
            bytes.copy_within(self.start + 9.., self.start + size);
            bytes.truncate(bytes.len() - (9 - size));
        }
        Ok(())
    }
}

macro_rules! sequence_traits {
    ($($trait:ident::$method:ident),* $(,)?) => {$(
        impl ser::$trait for Sequence<'_> {
            type Ok = ();
            type Error = Error;
            fn $method<T: ?Sized + Serialize>(&mut self, value: &T) -> Result {
                self.element(value)
            }
            fn end(self) -> Result { self.finish() }
        }
    )*};
}
sequence_traits! {
    SerializeSeq::serialize_element,
    SerializeTuple::serialize_element,
    SerializeTupleStruct::serialize_field,
}

enum Variant<'a> {
    Tuple(Sequence<'a>),
    Tag {
        encoder: &'a mut Encoder,
        fields: u8,
    },
}

impl ser::SerializeTupleVariant for Variant<'_> {
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, value: &T) -> Result {
        match self {
            Self::Tuple(sequence) => sequence.element(value),
            Self::Tag { encoder, fields } => {
                match *fields {
                    0 => {
                        let bytes = encode(value)?;
                        let tag: u64 = ciborium::from_reader(bytes.as_slice()).map_err(error)?;
                        encoder.header(6, tag);
                    }
                    1 => value.serialize(&mut **encoder)?,
                    _ => return Err(error("invalid tag input")),
                }
                *fields += 1;
                Ok(())
            }
        }
    }

    fn end(self) -> Result {
        match self {
            Self::Tuple(sequence) => sequence.finish(),
            Self::Tag { fields: 2, .. } => Ok(()),
            _ => Err(error("invalid tag input")),
        }
    }
}

struct Map<'a> {
    encoder: &'a mut Encoder,
    entries: Vec<(Vec<u8>, Vec<u8>)>,
    key: Option<Vec<u8>>,
}

impl Map<'_> {
    fn entry<K: ?Sized + Serialize, V: ?Sized + Serialize>(
        &mut self,
        key: &K,
        value: &V,
    ) -> Result {
        self.entries.push((encode(key)?, encode(value)?));
        Ok(())
    }

    fn finish(mut self) -> Result {
        if self.key.is_some() {
            return Err(error("CBOR map key without value"));
        }
        // RFC 8949 core order, not RFC 7049's length-first key order.
        self.entries.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        if self.entries.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(error("duplicate canonical CBOR map key"));
        }
        self.encoder.header(5, self.entries.len() as u64);
        for (key, value) in self.entries {
            self.encoder.bytes.extend_from_slice(&key);
            self.encoder.bytes.extend_from_slice(&value);
        }
        Ok(())
    }
}

impl ser::SerializeMap for Map<'_> {
    type Ok = ();
    type Error = Error;

    fn serialize_key<T: ?Sized + Serialize>(&mut self, key: &T) -> Result {
        if self.key.is_some() {
            return Err(error("CBOR map key without value"));
        }
        self.key = Some(encode(key)?);
        Ok(())
    }

    fn serialize_value<T: ?Sized + Serialize>(&mut self, value: &T) -> Result {
        let key = self
            .key
            .take()
            .ok_or_else(|| error("CBOR map value without key"))?;
        self.entries.push((key, encode(value)?));
        Ok(())
    }

    fn serialize_entry<K: ?Sized + Serialize, V: ?Sized + Serialize>(
        &mut self,
        key: &K,
        value: &V,
    ) -> Result {
        self.entry(key, value)
    }

    fn end(self) -> Result {
        self.finish()
    }
}

macro_rules! struct_traits {
    ($($trait:ident),* $(,)?) => {$(
        impl ser::$trait for Map<'_> {
            type Ok = ();
            type Error = Error;
            fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result {
                self.entry(key, value)
            }
            fn end(self) -> Result { self.finish() }
        }
    )*};
}
struct_traits! { SerializeStruct, SerializeStructVariant }

#[cfg(test)]
mod tests {
    use super::*;
    use ciborium::value::Value;

    // A materialized specification oracle, not a production reader or encoder.
    fn reference<T: Serialize>(value: &T) -> Vec<u8> {
        fn normalize(value: &mut Value) {
            match value {
                Value::Float(f) if *f == 0.0 => *f = 0.0,
                Value::Array(values) => values.iter_mut().for_each(normalize),
                Value::Map(entries) => {
                    for (key, value) in entries.iter_mut() {
                        normalize(key);
                        normalize(value);
                    }
                    entries.sort_by_cached_key(|(key, _)| {
                        let mut bytes = Vec::new();
                        ciborium::into_writer(key, &mut bytes).unwrap();
                        bytes
                    });
                }
                Value::Tag(_, value) => normalize(value),
                _ => {}
            }
        }
        let mut value = Value::serialized(value).unwrap();
        normalize(&mut value);
        let mut bytes = Vec::new();
        ciborium::into_writer(&value, &mut bytes).unwrap();
        bytes
    }

    fn equivalent<T: Serialize>(value: &T) {
        assert_eq!(encode(value).unwrap(), reference(value));
    }

    #[test]
    fn scalar_and_compound_encodings_keep_the_same_content_ids() {
        #[derive(Serialize)]
        enum Example {
            Unit,
            Newtype(Vec<f64>),
            Tuple(u64, String),
            Struct { z: Option<i64>, a: Vec<[f32; 3]> },
        }
        for value in [
            Example::Unit,
            Example::Newtype(vec![-0.0, 1.5, f64::MIN_POSITIVE, f64::MAX]),
            Example::Tuple(u64::MAX, "é\nhello".into()),
            Example::Struct {
                z: Some(i64::MIN),
                a: vec![[0.0, 1.5, f32::MAX]; 30],
            },
        ] {
            equivalent(&value);
        }
        for value in [
            0,
            23,
            24,
            255,
            256,
            65535,
            65536,
            u64::MAX as u128,
            u128::MAX,
        ] {
            equivalent(&value);
        }
        for value in [i128::MIN, i64::MIN as i128, -25, -24, -1, 0, i128::MAX] {
            equivalent(&value);
        }
        equivalent(&None::<u8>);
        equivalent(&Some(true));
        equivalent(&'🧱');
        equivalent(&Value::Tag(
            55799,
            Box::new(Value::Map(vec![
                (Value::Text("".into()), Value::Bytes(vec![0, 128, 255])),
                (Value::Integer(1000.into()), Value::Float(-0.0)),
            ])),
        ));
        equivalent(&Value::Map(vec![
            (Value::Array(vec![Value::Float(-0.0)]), Value::Null),
            (
                Value::Tag(9, Box::new(Value::Bool(true))),
                Value::Bool(false),
            ),
        ]));
    }

    #[test]
    fn indefinite_iterators_get_definite_canonical_headers_without_collecting_values() {
        struct UnknownLength(usize);
        impl Serialize for UnknownLength {
            fn serialize<S: ser::Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                use ser::SerializeSeq;
                let mut seq = serializer.serialize_seq(None)?;
                for i in 0..self.0 {
                    seq.serialize_element(&(i, [0.0, -0.0, 1.5]))?;
                }
                seq.end()
            }
        }
        for length in [0, 1, 23, 24, 255, 256, 65535, 65536] {
            equivalent(&(UnknownLength(length), 42));
        }
        equivalent(&[UnknownLength(24), UnknownLength(256)]);
    }

    #[test]
    fn nonfinite_numbers_and_colliding_normalized_keys_are_rejected_recursively() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(encode(&vec![Some(bad)]).is_err());
            assert!(encode(&Value::Tag(1, Box::new(Value::Float(bad)))).is_err());
        }
        assert!(
            encode(&Value::Map(vec![
                (Value::Float(0.0), Value::Null),
                (Value::Float(-0.0), Value::Null),
            ]))
            .is_err()
        );
        assert!(
            encode(&Value::Map(vec![
                (Value::Text("a".into()), Value::Null),
                (Value::Text("a".into()), Value::Null),
            ]))
            .is_err()
        );
    }
}
