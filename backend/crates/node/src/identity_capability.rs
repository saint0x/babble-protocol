use crate::{CapabilityBindingUsage, LocalNode};
use babel_capabilities::CapabilityReceipt;
use babel_identity::Identity;
use babel_judgment::JudgmentProvider;
use babel_types::{IdentityId, ObjectId, Result};

const IDENTITY_CURRENT_CAPABILITY: &str = "babel.identity.current";
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
            .ok_or_else(|| babel_types::Error::NotFound(format!("identity {identity_id}")))?
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
