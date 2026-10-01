use crate::{CapabilityBindingUsage, LocalNode};
use babble_capabilities::CapabilityReceipt;
use babble_identity::Identity;
use babble_judgment::JudgmentProvider;
use babble_types::{IdentityId, ObjectId, Result};

const IDENTITY_CURRENT_CAPABILITY: &str = "babble.identity.current";
const IDENTITY_CURRENT_VERSION: u32 = 1;

impl<P> LocalNode<P>
where
    P: JudgmentProvider,
{
    pub fn identity_current(
        &self,
        object_id: &ObjectId,
        identity_id: &IdentityId,
        grant_ids: &[String],
    ) -> Result<(Identity, CapabilityReceipt)> {
        self.check_ready()?;
        self.require_object(object_id)?;
        let identity = self
            .identity(identity_id)
            .ok_or_else(|| babble_types::Error::NotFound(format!("identity {identity_id}")))?
            .clone();
        let receipt = self.authorize_capability_binding_with_usage(
            object_id,
            IDENTITY_CURRENT_CAPABILITY,
            IDENTITY_CURRENT_VERSION,
            grant_ids,
            CapabilityBindingUsage {
                requested_bytes: identity.handle.len() as u64,
                realtime_connections: 0,
                windows: Vec::new(),
            },
        )?;
        Ok((identity, receipt))
    }
}
