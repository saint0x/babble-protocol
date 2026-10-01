use super::*;

const OTHER: &str = "550e8400-e29b-41d4-a716-446655440001";
const HEADER: &str = "x-babel-surface-document";

#[tokio::test]
async fn document_bound_session_requires_fresh_runtime_after_restart() {
    let (fixture, app, alice, object, ids, session) = setup().await;
    register_document(&app, &alice, &session).await;
    drop(app);
    let app = fixture.app();
    assert_eq!(persisted(&fixture, &session).as_deref(), Some(DOCUMENT));
    assert_eq!(
        bind(&app, &alice, &session, DOCUMENT).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &app,
            &alice,
            bound(&object, &session, ids.clone()),
            &[(HEADER, DOCUMENT)]
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let restarted = request(
        &app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&alice.token),
        json!({"object_id":object,"role":"Feed","session_id":session}),
    )
    .await;
    assert_eq!(restarted.0, StatusCode::FORBIDDEN);
    assert_eq!(
        bind(&app, &alice, &session, OTHER).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(persisted(&fixture, &session).as_deref(), Some(DOCUMENT));
    let fresh = start(&app, &alice, &object).await;
    assert_ne!(fresh, session);
    assert_eq!(bind(&app, &alice, &fresh, OTHER).await.0, StatusCode::OK);
    let result = call(
        &app,
        &alice,
        bound(&object, &fresh, ids),
        &[(HEADER, OTHER)],
    )
    .await;
    assert_eq!(result.0, StatusCode::OK);
    assert!(result.1["error"].is_null(), "{}", result.1);
}

#[tokio::test]
async fn document_column_migration_leaves_existing_sessions_unbound() {
    let (fixture, app, alice, object, ids, session) = setup().await;
    drop(app);
    let db = rusqlite::Connection::open(fixture.root.join("auth/accounts.sqlite3")).unwrap();
    db.execute("ALTER TABLE surface_owners DROP COLUMN document_id", [])
        .unwrap();
    drop(db);
    let app = fixture.app();
    assert_eq!(persisted(&fixture, &session), None);
    let restarted = request(
        &app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&alice.token),
        json!({"object_id":object,"role":"Feed","session_id":session}),
    )
    .await;
    assert_eq!(restarted.0, StatusCode::OK);
    assert_eq!(
        call(
            &app,
            &alice,
            bound(&object, &session, ids.clone()),
            &[(HEADER, DOCUMENT)]
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    register_document(&app, &alice, &session).await;
    let result = call(
        &app,
        &alice,
        bound(&object, &session, ids),
        &[(HEADER, DOCUMENT)],
    )
    .await;
    assert_eq!(result.0, StatusCode::OK);
    assert!(result.1["error"].is_null());
}

async fn setup() -> (Fixture, Router, Account, String, Vec<String>, String) {
    let fixture = Fixture::new();
    let app = fixture.app();
    let account = register(&app, "document-owner").await;
    let target = publish(&app, &account, "document target").await;
    let (object, caps) = controller(&app, &account, &target, true).await;
    let grants = grants(&app, &account, &object, &caps).await;
    let session = start(&app, &account, &object).await;
    (fixture, app, account, object, grants, session)
}

async fn start(app: &Router, account: &Account, object: &str) -> String {
    let result = request(
        app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&account.token),
        json!({"object_id":object,"role":"Feed"}),
    )
    .await;
    assert_eq!(result.0, StatusCode::OK, "{}", result.1);
    result.1["session"]["id"].as_str().unwrap().to_owned()
}

async fn bind(
    app: &Router,
    account: &Account,
    session: &str,
    document: &str,
) -> (StatusCode, Value) {
    request(
        app,
        "PUT",
        &format!("/runtime/surfaces/sessions/{session}/document"),
        Some(&account.token),
        json!({"document_id":document}),
    )
    .await
}

async fn call(
    app: &Router,
    account: &Account,
    binding: RpcBinding,
    headers: &[(&str, &str)],
) -> (StatusCode, Value) {
    request_with_headers(
        app,
        "POST",
        "/rpc",
        Some(&account.token),
        envelope(
            "babel.storage.local.set.v1",
            binding,
            json!({"key":"document","value":true}),
        ),
        headers,
    )
    .await
}

fn persisted(fixture: &Fixture, session: &str) -> Option<String> {
    rusqlite::Connection::open(fixture.root.join("auth/accounts.sqlite3"))
        .unwrap()
        .query_row(
            "SELECT document_id FROM surface_owners WHERE session_id=?1",
            [session],
            |row| row.get(0),
        )
        .unwrap()
}

#[tokio::test]
async fn document_registration_is_immutable_and_scoped_to_originating_login() {
    let (fixture, app, alice, object, ids, session) = setup().await;
    let second = request(
        &app,
        "POST",
        "/auth/login",
        None,
        json!({"identity_id":alice.id,"password":PASSWORD}),
    )
    .await;
    let same_identity = Account {
        id: alice.id.clone(),
        token: second.1["token"].as_str().unwrap().into(),
    };
    let bob = register(&app, "document-other").await;
    for account in [&same_identity, &bob] {
        assert_eq!(
            bind(&app, account, &session, DOCUMENT).await.0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        request(
            &app,
            "PUT",
            &format!("/runtime/surfaces/sessions/{session}/document"),
            None,
            json!({"document_id":DOCUMENT})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(persisted(&fixture, &session), None);
    let (first, retry) = tokio::join!(
        bind(&app, &alice, &session, DOCUMENT),
        bind(&app, &alice, &session, DOCUMENT)
    );
    assert_eq!(first, retry);
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(
        first.1,
        json!({"session_id":session,"document_id":DOCUMENT})
    );
    assert_eq!(
        bind(&app, &alice, &session, OTHER).await.0,
        StatusCode::CONFLICT
    );
    for account in [&same_identity, &bob] {
        assert_eq!(
            bind(&app, account, &session, DOCUMENT).await.0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            call(
                &app,
                account,
                bound(&object, &session, ids.clone()),
                &[(HEADER, DOCUMENT)]
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(persisted(&fixture, &session).as_deref(), Some(DOCUMENT));
    let result = call(
        &app,
        &alice,
        bound(&object, &session, ids),
        &[(HEADER, DOCUMENT)],
    )
    .await;
    assert_eq!(result.0, StatusCode::OK, "{}", result.1);
    assert!(result.1["error"].is_null(), "{}", result.1);

    let competing = start(&app, &alice, &object).await;
    let (a, b) = tokio::join!(
        bind(&app, &alice, &competing, DOCUMENT),
        bind(&app, &alice, &competing, OTHER)
    );
    assert!(matches!(
        (a.0, b.0),
        (StatusCode::OK, StatusCode::CONFLICT) | (StatusCode::CONFLICT, StatusCode::OK)
    ));
    let winner = if a.0 == StatusCode::OK {
        DOCUMENT
    } else {
        OTHER
    };
    assert_eq!(persisted(&fixture, &competing).as_deref(), Some(winner));
}

#[tokio::test]
async fn document_headers_fail_closed_for_unregistered_missing_duplicate_malformed_and_forged_context()
 {
    let (fixture, app, alice, object, ids, session) = setup().await;
    let binding = bound(&object, &session, ids.clone());
    assert_eq!(
        call(&app, &alice, binding.clone(), &[(HEADER, DOCUMENT)])
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    for malformed in [
        "",
        " ",
        "550E8400-e29b-41d4-a716-446655440000",
        "550e8400e29b41d4a716446655440000",
        "550e8400-e29b-41d4-a716-44665544000g",
        "550e8400-e29b-41d4-a716-446655440000,550e8400-e29b-41d4-a716-446655440001",
        "550e8400-e29b-41d4-a716-446655440000 ",
        "550e8400-e29b-41d4-a716-44665544000é",
    ] {
        assert_eq!(
            bind(&app, &alice, &session, malformed).await.0,
            StatusCode::BAD_REQUEST,
            "{malformed}"
        );
        assert_eq!(
            call(&app, &alice, binding.clone(), &[(HEADER, malformed)])
                .await
                .0,
            StatusCode::BAD_REQUEST,
            "{malformed}"
        );
    }
    assert_eq!(persisted(&fixture, &session), None);
    register_document(&app, &alice, &session).await;
    assert_eq!(
        call(&app, &alice, binding.clone(), &[]).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(&app, &alice, binding.clone(), &[(HEADER, OTHER)])
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    for headers in [
        vec![(HEADER, DOCUMENT), (HEADER, DOCUMENT)],
        vec![(HEADER, DOCUMENT), ("X-Babel-Surface-Document", OTHER)],
    ] {
        assert_eq!(
            call(&app, &alice, binding.clone(), &headers).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    let different_object = publish(&app, &alice, "forged object").await;
    let other_session = start(&app, &alice, &object).await;
    assert_eq!(
        bind(&app, &alice, &other_session, OTHER).await.0,
        StatusCode::OK
    );
    for forged in [
        bound(&different_object, &session, ids.clone()),
        bound(&object, &other_session, ids.clone()),
        bound(&object, "surf_missing", ids.clone()),
    ] {
        assert_eq!(
            call(&app, &alice, forged, &[(HEADER, DOCUMENT)]).await.0,
            StatusCode::FORBIDDEN
        );
    }
    for mut absent in [host(), host_object(&object, ids.clone())] {
        assert_eq!(
            call(&app, &alice, absent.clone(), &[(HEADER, DOCUMENT)])
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
        absent.object_id = None;
        absent.surface_session_id = Some(session.clone());
        assert_eq!(
            call(&app, &alice, absent, &[(HEADER, DOCUMENT)]).await.0,
            StatusCode::BAD_REQUEST
        );
    }
    // Even public reads carrying both bindings require their document context.
    for (method, path, body) in [
        ("GET", "/health".to_owned(), Value::Null),
        (
            "PUT",
            format!("/runtime/surfaces/sessions/{session}/document"),
            json!({"document_id":DOCUMENT}),
        ),
        (
            "POST",
            "/rpc".to_owned(),
            envelope("babel.object.get.v1", host(), json!({"object_id":object})),
        ),
        (
            "POST",
            "/rpc".to_owned(),
            envelope(
                "babel.runtime.surface.session.get.v1",
                host(),
                json!({"session_id":session}),
            ),
        ),
    ] {
        assert_eq!(
            request_with_headers(
                &app,
                method,
                &path,
                Some(&alice.token),
                body,
                &[(HEADER, DOCUMENT)]
            )
            .await
            .0,
            StatusCode::BAD_REQUEST,
            "{method} {path}"
        );
    }
    let public = envelope(
        "babel.object.get.v1",
        binding.clone(),
        json!({"object_id":object}),
    );
    assert_eq!(
        request(&app, "POST", "/rpc", Some(&alice.token), public.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let result = request_with_headers(
        &app,
        "POST",
        "/rpc",
        Some(&alice.token),
        public,
        &[(HEADER, DOCUMENT)],
    )
    .await;
    assert_eq!(result.0, StatusCode::OK);
    assert!(result.1["error"].is_null(), "{}", result.1);
    // No document-registration method is exposed in the embedded RPC catalog.
    let mut delegated = envelope(
        "babel.runtime.surface.session.get.v1",
        binding,
        json!({"session_id":session,"document_id":OTHER}),
    );
    delegated["method"] = json!("babel.runtime.surface.session.document.v1");
    assert_eq!(
        request_with_headers(
            &app,
            "POST",
            "/rpc",
            Some(&alice.token),
            delegated,
            &[(HEADER, DOCUMENT)]
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let host_read = rpc(
        &app,
        &alice,
        "babel.runtime.surface.session.get.v1",
        host(),
        json!({"session_id":session}),
    )
    .await;
    assert_eq!(host_read.0, StatusCode::OK);
    assert!(host_read.1["error"].is_null());
    let host_write = call(&app, &alice, host_object(&object, ids), &[]).await;
    assert_eq!(host_write.0, StatusCode::OK);
    assert!(host_write.1["error"].is_null());
}

#[tokio::test]
async fn document_binding_survives_restart_suspend_revocation_and_cannot_revive_terminal_sessions()
{
    let (fixture, app, alice, object, ids, session) = setup().await;
    register_document(&app, &alice, &session).await;
    assert_eq!(
        bind(&app, &alice, &session, DOCUMENT).await.0,
        StatusCode::OK
    );
    assert_eq!(
        bind(&app, &alice, &session, OTHER).await.0,
        StatusCode::CONFLICT
    );
    let read = call(
        &app,
        &alice,
        bound(&object, &session, ids.clone()),
        &[(HEADER, DOCUMENT)],
    )
    .await;
    assert_eq!(read.0, StatusCode::OK);
    assert!(read.1["error"].is_null());
    for lifecycle in ["warm", "active", "suspended"] {
        assert_eq!(
            request(
                &app,
                "POST",
                &format!("/runtime/surfaces/sessions/{session}/lifecycle"),
                Some(&alice.token),
                json!({"lifecycle":lifecycle,"reason":"document regression"})
            )
            .await
            .0,
            StatusCode::OK
        );
        let expected = if lifecycle == "suspended" {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::OK
        };
        assert_eq!(bind(&app, &alice, &session, DOCUMENT).await.0, expected);
    }
    assert_eq!(
        bind(&app, &alice, &session, OTHER).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(persisted(&fixture, &session).as_deref(), Some(DOCUMENT));
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("/runtime/surfaces/sessions/{session}/lifecycle"),
            Some(&alice.token),
            json!({"lifecycle":"active","reason":"resume same document"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        bind(&app, &alice, &session, OTHER).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/capabilities/revocations",
            Some(&alice.token),
            json!({"author_id":alice.id,"object_id":object,"grant_id":ids[0]})
        )
        .await
        .0,
        StatusCode::OK
    );
    for document in [DOCUMENT, OTHER] {
        assert_eq!(
            bind(&app, &alice, &session, document).await.0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        call(
            &app,
            &alice,
            bound(&object, &session, ids),
            &[(HEADER, DOCUMENT)]
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    drop(app);
    let app = fixture.app();
    // Retire the old document even if restart preceded background cleanup.
    let restarted = request(
        &app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&alice.token),
        json!({"object_id":object,"role":"Feed","session_id":session}),
    )
    .await;
    assert_eq!(restarted.0, StatusCode::FORBIDDEN);
    assert_eq!(
        bind(&app, &alice, &session, DOCUMENT).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(persisted(&fixture, &session).as_deref(), Some(DOCUMENT));
}

#[tokio::test]
async fn document_registration_rejects_expired_leases_logins_and_evicted_sessions_after_restart() {
    let (fixture, app, alice, object, ids, _) = setup().await;
    let db = rusqlite::Connection::open(fixture.root.join("auth/accounts.sqlite3")).unwrap();
    for invalidation in ["lease", "evicted", "login"] {
        let session = start(&app, &alice, &object).await;
        register_document(&app, &alice, &session).await;
        let unbound = start(&app, &alice, &object).await;
        for id in [&session, &unbound] {
            match invalidation {
                "lease" => {
                    db.execute(
                        "UPDATE surface_owners SET lease_expires_at=0 WHERE session_id=?1",
                        [id],
                    )
                    .unwrap();
                }
                "evicted" => {
                    assert_eq!(
                        request(
                            &app,
                            "POST",
                            &format!("/runtime/surfaces/sessions/{id}/lifecycle"),
                            Some(&alice.token),
                            json!({"lifecycle":"evicted","reason":"document regression"})
                        )
                        .await
                        .0,
                        StatusCode::OK
                    );
                }
                _ => {
                    db.execute("UPDATE sessions SET expires_at=0", []).unwrap();
                }
            }
        }
        let expected = if invalidation == "login" {
            StatusCode::UNAUTHORIZED
        } else {
            StatusCode::FORBIDDEN
        };
        for id in [&session, &unbound] {
            for doc in [DOCUMENT, OTHER] {
                assert_eq!(
                    bind(&app, &alice, id, doc).await.0,
                    expected,
                    "{invalidation}"
                );
            }
        }
        assert_eq!(
            call(
                &app,
                &alice,
                bound(&object, &session, ids.clone()),
                &[(HEADER, DOCUMENT)]
            )
            .await
            .0,
            expected
        );
        assert_eq!(persisted(&fixture, &session).as_deref(), Some(DOCUMENT));
        assert_eq!(persisted(&fixture, &unbound), None);
        let restarted = fixture.app();
        assert_eq!(
            bind(&restarted, &alice, &session, DOCUMENT).await.0,
            expected
        );
    }
}
