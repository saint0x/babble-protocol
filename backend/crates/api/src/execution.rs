use crate::error::ApiError;
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::{DefaultBodyLimit, Request, State},
    http::{StatusCode, header},
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Response},
};
use std::{sync::Arc, time::Duration};
use tokio::sync::Semaphore;

// Hex-encoded 8 MiB blobs also need room for the bounded RPC envelope.
pub(crate) const MAX_BODY_BYTES: usize = 16 * 1024 * 1024 + 64 * 1024;
// Envelope headroom must not increase the decoded upload or response limit.
pub(crate) const MAX_BUFFERED_BLOB_BYTES: usize = 8 * 1024 * 1024;
const BODY_TIMEOUT: Duration = Duration::from_secs(10);

tokio::task_local! {
    pub(crate) static INGRESS: babble_types::Timestamp;
}

// The node and its file store are synchronous. Bound admitted work, read request
// bodies asynchronously, then run the entire authorized handler off the reactor.
pub(crate) fn bounded(router: Router) -> Router {
    with_capacity(router, 16)
}

fn with_capacity(router: Router, capacity: usize) -> Router {
    router
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .layer(from_fn_with_state(
            Arc::new(Semaphore::new(capacity)),
            execute,
        ))
}

async fn execute(State(capacity): State<Arc<Semaphore>>, request: Request, next: Next) -> Response {
    let private = request.uri().path().starts_with("/moderation/");
    let mut response = execute_inner(capacity, request, next, private).await;
    if private { response.headers_mut().insert(header::CACHE_CONTROL, "no-store".parse().unwrap()); }
    response
}

async fn execute_inner(capacity: Arc<Semaphore>, request: Request, next: Next, private: bool) -> Response {
    let ingress = babble_types::Timestamp::now();
    let Ok(permit) = capacity.try_acquire_owned() else {
        let mut response =
            ApiError::unavailable("node request capacity exceeded; retry later").into_response();
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, "1".parse().unwrap());
        return response;
    };
    let (parts, body) = request.into_parts();
    let bytes = match tokio::time::timeout(BODY_TIMEOUT, to_bytes(body, if private { 64 * 1024 } else { MAX_BODY_BYTES })).await {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(_)) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
        Err(_) => return StatusCode::REQUEST_TIMEOUT.into_response(),
    };
    let runtime = tokio::runtime::Handle::current();
    let job = tokio::task::spawn_blocking(move || {
        // Keep admission occupied even if the HTTP client disconnects. Dropping
        // the awaiting future does not cancel an already-running node mutation.
        let _permit = permit;
        runtime.block_on(INGRESS.scope(
            ingress,
            next.run(Request::from_parts(parts, Body::from(bytes))),
        ))
    });
    match job.await {
        Ok(response) => response,
        Err(_) => ApiError::internal("node request worker failed").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use std::sync::{Mutex, mpsc};
    use tower::ServiceExt;

    fn waiting_router(capacity: usize) -> (Router, mpsc::Sender<()>, Arc<Semaphore>) {
        let (release, wait) = mpsc::channel();
        let wait = Arc::new(Mutex::new(wait));
        let started = Arc::new(Semaphore::new(0));
        let notify = started.clone();
        let router = Router::new()
            .route(
                "/wait",
                get(move || {
                    let wait = wait.clone();
                    let notify = notify.clone();
                    async move {
                        notify.add_permits(1);
                        wait.lock()
                            .unwrap()
                            .recv_timeout(Duration::from_secs(2))
                            .unwrap();
                        StatusCode::OK
                    }
                }),
            )
            .route("/fast", get(|| async { StatusCode::OK }));
        (with_capacity(router, capacity), release, started)
    }

    fn request(uri: &str) -> Request {
        Request::builder().uri(uri).body(Body::empty()).unwrap()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn blocking_node_work_does_not_stall_the_async_reactor() {
        let (router, release, started) = waiting_router(2);
        let task = tokio::spawn(router.clone().oneshot(request("/wait")));
        started.acquire().await.unwrap().forget();
        let fast =
            tokio::time::timeout(Duration::from_millis(500), router.oneshot(request("/fast")))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(fast.status(), StatusCode::OK);
        release.send(()).unwrap();
        assert_eq!(task.await.unwrap().unwrap().status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn disconnect_does_not_release_capacity_while_mutation_is_running() {
        let (router, release, started) = waiting_router(1);
        let task = tokio::spawn(router.clone().oneshot(request("/wait")));
        started.acquire().await.unwrap().forget();
        task.abort();
        let busy = router.clone().oneshot(request("/fast")).await.unwrap();
        assert_eq!(busy.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(busy.headers()[header::RETRY_AFTER], "1");
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if router
                    .clone()
                    .oneshot(request("/fast"))
                    .await
                    .unwrap()
                    .status()
                    == StatusCode::OK
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn maximum_blob_hex_has_bounded_rpc_envelope_headroom() {
        assert_eq!(MAX_BUFFERED_BLOB_BYTES, 8 * 1024 * 1024);
        let router = with_capacity(
            Router::new().route(
                "/",
                get(|body: axum::body::Bytes| async move { body.len().to_string() }),
            ),
            1,
        );
        let body = format!(
            r#"{{"protocol":"babble.rpc.v1","payload":{{"bytes_hex":"{}","media_type":"text/javascript"}}}}"#,
            "00".repeat(MAX_BUFFERED_BLOB_BYTES)
        );
        let expected = body.len().to_string();
        let response = router
            .oneshot(Request::builder().uri("/").body(Body::from(body)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(to_bytes(response.into_body(), 100).await.unwrap(), expected);
    }

    #[test]
    fn envelope_headroom_does_not_expand_decoded_blob_admission() {
        let exact = "00".repeat(MAX_BUFFERED_BLOB_BYTES);
        assert_eq!(
            crate::routes::decode_hex(&exact).unwrap().len(),
            MAX_BUFFERED_BLOB_BYTES
        );
        assert!(crate::routes::decode_hex(&(exact + "00")).is_err());
    }

    #[tokio::test]
    async fn oversized_body_is_rejected_before_handler_execution() {
        let router = with_capacity(
            Router::new().route(
                "/",
                get(|| async {
                    panic!("oversized request reached handler");
                    #[allow(unreachable_code)]
                    StatusCode::OK
                }),
            ),
            1,
        );
        let request = Request::builder()
            .uri("/")
            .body(Body::from(vec![0; MAX_BODY_BYTES + 1]))
            .unwrap();
        assert_eq!(
            router.oneshot(request).await.unwrap().status(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }
}
