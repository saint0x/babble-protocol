use babble_authoring::{EdgeDraft, ObjectDraft};
use babble_crypto::Keypair;
use babble_graph::{Appreciation, Edge, EdgeOrigin, ReactionValue, Relation};
use babble_identity::{Identity, IdentityKeyScope, IdentityKind};
use babble_judgment_local::LocalProvider;
use babble_node::{FollowingQuery, ImportBundle, LocalNode};
use babble_object::{Object, Provenance};
use babble_types::Error;
use std::path::PathBuf;

struct Fixture {
    root: PathBuf,
    node: LocalNode<LocalProvider>,
    alice: Identity,
    bob: Identity,
    key: Keypair,
    a: Object,
    b: Object,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "babble-safety-node-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let key = Keypair::generate();
        let alice = Identity::create(IdentityKind::Person, "alice", &key).unwrap();
        node.import_signing_identity(alice.clone(), key.clone())
            .unwrap();
        let bob = node.create_identity(IdentityKind::Person, "bob").unwrap();
        let a = node.publish_text(&alice.id, "alice post").unwrap();
        let b = node.publish_text(&bob.id, "bob post").unwrap();
        Self {
            root,
            node,
            alice,
            bob,
            key,
            a,
            b,
        }
    }
    fn safety(&mut self, blocked: bool, muted: bool, revision: u64) {
        self.node
            .set_safety(
                &self.alice.id,
                &self.bob.id,
                blocked,
                muted,
                revision,
                &format!("safety-{revision}"),
            )
            .unwrap();
    }
    fn query(cursor: Option<String>) -> FollowingQuery {
        FollowingQuery {
            limit: 1,
            cursor,
            search: None,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn safety_restart_rotation_exact_retries_tombstones_and_public_export_exclusion() {
    let mut f = Fixture::new();
    let public = f
        .node
        .object_bundle(&[f.a.id.clone(), f.b.id.clone()].into())
        .unwrap();
    let before = serde_json::to_value(&public).unwrap();
    assert_eq!(
        f.node
            .safety_state(&f.alice.id, &f.bob.id)
            .unwrap()
            .revision,
        0
    );
    let on = f
        .node
        .set_safety(&f.alice.id, &f.bob.id, true, true, 0, "on")
        .unwrap();
    assert_eq!(
        serde_json::to_value(
            f.node
                .object_bundle(&[f.a.id.clone(), f.b.id.clone()].into())
                .unwrap()
        )
        .unwrap(),
        before
    );
    assert!(
        f.node
            .safety_snapshot(&f.bob.id)
            .unwrap()
            .entries
            .is_empty()
    );
    let snapshot = f.node.safety_snapshot(&f.alice.id).unwrap();
    assert_eq!(snapshot.entries[0].identity, f.bob);
    assert_eq!(snapshot.entries[0].state, on);
    assert_eq!(snapshot.revision, 1);
    assert_eq!(
        f.node
            .set_safety(&f.alice.id, &f.bob.id, true, true, 1, "noop")
            .unwrap(),
        on
    );
    assert_eq!(f.node.safety_snapshot(&f.alice.id).unwrap(), snapshot);
    f.node
        .rotate_identity_key(&f.alice.id, IdentityKeyScope::Root, None, "rotate")
        .unwrap();
    let muted = f
        .node
        .set_safety(&f.alice.id, &f.bob.id, false, true, 1, "unblock")
        .unwrap();
    assert!(muted.muted && !muted.blocked);
    let off = f
        .node
        .set_safety(&f.alice.id, &f.bob.id, false, false, 2, "clear")
        .unwrap();
    f.node = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    assert_eq!(
        f.node
            .set_safety(&f.alice.id, &f.bob.id, true, true, 0, "on")
            .unwrap(),
        on
    );
    assert_eq!(f.node.safety_state(&f.alice.id, &f.bob.id).unwrap(), off);
    assert_eq!(f.node.safety_snapshot(&f.alice.id).unwrap().revision, 3);
    assert!(
        f.node
            .safety_snapshot(&f.alice.id)
            .unwrap()
            .entries
            .is_empty()
    );
    for (b, m, rev, key) in [
        (false, false, 0, "on"),
        (true, true, 3, "on"),
        (true, false, 2, "stale"),
    ] {
        assert!(matches!(
            f.node.set_safety(&f.alice.id, &f.bob.id, b, m, rev, key),
            Err(Error::Conflict(_))
        ));
    }
}

#[test]
fn safety_bidirectional_blocks_all_native_graph_publication_paths_but_imports_stay_public() {
    let mut f = Fixture::new();
    f.safety(true, false, 0);
    let before = (
        f.node.store().list_objects().unwrap().len(),
        f.node.store().list_edges().unwrap().len(),
        f.node.store().list_events().unwrap().len(),
    );
    for (actor, source, target) in [
        (&f.alice.id, &f.a.id, &f.b.id),
        (&f.bob.id, &f.b.id, &f.a.id),
    ] {
        assert!(
            f.node
                .set_following(
                    actor,
                    if actor == &f.alice.id {
                        &f.bob.id
                    } else {
                        &f.alice.id
                    },
                    true,
                    0,
                    "blocked-follow"
                )
                .is_err()
        );
        for relation in [
            Relation::ReplyTo,
            Relation::Quotes,
            Relation::Follows,
            Relation::Supports,
            Relation::Custom("links".into()),
        ] {
            assert!(
                f.node
                    .publish_edge(
                        actor,
                        source.clone(),
                        target.clone(),
                        relation.clone(),
                        EdgeOrigin::HumanAssertion
                    )
                    .is_err()
            );
            let draft = EdgeDraft::new(
                source.clone(),
                target.clone(),
                relation,
                EdgeOrigin::ApplicationAssertion,
            )
            .unwrap();
            assert!(f.node.publish_edge_draft(actor, draft).is_err());
        }
        assert!(
            f.node
                .infer_relationship_edge(actor, source, target, "supports", 0.0)
                .is_err()
        );
        assert!(
            f.node
                .fork_object(actor, target, ObjectDraft::text("fork").unwrap())
                .is_err()
        );
        assert!(
            f.node
                .remix_object(
                    actor,
                    vec![target.clone()],
                    ObjectDraft::text("remix").unwrap()
                )
                .is_err()
        );
        let draft = ObjectDraft::text("provenance bypass")
            .unwrap()
            .with_provenance(Provenance {
                parent: Some(target.clone()),
                forked_from: None,
                remixed_from: vec![],
            })
            .unwrap();
        assert!(f.node.publish_draft(actor, draft).is_err());
        let value = ReactionValue {
            appreciation: Some(Appreciation::Like),
            ..Default::default()
        };
        assert!(
            f.node
                .set_reaction(actor, target, value, 0, "blocked-reaction")
                .is_err()
        );
    }
    let embedded = Edge::new(
        f.a.id.clone(),
        f.b.id.clone(),
        Relation::ReplyTo,
        EdgeOrigin::HumanAssertion,
        Some(f.alice.id.clone()),
    )
    .unwrap()
    .sign(&f.alice, &f.key)
    .unwrap();
    let record = Object::text(&f.alice, "embedded bypass")
        .unwrap()
        .with_relations(vec![embedded.clone()])
        .unwrap()
        .sign(&f.alice, &f.key)
        .unwrap();
    assert!(
        f.node
            .publish_object_record(&f.alice.id, record.clone())
            .is_err()
    );
    assert_eq!(
        before,
        (
            f.node.store().list_objects().unwrap().len(),
            f.node.store().list_edges().unwrap().len(),
            f.node.store().list_events().unwrap().len()
        )
    );
    f.node
        .import_bundle(ImportBundle {
            identities: vec![],
            objects: vec![record.clone()],
            edges: vec![embedded],
            events: vec![],
        })
        .unwrap();
    assert!(f.node.object(&record.id).is_some());
    assert!(f.node.object(&f.b.id).is_some());
    f.safety(false, false, 1);
    f.node
        .publish_edge(
            &f.alice.id,
            f.a.id.clone(),
            f.b.id.clone(),
            Relation::ReplyTo,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();
}

#[test]
fn safety_mutes_filter_following_invalidate_cursors_and_allow_interaction_and_withdrawals() {
    let mut f = Fixture::new();
    let third = f
        .node
        .create_identity(IdentityKind::Person, "third")
        .unwrap();
    f.node.publish_text(&third.id, "third post").unwrap();
    f.node
        .set_following(&f.alice.id, &f.bob.id, true, 0, "follow")
        .unwrap();
    f.node
        .set_following(&f.alice.id, &third.id, true, 0, "third")
        .unwrap();
    let cursor = f
        .node
        .following_feed(&f.alice.id, &Fixture::query(None))
        .unwrap()
        .next_cursor;
    assert!(cursor.is_some());
    f.safety(false, true, 0);
    assert!(matches!(
        f.node.following_feed(&f.alice.id, &Fixture::query(cursor)),
        Err(Error::Conflict(_))
    ));
    let page = f
        .node
        .following_feed(&f.alice.id, &Fixture::query(None))
        .unwrap();
    assert!(page.objects.iter().all(|o| o.author == third.id));
    assert!(page.next_cursor.is_none());
    f.node
        .publish_edge(
            &f.alice.id,
            f.a.id.clone(),
            f.b.id.clone(),
            Relation::Quotes,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();
    let like = ReactionValue {
        appreciation: Some(Appreciation::Like),
        stance: Some(babble_graph::Stance::Support),
        ..Default::default()
    };
    let original = f
        .node
        .set_reaction(&f.alice.id, &f.b.id, like.clone(), 0, "like")
        .unwrap();
    f.node
        .publish_edge(
            &f.alice.id,
            f.a.id.clone(),
            f.b.id.clone(),
            Relation::Follows,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();
    f.safety(true, true, 1);
    assert_eq!(
        f.node
            .set_reaction(&f.alice.id, &f.b.id, like, 0, "like")
            .unwrap(),
        original
    );
    let partial = ReactionValue {
        appreciation: Some(Appreciation::Like),
        ..Default::default()
    };
    f.node
        .set_reaction(&f.alice.id, &f.b.id, partial, 1, "partial-withdraw")
        .unwrap();
    f.node
        .set_reaction(
            &f.alice.id,
            &f.b.id,
            ReactionValue::default(),
            2,
            "withdraw",
        )
        .unwrap();
    f.node
        .set_following(&f.alice.id, &f.bob.id, false, 1, "unfollow")
        .unwrap();
    let withdrawal = EdgeDraft::new(
        f.a.id.clone(),
        f.b.id.clone(),
        Relation::Custom("unfollows".into()),
        EdgeOrigin::HumanAssertion,
    )
    .unwrap()
    .with_metadata([("removes_relation".into(), serde_json::json!("follows"))].into())
    .unwrap();
    f.node.publish_edge_draft(&f.alice.id, withdrawal).unwrap();
    f.node = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    assert!(f.node.safety_state(&f.alice.id, &f.bob.id).unwrap().blocked);
}

#[test]
fn safety_malformed_targets_and_requests_never_write() {
    let mut f = Fixture::new();
    for key in ["", " ", "line\nbreak"] {
        assert!(
            f.node
                .set_safety(&f.alice.id, &f.bob.id, true, false, 0, key)
                .is_err()
        );
    }
    assert!(
        f.node
            .set_safety(&f.alice.id, &f.bob.id, true, false, u64::MAX, "range")
            .is_err()
    );
    assert!(
        f.node
            .set_safety(&f.alice.id, &f.alice.id, true, false, 0, "self")
            .is_err()
    );
    assert!(
        f.node
            .set_safety(
                &f.alice.id,
                &babble_types::IdentityId::new_unchecked("bad"),
                true,
                false,
                0,
                "bad"
            )
            .is_err()
    );
    assert!(
        f.node
            .safety_snapshot(&f.alice.id)
            .unwrap()
            .entries
            .is_empty()
    );
}

#[test]
fn safety_reopen_fails_closed_for_tampered_records_and_missing_database() {
    for sql in [
        "UPDATE pairs SET muted=1",
        "UPDATE actions SET record=json_set(record,'$.payload.state.blocked',0)",
        "UPDATE receipts SET record=json_set(record,'$.payload.request.muted',1)",
        "DELETE FROM receipt_heads",
    ] {
        let mut f = Fixture::new();
        f.safety(true, false, 0);
        rusqlite::Connection::open(f.root.join("private_safety/safety.sqlite3"))
            .unwrap()
            .execute_batch(sql)
            .unwrap();
        assert!(
            LocalNode::open(&f.root, LocalProvider::default()).is_err(),
            "{sql}"
        );
    }
    let mut f = Fixture::new();
    f.safety(true, false, 0);
    std::fs::remove_file(f.root.join("private_safety/safety.sqlite3")).unwrap();
    assert!(LocalNode::open(&f.root, LocalProvider::default()).is_err());
    assert!(!f.root.join("private_safety/safety.sqlite3").exists());
}

#[test]
fn safety_unfollow_exemption_requires_a_canonical_owned_withdrawal() {
    let mut f = Fixture::new();
    let metadata = || [("removes_relation".into(), serde_json::json!("follows"))].into();
    let draft = |source, target, origin| {
        EdgeDraft::new(source, target, Relation::Custom("unfollows".into()), origin).unwrap()
    };
    f.safety(true, false, 0);
    // A matching label and metadata without a prior signed follow are not a withdrawal.
    assert!(
        f.node
            .publish_edge_draft(
                &f.alice.id,
                draft(f.a.id.clone(), f.b.id.clone(), EdgeOrigin::HumanAssertion)
                    .with_metadata(metadata())
                    .unwrap()
            )
            .is_err()
    );
    f.safety(false, false, 1);
    f.node
        .publish_edge(
            &f.alice.id,
            f.a.id.clone(),
            f.b.id.clone(),
            Relation::Follows,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();
    f.node
        .publish_edge(
            &f.alice.id,
            f.b.id.clone(),
            f.a.id.clone(),
            Relation::Follows,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();
    f.safety(true, false, 2);
    for invalid in [
        draft(f.a.id.clone(), f.b.id.clone(), EdgeOrigin::HumanAssertion),
        draft(f.a.id.clone(), f.b.id.clone(), EdgeOrigin::JudgmentDerived)
            .with_metadata(metadata())
            .unwrap(),
        draft(f.b.id.clone(), f.a.id.clone(), EdgeOrigin::HumanAssertion)
            .with_metadata(metadata())
            .unwrap(),
        draft(f.a.id.clone(), f.b.id.clone(), EdgeOrigin::HumanAssertion)
            .with_metadata([("removes_relation".into(), serde_json::json!("quotes"))].into())
            .unwrap(),
        draft(f.a.id.clone(), f.b.id.clone(), EdgeOrigin::HumanAssertion)
            .with_metadata(
                [
                    ("removes_relation".into(), serde_json::json!("follows")),
                    ("text".into(), serde_json::json!("new relationship")),
                ]
                .into(),
            )
            .unwrap(),
    ] {
        assert!(f.node.publish_edge_draft(&f.alice.id, invalid).is_err());
    }
    let embedded = draft(f.a.id.clone(), f.b.id.clone(), EdgeOrigin::HumanAssertion)
        .sign(&f.alice, &f.key)
        .unwrap();
    let record = Object::text(&f.alice, "label bypass")
        .unwrap()
        .with_relations(vec![embedded])
        .unwrap()
        .sign(&f.alice, &f.key)
        .unwrap();
    assert!(f.node.publish_object_record(&f.alice.id, record).is_err());
    f.node
        .publish_edge_draft(
            &f.alice.id,
            draft(f.a.id.clone(), f.b.id.clone(), EdgeOrigin::HumanAssertion)
                .with_metadata(metadata())
                .unwrap(),
        )
        .unwrap();
}
