use super::*;

const PREPARE: &str = "/invocations/v1/browser/prepare";

#[tokio::test]
async fn browser_surface_rpc_lazy_consent_and_suspend_loses_executor() {
    for method in ["babel.clipboard.write.v2", "babel.fullscreen.enter.v2"] {
        let f = Fixture::new();
        let principal = f
            .state
            .auth
            .with_store(|s| s.authenticate(&f.token))
            .unwrap();
        let session = crate::auth::start_surface(
            &f.state,
            crate::StartSurfaceSessionRequest {
                object_id: f.controller.clone(),
                role: SurfaceRole::Feed,
                session_id: None,
            },
            &principal,
        )
        .unwrap();
        assert_eq!(
            session.plan.admission,
            babel_runtime::RuntimeAdmissionStatus::Ready
        );
        crate::auth::bind_surface_document(
            &f.state,
            session.id.clone(),
            DOCUMENT.into(),
            &principal,
        )
        .unwrap();
        {
            let mut node = f.state.node.lock().unwrap();
            node.transition_surface_session(
                &session.id,
                babel_runtime::SurfaceLifecycle::Warm,
                "warm",
            )
            .unwrap();
            node.transition_surface_session(
                &session.id,
                babel_runtime::SurfaceLifecycle::Active,
                "visible",
            )
            .unwrap();
        }
        let request = RpcRequestEnvelope::new(
            &babel_rpc_catalog().unwrap(),
            "browser-rpc",
            method,
            RpcBinding::object(
                f.controller.clone(),
                session.id.to_string(),
                "runtime",
                "https://host.example",
                vec![],
            )
            .unwrap(),
            input(&f, "unused", method)["payload"].clone(),
        )
        .unwrap()
        .with_idempotency_key("browser-rpc");
        let headers = Some(("x-babel-surface-document", DOCUMENT));
        let request = serde_json::to_value(request).unwrap();
        let (_, response) = f
            .call("POST", "/rpc", request.clone(), &f.token, headers)
            .await;
        assert_eq!(
            response["error"]["code"], "PERMISSION_REQUIRED",
            "{response}"
        );
        let view = &response["error"]["details"]["invocation"];
        let id = view["invocation_id"].as_str().unwrap();
        assert_eq!(view["origin"]["session_id"], session.id.as_str());
        assert_eq!(
            f.call(
                "POST",
                &path(id, "decision"),
                json!({"decision":"allow_once"}),
                &f.token,
                headers
            )
            .await
            .0,
            StatusCode::OK
        );
        let (code, running) = f
            .call("POST", &path(id, "dispatch"), json!({}), &f.token, headers)
            .await;
        assert_eq!(code, StatusCode::OK, "{running}");
        assert!(!running["execution_ticket"].is_null());
        let (_, retry) = f.call("POST", "/rpc", request, &f.token, headers).await;
        assert_eq!(retry["error"]["code"], "CAPABILITY_DENIED");
        assert!(retry["error"]["details"]["invocation"]["execution_ticket"].is_null());
        f.state
            .node
            .lock()
            .unwrap()
            .transition_surface_session(
                &session.id,
                babel_runtime::SurfaceLifecycle::Suspended,
                "hidden",
            )
            .unwrap();
        let (_, unknown) = f
            .call("GET", &path(id, "status"), json!({}), &f.token, headers)
            .await;
        assert_eq!(unknown["state"]["kind"], "unknown", "{unknown}");
        assert!(unknown["result"].is_null());
        let ack = json!({"dispatch_id":running["state"]["dispatch_id"],"result":{"kind":"failed","code":"context_lost"}});
        let (code, failed) = f
            .call("POST", &path(id, "ack"), ack, &f.token, headers)
            .await;
        assert_eq!(code, StatusCode::OK, "{failed}");
        assert_eq!(failed["state"]["kind"], "failed");
    }
}

