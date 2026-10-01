use super::*;
use crate::{ApiState, auth, schema::StartSurfaceSessionRequest};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use babel_authoring::ObjectDraft;
use babel_identity::IdentityKind;
use babel_judgment_local::LocalProvider;
use babel_object::{
    Surface, SurfaceTarget,
    bundle::{BundleFile, BundleFileKind, BundleManifest},
};
use babel_runtime::{RuntimeAdmissionStatus, SurfaceLifecycle};
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use tower::ServiceExt;

struct Harness {
    state: ApiState<LocalProvider>,
    root: PathBuf,
    object: ObjectId,
    principal: Principal,
    token: String,
}

#[tokio::test]
async fn moderation_restrict_withdraws_live_gateway_heartbeat_and_restores_only_new_admission() {
    use babel_graph::moderation::*;
    let h = Harness::new();
    let (reporter,reviewer,independent) = {
        let mut node=h.state.node.lock().unwrap();
        let reporter=node.create_identity(IdentityKind::Person,"reporter").unwrap().id;
        let reviewer=node.create_identity(IdentityKind::Person,"reviewer").unwrap().id;
        let independent=node.create_identity(IdentityKind::Person,"independent").unwrap().id;
        node.configure_moderators(&format!("{reviewer},{independent}")).unwrap();
        (reporter,reviewer,independent)
    };
    let session=h.start(None).unwrap();
    let mount=session.plan.verified_mount.as_ref().unwrap();
    assert_eq!(h.get(mount,"/app/index.html").await.status(),StatusCode::OK);
    let case = {
        let mut node=h.state.node.lock().unwrap();
        let case=node.moderation_report(&reporter,ReportRequest{object_id:h.object.clone(),reason:ModerationReason::Malware,details:"Reported malicious execution with clear evidence".into(),idempotency_key:"report".into()}).unwrap();
        node.moderation_decide(&reviewer,case.id.clone(),DecisionRequest{outcome:ModerationOutcome::Restrict,reason:ModerationReason::Malware,explanation:"Verified malicious behavior and restricted execution".into(),policy_version:POLICY.into(),source_signals:vec![],expected_revision:1,idempotency_key:"restrict".into()}).unwrap();
        case
    };
    for path in ["/app/index.html","/app/main.js"] {assert!(h.get(mount,path).await.status().is_client_error());}
    assert!(h.start(None).is_err());
    assert!(auth::heartbeat_surface(&h.state,session.id.clone(),&h.principal).is_err());
    assert_eq!(h.state.node.lock().unwrap().surface_session(&session.id).unwrap().lifecycle,SurfaceLifecycle::Evicted);
    {
        let mut node=h.state.node.lock().unwrap();
        node.moderation_appeal(&IdentityId::new_unchecked(h.principal.identity_id.clone()),case.id.clone(),AppealRequest{details:"Author supplied new evidence contesting the decision".into(),expected_revision:2,idempotency_key:"appeal".into()}).unwrap();
        node.moderation_decide(&independent,case.id,DecisionRequest{outcome:ModerationOutcome::NoAction,reason:ModerationReason::Malware,explanation:"Independent appeal review reversed the restriction".into(),policy_version:POLICY.into(),source_signals:vec![],expected_revision:3,idempotency_key:"reverse".into()}).unwrap();
    }
    assert!(h.start(Some(session.id.clone())).is_err());
    assert!(h.get(mount,"/app/index.html").await.status().is_client_error());
    let fresh=h.start(None).unwrap();
    assert_eq!(h.get(fresh.plan.verified_mount.as_ref().unwrap(),"/app/index.html").await.status(),StatusCode::OK);
}
impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
impl Harness {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("babel-gateway-{}", random_token().unwrap()));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let identity = node
            .create_identity(IdentityKind::Person, "gateway")
            .unwrap();
        let mut files = Vec::new();
        for (path, bytes, media_type, kind) in [
            (
                "app/index.html",
                "<!doctype html><script type=module src=main.js></script>",
                "text/html",
                BundleFileKind::Document,
            ),
            (
                "app/main.js",
                "globalThis.version = 'verified'",
                "text/javascript",
                BundleFileKind::Script,
            ),
            (
                "other.html",
                "<!doctype html><script src=app/main.js></script>",
                "text/html",
                BundleFileKind::Document,
            ),
        ] {
            let integrity = node.store().put_blob(bytes.as_bytes()).unwrap();
            files.push(BundleFile {
                path: path.into(),
                source_uri: format!("babel://blobs/{integrity}"),
                integrity,
                size_bytes: bytes.len() as u64,
                media_type: media_type.into(),
                kind,
            });
        }
        let manifest = BundleManifest {
            version: 1,
            entry_path: "app/index.html".into(),
            files,
        };
        let entry = &manifest.files[0];
        let object = node
            .publish_draft(
                &identity.id,
                ObjectDraft::text("bundle")
                    .unwrap()
                    .with_surface(Surface {
                        role: SurfaceRole::Feed,
                        target: SurfaceTarget::Web,
                        entry: entry.source_uri.clone(),
                        integrity: Some(entry.integrity.clone()),
                        bundle: Some(manifest),
                    })
                    .unwrap(),
            )
            .unwrap()
            .id;
        let state = ApiState::new(node).with_bundle_gateway(
            GatewayConfig::loopback(
                "127.0.0.1:19877".parse().unwrap(),
                &["http://127.0.0.1:4321".into()],
            )
            .unwrap(),
        );
        let token = state
            .auth
            .with_store(|store| store.issue(identity.id.as_str()))
            .unwrap()
            .0;
        let principal = state
            .auth
            .with_store(|store| store.authenticate(&token))
            .unwrap();
        Self {
            state,
            root,
            object,
            principal,
            token,
        }
    }
    fn start(&self, id: Option<SurfaceSessionId>) -> Result<SurfaceSession, ApiError> {
        auth::start_surface(
            &self.state,
            StartSurfaceSessionRequest {
                object_id: self.object.to_string(),
                role: SurfaceRole::Feed,
                session_id: id,
            },
            &self.principal,
        )
    }
    async fn get(&self, mount: &VerifiedSurfaceMount, path: &str) -> axum::response::Response {
        router(self.state.clone())
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header(header::HOST, mount.origin.strip_prefix("http://").unwrap())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
    }
}

