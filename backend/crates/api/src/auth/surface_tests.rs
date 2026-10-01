use super::*;
use axum::{
    body::{Body, to_bytes},
    extract::Request,
    middleware::Next,
    response::Response,
};
use babel_authoring::ObjectDraft;
use babel_capabilities::GrantDecision;
use babel_judgment_local::LocalProvider;
use babel_node::LocalNode;
use babel_object::{CapabilityRequest, Resource, Surface, SurfaceRole, SurfaceTarget};
use babel_rpc::{RpcBinding, RpcRequestEnvelope, babel_rpc_catalog};
use serde_json::{Value, json};
use tower::ServiceExt;

#[tokio::test]
async fn auth_surface_eviction_admission_does_not_retire_and_terminal_cleanup_retries() {
    let root =
        std::env::temp_dir().join(format!("babel-eviction-order-{}", random_token().unwrap()));
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let identity = node
        .create_identity(IdentityKind::Person, "eviction-order")
        .unwrap();
    let hash = babel_types::Hash::from_bytes(b"surface");
    let object = node
        .publish_draft(
            &identity.id,
            ObjectDraft::text("Surface")
                .unwrap()
                .with_resource(Resource {
                    uri: "surface.js".into(),
                    media_type: "text/javascript".into(),
                    integrity: hash.clone(),
                })
                .unwrap()
                .with_surface(Surface {
                    bundle: None,
                    role: SurfaceRole::Feed,
                    target: SurfaceTarget::Web,
                    entry: "surface.js".into(),
                    integrity: Some(hash),
                })
                .unwrap(),
        )
        .unwrap();
    let state = ApiState::new(node);
    let token = state
        .auth
        .with_store(|store| store.issue(identity.id.as_str()))
        .unwrap()
        .0;
    let principal = state
        .auth
        .with_store(|store| store.authenticate(&token))
        .unwrap();
    let session = surface::start_surface(
        &state,
        crate::schema::StartSurfaceSessionRequest {
            object_id: object.id.to_string(),
            role: SurfaceRole::Feed,
            session_id: None,
        },
        &principal,
    )
    .unwrap();
    surface::EXECUTION
        .scope(
            surface::ExecutionAuthorization {
                principal: principal.clone(),
                surface: Some(surface::SurfaceAuthorization {
                    session: session.id.to_string(),
                    object: None,
                    document: None,
                    access: surface::SurfaceAccess::Evict,
                }),
            },
            async {
                // A handler can fail after authority is checked but before it transitions.
                let _node = lock_node(&state).unwrap();
            },
        )
        .await;
    assert_eq!(
        state
            .auth
            .with_store(|store| store.surface_cleanup_batch(""))
            .unwrap()
            .len(),
        1
    );
    let mut node = state.node.lock().unwrap();
    assert_eq!(
        node.surface_session(&session.id).unwrap().lifecycle,
        session.lifecycle
    );
    node.transition_surface_session(
        &session.id,
        babel_runtime::SurfaceLifecycle::Evicted,
        "close",
    )
    .unwrap();
    // Model a successful runtime transition whose retirement write failed.
    surface::cleanup_locked(&state.auth, &mut node, "").unwrap();
    assert!(
        state
            .auth
            .with_store(|store| store.surface_cleanup_batch(""))
            .unwrap()
            .is_empty()
    );
    surface::require_owner(
        &state.auth,
        &node,
        &principal,
        &surface::SurfaceAuthorization {
            session: session.id.to_string(),
            object: None,
            document: None,
            access: surface::SurfaceAccess::Inspect,
        },
    )
    .unwrap();
    drop(node);
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn auth_surface_cleanup_batches_advance_past_live_rows_and_drain_revoked_origin() {
    let root =
        std::env::temp_dir().join(format!("babel-surface-batches-{}", random_token().unwrap()));
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let auth = Auth::new(&root);
    let (live_token, stale_token) = auth
        .with_store(|store| Ok((store.issue("batch-owner")?.0, store.issue("batch-owner")?.0)))
        .unwrap();
    let live = auth
        .with_store(|store| store.authenticate(&live_token))
        .unwrap();
    let stale = auth
        .with_store(|store| store.authenticate(&stale_token))
        .unwrap();
    let batch = store::CLEANUP_BATCH;
    for i in 0..(batch * 2 + 7) {
        auth.with_store(|store| {
            store.reserve_surface(
                &format!("surf_{i:064x}"),
                if i < batch { &live } else { &stale },
                "object",
            )
        })
        .unwrap();
    }
    auth.with_store(|store| store.revoke(&stale_token)).unwrap();
    let first = auth
        .with_store(|store| store.surface_cleanup_batch(""))
        .unwrap();
    assert_eq!(first.len(), batch);
    assert!(first.iter().all(|(_, abandoned)| !abandoned));
    let mut cursor = String::new();
    let mut passes = 0;
    loop {
        passes += 1;
        match surface::cleanup_locked(&auth, &mut node, &cursor).unwrap() {
            Some(next) => {
                assert!(next > cursor);
                cursor = next;
            }
            None => break,
        }
    }
    assert_eq!(
        passes, 3,
        "full live batches must advance, not stop cleanup"
    );
    assert_eq!(
        auth.with_store(|store| store.origin_surface_batch(&stale.account_session))
            .unwrap()
            .len(),
        0
    );
    for i in (batch * 2 + 7)..(batch * 4 + 7) {
        auth.with_store(|store| store.reserve_surface(&format!("surf_{i:064x}"), &live, "object"))
            .unwrap();
    }
    auth.with_store(|store| store.revoke(&live_token)).unwrap();
    surface::cleanup_origin_locked(&auth, &mut node, &live.account_session).unwrap();
    assert!(
        auth.with_store(|store| store.surface_cleanup_batch(""))
            .unwrap()
            .is_empty()
    );
    let db = rusqlite::Connection::open(root.join("auth/accounts.sqlite3")).unwrap();
    let retired: usize = db
        .query_row(
            "SELECT COUNT(*) FROM surface_owners WHERE retired=1",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(retired, batch * 4 + 7);
    drop(db);
    drop(auth);
    drop(node);
    std::fs::remove_dir_all(root).unwrap();
}

#[derive(Clone)]
struct Gate {
    admitted: Arc<Semaphore>,
    release: Arc<Semaphore>,
}

async fn pause_after_admission(State(gate): State<Gate>, request: Request, next: Next) -> Response {
    gate.admitted.add_permits(1);
    gate.release.acquire().await.unwrap().forget();
    next.run(request).await
}

async fn http(
    app: Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json");
    if path == "/rpc"
        && body["binding"]["object_id"].is_string()
        && body["binding"]["surface_session_id"].is_string()
    {
        builder = builder.header(
            "x-babel-surface-document",
            "550e8400-e29b-41d4-a716-446655440000",
        );
    }
    let response = app
        .oneshot(
            builder
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn rpc(method: &str, binding: RpcBinding, payload: Value) -> Value {
    serde_json::to_value(
        RpcRequestEnvelope::new(
            &babel_rpc_catalog().unwrap(),
            "queued-call",
            method,
            binding,
            payload,
        )
        .unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn auth_surface_queued_http_rpc_rechecks_lifecycle_and_credentials_under_execution_lock() {
    queued_surface_invalidation(false).await;
}

#[tokio::test]
async fn auth_surface_document_registration_rechecks_after_admission_before_commit() {
    queued_surface_invalidation(true).await;
}

async fn queued_surface_invalidation(registration: bool) {
    for invalidation in [
        "suspended",
        "evicted",
        "logout",
        "expired",
        "document",
        "unbound",
        "lease",
    ] {
        if registration && matches!(invalidation, "document" | "unbound") {
            continue;
        }
        let root =
            std::env::temp_dir().join(format!("babel-surface-queue-{}", random_token().unwrap()));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let identity = node
            .create_identity(IdentityKind::Person, "queued-author")
            .unwrap();
        let hash = babel_types::Hash::from_bytes(b"queued surface");
        let capability = CapabilityRequest {
            id: "babel.storage.local".into(),
            version: 1,
            scope: json!({"namespace":"self"}),
        };
        let draft = ObjectDraft::text("queued surface")
            .unwrap()
            .with_resource(Resource {
                uri: "surface.js".into(),
                media_type: "text/javascript".into(),
                integrity: hash.clone(),
            })
            .unwrap()
            .with_surface(Surface {
                bundle: None,
                role: SurfaceRole::Feed,
                target: SurfaceTarget::Web,
                entry: "surface.js".into(),
                integrity: Some(hash),
            })
            .unwrap()
            .with_capability(capability.clone())
            .unwrap();
        let object = node.publish_draft(&identity.id, draft).unwrap();
        let grant = node
            .grant_capability(
                &identity.id,
                &object.id,
                capability,
                GrantDecision::Approved,
            )
            .unwrap();
        let grant_id = grant.payload["grant"]["id"].as_str().unwrap().to_owned();
        let state = ApiState::new(node);
        let token = state
            .auth
            .with_store(|store| store.issue(identity.id.as_str()))
            .unwrap()
            .0;
        let app = crate::router(state.clone());
        let started = http(
            app.clone(),
            "POST",
            "/runtime/surfaces/sessions",
            &token,
            json!({"object_id":object.id,"role":"Feed"}),
        )
        .await;
        assert_eq!(started.0, StatusCode::OK, "{}", started.1);
        let id = started.1["session"]["id"].as_str().unwrap().to_owned();
        if !registration {
            let registered = http(
                app.clone(),
                "PUT",
                &format!("/runtime/surfaces/sessions/{id}/document"),
                &token,
                json!({"document_id":"550e8400-e29b-41d4-a716-446655440000"}),
            )
            .await;
            assert_eq!(registered.0, StatusCode::OK, "{}", registered.1);
        }
        let gate = Gate {
            admitted: Arc::new(Semaphore::new(0)),
            release: Arc::new(Semaphore::new(0)),
        };
        // Real handlers and auth policy, with a test-only scheduling barrier
        // between admission and execution. No production handler is replaced.
        let queued = crate::execution::bounded(
            crate::routes::trusted_router(state.clone())
                .layer(axum::middleware::from_fn_with_state(
                    gate.clone(),
                    pause_after_admission,
                ))
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    policy::authorize::<LocalProvider>,
                )),
        );
        let binding = RpcBinding::object(
            object.id.to_string(),
            &id,
            "queued",
            "https://babel.test",
            vec![grant_id.clone()],
        )
        .unwrap();
        let operation = rpc(
            "babel.storage.local.set.v1",
            binding,
            json!({"key":"queued","value":"must not commit"}),
        );
        let credential = token.clone();
        let path = format!("/runtime/surfaces/sessions/{id}/document");
        let pending = tokio::spawn(async move {
            if registration {
                http(
                    queued,
                    "PUT",
                    &path,
                    &credential,
                    json!({"document_id":"550e8400-e29b-41d4-a716-446655440000"}),
                )
                .await
            } else {
                http(queued, "POST", "/rpc", &credential, operation).await
            }
        });
        gate.admitted.acquire().await.unwrap().forget();
        if invalidation == "logout" {
            assert_eq!(
                http(app.clone(), "DELETE", "/auth/session", &token, Value::Null)
                    .await
                    .0,
                StatusCode::NO_CONTENT
            );
        } else if invalidation == "expired" {
            let db = rusqlite::Connection::open(root.join("auth/accounts.sqlite3")).unwrap();
            db.execute("UPDATE sessions SET expires_at=0", []).unwrap();
        } else if matches!(invalidation, "document" | "unbound" | "lease") {
            // Fault injection after admission proves execution reads durable authority again.
            let db = rusqlite::Connection::open(root.join("auth/accounts.sqlite3")).unwrap();
            match invalidation {
                "document" => db.execute(
                    "UPDATE surface_owners SET document_id='550e8400-e29b-41d4-a716-446655440001'",
                    [],
                ),
                "unbound" => db.execute("UPDATE surface_owners SET document_id=NULL", []),
                _ => db.execute("UPDATE surface_owners SET lease_expires_at=0", []),
            }
            .unwrap();
        } else {
            let result = http(
                app.clone(),
                "POST",
                &format!("/runtime/surfaces/sessions/{id}/lifecycle"),
                &token,
                json!({"lifecycle":invalidation,"reason":"invalidate admitted request"}),
            )
            .await;
            assert_eq!(result.0, StatusCode::OK, "{}", result.1);
        }
        gate.release.add_permits(1);
        let rejected = pending.await.unwrap();
        if registration {
            assert_ne!(rejected.0, StatusCode::OK, "{invalidation}: {}", rejected.1);
            let db = rusqlite::Connection::open(root.join("auth/accounts.sqlite3")).unwrap();
            let document: Option<String> = db
                .query_row(
                    "SELECT document_id FROM surface_owners WHERE session_id=?1",
                    [&id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                document, None,
                "{invalidation}: queued registration committed"
            );
        }
        assert!(
            rejected.0 != StatusCode::OK || !rejected.1["error"].is_null(),
            "{invalidation}: {}",
            rejected.1
        );
        let node = state.node.lock().unwrap();
        let (stored, _) = node
            .local_storage_get(&object.id, &identity.id, "queued", &[grant_id])
            .unwrap();
        assert!(
            stored.is_none(),
            "{invalidation}: queued write committed after invalidation"
        );
        drop(node);
        drop(app);
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn auth_surface_start_admitted_before_logout_cannot_create_orphan() {
    let root = std::env::temp_dir().join(format!(
        "babel-surface-start-queue-{}",
        random_token().unwrap()
    ));
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let identity = node
        .create_identity(IdentityKind::Person, "queued-start")
        .unwrap();
    let hash = babel_types::Hash::from_bytes(b"surface start");
    let object = node
        .publish_draft(
            &identity.id,
            ObjectDraft::text("queued start")
                .unwrap()
                .with_resource(Resource {
                    uri: "surface.js".into(),
                    media_type: "text/javascript".into(),
                    integrity: hash.clone(),
                })
                .unwrap()
                .with_surface(Surface {
                    bundle: None,
                    role: SurfaceRole::Feed,
                    target: SurfaceTarget::Web,
                    entry: "surface.js".into(),
                    integrity: Some(hash),
                })
                .unwrap(),
        )
        .unwrap();
    let state = ApiState::new(node);
    let token = state
        .auth
        .with_store(|store| store.issue(identity.id.as_str()))
        .unwrap()
        .0;
    let app = crate::router(state.clone());
    let gate = Gate {
        admitted: Arc::new(Semaphore::new(0)),
        release: Arc::new(Semaphore::new(0)),
    };
    let queued = crate::execution::bounded(
        crate::routes::trusted_router(state.clone())
            .layer(axum::middleware::from_fn_with_state(
                gate.clone(),
                pause_after_admission,
            ))
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                policy::authorize::<LocalProvider>,
            )),
    );
    let credential = token.clone();
    let pending = tokio::spawn(async move {
        http(
            queued,
            "POST",
            "/runtime/surfaces/sessions",
            &credential,
            json!({"object_id":object.id,"role":"Feed"}),
        )
        .await
    });
    gate.admitted.acquire().await.unwrap().forget();
    assert_eq!(
        http(app.clone(), "DELETE", "/auth/session", &token, Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
    gate.release.add_permits(1);
    assert_eq!(pending.await.unwrap().0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        state
            .node
            .lock()
            .unwrap()
            .surface_runtime_health()
            .session_count,
        0
    );
    drop(app);
    drop(state);
    std::fs::remove_dir_all(root).unwrap();
}
