use babble_authoring::ObjectDraft;
use babble_identity::IdentityKind;
use babble_judgment_local::LocalProvider;
use babble_node::{LocalNode, moderation::*};
use babble_object::{Surface, SurfaceRole, SurfaceTarget};
use babble_runtime::{SurfaceLifecycle, SurfaceSessionId};
use babble_types::{IdentityId, ObjectId};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
    node: LocalNode<LocalProvider>,
    author: IdentityId,
    reporter: IdentityId,
    first: IdentityId,
    second: IdentityId,
    object: ObjectId,
}
impl Fixture {
    fn new() -> Self {
        static N: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babble-moderation-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let author = node
            .create_identity(IdentityKind::Person, "author")
            .unwrap()
            .id;
        let reporter = node
            .create_identity(IdentityKind::Person, "reporter")
            .unwrap()
            .id;
        let first = node
            .create_identity(IdentityKind::Person, "first")
            .unwrap()
            .id;
        let second = node
            .create_identity(IdentityKind::Person, "second")
            .unwrap()
            .id;
        node.configure_moderators(&format!("{first},{second}"))
            .unwrap();
        let blob = node
            .put_media_blob("text/html", b"<!doctype html><p>moderation</p>")
            .unwrap();
        let draft = ObjectDraft::text("Moderation execution object")
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
            .unwrap();
        let object = node.publish_draft(&author, draft).unwrap().id;
        Self {
            root,
            node,
            author,
            reporter,
            first,
            second,
            object,
        }
    }
    fn report(&mut self, key: &str) -> ModerationCase {
        self.node
            .moderation_report(&self.reporter, report(&self.object, key))
            .unwrap()
    }
    fn decide(
        &mut self,
        c: &ModerationCase,
        outcome: ModerationOutcome,
        key: &str,
    ) -> ModerationCase {
        self.node
            .moderation_decide(
                &self.first,
                c.id.clone(),
                decision(outcome, c.revision, key),
            )
            .unwrap()
    }
    fn restart(&mut self) {
        self.node = LocalNode::open(&self.root, LocalProvider::default()).unwrap();
        self.node
            .configure_moderators(&format!("{},{}", self.first, self.second))
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn report(object: &ObjectId, key: &str) -> ReportRequest {
    ReportRequest {
        object_id: object.clone(),
        reason: ModerationReason::Fraud,
        details: "Private report explanation sufficiently detailed".into(),
        idempotency_key: key.into(),
    }
}
fn decision(outcome: ModerationOutcome, revision: u64, key: &str) -> DecisionRequest {
    DecisionRequest {
        outcome,
        reason: ModerationReason::Fraud,
        explanation: "Reviewed evidence and applied the integrity policy".into(),
        policy_version: POLICY.into(),
        source_signals: vec![],
        expected_revision: revision,
        idempotency_key: key.into(),
    }
}
fn appeal(revision: u64, key: &str) -> AppealRequest {
    AppealRequest {
        details: "Private appeal explanation sufficiently detailed".into(),
        expected_revision: revision,
        idempotency_key: key.into(),
    }
}

#[test]
fn moderation_full_lifecycle_retires_runtime_preserves_history_composes_and_restarts() {
    let mut f = Fixture::new();
    let session = f
        .node
        .start_surface_session_for_identity(
            &f.object,
            SurfaceRole::Feed,
            SurfaceSessionId::from_material("moderation"),
            &f.author,
        )
        .unwrap();
    let signed = f.node.object(&f.object).unwrap().clone();
    let events = f.node.store().list_events().unwrap();
    let first = f.report("first");
    assert!(f.node.require_moderation_execution(&f.object).is_ok());
    assert!(f.node.moderation_case(&f.author, &first.id).is_err());
    let restricted = f.decide(&first, ModerationOutcome::Restrict, "restrict");
    assert!(f.node.require_moderation_execution(&f.object).is_err());
    assert!(
        f.node
            .prepare_surface(&f.object, SurfaceRole::Feed)
            .is_err()
    );
    assert!(
        f.node
            .prepare_surface_for_identity(&f.object, SurfaceRole::Feed, Some(&f.author))
            .is_err()
    );
    assert_eq!(
        f.node.surface_session(&session.id).unwrap().lifecycle,
        SurfaceLifecycle::Evicted
    );
    assert!(
        f.node
            .transition_surface_session(&session.id, SurfaceLifecycle::Active, "resume")
            .is_err()
    );
    assert!(
        f.node
            .authorize_capability_binding(&f.object, "babble.storage.local", 1, &[])
            .is_err()
    );
    assert!(
        matches!(f.node.network_fetch(&f.object,"GET","https://example.com/",Default::default(),None,&[]),Err(babble_types::Error::Conflict(message)) if message.contains("execution is restricted"))
    );
    let redacted = f.node.moderation_case(&f.author, &first.id).unwrap();
    assert!(
        redacted.reporter_id.is_none() && redacted.details.is_none() && redacted.reason.is_none()
    );
    let appealed = f
        .node
        .moderation_appeal(&f.author, first.id.clone(), appeal(2, "appeal"))
        .unwrap();
    assert!(appealed.reporter_id.is_none());
    assert!(
        f.node
            .moderation_case(&f.reporter, &first.id)
            .unwrap()
            .appeal
            .unwrap()
            .details
            .is_none()
    );
    assert!(
        f.node
            .moderation_decide(
                &f.first,
                first.id.clone(),
                decision(ModerationOutcome::NoAction, 3, "self-appeal")
            )
            .is_err()
    );
    assert_eq!(
        f.node
            .moderation_decide(
                &f.first,
                first.id.clone(),
                decision(ModerationOutcome::Restrict, 1, "restrict")
            )
            .unwrap(),
        restricted
    );
    let second = f.report("second");
    f.decide(&second, ModerationOutcome::Restrict, "restrict-second");
    let closed = f
        .node
        .moderation_decide(
            &f.second,
            first.id.clone(),
            decision(ModerationOutcome::NoAction, 3, "reverse"),
        )
        .unwrap();
    assert_eq!(closed.status, ModerationStatus::Closed);
    assert!(f.node.require_moderation_execution(&f.object).is_err());
    f.node
        .moderation_appeal(&f.author, second.id.clone(), appeal(2, "appeal-second"))
        .unwrap();
    f.node
        .moderation_decide(
            &f.second,
            second.id.clone(),
            decision(ModerationOutcome::NoAction, 3, "reverse-second"),
        )
        .unwrap();
    assert!(
        f.node
            .prepare_surface_for_identity(&f.object, SurfaceRole::Feed, Some(&f.author))
            .is_ok()
    );
    f.node
        .start_surface_session_for_identity(
            &f.object,
            SurfaceRole::Feed,
            SurfaceSessionId::from_material("fresh"),
            &f.author,
        )
        .unwrap();
    assert_eq!(f.node.object(&f.object).unwrap(), &signed);
    assert_eq!(f.node.store().list_events().unwrap(), events);
    f.restart();
    assert_eq!(f.node.moderation_case(&f.first, &first.id).unwrap(), closed);
    assert_eq!(
        f.node
            .moderation_report(&f.reporter, report(&f.object, "first"))
            .unwrap(),
        first
    );
    assert!(f.node.require_moderation_execution(&f.object).is_ok());
    f.node.configure_moderators("").unwrap();
    assert!(
        f.node
            .moderation_decide(
                &f.first,
                first.id.clone(),
                decision(ModerationOutcome::Restrict, 1, "restrict")
            )
            .is_err()
    );
    assert!(f.node.moderation_case(&f.first, &first.id).is_err());
}

#[test]
fn moderation_authority_cas_signals_bounds_pagination_and_reporter_appeal() {
    let mut f = Fixture::new();
    let one = f.report("one");
    let two = f.report("two");
    let page = f
        .node
        .moderation_list(&f.reporter, ModerationScope::Mine, None, 1)
        .unwrap();
    assert_eq!(page.items[0].id, two.id);
    f.report("three");
    let next = f
        .node
        .moderation_list(&f.reporter, ModerationScope::Mine, page.next_before, 1)
        .unwrap();
    assert_eq!(next.items[0].id, one.id);
    assert!(
        f.node
            .moderation_list(&f.reporter, ModerationScope::Mine, None, 101)
            .is_err()
    );
    assert!(
        f.node
            .moderation_list(&f.reporter, ModerationScope::Queue, None, 25)
            .is_err()
    );
    f.node
        .configure_moderators(&format!(
            "{},{},{},{}",
            f.author, f.reporter, f.first, f.second
        ))
        .unwrap();
    for actor in [&f.author, &f.reporter] {
        assert!(
            f.node
                .moderation_decide(
                    actor,
                    one.id.clone(),
                    decision(ModerationOutcome::Restrict, 1, "own")
                )
                .is_err()
        );
    }
    assert!(
        f.node
            .moderation_decide(
                &f.first,
                one.id.clone(),
                decision(ModerationOutcome::Restrict, 2, "stale")
            )
            .is_err()
    );
    let mut signal = decision(ModerationOutcome::Restrict, 1, "signal");
    signal
        .source_signals
        .push(babble_types::JudgmentId::new_unchecked(format!(
            "jud_{}",
            "a".repeat(64)
        )));
    assert!(
        f.node
            .moderation_decide(&f.first, one.id.clone(), signal)
            .is_err()
    );
    let dismissed = f.decide(&one, ModerationOutcome::NoAction, "dismiss");
    assert!(
        f.node
            .moderation_appeal(&f.author, one.id.clone(), appeal(2, "wrong-appellant"))
            .is_err()
    );
    f.node
        .moderation_appeal(&f.reporter, one.id.clone(), appeal(2, "appeal"))
        .unwrap();
    assert!(
        f.node
            .moderation_appeal(&f.reporter, one.id.clone(), appeal(3, "appeal-again"))
            .is_err()
    );
    f.node
        .moderation_decide(
            &f.second,
            one.id.clone(),
            decision(ModerationOutcome::Restrict, 3, "final"),
        )
        .unwrap();
    assert!(
        f.node
            .moderation_decide(
                &f.first,
                one.id.clone(),
                decision(ModerationOutcome::Restrict, 1, "dismiss")
            )
            .is_err()
    );
    assert_eq!(
        f.node
            .moderation_decide(
                &f.first,
                one.id.clone(),
                decision(ModerationOutcome::NoAction, 1, "dismiss")
            )
            .unwrap(),
        dismissed
    );
    let mut changed = report(&f.object, "one");
    changed.details = "Changed report intent that must be rejected".into();
    assert!(f.node.moderation_report(&f.reporter, changed).is_err());
    for details in ["x".repeat(19), "x".repeat(4001), " ".repeat(16001)] {
        let mut r = report(&f.object, "invalid");
        r.details = details;
        assert!(f.node.moderation_report(&f.reporter, r).is_err());
    }
    f.restart();
    assert!(f.node.require_moderation_execution(&f.object).is_err());
}

#[test]
fn moderation_missing_database_and_tampered_signed_receipts_fail_closed() {
    let mut f = Fixture::new();
    let c = f.report("tamper");
    f.decide(&c, ModerationOutcome::Restrict, "restrict");
    let path = f.root.join("private_moderation/moderation.sqlite3");
    let backup = path.with_extension("backup");
    std::fs::rename(&path, &backup).unwrap();
    assert!(
        f.node
            .moderation_report(&f.reporter, report(&f.object, "storage-retry"))
            .is_err()
    );
    assert!(f.node.require_moderation_execution(&f.object).is_err());
    assert!(LocalNode::open(&f.root, LocalProvider::default()).is_err());
    assert!(!path.exists());
    std::fs::rename(backup, &path).unwrap();
    f.node
        .moderation_report(&f.reporter, report(&f.object, "storage-retry"))
        .unwrap();
    // Corrupt the signed record without changing SQLite structure.
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute("UPDATE receipts SET record=replace(record,'Private report explanation','Forged report explanation')",[]).unwrap();
    assert!(LocalNode::open(&f.root, LocalProvider::default()).is_err());
}

#[test]
fn moderation_configuration_strict_without_environment_races() {
    let mut f = Fixture::new();
    assert!(parse_reviewers("").unwrap().is_empty());
    for value in [
        " ".into(),
        format!("{},", f.first),
        format!(" {},{}", f.first, f.second),
        format!("{},{}", f.first, f.first),
        format!("id_{}", "G".repeat(64)),
        format!("id_{}", "a".repeat(63)),
    ] {
        assert!(f.node.configure_moderators(&value).is_err());
        assert!(!f.node.moderation_access(&f.first).unwrap().can_review);
    }
}

#[test]
fn moderation_discovery_following_cursor_and_personal_safety_compose() {
    let mut f = Fixture::new();
    f.node
        .publish_draft(
            &f.author,
            ObjectDraft::text("Another public author object").unwrap(),
        )
        .unwrap();
    f.node
        .set_following(&f.reporter, &f.author, true, 0, "follow")
        .unwrap();
    let mut query = babble_node::FollowingQuery {
        limit: 1,
        cursor: None,
        search: None,
    };
    let initial = f.node.following_feed(&f.reporter, &query).unwrap();
    assert!(initial.next_cursor.is_some());
    assert!(
        f.node
            .discover_objects(babble_node::DiscoveryQuery::default())
            .unwrap()
            .objects
            .iter()
            .any(|o| o.id == f.object)
    );
    let c = f.report("report");
    f.decide(&c, ModerationOutcome::Restrict, "restrict");
    query.cursor = initial.next_cursor;
    assert!(f.node.following_feed(&f.reporter, &query).is_err());
    query.cursor = None;
    query.limit = 50;
    assert!(
        f.node
            .following_feed(&f.reporter, &query)
            .unwrap()
            .objects
            .iter()
            .all(|o| o.id != f.object)
    );
    assert!(
        f.node
            .discover_objects(babble_node::DiscoveryQuery::default())
            .unwrap()
            .objects
            .iter()
            .all(|o| o.id != f.object)
    );
    let search = babble_node::ObjectSearchQuery {
        query: None,
        author: None,
        kind: None,
        limit: 50,
    };
    assert!(
        f.node
            .search_objects(search)
            .unwrap()
            .iter()
            .all(|o| o.object.id != f.object)
    );
    f.node
        .set_safety(&f.reporter, &f.author, false, true, 0, "mute")
        .unwrap();
    f.node
        .moderation_appeal(&f.author, c.id.clone(), appeal(2, "appeal"))
        .unwrap();
    f.node
        .moderation_decide(
            &f.second,
            c.id,
            decision(ModerationOutcome::NoAction, 3, "reverse"),
        )
        .unwrap();
    assert!(
        f.node
            .following_feed(&f.reporter, &query)
            .unwrap()
            .objects
            .is_empty()
    );
    assert!(f.node.require_moderation_execution(&f.object).is_ok());
}

#[test]
fn moderation_atomic_failure_rolls_back_receipt_audit_projection_and_retry() {
    let mut f = Fixture::new();
    let c = f.report("report");
    let path = f.root.join("private_moderation/moderation.sqlite3");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch("CREATE TRIGGER fail_projection BEFORE UPDATE ON cases BEGIN SELECT RAISE(ABORT,'injected write failure'); END;").unwrap();
    assert!(matches!(
        f.node.moderation_decide(
            &f.first,
            c.id.clone(),
            decision(ModerationOutcome::Restrict, 1, "retry")
        ),
        Err(babble_types::Error::StorageUnavailable(_))
    ));
    assert_eq!(f.node.moderation_case(&f.first, &c.id).unwrap(), c);
    assert!(f.node.require_moderation_execution(&f.object).is_ok());
    assert_eq!(
        db.query_row("SELECT count(*) FROM receipts", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        1
    );
    db.execute_batch("DROP TRIGGER fail_projection;").unwrap();
    f.decide(&c, ModerationOutcome::Restrict, "retry");
    f.restart();
    assert!(f.node.require_moderation_execution(&f.object).is_err());
    db.execute("UPDATE cases SET restricted=0", []).unwrap();
    assert!(LocalNode::open(&f.root, LocalProvider::default()).is_err());
}

#[test]
fn moderation_intake_limit_preserves_retries_review_and_appeal() {
    let mut f = Fixture::new();
    let first = f.report("first");
    for n in 1..1000 {
        f.report(&format!("report-{n}"));
    }
    assert!(
        matches!(f.node.moderation_report(&f.reporter,report(&f.object,"excess")),Err(babble_types::Error::Conflict(message)) if message == REPORT_INTAKE_LIMIT)
    );
    assert_eq!(
        f.node
            .moderation_report(&f.reporter, report(&f.object, "first"))
            .unwrap(),
        first
    );
    f.decide(&first, ModerationOutcome::NoAction, "dismiss");
    f.node
        .moderation_appeal(&f.reporter, first.id.clone(), appeal(2, "appeal"))
        .unwrap();
    f.node
        .moderation_decide(
            &f.second,
            first.id,
            decision(ModerationOutcome::Restrict, 3, "final"),
        )
        .unwrap();
    f.restart();
    assert!(f.node.require_moderation_execution(&f.object).is_err());
}

#[test]
fn moderation_signals_must_exist_match_object_and_be_unique() {
    let mut f = Fixture::new();
    let case = f.report("report");
    let other = f
        .node
        .publish_draft(
            &f.author,
            ObjectDraft::text("Unrelated evidence object").unwrap(),
        )
        .unwrap();
    let own = f.node.store().object_judgment_inputs(&f.object).unwrap()[0]
        .judgment_id
        .clone();
    let foreign = f.node.store().object_judgment_inputs(&other.id).unwrap()[0]
        .judgment_id
        .clone();
    for signals in [
        vec![foreign],
        vec![own.clone(), own.clone()],
        vec![own.clone(); 21],
    ] {
        let mut request = decision(ModerationOutcome::Restrict, 1, "signals");
        request.source_signals = signals;
        assert!(
            f.node
                .moderation_decide(&f.first, case.id.clone(), request)
                .is_err()
        );
    }
    let mut request = decision(ModerationOutcome::Restrict, 1, "signals");
    request.source_signals = vec![own];
    f.node
        .moderation_decide(&f.first, case.id, request)
        .unwrap();
    f.restart();
}

#[test]
fn moderation_concurrent_revision_decisions_commit_exactly_one_history() {
    let mut f = Fixture::new();
    let case = f.report("report");
    let mut other = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    other
        .configure_moderators(&format!("{},{}", f.first, f.second))
        .unwrap();
    let mut first = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    first
        .configure_moderators(&format!("{},{}", f.first, f.second))
        .unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut handles = vec![];
    for (mut node, actor, key) in [
        (first, f.first.clone(), "first"),
        (other, f.second.clone(), "second"),
    ] {
        let barrier = barrier.clone();
        let id = case.id.clone();
        handles.push(std::thread::spawn(move || {
            barrier.wait();
            node.moderation_decide(&actor, id, decision(ModerationOutcome::Restrict, 1, key))
        }));
    }
    let outcomes: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        outcomes
            .iter()
            .filter(|r| matches!(r, Err(babble_types::Error::Conflict(_))))
            .count(),
        1
    );
    f.restart();
    assert_eq!(
        f.node
            .moderation_case(&f.first, &case.id)
            .unwrap()
            .decisions
            .len(),
        1
    );
}

#[test]
fn moderation_following_cursor_depends_only_on_effective_restrictions() {
    let mut f = Fixture::new();
    for n in 0..3 {
        f.node
            .publish_draft(
                &f.author,
                ObjectDraft::text(&format!("Additional Object {n}")).unwrap(),
            )
            .unwrap();
    }
    f.node
        .set_following(&f.reporter, &f.author, true, 0, "follow")
        .unwrap();
    let mut query = babble_node::FollowingQuery {
        limit: 1,
        cursor: None,
        search: None,
    };
    query.cursor = f
        .node
        .following_feed(&f.reporter, &query)
        .unwrap()
        .next_cursor;
    let unrelated = f.report("unrelated");
    assert!(f.node.following_feed(&f.reporter, &query).is_ok());
    f.decide(&unrelated, ModerationOutcome::NoAction, "dismiss");
    assert!(f.node.following_feed(&f.reporter, &query).is_ok());
    f.node
        .moderation_appeal(&f.reporter, unrelated.id, appeal(2, "appeal-unrelated"))
        .unwrap();
    assert!(f.node.following_feed(&f.reporter, &query).is_ok());
    let first = f.report("first");
    f.decide(&first, ModerationOutcome::Restrict, "restrict-first");
    assert!(f.node.following_feed(&f.reporter, &query).is_err());
    query.cursor = None;
    query.cursor = f
        .node
        .following_feed(&f.reporter, &query)
        .unwrap()
        .next_cursor;
    let second = f.report("second");
    f.decide(&second, ModerationOutcome::Restrict, "restrict-second");
    assert!(f.node.following_feed(&f.reporter, &query).is_ok());
    f.node
        .moderation_appeal(&f.author, first.id.clone(), appeal(2, "appeal-first"))
        .unwrap();
    f.node
        .moderation_decide(
            &f.second,
            first.id,
            decision(ModerationOutcome::NoAction, 3, "reverse-first"),
        )
        .unwrap();
    assert!(f.node.following_feed(&f.reporter, &query).is_ok());
    f.node
        .moderation_appeal(&f.author, second.id.clone(), appeal(2, "appeal-second"))
        .unwrap();
    f.node
        .moderation_decide(
            &f.second,
            second.id,
            decision(ModerationOutcome::NoAction, 3, "reverse-second"),
        )
        .unwrap();
    assert!(f.node.following_feed(&f.reporter, &query).is_err());
}
