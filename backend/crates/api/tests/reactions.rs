use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use babble_api::{ApiState, router};
use babble_graph::ReactionRecord;
use babble_identity::Identity;
use babble_judgment_local::LocalProvider;
use babble_node::LocalNode;
use babble_rpc::{RpcBinding, RpcRequestEnvelope, babble_rpc_catalog};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tower::ServiceExt;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "babble-reactions-http-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        )))
    }
    fn state(&self) -> ApiState<LocalProvider> {
        ApiState::new(LocalNode::open(&self.0, LocalProvider::default()).unwrap())
    }
    fn app(&self) -> Router {
        router(self.state())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
struct Account {
    id: String,
    token: String,
}

async fn raw(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: String,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("content-type", "application/json");
    if let Some(token) = token {
        builder = builder.header("authorization", format!("Bearer {token}"));
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = response.status();
    if status.is_success() {
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| json!({"text": String::from_utf8_lossy(&bytes)})),
    )
}
async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    raw(
        app,
        method,
        uri,
        token,
        if body.is_null() {
            String::new()
        } else {
            body.to_string()
        },
    )
    .await
}
fn ok(response: (StatusCode, Value)) -> Value {
    assert_eq!(response.0, StatusCode::OK, "{}", response.1);
    response.1
}
fn rpc_ok(response: (StatusCode, Value)) -> Value {
    let body = ok(response);
    assert!(body["error"].is_null(), "{body}");
    body.get("result").expect("RPC result").clone()
}
fn rejected(response: (StatusCode, Value)) {
    assert!(
        response.0.is_client_error()
            || (response.0 == StatusCode::OK && !response.1["error"].is_null()),
        "unexpected acceptance: {response:?}"
    );
}
async fn register(app: &Router, handle: &str) -> Account {
    let body = ok(request(
        app,
        "POST",
        "/auth/register",
        None,
        json!({"handle":handle,"kind":"Person","password":"Durable reaction regression 930403!"}),
    )
    .await);
    Account {
        id: body["identity"]["id"].as_str().unwrap().into(),
        token: body["token"].as_str().unwrap().into(),
    }
}
async fn publish(app: &Router, actor: &Account, text: &str) -> String {
    ok(request(
        app,
        "POST",
        "/objects/text",
        Some(&actor.token),
        json!({"author_id":actor.id,"text":text}),
    )
    .await)["object"]["id"]
        .as_str()
        .unwrap()
        .into()
}
fn value(
    appreciation: Option<&str>,
    engagement: Option<&str>,
    stance: Option<&str>,
    certainty: Option<u8>,
) -> Value {
    json!({"appreciation":appreciation,"engagement":engagement,"stance":stance,"certainty":certainty})
}
fn empty() -> Value {
    value(None, None, None, None)
}
fn like() -> Value {
    value(Some("like"), None, None, None)
}
fn dislike() -> Value {
    value(Some("dislike"), None, None, None)
}
fn path(object: &str) -> String {
    format!("/objects/{object}/reactions")
}
fn mutation(value: Value, revision: u64, key: &str) -> Value {
    json!({"value":value,"expected_revision":revision,"idempotency_key":key})
}
async fn set(
    app: &Router,
    actor: &Account,
    object: &str,
    value: Value,
    revision: u64,
    key: &str,
) -> Value {
    ok(request(
        app,
        "PUT",
        &format!("{}/mine", path(object)),
        Some(&actor.token),
        mutation(value, revision, key),
    )
    .await)
}
async fn mine(app: &Router, actor: &Account, object: &str) -> Value {
    ok(request(
        app,
        "GET",
        &format!("{}/mine", path(object)),
        Some(&actor.token),
        Value::Null,
    )
    .await)
}
async fn summary(app: &Router, object: &str) -> Value {
    ok(request(app, "GET", &path(object), None, Value::Null).await)
}
async fn record(app: &Router, actor: &Account, object: &str) -> Value {
    ok(request(
        app,
        "GET",
        &format!("{}/actors/{}", path(object), actor.id),
        None,
        Value::Null,
    )
    .await)
}
fn host() -> RpcBinding {
    RpcBinding::host("reactions-test", "https://babble.test").unwrap()
}
fn envelope(
    method: &str,
    binding: RpcBinding,
    payload: Value,
    key: Option<&str>,
) -> RpcRequestEnvelope {
    let request = RpcRequestEnvelope::new(
        &babble_rpc_catalog().unwrap(),
        "reaction-regression",
        &format!("babble.social.reactions.{method}.v1"),
        binding,
        payload,
    )
    .unwrap();
    match key {
        Some(key) => request.with_idempotency_key(key),
        None => request,
    }
}
async fn rpc(
    app: &Router,
    token: Option<&str>,
    method: &str,
    payload: Value,
    key: Option<&str>,
) -> (StatusCode, Value) {
    request(
        app,
        "POST",
        "/rpc",
        token,
        json!(envelope(method, host(), payload, key)),
    )
    .await
}

