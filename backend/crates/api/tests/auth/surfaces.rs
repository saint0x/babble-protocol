use super::*;

async fn start(app: &Router, account: &Account, object: &str) -> String {
    let result = rpc(
        app,
        account,
        "babel.runtime.surface.session.start.v1",
        host(),
        json!({"object_id":object,"role":"Feed"}),
    )
    .await;
    assert_eq!(result.0, StatusCode::OK, "{}", result.1);
    let id = result.1["result"]["session"]["id"]
        .as_str()
        .expect("session started")
        .to_owned();
    register_document(app, account, &id).await;
    id
}

async fn transition(
    app: &Router,
    account: &Account,
    id: &str,
    lifecycle: &str,
) -> (StatusCode, Value) {
    let mut binding = host();
    binding.surface_session_id = Some(id.to_owned());
    rpc(
        app,
        account,
        "babel.runtime.surface.session.transition.v1",
        binding,
        json!({"lifecycle":lifecycle,"reason":"auth regression"}),
    )
    .await
}

async fn login_again(app: &Router, account: &Account) -> Account {
    let (status, body) = request(
        app,
        "POST",
        "/auth/login",
        None,
        json!({"identity_id":account.id,"password":PASSWORD}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    Account {
        id: account.id.clone(),
        token: body["token"].as_str().unwrap().to_owned(),
    }
}

fn native_lifecycle(state: &ApiState<LocalProvider>, id: &str) -> Value {
    let request = serde_json::from_value(envelope(
        "babel.runtime.surface.session.get.v1",
        host(),
        json!({"session_id":id}),
    ))
    .unwrap();
    let response = babel_api::dispatch_rpc_request(state, request);
    assert!(response.error.is_none(), "{:?}", response.error);
    response.result.unwrap()["session"]["lifecycle"].clone()
}

#[tokio::test]
async fn auth_surface_author_cannot_execute_suspended_evicted_or_mismatched_sessions() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let alice = register(&app, "surface-author").await;
    let bob = register(&app, "other-viewer").await;
    let target = publish(&app, &alice, "target").await;
    let (object, caps) = controller(&app, &alice, &target, true).await;
    let alice_grants = grants(&app, &alice, &object, &caps).await;
    grants(&app, &bob, &object, &caps).await;
    let session = start(&app, &alice, &object).await;
    let foreign = start(&app, &bob, &object).await;
    let base = format!("/runtime/surfaces/sessions/{session}");
    let binding = bound(&object, &session, alice_grants.clone());
    let write = |binding| {
        rpc(
            &app,
            &alice,
            "babel.storage.local.set.v1",
            binding,
            json!({"key":"lifecycle","value":"allowed"}),
        )
    };
    assert!(write(binding.clone()).await.1["error"].is_null());
    for id in [&foreign, "surf_missing"] {
        assert_eq!(
            write(bound(&object, id, alice_grants.clone())).await.0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        rpc(
            &app,
            &alice,
            "babel.object.get.v1",
            bound(&target, &session, vec![]),
            json!({"object_id":target})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut wrong_identity = binding.clone();
    wrong_identity.identity_id = Some(bob.id.clone());
    assert_eq!(write(wrong_identity).await.0, StatusCode::FORBIDDEN);
    let mut mismatch = host();
    mismatch.surface_session_id = Some(session.clone());
    assert_eq!(
        rpc(
            &app,
            &alice,
            "babel.runtime.surface.session.get.v1",
            mismatch,
            json!({"session_id":foreign})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            &format!("{base}/lifecycle"),
            Some(&alice.token),
            json!({"session_id":foreign,"lifecycle":"evicted","reason":"wrong ID"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert!(transition(&app, &alice, &session, "suspended").await.1["error"].is_null());
    assert_eq!(write(binding.clone()).await.0, StatusCode::FORBIDDEN);
    assert_eq!(
        rpc(
            &app,
            &alice,
            "babel.object.get.v1",
            binding.clone(),
            json!({"object_id":object})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "GET", &base, Some(&alice.token), Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    assert!(transition(&app, &alice, &session, "active").await.1["error"].is_null());
    assert!(write(binding.clone()).await.1["error"].is_null());
    for _ in 0..2 {
        let result = request(
            &app,
            "POST",
            &format!("{base}/lifecycle"),
            Some(&alice.token),
            json!({"lifecycle":"evicted","reason":"close"}),
        )
        .await;
        assert_eq!(result.0, StatusCode::OK, "{}", result.1);
        assert_eq!(result.1["session"]["lifecycle"], "evicted");
    }
    assert_eq!(write(binding).await.0, StatusCode::FORBIDDEN);
    let repeated = transition(&app, &alice, &session, "evicted").await;
    assert_eq!(repeated.0, StatusCode::OK, "{}", repeated.1);
    assert!(repeated.1["error"].is_null());
    assert_eq!(
        request(&app, "GET", &base, Some(&alice.token), Value::Null)
            .await
            .1["session"]["lifecycle"],
        "evicted"
    );
    assert_eq!(
        rpc(
            &app,
            &alice,
            "babel.runtime.surface.session.get.v1",
            host(),
            json!({"session_id":session})
        )
        .await
        .1["result"]["session"]["lifecycle"],
        "evicted"
    );
    for state in ["warm", "active"] {
        assert_eq!(
            transition(&app, &alice, &session, state).await.0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        transition(&app, &bob, &session, "evicted").await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/runtime/surfaces/sessions",
            Some(&alice.token),
            json!({"object_id":object,"role":"Feed","session_id":session})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_ne!(start(&app, &alice, &object).await, session);
}

#[tokio::test]
async fn auth_surface_logout_and_expiry_evict_originating_device_without_client_cleanup() {
    let fixture = Fixture::new();
    let state = ApiState::new(LocalNode::open(&fixture.root, LocalProvider::default()).unwrap());
    let app = router(state.clone());
    let alice = register(&app, "devices").await;
    let other_device = login_again(&app, &alice).await;
    let target = publish(&app, &alice, "target").await;
    let (object, caps) = controller(&app, &alice, &target, true).await;
    let ids = grants(&app, &alice, &object, &caps).await;
    let first = start(&app, &alice, &object).await;
    let second = start(&app, &other_device, &object).await;
    for method in ["GET", "POST"] {
        let path = if method == "GET" {
            format!("/runtime/surfaces/sessions/{first}")
        } else {
            format!("/runtime/surfaces/sessions/{first}/lifecycle")
        };
        assert_eq!(
            request(
                &app,
                method,
                &path,
                Some(&other_device.token),
                if method == "GET" {
                    Value::Null
                } else {
                    json!({"lifecycle":"evicted","reason":"other device"})
                }
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        request(
            &app,
            "DELETE",
            "/auth/session",
            Some(&alice.token),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(native_lifecycle(&state, &first), "evicted");
    assert_eq!(native_lifecycle(&state, &second), "prefetched");
    assert_eq!(
        rpc(
            &app,
            &alice,
            "babel.storage.local.set.v1",
            bound(&object, &first, ids.clone()),
            json!({"key":"after-logout","value":true})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let fresh = login_again(&app, &alice).await;
    let third = start(&app, &fresh, &object).await;
    assert_eq!(
        request(
            &app,
            "POST",
            "/runtime/surfaces/sessions",
            Some(&fresh.token),
            json!({"object_id":object,"role":"Feed","session_id":first})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let db = rusqlite::Connection::open(fixture.root.join("auth/accounts.sqlite3")).unwrap();
    db.execute(
        "UPDATE sessions SET expires_at=0 WHERE token_hash=?1",
        [blake3::hash(other_device.token.as_bytes())
            .to_hex()
            .to_string()],
    )
    .unwrap();
    // Observe the server sweeper through SQLite only: no HTTP/native call may
    // accidentally perform request-driven cleanup on behalf of the idle client.
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let retired: bool = db
                .query_row(
                    "SELECT retired FROM surface_owners WHERE session_id=?1",
                    [&second],
                    |row| row.get(0),
                )
                .unwrap();
            if retired {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("idle expired session was cleaned by server");
    assert_eq!(native_lifecycle(&state, &second), "evicted");
    assert_eq!(native_lifecycle(&state, &third), "prefetched");
    assert_eq!(
        transition(&app, &other_device, &second, "active").await.0,
        StatusCode::UNAUTHORIZED
    );
    assert!(
        rpc(
            &app,
            &fresh,
            "babel.storage.local.set.v1",
            bound(&object, &third, ids),
            json!({"key":"fresh-device","value":true})
        )
        .await
        .1["error"]
            .is_null()
    );
}

#[tokio::test]
async fn auth_surface_legacy_ownership_migrates_fail_closed_without_rebinding() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let alice = register(&app, "legacy-surface").await;
    let target = publish(&app, &alice, "target").await;
    let (object, caps) = controller(&app, &alice, &target, true).await;
    grants(&app, &alice, &object, &caps).await;
    let old = start(&app, &alice, &object).await;
    drop(app);
    let db = rusqlite::Connection::open(fixture.root.join("auth/accounts.sqlite3")).unwrap();
    db.execute_batch(
        "DROP INDEX surface_account_session;
        DROP INDEX surface_account_pending;
        DROP INDEX surface_cleanup_pending;
        ALTER TABLE surface_owners DROP COLUMN account_session;
        ALTER TABLE surface_owners DROP COLUMN retired;",
    )
    .unwrap();
    let app = fixture.app();
    assert_eq!(
        request(
            &app,
            "POST",
            "/runtime/surfaces/sessions",
            Some(&alice.token),
            json!({"object_id":object,"role":"Feed","session_id":old})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let binding: Option<String> = db
        .query_row(
            "SELECT account_session FROM surface_owners WHERE session_id=?1",
            [&old],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(binding, None);
    assert_ne!(start(&app, &alice, &object).await, old);
}

#[tokio::test]
async fn auth_viewer_consent_runtime_isolation_and_hostile_surface_boundary() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let author = register(&app, "author").await;
    let viewer = register(&app, "viewer").await;
    let target = publish(&app, &author, "target").await;
    let (object, caps) = controller(&app, &author, &target, true).await;
    let viewer_grants = grants(&app, &viewer, &object, &caps).await;
    let start = json!({"object_id":object,"role":"Feed"});
    assert_eq!(
        request(
            &app,
            "POST",
            "/runtime/surfaces/prepare",
            None,
            start.clone()
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/runtime/surfaces/sessions",
            None,
            start.clone()
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, started) = rpc(
        &app,
        &viewer,
        "babel.runtime.surface.session.start.v1",
        host(),
        start,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert!(started["error"].is_null(), "{started}");
    let session = started["result"]["session"]["id"].as_str().unwrap();
    assert!(session.starts_with("surf_"));
    register_document(&app, &viewer, session).await;
    let binding = bound(&object, session, viewer_grants.clone());
    transition(&app, &viewer, session, "warm").await;
    transition(&app, &viewer, session, "active").await;
    let (_, prompt) = rpc(
        &app,
        &viewer,
        "babel.social.reply.v2",
        binding.clone(),
        json!({"author_id":viewer.id,"target_object_id":target,"text":"viewer-approved reply"}),
    )
    .await;
    assert_eq!(prompt["error"]["code"], "PERMISSION_REQUIRED", "{prompt}");
    let invocation = prompt["error"]["details"]["invocation"]["invocation_id"]
        .as_str()
        .unwrap();
    for (action, body) in [
        ("decision", json!({"decision":"allow_once"})),
        ("execute", json!({})),
    ] {
        let (status, result) = request_with_headers(
            &app,
            "POST",
            &format!("/invocations/v1/{invocation}/{action}"),
            Some(&viewer.token),
            body,
            &[("x-babel-surface-document", DOCUMENT)],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{result}");
    }
    for (method, payload) in [
        (
            "babel.storage.local.set.v1",
            json!({"key":"private","value":"viewer"}),
        ),
        (
            "babel.storage.object.set.v1",
            json!({"key":"shared","value":"viewer consent"}),
        ),
    ] {
        let result = rpc(&app, &viewer, method, binding.clone(), payload).await;
        assert_eq!(result.0, StatusCode::OK, "{}", result.1);
        assert!(result.1["error"].is_null(), "{}", result.1);
    }
    for method in [
        "babel.object.publish_text.v1",
        "babel.capabilities.grant.v1",
        "babel.capabilities.revoke.v1",
        "babel.runtime.surface.session.start.v1",
        "babel.runtime.surface.session.transition.v1",
        "babel.judgment.object.evaluate.v1",
        "babel.observability.snapshot.v1",
    ] {
        assert_eq!(
            rpc(
                &app,
                &viewer,
                method,
                binding.clone(),
                json!({"author_id":viewer.id,"text":"hostile","object_id":object})
            )
            .await
            .0,
            StatusCode::FORBIDDEN,
            "{method}"
        );
    }
    let base = format!("/runtime/surfaces/sessions/{session}");
    assert_eq!(
        request(&app, "GET", &base, Some(&author.token), Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(&app, "GET", &base, Some(&viewer.token), Value::Null)
            .await
            .0,
        StatusCode::OK
    );
    let checkpoint = request(
        &app,
        "POST",
        &format!("{base}/state/checkpoint"),
        Some(&viewer.token),
        json!({"state":{"private":true},"reason":"test"}),
    )
    .await;
    assert_eq!(checkpoint.0, StatusCode::OK, "{}", checkpoint.1);
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("{base}/state"),
            Some(&author.token),
            Value::Null
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut host_binding = host();
    host_binding.surface_session_id = Some(session.into());
    let transitioned = rpc(
        &app,
        &viewer,
        "babel.runtime.surface.session.transition.v1",
        host_binding.clone(),
        json!({"lifecycle":"active","reason":"viewer opens"}),
    )
    .await;
    assert_eq!(transitioned.0, StatusCode::OK, "{}", transitioned.1);
    assert!(transitioned.1["error"].is_null(), "{}", transitioned.1);
    assert_eq!(
        rpc(
            &app,
            &author,
            "babel.runtime.surface.session.transition.v1",
            host_binding,
            json!({"lifecycle":"Active","reason":"hijack"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let secret = rpc(
        &app,
        &viewer,
        "babel.storage.object.get.v1",
        binding,
        json!({"key":format!("runtime/surface_sessions/{session}/state")}),
    )
    .await;
    assert!(!secret.1["error"].is_null());
}

#[tokio::test]
async fn auth_two_viewers_get_only_their_grants_and_session_retries_preserve_ownership() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    let target = publish(&app, &alice, "target").await;
    let (object, caps) = controller(&app, &alice, &target, true).await;
    let alice_grants = grants(&app, &alice, &object, &caps).await;
    let id = babel_runtime::SurfaceSessionId::from_material("retry surface");
    let start = json!({"object_id":object,"role":"Feed","session_id":id});
    let failed = request(
        &app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&bob.token),
        start.clone(),
    )
    .await;
    assert_eq!(failed.0, StatusCode::CONFLICT, "{}", failed.1);
    let db = rusqlite::Connection::open(fixture.root.join("auth/accounts.sqlite3")).unwrap();
    let reserved: u64 = db
        .query_row("SELECT COUNT(*) FROM surface_owners", [], |row| row.get(0))
        .unwrap();
    assert_eq!(reserved, 0, "failed admission must not leave a reservation");
    let bob_grants = grants(&app, &bob, &object, &caps).await;
    let alice_id = babel_runtime::SurfaceSessionId::from_material("alice private session");
    let own_start = request(
        &app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&alice.token),
        json!({"object_id":object,"role":"Feed","session_id":alice_id}),
    )
    .await;
    assert_eq!(own_start.0, StatusCode::OK, "{}", own_start.1);
    let public = request(
        &app,
        "POST",
        "/runtime/surfaces/prepare",
        None,
        json!({"object_id":object,"role":"Feed"}),
    )
    .await;
    let public_text = public.1.to_string();
    for id in alice_grants.iter().chain(&bob_grants) {
        assert!(!public_text.contains(id));
    }
    let prepared = request(
        &app,
        "POST",
        "/runtime/surfaces/prepare",
        Some(&bob.token),
        json!({"object_id":object,"role":"Feed"}),
    )
    .await;
    for id in &alice_grants {
        assert!(!prepared.1.to_string().contains(id));
    }
    for id in &bob_grants {
        assert!(prepared.1.to_string().contains(id));
    }
    let started = request(
        &app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&bob.token),
        start.clone(),
    )
    .await;
    assert_eq!(started.0, StatusCode::OK, "{}", started.1);
    for id in &alice_grants {
        assert!(!started.1.to_string().contains(id));
    }
    for id in &bob_grants {
        assert!(started.1.to_string().contains(id));
    }
    assert_eq!(
        request(
            &app,
            "POST",
            "/runtime/surfaces/sessions",
            Some(&bob.token),
            start.clone()
        )
        .await,
        started
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/runtime/surfaces/sessions",
            Some(&alice.token),
            start.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        rpc(
            &app,
            &alice,
            "babel.runtime.surface.session.start.v1",
            host(),
            start.clone()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut victim_binding = host();
    victim_binding.surface_session_id = Some(id.to_string());
    assert_eq!(rpc(&app, &alice, "babel.runtime.surface.session.transition.v1", victim_binding, json!({"session_id":alice_id,"lifecycle":"evicted","reason":"mismatched session attack"})).await.0, StatusCode::FORBIDDEN);
    let counterfeit = bound(&object, id.as_str(), alice_grants.clone());
    assert_eq!(
        rpc(
            &app,
            &bob,
            "babel.storage.local.get.v1",
            counterfeit,
            json!({"key":"private"})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    drop(app);
    let mut node = LocalNode::open(&fixture.root, LocalProvider::default()).unwrap();
    let native_id = babel_runtime::SurfaceSessionId::from_material("trusted-native");
    node.start_surface_session(
        &babel_types::ObjectId::new_unchecked(object.clone()),
        babel_object::SurfaceRole::Feed,
        Some(native_id.clone()),
    )
    .unwrap();
    let app = router(ApiState::new(node));
    assert_eq!(
        request(
            &app,
            "POST",
            "/runtime/surfaces/sessions",
            Some(&bob.token),
            json!({"object_id":object,"role":"Feed","session_id":native_id})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let restarted = request(
        &app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&bob.token),
        start,
    )
    .await;
    assert_eq!(restarted.0, StatusCode::OK, "{}", restarted.1);
}
