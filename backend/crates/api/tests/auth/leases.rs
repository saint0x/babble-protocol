use super::*;
use rusqlite::{Connection, params};
use std::time::Duration;

const HEARTBEAT: &str = "babble.runtime.surface.session.heartbeat.v1";
const TTL_MS: i64 = 60_000;

fn now_ms() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

fn database(fixture: &Fixture) -> Connection {
    let db = Connection::open(fixture.root.join("auth/accounts.sqlite3")).unwrap();
    db.busy_timeout(Duration::from_secs(5)).unwrap();
    db
}

fn deadline(db: &Connection, id: &str) -> i64 {
    db.query_row(
        "SELECT lease_expires_at FROM surface_owners WHERE session_id=?1",
        [id],
        |row| row.get(0),
    )
    .unwrap()
}

fn set_deadline(db: &Connection, id: &str, expires: i64) {
    assert_eq!(
        db.execute(
            "UPDATE surface_owners SET lease_expires_at=?2 WHERE session_id=?1",
            params![id, expires],
        )
        .unwrap(),
        1
    );
}

fn host_session(id: &str) -> RpcBinding {
    let mut binding = host();
    binding.surface_session_id = Some(id.to_owned());
    assert!(binding.object_id.is_none());
    binding
}

async fn start(app: &Router, account: &Account, object: &str, id: Option<&str>) -> String {
    let (status, body) = request(
        app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&account.token),
        json!({"object_id":object,"role":"Feed","session_id":id}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let id = body["session"]["id"].as_str().unwrap().to_owned();
    register_document(app, account, &id).await;
    id
}

async fn setup(app: &Router) -> (Account, String, Vec<String>) {
    let account = register(app, "lease-owner").await;
    let target = publish(app, &account, "lease target").await;
    let (object, caps) = controller(app, &account, &target, true).await;
    let ids = grants(app, &account, &object, &caps).await;
    (account, object, ids)
}

async fn heartbeat(app: &Router, account: &Account, id: &str, http: bool) -> (StatusCode, Value) {
    if http {
        request(
            app,
            "POST",
            &format!("/runtime/surfaces/sessions/{id}/heartbeat"),
            Some(&account.token),
            Value::Null,
        )
        .await
    } else {
        let (status, body) = rpc(app, account, HEARTBEAT, host_session(id), json!({})).await;
        if body["error"].is_null() {
            (status, body.get("result").cloned().unwrap_or(body))
        } else {
            (status, body)
        }
    }
}

async fn transition(
    app: &Router,
    account: &Account,
    id: &str,
    lifecycle: &str,
) -> (StatusCode, Value) {
    rpc(
        app,
        account,
        "babble.runtime.surface.session.transition.v1",
        host_session(id),
        json!({"lifecycle":lifecycle,"reason":"lease regression"}),
    )
    .await
}

fn native_lifecycle(state: &ApiState<LocalProvider>, id: &str) -> Value {
    let request = serde_json::from_value(envelope(
        "babble.runtime.surface.session.get.v1",
        host(),
        json!({"session_id":id}),
    ))
    .unwrap();
    let response = babble_api::dispatch_rpc_request(state, request);
    assert!(response.error.is_none(), "{:?}", response.error);
    response.result.unwrap()["session"]["lifecycle"].clone()
}

fn assert_native_released_after_restart(state: &ApiState<LocalProvider>, id: &str) {
    let request = serde_json::from_value(envelope(
        "babble.runtime.surface.session.get.v1",
        host(),
        json!({"session_id":id}),
    ))
    .unwrap();
    let response = babble_api::dispatch_rpc_request(state, request);
    // A cold runtime need not recreate an expired session just to evict it.
    if let Some(error) = response.error {
        assert!(
            matches!(error.code, babble_rpc::RpcErrorCode::NotFound),
            "{error:?}"
        );
    } else {
        assert_eq!(response.result.unwrap()["session"]["lifecycle"], "evicted");
    }
}

fn native_health(state: &ApiState<LocalProvider>) -> babble_runtime::SurfaceRuntimeHealthSnapshot {
    let request = serde_json::from_value(envelope(
        "babble.runtime.surface.health.v1",
        host(),
        json!({}),
    ))
    .unwrap();
    let response = babble_api::dispatch_rpc_request(state, request);
    assert!(response.error.is_none(), "{:?}", response.error);
    serde_json::from_value(response.result.unwrap()["health"].clone()).unwrap()
}

fn assert_lease(body: &Value, id: &str, db: &Connection, before: i64, after: i64) {
    let lease = &body["lease"];
    assert_eq!(lease["session_id"], id, "{body}");
    let expires = time::OffsetDateTime::parse(
        lease["expires_at"].as_str().expect("RFC3339 lease expiry"),
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    let expires = (expires.unix_timestamp_nanos() / 1_000_000) as i64;
    assert_eq!(
        expires,
        deadline(db, id),
        "wire and durable deadlines disagree"
    );
    let ttl = lease["ttl_ms"].as_i64().expect("integer ttl_ms");
    let renew = lease["renew_after_ms"]
        .as_i64()
        .expect("integer renew_after_ms");
    assert!((1..=TTL_MS).contains(&ttl), "{body}");
    assert!(renew > 0 && renew < ttl, "{body}");
    assert!(expires > before && expires <= after + TTL_MS, "{body}");
    assert!(expires >= before + ttl && expires <= after + ttl, "{body}");
}

async fn await_retired(db: &Connection, ids: &[String]) {
    // Only SQLite polling here: an HTTP/native read could trigger cleanup itself.
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if ids.iter().all(|id| {
                db.query_row(
                    "SELECT retired FROM surface_owners WHERE session_id=?1",
                    [id],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap()
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("server must retire idle leases within the bounded cleanup window");
}

#[tokio::test]
async fn auth_lease_renewal_is_explicit_persisted_and_reads_do_not_extend_it() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let (account, object, ids) = setup(&app).await;
    let db = database(&fixture);
    let before = now_ms();
    let id = start(&app, &account, &object, None).await;
    assert!((before + TTL_MS..=now_ms() + TTL_MS).contains(&deadline(&db, &id)));

    for http in [false, true] {
        let shortened = now_ms() + 15_000;
        set_deadline(&db, &id, shortened);
        let before = now_ms();
        let (status, body) = heartbeat(&app, &account, &id, http).await;
        let after = now_ms();
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_lease(&body, &id, &db, before, after);
        assert!((before + TTL_MS..=after + TTL_MS).contains(&deadline(&db, &id)));
        assert!(deadline(&db, &id) > shortened);
    }

    for lifecycle in ["warm", "active"] {
        let changed = transition(&app, &account, &id, lifecycle).await;
        assert_eq!(changed.0, StatusCode::OK, "{}", changed.1);
        assert!(changed.1["error"].is_null(), "{}", changed.1);
        assert_eq!(
            heartbeat(&app, &account, &id, false).await.0,
            StatusCode::OK
        );
    }
    let checkpoint = request(
        &app,
        "POST",
        &format!("/runtime/surfaces/sessions/{id}/state/checkpoint"),
        Some(&account.token),
        json!({"state":{"saved":true},"reason":"read-only lease regression"}),
    )
    .await;
    assert_eq!(checkpoint.0, StatusCode::OK, "{}", checkpoint.1);
    let fixed = now_ms() + 25_000;
    set_deadline(&db, &id, fixed);
    for suffix in ["", "/state"] {
        let result = request(
            &app,
            "GET",
            &format!("/runtime/surfaces/sessions/{id}{suffix}"),
            Some(&account.token),
            Value::Null,
        )
        .await;
        assert_eq!(result.0, StatusCode::OK, "{}", result.1);
        assert_eq!(deadline(&db, &id), fixed);
    }
    for (method, binding, payload) in [
        (
            "babble.runtime.surface.session.get.v1",
            host_session(&id),
            json!({"session_id":id}),
        ),
        (
            "babble.runtime.surface.session.state.get.v1",
            host_session(&id),
            json!({"session_id":id}),
        ),
        (
            "babble.object.get.v1",
            bound(&object, &id, ids.clone()),
            json!({"object_id":object}),
        ),
        (
            "babble.storage.local.get.v1",
            bound(&object, &id, ids),
            json!({"key":"absent"}),
        ),
    ] {
        let result = rpc(&app, &account, method, binding, payload).await;
        assert_eq!(result.0, StatusCode::OK, "{method}: {}", result.1);
        assert!(result.1["error"].is_null(), "{method}: {}", result.1);
        assert_eq!(deadline(&db, &id), fixed, "{method} renewed the lease");
    }
    assert_eq!(start(&app, &account, &object, Some(&id)).await, id);
    assert_eq!(deadline(&db, &id), fixed, "start retry renewed the lease");
}

#[tokio::test]
async fn auth_lease_initial_reservation_and_heartbeat_are_capped_by_login_expiry() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let (account, object, _) = setup(&app).await;
    let db = database(&fixture);
    let cap_seconds = time::OffsetDateTime::now_utc().unix_timestamp() + 30;
    db.execute(
        "UPDATE sessions SET expires_at=?1 WHERE token_hash=?2",
        params![
            cap_seconds,
            blake3::hash(account.token.as_bytes()).to_hex().to_string()
        ],
    )
    .unwrap();
    let id = start(&app, &account, &object, None).await;
    assert_eq!(deadline(&db, &id), cap_seconds * 1000);
    for http in [false, true] {
        set_deadline(&db, &id, now_ms() + 10_000);
        let before = now_ms();
        let (status, body) = heartbeat(&app, &account, &id, http).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_lease(&body, &id, &db, before, now_ms());
        assert_eq!(deadline(&db, &id), cap_seconds * 1000);
    }
}

#[tokio::test]
async fn auth_lease_heartbeat_requires_originating_login_and_unbound_host() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let (account, object, ids) = setup(&app).await;
    let login = request(
        &app,
        "POST",
        "/auth/login",
        None,
        json!({"identity_id":account.id,"password":PASSWORD}),
    )
    .await;
    assert_eq!(login.0, StatusCode::OK, "{}", login.1);
    let same_identity = Account {
        id: account.id.clone(),
        token: login.1["token"].as_str().unwrap().to_owned(),
    };
    let foreign = register(&app, "foreign-lease-owner").await;
    let wrong_object = publish(&app, &account, "wrong object").await;
    let id = start(&app, &account, &object, None).await;
    let db = database(&fixture);
    let fixed = deadline(&db, &id);
    for caller in [&same_identity, &foreign] {
        for http in [false, true] {
            let result = heartbeat(&app, caller, &id, http).await;
            assert_eq!(result.0, StatusCode::FORBIDDEN, "{}", result.1);
            assert_eq!(deadline(&db, &id), fixed);
        }
    }
    let mut wrong_identity = host_session(&id);
    wrong_identity.identity_id = Some(foreign.id.clone());
    let mut object_host = host_session(&id);
    object_host.object_id = Some(object.clone());
    for binding in [
        bound(&object, &id, ids.clone()),
        bound(&wrong_object, &id, ids),
        object_host,
        wrong_identity,
    ] {
        let result = rpc(&app, &account, HEARTBEAT, binding, json!({})).await;
        assert_eq!(result.0, StatusCode::FORBIDDEN, "{}", result.1);
        assert_eq!(deadline(&db, &id), fixed);
    }
    let unauthenticated = request(
        &app,
        "POST",
        &format!("/runtime/surfaces/sessions/{id}/heartbeat"),
        None,
        Value::Null,
    )
    .await;
    assert_eq!(unauthenticated.0, StatusCode::UNAUTHORIZED);
    let unauthenticated = request(
        &app,
        "POST",
        "/rpc",
        None,
        envelope(HEARTBEAT, host_session(&id), json!({})),
    )
    .await;
    assert_eq!(unauthenticated.0, StatusCode::UNAUTHORIZED);
    assert_eq!(deadline(&db, &id), fixed);
    assert_eq!(
        heartbeat(&app, &account, &id, false).await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn auth_lease_suspended_and_terminal_sessions_cannot_renew() {
    let fixture = Fixture::new();
    let state = ApiState::new(LocalNode::open(&fixture.root, LocalProvider::default()).unwrap());
    let app = router(state.clone());
    let (account, object, _) = setup(&app).await;
    let db = database(&fixture);
    for lifecycle in ["suspended", "evicted"] {
        let id = start(&app, &account, &object, None).await;
        let changed = transition(&app, &account, &id, lifecycle).await;
        assert_eq!(changed.0, StatusCode::OK, "{}", changed.1);
        assert!(changed.1["error"].is_null(), "{}", changed.1);
        let fixed = deadline(&db, &id);
        for http in [false, true] {
            let result = heartbeat(&app, &account, &id, http).await;
            assert_eq!(result.0, StatusCode::FORBIDDEN, "{lifecycle}: {}", result.1);
            assert_eq!(deadline(&db, &id), fixed);
        }
        assert_eq!(native_lifecycle(&state, &id), lifecycle);
    }
}

#[tokio::test]
async fn auth_lease_expiry_denies_execution_management_and_restart_before_cleanup() {
    let fixture = Fixture::new();
    let state = ApiState::new(LocalNode::open(&fixture.root, LocalProvider::default()).unwrap());
    let app = router(state.clone());
    let (account, object, ids) = setup(&app).await;
    let id = start(&app, &account, &object, None).await;
    let db = database(&fixture);
    assert_eq!(native_lifecycle(&state, &id), "prefetched");
    let expired = now_ms() - 1;
    set_deadline(&db, &id, expired);
    // Exercise expiry immediately, without waiting for server cleanup.
    let write = rpc(
        &app,
        &account,
        "babble.storage.local.set.v1",
        bound(&object, &id, ids.clone()),
        json!({"key":"expired-write","value":true}),
    )
    .await;
    assert_eq!(write.0, StatusCode::FORBIDDEN, "{}", write.1);
    for http in [false, true] {
        assert_eq!(
            heartbeat(&app, &account, &id, http).await.0,
            StatusCode::FORBIDDEN
        );
    }
    for lifecycle in ["warm", "active"] {
        assert_eq!(
            transition(&app, &account, &id, lifecycle).await.0,
            StatusCode::FORBIDDEN
        );
    }
    let checkpoint = request(
        &app,
        "POST",
        &format!("/runtime/surfaces/sessions/{id}/state/checkpoint"),
        Some(&account.token),
        json!({"state":{"forbidden":true},"reason":"expired"}),
    )
    .await;
    assert_eq!(checkpoint.0, StatusCode::FORBIDDEN, "{}", checkpoint.1);
    let restart = request(
        &app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&account.token),
        json!({"object_id":object,"role":"Feed","session_id":id}),
    )
    .await;
    assert_eq!(restart.0, StatusCode::FORBIDDEN, "{}", restart.1);
    assert_eq!(deadline(&db, &id), expired);
    for _ in 0..2 {
        let inspected = request(
            &app,
            "GET",
            &format!("/runtime/surfaces/sessions/{id}"),
            Some(&account.token),
            Value::Null,
        )
        .await;
        assert_eq!(inspected.0, StatusCode::OK, "{}", inspected.1);
        let evicted = transition(&app, &account, &id, "evicted").await;
        assert_eq!(evicted.0, StatusCode::OK, "{}", evicted.1);
        assert!(evicted.1["error"].is_null(), "{}", evicted.1);
    }
    assert_eq!(native_lifecycle(&state, &id), "evicted");
    let fresh = start(&app, &account, &object, None).await;
    let stored = rpc(
        &app,
        &account,
        "babble.storage.local.get.v1",
        bound(&object, &fresh, ids),
        json!({"key":"expired-write"}),
    )
    .await;
    assert_eq!(stored.0, StatusCode::OK, "{}", stored.1);
    assert!(stored.1["error"].is_null(), "{}", stored.1);
    assert_eq!(
        stored.1["result"].get("entry"),
        Some(&Value::Null),
        "expired write committed: {}",
        stored.1
    );
}

#[tokio::test]
async fn auth_lease_idle_expiry_evicts_native_resource_while_login_remains_valid() {
    let fixture = Fixture::new();
    let state = ApiState::new(LocalNode::open(&fixture.root, LocalProvider::default()).unwrap());
    let app = router(state.clone());
    let (account, object, _) = setup(&app).await;
    let live = start(&app, &account, &object, None).await;
    let single_session_totals = native_health(&state).totals;
    let expired = start(&app, &account, &object, None).await;
    assert!(native_health(&state).totals.memory_bytes > single_session_totals.memory_bytes);
    let db = database(&fixture);
    let live_deadline = deadline(&db, &live);
    set_deadline(&db, &expired, 0);
    await_retired(&db, std::slice::from_ref(&expired)).await;
    assert_eq!(native_lifecycle(&state, &expired), "evicted");
    assert_eq!(native_lifecycle(&state, &live), "prefetched");
    let health = native_health(&state);
    assert_eq!(
        health.totals, single_session_totals,
        "expired resource reservations remain allocated"
    );
    assert_eq!(
        health.session_count, 2,
        "terminal history remains inspectable"
    );
    assert_eq!(health.lifecycle_counts.evicted, 1);
    assert_eq!(deadline(&db, &expired), 0);
    assert_eq!(deadline(&db, &live), live_deadline);
    assert_eq!(
        request(
            &app,
            "GET",
            "/auth/session",
            Some(&account.token),
            Value::Null
        )
        .await
        .0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn auth_lease_restart_preserves_deadline_and_expired_tombstone() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let (account, object, _) = setup(&app).await;
    let id = start(&app, &account, &object, None).await;
    let db = database(&fixture);
    let fixed = now_ms() + 30_000;
    set_deadline(&db, &id, fixed);
    drop(app);
    let app = fixture.app();
    assert_eq!(deadline(&db, &id), fixed);
    assert_eq!(
        request(
            &app,
            "POST",
            "/runtime/surfaces/sessions",
            Some(&account.token),
            json!({"object_id":object,"role":"Feed","session_id":id})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(deadline(&db, &id), fixed);
    set_deadline(&db, &id, 0);
    drop(app);
    let state = ApiState::new(LocalNode::open(&fixture.root, LocalProvider::default()).unwrap());
    let app = router(state.clone());
    await_retired(&db, std::slice::from_ref(&id)).await;
    assert_native_released_after_restart(&state, &id);
    for http in [false, true] {
        assert_eq!(
            heartbeat(&app, &account, &id, http).await.0,
            StatusCode::FORBIDDEN
        );
    }
    let restarted = rpc(
        &app,
        &account,
        "babble.runtime.surface.session.start.v1",
        host(),
        json!({"object_id":object,"role":"Feed","session_id":id}),
    )
    .await;
    assert_eq!(restarted.0, StatusCode::FORBIDDEN, "{}", restarted.1);
    assert_eq!(deadline(&db, &id), 0);
    assert_ne!(start(&app, &account, &object, None).await, id);
}

#[tokio::test]
async fn auth_lease_legacy_schema_migrates_with_zero_deadline_and_no_resurrection() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let (account, object, _) = setup(&app).await;
    let id = start(&app, &account, &object, None).await;
    drop(app);
    let db = database(&fixture);
    // Rebuild the actual preceding schema, including a provable originating login.
    db.execute_batch(
        "BEGIN IMMEDIATE;
         ALTER TABLE surface_owners RENAME TO leased_surface_owners;
         CREATE TABLE surface_owners (
             session_id TEXT PRIMARY KEY, identity_id TEXT NOT NULL, object_id TEXT NOT NULL,
             account_session TEXT, retired INTEGER NOT NULL DEFAULT 0);
         INSERT INTO surface_owners SELECT session_id, identity_id, object_id, account_session, retired
             FROM leased_surface_owners;
         DROP TABLE leased_surface_owners;
         COMMIT;",
    ).unwrap();
    let state = ApiState::new(LocalNode::open(&fixture.root, LocalProvider::default()).unwrap());
    let app = router(state.clone());
    assert_eq!(
        deadline(&db, &id),
        0,
        "legacy rows must not acquire a fresh lease"
    );
    for http in [false, true] {
        assert_eq!(
            heartbeat(&app, &account, &id, http).await.0,
            StatusCode::FORBIDDEN
        );
    }
    let restart = request(
        &app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&account.token),
        json!({"object_id":object,"role":"Feed","session_id":id}),
    )
    .await;
    assert_eq!(restart.0, StatusCode::FORBIDDEN, "{}", restart.1);
    await_retired(&db, std::slice::from_ref(&id)).await;
    assert_native_released_after_restart(&state, &id);
    assert_eq!(deadline(&db, &id), 0);
    assert_ne!(start(&app, &account, &object, None).await, id);
}

#[tokio::test]
async fn auth_lease_concurrent_heartbeats_cannot_restore_an_expired_session() {
    let fixture = Fixture::new();
    let state = ApiState::new(LocalNode::open(&fixture.root, LocalProvider::default()).unwrap());
    let app = router(state.clone());
    let (account, object, _) = setup(&app).await;
    let id = start(&app, &account, &object, None).await;
    let db = database(&fixture);
    set_deadline(&db, &id, 0);
    let mut requests = tokio::task::JoinSet::new();
    for i in 0..16 {
        let (app, account, id) = (app.clone(), account.clone(), id.clone());
        requests.spawn(async move { heartbeat(&app, &account, &id, i % 2 == 0).await });
    }
    while let Some(result) = requests.join_next().await {
        let result = result.unwrap();
        assert_eq!(result.0, StatusCode::FORBIDDEN, "{}", result.1);
    }
    assert_eq!(deadline(&db, &id), 0);
    await_retired(&db, std::slice::from_ref(&id)).await;
    assert_eq!(native_lifecycle(&state, &id), "evicted");
    assert_eq!(
        heartbeat(&app, &account, &id, true).await.0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(deadline(&db, &id), 0);
}

#[tokio::test]
async fn auth_lease_cleanup_advances_past_a_full_live_batch() {
    let fixture = Fixture::new();
    let state = ApiState::new(LocalNode::open(&fixture.root, LocalProvider::default()).unwrap());
    let app = router(state.clone());
    let (account, object, _) = setup(&app).await;
    let mut live = Vec::new();
    // The server's existing cleanup batch is 128; sorted live rows precede expiry.
    for i in 0..129 {
        let id = format!("surf_{i:064x}");
        live.push(start(&app, &account, &object, Some(&id)).await);
    }
    let expired_id = format!("surf_{:064x}", 130);
    let expired = start(&app, &account, &object, Some(&expired_id)).await;
    let db = database(&fixture);
    set_deadline(&db, &expired, 0);
    await_retired(&db, std::slice::from_ref(&expired)).await;
    assert_eq!(native_lifecycle(&state, &expired), "evicted");
    for id in live {
        assert_eq!(native_lifecycle(&state, &id), "prefetched");
        let retired: bool = db
            .query_row(
                "SELECT retired FROM surface_owners WHERE session_id=?1",
                [&id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!retired, "cleanup retired a live lease: {id}");
    }
}
