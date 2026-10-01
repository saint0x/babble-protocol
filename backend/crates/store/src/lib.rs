use babel_graph::Edge;
use babel_identity::Identity;
use babel_judgment::Judgment;
use babel_object::Object;
use babel_personalization::EncryptedLocalUserModel;
use babel_state::Event;
use babel_types::{
    Canonical, EdgeId, Error as CoreError, EventId, Hash, IdentityId, JudgmentId, ObjectId, Result,
    Timestamp,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

mod blobs;
mod bundles;
mod following;
mod safety;
mod moderation;
mod invocations;
mod judgment_index;
mod object_judgments;
mod publication;
mod reactions;
mod receipts;
pub use blobs::BlobReadError;
pub use bundles::{VerifiedBundle, VerifiedBundleFile};
pub use object_judgments::ObjectJudgmentInput;
pub use publication::{PublicationBatch, PublicationError};
pub use receipts::{PublicationOutcome, PublicationReceipt, PublicationRequest};

#[derive(Clone, Debug)]
pub struct FileStore {
    root: PathBuf,
    recovery_required: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ObjectStorageRecord {
    pub object_id: ObjectId,
    pub key: String,
    pub value: Value,
    pub updated_at: Timestamp,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LocalStorageRecord {
    pub object_id: ObjectId,
    pub identity_id: IdentityId,
    pub namespace: String,
    pub key: String,
    pub value: Value,
    pub updated_at: Timestamp,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PersonalizationSyncRecord {
    pub envelope_hash: Hash,
    pub identity_id: IdentityId,
    pub device_id: String,
    pub envelope: EncryptedLocalUserModel,
    pub uploaded_at: Timestamp,
    pub size_bytes: u64,
}

impl FileStore {
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let store = Self {
            root: root.into(),
            recovery_required: Default::default(),
        };
        for dir in [
            "identities",
            "objects",
            "edges",
            "events",
            "judgments",
            "object_judgment_inputs",
            "publication_receipts",
            "invocations",
            "blobs",
            "object_storage",
            "local_storage",
            "personalization_sync",
        ] {
            fs::create_dir_all(store.root.join(dir))
                .map_err(|err| CoreError::Conflict(format!("create store dir {dir}: {err}")))?;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                store.root.join("invocations"),
                fs::Permissions::from_mode(0o700),
            )
            .map_err(|err| {
                CoreError::Conflict(format!("protect private invocation directory: {err}"))
            })?;
        }
        store.recover_publication()?;
        store.list_invocations()?;
        store.initialize_following()?;
        store.initialize_safety()?;
        store.initialize_moderation()?;
        store.initialize_reactions()?;
        Ok(store)
    }

    pub fn put_identity(&self, identity: &Identity) -> Result<()> {
        let _guard = self.publication_guard()?;
        identity.verify()?;
        write_json(&self.path("identities", identity.id.as_str())?, identity)
    }

    pub fn get_identity(&self, id: &IdentityId) -> Result<Option<Identity>> {
        let _guard = self.publication_guard()?;
        id.validate()?;
        read_json(&self.path("identities", id.as_str())?)
    }

    pub fn list_identities(&self) -> Result<Vec<Identity>> {
        let _guard = self.publication_guard()?;
        read_all_json(&self.root.join("identities"))
    }

    pub fn put_object(&self, object: &Object, author: &Identity) -> Result<()> {
        let _guard = self.publication_guard()?;
        object.verify(author)?;
        write_json(&self.path("objects", object.id.as_str())?, object)
    }

    pub fn get_object(&self, id: &ObjectId) -> Result<Option<Object>> {
        let _guard = self.publication_guard()?;
        id.validate()?;
        read_json(&self.path("objects", id.as_str())?)
    }

    pub fn list_objects(&self) -> Result<Vec<Object>> {
        let _guard = self.publication_guard()?;
        read_all_json(&self.root.join("objects"))
    }

    pub fn put_edge(&self, edge: &Edge, author: &Identity) -> Result<()> {
        let _guard = self.publication_guard()?;
        edge.verify(author)?;
        write_json(&self.path("edges", edge.id.as_str())?, edge)
    }

    pub fn get_edge(&self, id: &EdgeId) -> Result<Option<Edge>> {
        let _guard = self.publication_guard()?;
        id.validate()?;
        read_json(&self.path("edges", id.as_str())?)
    }

    pub fn list_edges(&self) -> Result<Vec<Edge>> {
        let _guard = self.publication_guard()?;
        read_all_json(&self.root.join("edges"))
    }

    pub fn put_event(&self, event: &Event, actor: &Identity) -> Result<()> {
        let _guard = self.publication_guard()?;
        event.verify(actor)?;
        if let Some(existing) = read_json::<Event>(&self.path("events", event.id.as_str())?)? {
            if existing == *event {
                return Ok(());
            }
            return Err(CoreError::Conflict(format!(
                "event id conflict: {}",
                event.id
            )));
        }
        write_json(&self.path("events", event.id.as_str())?, event)
    }

    pub fn get_event(&self, id: &EventId) -> Result<Option<Event>> {
        let _guard = self.publication_guard()?;
        id.validate()?;
        read_json(&self.path("events", id.as_str())?)
    }

    pub fn list_events(&self) -> Result<Vec<Event>> {
        let _guard = self.publication_guard()?;
        read_all_json(&self.root.join("events"))
    }

    pub fn put_judgment(&self, judgment: &Judgment) -> Result<()> {
        let mut batch = PublicationBatch::new();
        batch.judgment(judgment)?;
        self.commit_publication(batch)
            .map_err(|error| CoreError::Conflict(error.to_string()))
    }

    pub fn get_judgment(&self, id: &JudgmentId) -> Result<Option<Judgment>> {
        let _guard = self.publication_guard()?;
        id.validate()?;
        read_json(&self.path("judgments", id.as_str())?)
    }

    pub fn list_judgments(&self) -> Result<Vec<Judgment>> {
        let _guard = self.publication_guard()?;
        read_all_json(&self.root.join("judgments"))
    }

    pub fn contains_blob(&self, hash: &Hash) -> Result<bool> {
        hash.validate()?;
        Ok(self.blob_path(hash)?.exists())
    }

    pub fn put_object_storage(
        &self,
        object_id: &ObjectId,
        key: impl Into<String>,
        value: Value,
    ) -> Result<ObjectStorageRecord> {
        object_id.validate()?;
        let key = key.into();
        validate_storage_key(&key)?;
        let size_bytes = serde_json::to_vec(&value)
            .map_err(|err| CoreError::Canonical(format!("encode object storage value: {err}")))?
            .len() as u64;
        let record = ObjectStorageRecord {
            object_id: object_id.clone(),
            key,
            value,
            updated_at: Timestamp::now(),
            size_bytes,
        };
        write_json(&self.object_storage_path(object_id, &record.key)?, &record)?;
        Ok(record)
    }

    pub fn get_object_storage(
        &self,
        object_id: &ObjectId,
        key: &str,
    ) -> Result<Option<ObjectStorageRecord>> {
        object_id.validate()?;
        validate_storage_key(key)?;
        read_json(&self.object_storage_path(object_id, key)?)
    }

    pub fn list_object_storage(
        &self,
        object_id: &ObjectId,
        prefix: Option<&str>,
        limit: usize,
    ) -> Result<Vec<ObjectStorageRecord>> {
        object_id.validate()?;
        if let Some(prefix) = prefix {
            validate_storage_prefix(prefix)?;
        }
        let dir = self.object_storage_dir(object_id)?;
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut records = read_all_json::<ObjectStorageRecord>(&dir)?;
        records.retain(|record| prefix.is_none_or(|prefix| record.key.starts_with(prefix)));
        records.sort_by(|left, right| left.key.cmp(&right.key));
        records.truncate(limit);
        Ok(records)
    }

    pub fn delete_object_storage(
        &self,
        object_id: &ObjectId,
        key: &str,
    ) -> Result<Option<ObjectStorageRecord>> {
        object_id.validate()?;
        validate_storage_key(key)?;
        let path = self.object_storage_path(object_id, key)?;
        let record = read_json(&path)?;
        if record.is_some() {
            fs::remove_file(&path)
                .map_err(|err| CoreError::Conflict(format!("delete {}: {err}", path.display())))?;
        }
        Ok(record)
    }

    pub fn put_local_storage(
        &self,
        object_id: &ObjectId,
        identity_id: &IdentityId,
        namespace: &str,
        key: impl Into<String>,
        value: Value,
    ) -> Result<LocalStorageRecord> {
        object_id.validate()?;
        identity_id.validate()?;
        validate_storage_namespace(namespace)?;
        let key = key.into();
        validate_storage_key(&key)?;
        let size_bytes = serde_json::to_vec(&value)
            .map_err(|err| CoreError::Canonical(format!("encode local storage value: {err}")))?
            .len() as u64;
        let record = LocalStorageRecord {
            object_id: object_id.clone(),
            identity_id: identity_id.clone(),
            namespace: namespace.to_string(),
            key,
            value,
            updated_at: Timestamp::now(),
            size_bytes,
        };
        write_json(
            &self.local_storage_path(object_id, identity_id, namespace, &record.key)?,
            &record,
        )?;
        Ok(record)
    }

    pub fn get_local_storage(
        &self,
        object_id: &ObjectId,
        identity_id: &IdentityId,
        namespace: &str,
        key: &str,
    ) -> Result<Option<LocalStorageRecord>> {
        object_id.validate()?;
        identity_id.validate()?;
        validate_storage_namespace(namespace)?;
        validate_storage_key(key)?;
        read_json(&self.local_storage_path(object_id, identity_id, namespace, key)?)
    }

    pub fn list_local_storage(
        &self,
        object_id: &ObjectId,
        identity_id: &IdentityId,
        namespace: &str,
        prefix: Option<&str>,
        limit: usize,
    ) -> Result<Vec<LocalStorageRecord>> {
        object_id.validate()?;
        identity_id.validate()?;
        validate_storage_namespace(namespace)?;
        if let Some(prefix) = prefix {
            validate_storage_prefix(prefix)?;
        }
        let dir = self.local_storage_dir(object_id, identity_id, namespace)?;
        if !dir.exists() {
            return Ok(Vec::new());
        }
        let mut records = read_all_json::<LocalStorageRecord>(&dir)?;
        records.retain(|record| prefix.is_none_or(|prefix| record.key.starts_with(prefix)));
        records.sort_by(|left, right| left.key.cmp(&right.key));
        records.truncate(limit);
        Ok(records)
    }

    pub fn delete_local_storage(
        &self,
        object_id: &ObjectId,
        identity_id: &IdentityId,
        namespace: &str,
        key: &str,
    ) -> Result<Option<LocalStorageRecord>> {
        object_id.validate()?;
        identity_id.validate()?;
        validate_storage_namespace(namespace)?;
        validate_storage_key(key)?;
        let path = self.local_storage_path(object_id, identity_id, namespace, key)?;
        let record = read_json(&path)?;
        if record.is_some() {
            fs::remove_file(&path)
                .map_err(|err| CoreError::Conflict(format!("delete {}: {err}", path.display())))?;
        }
        Ok(record)
    }

    pub fn put_personalization_sync_envelope(
        &self,
        envelope: EncryptedLocalUserModel,
    ) -> Result<PersonalizationSyncRecord> {
        envelope.validate()?;
        let envelope_hash = envelope.canonical_hash()?;
        let identity_id = envelope.recipient.identity_id.clone();
        let device_id = envelope.recipient.device_id.clone();
        let size_bytes = serde_json::to_vec(&envelope)
            .map_err(|err| {
                CoreError::Canonical(format!("encode personalization sync envelope: {err}"))
            })?
            .len() as u64;
        let record = PersonalizationSyncRecord {
            envelope_hash,
            identity_id,
            device_id,
            envelope,
            uploaded_at: Timestamp::now(),
            size_bytes,
        };
        write_json(
            &self.personalization_sync_path(
                &record.identity_id,
                &record.device_id,
                &record.envelope_hash,
            )?,
            &record,
        )?;
        Ok(record)
    }

    pub fn get_personalization_sync_envelope(
        &self,
        identity_id: &IdentityId,
        device_id: &str,
        envelope_hash: &Hash,
    ) -> Result<Option<PersonalizationSyncRecord>> {
        identity_id.validate()?;
        validate_sync_device_id(device_id)?;
        envelope_hash.validate()?;
        read_json(&self.personalization_sync_path(identity_id, device_id, envelope_hash)?)
    }

    pub fn list_personalization_sync_envelopes(
        &self,
        identity_id: &IdentityId,
        device_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PersonalizationSyncRecord>> {
        identity_id.validate()?;
        let mut records = Vec::new();
        if let Some(device_id) = device_id {
            validate_sync_device_id(device_id)?;
            let dir = self.personalization_sync_device_dir(identity_id, device_id)?;
            if dir.exists() {
                records.extend(read_all_json::<PersonalizationSyncRecord>(&dir)?);
            }
        } else {
            let dir = self.personalization_sync_identity_dir(identity_id)?;
            if dir.exists() {
                for device_dir in fs::read_dir(&dir).map_err(|err| {
                    CoreError::Conflict(format!("read dir {}: {err}", dir.display()))
                })? {
                    let device_dir = device_dir
                        .map_err(|err| {
                            CoreError::Conflict(format!("read dir {}: {err}", dir.display()))
                        })?
                        .path();
                    if device_dir.is_dir() {
                        records.extend(read_all_json::<PersonalizationSyncRecord>(&device_dir)?);
                    }
                }
            }
        }
        records.sort_by(|left, right| {
            left.uploaded_at
                .cmp(&right.uploaded_at)
                .then_with(|| left.envelope_hash.cmp(&right.envelope_hash))
        });
        records.reverse();
        records.truncate(limit);
        Ok(records)
    }

    pub fn delete_personalization_sync_envelope(
        &self,
        identity_id: &IdentityId,
        device_id: &str,
        envelope_hash: &Hash,
    ) -> Result<Option<PersonalizationSyncRecord>> {
        identity_id.validate()?;
        validate_sync_device_id(device_id)?;
        envelope_hash.validate()?;
        let path = self.personalization_sync_path(identity_id, device_id, envelope_hash)?;
        let record = read_json(&path)?;
        if record.is_some() {
            fs::remove_file(&path)
                .map_err(|err| CoreError::Conflict(format!("delete {}: {err}", path.display())))?;
        }
        Ok(record)
    }

    fn path(&self, dir: &str, id: &str) -> Result<PathBuf> {
        ensure_safe_component(id)?;
        Ok(self.root.join(dir).join(format!("{id}.json")))
    }

    fn blob_path(&self, hash: &Hash) -> Result<PathBuf> {
        hash.validate()?;
        let value = hash.as_str();
        if !value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(CoreError::Canonical(
                "blob hash must be 64 lowercase hexadecimal characters".into(),
            ));
        }
        Ok(self.root.join("blobs").join(value))
    }

    fn object_storage_dir(&self, object_id: &ObjectId) -> Result<PathBuf> {
        object_id.validate()?;
        ensure_safe_component(object_id.as_str())?;
        Ok(self.root.join("object_storage").join(object_id.as_str()))
    }

    fn object_storage_path(&self, object_id: &ObjectId, key: &str) -> Result<PathBuf> {
        validate_storage_key(key)?;
        let key_hash = Hash::from_bytes(key.as_bytes());
        Ok(self
            .object_storage_dir(object_id)?
            .join(format!("{}.json", key_hash.as_str())))
    }

    fn local_storage_dir(
        &self,
        object_id: &ObjectId,
        identity_id: &IdentityId,
        namespace: &str,
    ) -> Result<PathBuf> {
        object_id.validate()?;
        identity_id.validate()?;
        validate_storage_namespace(namespace)?;
        ensure_safe_component(object_id.as_str())?;
        ensure_safe_component(identity_id.as_str())?;
        let namespace_hash = Hash::from_bytes(namespace.as_bytes());
        Ok(self
            .root
            .join("local_storage")
            .join(object_id.as_str())
            .join(identity_id.as_str())
            .join(namespace_hash.as_str()))
    }

    fn local_storage_path(
        &self,
        object_id: &ObjectId,
        identity_id: &IdentityId,
        namespace: &str,
        key: &str,
    ) -> Result<PathBuf> {
        validate_storage_key(key)?;
        let key_hash = Hash::from_bytes(key.as_bytes());
        Ok(self
            .local_storage_dir(object_id, identity_id, namespace)?
            .join(format!("{}.json", key_hash.as_str())))
    }

    fn personalization_sync_identity_dir(&self, identity_id: &IdentityId) -> Result<PathBuf> {
        identity_id.validate()?;
        ensure_safe_component(identity_id.as_str())?;
        Ok(self
            .root
            .join("personalization_sync")
            .join(identity_id.as_str()))
    }

    fn personalization_sync_device_dir(
        &self,
        identity_id: &IdentityId,
        device_id: &str,
    ) -> Result<PathBuf> {
        validate_sync_device_id(device_id)?;
        let device_hash = Hash::from_bytes(device_id.as_bytes());
        Ok(self
            .personalization_sync_identity_dir(identity_id)?
            .join(device_hash.as_str()))
    }

    fn personalization_sync_path(
        &self,
        identity_id: &IdentityId,
        device_id: &str,
        envelope_hash: &Hash,
    ) -> Result<PathBuf> {
        envelope_hash.validate()?;
        Ok(self
            .personalization_sync_device_dir(identity_id, device_id)?
            .join(format!("{}.json", envelope_hash.as_str())))
    }
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value)
        .map_err(|err| CoreError::Canonical(format!("encode {}: {err}", path.display())))?;
    write_atomic(path, &bytes)
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path)
        .map_err(|err| CoreError::Conflict(format!("read {}: {err}", path.display())))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|err| CoreError::Canonical(format!("decode {}: {err}", path.display())))
}

