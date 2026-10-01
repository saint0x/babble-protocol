use super::*;

#[test]
fn safety_invocations_check_live_blocks_after_approval_and_allow_unfollow() {
    for action in ["follow", "reply", "share", "unfollow"] {
        for reverse in [false, true] {
            let mut f = Fixture::new();
            let other = f.node.create_identity(IdentityKind::Person, "other").unwrap();
            let draft = ObjectDraft::text("owned controller").unwrap().with_capability(CapabilityRequest {
                id: format!("babble.social.{action}"), version: 1, scope: json!({"object_id":f.target.id})
            }).unwrap();
            f.source = f.node.publish_draft(&other.id, draft).unwrap();
            f.context = host_context(&f.node, &other.id, &f.source);
            if action == "unfollow" {
                f.node.publish_edge(&other.id, f.source.id.clone(), f.target.id.clone(),
                    Relation::Follows, EdgeOrigin::HumanAssertion).unwrap();
            }
            let record = f.prepare(action, action);
            f.approve(&record);
            let (owner, target) = if reverse { (f.actor.id.clone(), other.id.clone()) } else { (other.id.clone(), f.actor.id.clone()) };
            f.node.set_safety(&owner, &target, true, false, 0, "block").unwrap();
            let before = f.counts();
            if action == "unfollow" {
                f.execute(&record).unwrap();
            } else {
                assert!(f.execute(&record).is_err(), "{action}, reverse={reverse}");
                assert_eq!(before, f.counts());
                f.node.set_safety(&owner, &target, false, true, 1, "unblock").unwrap();
                f.execute(&record).unwrap();
                f.node.set_safety(&owner, &target, true, true, 2, "reblock").unwrap();
                let completed = f.counts();
                f.execute(&record).unwrap();
                assert_eq!(completed, f.counts());
            }
        }
    }
}

#[test]
fn safety_unfollow_can_withdraw_own_follow_from_an_application_controller() {
    let mut f = Fixture::new();
    let viewer = f.node.create_identity(IdentityKind::Person, "viewer").unwrap();
    f.context.actor = viewer.id.clone();
    f.node.publish_edge(&viewer.id, f.source.id.clone(), f.target.id.clone(),
        Relation::Follows, EdgeOrigin::HumanAssertion).unwrap();
    let record = f.prepare("withdraw-from-controller", "unfollow");
    f.approve(&record);
    f.node.set_safety(&viewer.id, &f.actor.id, true, false, 0, "block").unwrap();
    f.execute(&record).unwrap();
}
use babble_capabilities::GrantDecision;
use babble_identity::{Identity, IdentityKind};
use babble_judgment_local::LocalProvider;
use babble_object::{CapabilityRequest, Surface, SurfaceRole, SurfaceTarget};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

pub(crate) fn host_context<P: JudgmentProvider>(
    node: &LocalNode<P>,
    actor: &babble_types::IdentityId,
    object: &Object,
) -> InvocationContext {
    InvocationContext {
        actor: actor.clone(),
        login_id: "test-login".into(),
        object_id: object.id.clone(),
        object_version: object.canonical_hash().unwrap(),
        origin: InvocationOrigin::HostAction {
            document_id: "test-document".into(),
        },
        policy_revision: node.invocation_policy_revision().unwrap(),
        context_epoch: node.invocation_epoch(),
    }
}

pub(crate) fn invoke<P: JudgmentProvider>(
    node: &mut LocalNode<P>,
    actor: &babble_types::IdentityId,
    source: &Object,
    target: &ObjectId,
    action: &str,
    text: Option<&str>,
    media: Option<&SocialMediaAttachment>,
) -> Result<SocialInvocationResult> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let context = host_context(node, actor, source);
    let key = format!("test-{}", NEXT.fetch_add(1, Ordering::Relaxed));
    let record = node.prepare_social_invocation(
        context.clone(),
        &key,
        &format!("babble.social.{action}"),
        SocialInvocationPayload {
            target_object_id: target.clone(),
            text: text.map(str::to_string),
            media: media.cloned(),
        },
        Timestamp(Timestamp::now().0 + time::Duration::seconds(60)),
    )?;
    node.decide_social_invocation(&context, &key, record.id(), true)?;
    node.execute_social_invocation(&context, &key, record.id())
}

