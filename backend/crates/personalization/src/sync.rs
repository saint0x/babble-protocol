use crate::LocalUserModel;
use babble_types::{Canonical, Error, Hash, IdentityId, Result, Timestamp};
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand_core::{OsRng, RngCore};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::fmt::{Debug, Formatter};
use zeroize::{Zeroize, ZeroizeOnDrop};

pub const PERSONALIZATION_SYNC_VERSION: &str = "babble.personalization.sync.v1";
pub const PERSONALIZATION_SYNC_DATA_CLASS: &str = "encrypted_synchronized_state";
pub const PERSONALIZATION_SYNC_ALGORITHM: &str = "XChaCha20-Poly1305";
const PERSONALIZATION_MODEL_PAYLOAD: &str = "babble.personalization.local_user_model.v1";
const SYNC_KEY_BYTES: usize = 32;
const XCHACHA_NONCE_BYTES: usize = 24;

#[derive(Clone, Eq, PartialEq, Zeroize, ZeroizeOnDrop)]
pub struct PersonalizationSyncKey {
    bytes: [u8; SYNC_KEY_BYTES],
}

impl PersonalizationSyncKey {
    pub fn generate() -> Self {
        let mut bytes = [0_u8; SYNC_KEY_BYTES];
        OsRng.fill_bytes(&mut bytes);
        Self { bytes }
    }

    pub fn from_hex(value: &str) -> Result<Self> {
        let bytes = hex::decode(value.trim())
            .map_err(|_| Error::Conflict("invalid personalization sync key hex".to_string()))?;
        let bytes: [u8; SYNC_KEY_BYTES] = bytes.try_into().map_err(|_| {
            Error::Conflict(format!(
                "personalization sync key must be {SYNC_KEY_BYTES} bytes"
            ))
        })?;
        Ok(Self { bytes })
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.bytes)
    }

    fn cipher(&self) -> XChaCha20Poly1305 {
        XChaCha20Poly1305::new(Key::from_slice(&self.bytes))
    }
}

