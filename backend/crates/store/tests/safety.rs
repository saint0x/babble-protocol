use babel_crypto::Keypair;
use babel_graph::{
    SafetyAction, SafetyActionPayload, SafetyReceipt, SafetyReceiptPayload, SafetyRequest,
    SafetyState,
};
use babel_identity::{Identity, IdentityKind};
use babel_store::FileStore;
use babel_types::{Canonical, Error, Timestamp};
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
    target: Identity,
    key: Keypair,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babel-safety-store-{}-{}-{}",
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
        let target =
            Identity::create(IdentityKind::Person, "target", &Keypair::generate()).unwrap();
        store.put_identity(&author).unwrap();
        store.put_identity(&target).unwrap();
        Self {
            root,
            store,
            author,
            target,
            key,
        }
    }
    fn request(&self, blocked: bool, revision: u64, key: &str) -> SafetyRequest {
        SafetyRequest {
            author_id: self.author.id.clone(),
            target_id: self.target.id.clone(),
            blocked,
            muted: false,
            expected_revision: revision,
            idempotency_key: key.into(),
        }
    }
    fn verify(&self) -> babel_types::Result<()> {
        self.store.verify_safety_records(|id, _| {
            if id == &self.author.id {
                Ok(self.author.clone())
            } else if id == &self.target.id {
                Ok(self.target.clone())
            } else {
                Err(Error::NotFound("identity".into()))
            }
        })
    }
    fn connection(&self) -> Connection {
        Connection::open(self.root.join("private_safety/safety.sqlite3")).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn commit(
    store: &FileStore,
    request: &SafetyRequest,
    key: &Keypair,
) -> babel_types::Result<SafetyState> {
    commit_at(store, request, key, Timestamp::now())
}

fn commit_at(
    store: &FileStore,
    request: &SafetyRequest,
    key: &Keypair,
    created_at: Timestamp,
) -> babel_types::Result<SafetyState> {
    let identities = [
        store
            .get_identity(&request.author_id)?
            .ok_or(Error::Signature)?,
        store
            .get_identity(&request.target_id)?
            .ok_or(Error::Signature)?,
    ];
    store.commit_safety(
        request,
        |id, _| {
            identities
                .iter()
                .find(|identity| &identity.id == id)
                .cloned()
                .ok_or(Error::Signature)
        },
        |state, previous_id, changed, sequence, receipt_previous_id| {
            let action = changed
                .then(|| {
                    SafetyAction::sign(
                        SafetyActionPayload {
                            state: state.clone(),
                            previous_id,
                            created_at,
                            request_id: request.canonical_hash()?,
                        },
                        key,
                    )
                })
                .transpose()?;
            let receipt = SafetyReceipt::sign(
                SafetyReceiptPayload {
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
fn safety_restart_receipts_noops_cas_and_no_public_records() {
    let mut f = Fixture::new();
    let no = f.request(false, 0, "noop");
    assert_eq!(commit(&f.store, &no, &f.key).unwrap().revision, 0);
    let on = f.request(true, 0, "on");
    let original = commit(&f.store, &on, &f.key).unwrap();
    assert_eq!(original.revision, 1);
    assert!(matches!(
        commit(&f.store, &f.request(false, 0, "stale"), &f.key),
        Err(Error::Conflict(_))
    ));
    let off = f.request(false, 1, "off");
    assert_eq!(commit(&f.store, &off, &f.key).unwrap().revision, 2);
    f.store = FileStore::open(&f.root).unwrap();
    f.verify().unwrap();
    assert_eq!(commit(&f.store, &on, &f.key).unwrap(), original);
    assert!(
        !f.store
            .safety_state(&f.author.id, &f.target.id)
            .unwrap()
            .blocked
    );
    assert_eq!(f.store.safety_snapshot(&f.author.id).unwrap(), (2, vec![]));
    assert!(matches!(
        commit(&f.store, &f.request(false, 0, "on"), &f.key),
        Err(Error::Conflict(_))
    ));
    assert!(f.store.list_events().unwrap().is_empty());
    assert!(f.store.list_edges().unwrap().is_empty());
    assert!(f.store.list_objects().unwrap().is_empty());
    let counts: (u64, u64) = f
        .connection()
        .query_row(
            "SELECT (SELECT count(*) FROM actions),(SELECT count(*) FROM receipts)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(counts, (2, 3));
}

#[test]
fn safety_concurrent_cas_has_one_winner_and_duplicate_retries_one_receipt() {
    let f = Fixture::new();
    let barrier = Arc::new(Barrier::new(8));
    let handles = (0..8)
        .map(|n| {
            let store = f.store.clone();
            let key = f.key.clone();
            let request = f.request(true, 0, &format!("key-{n}"));
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                commit(&store, &request, &key)
            })
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .filter(|r| r.is_err())
            .all(|r| matches!(r, Err(Error::Conflict(_))))
    );
    let request = f.request(false, 1, "same-key");
    let handles = (0..8)
        .map(|_| {
            let store = f.store.clone();
            let key = f.key.clone();
            let request = request.clone();
            std::thread::spawn(move || commit(&store, &request, &key).unwrap())
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect::<Vec<_>>();
    assert!(
        results
            .iter()
            .all(|result| result == &results[0] && result.revision == 2)
    );
    let count: usize = f
        .connection()
        .query_row(
            "SELECT count(*) FROM receipts WHERE key='same-key'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
    f.verify().unwrap();
}

#[test]
fn safety_verification_rejects_action_state_receipt_and_version_tampering() {
    for sql in [
        "UPDATE actions SET record=json_set(record,'$.payload.state.blocked',0)",
        "UPDATE pairs SET blocked=0",
        "UPDATE receipts SET record=json_set(record,'$.payload.request.idempotency_key','forged')",
        "UPDATE versions SET version=20",
        "DELETE FROM pairs",
    ] {
        let f = Fixture::new();
        commit(&f.store, &f.request(true, 0, "key"), &f.key).unwrap();
        f.connection().execute_batch(sql).unwrap();
        assert!(f.verify().is_err(), "accepted tampering: {sql}");
    }
}

#[test]
fn safety_transaction_rolls_back_on_signing_failure_and_honors_recovery_guard() {
    let f = Fixture::new();
    assert!(
        f.store
            .commit_safety(
                &f.request(true, 0, "key"),
                |id, _| f.store.get_identity(id)?.ok_or(Error::Signature),
                |_, _, _, _, _| Err(Error::Signature)
            )
            .is_err()
    );
    assert_eq!(
        f.store
            .safety_state(&f.author.id, &f.target.id)
            .unwrap()
            .revision,
        0
    );
    commit(&f.store, &f.request(true, 0, "key"), &f.key).unwrap();
    std::fs::write(f.root.join(".publication-committed"), b"broken").unwrap();
    assert!(f.store.safety_state(&f.author.id, &f.target.id).is_err());
    assert!(commit(&f.store, &f.request(false, 1, "later"), &f.key).is_err());
}

#[cfg(unix)]
#[test]
fn safety_database_permissions_are_private() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    assert_eq!(
        std::fs::metadata(f.root.join("private_safety"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(f.root.join("private_safety/safety.sqlite3"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn safety_clock_rollback_rejects_changes_and_noops_before_commit() {
    let f = Fixture::new();
    let now = Timestamp::now();
    commit_at(&f.store, &f.request(true, 0, "on"), &f.key, now).unwrap();
    let earlier = Timestamp(now.0 - std::time::Duration::from_secs(1));
    for request in [f.request(false, 1, "off"), f.request(true, 1, "noop")] {
        assert!(matches!(
            commit_at(&f.store, &request, &f.key, earlier),
            Err(Error::StorageUnavailable(_))
        ));
    }
    assert_eq!(
        f.store
            .safety_state(&f.author.id, &f.target.id)
            .unwrap()
            .revision,
        1
    );
    let receipts: usize = f
        .connection()
        .query_row("SELECT count(*) FROM receipts", [], |row| row.get(0))
        .unwrap();
    assert_eq!(receipts, 1);
    f.verify().unwrap();
    commit_at(&f.store, &f.request(false, 1, "off"), &f.key, now).unwrap();
    f.verify().unwrap();
}

#[test]
fn safety_receipt_deletions_and_chain_head_tampering_fail_verification() {
    for sql in [
        "DELETE FROM receipts WHERE key='on'",
        "DELETE FROM receipts WHERE key='noop'",
        "DELETE FROM receipts WHERE key='tail-noop'",
        "DELETE FROM receipt_heads",
        "UPDATE receipt_heads SET sequence=1",
        "UPDATE receipts SET sequence=9 WHERE key='noop'",
    ] {
        let f = Fixture::new();
        commit(&f.store, &f.request(true, 0, "on"), &f.key).unwrap();
        commit(&f.store, &f.request(true, 1, "noop"), &f.key).unwrap();
        commit(&f.store, &f.request(false, 1, "off"), &f.key).unwrap();
        commit(&f.store, &f.request(false, 2, "tail-noop"), &f.key).unwrap();
        f.verify().unwrap();
        let connection = f.connection();
        connection
            .execute_batch("PRAGMA foreign_keys=OFF;")
            .unwrap();
        connection.execute_batch(sql).unwrap();
        assert!(f.verify().is_err(), "accepted receipt tamper: {sql}");
    }
}

#[test]
fn safety_active_bound_preserves_tombstones_and_always_allows_clearing() {
    let f = Fixture::new();
    let mut targets = vec![f.target.clone()];
    for n in 1..1001 {
        let target =
            Identity::create(IdentityKind::Person, &format!("target-{n}"), &f.key).unwrap();
        f.store.put_identity(&target).unwrap();
        targets.push(target);
    }
    for (n, target) in targets.iter().take(1000).enumerate() {
        let mut request = f.request(n % 2 == 0, 0, &format!("on-{n}"));
        request.target_id = target.id.clone();
        request.muted = n % 2 != 0;
        commit(&f.store, &request, &f.key).unwrap();
    }
    let (revision, states) = f.store.safety_snapshot(&f.author.id).unwrap();
    assert_eq!(revision, 1000);
    assert_eq!(states.len(), 1000);
    assert!(states.windows(2).all(|w| w[0].target_id < w[1].target_id));
    let mut extra = f.request(true, 0, "extra");
    extra.target_id = targets[1000].id.clone();
    assert!(matches!(
        commit(&f.store, &extra, &f.key),
        Err(Error::Conflict(_))
    ));
    let mut switch = f.request(false, 1, "switch");
    switch.muted = true;
    commit(&f.store, &switch, &f.key).unwrap();
    commit(&f.store, &f.request(false, 2, "clear"), &f.key).unwrap();
    commit(&f.store, &extra, &f.key).unwrap();
    assert_eq!(f.store.safety_snapshot(&f.author.id).unwrap().1.len(), 1000);
    assert_eq!(
        f.store
            .safety_state(&f.author.id, &f.target.id)
            .unwrap()
            .revision,
        3
    );
    f.store
        .verify_safety_records(|id, _| {
            if id == &f.author.id {
                Ok(f.author.clone())
            } else {
                targets
                    .iter()
                    .find(|identity| &identity.id == id)
                    .cloned()
                    .ok_or(Error::Signature)
            }
        })
        .unwrap();
}

#[test]
fn safety_wrong_signer_and_live_receipt_tampering_fail_closed() {
    let f = Fixture::new();
    assert!(commit(&f.store, &f.request(true, 0, "wrong"), &Keypair::generate()).is_err());
    let request = f.request(true, 0, "on");
    commit(&f.store, &request, &f.key).unwrap();
    f.connection()
        .execute_batch("UPDATE receipts SET record=json_set(record,'$.payload.state.muted',1)")
        .unwrap();
    assert!(commit(&f.store, &request, &f.key).is_err());
}
