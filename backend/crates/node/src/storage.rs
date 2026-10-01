use crate::{CapabilityBindingUsage, LocalNode};
use babel_capabilities::CapabilityReceipt;
use babel_judgment::JudgmentProvider;
use babel_store::ObjectStorageRecord;
use babel_types::{ObjectId, Result};
use serde_json::Value;

const OBJECT_STORAGE_CAPABILITY: &str = "babel.storage.object";
const OBJECT_STORAGE_CAPABILITY_VERSION: u32 = 1;
const OBJECT_STORAGE_LIST_LIMIT: usize = 256;

impl<P> LocalNode<P>
where
    P: JudgmentProvider,
{
    pub fn object_storage_get(
        &self,
        object_id: &ObjectId,
        key: &str,
        grant_ids: &[String],
    ) -> Result<(Option<ObjectStorageRecord>, CapabilityReceipt)> {
        self.check_ready()?;
        self.require_object(object_id)?;
        let receipt = self.authorize_storage_call(object_id, key, grant_ids, 0)?;
        Ok((self.store.get_object_storage(object_id, key)?, receipt))
    }

    pub fn object_storage_set(
        &self,
        object_id: &ObjectId,
        key: &str,
        value: Value,
        grant_ids: &[String],
    ) -> Result<(ObjectStorageRecord, CapabilityReceipt)> {
        self.check_ready()?;
        self.require_object(object_id)?;
        let requested_bytes = storage_value_size(&value)?;
        let receipt = self.authorize_storage_call(object_id, key, grant_ids, requested_bytes)?;
        let current = self.store.get_object_storage(object_id, key)?;
        let current_size = current.as_ref().map_or(0, |record| record.size_bytes);
        let used = self.object_storage_used_bytes(object_id)?;
        let next_used = used
            .checked_sub(current_size)
            .and_then(|remaining| remaining.checked_add(requested_bytes))
            .ok_or_else(|| {
                babel_types::Error::Conflict("object storage byte accounting overflow".to_string())
            })?;
        if next_used > receipt.quota.persistent_bytes {
            return Err(babel_types::Error::Conflict(format!(
                "object storage quota exceeded: {next_used} > {}",
                receipt.quota.persistent_bytes
            )));
        }
        let record = self.store.put_object_storage(object_id, key, value)?;
        Ok((record, receipt))
    }

    pub fn object_storage_delete(
        &self,
        object_id: &ObjectId,
        key: &str,
        grant_ids: &[String],
    ) -> Result<(Option<ObjectStorageRecord>, CapabilityReceipt)> {
        self.check_ready()?;
        self.require_object(object_id)?;
        let receipt = self.authorize_storage_call(object_id, key, grant_ids, 0)?;
        Ok((self.store.delete_object_storage(object_id, key)?, receipt))
    }

    pub fn object_storage_list(
        &self,
        object_id: &ObjectId,
        prefix: Option<&str>,
        limit: usize,
        grant_ids: &[String],
    ) -> Result<(Vec<ObjectStorageRecord>, CapabilityReceipt)> {
        self.check_ready()?;
        self.require_object(object_id)?;
        let receipt = self.authorize_storage_call(object_id, prefix.unwrap_or(""), grant_ids, 0)?;
        let entries = self
            .store
            .list_object_storage(object_id, prefix, usize::MAX)?
            .into_iter()
            .filter(|entry| !entry.key.starts_with("runtime/"))
            .take(limit.min(OBJECT_STORAGE_LIST_LIMIT))
            .collect();
        Ok((entries, receipt))
    }

    fn authorize_storage_call(
        &self,
        object_id: &ObjectId,
        key: &str,
        grant_ids: &[String],
        requested_value_bytes: u64,
    ) -> Result<CapabilityReceipt> {
        if key.starts_with("runtime/") {
            return Err(babel_types::Error::Conflict(
                "runtime state is private to its session".into(),
            ));
        }
        let requested_bytes = (key.len() as u64)
            .checked_add(requested_value_bytes)
            .ok_or_else(|| {
                babel_types::Error::Conflict("storage call size overflow".to_string())
            })?;
        self.authorize_capability_binding_with_usage(
            object_id,
            OBJECT_STORAGE_CAPABILITY,
            OBJECT_STORAGE_CAPABILITY_VERSION,
            grant_ids,
            CapabilityBindingUsage {
                requested_bytes,
                realtime_connections: 0,
                windows: Vec::new(),
            },
        )
    }

    fn object_storage_used_bytes(&self, object_id: &ObjectId) -> Result<u64> {
        self.store
            .list_object_storage(object_id, None, usize::MAX)?
            .into_iter()
            .try_fold(0_u64, |total, record| {
                total.checked_add(record.size_bytes).ok_or_else(|| {
                    babel_types::Error::Conflict("object storage usage overflow".to_string())
                })
            })
    }
}

fn storage_value_size(value: &Value) -> Result<u64> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len() as u64)
        .map_err(|err| babel_types::Error::Canonical(format!("encode object storage value: {err}")))
}
