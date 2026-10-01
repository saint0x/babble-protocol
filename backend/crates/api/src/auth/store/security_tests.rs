use super::*;
use std::path::PathBuf;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("babel-auth-security-{}", random_token().unwrap())))
    }
    fn db(&self) -> PathBuf {
        self.0.join("auth/accounts.sqlite3")
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn account(store: &mut AuthStore, id: &str) -> (String, Principal) {
    store.create_account(id, "observed-hash").unwrap();
    let token = store.issue_verified(id, "observed-hash").unwrap().0;
    let principal = store.authenticate(&token).unwrap();
    (token, principal)
}

#[test]
fn security_verified_login_cannot_issue_after_password_change() {
    let root = Root::new();
    let mut store = AuthStore::open(&root.0).unwrap();
    let (token, actor) = account(&mut store, "owner");
    let observed = store.password_hash("owner").unwrap().unwrap();
    // A password worker has verified this snapshot while a second connection commits.
    let mut other = AuthStore::open(&root.0).unwrap();
    other
        .change_password(&actor, &observed, Some("new-hash"))
        .unwrap();
    assert!(store.issue_verified("owner", &observed).is_err());
    assert!(store.authenticate(&token).is_err());
    assert!(store.issue_verified("owner", "new-hash").is_ok());
    assert!(store.issue_verified("unknown", "new-hash").is_err());
}

#[test]
fn security_password_change_revokes_login_that_committed_first() {
    let root = Root::new();
    let mut store = AuthStore::open(&root.0).unwrap();
    let (_, actor) = account(&mut store, "owner");
    let concurrent = store.issue_verified("owner", "observed-hash").unwrap().0;
    store
        .change_password(&actor, "observed-hash", Some("new-hash"))
        .unwrap();
    assert!(store.authenticate(&concurrent).is_err());
}

#[test]
fn security_revocation_or_expiry_during_password_work_cannot_commit() {
    let root = Root::new();
    let mut store = AuthStore::open(&root.0).unwrap();
    let (_, actor) = account(&mut store, "owner");
    let peer_token = store.issue_verified("owner", "observed-hash").unwrap().0;
    let peer = store.authenticate(&peer_token).unwrap();
    store.revoke_sessions(&peer, None).unwrap();
    assert_eq!(
        store
            .change_password(&actor, "observed-hash", Some("new-hash"))
            .unwrap_err()
            .code(),
        "unauthorized"
    );
    assert!(store.revoke_sessions(&actor, None).is_err());
    assert!(store.sessions(&actor).is_err());
    store
        .db
        .execute("UPDATE sessions SET expires_at=0", [])
        .unwrap();
    assert!(
        store
            .change_password(&peer, "observed-hash", Some("new-hash"))
            .is_err()
    );
    assert!(store.sessions(&peer).is_err());
    assert_eq!(
        store.password_hash("owner").unwrap().as_deref(),
        Some("observed-hash")
    );
}

#[test]
fn security_password_cas_and_wrong_password_preserve_sessions() {
    let root = Root::new();
    let mut store = AuthStore::open(&root.0).unwrap();
    let (token, actor) = account(&mut store, "owner");
    assert_eq!(
        store
            .change_password(&actor, "observed-hash", None)
            .unwrap_err()
            .code(),
        "forbidden"
    );
    assert_eq!(
        store
            .change_password(&actor, "stale-hash", Some("new-hash"))
            .unwrap_err()
            .code(),
        "forbidden"
    );
    assert!(store.authenticate(&token).is_ok());
    assert_eq!(
        store.password_hash("owner").unwrap().as_deref(),
        Some("observed-hash")
    );
}

#[test]
fn security_transaction_fault_rolls_back_password_and_partial_revocation() {
    let root = Root::new();
    let mut store = AuthStore::open(&root.0).unwrap();
    let (token, actor) = account(&mut store, "owner");
    let second = store.issue_verified("owner", "observed-hash").unwrap().0;
    // Abort only after DELETE has started, including after UPDATE accounts.
    store.db.execute_batch("CREATE TRIGGER fail_revoke AFTER DELETE ON sessions BEGIN SELECT RAISE(ABORT, 'injected'); END;").unwrap();
    assert!(
        store
            .change_password(&actor, "observed-hash", Some("new-hash"))
            .is_err()
    );
    assert!(store.revoke_sessions(&actor, None).is_err());
    assert_eq!(
        store.password_hash("owner").unwrap().as_deref(),
        Some("observed-hash")
    );
    drop(store);
    let mut store = AuthStore::open(&root.0).unwrap();
    assert!(store.authenticate(&token).is_ok());
    assert!(store.authenticate(&second).is_ok());
    store.db.execute_batch("DROP TRIGGER fail_revoke").unwrap();
    store
        .change_password(&actor, "observed-hash", Some("new-hash"))
        .unwrap();
    drop(store);
    let mut store = AuthStore::open(&root.0).unwrap();
    assert!(store.authenticate(&token).is_err());
    assert!(store.authenticate(&second).is_err());
    assert!(store.issue_verified("owner", "observed-hash").is_err());
    assert!(store.issue_verified("owner", "new-hash").is_ok());
}

#[test]
fn security_public_ids_are_private_stable_and_revocation_is_owner_scoped() {
    let root = Root::new();
    let mut store = AuthStore::open(&root.0).unwrap();
    let (token, owner) = account(&mut store, "owner");
    let (foreign_token, foreign) = account(&mut store, "foreign");
    let foreign_id = store.sessions(&foreign).unwrap()[0].id.clone();
    let id = store.sessions(&owner).unwrap()[0].id.clone();
    assert_eq!(id.len(), 72);
    assert!(id.starts_with("account_"));
    assert!(
        id[8..]
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    );
    assert_ne!(&id[8..], token);
    assert_ne!(&id[8..], digest(&token));
    assert!(store.sessions(&owner).unwrap()[0].created_at.is_some());
    assert!(
        store
            .revoke_sessions(&owner, Some(&foreign_id))
            .unwrap()
            .is_empty()
    );
    assert!(
        store
            .revoke_sessions(&owner, Some("unknown"))
            .unwrap()
            .is_empty()
    );
    drop(store);
    let mut store = AuthStore::open(&root.0).unwrap();
    assert_eq!(store.sessions(&owner).unwrap()[0].id, id);
    assert!(store.authenticate(&foreign_token).is_ok());
    assert_eq!(store.revoke_sessions(&owner, Some(&id)).unwrap().len(), 1);
    assert!(store.authenticate(&token).is_err());
    assert!(store.authenticate(&foreign_token).is_ok());
}

#[test]
fn security_sessions_cap_current_marker_and_revoke_others() {
    let root = Root::new();
    let mut store = AuthStore::open(&root.0).unwrap();
    let (first, _) = account(&mut store, "owner");
    let tokens: Vec<_> = (0..20)
        .map(|_| store.issue_verified("owner", "observed-hash").unwrap().0)
        .collect();
    assert!(store.authenticate(&first).is_err());
    for token in &tokens[..4] {
        assert!(store.authenticate(token).is_err());
    }
    let actor = store.authenticate(&tokens[19]).unwrap();
    let sessions = store.sessions(&actor).unwrap();
    assert_eq!(sessions.len(), 16);
    assert_eq!(sessions.iter().filter(|s| s.current).count(), 1);
    assert!(sessions[0].current);
    assert_eq!(store.revoke_sessions(&actor, None).unwrap().len(), 15);
    assert_eq!(store.sessions(&actor).unwrap().len(), 1);
    assert!(store.revoke_sessions(&actor, None).unwrap().is_empty());
}

#[test]
fn security_legacy_migration_preserves_credentials_null_dates_and_origin_binding() {
    let root = Root::new();
    fs::create_dir_all(root.0.join("auth")).unwrap();
    let db = Connection::open(root.db()).unwrap();
    db.execute_batch("CREATE TABLE sessions (token_hash TEXT PRIMARY KEY, identity_id TEXT NOT NULL, expires_at INTEGER NOT NULL);").unwrap();
    let token = random_token().unwrap();
    let expires = OffsetDateTime::now_utc().unix_timestamp() + 600;
    db.execute(
        "INSERT INTO sessions VALUES (?1,'owner',?2)",
        params![digest(&token), expires],
    )
    .unwrap();
    drop(db);
    let mut store = AuthStore::open(&root.0).unwrap();
    let actor = store.authenticate(&token).unwrap();
    let info = store.sessions(&actor).unwrap().remove(0);
    assert!(info.created_at.is_none());
    assert!(info.current);
    assert_eq!(actor.expires_at, expires);
    assert_eq!(actor.account_session, digest(&token));
    store
        .reserve_surface("surf_legacy", &actor, "object")
        .unwrap();
    drop(store);
    let mut store = AuthStore::open(&root.0).unwrap();
    assert_eq!(store.sessions(&actor).unwrap()[0].id, info.id);
    assert!(store.sessions(&actor).unwrap()[0].created_at.is_none());
    assert!(
        store
            .owns_surface("surf_legacy", &actor, Some("object"), false)
            .unwrap()
    );
    store.revoke_sessions(&actor, Some(&info.id)).unwrap();
    assert_eq!(
        store.surface_cleanup_batch("").unwrap(),
        vec![("surf_legacy".into(), true)]
    );
}

#[test]
fn security_failed_migration_leaves_legacy_tokens_and_can_retry() {
    let root = Root::new();
    fs::create_dir_all(root.0.join("auth")).unwrap();
    let db = Connection::open(root.db()).unwrap();
    db.execute_batch("CREATE TABLE sessions (token_hash TEXT PRIMARY KEY, identity_id TEXT NOT NULL, expires_at INTEGER NOT NULL);
        CREATE TABLE surface_owners (session_id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, object_id TEXT NOT NULL, retired INTEGER NOT NULL);").unwrap();
    let token = random_token().unwrap();
    db.execute(
        "INSERT INTO sessions VALUES (?1,'owner',?2)",
        params![
            digest(&token),
            OffsetDateTime::now_utc().unix_timestamp() + 600
        ],
    )
    .unwrap();
    // The later Surface migration fails after the session table rebuild.
    assert!(AuthStore::open(&root.0).is_err());
    let columns = db
        .prepare("PRAGMA table_info(sessions)")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!columns.contains(&"public_id".into()));
    db.execute_batch("DROP TABLE surface_owners").unwrap();
    drop(db);
    let store = AuthStore::open(&root.0).unwrap();
    assert!(store.authenticate(&token).is_ok());
}

#[test]
fn security_concurrent_password_changes_have_exactly_one_commit() {
    let root = Root::new();
    let mut store = AuthStore::open(&root.0).unwrap();
    let (_, actor) = account(&mut store, "owner");
    let peer = store.issue_verified("owner", "observed-hash").unwrap().0;
    let peer = store.authenticate(&peer).unwrap();
    let other = AuthStore::open(&root.0).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let gate = barrier.clone();
    let thread = std::thread::spawn(move || {
        let mut other = other;
        gate.wait();
        other
            .change_password(&peer, "observed-hash", Some("peer-hash"))
            .is_ok()
    });
    barrier.wait();
    let success = store
        .change_password(&actor, "observed-hash", Some("actor-hash"))
        .is_ok();
    assert_ne!(success, thread.join().unwrap());
    assert!(store.authenticate_hash(&actor.account_session).is_err());
}