#[tokio::test]
async fn browser_prepare_rejects_unbounded_unknown_payload_and_rebound_context() {
    let f = Fixture::new();
    f.register().await;
    for payload in [
        json!({"text":"x","extra":true}),
        json!({"text":"x".repeat(65536)}),
        json!({}),
    ] {
        let mut request = input(&f, "invalid", "babel.clipboard.write.v2");
        request["payload"] = payload;
        assert!(f.host("POST", PREPARE, request).await.0.is_client_error());
    }
    assert!(
        f.state
            .node
            .lock()
            .unwrap()
            .store()
            .list_invocations()
            .unwrap()
            .is_empty()
    );
    let initial = approved(&f, "binding", "babel.clipboard.write.v2").await;
    let id: InvocationId = serde_json::from_value(initial["invocation_id"].clone()).unwrap();
    let mut node = f.state.node.lock().unwrap();
    let record = node.invocation_by_id(&id).unwrap().unwrap();
    for field in ["login", "version", "policy", "epoch", "document", "object"] {
        let mut ctx = record.intent().context.clone();
        match field {
            "login" => ctx.login_id = "another-login".into(),
            "version" => ctx.object_version = Hash::from_bytes(b"different-version"),
            "policy" => ctx.policy_revision = Hash::from_bytes(b"different-policy"),
            "epoch" => ctx.context_epoch = Hash::from_bytes(b"different-epoch"),
            "object" => {
                ctx.object_id =
                    babel_types::ObjectId::from_hash(&Hash::from_bytes(b"different-object"))
            }
            _ => {
                ctx.origin = InvocationOrigin::HostAction {
                    document_id: OTHER_DOCUMENT.into(),
                }
            }
        }
        assert!(
            node.dispatch_browser_invocation(&ctx, "binding", &id)
                .is_err(),
            "{field}"
        );
    }
    assert_eq!(
        node.invocation_by_id(&id).unwrap().unwrap().state(),
        &InvocationState::Approved
    );
}

fn input(f: &Fixture, key: &str, method: &str) -> Value {
    json!({"origin":{"kind":"host_action","document_id":DOCUMENT},"object_id":f.controller,
        "method":method,"request_key":key,"payload":if method.contains("clipboard") {
            json!({"text":"exact clipboard text\n"})
        } else { json!({"target_hint":"surface-root","navigation_ui":"hide"}) }})
}

fn path(id: &str, operation: &str) -> String {
    format!("/invocations/v1/browser/{id}/{operation}")
}

async fn prepared(f: &Fixture, key: &str, method: &str) -> Value {
    let (code, value) = f.host("POST", PREPARE, input(f, key, method)).await;
    assert_eq!(code, StatusCode::OK, "{value}");
    assert!(value["execution_ticket"].is_null());
    value
}