#[test]
fn gateway_configuration_rejects_nonloopback_and_ambiguous_parent_origins() {
    for address in [
        "0.0.0.0:8788",
        "192.168.1.1:8788",
        "[::1]:8788",
        "127.0.0.2:8788",
        "127.0.0.1:0",
    ] {
        assert!(
            GatewayConfig::loopback(address.parse().unwrap(), &["https://host.example".into()])
                .is_err()
        );
    }
    for origin in [
        "*",
        "null",
        "https://host.example/",
        "https://host.example/path",
        "https://host.example?x",
        "https://user@host.example",
        "https://HOST.example",
        "https://host.example:abc",
        "https://host.example:443",
        "https://host.example#fragment",
        "https://host.example.",
        "https://host.example 'unsafe-inline'",
    ] {
        assert!(
            GatewayConfig::loopback("127.0.0.1:8788".parse().unwrap(), &[origin.into()]).is_err(),
            "{origin}"
        );
    }
    assert!(GatewayConfig::loopback("127.0.0.1:8788".parse().unwrap(), &[]).is_err());
}

#[tokio::test]
async fn verified_start_serves_exact_snapshot_mime_headers_and_reuses_only_its_own_mount() {
    let h = Harness::new();
    let plan = prepare(
        h.state.gateway.as_ref(),
        &h.state.node.lock().unwrap(),
        &h.object,
        SurfaceRole::Feed,
        Some(&IdentityId::new_unchecked(h.principal.identity_id.clone())),
    )
    .unwrap();
    assert_eq!(plan.admission, RuntimeAdmissionStatus::Ready);
    assert!(plan.verified_mount.is_none());
    let session = h.start(None).unwrap();
    let mount = session.plan.verified_mount.as_ref().unwrap();
    assert_eq!(
        mount,
        h.start(Some(session.id.clone()))
            .unwrap()
            .plan
            .verified_mount
            .as_ref()
            .unwrap()
    );
    let next = h.start(None).unwrap();
    assert_ne!(mount.origin, next.plan.verified_mount.unwrap().origin);
    let hash = &session.plan.surface.bundle.as_ref().unwrap().files[1].integrity;
    fs::write(h.root.join("blobs").join(hash.as_str()), b"corrupted").unwrap();
    let response = h.get(mount, "/app/main.js").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "text/javascript");
    assert_eq!(
        response.headers()[header::X_CONTENT_TYPE_OPTIONS],
        "nosniff"
    );
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
    assert_eq!(
        response.headers()["cross-origin-resource-policy"],
        "same-origin"
    );
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    assert!(
        !response
            .headers()
            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
    );
    let csp = response.headers()[header::CONTENT_SECURITY_POLICY]
        .to_str()
        .unwrap();
    for directive in [
        "script-src 'self'",
        "worker-src 'none'",
        "frame-src 'none'",
        "base-uri 'none'",
        "form-action 'none'",
        "frame-ancestors http://127.0.0.1:4321",
        "sandbox allow-scripts allow-same-origin",
    ] {
        assert!(csp.contains(directive));
    }
    assert!(!csp.contains("unsafe-"));
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap(),
        "globalThis.version = 'verified'"
    );
    assert!(
        h.start(None).is_err(),
        "new sessions must not reuse an old receipt after blob corruption"
    );
    assert!(
        prepare(
            h.state.gateway.as_ref(),
            &h.state.node.lock().unwrap(),
            &h.object,
            SurfaceRole::Feed,
            None
        )
        .is_err()
    );
}

