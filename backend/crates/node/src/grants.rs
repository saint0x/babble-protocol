use super::*;

impl<P: JudgmentProvider> LocalNode<P> {
    pub(crate) fn commit_consent_event(&mut self, event: Event) -> Result<Event> {
        let signer = self
            .state
            .signing_identity_at(&event.actor, event.created_at)?;
        let mut batch = babel_store::PublicationBatch::new();
        batch.event(&event, &signer)?;
        self.attach_publication_receipt(
            &mut batch,
            babel_store::PublicationOutcome {
                object: None,
                edges: Vec::new(),
                event: event.id.clone(),
            },
        )?;
        self.store
            .commit_publication(batch)
            .map_err(crate::publication::publication_error)?;
        self.state.apply_event(event.clone()).map_err(|error| {
            babel_types::Error::Conflict(format!(
                "consent committed; in-memory apply failed, reopen node: {error}"
            ))
        })?;
        self.reconcile_surface_permissions()?;
        Ok(event)
    }

    /// Admission captures specific grants. Loss of that authority is terminal;
    /// another approval cannot silently replace it in a running session.
    pub fn reconcile_surface_permissions(&mut self) -> Result<()> {
        self.reconcile_surface_permissions_at(Timestamp::now())
    }

    fn reconcile_surface_permissions_at(&mut self, now: Timestamp) -> Result<()> {
        self.check_ready()?;
        self.reconcile_moderation_sessions().map_err(|_| {
            babel_types::Error::StorageUnavailable(
                "moderation runtime retirement unavailable; retry the same request key".into(),
            )
        })?;
        if !self.surface_sessions.values().any(|session| {
            session.lifecycle != SurfaceLifecycle::Evicted
                && session
                    .plan
                    .capability_decisions
                    .iter()
                    .any(|decision| decision.grant.is_some())
        }) {
            return Ok(());
        }
        let grants = project_grants(&self.store.list_events()?)?;
        let mut invalidated = Vec::new();
        for session in self.surface_sessions.values_mut() {
            if session.lifecycle == SurfaceLifecycle::Evicted {
                continue;
            }
            let invalid = session.plan.capability_decisions.iter().any(|decision| {
                let Some(admitted) = &decision.grant else {
                    return false;
                };
                !grants.get(&admitted.id).is_some_and(|(_, current)| {
                    current == admitted
                        && current.object_id == session.plan.object_id
                        && current.decision == GrantDecision::Approved
                        && current.revoked_at.is_none()
                        && current.expires_at.is_none_or(|expires| expires > now)
                })
            });
            if invalid {
                invalidated.push(session.id.clone());
                session.transition(
                    SurfaceLifecycle::Evicted,
                    "Surface permission revoked, expired or unavailable",
                )?;
            }
        }
        for id in invalidated {
            self.invalidate_social_session(id.as_str(), babel_capabilities::invocation::InvocationInvalidation::PolicyChanged)?;
        }
        Ok(())
    }

    pub fn capability_review_for_identity(
        &self,
        object: &ObjectId,
        identity: Option<&IdentityId>,
    ) -> Result<(Vec<CapabilityDecision>, Vec<CapabilityGrant>)> {
        self.check_ready()?;
        let object_record = self.require_object(object)?;
        let grants = match identity {
            Some(identity) => self.capability_grants_for_identity(object, identity)?,
            None => Vec::new(),
        };
        let decisions = self
            .capability_broker
            .evaluate_object(object_record, &grants);
        Ok((decisions, grants))
    }

    pub fn prepare_surface_for_identity(
        &self,
        object: &ObjectId,
        role: SurfaceRole,
        identity: Option<&IdentityId>,
    ) -> Result<SurfaceSessionPlan> {
        self.check_ready()?;
        self.require_moderation_execution(object)?;
        let grants = match identity {
            Some(identity) => self.capability_grants_for_identity(object, identity)?,
            None => Vec::new(),
        };
        SurfaceRuntime::new(self.capability_broker.clone()).prepare_surface(
            self.require_object(object)?,
            role,
            &grants,
        )
    }

    pub fn start_surface_session_for_identity(
        &mut self,
        object: &ObjectId,
        role: SurfaceRole,
        id: SurfaceSessionId,
        identity: &IdentityId,
    ) -> Result<SurfaceSession> {
        self.check_ready()?;
        let runtime = SurfaceRuntime::new(self.capability_broker.clone());
        let plan = self.prepare_surface_for_identity(object, role, Some(identity))?;
        let session = runtime.start_session(plan, Some(id))?;
        if self.surface_sessions.contains_key(&session.id) {
            return Err(babel_types::Error::Conflict(
                "Surface session already exists".into(),
            ));
        }
        self.surface_sessions
            .insert(session.id.clone(), session.clone());
        Ok(session)
    }