impl Debug for PersonalizationSyncKey {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PersonalizationSyncKey")
            .field("bytes", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersonalizationSyncRecipient {
    pub identity_id: IdentityId,
    pub device_id: String,
}

impl PersonalizationSyncRecipient {
    pub fn new(identity_id: IdentityId, device_id: impl Into<String>) -> Result<Self> {
        let recipient = Self {
            identity_id,
            device_id: device_id.into(),
        };
        recipient.validate()?;
        Ok(recipient)
    }

    pub fn validate(&self) -> Result<()> {
        self.identity_id.validate()?;
        validate_device_id(&self.device_id)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EncryptedLocalUserModel {
    pub version: String,
    pub data_class: String,
    pub algorithm: String,
    pub recipient: PersonalizationSyncRecipient,
    pub model_revision: Option<String>,
    pub exported_at: Timestamp,
    pub nonce: String,
    pub ciphertext: String,
}

impl EncryptedLocalUserModel {
    pub fn seal(
        model: &LocalUserModel,
        recipient: PersonalizationSyncRecipient,
        key: &PersonalizationSyncKey,
    ) -> Result<Self> {
        let mut nonce = [0_u8; XCHACHA_NONCE_BYTES];
        OsRng.fill_bytes(&mut nonce);
        Self::seal_with_nonce(model, recipient, key, Timestamp::now(), nonce)
    }

    pub fn open(
        &self,
        expected_recipient: &PersonalizationSyncRecipient,
        key: &PersonalizationSyncKey,
    ) -> Result<LocalUserModel> {
        self.validate()?;
        expected_recipient.validate()?;
        if &self.recipient != expected_recipient {
            return Err(Error::Signature);
        }
        let nonce_bytes = decode_fixed_hex::<XCHACHA_NONCE_BYTES>(&self.nonce, "nonce")?;
        let ciphertext = hex::decode(&self.ciphertext)
            .map_err(|_| Error::Conflict("invalid personalization ciphertext hex".to_string()))?;
        let plaintext = key
            .cipher()
            .decrypt(
                XNonce::from_slice(&nonce_bytes),
                chacha20poly1305::aead::Payload {
                    msg: &ciphertext,
                    aad: &self.associated_data()?,
                },
            )
            .map_err(|_| Error::Signature)?;
        let payload: LocalUserModelSyncPayload =
            serde_json::from_slice(&plaintext).map_err(|err| {
                Error::Canonical(format!("decode encrypted LocalUserModel payload: {err}"))
            })?;
        if payload.version != PERSONALIZATION_MODEL_PAYLOAD {
            return Err(Error::Conflict(format!(
                "unsupported personalization payload version: {}",
                payload.version
            )));
        }
        Ok(payload.model)
    }

    pub fn ciphertext_hash(&self) -> Result<Hash> {
        let bytes = hex::decode(&self.ciphertext)
            .map_err(|_| Error::Conflict("invalid personalization ciphertext hex".to_string()))?;
        Ok(Hash::from_bytes(&bytes))
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != PERSONALIZATION_SYNC_VERSION {
            return Err(Error::Conflict(format!(
                "unsupported personalization sync version: {}",
                self.version
            )));
        }
        if self.data_class != PERSONALIZATION_SYNC_DATA_CLASS {
            return Err(Error::Conflict(format!(
                "unexpected personalization sync data class: {}",
                self.data_class
            )));
        }
        if self.algorithm != PERSONALIZATION_SYNC_ALGORITHM {
            return Err(Error::Conflict(format!(
                "unsupported personalization sync algorithm: {}",
                self.algorithm
            )));
        }
        self.recipient.validate()?;
        if let Some(revision) = &self.model_revision {
            validate_revision(revision)?;
        }
        decode_fixed_hex::<XCHACHA_NONCE_BYTES>(&self.nonce, "nonce")?;
        let ciphertext = hex::decode(&self.ciphertext)
            .map_err(|_| Error::Conflict("invalid personalization ciphertext hex".to_string()))?;
        if ciphertext.is_empty() {
            return Err(Error::Conflict(
                "personalization ciphertext cannot be empty".to_string(),
            ));
        }
        Ok(())
    }

    pub(crate) fn seal_with_nonce(
        model: &LocalUserModel,
        recipient: PersonalizationSyncRecipient,
        key: &PersonalizationSyncKey,
        exported_at: Timestamp,
        nonce: [u8; XCHACHA_NONCE_BYTES],
    ) -> Result<Self> {
        recipient.validate()?;
        if let Some(revision) = &model.model_revision {
            validate_revision(revision)?;
        }
        let payload = LocalUserModelSyncPayload {
            version: PERSONALIZATION_MODEL_PAYLOAD.to_string(),
            model: model.normalized(),
        };
        let plaintext = serde_json::to_vec(&payload).map_err(|err| {
            Error::Canonical(format!("encode encrypted LocalUserModel payload: {err}"))
        })?;
        let mut envelope = Self {
            version: PERSONALIZATION_SYNC_VERSION.to_string(),
            data_class: PERSONALIZATION_SYNC_DATA_CLASS.to_string(),
            algorithm: PERSONALIZATION_SYNC_ALGORITHM.to_string(),
            recipient,
            model_revision: payload.model.model_revision.clone(),
            exported_at,
            nonce: hex::encode(nonce),
            ciphertext: String::new(),
        };
        let ciphertext = key
            .cipher()
            .encrypt(
                XNonce::from_slice(&nonce),
                chacha20poly1305::aead::Payload {
                    msg: &plaintext,
                    aad: &envelope.associated_data()?,
                },
            )
            .map_err(|_| Error::Signature)?;
        envelope.ciphertext = hex::encode(ciphertext);
        Ok(envelope)
    }

    fn associated_data(&self) -> Result<Vec<u8>> {
        PersonalizationSyncAssociatedData {
            version: self.version.clone(),
            data_class: self.data_class.clone(),
            algorithm: self.algorithm.clone(),
            recipient: self.recipient.clone(),
            model_revision: self.model_revision.clone(),
            exported_at: self.exported_at,
        }
        .canonical_bytes()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct LocalUserModelSyncPayload {
    version: String,
    model: LocalUserModel,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct PersonalizationSyncAssociatedData {
    version: String,
    data_class: String,
    algorithm: String,
    recipient: PersonalizationSyncRecipient,
    model_revision: Option<String>,
    exported_at: Timestamp,
}

fn decode_fixed_hex<const N: usize>(value: &str, label: &str) -> Result<[u8; N]> {
    let bytes = hex::decode(value)
        .map_err(|_| Error::Conflict(format!("invalid personalization {label} hex")))?;
    bytes
        .try_into()
        .map_err(|_| Error::Conflict(format!("personalization {label} must be {N} bytes")))
}

fn validate_device_id(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 {
        return Err(Error::Conflict(
            "personalization sync device id must be 1..128 bytes".to_string(),
        ));
    }
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
    {
        return Err(Error::Conflict(
            "personalization sync device id contains unsupported characters".to_string(),
        ));
    }
    Ok(())
}

fn validate_revision(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 {
        return Err(Error::Conflict(
            "personalization model revision must be 1..128 bytes".to_string(),
        ));
    }
    if value.chars().any(char::is_control) {
        return Err(Error::Conflict(
            "personalization model revision cannot contain control characters".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use babble_types::Hash;
    use std::collections::{BTreeMap, BTreeSet};
    use time::OffsetDateTime;

    #[test]
    fn sync_key_debug_and_serialization_do_not_expose_secret() {
        let key = PersonalizationSyncKey::from_hex(
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        .unwrap();

        assert_eq!(
            key.to_hex(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert!(!format!("{key:?}").contains("aaaaaaaa"));
    }

    #[test]
    fn encrypted_user_model_round_trips_without_plaintext_leakage() {
        let key = fixed_key("11");
        let recipient = recipient("alice", "desktop-main");
        let model = private_model();

        let envelope = EncryptedLocalUserModel::seal_with_nonce(
            &model,
            recipient.clone(),
            &key,
            timestamp(1000),
            [7_u8; XCHACHA_NONCE_BYTES],
        )
        .unwrap();
        let opened = envelope.open(&recipient, &key).unwrap();

        assert_eq!(opened, model.normalized());
        assert_eq!(envelope.data_class, PERSONALIZATION_SYNC_DATA_CLASS);
        assert_eq!(envelope.algorithm, PERSONALIZATION_SYNC_ALGORITHM);
        assert_eq!(envelope.model_revision.as_deref(), Some("rev-private-7"));
        assert_ne!(envelope.ciphertext_hash().unwrap(), Hash::from_bytes(b""));
        let serialized = serde_json::to_string(&envelope).unwrap();
        assert!(!serialized.contains("distributed systems"));
        assert!(!serialized.contains("spoiler"));
        assert!(!serialized.contains("obj_"));
        assert!(serialized.contains("encrypted_synchronized_state"));
    }

    #[test]
    fn encrypted_user_model_rejects_wrong_key_and_recipient() {
        let key = fixed_key("22");
        let other_key = fixed_key("33");
        let target_recipient = recipient("alice", "desktop-main");
        let other_recipient = recipient("alice", "phone-main");
        let envelope = EncryptedLocalUserModel::seal_with_nonce(
            &private_model(),
            target_recipient.clone(),
            &key,
            timestamp(2000),
            [8_u8; XCHACHA_NONCE_BYTES],
        )
        .unwrap();

        assert!(envelope.open(&target_recipient, &other_key).is_err());
        assert!(envelope.open(&other_recipient, &key).is_err());
    }

    #[test]
    fn encrypted_user_model_detects_metadata_and_ciphertext_tampering() {
        let key = fixed_key("44");
        let recipient = recipient("alice", "desktop-main");
        let envelope = EncryptedLocalUserModel::seal_with_nonce(
            &private_model(),
            recipient.clone(),
            &key,
            timestamp(3000),
            [9_u8; XCHACHA_NONCE_BYTES],
        )
        .unwrap();

        let mut tampered_recipient = envelope.clone();
        tampered_recipient.recipient.device_id = "desktop-other".to_string();
        assert!(
            tampered_recipient
                .open(&tampered_recipient.recipient, &key)
                .is_err()
        );

        let mut tampered_revision = envelope.clone();
        tampered_revision.model_revision = Some("rev-public".to_string());
        assert!(tampered_revision.open(&recipient, &key).is_err());

        let mut tampered_ciphertext = envelope;
        let mut bytes = hex::decode(&tampered_ciphertext.ciphertext).unwrap();
        bytes[0] ^= 0x01;
        tampered_ciphertext.ciphertext = hex::encode(bytes);
        assert!(tampered_ciphertext.open(&recipient, &key).is_err());
    }

    #[test]
    fn sync_recipient_validation_rejects_ambiguous_device_ids() {
        let identity_id = identity_id("alice");
        assert!(PersonalizationSyncRecipient::new(identity_id.clone(), "").is_err());
        assert!(PersonalizationSyncRecipient::new(identity_id.clone(), "with space").is_err());
        assert!(PersonalizationSyncRecipient::new(identity_id, "desktop-main").is_ok());
    }

    fn private_model() -> LocalUserModel {
        LocalUserModel {
            model_revision: Some("rev-private-7".to_string()),
            interests: vec!["distributed systems".to_string(), "CRDT".to_string()],
            expertise: vec!["protocol design".to_string()],
            muted_terms: vec!["spoiler".to_string()],
            hidden_terms: vec!["ragebait".to_string()],
            hidden_authors: BTreeSet::from([identity_id("blocked")]),
            creator_affinity: BTreeMap::from([(identity_id("creator"), 0.82)]),
            seen_objects: BTreeMap::from([(object_id("seen"), 2)]),
            novelty_tolerance: 0.72,
            exploration_preference: 0.67,
            evidence_preference: 0.91,
            contradiction_tolerance: 0.44,
        }
    }

    fn recipient(identity_seed: &str, device_id: &str) -> PersonalizationSyncRecipient {
        PersonalizationSyncRecipient::new(identity_id(identity_seed), device_id).unwrap()
    }

    fn fixed_key(byte_hex: &str) -> PersonalizationSyncKey {
        PersonalizationSyncKey::from_hex(&byte_hex.repeat(SYNC_KEY_BYTES)).unwrap()
    }

    fn timestamp(seconds: i64) -> Timestamp {
        Timestamp(OffsetDateTime::from_unix_timestamp(seconds).unwrap())
    }

    fn object_id(seed: &str) -> babble_types::ObjectId {
        babble_types::ObjectId::from_hash(&Hash::from_bytes(seed.as_bytes()))
    }

    fn identity_id(seed: &str) -> IdentityId {
        IdentityId::from_hash(&Hash::from_bytes(seed.as_bytes()))
    }
}
