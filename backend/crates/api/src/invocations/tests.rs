use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use babble_authoring::ObjectDraft;
use babble_identity::IdentityKind;
use babble_judgment_local::LocalProvider;
use babble_object::{CapabilityRequest, Resource, Surface, SurfaceRole, SurfaceTarget};
use babble_rpc::{RpcBinding, babble_rpc_catalog};
use babble_types::Hash;
use serde_json::json;
use tower::ServiceExt;

const DOCUMENT: &str = "00000000-0000-4000-8000-000000000001";
const OTHER_DOCUMENT: &str = "00000000-0000-4000-8000-000000000002";

mod browser;

#[tokio::test]
async fn safety_http_execute_rechecks_block_after_host_approval() {
    for action in ["follow", "reply", "share", "unfollow"] {
        let mut f = Fixture::new();
        let (owner, actor) = {
            let mut node = f.state.node.lock().unwrap();
            let owner = node.create_identity(IdentityKind::Person, "target-owner").unwrap();
            let target = node.publish_text(&owner.id, "other target").unwrap();
            let actor = babble_types::IdentityId::new_unchecked(f.actor.clone());
            let draft = ObjectDraft::text("safety controller").unwrap().with_capability(CapabilityRequest {
                id: format!("babble.social.{action}"), version: 1, scope: json!({"object_id":target.id})
            }).unwrap();
            f.controller = node.publish_draft(&actor, draft).unwrap().id.to_string();
            f.target = target.id.to_string();
            if action == "unfollow" {
                node.publish_edge(&actor, babble_types::ObjectId::new_unchecked(f.controller.clone()),
                    target.id, babble_graph::Relation::Follows, babble_graph::EdgeOrigin::HumanAssertion).unwrap();
            }
            (owner.id, actor)
        };
        f.register().await;
        let payload = if matches!(action, "reply" | "share") {
            json!({"author_id":f.actor,"target_object_id":f.target,"text":"approved"})
        } else { json!({"author_id":f.actor,"target_object_id":f.target}) };
        let (status, prepared) = f.host("POST", "/invocations/v1/prepare", json!({
            "origin":{"kind":"host_action","document_id":DOCUMENT}, "object_id":f.controller,
            "method":format!("babble.social.{action}"),"request_key":"safety-invocation","payload":payload
        })).await;
        assert_eq!(status, StatusCode::OK, "{prepared}");
        let id = prepared["invocation_id"].as_str().unwrap();
        let (status, body) = f.host("POST", &format!("/invocations/v1/{id}/decision"), json!({"decision":"allow_once"})).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        f.state.node.lock().unwrap().set_safety(&owner, &actor, true, false, 0, "block").unwrap();
        let (status, body) = f.host("POST", &format!("/invocations/v1/{id}/execute"), json!({})).await;
        if action == "unfollow" { assert_eq!(status, StatusCode::OK, "{body}"); }
        else {
            assert_eq!(status, StatusCode::CONFLICT, "{body}");
            assert!(body.to_string().contains("interaction unavailable"));
            f.state.node.lock().unwrap().set_safety(&owner, &actor, false, true, 1, "unblock").unwrap();
            let (status, body) = f.host("POST", &format!("/invocations/v1/{id}/execute"), json!({})).await;
            assert_eq!(status, StatusCode::OK, "{body}");
        }
    }
}