async fn approved(f: &Fixture, key: &str, method: &str) -> Value {
    let value = prepared(f, key, method).await;
    let id = value["invocation_id"].as_str().unwrap();
    let (code, approved) = f
        .host(
            "POST",
            &path(id, "decision"),
            json!({"decision":"allow_once"}),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{approved}");
    assert_eq!(approved["state"]["kind"], "approved");
    value
}

#[tokio::test]
async fn browser_dispatch_once_typed_ack_retries_conflicts_and_social_isolation() {
    for method in ["babel.clipboard.write.v2", "babel.fullscreen.enter.v2"] {
        let f = Fixture::new();
        f.register().await;
        let initial = approved(&f, "once", method).await;
        let id = initial["invocation_id"].as_str().unwrap();
        let dispatch_path = path(id, "dispatch");
        let (first, second) = tokio::join!(
            f.host("POST", &dispatch_path, json!({})),
            f.host("POST", &dispatch_path, json!({}))
        );
        assert_eq!(first.0, StatusCode::OK, "{:?}", first);
        assert_eq!(second.0, StatusCode::OK, "{:?}", second);
        assert_ne!(
            first.1["execution_ticket"].is_null(),
            second.1["execution_ticket"].is_null()
        );
        let running = if first.1["execution_ticket"].is_null() {
            second.1
        } else {
            first.1
        };
        assert_eq!(running["execution_ticket"]["executor"], "babel.browser.v1");
        assert_eq!(
            running["execution_ticket"]["dispatch_id"],
            running["state"]["dispatch_id"]
        );
        assert!(running["result"].is_null());
        let retry = prepared(&f, "once", method).await;
        assert_eq!(retry["state"]["kind"], "running");
        assert_eq!(retry["deadline"], initial["deadline"]);
        let result = if method.contains("clipboard") {
            json!({"kind":"clipboard_write","written":true})
        } else {
            json!({"kind":"fullscreen_enter","entered":true})
        };
        let ack = json!({"dispatch_id":running["state"]["dispatch_id"],"result":result});
        let mut wrong = ack.clone();
        wrong["dispatch_id"] = json!(Hash::from_bytes(b"other dispatch"));
        assert_eq!(
            f.host("POST", &path(id, "ack"), wrong).await.0,
            StatusCode::CONFLICT
        );
        let mut wrong = ack.clone();
        wrong["result"] = json!({"kind":"clipboard_write","written":false});
        assert_eq!(
            f.host("POST", &path(id, "ack"), wrong).await.0,
            StatusCode::CONFLICT
        );
        for _ in 0..2 {
            let (code, completed) = f.host("POST", &path(id, "ack"), ack.clone()).await;
            assert_eq!(code, StatusCode::OK, "{completed}");
            assert_eq!(completed["state"]["kind"], "completed");
            assert_eq!(completed["state"]["outcome"]["result"], result);
            assert_eq!(completed["result"], result);
            assert!(completed["execution_ticket"].is_null());
        }
        let failure = json!({"dispatch_id":running["state"]["dispatch_id"],"result":{"kind":"failed","code":"native_error"}});
        assert_eq!(
            f.host("POST", &path(id, "ack"), failure).await.0,
            StatusCode::CONFLICT
        );
        let (_, status) = f.host("GET", &path(id, "status"), json!({})).await;
        assert!(status["execution_ticket"].is_null());
        assert_eq!(status["result"], result);
        let (_, retry) = f.host("POST", &dispatch_path, json!({})).await;
        assert!(retry["execution_ticket"].is_null());
        assert!(
            f.host("POST", &format!("/invocations/v1/{id}/execute"), json!({}))
                .await
                .0
                .is_client_error()
        );
        let mut changed = input(&f, "once", method);
        changed["payload"] = json!({"text":"changed"});
        assert!(f.host("POST", PREPARE, changed).await.0.is_client_error());
    }
}

#[tokio::test]
async fn browser_ack_failed_is_durable_bound_and_never_redispatched() {
    let f = Fixture::new();
    f.register().await;
    let initial = approved(&f, "failure", "babel.fullscreen.enter.v2").await;
    let id = initial["invocation_id"].as_str().unwrap();
    let (_, running) = f.host("POST", &path(id, "dispatch"), json!({})).await;
    let ack = json!({"dispatch_id":running["state"]["dispatch_id"],"result":{"kind":"failed","code":"context_lost"}});
    for _ in 0..2 {
        let (code, result) = f.host("POST", &path(id, "ack"), ack.clone()).await;
        assert_eq!(code, StatusCode::OK, "{result}");
        assert_eq!(
            result["state"],
            json!({"kind":"failed","code":"context_lost"})
        );
        assert_eq!(result["result"], ack["result"]);
    }
    let mut changed = ack.clone();
    changed["result"]["code"] = json!("native_error");
    assert_eq!(
        f.host("POST", &path(id, "ack"), changed).await.0,
        StatusCode::CONFLICT
    );
    let mut changed = ack;
    changed["dispatch_id"] = json!(Hash::from_bytes(b"different"));
    assert_eq!(
        f.host("POST", &path(id, "ack"), changed).await.0,
        StatusCode::CONFLICT
    );
    let (_, retry) = f.host("POST", &path(id, "dispatch"), json!({})).await;
    assert_eq!(retry["state"]["kind"], "failed");
    assert!(retry["execution_ticket"].is_null());
}

#[tokio::test]
async fn browser_auth_other_login_document_actor_and_anonymous_cannot_obtain_or_ack() {
    let f = Fixture::new();
    f.register().await;
    let initial = approved(&f, "auth", "babel.clipboard.write.v2").await;
    let id = initial["invocation_id"].as_str().unwrap();
    let (_, running) = f.host("POST", &path(id, "dispatch"), json!({})).await;
    let ack = json!({"dispatch_id":running["state"]["dispatch_id"],"result":{"kind":"clipboard_write","written":true}});
    let other = f
        .state
        .node
        .lock()
        .unwrap()
        .create_identity(IdentityKind::Person, "other")
        .unwrap();
    let other_token = f
        .state
        .auth
        .with_store(|s| Ok(s.issue(other.id.as_str())?.0))
        .unwrap();
    for (token, header, document) in [
        (&f.second_login, "x-babel-host-document", DOCUMENT),
        (&other_token, "x-babel-host-document", DOCUMENT),
        (&f.token, "x-babel-host-document", OTHER_DOCUMENT),
        (&f.token, "x-babel-surface-document", DOCUMENT),
        (&String::new(), "x-babel-host-document", DOCUMENT),
    ] {
        for (operation, method, body) in [
            ("status", "GET", json!({})),
            ("dispatch", "POST", json!({})),
            ("ack", "POST", ack.clone()),
        ] {
            let (code, _) = f
                .call(
                    method,
                    &path(id, operation),
                    body,
                    token,
                    Some((header, document)),
                )
                .await;
            assert!(
                matches!(code, StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED),
                "{operation}: {code}"
            );
        }
    }
}

#[tokio::test]
async fn browser_cancel_deny_deadline_and_restart_never_fabricate_success() {
    let f = Fixture::new();
    f.register().await;
    for operation in ["cancel", "decision"] {
        let initial = prepared(&f, operation, "babel.clipboard.write.v2").await;
        let id = initial["invocation_id"].as_str().unwrap();
        let (_, terminal) = f
            .host("POST", &path(id, operation), json!({"decision":"deny"}))
            .await;
        assert_eq!(
            terminal["state"]["kind"],
            if operation == "cancel" {
                "cancelled"
            } else {
                "denied"
            }
        );
        let (_, retry) = f.host("POST", &path(id, "dispatch"), json!({})).await;
        assert_eq!(retry["state"], terminal["state"]);
        assert!(retry["execution_ticket"].is_null());
    }
    let mut short = input(&f, "expired", "babel.clipboard.write.v2");
    short["timeout_ms"] = json!(100);
    let (code, initial) = f.host("POST", PREPARE, short).await;
    assert_eq!(code, StatusCode::OK, "{initial}");
    tokio::time::sleep(std::time::Duration::from_millis(130)).await;
    let (_, expired) = f
        .host(
            "GET",
            &path(initial["invocation_id"].as_str().unwrap(), "status"),
            json!({}),
        )
        .await;
    assert_eq!(expired["state"]["kind"], "expired");
    let pending = prepared(&f, "restart-pending", "babel.clipboard.write.v2").await;
    let initial = approved(&f, "restart-running", "babel.clipboard.write.v2").await;
    let id = initial["invocation_id"].as_str().unwrap();
    let (_, running) = f.host("POST", &path(id, "dispatch"), json!({})).await;
    *f.state.node.lock().unwrap() = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    let (code, unknown) = f.host("GET", &path(id, "status"), json!({})).await;
    assert_eq!(code, StatusCode::OK, "{unknown}");
    assert_eq!(unknown["state"]["kind"], "unknown");
    assert!(unknown["result"].is_null());
    assert!(unknown["execution_ticket"].is_null());
    let (_, invalid) = f
        .host(
            "GET",
            &path(pending["invocation_id"].as_str().unwrap(), "status"),
            json!({}),
        )
        .await;
    assert_eq!(invalid["state"]["kind"], "invalidated");
    let ack = json!({"dispatch_id":running["state"]["dispatch_id"],"result":{"kind":"clipboard_write","written":true}});
    let (code, completed) = f.host("POST", &path(id, "ack"), ack).await;
    assert_eq!(code, StatusCode::OK, "{completed}");
    assert_eq!(completed["state"]["kind"], "completed");
}

#[tokio::test]
async fn browser_dispatch_atomic_aggregate_quota_blocks_preapproved_excess() {
    let f = Fixture::new();
    f.register().await;
    let mut records = Vec::new();
    for i in 0..11 {
        records.push(approved(&f, &format!("quota-{i}"), "babel.fullscreen.enter.v2").await);
    }
    for record in &records[..10] {
        let (code, running) = f
            .host(
                "POST",
                &path(record["invocation_id"].as_str().unwrap(), "dispatch"),
                json!({}),
            )
            .await;
        assert_eq!(code, StatusCode::OK, "{running}");
        assert!(!running["execution_ticket"].is_null());
    }
    let (code, error) = f
        .host(
            "POST",
            &path(records[10]["invocation_id"].as_str().unwrap(), "dispatch"),
            json!({}),
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT, "{error}");
    assert!(error.to_string().contains("quota"));
    let node = f.state.node.lock().unwrap();
    assert_eq!(
        node.store()
            .list_invocations()
            .unwrap()
            .iter()
            .filter(|r| r.consumed_at().is_some())
            .count(),
        10
    );
    assert!(
        node.clipboard_write(
            &babel_types::ObjectId::new_unchecked(f.controller.clone()),
            "text",
            &[]
        )
        .is_err()
    );
    assert!(
        node.fullscreen_enter(
            &babel_types::ObjectId::new_unchecked(f.controller.clone()),
            None,
            &[]
        )
        .is_err()
    );
}