    /// Grant consent belongs to the signed event's actor, independently of the
    /// Object author. Ambiguous imported grant IDs fail closed.
    pub fn grants_belong_to(
        &self,
        object: &ObjectId,
        identity: &IdentityId,
        ids: &[String],
    ) -> Result<bool> {
        self.check_ready()?;
        if ids.is_empty() {
            return Ok(true);
        }
        let wanted: BTreeSet<_> = ids.iter().map(String::as_str).collect();
        if wanted.len() != ids.len() {
            return Ok(false);
        }
        let grants = project_grants(&self.store.list_events()?)?;
        Ok(wanted.into_iter().all(|id| {
            grants
                .get(&CapabilityGrantId::new_unchecked(id.to_owned()))
                .is_some_and(|(owner, grant)| owner == identity && &grant.object_id == object)
        }))
    }

    pub fn capability_grants_for_identity(
        &self,
        object: &ObjectId,
        identity: &IdentityId,
    ) -> Result<Vec<CapabilityGrant>> {
        self.check_ready()?;
        Ok(project_grants(&self.store.list_events()?)?
            .into_values()
            .filter(|(owner, grant)| owner == identity && &grant.object_id == object)
            .map(|(_, grant)| grant)
            .collect())
    }
}

pub(super) fn project_grants(
    events: &[Event],
) -> Result<BTreeMap<CapabilityGrantId, (IdentityId, CapabilityGrant)>> {
    let mut events: Vec<_> = events.iter().collect();
    events.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    let mut grants: BTreeMap<CapabilityGrantId, (IdentityId, CapabilityGrant)> = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for event in events {
        match event.kind {
            EventKind::CapabilityGranted => {
                let grant = event_grant(event)?;
                if event.target != EventTarget::Object(grant.object_id.clone()) {
                    continue;
                }
                if grants
                    .get(&grant.id)
                    .is_some_and(|(owner, _)| owner != &event.actor)
                {
                    grants.remove(&grant.id);
                    ambiguous.insert(grant.id.clone());
                }
                if !ambiguous.contains(&grant.id) {
                    // Re-importing an identical grant must not undo its revocation.
                    grants
                        .entry(grant.id.clone())
                        .or_insert((event.actor.clone(), grant));
                }
            }
            EventKind::CapabilityRevoked => {
                let id = event_grant_id(event)?;
                if let Some((owner, grant)) = grants.get_mut(&id)
                    && owner == &event.actor
                    && event.target == EventTarget::Object(grant.object_id.clone())
                {
                    grant.revoked_at = Some(event.created_at);
                }
            }
            _ => {}
        }
    }
    Ok(grants)
}

#[cfg(test)]
mod tests {
    use super::*;
    use babel_judgment_local::LocalProvider;
    use babel_object::{Surface, SurfaceTarget};

    #[test]
    fn expiry_retires_admitted_sessions_at_the_exact_boundary() {
        let root = std::env::temp_dir().join(format!(
            "babel-session-expiry-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let actor = node
            .create_identity(IdentityKind::Person, "expiry")
            .unwrap();
        let blob = node
            .put_media_blob("text/html", b"<!doctype html><p>Expiry</p>")
            .unwrap();
        let request = CapabilityRequest {
            id: "babel.storage.local".into(),
            version: 1,
            scope: serde_json::json!({"namespace":"self"}),
        };
        let object = node
            .publish_draft(
                &actor.id,
                ObjectDraft::text("expiry")
                    .unwrap()
                    .with_resource(blob.resource())
                    .unwrap()
                    .with_surface(Surface {
                        role: SurfaceRole::Feed,
                        target: SurfaceTarget::Web,
                        entry: blob.uri,
                        integrity: Some(blob.integrity),
                        bundle: None,
                    })
                    .unwrap()
                    .with_capability(request.clone())
                    .unwrap(),
            )
            .unwrap();
        let expires = Timestamp(Timestamp::now().0 + time::Duration::hours(1));
        node.grant_capability_draft(
            &actor.id,
            CapabilityGrantDraft {
                object_id: object.id.clone(),
                request,
                decision: GrantDecision::Approved,
                expires_at: Some(expires),
            },
        )
        .unwrap();
        let session = node
            .start_surface_session_for_identity(
                &object.id,
                SurfaceRole::Feed,
                SurfaceSessionId::from_material("expiry boundary"),
                &actor.id,
            )
            .unwrap();
        node.reconcile_surface_permissions_at(Timestamp(
            expires.0 - time::Duration::nanoseconds(1),
        ))
        .unwrap();
        assert_eq!(
            node.surface_session(&session.id).unwrap().lifecycle,
            SurfaceLifecycle::Prefetched
        );
        node.reconcile_surface_permissions_at(expires).unwrap();
        assert_eq!(
            node.surface_session(&session.id).unwrap().lifecycle,
            SurfaceLifecycle::Evicted
        );
        node.reconcile_surface_permissions_at(Timestamp(expires.0 - time::Duration::seconds(1)))
            .unwrap();
        assert_eq!(
            node.surface_session(&session.id).unwrap().lifecycle,
            SurfaceLifecycle::Evicted
        );
        drop(node);
        std::fs::remove_dir_all(root).unwrap();
    }
}