fn read_all_json<T: DeserializeOwned>(dir: &Path) -> Result<Vec<T>> {
    let mut paths = fs::read_dir(dir)
        .map_err(|err| CoreError::Conflict(format!("read dir {}: {err}", dir.display())))?
        .map(|entry| {
            entry
                .map(|entry| entry.path())
                .map_err(|err| CoreError::Conflict(format!("read dir {}: {err}", dir.display())))
        })
        .collect::<Result<Vec<_>>>()?;
    paths.sort();

    let mut values = Vec::new();
    for path in paths {
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        if let Some(value) = read_json(&path)? {
            values.push(value);
        }
    }
    Ok(values)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| CoreError::Conflict(format!("missing parent for {}", path.display())))?;
    fs::create_dir_all(parent)
        .map_err(|err| CoreError::Conflict(format!("create {}: {err}", parent.display())))?;

    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    {
        let mut file = fs::File::create(&tmp)
            .map_err(|err| CoreError::Conflict(format!("create {}: {err}", tmp.display())))?;
        file.write_all(bytes)
            .map_err(|err| CoreError::Conflict(format!("write {}: {err}", tmp.display())))?;
        file.sync_all()
            .map_err(|err| CoreError::Conflict(format!("sync {}: {err}", tmp.display())))?;
    }
    fs::rename(&tmp, path)
        .map_err(|err| CoreError::Conflict(format!("rename {}: {err}", tmp.display())))?;
    Ok(())
}