#[tokio::test]
async fn reactions_public_records_are_signed_attributed_and_actors_are_independent() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    let object = publish(&app, &alice, "Public reaction target").await;
    let absent = record(&app, &bob, &object).await;
    assert_eq!(
        absent,
        json!({"state":{"author_id":bob.id,"object_id":object,"value":empty(),"revision":0},"action":null})
    );
    let first = set(&app, &alice, &object, like(), 0, "alice-first").await;
    let second = set(&app, &bob, &object, dislike(), 0, "bob-first").await;
    assert_eq!(mine(&app, &alice, &object).await, first);
    assert_eq!(mine(&app, &bob, &object).await, second);
    for actor in [&alice, &bob] {
        let wire = record(&app, actor, &object).await;
        let record: ReactionRecord = serde_json::from_value(wire.clone()).unwrap();
        let identity: Identity = serde_json::from_value(
            ok(request(
                &app,
                "GET",
                &format!("/identities/{}", actor.id),
                None,
                Value::Null,
            )
            .await)["identity"]
                .clone(),
        )
        .unwrap();
        let action = record.action.unwrap();
        assert_eq!(action.payload.state, record.state);
        action.verify(&identity).unwrap();
        let mut changed = action.clone();
        changed.payload.state.revision += 1;
        assert!(changed.verify(&identity).is_err());
        assert_eq!(
            rpc_ok(
                rpc(
                    &app,
                    Some(&bob.token),
                    "record",
                    json!({"object_id":object,"actor_id":actor.id}),
                    None
                )
                .await
            ),
            wire
        );
        assert_eq!(
            rpc_ok(
                rpc(
                    &app,
                    None,
                    "record",
                    json!({"object_id":object,"actor_id":actor.id}),
                    None
                )
                .await
            ),
            wire
        );
    }
    assert_eq!(
        rpc_ok(rpc(&app, None, "summary", json!({"object_id":object}), None).await),
        summary(&app, &object).await
    );
    assert_eq!(
        rpc_ok(
            rpc(
                &app,
                Some(&alice.token),
                "mine",
                json!({"object_id":object}),
                None
            )
            .await
        ),
        first
    );
}

