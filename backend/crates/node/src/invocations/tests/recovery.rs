use super::*;

#[test]
fn invocation_recovery_excludes_completed_surface_results() {
    let mut f = Fixture::new();
    let surface = f.source.surfaces[0].clone();
    let mut plan = babble_runtime::SurfaceRuntime::babble_default()
        .prepare_surface(&f.source, SurfaceRole::Expanded, &[])
        .unwrap();
    plan.admission = babble_runtime::RuntimeAdmissionStatus::Ready;
    let id = SurfaceSessionId::from_material("recovery-surface");
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
    let original = f.execute(&record).unwrap();
    let before = f.counts();
    assert!(
        f.node
            .recover_social_invocation(
                &f.actor.id,
                &f.context.login_id,
                &f.source.id,
                "surface",
                "babble.social.reply",
                f.payload("reply")
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(
        f.node
            .social_invocation_result(original.invocation)
            .unwrap()
            .receipt,
        original.receipt
    );
    assert_eq!(f.counts(), before);
}

#[test]
fn invocation_recovery_returns_original_media_result_after_document_loss_and_restart() {
    let mut f = Fixture::new();
    let media = SocialMediaAttachment {
        title: " original media ".into(),
        resources: vec![
            f.node
                .put_media_blob("image/png", b"recovery media")
                .unwrap(),
        ],
    };
    let mut payload = f.payload("reply");
    payload.media = Some(media.clone());
    let record = f
        .node
        .prepare_social_invocation(
            f.context.clone(),
            "recover",
            "babble.social.reply",
            payload.clone(),
            Timestamp(Timestamp::now().0 + time::Duration::seconds(60)),
        )
        .unwrap();
    f.approve(&record);
    let original = f.execute(&record).unwrap();
    f.node
        .invalidate_social_document(
            &f.context.login_id,
            "test-document",
            InvocationInvalidation::ContextLost,
        )
        .unwrap();
    let reopened = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    assert_ne!(reopened.invocation_epoch(), f.context.context_epoch);
    // Recovery reads the stored acknowledgement, without executing or fetching media.
    fs::remove_file(
        f.root
            .join("blobs")
            .join(media.resources[0].integrity.as_str()),
    )
    .unwrap();
    let before = f.counts();
    let records = reopened.store.list_invocations().unwrap();
    assert_eq!(
        reopened
            .recover_social_invocation(
                &f.actor.id,
                &f.context.login_id,
                &f.source.id,
                "recover",
                "babble.social.reply",
                payload.clone()
            )
            .unwrap(),
        Some(original.clone())
    );
    payload.text = Some("hello".into());
    payload.media.as_mut().unwrap().title = "original media".into();
    assert_eq!(
        reopened
            .recover_social_invocation(
                &f.actor.id,
                &f.context.login_id,
                &f.source.id,
                "recover",
                "babble.social.reply",
                payload
            )
            .unwrap(),
        Some(original)
    );
    assert_eq!(f.counts(), before);
    assert_eq!(reopened.store.list_invocations().unwrap(), records);
}

#[test]
fn invocation_recovery_requires_original_actor_login_source_method_and_payload() {
    let mut f = Fixture::new();
    let record = f.prepare("recover", "reply");
    f.approve(&record);
    f.execute(&record).unwrap();
    let payload = f.payload("reply");
    assert!(
        f.node
            .recover_social_invocation(
                &f.actor.id,
                "another-login",
                &f.source.id,
                "recover",
                "babble.social.reply",
                payload.clone()
            )
            .unwrap()
            .is_none()
    );
    let another = IdentityId::from_hash(&Hash::from_bytes(b"another-actor"));
    assert!(
        f.node
            .recover_social_invocation(
                &another,
                &f.context.login_id,
                &f.source.id,
                "recover",
                "babble.social.reply",
                payload.clone()
            )
            .unwrap()
            .is_none()
    );
    assert!(
        f.node
            .recover_social_invocation(
                &f.actor.id,
                &f.context.login_id,
                &f.source.id,
                "missing-key",
                "babble.social.reply",
                payload.clone()
            )
            .unwrap()
            .is_none()
    );
    for field in ["source", "method", "text", "target", "media"] {
        let mut candidate = payload.clone();
        let mut source = f.source.id.clone();
        let mut method = "babble.social.reply";
        match field {
            "source" => source = f.target.id.clone(),
            "method" => method = "babble.social.share",
            "text" => candidate.text = Some("changed content".into()),
            "target" => candidate.target_object_id = f.source.id.clone(),
            _ => {
                candidate.media = Some(SocialMediaAttachment {
                    title: "changed".into(),
                    resources: vec![
                        f.node
                            .put_media_blob("image/png", b"changed media")
                            .unwrap(),
                    ],
                })
            }
        }
        assert!(
            f.node
                .recover_social_invocation(
                    &f.actor.id,
                    &f.context.login_id,
                    &source,
                    "recover",
                    method,
                    candidate
                )
                .is_err(),
            "{field}"
        );
    }
}

#[test]
fn invocation_recovery_never_returns_or_changes_unconsumed_or_denied_authority() {
    let mut f = Fixture::new();
    for state in ["pending", "approved", "denied", "cancelled", "expired"] {
        let record = f.prepare(state, "reply");
        match state {
            "approved" => f.approve(&record),
            "denied" => {
                f.node
                    .decide_social_invocation(&f.context, state, record.id(), false)
                    .unwrap();
            }
            "cancelled" => {
                f.node
                    .cancel_social_invocation(&f.context, state, record.id())
                    .unwrap();
            }
            "expired" => {
                f.node
                    .store
                    .transition_invocation(
                        &record,
                        InvocationAction::Expire,
                        &f.context,
                        record.intent().deadline,
                    )
                    .unwrap();
            }
            _ => (),
        }
        let before = f.node.store.list_invocations().unwrap();
        assert!(
            f.node
                .recover_social_invocation(
                    &f.actor.id,
                    &f.context.login_id,
                    &f.source.id,
                    state,
                    "babble.social.reply",
                    f.payload("reply")
                )
                .unwrap()
                .is_none()
        );
        assert_eq!(f.node.store.list_invocations().unwrap(), before);
    }
    let mut intent = f.prepare("past-seed", "reply").intent().clone();
    intent.request_key = "past-pending".into();
    intent.created_at = Timestamp(Timestamp::now().0 - time::Duration::seconds(120));
    intent.deadline = Timestamp(intent.created_at.0 + time::Duration::seconds(60));
    let expired_pending = f
        .node
        .store
        .prepare_invocation(intent.clone(), intent.created_at)
        .unwrap();
    assert!(
        f.node
            .recover_social_invocation(
                &f.actor.id,
                &f.context.login_id,
                &f.source.id,
                "past-pending",
                "babble.social.reply",
                f.payload("reply")
            )
            .unwrap()
            .is_none()
    );
    assert_eq!(
        f.node.store.invocation(&intent.key().unwrap()).unwrap(),
        Some(expired_pending)
    );
}
