use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::fmt::{Display, Formatter};
use time::OffsetDateTime;

pub const PROTOCOL_VERSION: &str = "babble";
pub const CANONICAL_ENCODING_VERSION: &str = "babble.canonical.v1";
const CANONICAL_PREAMBLE: &[u8] = b"babble.canonical.v1\0";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("canonical serialization failed: {0}")]
    Canonical(String),
    #[error("invalid identifier prefix: expected {expected}, got {actual}")]
    InvalidPrefix {
        expected: &'static str,
        actual: String,
    },
    #[error("invalid hash length: expected {expected}, got {actual}")]
    InvalidHashLength { expected: usize, actual: usize },
    #[error("signature verification failed")]
    Signature,
    #[error("object is unsigned")]
    UnsignedObject,
    #[error("edge is unsigned")]
    UnsignedEdge,
    #[error("event is unsigned")]
    UnsignedEvent,
    #[error("not found: {0}")]
    NotFound(String),
    #[error("state conflict: {0}")]
    Conflict(String),
    #[error("provider unavailable: {0}")]
    ProviderUnavailable(String),
    #[error("storage unavailable: {0}")]
    StorageUnavailable(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct Hash(String);

impl Hash {
    pub fn new_unchecked(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn from_bytes(bytes: &[u8]) -> Self {
        Self(hex::encode(blake3::hash(bytes).as_bytes()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn validate(&self) -> Result<()> {
        if self.0.len() != 64 {
            return Err(Error::InvalidHashLength {
                expected: 64,
                actual: self.0.len(),
            });
        }
        Ok(())
    }
}

impl Display for Hash {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

macro_rules! id_type {
    ($name:ident, $prefix:literal) => {
        #[derive(
            Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub const PREFIX: &'static str = $prefix;

            pub fn from_hash(hash: &Hash) -> Self {
                Self(format!("{}{}", Self::PREFIX, hash.as_str()))
            }

            pub fn new_unchecked(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn validate(&self) -> Result<()> {
                let Some(suffix) = self.0.strip_prefix(Self::PREFIX) else {
                    return Err(Error::InvalidPrefix {
                        expected: Self::PREFIX,
                        actual: self.0.clone(),
                    });
                };
                if suffix.len() != 64 {
                    return Err(Error::InvalidHashLength {
                        expected: 64,
                        actual: suffix.len(),
                    });
                }
                Ok(())
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(ObjectId, "obj_");
id_type!(IdentityId, "id_");
id_type!(EdgeId, "edge_");
id_type!(EventId, "evt_");
id_type!(JudgmentId, "jud_");
id_type!(CapabilityGrantId, "grant_");
id_type!(RealtimeRoomId, "room_");
id_type!(RealtimeSessionId, "sess_");
id_type!(RealtimeMessageId, "msg_");
id_type!(RealtimeSnapshotId, "snap_");

#[derive(
    Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct Timestamp(
    #[serde(with = "time::serde::rfc3339")]
    #[schemars(with = "String")]
    pub OffsetDateTime,
);

impl Timestamp {
    pub fn now() -> Self {
        Self(OffsetDateTime::now_utc())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Protocol {
    pub name: String,
    pub version: u32,
}

impl Default for Protocol {
    fn default() -> Self {
        Self {
            name: PROTOCOL_VERSION.to_string(),
            version: 1,
        }
    }
}

pub trait Canonical {
    fn canonical_bytes(&self) -> Result<Vec<u8>>
    where
        Self: Serialize,
    {
        let value = serde_json::to_value(self).map_err(|err| Error::Canonical(err.to_string()))?;
        canonical_value_bytes(&value)
    }

    fn canonical_hash(&self) -> Result<Hash>
    where
        Self: Serialize,
    {
        Ok(Hash::from_bytes(&self.canonical_bytes()?))
    }
}

impl<T> Canonical for T where T: Serialize {}

pub fn canonical_value_bytes(value: &Value) -> Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(128);
    bytes.extend_from_slice(CANONICAL_PREAMBLE);
    encode_value(value, &mut bytes)?;
    Ok(bytes)
}

fn encode_value(value: &Value, bytes: &mut Vec<u8>) -> Result<()> {
    match value {
        Value::Null => bytes.push(b'n'),
        Value::Bool(false) => bytes.push(b'f'),
        Value::Bool(true) => bytes.push(b't'),
        Value::Number(number) => encode_number(number, bytes)?,
        Value::String(value) => encode_string(value, bytes),
        Value::Array(values) => {
            bytes.push(b'a');
            encode_len(values.len(), bytes)?;
            for value in values {
                encode_value(value, bytes)?;
            }
        }
        Value::Object(values) => encode_object(values, bytes)?,
    }
    Ok(())
}

fn encode_number(number: &Number, bytes: &mut Vec<u8>) -> Result<()> {
    if let Some(value) = number.as_i64() {
        bytes.push(b'i');
        bytes.extend_from_slice(&value.to_be_bytes());
        return Ok(());
    }
    if let Some(value) = number.as_u64() {
        bytes.push(b'u');
        bytes.extend_from_slice(&value.to_be_bytes());
        return Ok(());
    }
    let value = number
        .as_f64()
        .ok_or_else(|| Error::Canonical(format!("unsupported JSON number: {number}")))?;
    if !value.is_finite() {
        return Err(Error::Canonical(format!(
            "non-finite JSON number: {number}"
        )));
    }
    bytes.push(b'd');
    bytes.extend_from_slice(&value.to_bits().to_be_bytes());
    Ok(())
}

fn encode_string(value: &str, bytes: &mut Vec<u8>) {
    bytes.push(b's');
    encode_len(value.len(), bytes).expect("usize length always fits into u64");
    bytes.extend_from_slice(value.as_bytes());
}

fn encode_object(values: &Map<String, Value>, bytes: &mut Vec<u8>) -> Result<()> {
    bytes.push(b'o');
    encode_len(values.len(), bytes)?;
    let mut entries = values.iter().collect::<Vec<_>>();
    entries.sort_by(|(left, _), (right, _)| left.as_bytes().cmp(right.as_bytes()));
    for (key, value) in entries {
        encode_string(key, bytes);
        encode_value(value, bytes)?;
    }
    Ok(())
}

fn encode_len(len: usize, bytes: &mut Vec<u8>) -> Result<()> {
    let len = u64::try_from(len)
        .map_err(|_| Error::Canonical("value length does not fit in u64".to_string()))?;
    bytes.extend_from_slice(&len.to_be_bytes());
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Versioned<T> {
    pub protocol: Protocol,
    pub value: T,
}

impl<T> Versioned<T> {
    pub fn new(value: T) -> Self {
        Self {
            protocol: Protocol::default(),
            value,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, json};

    #[test]
    fn canonical_value_encoding_sorts_object_keys() {
        let first = json!({
            "beta": [2, 3],
            "alpha": {"z": true, "a": null}
        });
        let mut nested = Map::new();
        nested.insert("a".to_string(), Value::Null);
        nested.insert("z".to_string(), Value::Bool(true));
        let mut second_map = Map::new();
        second_map.insert("alpha".to_string(), Value::Object(nested));
        second_map.insert("beta".to_string(), json!([2, 3]));
        let second = Value::Object(second_map);

        assert_eq!(
            canonical_value_bytes(&first).unwrap(),
            canonical_value_bytes(&second).unwrap()
        );
        assert_eq!(
            first.canonical_hash().unwrap(),
            second.canonical_hash().unwrap()
        );
    }

    #[test]
    fn canonical_value_encoding_preserves_type_boundaries() {
        let text_one = canonical_value_bytes(&Value::String("1".to_string())).unwrap();
        let int_one = canonical_value_bytes(&json!(1)).unwrap();
        let float_one = canonical_value_bytes(&json!(1.0)).unwrap();
        let array_one = canonical_value_bytes(&json!([1])).unwrap();

        assert_ne!(text_one, int_one);
        assert_ne!(int_one, float_one);
        assert_ne!(int_one, array_one);
    }

    #[test]
    fn canonical_hash_uses_versioned_binary_preamble() {
        let bytes = json!({"a": 1}).canonical_bytes().unwrap();

        assert!(bytes.starts_with(CANONICAL_PREAMBLE));
        assert_eq!(CANONICAL_ENCODING_VERSION, "babble.canonical.v1");
    }
}