struct Fixture {
    root: PathBuf,
    node: LocalNode<LocalProvider>,
    actor: Identity,
    source: Object,
    target: Object,
    context: InvocationContext,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babble-social-invocations-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let actor = node
            .create_identity(IdentityKind::Person, "invocation author")
            .unwrap();
        let target = node.publish_text(&actor.id, "target").unwrap();
        let blob = node
            .put_media_blob("text/html", b"<html>surface</html>")
            .unwrap();
        let mut draft = ObjectDraft::text("controller")
            .unwrap()
            .with_resource(blob.resource())
            .unwrap()
            .with_surface(Surface {
                role: SurfaceRole::Expanded,
                target: SurfaceTarget::Web,
                entry: blob.uri,
                integrity: Some(blob.integrity),
                bundle: None,
            })
            .unwrap();
        for action in ["follow", "unfollow", "share", "reply"] {
            draft = draft
                .with_capability(CapabilityRequest {
                    id: format!("babble.social.{action}"),
                    version: 1,
                    scope: json!({"object_id": target.id}),
                })
                .unwrap();
        }
        let source = node.publish_draft(&actor.id, draft).unwrap();
        let context = host_context(&node, &actor.id, &source);
        Self {
            root,
            node,
            actor,
            source,
            target,
            context,
        }
    }
    fn payload(&self, action: &str) -> SocialInvocationPayload {
        SocialInvocationPayload {
            target_object_id: self.target.id.clone(),
            text: matches!(action, "share" | "reply").then(|| " hello ".into()),
            media: None,
        }
    }
    fn prepare(&mut self, key: &str, action: &str) -> InvocationRecord {
        self.node
            .prepare_social_invocation(
                self.context.clone(),
                key,
                &format!("babble.social.{action}"),
                self.payload(action),
                Timestamp(Timestamp::now().0 + time::Duration::seconds(60)),
            )
            .unwrap()
    }
    fn approve(&mut self, record: &InvocationRecord) {
        self.node
            .decide_social_invocation(
                &self.context,
                &record.intent().request_key,
                record.id(),
                true,
            )
            .unwrap();
    }
    fn execute(&mut self, record: &InvocationRecord) -> Result<SocialInvocationResult> {
        self.node.execute_social_invocation(
            &self.context,
            &record.intent().request_key,
            record.id(),
        )
    }
    fn counts(&self) -> (usize, usize, usize, usize) {
        (
            self.node.store.list_objects().unwrap().len(),
            self.node.store.list_edges().unwrap().len(),
            self.node.store.list_events().unwrap().len(),
            self.node.store.list_judgments().unwrap().len(),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn invocation_four_social_effects_are_atomic_once_and_replay_the_original_receipt() {
    let mut f = Fixture::new();
    for action in ["follow", "unfollow", "share", "reply"] {
        let before = f.counts();
        let record = f.prepare(action, action);
        assert_eq!(record.intent().method_version, 2);
        assert_eq!(record.intent().capability_version, 1);
        assert!(f.execute(&record).is_err());
        assert_eq!(before, f.counts());
        f.approve(&record);
        let result = f.execute(&record).unwrap();
        assert!(result.invocation.consumed_at().is_some());
        assert_eq!(
            result.receipt.request.fingerprint,
            record.intent().fingerprint().unwrap()
        );
        result.edge.verify(&f.actor).unwrap();
        if let Some(object) = &result.object {
            object.verify(&f.actor).unwrap();
            assert_eq!(object.payload["text"], "hello");
        }
        assert_eq!(f.counts().1, before.1 + 1);
        let after = f.counts();
        assert_eq!(f.execute(&record).unwrap(), result);
        assert_eq!(f.prepare(action, action), result.invocation);
        assert_eq!(after, f.counts());
        let head = f
            .node
            .store
            .invocation(&record.intent().key().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(head.revision(), 2);
        assert_eq!(head, result.invocation);
    }
    let records = f.node.store.list_invocations().unwrap();
    assert_eq!(
        records.iter().filter(|r| r.consumed_at().is_some()).count(),
        4
    );
    let reopened = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    for record in records {
        assert_eq!(
            reopened.social_invocation_result(record.clone()).unwrap(),
            f.node.social_invocation_result(record).unwrap()
        );
    }
}

#[test]
fn invocation_retry_freezes_deadline_payload_context_and_challenge() {
    let mut f = Fixture::new();
    let original = f.prepare("retry", "reply");
    assert_eq!(f.prepare("retry", "reply"), original);
    let mut payload = f.payload("reply");
    payload.text = Some("different".into());
    assert!(
        f.node
            .prepare_social_invocation(
                f.context.clone(),
                "retry",
                "babble.social.reply",
                payload,
                original.intent().deadline
            )
            .is_err()
    );
    for change in ["document", "version", "epoch", "policy", "origin"] {
        let mut context = f.context.clone();
        match change {
            "document" => {
                context.origin = InvocationOrigin::HostAction {
                    document_id: "replacement".into(),
                }
            }
            "version" => context.object_version = Hash::from_bytes(b"changed"),
            "epoch" => context.context_epoch = Hash::from_bytes(b"changed"),
            "policy" => context.policy_revision = Hash::from_bytes(b"changed"),
            _ => context.object_id = f.target.id.clone(),
        }
        assert!(
            f.node
                .decide_social_invocation(&context, "retry", original.id(), true)
                .is_err(),
            "{change}"
        );
    }
    let mut other_login = f.context.clone();
    other_login.login_id = "other-login".into();
    assert!(
        f.node
            .execute_social_invocation(&other_login, "retry", original.id())
            .is_err()
    );
    let other = f.prepare("other", "reply");
    assert!(
        f.node
            .decide_social_invocation(&f.context, "retry", other.id(), true)
            .is_err()
    );
}

#[test]
fn invocation_cancel_deny_restart_and_document_loss_never_revive() {
    let mut f = Fixture::new();
    for action in ["cancel", "deny", "document"] {
        let record = f.prepare(action, "follow");
        match action {
            "cancel" => {
                f.approve(&record);
                f.node
                    .cancel_social_invocation(&f.context, action, record.id())
                    .unwrap();
            }
            "deny" => {
                f.node
                    .decide_social_invocation(&f.context, action, record.id(), false)
                    .unwrap();
            }
            _ => {
                f.node
                    .invalidate_social_document(
                        &f.context.login_id,
                        "test-document",
                        InvocationInvalidation::ContextLost,
                    )
                    .unwrap();
            }
        }
        assert!(f.execute(&record).is_err());
        assert!(
            f.node
                .decide_social_invocation(&f.context, action, record.id(), true)
                .unwrap()
                .state()
                .is_terminal()
        );
    }
    let approved = f.prepare("restart", "follow");
    f.approve(&approved);
    let mut reopened = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    assert!(matches!(
        reopened
            .invocation_by_id(approved.id())
            .unwrap()
            .unwrap()
            .state(),
        InvocationState::Invalidated {
            reason: InvocationInvalidation::Restart
        }
    ));
    assert!(f.execute(&approved).is_err());
}

#[test]
fn invocation_aggregate_quota_is_shared_by_login_and_checked_again_at_consumption() {
    let mut f = Fixture::new();
    let records: Vec<_> = (0..11)
        .map(|n| f.prepare(&format!("quota-{n}"), "follow"))
        .collect();
    for record in &records {
        f.approve(record);
    }
    for record in records.iter().take(10) {
        f.execute(record).unwrap();
    }
    let before = f.counts();
    assert!(
        f.execute(&records[10])
            .unwrap_err()
            .to_string()
            .contains("quota")
    );
    assert_eq!(before, f.counts());
    assert_eq!(
        f.node
            .store
            .invocation(&records[10].intent().key().unwrap())
            .unwrap()
            .unwrap()
            .consumed_at(),
        None
    );
    let mut other_login = f.context.clone();
    other_login.login_id = "second-login".into();
    assert!(
        f.node
            .prepare_social_invocation(
                other_login,
                "new-login",
                "babble.social.follow",
                f.payload("follow"),
                records[0].intent().deadline
            )
            .unwrap_err()
            .to_string()
            .contains("quota")
    );
    f.execute(&records[0]).unwrap();
    assert_eq!(before, f.counts());
}

#[test]
fn invocation_native_grants_and_v1_cannot_bypass_one_use_social_authority() {
    let mut f = Fixture::new();
    let before = f.counts();
    for request in f.source.capabilities.clone() {
        assert!(
            f.node
                .grant_capability(&f.actor.id, &f.source.id, request, GrantDecision::Approved)
                .is_err()
        );
    }
    assert!(
        f.node
            .social_follow(&f.actor.id, &f.source.id, &f.target.id, &[])
            .is_err()
    );
    assert!(
        f.node
            .social_unfollow(&f.actor.id, &f.source.id, &f.target.id, &[])
            .is_err()
    );
    assert!(
        f.node
            .social_reply(&f.actor.id, &f.source.id, &f.target.id, "text", &[])
            .is_err()
    );
    assert!(
        f.node
            .social_share(&f.actor.id, &f.source.id, &f.target.id, "text", &[])
            .is_err()
    );
    assert!(
        f.node
            .prepare_social_invocation(
                f.context.clone(),
                "v1",
                "babble.social.follow.v1",
                f.payload("follow"),
                Timestamp::now()
            )
            .is_err()
    );
    assert_eq!(before, f.counts());
}

#[test]
fn invocation_media_is_validated_before_prompt_and_rechecked_before_commit() {
    let mut f = Fixture::new();
    let media = SocialMediaAttachment {
        title: " attached ".into(),
        resources: vec![f.node.put_media_blob("image/png", b"image bytes").unwrap()],
    };
    for action in ["reply", "share"] {
        let mut payload = f.payload(action);
        payload.media = Some(media.clone());
        let record = f
            .node
            .prepare_social_invocation(
                f.context.clone(),
                action,
                &format!("babble.social.{action}"),
                payload,
                Timestamp(Timestamp::now().0 + time::Duration::seconds(60)),
            )
            .unwrap();
        assert_eq!(record.intent().payload["media"]["title"], "attached");
        f.approve(&record);
        let result = f.execute(&record).unwrap();
        assert_eq!(result.object.as_ref().unwrap().kind.as_str(), "babble.media");
        assert_eq!(
            result.object.as_ref().unwrap().payload["resources"],
            json!(media.resources)
        );
        assert_eq!(f.execute(&record).unwrap(), result);
    }
    let mut payload = f.payload("reply");
    payload.media = Some(media.clone());
    let record = f
        .node
        .prepare_social_invocation(
            f.context.clone(),
            "missing",
            "babble.social.reply",
            payload,
            Timestamp(Timestamp::now().0 + time::Duration::seconds(60)),
        )
        .unwrap();
    f.approve(&record);
    fs::remove_file(
        f.root
            .join("blobs")
            .join(media.resources[0].integrity.as_str()),
    )
    .unwrap();
    let before = f.counts();
    assert!(f.execute(&record).is_err());
    assert_eq!(before, f.counts());
    assert_eq!(
        f.node
            .store
            .invocation(&record.intent().key().unwrap())
            .unwrap()
            .unwrap()
            .state(),
        &InvocationState::Approved
    );
}

#[test]
fn invocation_surface_suspension_invalidates_approval_even_after_resume() {
    let mut f = Fixture::new();
    // Exercise native lifecycle invalidation independently of API admission tests.
    let surface = f.source.surfaces[0].clone();
    let mut plan = babble_runtime::SurfaceRuntime::babble_default()
        .prepare_surface(&f.source, SurfaceRole::Expanded, &[])
        .unwrap();
    plan.admission = babble_runtime::RuntimeAdmissionStatus::Ready;
    let id = SurfaceSessionId::from_material("invocation-surface");
    let mut session = babble_runtime::SurfaceSession::start(id.clone(), plan, "test").unwrap();
    session.transition(SurfaceLifecycle::Warm, "warm").unwrap();
    session
        .transition(SurfaceLifecycle::Active, "active")
        .unwrap();
    f.node.surface_sessions.insert(id.clone(), session);
    f.context.origin = InvocationOrigin::Surface {
        session_id: id.to_string(),
        document_id: "surface-document".into(),
        role: "expanded".into(),
        entry: surface.entry,
        resource_digest: surface.integrity.unwrap(),
    };
    let record = f.prepare("surface", "reply");
    f.approve(&record);
    f.node
        .transition_surface_session(&id, SurfaceLifecycle::Suspended, "hidden")
        .unwrap();
    f.node
        .transition_surface_session(&id, SurfaceLifecycle::Warm, "resume")
        .unwrap();
    f.node
        .transition_surface_session(&id, SurfaceLifecycle::Active, "active")
        .unwrap();
    assert!(f.execute(&record).is_err());
    assert!(matches!(
        f.node
            .invocation_by_id(record.id())
            .unwrap()
            .unwrap()
            .state(),
        InvocationState::Invalidated { .. }
    ));
    let failed = f.prepare("invalidation-failure", "reply");
    f.approve(&failed);
    fs::create_dir(f.root.join(".publication-prepared")).unwrap();
    assert!(
        f.node
            .transition_surface_session(&id, SurfaceLifecycle::Suspended, "hidden")
            .is_err()
    );
    assert_eq!(
        f.node.surface_session(&id).unwrap().lifecycle,
        SurfaceLifecycle::Evicted
    );
    fs::remove_dir(f.root.join(".publication-prepared")).unwrap();
    assert!(
        f.node
            .transition_surface_session(&id, SurfaceLifecycle::Active, "resume")
            .is_err()
    );
    assert!(f.execute(&failed).is_err());
}

#[test]
fn invocation_pending_bound_preserves_retries_and_releases_cancelled_capacity() {
    let mut f = Fixture::new();
    let first = f.prepare("first", "follow");
    for n in 1..32 {
        f.prepare(&format!("pending-{n}"), "follow");
    }
    assert_eq!(f.prepare("first", "follow"), first);
    assert!(
        f.node
            .prepare_social_invocation(
                f.context.clone(),
                "overflow",
                "babble.social.follow",
                f.payload("follow"),
                first.intent().deadline
            )
            .unwrap_err()
            .to_string()
            .contains("pending limit")
    );
    f.node
        .cancel_social_invocation(&f.context, "first", first.id())
        .unwrap();
    f.prepare("new", "follow");
}

#[test]
fn invocation_expired_stored_deadline_cannot_be_refreshed_by_prepare_retry() {
    let mut f = Fixture::new();
    let mut intent = f.prepare("seed", "reply").intent().clone();
    intent.request_key = "expired".into();
    intent.created_at = Timestamp(Timestamp::now().0 - time::Duration::seconds(120));
    intent.deadline = Timestamp(intent.created_at.0 + time::Duration::seconds(60));
    let record = f
        .node
        .store
        .prepare_invocation(intent.clone(), intent.created_at)
        .unwrap();
    let retry = f.prepare("expired", "reply");
    assert_eq!(retry.id(), record.id());
    assert_eq!(retry.intent().deadline, intent.deadline);
    assert_eq!(retry.state(), &InvocationState::Expired);
    assert!(f.execute(&retry).is_err());
}

#[test]
fn invocation_byte_quota_is_cumulative_and_commit_rechecks_preapproved_requests() {
    let mut f = Fixture::new();
    let mut records = Vec::new();
    for key in ["bytes-a", "bytes-b"] {
        let record = f
            .node
            .prepare_social_invocation(
                f.context.clone(),
                key,
                "babble.social.share",
                SocialInvocationPayload {
                    target_object_id: f.target.id.clone(),
                    text: Some("x".repeat(8192)),
                    media: None,
                },
                Timestamp(Timestamp::now().0 + time::Duration::seconds(60)),
            )
            .unwrap();
        f.approve(&record);
        records.push(record);
    }
    f.execute(&records[0]).unwrap();
    let before = f.counts();
    assert!(
        f.execute(&records[1])
            .unwrap_err()
            .to_string()
            .contains("quota")
    );
    assert_eq!(before, f.counts());
}

#[test]
fn invocation_completed_result_survives_deadline_and_cancel_without_duplicate_effects() {
    let mut f = Fixture::new();
    let deadline = Timestamp(Timestamp::now().0 + time::Duration::seconds(2));
    let record = f
        .node
        .prepare_social_invocation(
            f.context.clone(),
            "completed",
            "babble.social.follow",
            f.payload("follow"),
            deadline,
        )
        .unwrap();
    f.approve(&record);
    let result = f.execute(&record).unwrap();
    let before = f.counts();
    let remaining = (deadline.0 - Timestamp::now().0)
        .whole_milliseconds()
        .max(0) as u64;
    std::thread::sleep(std::time::Duration::from_millis(remaining + 10));
    assert_eq!(
        f.node
            .recover_social_invocation(
                &f.actor.id,
                &f.context.login_id,
                &f.source.id,
                "completed",
                "babble.social.follow",
                f.payload("follow")
            )
            .unwrap(),
        Some(result.clone())
    );
    assert_eq!(f.execute(&record).unwrap(), result);
    assert_eq!(
        f.node
            .status_social_invocation(&f.context, "completed")
            .unwrap(),
        Some(result.invocation.clone())
    );
    assert_eq!(
        f.node
            .cancel_social_invocation(&f.context, "completed", record.id())
            .unwrap(),
        result.invocation
    );
    assert_eq!(f.prepare("completed", "follow"), result.invocation);
    assert_eq!(f.counts(), before);
}

#[test]
fn invocation_social_lookup_and_invalidation_leave_external_records_untouched() {
    let mut f = Fixture::new();
    let mut intent = f.prepare("social", "reply").intent().clone();
    intent.request_key = "external".into();
    intent.capability = babble_capabilities::CapabilityId::new("babble.ai.generate").unwrap();
    intent.method = "babble.ai.generate.v1".into();
    intent.method_version = 1;
    intent.executor = InvocationExecutor::External {
        provider: "test".into(),
        version: "1".into(),
    };
    let external = f
        .node
        .store
        .prepare_invocation(intent, Timestamp::now())
        .unwrap();
    assert!(f.node.invocation_by_id(external.id()).unwrap().is_none());
    assert!(
        f.node
            .status_social_invocation(&f.context, "external")
            .is_err()
    );
    f.node
        .invalidate_social_invocations(&f.context, InvocationInvalidation::ContextLost)
        .unwrap();
    assert_eq!(
        f.node
            .store
            .invocation(&external.intent().key().unwrap())
            .unwrap(),
        Some(external)
    );
}

mod recovery;
