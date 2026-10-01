use crate::LocalNode;
use babel_judgment::JudgmentProvider;
use babel_object::SurfaceRole;
use babel_runtime::{SurfaceRuntime, SurfaceSession, SurfaceSessionId, SurfaceSessionPlan};
use babel_store::VerifiedBundle;
use babel_types::{Error, IdentityId, ObjectId, Result};

impl<P: JudgmentProvider> LocalNode<P> {
    pub fn prepare_verified_surface_for_identity(
        &self,
        object_id: &ObjectId,
        role: SurfaceRole,
        identity: Option<&IdentityId>,
        receipt: &VerifiedBundle,
    ) -> Result<SurfaceSessionPlan> {
        self.check_ready()?;
        self.require_moderation_execution(object_id)?;
        let object = self.require_object(object_id)?;
        let author = self
            .state
            .signing_identity_at(&object.author, object.created_at)?;
        object.verify(&author)?;
        let grants = match identity {
            Some(identity) => {
                self.state.signing_identity(identity)?;
                self.capability_grants_for_identity(object_id, identity)?
            }
            None => Vec::new(),
        };
        SurfaceRuntime::new(self.capability_broker.clone())
            .prepare_verified_surface(object, role, &grants, receipt)
    }

    pub fn start_verified_surface_session_for_identity(
        &mut self,
        object_id: &ObjectId,
        role: SurfaceRole,
        id: SurfaceSessionId,
        identity: &IdentityId,
        receipt: &VerifiedBundle,
    ) -> Result<SurfaceSession> {
        id.validate()?;
        let plan =
            self.prepare_verified_surface_for_identity(object_id, role, Some(identity), receipt)?;
        let session =
            SurfaceRuntime::new(self.capability_broker.clone()).start_session(plan, Some(id))?;
        if self.surface_sessions.contains_key(&session.id) {
            return Err(Error::Conflict("Surface session already exists".into()));
        }
        self.surface_sessions
            .insert(session.id.clone(), session.clone());
        Ok(session)
    }

    /// Resolve the authoritative historical signing key before verifying the
    /// complete materialized bundle. Verification is not execution admission.
    pub fn verify_surface_bundle(
        &self,
        object_id: &ObjectId,
        role: SurfaceRole,
    ) -> Result<VerifiedBundle> {
        self.check_ready()?;
        let object = self
            .state
            .object(object_id)
            .ok_or_else(|| Error::NotFound(format!("bundle Object {object_id}")))?;
        let author = self
            .state
            .signing_identity_at(&object.author, object.created_at)?;
        self.store.verify_surface_bundle(object, &author, role)
    }
}
