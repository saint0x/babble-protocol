//! Account authority and local enforcement. Private receipts never become public events.
use crate::LocalNode;
pub use babble_graph::moderation::*;
use babble_judgment::JudgmentProvider;
use babble_runtime::SurfaceLifecycle;
use babble_types::{Error, IdentityId, ObjectId, Result};

impl<P: JudgmentProvider> LocalNode<P> {
    /// Trusted configuration only; no account or RPC operation exposes this setter.
    pub fn configure_moderators(&mut self, value: &str) -> Result<()> {
        self.moderators.clear();
        self.moderators = parse_reviewers(value)?;
        Ok(())
    }
    pub(crate) fn verify_moderation(&self) -> Result<()> {
        self.store.verify_moderation(
            |id, at| self.state.signing_identity_at(id, at),
            |id| Ok(self.require_object(id)?.author.clone()),
        )
    }
    pub fn moderation_access(&self, actor: &IdentityId) -> Result<ModerationAccess> {
        self.check_ready()?;
        self.state.signing_identity(actor)?;
        Ok(ModerationAccess {
            actor_id: actor.clone(),
            can_review: self.moderators.contains(actor),
            policy_version: POLICY.into(),
            reasons: REASONS.to_vec(),
        })
    }
    pub fn moderation_case(&self, actor: &IdentityId, id: &str) -> Result<ModerationCase> {
        let access = self.moderation_access(actor)?;
        self.store
            .moderation_case(id)?
            .ok_or_else(|| Error::NotFound("moderation case".into()))?
            .view(actor, access.can_review)
    }
    pub fn moderation_list(
        &self,
        actor: &IdentityId,
        scope: ModerationScope,
        before: Option<u64>,
        limit: usize,
    ) -> Result<ModerationPage> {
        let access = self.moderation_access(actor)?;
        if matches!(scope, ModerationScope::Queue) && !access.can_review {
            return Err(Error::NotFound("moderation queue".into()));
        }
        let mut page = self.store.moderation_list(actor, scope, before, limit)?;
        page.items = page
            .items
            .into_iter()
            .map(|c| c.view(actor, access.can_review))
            .collect::<Result<_>>()?;
        Ok(page)
    }
    pub fn moderation_report(
        &mut self,
        actor: &IdentityId,
        request: ReportRequest,
    ) -> Result<ModerationCase> {
        self.moderation_write(actor, ModerationIntent::Report(request))
    }
    pub fn moderation_decide(
        &mut self,
        actor: &IdentityId,
        case_id: String,
        request: DecisionRequest,
    ) -> Result<ModerationCase> {
        self.moderation_write(actor, ModerationIntent::Decision { case_id, request })
    }
    pub fn moderation_appeal(
        &mut self,
        actor: &IdentityId,
        case_id: String,
        request: AppealRequest,
    ) -> Result<ModerationCase> {
        self.moderation_write(actor, ModerationIntent::Appeal { case_id, request })
    }
    fn moderation_write(
        &mut self,
        actor: &IdentityId,
        intent: ModerationIntent,
    ) -> Result<ModerationCase> {
        let access = self.moderation_access(actor)?;
        intent.validate()?;
        let object = match &intent {
            ModerationIntent::Report(r) => r.object_id.clone(),
            ModerationIntent::Decision { case_id, .. }
            | ModerationIntent::Appeal { case_id, .. } => {
                self.moderation_case(actor, case_id)?.object_id
            }
        };
        if let ModerationIntent::Decision { request, .. } = &intent {
            if !access.can_review {
                return Err(Error::Conflict("authorized reviewer required".into()));
            }
            for id in &request.source_signals {
                if !self
                    .store
                    .get_object_judgment_input(id)?
                    .is_some_and(|input| input.object_id == object)
                {
                    return Err(Error::Canonical(
                        "signal must be an existing Judgment about this Object".into(),
                    ));
                }
            }
        }
        let subject = self.require_object(&object)?.author.clone();
        let key = self.local_keypair(actor)?;
        let result = self.store.commit_moderation(
            actor,
            &self.moderators,
            &intent,
            &subject,
            |id, at| self.state.signing_identity_at(id, at),
            |payload| ModerationReceipt::sign(payload, key),
        )?;
        // Admission reads committed policy even if runtime retirement needs a retry.
        self.reconcile_moderation_sessions().map_err(|_| {
            Error::StorageUnavailable(
                "moderation committed; runtime retirement pending, retry the same request key"
                    .into(),
            )
        })?;
        result.view(actor, access.can_review)
    }
    pub fn require_moderation_execution(&self, object: &ObjectId) -> Result<()> {
        self.check_ready()?;
        if self.store.moderation_restricted(object)? {
            return Err(Error::Conflict(
                "Object execution is restricted on this node".into(),
            ));
        }
        Ok(())
    }
    pub(crate) fn reconcile_moderation_sessions(&mut self) -> Result<()> {
        let (_, restricted) = self.store.moderation_restrictions()?;
        let mut ids = Vec::new();
        for session in self.surface_sessions.values_mut() {
            if session.lifecycle != SurfaceLifecycle::Evicted
                && restricted.contains(&session.plan.object_id)
            {
                session.transition(
                    SurfaceLifecycle::Evicted,
                    "Object execution unavailable on this node",
                )?;
                ids.push(session.id.clone());
            }
        }
        for id in ids {
            self.invalidate_social_session(
                id.as_str(),
                babble_capabilities::invocation::InvocationInvalidation::PolicyChanged,
            )?;
        }
        Ok(())
    }
}
