use crate::{CapabilityBindingUsage, LocalNode};
use babble_capabilities::CapabilityReceipt;
use babble_judgment::JudgmentProvider;
use babble_store::LocalStorageRecord;
use babble_types::{IdentityId, ObjectId, Result};
use serde_json::Value;

const LOCAL_STORAGE_CAPABILITY: &str = "babble.storage.local";
const LOCAL_STORAGE_CAPABILITY_VERSION: u32 = 1;
const LOCAL_STORAGE_LIST_LIMIT: usize = 256;

impl<P> LocalNode<P>
where
    P: JudgmentProvider,
{
    pub fn local_storage_get(
        &self,
        object_id: &ObjectId,
        identity_id: &IdentityId,
        key: &str,
        grant_ids: &[String],
    ) -> Result<(Option<LocalStorageRecord>, CapabilityReceipt)> {
        self.check_ready()?;
        self.require_object(object_id)?;
        self.require_identity(identity_id)?;
        let receipt = self.authorize_local_storage_call(object_id, key, grant_ids, 0)?;
        let namespace = local_storage_namespace(&receipt)?;
        Ok((
            self.store
                .get_local_storage(object_id, identity_id, &namespace, key)?,
            receipt,
        ))
    }

    pub fn local_storage_set(
        &self,
        object_id: &ObjectId,
        identity_id: &IdentityId,
        key: &str,
        value: Value,
        grant_ids: &[String],
    ) -> Result<(LocalStorageRecord, CapabilityReceipt)> {
        self.check_ready()?;
        self.require_object(object_id)?;
        self.require_identity(identity_id)?;
        let requested_bytes = storage_value_size(&value)?;
        let receipt =
            self.authorize_local_storage_call(object_id, key, grant_ids, requested_bytes)?;
        let namespace = local_storage_namespace(&receipt)?;
        let current = self
            .store
            .get_local_storage(object_id, identity_id, &namespace, key)?;
        let current_size = current.as_ref().map_or(0, |record| record.size_bytes);
        let used = self.local_storage_used_bytes(object_id, identity_id, &namespace)?;
        let next_used = used
            .checked_sub(current_size)
            .and_then(|remaining| remaining.checked_add(requested_bytes))
            .ok_or_else(|| {
                babble_types::Error::Conflict("local storage byte accounting overflow".to_string())
            })?;
        if next_used > receipt.quota.persistent_bytes {
            return Err(babble_types::Error::Conflict(format!(
                "local storage quota exceeded: {next_used} > {}",
                receipt.quota.persistent_bytes
            )));
        }
        let record =
            self.store
                .put_local_storage(object_id, identity_id, &namespace, key, value)?;
        Ok((record, receipt))
    }

    pub fn local_storage_delete(
        &self,
        object_id: &ObjectId,
        identity_id: &IdentityId,
        key: &str,
        grant_ids: &[String],
    ) -> Result<(Option<LocalStorageRecord>, CapabilityReceipt)> {
        self.check_ready()?;
        self.require_object(object_id)?;
        self.require_identity(identity_id)?;
        let receipt = self.authorize_local_storage_call(object_id, key, grant_ids, 0)?;
        let namespace = local_storage_namespace(&receipt)?;
        Ok((
            self.store
                .delete_local_storage(object_id, identity_id, &namespace, key)?,
            receipt,
        ))
    }

    pub fn local_storage_list(
        &self,
        object_id: &ObjectId,
        identity_id: &IdentityId,
        prefix: Option<&str>,
        limit: usize,
        grant_ids: &[String],
    ) -> Result<(Vec<LocalStorageRecord>, CapabilityReceipt)> {
        self.check_ready()?;
        self.require_object(object_id)?;
        self.require_identity(identity_id)?;
        let receipt =
            self.authorize_local_storage_call(object_id, prefix.unwrap_or(""), grant_ids, 0)?;
        let namespace = local_storage_namespace(&receipt)?;
        Ok((
            self.store.list_local_storage(
                object_id,
                identity_id,
                &namespace,
                prefix,
                limit.min(LOCAL_STORAGE_LIST_LIMIT),
            )?,
            receipt,
        ))
    }

    fn require_identity(&self, identity_id: &IdentityId) -> Result<()> {
        self.identity(identity_id)
            .ok_or_else(|| babble_types::Error::NotFound(format!("identity {identity_id}")))?;
        Ok(())
    }

    fn authorize_local_storage_call(
        &self,
        object_id: &ObjectId,
        key: &str,
        grant_ids: &[String],
        requested_value_bytes: u64,
    ) -> Result<CapabilityReceipt> {
        let requested_bytes = (key.len() as u64)
            .checked_add(requested_value_bytes)
            .ok_or_else(|| {
                babble_types::Error::Conflict("local storage call size overflow".to_string())
            })?;
        self.authorize_capability_binding_with_usage(
            object_id,
            LOCAL_STORAGE_CAPABILITY,
            LOCAL_STORAGE_CAPABILITY_VERSION,
            grant_ids,
            CapabilityBindingUsage {
                requested_bytes,
                realtime_connections: 0,
                windows: Vec::new(),
            },
        )
    }

    fn local_storage_used_bytes(
        &self,
        object_id: &ObjectId,
        identity_id: &IdentityId,
        namespace: &str,
    ) -> Result<u64> {
        self.store
            .list_local_storage(object_id, identity_id, namespace, None, usize::MAX)?
            .into_iter()
            .try_fold(0_u64, |total, record| {
                total.checked_add(record.size_bytes).ok_or_else(|| {
                    babble_types::Error::Conflict("local storage usage overflow".to_string())
                })
            })
    }
}

fn local_storage_namespace(receipt: &CapabilityReceipt) -> Result<String> {
    let namespace = receipt
        .scope
        .get("namespace")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            babble_types::Error::Conflict(
                "storage.local grant scope must include namespace".to_string(),
            )
        })?;
    Ok(namespace.to_string())
}

fn storage_value_size(value: &Value) -> Result<u64> {
    serde_json::to_vec(value)
        .map(|bytes| bytes.len() as u64)
        .map_err(|err| babble_types::Error::Canonical(format!("encode local storage value: {err}")))
}