#[tokio::test]
async fn gateway_has_no_proxy_api_redirect_or_document_fallback() {
    let h = Harness::new();
    let session = h.start(None).unwrap();
    let mount = session.plan.verified_mount.unwrap();
    for path in [
        "/",
        "/health",
        "/rpc",
        "/missing.js",
        "/other.html",
        "/app/main.js?media_type=text/html",
        "/app/%6dain.js",
        "/app/../app/main.js",
        "//app/main.js",
    ] {
        let response = h.get(&mount, path).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
        assert!(!response.headers().contains_key(header::LOCATION));
        assert_eq!(
            response.headers()[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff"
        );
    }
    for host in [
        "127.0.0.1:19877".to_owned(),
        mount.origin.strip_prefix("http://").unwrap().to_uppercase(),
        format!("{}.evil", mount.origin.strip_prefix("http://").unwrap()),
    ] {
        let response = router(h.state.clone())
            .oneshot(
                Request::builder()
                    .uri("/app/index.html")
                    .header(header::HOST, host)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
    let host = mount.origin.strip_prefix("http://").unwrap();
    for (method, path, destination, expected) in [
        (
            "POST",
            "/app/index.html",
            "iframe",
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        ("GET", "/app/main.js", "iframe", StatusCode::NOT_FOUND),
        ("GET", "/app/index.html", "document", StatusCode::NOT_FOUND),
    ] {
        let response = router(h.state.clone())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header(header::HOST, host)
                    .header("sec-fetch-dest", destination)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    let duplicate = Request::builder()
        .uri("/app/index.html")
        .header(header::HOST, host)
        .header(header::HOST, host)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        router(h.state.clone())
            .oneshot(duplicate)
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn session_eviction_and_account_revocation_close_delivery_and_reclaim_snapshots() {
    let h = Harness::new();
    let session = h.start(None).unwrap();
    let mount = session.plan.verified_mount.unwrap();
    h.state
        .node
        .lock()
        .unwrap()
        .transition_surface_session(&session.id, SurfaceLifecycle::Evicted, "test")
        .unwrap();
    assert_eq!(
        h.get(&mount, "/app/main.js").await.status(),
        StatusCode::GONE
    );
    let session = h.start(None).unwrap();
    let mount = session.plan.verified_mount.unwrap();
    h.state
        .auth
        .with_store(|store| store.revoke(&h.token))
        .unwrap();
    assert_eq!(
        h.get(&mount, "/app/main.js").await.status(),
        StatusCode::GONE
    );
    let gateway = h.state.gateway.as_ref().unwrap();
    gateway
        .prune(&h.state.auth, &h.state.node.lock().unwrap())
        .unwrap();
    assert!(gateway.mounts.lock().unwrap().is_empty());
    assert_eq!(
        h.get(&mount, "/app/main.js").await.status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn snapshot_and_delivery_capacity_fail_closed_without_leaking_active_sessions() {
    let h = Harness::new();
    for _ in 0..MAX_MOUNTS {
        h.start(None).unwrap();
    }
    assert!(h.start(None).is_err());
    let gateway = h.state.gateway.as_ref().unwrap();
    assert_eq!(gateway.mounts.lock().unwrap().len(), MAX_MOUNTS);
    let mount = gateway
        .mounts
        .lock()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .descriptor
        .clone();
    let mut responses = Vec::new();
    for _ in 0..16 {
        let response = h.get(&mount, "/app/main.js").await;
        assert_eq!(response.status(), StatusCode::OK);
        responses.push(response);
    }
    assert_eq!(
        h.get(&mount, "/app/main.js").await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(responses);
    assert_eq!(h.get(&mount, "/app/main.js").await.status(), StatusCode::OK);
}

async fn post(h: &Harness, path: &str, value: Value, token: Option<&str>) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let response = crate::router(h.state.clone())
        .oneshot(request.body(Body::from(value.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, serde_json::from_slice(&body).unwrap())
}

#[tokio::test]
async fn http_prepare_and_start_preserve_account_ownership_and_disabled_gateway_blocking() {
    let mut h = Harness::new();
    let request = json!({"object_id":h.object,"role":"Feed"});
    let (status, prepared) = post(&h, "/runtime/surfaces/prepare", request.clone(), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(prepared["plan"]["admission"], "ready");
    assert!(prepared["plan"]["verified_mount"].is_null());
    assert_eq!(
        post(&h, "/runtime/surfaces/sessions", request.clone(), None)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, started) = post(
        &h,
        "/runtime/surfaces/sessions",
        request.clone(),
        Some(&h.token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert!(
        started["session"]["plan"]["verified_mount"]["origin"]
            .as_str()
            .unwrap()
            .contains(".localhost:")
    );
    let mut retry = request.clone();
    retry["session_id"] = started["session"]["id"].clone();
    let second_token = h
        .state
        .auth
        .with_store(|store| store.issue(&h.principal.identity_id))
        .unwrap()
        .0;
    assert_eq!(
        post(
            &h,
            "/runtime/surfaces/sessions",
            retry.clone(),
            Some(&second_token)
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        post(&h, "/runtime/surfaces/sessions", retry, Some(&h.token))
            .await
            .1,
        started
    );
    h.state.gateway = None;
    let (status, prepared) = post(&h, "/runtime/surfaces/prepare", request.clone(), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(prepared["plan"]["admission"], "blocked");
    assert_eq!(
        post(&h, "/runtime/surfaces/sessions", request, Some(&h.token))
            .await
            .0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn grant_revocation_prevents_further_resource_delivery_even_for_a_valid_snapshot() {
    let mut h = Harness::new();
    let identity = IdentityId::new_unchecked(h.principal.identity_id.clone());
    let capability = babel_object::CapabilityRequest {
        id: "babel.storage.local".into(),
        version: 1,
        scope: json!({"namespace":"self"}),
    };
    let mut node = h.state.node.lock().unwrap();
    let surface = node.object(&h.object).unwrap().surfaces[0].clone();
    h.object = node
        .publish_draft(
            &identity,
            ObjectDraft::text("consented bundle")
                .unwrap()
                .with_surface(surface)
                .unwrap()
                .with_capability(capability.clone())
                .unwrap(),
        )
        .unwrap()
        .id;
    drop(node);
    assert!(h.start(None).is_err());
    let event = h
        .state
        .node
        .lock()
        .unwrap()
        .grant_capability(
            &identity,
            &h.object,
            capability,
            babel_capabilities::GrantDecision::Approved,
        )
        .unwrap();
    let grant = babel_types::CapabilityGrantId::new_unchecked(
        event.payload["grant"]["id"].as_str().unwrap().to_owned(),
    );
    let mount = h.start(None).unwrap().plan.verified_mount.unwrap();
    assert_eq!(h.get(&mount, "/app/main.js").await.status(), StatusCode::OK);
    h.state
        .node
        .lock()
        .unwrap()
        .revoke_capability(&identity, &h.object, &grant)
        .unwrap();
    assert_eq!(
        h.get(&mount, "/app/main.js").await.status(),
        StatusCode::GONE
    );
    assert!(
        h.start(Some(mount.session_id.clone())).is_err(),
        "a retry must recheck revoked grants"
    );
}

#[tokio::test]
async fn suspended_mount_is_retained_but_cannot_deliver_until_resumed() {
    let h = Harness::new();
    let session = h.start(None).unwrap();
    let mount = session.plan.verified_mount.unwrap();
    {
        let mut node = h.state.node.lock().unwrap();
        node.transition_surface_session(&session.id, SurfaceLifecycle::Warm, "warm")
            .unwrap();
        node.transition_surface_session(&session.id, SurfaceLifecycle::Active, "active")
            .unwrap();
        node.transition_surface_session(&session.id, SurfaceLifecycle::Suspended, "suspend")
            .unwrap();
        h.state
            .gateway
            .as_ref()
            .unwrap()
            .prune(&h.state.auth, &node)
            .unwrap();
    }
    assert_eq!(
        h.state
            .gateway
            .as_ref()
            .unwrap()
            .mounts
            .lock()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        h.get(&mount, "/app/main.js").await.status(),
        StatusCode::GONE
    );
    assert!(h.start(Some(session.id.clone())).is_err());
    h.state
        .node
        .lock()
        .unwrap()
        .transition_surface_session(&session.id, SurfaceLifecycle::Warm, "resume")
        .unwrap();
    assert_eq!(h.get(&mount, "/app/main.js").await.status(), StatusCode::OK);
    assert_eq!(
        h.start(Some(session.id)).unwrap().plan.verified_mount,
        Some(mount)
    );
}
