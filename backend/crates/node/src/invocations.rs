//! Durable one-use social effects. The API authenticates the login and document
//! at ingress and again under its node lock. Native callers are trusted hosts;
//! an InvocationContext parsed directly from an untrusted request is not authority.
use crate::{LocalNode, SocialMediaAttachment};
use babble_authoring::{EdgeDraft, ObjectDraft};
use babble_capabilities::invocation::{
    InvocationAction, InvocationContext, InvocationExecutor, InvocationId, InvocationIntent,
    InvocationInvalidation, InvocationOrigin, InvocationOutcome, InvocationRecord, InvocationState,
    MAX_INVOCATION_TTL_MS, invocation_key, invocation_key_for_login, is_social_invocation,
    is_one_use_invocation,
};
use babble_capabilities::{CapabilityDefinition, PermissionMode};
use babble_graph::{Edge, EdgeOrigin, Relation};
use babble_judgment::JudgmentProvider;
use babble_object::Object;
use babble_runtime::{SurfaceLifecycle, SurfaceSessionId};
use babble_state::EventKind;
use babble_store::{PublicationBatch, PublicationReceipt, PublicationRequest};
use babble_types::{Canonical, Error, Hash, IdentityId, ObjectId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[cfg(test)]
pub(crate) mod tests;

mod browser;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SocialInvocationPayload {
    pub target_object_id: ObjectId,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub media: Option<SocialMediaAttachment>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SocialInvocationResult {
    pub invocation: InvocationRecord,
    pub object: Option<Object>,
    pub edge: Edge,
    pub receipt: PublicationReceipt,
}

impl<P: JudgmentProvider> LocalNode<P> {
    pub fn invocation_epoch(&self) -> Hash {
        self.invocation_epoch.clone()
    }

    pub fn invocation_policy_revision(&self) -> Result<Hash> {
        let definitions: Vec<_> = self
            .capability_definitions()
            .into_iter()
            .filter(|d| is_one_use_invocation(d.id.as_str()))
            .collect();
        ("babble.social.invocation.policy.v1", definitions).canonical_hash()
    }

    /// Resolve defaults once here. A retry compares normalized user intent and
    /// context but always retains the original server creation time and deadline.
    pub fn prepare_social_invocation(
        &mut self,
        context: InvocationContext,
        request_key: &str,
        method: &str,
        payload: SocialInvocationPayload,
        deadline: Timestamp,
    ) -> Result<InvocationRecord> {
        self.reconcile_surface_permissions()?;
        let definition = self.social_definition(method)?;
        let payload = normalize_payload(method, payload)?;
        let key = invocation_key(&context, request_key)?;
        let payload_value = serde_json::to_value(&payload).map_err(encoding)?;
        if let Some(existing) = self.store.invocation(&key)? {
            let intent = existing.intent();
            if intent.context != context
                || intent.method != method
                || intent.payload != payload_value
            {
                return Err(conflict(
                    "invocation key reused with changed intent or context",
                ));
            }
            return self.refresh_invocation(existing);
        }
        self.check_social_context(&context)?;
        let scope = self.social_scope(&context, &definition, &payload)?;
        self.validate_social_payload(&payload)?;
        let now = Timestamp::now();
        let budget = definition
            .quota
            .max_call_ms
            .min(MAX_INVOCATION_TTL_MS as u64);
        let deadline = deadline.min(Timestamp(now.0 + std::time::Duration::from_millis(budget)));
        let intent = InvocationIntent {
            request_key: request_key.into(),
            context,
            method: method.into(),
            method_version: 2,
            capability: definition.id,
            capability_version: definition.version,
            scope,
            executor: InvocationExecutor::LocalPublication,
            payload: payload_value,
            created_at: now,
            deadline,
        };
        intent.validate()?;
        self.store.check_social_invocation_quota(&intent, now)?;
        self.store
            .prepare_invocation(intent, now)
            .map_err(publication_error)
    }

    /// Recover a committed HostAction acknowledgement for the original login.
    /// This history read confers no execution authority and never refreshes a phase.
    pub fn recover_social_invocation(
        &self,
        actor: &IdentityId,
        login_id: &str,
        object_id: &ObjectId,
        request_key: &str,
        method: &str,
        payload: SocialInvocationPayload,
    ) -> Result<Option<SocialInvocationResult>> {
        self.check_ready()?;
        object_id.validate()?;
        let capability = method
            .strip_suffix("")
            .filter(|id| is_social_invocation(id))
            .ok_or_else(|| conflict("unsupported social invocation method"))?;
        let payload =
            serde_json::to_value(normalize_payload(method, payload)?).map_err(encoding)?;
        let key = invocation_key_for_login(actor, login_id, request_key)?;
        let Some(record) = self.store.invocation(&key)? else {
            return Ok(None);
        };
        if !matches!(
            record.state(),
            InvocationState::Completed {
                outcome: InvocationOutcome::Publication { .. }
            }
        ) {
            return Ok(None);
        }
        let intent = record.intent();
        if !matches!(intent.context.origin, InvocationOrigin::HostAction { .. }) {
            return Ok(None);
        }
        if &intent.context.actor != actor
            || intent.context.login_id != login_id
            || &intent.context.object_id != object_id
            || intent.request_key != request_key
            || intent.method != method
            || intent.method_version != 2
            || intent.capability.as_str() != capability
            || intent.capability_version != 1
            || intent.executor != InvocationExecutor::LocalPublication
            || intent.payload != payload
        {
            return Err(conflict("invocation recovery intent mismatch"));
        }
        self.social_invocation_result(record).map(Some)
    }

    /// Private native lookup. HTTP must authenticate before exposing any record.
    pub fn invocation_by_id(&mut self, id: &InvocationId) -> Result<Option<InvocationRecord>> {
        self.check_ready()?;
        let record = self
            .store
            .list_invocations()?
            .into_iter()
            .find(|r| r.id() == id && is_one_use_invocation(r.intent().capability.as_str()));
        record.map(|r| self.refresh_invocation(r)).transpose()
    }

    pub fn status_social_invocation(
        &mut self,
        context: &InvocationContext,
        request_key: &str,
    ) -> Result<Option<InvocationRecord>> {
        self.check_ready()?;
        let record = self
            .store
            .invocation(&invocation_key(context, request_key)?)?;
        record
            .map(|record| {
                if !is_social_invocation(record.intent().capability.as_str()) {
                    return Err(conflict("not a social invocation"));
                }
                require_context(&record, context)?;
                self.refresh_invocation(record)
            })
            .transpose()
    }

    pub fn decide_social_invocation(
        &mut self,
        context: &InvocationContext,
        request_key: &str,
        challenge: &InvocationId,
        approve: bool,
    ) -> Result<InvocationRecord> {
        self.reconcile_surface_permissions()?;
        let record = self.require_social_invocation(context, request_key, challenge)?;
        if record.state().is_terminal() || record.state() == &InvocationState::Approved {
            return Ok(record);
        }
        self.check_social_context(context)?;
        self.store
            .transition_invocation(
                &record,
                if approve {
                    InvocationAction::Approve
                } else {
                    InvocationAction::Deny
                },
                context,
                Timestamp::now(),
            )
            .map_err(publication_error)
    }

    pub fn cancel_social_invocation(
        &mut self,
        context: &InvocationContext,
        request_key: &str,
        challenge: &InvocationId,
    ) -> Result<InvocationRecord> {
        let record = self.require_social_invocation(context, request_key, challenge)?;
        if record.state().is_terminal() {
            return Ok(record);
        }
        self.store
            .transition_invocation(&record, InvocationAction::Cancel, context, Timestamp::now())
            .map_err(publication_error)
    }

    /// No caller-supplied payload or grants are accepted at consumption.
    pub fn execute_social_invocation(
        &mut self,
        context: &InvocationContext,
        request_key: &str,
        challenge: &InvocationId,
    ) -> Result<SocialInvocationResult> {
        self.reconcile_surface_permissions()?;
        let record = self.require_social_invocation(context, request_key, challenge)?;
        if matches!(record.state(), InvocationState::Completed { .. }) {
            return self.social_invocation_result(record);
        }
        if record.state() != &InvocationState::Approved {
            return Err(conflict("social invocation is not approved"));
        }
        self.check_social_execution(&record)?;
        if self.executing_invocation.is_some() || self.publication_request.is_some() {
            return Err(conflict("nested invocation publication"));
        }
        let payload: SocialInvocationPayload =
            serde_json::from_value(record.intent().payload.clone()).map_err(encoding)?;
        let request = PublicationRequest {
            id: ("babble.invocation.publication.v1", record.id()).canonical_hash()?,
            fingerprint: record.intent().fingerprint()?,
            author: context.actor.clone(),
        };
        self.executing_invocation = Some(record.clone());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.with_publication_request(request, |node| {
                node.publish_social_invocation(&record, payload)
            })
        }));
        self.executing_invocation = None;
        match result {
            Ok(result) => result?,
            Err(panic) => std::panic::resume_unwind(panic),
        }
        let completed = self
            .store
            .invocation(&record.intent().key()?)?
            .ok_or_else(|| conflict("committed invocation missing"))?;
        self.social_invocation_result(completed)
    }

    pub fn invalidate_social_invocations(
        &mut self,
        context: &InvocationContext,
        reason: InvocationInvalidation,
    ) -> Result<usize> {
        context.validate()?;
        let records = self.store.list_invocations()?;
        let mut count = 0;
        for record in records {
            if &record.intent().context == context
                && (unconsumed(&record) || matches!(record.state(), InvocationState::Running { .. }))
                && is_one_use_invocation(record.intent().capability.as_str())
            {
                self.invalidate_invocation(&record, reason.clone())?;
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn invalidate_social_session(
        &mut self,
        session_id: &str,
        reason: InvocationInvalidation,
    ) -> Result<usize> {
        let result = self.invalidate_social_origins(
            |context| {
                matches!(&context.origin,
            InvocationOrigin::Surface { session_id: id, .. } if id == session_id)
            },
            reason,
        );
        if result.is_err() {
            // A failed durable invalidation must not allow a later resume to
            // restore approval. Restart will invalidate the remaining records.
            if let Ok(id) = SurfaceSessionId::new(session_id)
                && let Some(session) = self.surface_sessions.get_mut(&id)
            {
                session.transition(SurfaceLifecycle::Evicted, "invocation invalidation failed")?;
            }
        }
        result
    }

    pub fn invalidate_social_document(
        &mut self,
        login_id: &str,
        document_id: &str,
        reason: InvocationInvalidation,
    ) -> Result<usize> {
        self.invalidate_social_origins(
            |context| {
                context.login_id == login_id
                    && match &context.origin {
                        InvocationOrigin::Surface {
                            document_id: id, ..
                        }
                        | InvocationOrigin::HostAction { document_id: id } => id == document_id,
                    }
            },
            reason,
        )
    }

    fn invalidate_social_origins(
        &mut self,
        matches: impl Fn(&InvocationContext) -> bool,
        reason: InvocationInvalidation,
    ) -> Result<usize> {
        let mut count = 0;
        for record in self.store.list_invocations()? {
            if is_one_use_invocation(record.intent().capability.as_str())
                && (unconsumed(&record) || matches!(record.state(), InvocationState::Running { .. }))
                && matches(&record.intent().context)
            {
                self.invalidate_invocation(&record, reason.clone())?;
                count += 1;
            }
        }
        Ok(count)
    }

    pub(crate) fn invalidate_restarted_invocations(&mut self) -> Result<()> {
        for record in self.store.list_invocations()? {
            if is_one_use_invocation(record.intent().capability.as_str())
                && (unconsumed(&record) || matches!(record.state(), InvocationState::Running { .. })) {
                self.invalidate_invocation(&record, InvocationInvalidation::Restart)?;
            }
        }
        Ok(())
    }

    fn invalidate_invocation(
        &self,
        record: &InvocationRecord,
        reason: InvocationInvalidation,
    ) -> Result<InvocationRecord> {
        self.store
            .transition_invocation(
                record,
                match record.state() {
                    InvocationState::Running { dispatch_id } => InvocationAction::MarkUnknown { dispatch_id: dispatch_id.clone() },
                    _ => InvocationAction::Invalidate { reason },
                },
                &record.intent().context,
                Timestamp::now().max(record.updated_at()),
            )
            .map_err(publication_error)
    }

    fn refresh_invocation(&self, record: InvocationRecord) -> Result<InvocationRecord> {
        if matches!(record.state(), InvocationState::Running { .. })
            && (Timestamp::now() >= record.intent().deadline
                || self.check_social_context(&record.intent().context).is_err()) {
            return self.invalidate_invocation(&record, InvocationInvalidation::ContextLost);
        }
        if !unconsumed(&record) {
            return Ok(record);
        }
        let context = &record.intent().context;
        if context.context_epoch != self.invocation_epoch {
            return self.invalidate_invocation(&record, InvocationInvalidation::Restart);
        }
        if context.policy_revision != self.invocation_policy_revision()? {
            return self.invalidate_invocation(&record, InvocationInvalidation::PolicyChanged);
        }
        if self.check_social_context(context).is_err() {
            return self.invalidate_invocation(&record, InvocationInvalidation::ContextLost);
        }
        let now = Timestamp::now();
        if now >= record.intent().deadline {
            return self
                .store
                .transition_invocation(&record, InvocationAction::Expire, context, now)
                .map_err(publication_error);
        }
        Ok(record)
    }

    fn require_social_invocation(
        &mut self,
        context: &InvocationContext,
        request_key: &str,
        challenge: &InvocationId,
    ) -> Result<InvocationRecord> {
        let record = self
            .status_social_invocation(context, request_key)?
            .ok_or_else(|| Error::NotFound("social invocation".into()))?;
        if record.id() != challenge || !is_social_invocation(record.intent().capability.as_str()) {
            return Err(conflict("invocation challenge mismatch"));
        }
        Ok(record)
    }

    fn social_definition(&self, method: &str) -> Result<CapabilityDefinition> {
        let capability = method
            .strip_suffix("")
            .filter(|id| is_social_invocation(id))
            .ok_or_else(|| conflict("unsupported social invocation method"))?;
        self.capability_definitions()
            .into_iter()
            .find(|d| {
                d.id.as_str() == capability
                    && d.version == 1
                    && d.permission == PermissionMode::AskEachTime
            })
            .ok_or_else(|| conflict("social invocation capability unavailable"))
    }

    fn check_social_context(&self, context: &InvocationContext) -> Result<()> {
        self.check_ready()?;
        context.validate()?;
        if context.context_epoch != self.invocation_epoch
            || context.policy_revision != self.invocation_policy_revision()?
        {
            return Err(conflict("invocation epoch or policy mismatch"));
        }
        self.local_identity(&context.actor)?;
        self.local_keypair(&context.actor)?;
        self.require_moderation_execution(&context.object_id)?;
        let object = self.require_object(&context.object_id)?;
        if object.canonical_hash()? != context.object_version {
            return Err(conflict("invocation Object version mismatch"));
        }
        if let InvocationOrigin::Surface {
            session_id,
            role,
            entry,
            resource_digest,
            ..
        } = &context.origin
        {
            let session = self.surface_session(&SurfaceSessionId::new(session_id)?)?;
            let actual_role = serde_json::to_value(&session.plan.surface.role).map_err(encoding)?;
            let digest = session
                .plan
                .bundle_verification
                .as_ref()
                .map(|b| &b.manifest_hash)
                .or(session.plan.surface.integrity.as_ref());
            if session.plan.object_id != context.object_id
                || session.lifecycle != SurfaceLifecycle::Active
                || actual_role.as_str().map(str::to_ascii_lowercase).as_ref() != Some(role)
                || session.plan.surface.entry != *entry
                || digest != Some(resource_digest)
            {
                return Err(conflict("invocation requires its active admitted Surface"));
            }
            if session.plan.capability_decisions.iter().any(|decision| {
                decision.grant.as_ref().is_some_and(|grant| {
                    grant.revoked_at.is_some()
                        || grant.expires_at.is_some_and(|at| at <= Timestamp::now())
                })
            }) {
                return Err(conflict("invocation Surface admission authority expired"));
            }
        }
        Ok(())
    }

    fn social_scope(
        &self,
        context: &InvocationContext,
        definition: &CapabilityDefinition,
        payload: &SocialInvocationPayload,
    ) -> Result<Value> {
        let object = self.require_object(&context.object_id)?;
        self.require_object(&payload.target_object_id)?;
        let declared = object.capabilities.iter().any(|request| {
            request.id == definition.id.as_str()
                && request.version == definition.version
                && match request.scope.get("object_id").and_then(Value::as_str) {
                    Some(target) => target == payload.target_object_id.as_str(),
                    None => {
                        request.scope == json!({}) && payload.target_object_id == context.object_id
                    }
                }
        });
        if !declared {
            return Err(conflict(
                "social invocation target is outside declared scope",
            ));
        }
        Ok(json!({"object_id": payload.target_object_id}))
    }

    fn validate_social_payload(&self, payload: &SocialInvocationPayload) -> Result<()> {
        self.require_object(&payload.target_object_id)?;
        if payload.text.is_some() || payload.media.is_some() {
            social_draft(payload)?.validate()?;
        }
        if let Some(media) = &payload.media {
            for resource in &media.resources {
                if resource.size_bytes == 0
                    || resource.size_bytes > crate::social::MAX_ATTACHMENT_BLOB_BYTES as u64
                {
                    return Err(conflict("social attachment blob must be 1-8388608 bytes"));
                }
                let bytes = self
                    .store
                    .get_blob_bounded(&resource.integrity, resource.size_bytes as usize)
                    .map_err(|error| conflict(&error.to_string()))?
                    .ok_or_else(|| Error::NotFound(format!("media blob {}", resource.integrity)))?;
                if bytes.len() as u64 != resource.size_bytes {
                    return Err(conflict("social attachment blob size mismatch"));
                }
            }
        }
        Ok(())
    }

    fn check_social_execution(&self, record: &InvocationRecord) -> Result<()> {
        self.check_social_context(&record.intent().context)?;
        if Timestamp::now() >= record.intent().deadline {
            return Err(conflict("invocation deadline expired"));
        }
        let definition = self.social_definition(&record.intent().method)?;
        let payload: SocialInvocationPayload =
            serde_json::from_value(record.intent().payload.clone()).map_err(encoding)?;
        if record.intent().scope
            != self.social_scope(&record.intent().context, &definition, &payload)?
            || record.intent().capability != definition.id
            || record.intent().method_version != 2
            || record.intent().capability_version != definition.version
            || record.intent().executor != InvocationExecutor::LocalPublication
        {
            return Err(conflict("invocation capability scope mismatch"));
        }
        self.validate_social_payload(&payload)?;
        self.store
            .check_social_invocation_quota(record.intent(), Timestamp::now())
    }

    pub(crate) fn attach_invocation_consumption(&self, batch: &mut PublicationBatch) -> Result<()> {
        if let Some(record) = &self.executing_invocation {
            self.check_social_execution(record)?;
            let request = self
                .publication_request
                .as_ref()
                .ok_or_else(|| conflict("invocation receipt missing"))?;
            batch.invocation_transition(
                record,
                InvocationAction::CompletePublication {
                    receipt: request.id.clone(),
                },
                &record.intent().context,
                Timestamp::now(),
            )?;
        }
        Ok(())
    }

    fn publish_social_invocation(
        &mut self,
        record: &InvocationRecord,
        payload: SocialInvocationPayload,
    ) -> Result<()> {
        let context = &record.intent().context;
        if record.intent().capability.as_str() != "babble.social.unfollow" {
            self.require_object_interaction(&context.actor, &payload.target_object_id)?;
            self.require_object_interaction(&context.actor, &context.object_id)?;
        }
        match record.intent().capability.as_str() {
            "babble.social.follow" | "babble.social.unfollow" => {
                let unfollow = record.intent().capability.as_str() == "babble.social.unfollow";
                let mut draft = EdgeDraft::new(
                    context.object_id.clone(),
                    payload.target_object_id,
                    if unfollow {
                        Relation::Custom("unfollows".into())
                    } else {
                        Relation::Follows
                    },
                    EdgeOrigin::HumanAssertion,
                )?;
                if unfollow {
                    draft = draft.with_metadata(BTreeMap::from([(
                        "removes_relation".into(),
                        json!("follows"),
                    )]))?;
                }
                self.publish_edge_draft(&context.actor, draft)?;
            }
            "babble.social.share" | "babble.social.reply" => {
                let draft = social_draft(&payload)?;
                self.publish_related_draft(
                    &context.actor,
                    draft,
                    EventKind::ObjectPublished,
                    vec![(
                        payload.target_object_id,
                        if record.intent().capability.as_str() == "babble.social.share" {
                            Relation::Quotes
                        } else {
                            Relation::ReplyTo
                        },
                        EdgeOrigin::HumanAssertion,
                    )],
                )?;
            }
            _ => return Err(conflict("unsupported social invocation")),
        }
        Ok(())
    }

    pub fn social_invocation_result(
        &self,
        invocation: InvocationRecord,
    ) -> Result<SocialInvocationResult> {
        self.check_ready()?;
        if self.store.invocation(&invocation.intent().key()?)?.as_ref() != Some(&invocation) {
            return Err(conflict(
                "invocation result is not the durable current record",
            ));
        }
        let InvocationState::Completed {
            outcome: InvocationOutcome::Publication { receipt },
        } = invocation.state()
        else {
            return Err(conflict("invocation has no completed publication"));
        };
        let receipt = self
            .store
            .publication_receipt(receipt)?
            .ok_or_else(|| conflict("invocation receipt missing"))?;
        if receipt.outcome.edges.len() != 1
            || receipt.request.author != invocation.intent().context.actor
            || receipt.request.fingerprint != invocation.intent().fingerprint()?
        {
            return Err(conflict("invalid social invocation receipt"));
        }
        let edge = self
            .store
            .get_edge(&receipt.outcome.edges[0])?
            .ok_or_else(|| conflict("invocation edge missing"))?;
        let object = receipt
            .outcome
            .object
            .as_ref()
            .map(|id| {
                self.store
                    .get_object(id)?
                    .ok_or_else(|| conflict("invocation Object missing"))
            })
            .transpose()?;
        Ok(SocialInvocationResult {
            invocation,
            object,
            edge,
            receipt,
        })
    }
}

fn normalize_payload(
    method: &str,
    mut payload: SocialInvocationPayload,
) -> Result<SocialInvocationPayload> {
    payload.target_object_id.validate()?;
    if matches!(
        method,
        "babble.social.follow" | "babble.social.unfollow"
    ) {
        if payload.text.is_some() || payload.media.is_some() {
            return Err(conflict(
                "relationship invocation cannot contain text or media",
            ));
        }
    } else {
        payload.text = Some(payload.text.unwrap_or_default().trim().to_string());
        if let Some(media) = payload.media.as_mut() {
            media.title = media.title.trim().to_string();
        }
        social_draft(&payload)?.validate()?;
    }
    Ok(payload)
}

fn social_draft(payload: &SocialInvocationPayload) -> Result<ObjectDraft> {
    let text = payload.text.as_deref().unwrap_or_default();
    match &payload.media {
        Some(media) => ObjectDraft::media(&media.title, Some(text.into()), media.resources.clone()),
        None => ObjectDraft::text(text),
    }
}

fn require_context(record: &InvocationRecord, context: &InvocationContext) -> Result<()> {
    if &record.intent().context != context {
        return Err(conflict("invocation context mismatch"));
    }
    Ok(())
}
fn unconsumed(record: &InvocationRecord) -> bool {
    matches!(
        record.state(),
        InvocationState::Pending | InvocationState::Approved
    )
}
fn conflict(message: &str) -> Error {
    Error::Conflict(message.into())
}
fn encoding(error: serde_json::Error) -> Error {
    Error::Canonical(error.to_string())
}
fn publication_error(error: babble_store::PublicationError) -> Error {
    Error::Conflict(error.to_string())
}
