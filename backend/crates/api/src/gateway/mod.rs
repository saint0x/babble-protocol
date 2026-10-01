mod config;
mod transport;
pub use config::GatewayConfig;
pub use transport::router;
#[cfg(test)]
mod tests;

use crate::{
    auth::{Auth, Principal, random_token},
    error::ApiError,
};
use babble_judgment::JudgmentProvider;
use babble_node::LocalNode;
use babble_object::SurfaceRole;
use babble_runtime::{SurfaceSession, SurfaceSessionId, SurfaceSessionPlan, VerifiedSurfaceMount};
use babble_store::VerifiedBundle;
use babble_types::{IdentityId, ObjectId};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};
use tokio::sync::Semaphore;

const MAX_MOUNTS: usize = 64;
const MAX_RETAINED_BYTES: u64 = 128 * 1024 * 1024;

pub(crate) struct Gateway {
    config: GatewayConfig,
    mounts: Mutex<BTreeMap<String, Arc<Mount>>>,
    deliveries: Arc<Semaphore>,
}

struct Mount {
    descriptor: VerifiedSurfaceMount,
    bundle: VerifiedBundle,
    principal: Principal,
    size: u64,
}

impl Mount {
    fn authorize<P: JudgmentProvider>(
        &self,
        auth: &Auth,
        node: &LocalNode<P>,
        allow_suspended: bool,
    ) -> Result<(), ApiError> {
        crate::auth::authorize_bundle(
            auth,
            node,
            &self.principal,
            &self.descriptor.session_id,
            &self.descriptor.object_id,
            allow_suspended,
        )?;
        let identity = IdentityId::new_unchecked(self.principal.identity_id.clone());
        let plan = node.prepare_verified_surface_for_identity(
            &self.descriptor.object_id,
            self.descriptor.role.clone(),
            Some(&identity),
            &self.bundle,
        )?;
        if plan.admission != babble_runtime::RuntimeAdmissionStatus::Ready {
            return Err(ApiError::forbidden());
        }
        Ok(())
    }
}

impl Gateway {
    pub(crate) fn new(config: GatewayConfig) -> Self {
        Self {
            config,
            mounts: Mutex::new(BTreeMap::new()),
            deliveries: Arc::new(Semaphore::new(16)),
        }
    }

    pub(crate) fn check_capacity(&self, bundle: &VerifiedBundle) -> Result<(), ApiError> {
        let mounts = self
            .mounts
            .lock()
            .map_err(|_| ApiError::unavailable("bundle gateway unavailable"))?;
        capacity(&mounts, bundle).map(|_| ())
    }

    pub(crate) fn allocate(
        &self,
        session: &SurfaceSession,
        bundle: VerifiedBundle,
        principal: &Principal,
    ) -> Result<VerifiedSurfaceMount, ApiError> {
        let proof = session
            .plan
            .bundle_verification
            .as_ref()
            .ok_or_else(|| ApiError::conflict("session lacks verified bundle admission"))?;
        if proof.policy_version != 1
            || proof.manifest_hash != *bundle.manifest_hash()
            || session.plan.object_id != *bundle.object_id()
            || session.plan.surface.role != *bundle.role()
        {
            return Err(ApiError::conflict("verified bundle does not match session"));
        }
        let mut mounts = self
            .mounts
            .lock()
            .map_err(|_| ApiError::unavailable("bundle gateway unavailable"))?;
        if mounts
            .values()
            .any(|mount| mount.descriptor.session_id == session.id)
        {
            return Err(ApiError::conflict("session already has a bundle mount"));
        }
        let size = capacity(&mounts, &bundle)?;
        // 192 bits fit in one DNS label, unlike a full 256-bit hex token.
        let token = random_token()?;
        let port = self.config.bind_addr.port();
        let suffix = if port == 80 {
            String::new()
        } else {
            format!(":{port}")
        };
        let host = format!("m-{}.localhost{suffix}", &token[..48]);
        if mounts.contains_key(&host) {
            return Err(ApiError::unavailable("bundle origin collision"));
        }
        let origin = format!("http://{host}");
        let descriptor = VerifiedSurfaceMount {
            version: 1,
            session_id: session.id.clone(),
            object_id: bundle.object_id().clone(),
            role: bundle.role().clone(),
            manifest_hash: bundle.manifest_hash().clone(),
            entry_url: format!("{origin}/{}", bundle.entry_path()),
            origin,
        };
        mounts.insert(
            host,
            Arc::new(Mount {
                descriptor: descriptor.clone(),
                bundle,
                principal: principal.clone(),
                size,
            }),
        );
        Ok(descriptor)
    }

    pub(crate) fn descriptor<P: JudgmentProvider>(
        &self,
        session: &SurfaceSessionId,
        auth: &Auth,
        node: &LocalNode<P>,
    ) -> Result<VerifiedSurfaceMount, ApiError> {
        let mount = self
            .mounts
            .lock()
            .map_err(|_| ApiError::unavailable("bundle gateway unavailable"))?
            .values()
            .find(|mount| mount.descriptor.session_id == *session)
            .cloned()
            .ok_or_else(|| {
                ApiError::conflict("bundle mount no longer exists; start a fresh session")
            })?;
        mount.authorize(auth, node, false)?;
        Ok(mount.descriptor.clone())
    }

    pub(crate) fn prune<P: JudgmentProvider>(
        &self,
        auth: &Auth,
        node: &LocalNode<P>,
    ) -> Result<(), ApiError> {
        let mut mounts = self
            .mounts
            .lock()
            .map_err(|_| ApiError::unavailable("bundle gateway unavailable"))?;
        mounts.retain(|_, mount| mount.authorize(auth, node, true).is_ok());
        Ok(())
    }
}

fn capacity(
    mounts: &BTreeMap<String, Arc<Mount>>,
    bundle: &VerifiedBundle,
) -> Result<u64, ApiError> {
    let size = bundle
        .files()
        .map(|file| file.descriptor().size_bytes)
        .sum::<u64>();
    if mounts.len() >= MAX_MOUNTS
        || mounts.values().map(|mount| mount.size).sum::<u64>() + size > MAX_RETAINED_BYTES
    {
        return Err(ApiError::unavailable(
            "bundle mount capacity exceeded; close an existing Surface",
        ));
    }
    Ok(size)
}

pub(crate) fn prepare<P: JudgmentProvider>(
    gateway: Option<&Arc<Gateway>>,
    node: &LocalNode<P>,
    object: &ObjectId,
    role: SurfaceRole,
    identity: Option<&IdentityId>,
) -> Result<SurfaceSessionPlan, ApiError> {
    let plan = node.prepare_surface_for_identity(object, role.clone(), identity)?;
    if gateway.is_none() || plan.surface.bundle.is_none() {
        return Ok(plan);
    }
    let receipt = node.verify_surface_bundle(object, role.clone())?;
    Ok(node.prepare_verified_surface_for_identity(object, role, identity, &receipt)?)
}