#[tokio::test]
async fn reactions_counters_track_independent_axes_and_withdrawal_without_a_truth_average() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    let carol = register(&app, "carol").await;
    let object = publish(&app, &alice, "Reaction axes").await;
    set(
        &app,
        &alice,
        &object,
        value(Some("like"), Some("not_engaging"), Some("oppose"), Some(0)),
        0,
        "a",
    )
    .await;
    set(
        &app,
        &bob,
        &object,
        value(
            Some("dislike"),
            Some("engaging"),
            Some("support"),
            Some(100),
        ),
        0,
        "b",
    )
    .await;
    set(
        &app,
        &carol,
        &object,
        value(None, None, Some("uncertain"), None),
        0,
        "c",
    )
    .await;
    assert_eq!(
        summary(&app, &object).await,
        json!({"object_id":object,"participants":3,"likes":1,"dislikes":1,"engaging":1,"not_engaging":1,"support":1,"oppose":1,"uncertain":1,"certainty_responses":2})
    );
    let prior = record(&app, &alice, &object).await;
    let withdrawn = set(&app, &alice, &object, empty(), 1, "withdraw").await;
    assert_eq!(withdrawn["revision"], 2);
    let after = record(&app, &alice, &object).await;
    assert_eq!(after["state"], withdrawn);
    assert_eq!(
        after["action"]["payload"]["previous_id"],
        prior["action"]["id"]
    );
    assert_eq!(
        summary(&app, &object).await,
        json!({"object_id":object,"participants":2,"likes":0,"dislikes":1,"engaging":1,"not_engaging":0,"support":1,"oppose":0,"uncertain":1,"certainty_responses":1})
    );
    set(&app, &bob, &object, empty(), 1, "withdraw").await;
    set(&app, &carol, &object, empty(), 1, "withdraw").await;
    assert_eq!(
        summary(&app, &object).await,
        json!({"object_id":object,"participants":0,"likes":0,"dislikes":0,"engaging":0,"not_engaging":0,"support":0,"oppose":0,"uncertain":0,"certainty_responses":0})
    );
}

