use super::*;
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};

#[tokio::test]
async fn security_storage_failure_rolls_back_password_and_revocation_together() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let account = register(&app).await;
    let peer = login(&app, &account, PASSWORD).await;
    let before = sessions(&app, &account).await;
    let hash_before: String = fixture
        .db()
        .query_row(
            "SELECT password_hash FROM accounts WHERE identity_id=?1",
            [&account.id],
            |row| row.get(0),
        )
        .unwrap();
    // Fail after the UPDATE accounts statement but before session deletion can commit.
    fixture.db().execute_batch("CREATE TRIGGER reject_revocation BEFORE DELETE ON sessions BEGIN SELECT RAISE(ABORT, 'test injected failure'); END;").unwrap();
    let result = change(&app, &account, PASSWORD, REPLACEMENT).await;
    assert_eq!(result.0, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!result.1.to_string().contains("test injected failure"));
    assert!(!result.1.to_string().contains(&hash_before));
    let hash_after: String = fixture
        .db()
        .query_row(
            "SELECT password_hash FROM accounts WHERE identity_id=?1",
            [&account.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(hash_before, hash_after);
    assert_eq!(sessions(&app, &account).await, before);
    status(&app, &peer, StatusCode::OK).await;
    fixture
        .db()
        .execute_batch("DROP TRIGGER reject_revocation;")
        .unwrap();
    assert_eq!(
        change(&app, &account, PASSWORD, REPLACEMENT).await.0,
        StatusCode::NO_CONTENT
    );
    status(&app, &peer, StatusCode::UNAUTHORIZED).await;
}

#[tokio::test]
async fn security_concurrent_old_password_login_never_survives_password_commit() {
    let fixture = Fixture::new();
    let app = fixture.app();
    for _ in 0..3 {
        let account = register(&app).await;
        let (login_result, changed) = tokio::join!(
            request(
                &app,
                "POST",
                "/auth/login",
                None,
                json!({"identity_id":account.id,"password":PASSWORD})
            ),
            change(&app, &account, PASSWORD, REPLACEMENT)
        );
        assert_eq!(changed.0, StatusCode::NO_CONTENT, "{changed:?}");
        match login_result.0 {
            StatusCode::OK => {
                let issued = Account {
                    id: account.id.clone(),
                    token: login_result.1["token"].as_str().unwrap().to_owned(),
                };
                status(&app, &issued, StatusCode::UNAUTHORIZED).await;
            }
            StatusCode::UNAUTHORIZED => {}
            other => panic!("unexpected login status {other}"),
        }
        status(&app, &account, StatusCode::UNAUTHORIZED).await;
        let fresh = login(&app, &account, REPLACEMENT).await;
        assert_eq!(sessions(&app, &fresh).await.len(), 1);
    }
}

#[tokio::test]
async fn security_concurrent_password_changes_have_exactly_one_winner() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let account = register(&app).await;
    let peer = login(&app, &account, PASSWORD).await;
    let alternative = "a separate replacement password";
    let (first, second) = tokio::join!(
        change(&app, &account, PASSWORD, REPLACEMENT),
        change(&app, &peer, PASSWORD, alternative)
    );
    let (winner, loser, rejected) = if first.0 == StatusCode::NO_CONTENT {
        (REPLACEMENT, alternative, second.0)
    } else {
        assert_eq!(second.0, StatusCode::NO_CONTENT, "{first:?} / {second:?}");
        (alternative, REPLACEMENT, first.0)
    };
    assert!([StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN].contains(&rejected));
    status(&app, &account, StatusCode::UNAUTHORIZED).await;
    status(&app, &peer, StatusCode::UNAUTHORIZED).await;
    assert_eq!(
        request(
            &app,
            "POST",
            "/auth/login",
            None,
            json!({"identity_id":account.id,"password":loser})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    login(&app, &account, winner).await;
}

#[tokio::test]
async fn security_legacy_session_migration_preserves_credentials_without_inventing_creation_dates()
{
    let fixture = Fixture::new();
    let app = fixture.app();
    let account = register(&app).await;
    let second = login(&app, &account, PASSWORD).await;
    drop(app);
    // Recreate the exact pre-management schema, retaining credentials and expiry.
    // The migration must create IDs once, not derive them from the credentials.
    let db = fixture.db();
    db.execute_batch("BEGIN IMMEDIATE;
        CREATE TABLE legacy_sessions (token_hash TEXT PRIMARY KEY, identity_id TEXT NOT NULL, expires_at INTEGER NOT NULL);
        INSERT INTO legacy_sessions SELECT token_hash, identity_id, expires_at FROM sessions;
        DROP TABLE sessions;
        ALTER TABLE legacy_sessions RENAME TO sessions;
        CREATE INDEX sessions_identity ON sessions(identity_id);
        COMMIT;").unwrap();
    let legacy_password = "legacysecret12";
    let salt = SaltString::encode_b64(b"legacy-security-regression-salt").unwrap();
    let hash = Argon2::default()
        .hash_password(legacy_password.as_bytes(), &salt)
        .unwrap()
        .to_string();
    db.execute(
        "UPDATE accounts SET password_hash=?1 WHERE identity_id=?2",
        rusqlite::params![hash, account.id],
    )
    .unwrap();
    drop(db);
    let app = fixture.app();
    let list = sessions(&app, &account).await;
    assert_eq!(list.len(), 2);
    assert!(list.iter().all(|session| session["created_at"].is_null()));
    for session in &list {
        let serialized = session.to_string();
        for token in [&account.token, &second.token] {
            assert!(!serialized.contains(&blake3::hash(token.as_bytes()).to_hex().to_string()));
        }
    }
    drop(app);
    let app = fixture.app();
    assert_eq!(sessions(&app, &account).await, list);
    let legacy_login = login(&app, &account, legacy_password).await;
    assert!(
        !sessions(&app, &legacy_login)
            .await
            .iter()
            .find(|session| session["current"] == true)
            .unwrap()["created_at"]
            .is_null()
    );
    assert_eq!(
        change(&app, &legacy_login, legacy_password, REPLACEMENT)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    status(&app, &account, StatusCode::UNAUTHORIZED).await;
    status(&app, &second, StatusCode::UNAUTHORIZED).await;
}

#[tokio::test]
async fn security_expired_sessions_are_absent_and_cannot_manage_accounts() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let account = register(&app).await;
    let second = login(&app, &account, PASSWORD).await;
    let second_id = current_id(&sessions(&app, &second).await);
    fixture
        .db()
        .execute(
            "UPDATE sessions SET expires_at=0 WHERE token_hash=?1",
            [blake3::hash(second.token.as_bytes()).to_hex().to_string()],
        )
        .unwrap();
    let list = sessions(&app, &account).await;
    assert_eq!(list.len(), 1);
    assert_ne!(current_id(&list), second_id);
    for (method, uri, body) in [
        ("GET", "/auth/sessions", Value::Null),
        ("POST", "/auth/sessions/revoke-others", Value::Null),
        (
            "POST",
            "/auth/password",
            json!({"current_password":PASSWORD,"new_password":REPLACEMENT}),
        ),
    ] {
        assert_eq!(
            request(&app, method, uri, Some(&second.token), body)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    no_content(
        &app,
        "DELETE",
        &format!("/auth/sessions/{second_id}"),
        &account,
        Value::Null,
    )
    .await;
    status(&app, &account, StatusCode::OK).await;
}

#[tokio::test]
async fn security_active_session_cap_is_enforced_and_eviction_is_durable() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let first = register(&app).await;
    let mut accounts = vec![first.clone()];
    for _ in 0..17 {
        accounts.push(login(&app, &first, PASSWORD).await);
    }
    let latest = accounts.last().unwrap();
    assert_eq!(sessions(&app, latest).await.len(), 16);
    let mut live = Vec::new();
    for account in &accounts {
        let status = request(
            &app,
            "GET",
            "/auth/session",
            Some(&account.token),
            Value::Null,
        )
        .await
        .0;
        assert!([StatusCode::OK, StatusCode::UNAUTHORIZED].contains(&status));
        live.push(status);
    }
    assert_eq!(
        live.iter()
            .filter(|&&status| status == StatusCode::OK)
            .count(),
        16
    );
    assert_eq!(live.last(), Some(&StatusCode::OK));
    drop(app);
    let app = fixture.app();
    for (account, expected) in accounts.iter().zip(live) {
        status(&app, account, expected).await;
    }
}