struct Fixture {
    state: ApiState<LocalProvider>,
    root: std::path::PathBuf,
    token: String,
    second_login: String,
    actor: String,
    controller: String,
    target: String,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "babble-invocation-api-{}",
            crate::auth::random_token().unwrap()
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let actor = node
            .create_identity(IdentityKind::Person, "consent")
            .unwrap();
        let target = node
            .publish_draft(&actor.id, ObjectDraft::text("target").unwrap())
            .unwrap();
        let hash = Hash::from_bytes(b"surface bytes");
        let mut draft = ObjectDraft::text("controller")
            .unwrap()
            .with_resource(Resource {
                uri: "app.js".into(),
                media_type: "text/javascript".into(),
                integrity: hash.clone(),
            })
            .unwrap()
            .with_surface(Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Web,
                entry: "app.js".into(),
                integrity: Some(hash),
            })
            .unwrap();
        for action in ["follow", "unfollow", "share", "reply"] {
            draft = draft
                .with_capability(CapabilityRequest {
                    id: format!("babble.social.{action}"),
                    version: 1,
                    scope: json!({"object_id":target.id}),
                })
                .unwrap();
        }
        for capability in ["babble.clipboard.write", "babble.fullscreen.enter"] {
            draft = draft.with_capability(CapabilityRequest {
                id: capability.into(), version: 1, scope: json!({}),
            }).unwrap();
        }
        let controller = node.publish_draft(&actor.id, draft).unwrap();
        let state = ApiState::new(node);
        let (token, second_login) = state
            .auth
            .with_store(|store| {
                Ok((
                    store.issue(actor.id.as_str())?.0,
                    store.issue(actor.id.as_str())?.0,
                ))
            })
            .unwrap();
        Self {
            state,
            root,
            token,
            second_login,
            actor: actor.id.to_string(),
            controller: controller.id.to_string(),
            target: target.id.to_string(),
        }
    }

    async fn call(
        &self,
        method: &str,
        path: &str,
        body: Value,
        token: &str,
        document: Option<(&str, &str)>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {token}"));
        if let Some((header, value)) = document {
            request = request.header(header, value);
        }
        let response = crate::router(self.state.clone())
            .oneshot(
                request
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn host(&self, method: &str, path: &str, body: Value) -> (StatusCode, Value) {
        self.call(
            method,
            path,
            body,
            &self.token,
            Some(("x-babble-host-document", DOCUMENT)),
        )
        .await
    }

    async fn register(&self) {
        let (status, _) = self
            .call(
                "PUT",
                &format!("/invocations/v1/documents/{DOCUMENT}"),
                json!({"object_id":self.controller}),
                &self.token,
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK);
    }

    fn prepare(&self, key: &str) -> Value {
        json!({"origin":{"kind":"host_action","document_id":DOCUMENT}, "object_id":self.controller,
            "method":"babble.social.reply","request_key":key,
            "payload":{"author_id":self.actor,"target_object_id":self.target,"text":"approved text"}})
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[tokio::test]
async fn invocation_context_storage_failure_preserves_pending_record_for_retry() {
    let f = Fixture::new();
    f.register().await;
    let (_, prepared) = f
        .host(
            "POST",
            "/invocations/v1/prepare",
            f.prepare("storage-retry"),
        )
        .await;
    let id = prepared["invocation_id"].as_str().unwrap();
    let db = rusqlite::Connection::open(f.root.join("auth/accounts.sqlite3")).unwrap();
    db.execute(
        "ALTER TABLE host_documents RENAME TO unavailable_host_documents",
        [],
    )
    .unwrap();
    let (code, error) = f
        .host(
            "POST",
            &format!("/invocations/v1/{id}/decision"),
            json!({"decision":"allow_once"}),
        )
        .await;
    assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR, "{error}");
    assert_eq!(error["code"], "internal_error");
    {
        let node = f.state.node.lock().unwrap();
        let records = node.store().list_invocations().unwrap();
        assert_eq!(records[0].revision(), 0);
        assert!(matches!(records[0].state(), InvocationState::Pending));
        assert!(node.store().list_edges().unwrap().is_empty());
    }
    db.execute(
        "ALTER TABLE unavailable_host_documents RENAME TO host_documents",
        [],
    )
    .unwrap();
    let (code, approved) = f
        .host(
            "POST",
            &format!("/invocations/v1/{id}/decision"),
            json!({"decision":"allow_once"}),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{approved}");
    assert_eq!(approved["state"]["kind"], "approved");
}

#[tokio::test]
async fn invocation_completed_host_recovery_is_exact_login_bound_and_read_only_after_reload() {
    let mut f = Fixture::new();
    f.register().await;
    let input = f.prepare("lost-ack");
    let mut recovery = input.clone();
    recovery.as_object_mut().unwrap().remove("origin");
    let recover = "/invocations/v1/recover";
    assert_eq!(
        f.call("POST", recover, recovery.clone(), &f.token, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (_, prepared) = f.host("POST", "/invocations/v1/prepare", input).await;
    let id = prepared["invocation_id"].as_str().unwrap();
    assert_eq!(
        f.call("POST", recover, recovery.clone(), &f.token, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.host(
            "POST",
            &format!("/invocations/v1/{id}/decision"),
            json!({"decision":"allow_once"})
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.call("POST", recover, recovery.clone(), &f.token, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    let (code, completed) = f
        .host("POST", &format!("/invocations/v1/{id}/execute"), json!({}))
        .await;
    assert_eq!(code, StatusCode::OK, "{completed}");
    let (_, pending) = f
        .host(
            "POST",
            "/invocations/v1/prepare",
            f.prepare("still-pending"),
        )
        .await;
    let pending_id = pending["invocation_id"].as_str().unwrap();
    rusqlite::Connection::open(f.root.join("auth/accounts.sqlite3"))
        .unwrap()
        .execute("UPDATE host_documents SET lease_expires_at=0", [])
        .unwrap();
    assert_eq!(
        f.host("POST", &format!("/invocations/v1/{id}/execute"), json!({}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (code, restored) = f
        .call("POST", recover, recovery.clone(), &f.token, None)
        .await;
    assert_eq!(code, StatusCode::OK, "{restored}");
    assert_eq!(restored, completed);
    // A new process epoch and a new page document do not change the saved outcome.
    f.state = ApiState::new(LocalNode::open(&f.root, LocalProvider::default()).unwrap());
    assert_eq!(
        f.call(
            "PUT",
            &format!("/invocations/v1/documents/{OTHER_DOCUMENT}"),
            json!({"object_id":f.controller}),
            &f.token,
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        f.call("POST", recover, recovery.clone(), &f.token, None)
            .await,
        (StatusCode::OK, completed.clone())
    );
    assert_eq!(
        f.call("POST", recover, recovery.clone(), &f.second_login, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.host("POST", recover, recovery.clone()).await.0,
        StatusCode::FORBIDDEN
    );
    for field in ["text", "target_object_id"] {
        let mut changed = recovery.clone();
        changed["payload"][field] = if field == "text" {
            json!("changed")
        } else {
            json!(f.controller)
        };
        let (code, body) = f.call("POST", recover, changed, &f.token, None).await;
        assert_eq!(code, StatusCode::CONFLICT, "{body}");
        assert!(body.get("result").is_none());
    }
    let mut changed = recovery.clone();
    changed["method"] = json!("babble.social.share");
    assert_eq!(
        f.call("POST", recover, changed, &f.token, None).await.0,
        StatusCode::CONFLICT
    );
    let mut forged = recovery.clone();
    forged["payload"]["author_id"] = json!("forged-author");
    assert_eq!(
        f.call("POST", recover, forged, &f.token, None).await.0,
        StatusCode::FORBIDDEN
    );
    let mut pending_recovery = recovery.clone();
    pending_recovery["request_key"] = json!("still-pending");
    assert_eq!(
        f.call("POST", recover, pending_recovery, &f.token, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.call(
            "POST",
            &format!("/invocations/v1/{pending_id}/execute"),
            json!({}),
            &f.token,
            Some(("x-babble-host-document", OTHER_DOCUMENT))
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    {
        let node = f.state.node.lock().unwrap();
        assert_eq!(node.store().list_edges().unwrap().len(), 1);
        assert_eq!(node.store().list_objects().unwrap().len(), 3);
        let records = node.store().list_invocations().unwrap();
        let record = records.iter().find(|r| r.id().as_str() == id).unwrap();
        assert_eq!(record.revision(), 2);
    }
    assert_eq!(
        f.call("DELETE", "/auth/session", json!({}), &f.token, None)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.call("POST", recover, recovery, &f.token, None).await.0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn invocation_host_retry_consumes_once_and_returns_original_receipt() {
    let f = Fixture::new();
    f.register().await;
    let input = f.prepare("durable-operation");
    let (code, prepared) = f
        .host("POST", "/invocations/v1/prepare", input.clone())
        .await;
    assert_eq!(code, StatusCode::OK, "{prepared}");
    assert_eq!(prepared["state"]["kind"], "pending");
    let id = prepared["invocation_id"].as_str().unwrap();
    let (_, retry) = f
        .host("POST", "/invocations/v1/prepare", input.clone())
        .await;
    assert_eq!(prepared, retry);
    let (code, _) = f
        .host("POST", &format!("/invocations/v1/{id}/execute"), json!({}))
        .await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (code, _) = f
        .host(
            "POST",
            &format!("/invocations/v1/{id}/decision"),
            json!({"decision":"allow_once"}),
        )
        .await;
    assert_eq!(code, StatusCode::OK);
    let (code, result) = f
        .host("POST", &format!("/invocations/v1/{id}/execute"), json!({}))
        .await;
    assert_eq!(code, StatusCode::OK, "{result}");
    assert_eq!(result["state"]["kind"], "completed");
    assert!(result["result"]["receipt"]["request"]["fingerprint"].is_string());
    assert!(result["result"]["receipt"].get("grant_id").is_none());
    let (_, retry) = f
        .host("POST", "/invocations/v1/prepare", input.clone())
        .await;
    assert_eq!(result, retry);
    let (_, retry) = f
        .host("POST", &format!("/invocations/v1/{id}/execute"), json!({}))
        .await;
    assert_eq!(result, retry);
    let (_, cancelled) = f
        .host("POST", &format!("/invocations/v1/{id}/cancel"), json!({}))
        .await;
    assert_eq!(result, cancelled);
    let mut changed = input;
    changed["payload"]["text"] = json!("forged text");
    assert_eq!(
        f.host("POST", "/invocations/v1/prepare", changed).await.0,
        StatusCode::CONFLICT
    );
    let node = f.state.node.lock().unwrap();
    let records = node.store().list_invocations().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].revision(), 2);
    assert_eq!(node.store().list_objects().unwrap().len(), 3);
    assert_eq!(node.store().list_edges().unwrap().len(), 1);
}

#[tokio::test]
async fn invocation_rejects_forged_login_document_author_and_implicit_host() {
    let f = Fixture::new();
    let input = f.prepare("forgery");
    assert_eq!(
        f.host("POST", "/invocations/v1/prepare", input.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    f.register().await;
    let (_, prepared) = f
        .host("POST", "/invocations/v1/prepare", input.clone())
        .await;
    let path = format!(
        "/invocations/v1/{}/decision",
        prepared["invocation_id"].as_str().unwrap()
    );
    for (token, document) in [(&f.second_login, DOCUMENT), (&f.token, OTHER_DOCUMENT)] {
        assert_eq!(
            f.call(
                "POST",
                &path,
                json!({"decision":"allow_once"}),
                token,
                Some(("x-babble-host-document", document))
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        f.call(
            "POST",
            &path,
            json!({"decision":"allow_once"}),
            &f.token,
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut forged = input.clone();
    forged["request_key"] = json!("forged-author");
    forged["payload"]["author_id"] =
        json!("id:0000000000000000000000000000000000000000000000000000000000000000");
    assert_eq!(
        f.host("POST", "/invocations/v1/prepare", forged).await.0,
        StatusCode::FORBIDDEN
    );
    let mut extra = input;
    extra["context_epoch"] = json!("caller-epoch");
    assert_eq!(
        f.host("POST", "/invocations/v1/prepare", extra).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let catalog = babble_rpc_catalog().unwrap();
    let mut legacy_binding = RpcBinding::host("host", "https://host.example").unwrap();
    legacy_binding.object_id = Some(f.controller.clone());
    let request = RpcRequestEnvelope::new(
        &catalog,
        "legacy",
        "babble.social.reply.v1",
        legacy_binding,
        json!({"author_id":f.actor,"target_object_id":f.target,"text":"legacy"}),
    )
    .unwrap()
    .with_idempotency_key("legacy");
    let (_, response) = f
        .call(
            "POST",
            "/rpc",
            serde_json::to_value(request).unwrap(),
            &f.token,
            None,
        )
        .await;
    assert_eq!(response["error"]["code"], "UNSUPPORTED_VERSION");
    assert_eq!(
        response["error"]["details"]["supported_method"],
        "babble.social.reply"
    );
}

#[tokio::test]
async fn invocation_document_retirement_and_cancel_are_terminal() {
    let f = Fixture::new();
    f.register().await;
    let (_, prepared) = f
        .host("POST", "/invocations/v1/prepare", f.prepare("cancelled"))
        .await;
    let id = prepared["invocation_id"].as_str().unwrap();
    let (_, cancelled) = f
        .host("POST", &format!("/invocations/v1/{id}/cancel"), json!({}))
        .await;
    assert_eq!(cancelled["state"]["kind"], "cancelled");
    let (_, retried) = f
        .host(
            "POST",
            &format!("/invocations/v1/{id}/decision"),
            json!({"decision":"allow_once"}),
        )
        .await;
    assert_eq!(retried["state"]["kind"], "cancelled");
    let (_, prepared) = f
        .host("POST", "/invocations/v1/prepare", f.prepare("retire"))
        .await;
    let id = prepared["invocation_id"].as_str().unwrap();
    f.host(
        "POST",
        &format!("/invocations/v1/{id}/decision"),
        json!({"decision":"allow_once"}),
    )
    .await;
    assert_eq!(
        f.host(
            "DELETE",
            &format!("/invocations/v1/documents/{DOCUMENT}"),
            json!({})
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        f.host("POST", &format!("/invocations/v1/{id}/execute"), json!({}))
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.call(
            "PUT",
            &format!("/invocations/v1/documents/{DOCUMENT}"),
            json!({"object_id":f.controller}),
            &f.token,
            None
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let node = f.state.node.lock().unwrap();
    assert_eq!(node.store().list_objects().unwrap().len(), 2);
    assert!(
        node.store()
            .list_invocations()
            .unwrap()
            .iter()
            .any(|r| matches!(r.state(), InvocationState::Invalidated { .. }))
    );
}

#[tokio::test]
async fn invocation_surface_requires_active_bound_document_and_suspend_never_revives_approval() {
    let f = Fixture::new();
    let principal = f
        .state
        .auth
        .with_store(|store| store.authenticate(&f.token))
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
        babble_runtime::RuntimeAdmissionStatus::Ready
    );
    assert!(
        session
            .plan
            .capability_decisions
            .iter()
            .all(|decision| decision.status
                == babble_capabilities::CapabilityDecisionStatus::RequiresUser
                && decision.grant.is_none())
    );
    crate::auth::bind_surface_document(&f.state, session.id.clone(), DOCUMENT.into(), &principal)
        .unwrap();
    let binding = RpcBinding::object(
        f.controller.clone(),
        session.id.to_string(),
        "surface-runtime",
        "https://host.example",
        vec![],
    )
    .unwrap();
    let request = RpcRequestEnvelope::new(
        &babble_rpc_catalog().unwrap(),
        "surface-call",
        "babble.social.reply",
        binding,
        json!({"author_id":f.actor,"target_object_id":f.target,"text":"surface reply"}),
    )
    .unwrap()
    .with_idempotency_key("surface-key");
    let request = serde_json::to_value(request).unwrap();
    let headers = Some(("x-babble-surface-document", DOCUMENT));
    let (_, prefetch) = f
        .call("POST", "/rpc", request.clone(), &f.token, headers)
        .await;
    assert_eq!(prefetch["error"]["code"], "CAPABILITY_DENIED");
    {
        let mut node = f.state.node.lock().unwrap();
        node.transition_surface_session(&session.id, babble_runtime::SurfaceLifecycle::Warm, "warm")
            .unwrap();
        node.transition_surface_session(
            &session.id,
            babble_runtime::SurfaceLifecycle::Active,
            "visible",
        )
        .unwrap();
    }
    let (code, prompt) = f
        .call("POST", "/rpc", request.clone(), &f.token, headers)
        .await;
    assert_eq!(code, StatusCode::OK, "{prompt}");
    assert_eq!(prompt["error"]["code"], "PERMISSION_REQUIRED", "{prompt}");
    let invocation = &prompt["error"]["details"]["invocation"];
    let id = invocation["invocation_id"].as_str().unwrap();
    assert!(invocation.get("login_id").is_none());
    let path = format!("/invocations/v1/{id}/decision");
    assert_eq!(
        f.call(
            "POST",
            &path,
            json!({"decision":"allow_once"}),
            &f.second_login,
            headers
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.call(
            "POST",
            &path,
            json!({"decision":"allow_once"}),
            &f.token,
            headers
        )
        .await
        .0,
        StatusCode::OK
    );
    {
        let mut node = f.state.node.lock().unwrap();
        node.transition_surface_session(
            &session.id,
            babble_runtime::SurfaceLifecycle::Suspended,
            "hidden",
        )
        .unwrap();
        node.transition_surface_session(
            &session.id,
            babble_runtime::SurfaceLifecycle::Warm,
            "rewarm",
        )
        .unwrap();
        node.transition_surface_session(
            &session.id,
            babble_runtime::SurfaceLifecycle::Active,
            "visible again",
        )
        .unwrap();
    }
    let (_, status) = f
        .call(
            "GET",
            &format!("/invocations/v1/{id}/status"),
            json!({}),
            &f.token,
            headers,
        )
        .await;
    assert_eq!(status["state"]["kind"], "invalidated", "{status}");
    assert_eq!(
        f.call(
            "POST",
            &format!("/invocations/v1/{id}/execute"),
            json!({}),
            &f.token,
            headers
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        f.state
            .node
            .lock()
            .unwrap()
            .store()
            .list_objects()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn invocation_expiry_retry_cannot_refresh_budget_and_credentials_rechecked_under_lock() {
    let f = Fixture::new();
    f.register().await;
    let mut input = f.prepare("expires");
    input["timeout_ms"] = json!(100);
    let (code, prepared) = f
        .host("POST", "/invocations/v1/prepare", input.clone())
        .await;
    assert_eq!(code, StatusCode::OK, "{prepared}");
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;
    input["timeout_ms"] = json!(30_000);
    let (_, expired) = f.host("POST", "/invocations/v1/prepare", input).await;
    assert_eq!(expired["state"]["kind"], "expired");
    assert_eq!(expired["deadline"], prepared["deadline"]);
    let (_, prepared) = f
        .host("POST", "/invocations/v1/prepare", f.prepare("logout"))
        .await;
    let id = prepared["invocation_id"].as_str().unwrap();
    f.host(
        "POST",
        &format!("/invocations/v1/{id}/decision"),
        json!({"decision":"allow_once"}),
    )
    .await;
    f.state
        .auth
        .with_store(|store| store.revoke(&f.token))
        .unwrap();
    assert_eq!(
        f.host("POST", &format!("/invocations/v1/{id}/execute"), json!({}))
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.state
            .node
            .lock()
            .unwrap()
            .store()
            .list_objects()
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn invocation_all_four_social_methods_publish_real_bound_effects_including_media() {
    let f = Fixture::new();
    f.register().await;
    let media = f
        .state
        .node
        .lock()
        .unwrap()
        .put_media_blob("image/png", b"image-content")
        .unwrap();
    for action in ["follow", "unfollow", "share", "reply"] {
        let mut input = f.prepare(action);
        input["method"] = json!(format!("babble.social.{action}"));
        if matches!(action, "follow" | "unfollow") {
            input["payload"].as_object_mut().unwrap().remove("text");
        } else {
            input["payload"]["media"] = json!({"title":"Image", "resources":[media]});
        }
        let (code, prepared) = f.host("POST", "/invocations/v1/prepare", input).await;
        assert_eq!(code, StatusCode::OK, "{prepared}");
        let id = prepared["invocation_id"].as_str().unwrap();
        assert_eq!(
            f.host(
                "POST",
                &format!("/invocations/v1/{id}/decision"),
                json!({"decision":"allow_once"})
            )
            .await
            .0,
            StatusCode::OK
        );
        let (code, result) = f
            .host("POST", &format!("/invocations/v1/{id}/execute"), json!({}))
            .await;
        assert_eq!(code, StatusCode::OK, "{result}");
        assert_eq!(result["result"]["edge"]["target"], f.target);
        if matches!(action, "share" | "reply") {
            assert_eq!(result["result"]["object"]["kind"], "babble.media");
            assert_eq!(
                result["result"]["object"]["payload"]["resources"],
                json!([media])
            );
        }
    }
    let node = f.state.node.lock().unwrap();
    assert_eq!(node.store().list_invocations().unwrap().len(), 4);
    assert_eq!(node.store().list_edges().unwrap().len(), 4);
    assert_eq!(node.store().list_objects().unwrap().len(), 4);
}

#[tokio::test]
async fn invocation_cancel_execute_race_has_at_most_one_effect() {
    let f = Fixture::new();
    f.register().await;
    let (_, prepared) = f
        .host("POST", "/invocations/v1/prepare", f.prepare("race"))
        .await;
    let id = prepared["invocation_id"].as_str().unwrap();
    f.host(
        "POST",
        &format!("/invocations/v1/{id}/decision"),
        json!({"decision":"allow_once"}),
    )
    .await;
    let cancel = format!("/invocations/v1/{id}/cancel");
    let execute = format!("/invocations/v1/{id}/execute");
    let (_, executed) = tokio::join!(
        f.host("POST", &cancel, json!({})),
        f.host("POST", &execute, json!({}))
    );
    let (_, status) = f
        .host("GET", &format!("/invocations/v1/{id}/status"), json!({}))
        .await;
    let effects = usize::from(status["state"]["kind"] == "completed");
    assert!(matches!(
        status["state"]["kind"].as_str(),
        Some("completed" | "cancelled")
    ));
    assert_eq!(
        executed.0,
        if effects == 1 {
            StatusCode::OK
        } else {
            StatusCode::CONFLICT
        }
    );
    let node = f.state.node.lock().unwrap();
    assert_eq!(node.store().list_edges().unwrap().len(), effects);
    assert_eq!(node.store().list_objects().unwrap().len(), 2 + effects);
}

#[tokio::test]
async fn invocation_budget_starts_before_execution_lock_and_rejects_late_ingress() {
    let f = Fixture::new();
    f.register().await;
    let input: PrepareInvocationRequest = serde_json::from_value(f.prepare("late")).unwrap();
    let principal = f
        .state
        .auth
        .with_store(|store| store.authenticate(&f.token))
        .unwrap();
    let mut node = f.state.node.lock().unwrap();
    let ctx = context::current(&f.state, &node, &principal, &f.controller, &input.origin).unwrap();
    let ingress = Timestamp(time::OffsetDateTime::now_utc() - time::Duration::seconds(31));
    let result = crate::execution::INGRESS
        .scope(ingress, async { prepare_locked(&mut node, ctx, input) })
        .await;
    assert!(result.is_err());
    assert!(node.store().list_invocations().unwrap().is_empty());
}