fn ensure_safe_component(value: &str) -> Result<()> {
    let safe = value
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if safe {
        Ok(())
    } else {
        Err(CoreError::Conflict(format!(
            "unsafe store path component: {value}"
        )))
    }
}

fn validate_storage_key(value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 256
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
        })
        && value
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."));
    if valid {
        Ok(())
    } else {
        Err(CoreError::Conflict(format!(
            "invalid object storage key: {value}"
        )))
    }
}

fn validate_storage_namespace(value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
        })
        && value
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."));
    if valid {
        Ok(())
    } else {
        Err(CoreError::Conflict(format!(
            "invalid local storage namespace: {value}"
        )))
    }
}

fn validate_storage_prefix(value: &str) -> Result<()> {
    if value.is_empty() {
        return Ok(());
    }
    let valid = value.len() <= 256
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':' | b'/')
        })
        && value.split('/').enumerate().all(|(index, segment)| {
            let final_empty = segment.is_empty() && index + 1 == value.split('/').count();
            final_empty || !matches!(segment, "" | "." | "..")
        });
    if valid {
        Ok(())
    } else {
        Err(CoreError::Conflict(format!(
            "invalid object storage prefix: {value}"
        )))
    }
}

fn validate_sync_device_id(value: &str) -> Result<()> {
    let valid = !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'));
    if valid {
        Ok(())
    } else {
        Err(CoreError::Conflict(format!(
            "invalid personalization sync device id: {value}"
        )))
    }
}
