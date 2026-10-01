use crate::{ApiState, config::ServerConfig, router, seed::apply_seed_profile};
use axum::{
    Router,
    http::{Method, header},
};
use babel_node::LocalNode;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::cors::{AllowOrigin, CorsLayer};

pub async fn serve(config: ServerConfig) -> Result<(), ServeError> {
    let bind_addr = config.bind_addr;
    let startup = config.clone();
    let state = tokio::task::spawn_blocking(move || -> Result<_, ServeError> {
        let provider = startup.judgment.start()?;
        let mut node = LocalNode::open_with_algorithms(
            &startup.store_root,
            provider.clone(),
            Box::new(provider.clone()),
            Box::new(provider),
        )?;
        if let Some(profile) = startup.seed_profile {
            let report = apply_seed_profile(&mut node, profile, &startup.public_origin)?;
            eprintln!(
                "Babel API seed profile {:?}: {} Objects inserted",
                report.profile, report.inserted_objects
            );
        }
        let mut state = ApiState::new(node).with_moderators(&crate::config::moderator_ids_from_env()?)?;
        if let Some(gateway) = startup.bundle_gateway {
            state = state.with_bundle_gateway(gateway);
        }
        state
            .auth
            .check_ready()
            .map_err(|_| ServeError::AuthUnavailable)?;
        Ok(state)
    })
    .await
    .map_err(|_| ServeError::StartupFailed)??;
    let gateway = if let Some(gateway) = config.bundle_gateway.as_ref() {
        Some((
            TcpListener::bind(gateway.bind_addr()).await?,
            crate::gateway::router(state.clone()),
        ))
    } else {
        None
    };
    let app = configured_router(config, state);
    let listener = TcpListener::bind(bind_addr).await?;
    eprintln!("Babel API listening on http://{}", listener.local_addr()?);
    if let Some((gateway_listener, gateway_app)) = gateway {
        eprintln!(
            "Babel bundle gateway listening on http://{} (isolated *.localhost mounts)",
            gateway_listener.local_addr()?
        );
        tokio::try_join!(
            async {
                axum::serve(listener, app)
                    .with_graceful_shutdown(shutdown())
                    .await
            },
            async {
                axum::serve(gateway_listener, gateway_app)
                    .with_graceful_shutdown(shutdown())
                    .await
            },
        )?;
    } else {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown())
            .await?;
    }
    Ok(())
}

pub fn configured_router<P>(config: ServerConfig, state: ApiState<P>) -> Router
where
    P: babel_judgment::JudgmentProvider + Send + Sync + 'static,
{
    router(state).layer(cors(config.cors_origins))
}

#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error("node startup worker failed")]
    StartupFailed,
    #[error("account storage unavailable; refusing to serve")]
    AuthUnavailable,
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
    #[error(transparent)]
    Protocol(#[from] babel_types::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

async fn shutdown() {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("install termination signal handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = terminate.recv() => {},
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

fn cors(origins: Vec<axum::http::HeaderValue>) -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::CONTENT_TYPE,
            header::ACCEPT,
            header::AUTHORIZATION,
            axum::http::HeaderName::from_static("idempotency-key"),
            axum::http::HeaderName::from_static(crate::auth::DOCUMENT_HEADER),
            axum::http::HeaderName::from_static("x-babel-host-document"),
        ])
}

#[allow(dead_code)]
fn _assert_socket_addr(_: SocketAddr) {}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, routing::post};
    use tower::ServiceExt;

    #[tokio::test]
    async fn invocation_host_document_preflight_allows_only_configured_parent_origin() {
        let origin = "https://babel.test";
        let app = Router::new()
            .route("/invocations/v1/prepare", post(|| async {}))
            .layer(cors(vec![origin.parse().unwrap()]));
        for caller in [origin, "https://child.invalid"] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::OPTIONS)
                        .uri("/invocations/v1/prepare")
                        .header(header::ORIGIN, caller)
                        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                        .header(
                            header::ACCESS_CONTROL_REQUEST_HEADERS,
                            "authorization,content-type,x-babel-host-document",
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            if caller == origin {
                assert_eq!(
                    response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
                    origin
                );
                assert!(
                    response.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS]
                        .to_str()
                        .unwrap()
                        .contains("x-babel-host-document")
                );
            } else {
                assert!(
                    !response
                        .headers()
                        .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                );
            }
        }
    }

    #[tokio::test]
    async fn consent_preflight_allows_idempotency_header_for_configured_origin() {
        let origin = "https://babel.test";
        let app = Router::new()
            .route("/capabilities/grants", post(|| async {}))
            .layer(cors(vec![origin.parse().unwrap()]));
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/capabilities/grants")
                    .header(header::ORIGIN, origin)
                    .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
                    .header(
                        header::ACCESS_CONTROL_REQUEST_HEADERS,
                        "authorization,content-type,idempotency-key,x-babel-surface-document",
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(response.status().is_success());
        assert_eq!(
            response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
            origin
        );
        let allowed = response.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS]
            .to_str()
            .unwrap();
        for name in [
            "authorization",
            "content-type",
            "idempotency-key",
            "x-babel-surface-document",
        ] {
            assert!(
                allowed.split(',').any(|item| item.trim() == name),
                "{allowed}"
            );
        }
    }

    #[tokio::test]
    async fn document_preflight_allows_trusted_rpc_header_and_registration_put_only_for_configured_origin()
     {
        let app = Router::new()
            .route("/rpc", post(|| async {}))
            .route(
                "/runtime/surfaces/sessions/{id}/document",
                axum::routing::put(|| async {}),
            )
            .layer(cors(vec!["https://babel.test".parse().unwrap()]));
        for origin in ["https://babel.test", "https://untrusted.test"] {
            for (path, method) in [
                ("/rpc", "POST"),
                ("/runtime/surfaces/sessions/test/document", "PUT"),
            ] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(Method::OPTIONS)
                            .uri(path)
                            .header(header::ORIGIN, origin)
                            .header(header::ACCESS_CONTROL_REQUEST_METHOD, method)
                            .header(
                                header::ACCESS_CONTROL_REQUEST_HEADERS,
                                "authorization,content-type,x-babel-surface-document",
                            )
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                if origin == "https://babel.test" {
                    assert_eq!(
                        response.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN],
                        origin
                    );
                    assert!(
                        response.headers()[header::ACCESS_CONTROL_ALLOW_HEADERS]
                            .to_str()
                            .unwrap()
                            .split(',')
                            .any(|name| name.trim() == crate::auth::DOCUMENT_HEADER)
                    );
                    assert!(
                        response.headers()[header::ACCESS_CONTROL_ALLOW_METHODS]
                            .to_str()
                            .unwrap()
                            .split(',')
                            .any(|name| name.trim() == method)
                    );
                } else {
                    assert!(
                        !response
                            .headers()
                            .contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN)
                    );
                }
            }
        }
    }
}
