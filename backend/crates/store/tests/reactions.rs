use babble_crypto::Keypair;
use babble_graph::{
    Appreciation, Engagement, ReactionAction, ReactionActionPayload, ReactionReceipt,
    ReactionReceiptPayload, ReactionRequest, ReactionState, ReactionValue, Stance,
};
use babble_identity::{Identity, IdentityKind};
use babble_store::FileStore;
use babble_types::{Canonical, Error, ObjectId, Timestamp};
use rusqlite::Connection;
use std::{
    path::PathBuf,
    sync::{
        Arc, Barrier,
        atomic::{AtomicU64, Ordering},
    },
};

struct Fixture {
    root: PathBuf,
    store: FileStore,
    author: Identity,
    key: Keypair,
    object: ObjectId,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babble-reactions-store-{}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let store = FileStore::open(&root).unwrap();
        let key = Keypair::generate();
        let author = Identity::create(IdentityKind::Person, "author", &key).unwrap();
        store.put_identity(&author).unwrap();
        let object = ObjectId::new_unchecked(format!("obj_{}", "1".repeat(64)));
        Self {
            root,
            store,
            author,
            key,
            object,
        }
    }
    fn request(&self, value: ReactionValue, revision: u64, key: &str) -> ReactionRequest {
        ReactionRequest {
            author_id: self.author.id.clone(),
            object_id: self.object.clone(),
            value,
            expected_revision: revision,
            idempotency_key: key.into(),
        }
    }
    fn verify(&self) -> babble_types::Result<()> {
        self.store.verify_reaction_records(
            |id, _| {
                if id == &self.author.id {
                    Ok(self.author.clone())
                } else {
                    Err(Error::NotFound("identity".into()))
                }
            },
            |id, _| {
                if id == &self.object {
                    Ok(())
                } else {
                    Err(Error::NotFound("object".into()))
                }
            },
        )
    }
    fn connection(&self) -> Connection {
        Connection::open(self.root.join("public_reactions/reactions.sqlite3")).unwrap()
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
fn commit(
    store: &FileStore,
    request: &ReactionRequest,
    key: &Keypair,
) -> babble_types::Result<ReactionState> {
    commit_at(store, request, key, Timestamp::now())
}
fn commit_at(
    store: &FileStore,
    request: &ReactionRequest,
    key: &Keypair,
    created_at: Timestamp,
) -> babble_types::Result<ReactionState> {
    let author = store
        .get_identity(&request.author_id)?
        .ok_or_else(|| Error::NotFound("identity".into()))?;
    store.commit_reaction(
        request,
        |_, _| Ok(author.clone()),
        |state, previous_id, changed, sequence, receipt_previous_id| {
            let action = changed
                .then(|| {
                    ReactionAction::sign(
                        ReactionActionPayload {
                            state: state.clone(),
                            previous_id,
                            created_at,
                            request_id: request.canonical_hash()?,
                        },
                        key,
                    )
                })
                .transpose()?;
            let receipt = ReactionReceipt::sign(
                ReactionReceiptPayload {
                    request: request.clone(),
                    state: state.clone(),
                    created_at,
                    sequence,
                    previous_id: receipt_previous_id,
                },
                key,
            )?;
            Ok((action, receipt))
        },
    )
}

#[test]
fn reactions_restart_lost_response_noops_and_withdrawal() {
    let mut f = Fixture::new();
    let empty = f.request(Default::default(), 0, "empty");
    assert_eq!(commit(&f.store, &empty, &f.key).unwrap().revision, 0);
    assert!(
        f.store
            .reaction_record(&f.author.id, &f.object)
            .unwrap()
            .action
            .is_none()
    );
    let original = f.request(like(), 0, "on");
    let on = commit(&f.store, &original, &f.key).unwrap();
    assert_eq!(
        commit(&f.store, &f.request(like(), 1, "noop"), &f.key).unwrap(),
        on
    );
    let off = commit(&f.store, &f.request(Default::default(), 1, "off"), &f.key).unwrap();
    assert_eq!(off.revision, 2);
    f.store = FileStore::open(&f.root).unwrap();
    f.verify().unwrap();
    assert_eq!(commit(&f.store, &original, &f.key).unwrap(), on);
    assert_eq!(commit(&f.store, &empty, &f.key).unwrap().revision, 0);
    let record = f.store.reaction_record(&f.author.id, &f.object).unwrap();
    assert_eq!(record.state, off);
    record.action.unwrap().verify(&f.author).unwrap();
    assert_eq!(f.store.reaction_summary(&f.object).unwrap().participants, 0);
    assert!(matches!(
        commit(&f.store, &f.request(Default::default(), 0, "on"), &f.key),
        Err(Error::Conflict(_))
    ));
    assert!(f.store.list_objects().unwrap().is_empty());
    assert!(f.store.list_events().unwrap().is_empty());
    let counts: (usize, usize) = f
        .connection()
        .query_row(
            "SELECT (SELECT count(*) FROM actions),(SELECT count(*) FROM receipts)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (2, 4));
}

#[test]
fn reactions_all_axes_and_nullable_zero_certainty_have_independent_counts() {
    let f = Fixture::new();
    let values = [
        ReactionValue {
            appreciation: Some(Appreciation::Like),
            engagement: Some(Engagement::Engaging),
            stance: Some(Stance::Support),
            certainty: Some(0),
        },
        ReactionValue {
            appreciation: Some(Appreciation::Dislike),
            engagement: Some(Engagement::NotEngaging),
            stance: Some(Stance::Oppose),
            certainty: Some(100),
        },
        ReactionValue {
            stance: Some(Stance::Uncertain),
            ..Default::default()
        },
        ReactionValue::default(),
    ];
    for (i, value) in values.iter().enumerate() {
        commit(
            &f.store,
            &f.request(value.clone(), i as u64, &format!("axis-{i}")),
            &f.key,
        )
        .unwrap();
        let s = f.store.reaction_summary(&f.object).unwrap();
        assert_eq!(s.participants, u64::from(i != 3));
        assert_eq!(
            [
                s.likes,
                s.dislikes,
                s.engaging,
                s.not_engaging,
                s.support,
                s.oppose,
                s.uncertain,
                s.certainty_responses
            ],
            match i {
                0 => [1, 0, 1, 0, 1, 0, 0, 1],
                1 => [0, 1, 0, 1, 0, 1, 0, 1],
                2 => [0, 0, 0, 0, 0, 0, 1, 0],
                _ => [0; 8],
            }
        );
        f.verify().unwrap();
    }
}

#[test]
fn reactions_cas_concurrent_devices_and_duplicate_retry_have_one_winner() {
    let f = Fixture::new();
    let barrier = Arc::new(Barrier::new(8));
    let workers = (0..8)
        .map(|i| {
            let (store, key, barrier) = (f.store.clone(), f.key.clone(), barrier.clone());
            let request = f.request(like(), 0, &format!("device-{i}"));
            std::thread::spawn(move || {
                barrier.wait();
                commit(&store, &request, &key)
            })
        })
        .collect::<Vec<_>>();
    let results = workers
        .into_iter()
        .map(|w| w.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .filter(|r| r.is_err())
            .all(|r| matches!(r, Err(Error::Conflict(_))))
    );
    let workers = (0..8)
        .map(|_| {
            let (store, key) = (f.store.clone(), f.key.clone());
            let request = f.request(Default::default(), 1, "same-withdraw");
            std::thread::spawn(move || commit(&store, &request, &key).unwrap())
        })
        .collect::<Vec<_>>();
    for worker in workers {
        assert_eq!(worker.join().unwrap().revision, 2);
    }
    assert_eq!(f.store.reaction_summary(&f.object).unwrap().participants, 0);
    let count: u64 = f
        .connection()
        .query_row("SELECT count(*) FROM receipts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);
    f.verify().unwrap();
}

#[test]
fn reactions_tampering_deletion_substitution_and_current_last_action_fail_closed() {
    for sql in [
        "UPDATE actions SET record=json_set(record,'$.payload.state.value.certainty',90)",
        "UPDATE pairs SET value='{}'",
        "UPDATE pairs SET revision=1",
        "DELETE FROM pairs",
        "DELETE FROM actions WHERE revision=2",
        "DELETE FROM actions WHERE revision=1",
        "UPDATE actions SET receipt_id=(SELECT id FROM receipts WHERE key='noop') WHERE revision=1",
        "UPDATE actions SET record=(SELECT record FROM actions WHERE revision=1) WHERE revision=2",
        "DELETE FROM receipts WHERE key='on'",
        "DELETE FROM receipts WHERE key='noop'",
        "DELETE FROM receipts WHERE key='tail'",
        "UPDATE receipts SET record=json_set(record,'$.payload.request.idempotency_key','forged')",
        "UPDATE receipts SET record=(SELECT record FROM receipts WHERE key='on') WHERE key='off'",
        "UPDATE receipts SET object='forged' WHERE key='tail'",
        "DELETE FROM receipt_heads",
        "UPDATE receipt_heads SET sequence=1",
        "UPDATE summaries SET record=json_set(record,'$.likes',10)",
        "DELETE FROM summaries",
        "UPDATE summaries SET object='forged'",
        "DROP TABLE actions",
    ] {
        let f = Fixture::new();
        for (value, rev, key) in [
            (like(), 0, "on"),
            (like(), 1, "noop"),
            (Default::default(), 1, "off"),
            (Default::default(), 2, "tail"),
        ] {
            commit(&f.store, &f.request(value, rev, key), &f.key).unwrap();
        }
        f.verify().unwrap();
        let db = f.connection();
        db.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
        db.execute_batch(sql).unwrap();
        assert!(f.verify().is_err(), "accepted tamper: {sql}");
    }
}

#[test]
fn reactions_roll_back_all_tables_on_late_sql_failure_and_signing_failure() {
    let f = Fixture::new();
    let request = f.request(like(), 0, "key");
    assert!(
        f.store
            .commit_reaction(
                &request,
                |_, _| Ok(f.author.clone()),
                |_, _, _, _, _| Err(Error::Signature)
            )
            .is_err()
    );
    f.connection().execute_batch("CREATE TRIGGER fail_receipt BEFORE INSERT ON receipt_heads BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(commit(&f.store, &request, &f.key).is_err());
    for table in ["actions", "pairs", "receipts", "receipt_heads", "summaries"] {
        let count: usize = f
            .connection()
            .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0, "{table} survived rollback");
    }
    f.connection()
        .execute_batch("DROP TRIGGER fail_receipt")
        .unwrap();
    commit(&f.store, &request, &f.key).unwrap();
    f.verify().unwrap();
    std::fs::write(f.root.join(".publication-committed"), b"broken").unwrap();
    assert!(f.store.reaction_state(&f.author.id, &f.object).is_err());
    assert!(f.store.reaction_record(&f.author.id, &f.object).is_err());
    assert!(f.store.reaction_summary(&f.object).is_err());
    assert!(commit(&f.store, &request, &f.key).is_err());
}

#[test]
fn reactions_clock_rollback_and_deleted_database_fail_closed() {
    let f = Fixture::new();
    let now = Timestamp::now();
    commit_at(&f.store, &f.request(like(), 0, "on"), &f.key, now).unwrap();
    let earlier = Timestamp(now.0 - std::time::Duration::from_secs(1));
    for (value, key) in [(like(), "noop"), (Default::default(), "off")] {
        assert!(matches!(
            commit_at(&f.store, &f.request(value, 1, key), &f.key, earlier),
            Err(Error::StorageUnavailable(_))
        ));
    }
    f.verify().unwrap();
    std::fs::remove_file(f.root.join("public_reactions/reactions.sqlite3")).unwrap();
    assert!(FileStore::open(&f.root).is_err());
    assert!(f.store.reaction_summary(&f.object).is_err());
}

#[test]
fn reactions_serde_requires_every_axis_and_rejects_unknown_or_invalid_values() {
    let canonical =
        serde_json::json!({"appreciation":null,"engagement":null,"stance":null,"certainty":null});
    assert_eq!(
        serde_json::to_value(ReactionValue::default()).unwrap(),
        canonical
    );
    for field in ["appreciation", "engagement", "stance", "certainty"] {
        let mut missing = canonical.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<ReactionValue>(missing).is_err(),
            "accepted absent {field}"
        );
    }
    for (field, invalid) in [
        ("unknown", serde_json::json!(null)),
        ("appreciation", serde_json::json!("love")),
        ("certainty", serde_json::json!(0.5)),
        ("certainty", serde_json::json!(-1)),
        ("certainty", serde_json::json!(256)),
    ] {
        let mut value = canonical.clone();
        value[field] = invalid;
        assert!(serde_json::from_value::<ReactionValue>(value).is_err());
    }
    let f = Fixture::new();
    for value in [
        ReactionValue {
            certainty: Some(0),
            ..Default::default()
        },
        ReactionValue {
            stance: Some(Stance::Support),
            certainty: Some(101),
            ..Default::default()
        },
    ] {
        assert!(commit(&f.store, &f.request(value, 0, "invalid"), &f.key).is_err());
    }
    for key in ["", "with space", "\n", &"k".repeat(257)] {
        assert!(commit(&f.store, &f.request(like(), 0, key), &f.key).is_err());
    }
    assert!(
        commit(
            &f.store,
            &f.request(like(), 9_007_199_254_740_992, "revision"),
            &f.key
        )
        .is_err()
    );
}

#[test]
fn reactions_live_summary_rejects_out_of_range_and_impossible_axis_counts() {
    for change in [
        "'$.participants',9007199254740992",
        "'$.likes',2",
        "'$.certainty_responses',1",
        "'$.dislikes',1",
        "'$.participants',2",
        "'$.likes',-1",
        "'$.likes',0.5",
    ] {
        let f = Fixture::new();
        commit(&f.store, &f.request(like(), 0, "on"), &f.key).unwrap();
        f.connection()
            .execute_batch(&format!(
                "UPDATE summaries SET record=json_set(record,{change})"
            ))
            .unwrap();
        assert!(
            f.store.reaction_summary(&f.object).is_err(),
            "accepted {change}"
        );
    }
    let f = Fixture::new();
    commit(&f.store, &f.request(like(), 0, "on"), &f.key).unwrap();
    f.connection()
        .execute_batch("DELETE FROM summaries")
        .unwrap();
    assert!(f.store.reaction_summary(&f.object).is_err());
}