#[tokio::test]
async fn reactions_guest_forged_actor_and_logged_out_session_never_mutate() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let bob = register(&app, "bob").await;
    let object = publish(&app, &alice, "Authentication boundary").await;
    let uri = format!("{}/mine", path(&object));
    for method in ["GET", "PUT"] {
        assert_eq!(
            request(
                &app,
                method,
                &uri,
                None,
                if method == "GET" {
                    Value::Null
                } else {
                    mutation(like(), 0, "guest")
                }
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
    }
    for method in ["mine", "set"] {
        let payload = if method == "mine" {
            json!({"object_id":object})
        } else {
            json!({"object_id":object,"value":like(),"expected_revision":0})
        };
        assert_eq!(
            rpc(&app, None, method, payload, Some("guest")).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
    for field in ["author_id", "identity_id", "actor_id"] {
        for forged in [&alice.id, &bob.id] {
            let mut body = mutation(like(), 0, "forged");
            body[field] = json!(forged);
            rejected(request(&app, "PUT", &uri, Some(&bob.token), body).await);
            let mut payload = json!({"object_id":object,"value":like(),"expected_revision":0});
            payload[field] = json!(forged);
            rejected(rpc(&app, Some(&bob.token), "set", payload, Some("forged")).await);
        }
    }
    let mut binding = host();
    binding.identity_id = Some(alice.id.clone());
    rejected(
        request(
            &app,
            "POST",
            "/rpc",
            Some(&bob.token),
            json!(envelope(
                "set",
                binding,
                json!({"object_id":object,"value":like(),"expected_revision":0}),
                Some("forged-binding")
            )),
        )
        .await,
    );
    assert_eq!(
        request(
            &app,
            "DELETE",
            "/auth/session",
            Some(&bob.token),
            Value::Null,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&app, "GET", &uri, Some(&bob.token), Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "PUT",
            &uri,
            Some(&bob.token),
            mutation(like(), 0, "logout")
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        rpc(
            &app,
            Some(&bob.token),
            "set",
            json!({"object_id":object,"value":like(),"expected_revision":0}),
            Some("logout")
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(record(&app, &bob, &object).await["state"]["revision"], 0);
    assert_eq!(summary(&app, &object).await["participants"], 0);
}

#[tokio::test]
async fn reactions_cas_exact_retry_cross_transport_and_restart_preserve_later_state() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let object = publish(&app, &alice, "Durable retry").await;
    let first = set(&app, &alice, &object, like(), 0, "first").await;
    let payload = json!({"object_id":object,"value":like(),"expected_revision":0});
    assert_eq!(
        rpc_ok(
            rpc(
                &app,
                Some(&alice.token),
                "set",
                payload.clone(),
                Some("first")
            )
            .await
        ),
        first
    );
    let second_payload = json!({"object_id":object,"value":dislike(),"expected_revision":1});
    let second = rpc_ok(
        rpc(
            &app,
            Some(&alice.token),
            "set",
            second_payload,
            Some("second"),
        )
        .await,
    );
    assert_eq!(
        set(&app, &alice, &object, dislike(), 1, "second").await,
        second
    );
    drop(app);
    let app = f.app();
    assert_eq!(set(&app, &alice, &object, like(), 0, "first").await, first);
    assert_eq!(
        rpc_ok(rpc(&app, Some(&alice.token), "set", payload, Some("first")).await),
        first
    );
    assert_eq!(mine(&app, &alice, &object).await, second);
    for body in [
        mutation(like(), 0, "stale"),
        mutation(like(), 2, "first"),
        mutation(dislike(), 2, "second"),
    ] {
        assert_eq!(
            request(
                &app,
                "PUT",
                &format!("{}/mine", path(&object)),
                Some(&alice.token),
                body
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
    }
    assert_eq!(summary(&app, &object).await["dislikes"], 1);
    assert_eq!(summary(&app, &object).await["likes"], 0);
    for (key, revision) in [("rpc-stale", 0), ("first", 2)] {
        let response = rpc(
            &app,
            Some(&alice.token),
            "set",
            json!({"object_id":object,"value":like(),"expected_revision":revision}),
            Some(key),
        )
        .await;
        assert_eq!(response.0, StatusCode::OK, "{response:?}");
        assert_eq!(response.1["error"]["code"], "CONFLICT", "{response:?}");
        assert_eq!(response.1["error"]["retryable"], false);
    }
    assert_eq!(mine(&app, &alice, &object).await, second);
}

#[tokio::test]
async fn reactions_noop_has_durable_receipt_without_increment_or_new_action() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let object = publish(&app, &alice, "No-op receipts").await;
    let absent = set(&app, &alice, &object, empty(), 0, "empty-noop").await;
    assert_eq!(absent["revision"], 0);
    assert!(record(&app, &alice, &object).await["action"].is_null());
    let first = set(&app, &alice, &object, like(), 0, "first").await;
    let signed = record(&app, &alice, &object).await;
    assert_eq!(
        set(&app, &alice, &object, like(), 1, "same-noop").await,
        first
    );
    assert_eq!(record(&app, &alice, &object).await, signed);
    let second = set(&app, &alice, &object, dislike(), 1, "second").await;
    drop(app);
    let app = f.app();
    assert_eq!(
        set(&app, &alice, &object, empty(), 0, "empty-noop").await,
        absent
    );
    assert_eq!(
        set(&app, &alice, &object, like(), 1, "same-noop").await,
        first
    );
    assert_eq!(mine(&app, &alice, &object).await, second);
}

#[tokio::test]
async fn reactions_invalid_values_and_request_shapes_fail_without_writes() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let object = publish(&app, &alice, "Strict parsing").await;
    let mut invalid = Vec::new();
    for (field, bad) in [
        ("appreciation", json!("love")),
        ("engagement", json!("boring")),
        ("stance", json!("neutral")),
        ("certainty", json!(101)),
        ("certainty", json!(-1)),
        ("certainty", json!(0.5)),
        ("certainty", json!("NaN")),
        ("certainty", json!(true)),
    ] {
        let mut v = value(None, None, Some("support"), None);
        v[field] = bad;
        invalid.push(mutation(v, 0, "invalid"));
    }
    invalid.push(mutation(value(None, None, None, Some(0)), 0, "invalid"));
    for field in ["value", "expected_revision", "idempotency_key"] {
        let mut body = mutation(like(), 0, "invalid");
        body.as_object_mut().unwrap().remove(field);
        invalid.push(body);
    }
    let mut extra = mutation(like(), 0, "invalid");
    extra["extra"] = json!(true);
    invalid.push(extra);
    let mut extra = like();
    extra["extra"] = json!(true);
    invalid.push(mutation(extra, 0, "invalid"));
    invalid.push(mutation(Value::Null, 0, "invalid"));
    for key in [
        "",
        "with space",
        "new\nline",
        "non-ascii-\u{e9}",
        &"x".repeat(257),
    ] {
        invalid.push(mutation(like(), 0, key));
    }
    invalid.push(mutation(like(), 9_007_199_254_740_992, "invalid"));
    for body in invalid {
        let response = request(
            &app,
            "PUT",
            &format!("{}/mine", path(&object)),
            Some(&alice.token),
            body.clone(),
        )
        .await;
        assert!(
            response.0.is_client_error(),
            "accepted {body}: {response:?}"
        );
        let mut payload = body.clone();
        let key = payload.as_object_mut().unwrap().remove("idempotency_key");
        payload["object_id"] = json!(object);
        rejected(
            rpc(
                &app,
                Some(&alice.token),
                "set",
                payload,
                key.as_ref().and_then(Value::as_str),
            )
            .await,
        );
    }
    for token in ["NaN", "Infinity", "-Infinity"] {
        let body = format!(
            r#"{{"value":{{"appreciation":null,"engagement":null,"stance":"support","certainty":{token}}},"expected_revision":0,"idempotency_key":"nonfinite"}}"#
        );
        assert_eq!(
            raw(
                &app,
                "PUT",
                &format!("{}/mine", path(&object)),
                Some(&alice.token),
                body
            )
            .await
            .0,
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(mine(&app, &alice, &object).await["revision"], 0);
    assert_eq!(summary(&app, &object).await["participants"], 0);
}

#[tokio::test]
async fn reactions_each_nullable_axis_must_be_explicit_not_silently_withdrawn() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let object = publish(&app, &alice, "Missing fields must not erase intent").await;
    for field in ["appreciation", "engagement", "stance", "certainty"] {
        let mut v = like();
        v.as_object_mut().unwrap().remove(field);
        let response = request(
            &app,
            "PUT",
            &format!("{}/mine", path(&object)),
            Some(&alice.token),
            mutation(v.clone(), 0, field),
        )
        .await;
        assert!(
            response.0.is_client_error(),
            "missing {field} accepted: {response:?}"
        );
        rejected(
            rpc(
                &app,
                Some(&alice.token),
                "set",
                json!({"object_id":object,"value":v,"expected_revision":0}),
                Some(field),
            )
            .await,
        );
    }
    assert_eq!(mine(&app, &alice, &object).await["revision"], 0);
}

#[tokio::test]
async fn reactions_rpc_rejects_missing_extra_fields_and_payload_idempotency_keys() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let object = publish(&app, &alice, "RPC payload contract").await;
    for method in ["summary", "record", "mine", "set"] {
        let valid = match method {
            "record" => json!({"object_id":object,"actor_id":alice.id}),
            "set" => json!({"object_id":object,"value":like(),"expected_revision":0}),
            _ => json!({"object_id":object}),
        };
        for field in valid.as_object().unwrap().keys() {
            let mut invalid = valid.clone();
            invalid.as_object_mut().unwrap().remove(field);
            rejected(rpc(&app, Some(&alice.token), method, invalid, Some("rpc-shape")).await);
        }
        let mut extra = valid;
        extra["extra"] = json!(true);
        rejected(rpc(&app, Some(&alice.token), method, extra, Some("rpc-shape")).await);
    }
    rejected(rpc(&app, Some(&alice.token), "set", json!({"object_id":object,"value":like(),"expected_revision":0,"idempotency_key":"payload-only"}), None).await);
    assert_eq!(mine(&app, &alice, &object).await["revision"], 0);
}

#[tokio::test]
async fn reactions_unknown_and_malformed_objects_do_not_create_records() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let missing = babble_types::ObjectId::from_hash(&babble_types::Hash::from_bytes(
        b"missing reaction object",
    ))
    .to_string();
    for (object, expected) in [
        (missing.as_str(), StatusCode::NOT_FOUND),
        ("not-an-object", StatusCode::BAD_REQUEST),
    ] {
        for suffix in [
            String::new(),
            "/mine".into(),
            format!("/actors/{}", alice.id),
        ] {
            let response = request(
                &app,
                "GET",
                &format!("{}{suffix}", path(object)),
                Some(&alice.token),
                Value::Null,
            )
            .await;
            assert_eq!(response.0, expected, "{response:?}");
        }
        assert_eq!(
            request(
                &app,
                "PUT",
                &format!("{}/mine", path(object)),
                Some(&alice.token),
                mutation(like(), 0, "missing")
            )
            .await
            .0,
            expected
        );
        for method in ["summary", "mine", "record", "set"] {
            let payload = match method {
                "record" => json!({"object_id":object,"actor_id":alice.id}),
                "set" => json!({"object_id":object,"value":like(),"expected_revision":0}),
                _ => json!({"object_id":object}),
            };
            rejected(rpc(&app, Some(&alice.token), method, payload, Some("missing")).await);
        }
    }
    let object = publish(&app, &alice, "Actor lookup errors").await;
    let unknown_actor =
        babble_types::IdentityId::from_hash(&babble_types::Hash::from_bytes(b"unknown actor"))
            .to_string();
    for (actor, expected) in [
        ("not-an-identity", StatusCode::BAD_REQUEST),
        (unknown_actor.as_str(), StatusCode::NOT_FOUND),
    ] {
        assert_eq!(
            request(
                &app,
                "GET",
                &format!("{}/actors/{actor}", path(&object)),
                None,
                Value::Null
            )
            .await
            .0,
            expected
        );
        rejected(
            rpc(
                &app,
                None,
                "record",
                json!({"object_id":object,"actor_id":actor}),
                None,
            )
            .await,
        );
    }
}

#[tokio::test]
async fn reactions_object_and_surface_bound_mutations_are_denied_even_for_the_author() {
    let f = Fixture::new();
    let state = f.state();
    let app = router(state.clone());
    let alice = register(&app, "alice").await;
    let object = publish(&app, &alice, "Binding boundary").await;
    let first = set(&app, &alice, &object, like(), 0, "first").await;
    for (bound_object, surface) in [(true, false), (false, true), (true, true)] {
        let mut binding = host();
        binding.identity_id = Some(alice.id.clone());
        if bound_object {
            binding.object_id = Some(object.clone());
        }
        if surface {
            binding.surface_session_id = Some("reaction-session".into());
        }
        for method in ["mine", "set"] {
            let payload = if method == "mine" {
                json!({"object_id":object})
            } else {
                json!({"object_id":object,"value":dislike(),"expected_revision":1})
            };
            let env = envelope(method, binding.clone(), payload, Some("bound"));
            rejected(request(&app, "POST", "/rpc", Some(&alice.token), json!(env)).await);
            assert!(babble_api::dispatch_rpc_request(&state, env).error.is_some());
        }
    }
    assert_eq!(mine(&app, &alice, &object).await, first);
    assert_eq!(summary(&app, &object).await["likes"], 1);
}

#[tokio::test]
async fn reactions_native_trusted_identity_binding_works_but_is_not_http_authentication() {
    let f = Fixture::new();
    let state = f.state();
    let app = router(state.clone());
    let alice = register(&app, "alice").await;
    let object = publish(&app, &alice, "Native host authority").await;
    let mut binding = host();
    binding.identity_id = Some(alice.id.clone());
    let env = envelope(
        "set",
        binding,
        json!({"object_id":object,"value":like(),"expected_revision":0}),
        Some("native"),
    );
    assert_eq!(
        request(&app, "POST", "/rpc", None, json!(env)).await.0,
        StatusCode::UNAUTHORIZED
    );
    let response = babble_api::dispatch_rpc_request(&state, env);
    assert!(response.error.is_none(), "{response:?}");
    assert_eq!(response.result.unwrap(), mine(&app, &alice, &object).await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reactions_concurrent_router_requests_have_one_cas_winner_and_no_lost_update() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let object = publish(&app, &alice, "Concurrent reaction intents").await;
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(3));
    let mut tasks = Vec::new();
    for (key, v) in [("left", like()), ("right", dislike())] {
        let app = app.clone();
        let barrier = barrier.clone();
        let token = alice.token.clone();
        let uri = format!("{}/mine", path(&object));
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            request(&app, "PUT", &uri, Some(&token), mutation(v, 0, key)).await
        }));
    }
    barrier.wait().await;
    let mut responses = Vec::new();
    for task in tasks {
        responses.push(task.await.unwrap());
    }
    assert_eq!(
        responses.iter().filter(|r| r.0 == StatusCode::OK).count(),
        1,
        "{responses:?}"
    );
    assert_eq!(
        responses
            .iter()
            .filter(|r| r.0 == StatusCode::CONFLICT)
            .count(),
        1,
        "{responses:?}"
    );
    let winner = &responses.iter().find(|r| r.0 == StatusCode::OK).unwrap().1;
    assert_eq!(winner["revision"], 1);
    assert_eq!(mine(&app, &alice, &object).await, *winner);
    assert_eq!(record(&app, &alice, &object).await["state"], *winner);
    let counts = summary(&app, &object).await;
    assert_eq!(counts["participants"], 1);
    assert_eq!(
        counts["likes"].as_u64().unwrap() + counts["dislikes"].as_u64().unwrap(),
        1
    );
}

#[tokio::test]
async fn reactions_storage_failure_is_retryable_and_does_not_consume_the_intent() {
    let f = Fixture::new();
    let app = f.app();
    let alice = register(&app, "alice").await;
    let object = publish(&app, &alice, "Storage outage retry").await;
    let database = f.0.join("public_reactions/reactions.sqlite3");
    let backup = database.with_extension("backup");
    fs::rename(&database, &backup).unwrap();
    fs::create_dir(&database).unwrap();
    let response = request(
        &app,
        "PUT",
        &format!("{}/mine", path(&object)),
        Some(&alice.token),
        mutation(like(), 0, "storage-retry"),
    )
    .await;
    assert_eq!(response.0, StatusCode::SERVICE_UNAVAILABLE, "{response:?}");
    assert_eq!(response.1["code"], "storage_unavailable");
    let rpc_response = rpc(
        &app,
        Some(&alice.token),
        "set",
        json!({"object_id":object,"value":like(),"expected_revision":0}),
        Some("storage-retry"),
    )
    .await;
    assert_eq!(rpc_response.0, StatusCode::OK, "{rpc_response:?}");
    assert_eq!(rpc_response.1["error"]["code"], "STORAGE_UNAVAILABLE");
    assert_eq!(rpc_response.1["error"]["retryable"], true);
    fs::remove_dir(&database).unwrap();
    fs::rename(backup, database).unwrap();
    assert_eq!(mine(&app, &alice, &object).await["revision"], 0);
    let first = set(&app, &alice, &object, like(), 0, "storage-retry").await;
    assert_eq!(first["revision"], 1);
    assert_eq!(
        rpc_ok(
            rpc(
                &app,
                Some(&alice.token),
                "set",
                json!({"object_id":object,"value":like(),"expected_revision":0}),
                Some("storage-retry")
            )
            .await
        ),
        first
    );
}
