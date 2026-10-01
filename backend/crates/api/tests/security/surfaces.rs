use super::*;
use babel_rpc::{RpcBinding, RpcRequestEnvelope, babel_rpc_catalog};

fn envelope(method: &str, binding: RpcBinding, payload: Value) -> Value {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    serde_json::to_value(
        RpcRequestEnvelope::new(
            &babel_rpc_catalog().unwrap(),
            "security-regression",
            method,
            binding,
            payload,
        )
        .unwrap()
        .with_idempotency_key(format!(
            "security-{}",
            SEQUENCE.fetch_add(1, Ordering::Relaxed)
        )),
    )
    .unwrap()
}
fn host() -> RpcBinding {
    RpcBinding::host("security-host", "https://babel.test").unwrap()
}
async fn surface_object(app: &Router, account: &Account) -> (String, String) {
    let uploaded = request(app, "POST", "/media/blobs", Some(&account.token), json!({"media_type":"text/html","bytes_hex":hex::encode(b"<!doctype html><title>Security test</title>")})).await;
    assert_eq!(uploaded.0, StatusCode::OK, "{}", uploaded.1);
    let hash = uploaded.1["blob"]["integrity"].as_str().unwrap();
    let uri = format!("babel://blobs/{hash}");
    let capability = json!({"id":"babel.storage.local","version":1,"scope":{"namespace":"self"}});
    let mut draft =
        serde_json::to_value(babel_authoring::ObjectDraft::text("Security test Surface").unwrap())
            .unwrap();
    draft["surfaces"] = json!([{"role":"Feed","target":"Web","entry":uri,"integrity":hash}]);
    draft["resources"] = json!([{"uri":uri,"integrity":hash,"media_type":"text/html"}]);
    draft["capabilities"] = json!([capability]);
    let published = request(
        app,
        "POST",
        "/objects",
        Some(&account.token),
        json!({"author_id":account.id,"draft":draft}),
    )
    .await;
    assert_eq!(published.0, StatusCode::OK, "{}", published.1);
    let object = published.1["object"]["id"].as_str().unwrap().to_owned();
    let granted = request(app, "POST", "/capabilities/grants", Some(&account.token), json!({"author_id":account.id,"object_id":object,"capability":capability,"decision":"approved"})).await;
    assert_eq!(granted.0, StatusCode::OK, "{}", granted.1);
    (
        object,
        granted.1["event"]["payload"]["grant"]["id"]
            .as_str()
            .unwrap()
            .to_owned(),
    )
}
async fn start(app: &Router, account: &Account, object: &str) -> String {
    let response = request(
        app,
        "POST",
        "/runtime/surfaces/sessions",
        Some(&account.token),
        json!({"object_id":object,"role":"Feed"}),
    )
    .await;
    assert_eq!(response.0, StatusCode::OK, "{}", response.1);
    let id = response.1["session"]["id"].as_str().unwrap().to_owned();
    let registration = request(
        app,
        "PUT",
        &format!("/runtime/surfaces/sessions/{id}/document"),
        Some(&account.token),
        json!({"document_id":"550e8400-e29b-41d4-a716-446655440000"}),
    )
    .await;
    assert_eq!(registration.0, StatusCode::OK, "{}", registration.1);
    id
}
fn lifecycle(state: &ApiState<LocalProvider>, session: &str) -> Value {
    let response = babel_api::dispatch_rpc_request(
        state,
        serde_json::from_value(envelope(
            "babel.runtime.surface.session.get.v1",
            host(),
            json!({"session_id":session}),
        ))
        .unwrap(),
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    response.result.unwrap()["session"]["lifecycle"].clone()
}
async fn surface_write(
    app: &Router,
    account: &Account,
    object: &str,
    session: &str,
    grant: &str,
) -> (StatusCode, Value) {
    raw_with_headers(
        app,
        "POST",
        "/rpc",
        Some(&account.token),
        Some(
            &envelope(
                "babel.storage.local.set.v1",
                RpcBinding::object(
                    object,
                    session,
                    "security-host",
                    "https://babel.test",
                    vec![grant.to_owned()],
                )
                .unwrap(),
                json!({"key":"security","value":true}),
            )
            .to_string(),
        ),
        &[(
            "x-babel-surface-document",
            "550e8400-e29b-41d4-a716-446655440000",
        )],
    )
    .await
}

#[tokio::test]
async fn security_revocation_evicts_exact_origin_surfaces_and_denies_old_rpc_and_rest_immediately()
{
    let fixture = Fixture::new();
    let state = fixture.state();
    let app = router(state.clone());
    let alice = register(&app).await;
    let second = login(&app, &alice, PASSWORD).await;
    let bob = register(&app).await;
    let (object, grant) = surface_object(&app, &alice).await;
    let (bob_object, _) = surface_object(&app, &bob).await;
    let first_surface = start(&app, &alice, &object).await;
    let second_surface = start(&app, &second, &object).await;
    let bob_surface = start(&app, &bob, &bob_object).await;
    let write = surface_write(&app, &second, &object, &second_surface, &grant).await;
    assert_eq!(write.0, StatusCode::OK);
    assert!(write.1["error"].is_null(), "{}", write.1);
    let second_id = current_id(&sessions(&app, &second).await);
    no_content(
        &app,
        "DELETE",
        &format!("/auth/sessions/{second_id}"),
        &alice,
        Value::Null,
    )
    .await;
    assert_eq!(lifecycle(&state, &second_surface), "evicted");
    assert_eq!(lifecycle(&state, &first_surface), "prefetched");
    assert_eq!(lifecycle(&state, &bob_surface), "prefetched");
    assert_eq!(
        surface_write(&app, &second, &object, &second_surface, &grant)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let base = format!("/runtime/surfaces/sessions/{second_surface}");
    for (method, path, body) in [
        ("GET", base.clone(), Value::Null),
        ("POST", format!("{base}/heartbeat"), Value::Null),
        (
            "POST",
            format!("{base}/state/checkpoint"),
            json!({"state":{"forged":true},"reason":"revoked"}),
        ),
    ] {
        assert_eq!(
            request(&app, method, &path, Some(&second.token), body)
                .await
                .0,
            StatusCode::UNAUTHORIZED
        );
    }
    // A surviving login of the same identity must not inherit the retired host.
    assert_eq!(
        request(&app, "GET", &base, Some(&alice.token), Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let third = login(&app, &alice, PASSWORD).await;
    let third_surface = start(&app, &third, &object).await;
    no_content(
        &app,
        "POST",
        "/auth/sessions/revoke-others",
        &alice,
        Value::Null,
    )
    .await;
    assert_eq!(lifecycle(&state, &third_surface), "evicted");
    assert_eq!(lifecycle(&state, &first_surface), "prefetched");
    assert_eq!(
        change(&app, &alice, PASSWORD, REPLACEMENT).await.0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(lifecycle(&state, &first_surface), "evicted");
    assert_eq!(lifecycle(&state, &bob_surface), "prefetched");
    let retired: usize = fixture
        .db()
        .query_row(
            "SELECT COUNT(*) FROM surface_owners WHERE identity_id=?1 AND retired=1",
            [&alice.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retired, 3);
    drop(app);
    drop(state);
    let app = fixture.app();
    let fresh = login(&app, &alice, REPLACEMENT).await;
    for retired in [&first_surface, &second_surface, &third_surface] {
        assert_eq!(
            request(
                &app,
                "POST",
                "/runtime/surfaces/sessions",
                Some(&fresh.token),
                json!({"object_id":object,"role":"Feed","session_id":retired})
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_ne!(start(&app, &fresh, &object).await, first_surface);
}
