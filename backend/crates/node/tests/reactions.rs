use babel_crypto::Keypair;
use babel_identity::{Identity, IdentityKeyScope, IdentityKind};
use babel_judgment_local::LocalProvider;
use babel_node::{
    Appreciation, Engagement, ImportBundle, LocalNode, ReactionSummary, ReactionValue, Stance,
};
use babel_types::{Error, IdentityId, ObjectId};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
    node: LocalNode<LocalProvider>,
    author: Identity,
    other: Identity,
    object: ObjectId,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babel-reactions-node-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let author = node
            .create_identity(IdentityKind::Person, "author")
            .unwrap();
        let other = node.create_identity(IdentityKind::Person, "other").unwrap();
        let object = node
            .publish_text(&author.id, "Object to react to")
            .unwrap()
            .id;
        Self {
            root,
            node,
            author,
            other,
            object,
        }
    }
    fn reopen(&mut self) {
        self.node = LocalNode::open(&self.root, LocalProvider::default()).unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn like() -> ReactionValue {
    ReactionValue {
        appreciation: Some(Appreciation::Like),
        ..Default::default()
    }
}

#[test]
fn reactions_live_retry_verifies_receipt_signature_hash_and_original_state() {
    for sql in [
        "UPDATE receipts SET record=json_set(record,'$.payload.state.value.appreciation','dislike') WHERE key='on'",
        "UPDATE receipts SET record=json_set(record,'$.payload.state.revision',2) WHERE key='on'",
        "UPDATE receipts SET record=json_set(record,'$.signature',(SELECT json_extract(record,'$.signature') FROM receipts WHERE key='noop')) WHERE key='on'",
        "UPDATE receipts SET id='forged' WHERE key='on'",
        "UPDATE receipts SET sequence=99 WHERE key='on'",
        "UPDATE receipts SET object='forged' WHERE key='on'",
        "UPDATE actions SET record=json_set(record,'$.payload.state.value.appreciation','dislike') WHERE revision=1",
        "DELETE FROM actions WHERE revision=1",
    ] {
        let mut f = Fixture::new();
        let original = f
            .node
            .set_reaction(&f.author.id, &f.object, like(), 0, "on")
            .unwrap();
        f.node
            .set_reaction(&f.author.id, &f.object, like(), 1, "noop")
            .unwrap();
        // Historical receipt remains valid after rotating the currently admitted signer.
        f.node
            .rotate_identity_key(&f.author.id, IdentityKeyScope::Root, None, "rotate")
            .unwrap();
        assert_eq!(
            f.node
                .set_reaction(&f.author.id, &f.object, like(), 0, "on")
                .unwrap(),
            original
        );
        let db =
            rusqlite::Connection::open(f.root.join("public_reactions/reactions.sqlite3")).unwrap();
        db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        db.execute_batch(sql).unwrap();
        // The node remains open: startup verification cannot protect this retry.
        assert!(
            f.node
                .set_reaction(&f.author.id, &f.object, like(), 0, "on")
                .is_err(),
            "accepted {sql}"
        );
        let receipts: u64 = db
            .query_row("SELECT count(*) FROM receipts", [], |r| r.get(0))
            .unwrap();
        assert_eq!(receipts, 2);
    }
}

#[test]
fn reactions_exhaustive_axes_and_public_attribution_survive_reopen() {
    let mut f = Fixture::new();
    let other_value = ReactionValue {
        appreciation: Some(Appreciation::Dislike),
        engagement: Some(Engagement::NotEngaging),
        stance: Some(Stance::Uncertain),
        certainty: Some(0),
    };
    f.node
        .set_reaction(&f.other.id, &f.object, other_value, 0, "other")
        .unwrap();
    let mut revision = 0;
    let mut previous = ReactionValue::default();
    let mut n = 0;
    for appreciation in [None, Some(Appreciation::Like), Some(Appreciation::Dislike)] {
        for engagement in [
            None,
            Some(Engagement::Engaging),
            Some(Engagement::NotEngaging),
        ] {
            for stance in [
                None,
                Some(Stance::Support),
                Some(Stance::Oppose),
                Some(Stance::Uncertain),
            ] {
                for certainty in [None, Some(0), Some(100)] {
                    if stance.is_none() && certainty.is_some() {
                        continue;
                    }
                    let value = ReactionValue {
                        appreciation,
                        engagement,
                        stance,
                        certainty,
                    };
                    let state = f
                        .node
                        .set_reaction(
                            &f.author.id,
                            &f.object,
                            value.clone(),
                            revision,
                            &format!("case-{n}"),
                        )
                        .unwrap();
                    if previous != value {
                        revision += 1;
                    }
                    assert_eq!(state.revision, revision);
                    assert_eq!(state.value, value);
                    let expected = ReactionSummary {
                        object_id: f.object.clone(),
                        participants: 1 + u64::from(!value.is_empty()),
                        likes: u64::from(appreciation == Some(Appreciation::Like)),
                        dislikes: 1 + u64::from(appreciation == Some(Appreciation::Dislike)),
                        engaging: u64::from(engagement == Some(Engagement::Engaging)),
                        not_engaging: 1 + u64::from(engagement == Some(Engagement::NotEngaging)),
                        support: u64::from(stance == Some(Stance::Support)),
                        oppose: u64::from(stance == Some(Stance::Oppose)),
                        uncertain: 1 + u64::from(stance == Some(Stance::Uncertain)),
                        certainty_responses: 1 + u64::from(certainty.is_some()),
                    };
                    assert_eq!(f.node.reaction_summary(&f.object).unwrap(), expected);
                    previous = value;
                    n += 1;
                }
            }
        }
    }
    f.reopen();
    let record = f.node.reaction_record(&f.author.id, &f.object).unwrap();
    assert_eq!(record.state.revision, revision);
    assert_eq!(record.state.value, previous);
    record.action.unwrap().verify(&f.author).unwrap();
    assert_eq!(f.node.reaction_summary(&f.object).unwrap().participants, 2);
}

#[test]
fn reactions_durable_retry_cas_noops_withdrawal_rotation_and_object_isolation() {
    let mut f = Fixture::new();
    let other_object = f
        .node
        .publish_text(&f.other.id, "second Object")
        .unwrap()
        .id;
    let absent = f.node.reaction_record(&f.author.id, &f.object).unwrap();
    assert_eq!(absent.state.revision, 0);
    assert!(absent.action.is_none());
    let on = f
        .node
        .set_reaction(&f.author.id, &f.object, like(), 0, "on")
        .unwrap();
    assert_eq!(
        f.node
            .set_reaction(&f.author.id, &f.object, like(), 1, "noop")
            .unwrap(),
        on
    );
    assert!(matches!(
        f.node.set_reaction(
            &f.author.id,
            &f.object,
            Default::default(),
            0,
            "stale-device"
        ),
        Err(Error::Conflict(_))
    ));
    f.node
        .rotate_identity_key(&f.author.id, IdentityKeyScope::Root, None, "rotate")
        .unwrap();
    let off = f
        .node
        .set_reaction(&f.author.id, &f.object, Default::default(), 1, "off")
        .unwrap();
    assert_eq!(off.revision, 2);
    f.node
        .set_reaction(&f.author.id, &other_object, like(), 0, "other-object")
        .unwrap();
    f.reopen();
    assert_eq!(
        f.node
            .set_reaction(&f.author.id, &f.object, like(), 0, "on")
            .unwrap(),
        on
    );
    assert_eq!(
        f.node
            .set_reaction(&f.author.id, &f.object, like(), 1, "noop")
            .unwrap(),
        on
    );
    assert_eq!(f.node.reaction_state(&f.author.id, &f.object).unwrap(), off);
    assert_eq!(f.node.reaction_summary(&f.object).unwrap().participants, 0);
    assert_eq!(f.node.reaction_summary(&other_object).unwrap().likes, 1);
    let record = f.node.reaction_record(&f.author.id, &f.object).unwrap();
    record
        .action
        .unwrap()
        .verify(&f.node.signing_identity(&f.author.id).unwrap())
        .unwrap();
    assert!(matches!(
        f.node
            .set_reaction(&f.author.id, &other_object, like(), 0, "on"),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        f.node
            .set_reaction(&f.author.id, &f.object, like(), 1, "on"),
        Err(Error::Conflict(_))
    ));
}

#[test]
fn reactions_unknown_ids_missing_local_key_and_recovery_are_rejected() {
    let mut f = Fixture::new();
    let unknown_author = IdentityId::new_unchecked(format!("id_{}", "0".repeat(64)));
    let unknown_object = ObjectId::new_unchecked(format!("obj_{}", "0".repeat(64)));
    for author in [unknown_author, IdentityId::new_unchecked("bad")] {
        assert!(f.node.reaction_state(&author, &f.object).is_err());
        assert!(f.node.reaction_record(&author, &f.object).is_err());
        assert!(
            f.node
                .set_reaction(&author, &f.object, like(), 0, "bad-author")
                .is_err()
        );
    }
    for object in [unknown_object, ObjectId::new_unchecked("bad")] {
        assert!(f.node.reaction_state(&f.author.id, &object).is_err());
        assert!(f.node.reaction_record(&f.author.id, &object).is_err());
        assert!(f.node.reaction_summary(&object).is_err());
        assert!(
            f.node
                .set_reaction(&f.author.id, &object, like(), 0, "bad-object")
                .is_err()
        );
    }
    let remote = Identity::create(IdentityKind::Person, "remote", &Keypair::generate()).unwrap();
    f.node
        .import_bundle(ImportBundle {
            identities: vec![remote.clone()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        f.node
            .reaction_state(&remote.id, &f.object)
            .unwrap()
            .revision,
        0
    );
    assert!(
        f.node
            .set_reaction(&remote.id, &f.object, like(), 0, "remote")
            .is_err()
    );
    std::fs::write(f.root.join(".publication-committed"), b"broken").unwrap();
    assert!(f.node.reaction_state(&f.author.id, &f.object).is_err());
    assert!(f.node.reaction_record(&f.author.id, &f.object).is_err());
    assert!(f.node.reaction_summary(&f.object).is_err());
    assert!(
        f.node
            .set_reaction(&f.author.id, &f.object, like(), 0, "recovery")
            .is_err()
    );
}

#[test]
fn reactions_do_not_enter_object_discovery_or_generic_event_log() {
    let mut f = Fixture::new();
    let before = f
        .node
        .list_events(babel_node::EventListQuery {
            after: None,
            limit: 100,
        })
        .unwrap()
        .events;
    f.node
        .set_reaction(&f.author.id, &f.object, like(), 0, "on")
        .unwrap();
    assert_eq!(
        before,
        f.node
            .list_events(babel_node::EventListQuery {
                after: None,
                limit: 100
            })
            .unwrap()
            .events
    );
    let store = babel_store::FileStore::open(&f.root).unwrap();
    assert_eq!(store.list_objects().unwrap().len(), 1);
    assert!(store.list_edges().unwrap().is_empty());
}
