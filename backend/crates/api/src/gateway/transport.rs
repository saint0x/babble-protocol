use super::{Gateway, Mount};
use crate::{ApiState, routes::lock_node};
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{Next, from_fn_with_state},
    response::{IntoResponse, Response},
};
use babel_judgment::JudgmentProvider;
use babel_object::bundle::BundleFileKind;
use babel_store::VerifiedBundleFile;
use std::sync::Arc;
use tokio::sync::OwnedSemaphorePermit;

pub fn router<P: JudgmentProvider + Send + Sync + 'static>(state: ApiState<P>) -> Router {
    let gateway = state.gateway.clone();
    crate::execution::bounded(Router::new().fallback(deliver::<P>).with_state(state))
        .layer(from_fn_with_state(gateway, security_headers))
}

// The response owns both the immutable bytes and its capacity slot until the
// transport finishes or drops it. No mutable blob path is reopened here.
struct Delivery {
    file: VerifiedBundleFile,
    _permit: OwnedSemaphorePermit,
}
impl AsRef<[u8]> for Delivery {
    fn as_ref(&self) -> &[u8] {
        self.file.bytes()
    }
}

async fn deliver<P: JudgmentProvider + Send + Sync + 'static>(
    State(state): State<ApiState<P>>,
    request: Request,
) -> Response {
    response(&state, &request)
}

async fn security_headers(
    State(gateway): State<Option<Arc<Gateway>>>,
    request: Request,
    next: Next,
) -> Response {
    secure(next.run(request).await, gateway.as_deref())
}

fn response<P: JudgmentProvider>(state: &ApiState<P>, request: &Request) -> Response {
    let Some(gateway) = state.gateway.as_deref() else {
        return denied(StatusCode::NOT_FOUND);
    };
    if request.method() != "GET" && request.method() != "HEAD" {
        return denied(StatusCode::METHOD_NOT_ALLOWED);
    }
    let uri = request.uri();
    if uri.scheme().is_some()
        || uri.authority().is_some()
        || uri.query().is_some()
        || uri.path().contains('%')
        || uri.path().contains('\\')
    {
        return denied(StatusCode::NOT_FOUND);
    }
    let mut hosts = request.headers().get_all(header::HOST).iter();
    let Some(Ok(host)) = hosts.next().map(|value| value.to_str()) else {
        return denied(StatusCode::NOT_FOUND);
    };
    if hosts.next().is_some() || host.len() > 100 {
        return denied(StatusCode::NOT_FOUND);
    }
    let mount: Arc<Mount> = match gateway.mounts.lock() {
        Ok(mounts) => match mounts.get(host) {
            Some(mount) => Arc::clone(mount),
            None => return denied(StatusCode::NOT_FOUND),
        },
        Err(_) => return denied(StatusCode::SERVICE_UNAVAILABLE),
    };
    let Ok(node) = lock_node(state) else {
        return denied(StatusCode::SERVICE_UNAVAILABLE);
    };
    if mount.authorize(&state.auth, &node, false).is_err() {
        return denied(StatusCode::GONE);
    }
    let Some(path) = uri.path().strip_prefix('/') else {
        return denied(StatusCode::NOT_FOUND);
    };
    let Some(file) = mount.bundle.file(path) else {
        return denied(StatusCode::NOT_FOUND);
    };
    let destination = request
        .headers()
        .get("sec-fetch-dest")
        .and_then(|value| value.to_str().ok());
    if file.descriptor().kind == BundleFileKind::Document && path != mount.bundle.entry_path()
        || destination == Some("document")
        || matches!(destination, Some("iframe" | "frame")) && path != mount.bundle.entry_path()
    {
        return denied(StatusCode::NOT_FOUND);
    }
    let Ok(permit) = gateway.deliveries.clone().try_acquire_owned() else {
        return denied(StatusCode::SERVICE_UNAVAILABLE);
    };
    let media_type = file.descriptor().media_type.clone();
    let size = file.descriptor().size_bytes;
    let body = if request.method() == "HEAD" {
        Body::empty()
    } else {
        Body::from(Bytes::from_owner(Delivery {
            file: file.clone(),
            _permit: permit,
        }))
    };
    let mut response = Response::new(body);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        media_type.parse().expect("validated manifest MIME"),
    );
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, size.into());
    response
}

fn denied(status: StatusCode) -> Response {
    (status, "Bundle resource unavailable").into_response()
}

fn secure(mut response: Response, gateway: Option<&Gateway>) -> Response {
    let ancestors = gateway
        .map(|gateway| {
            gateway
                .config
                .ancestors
                .to_str()
                .expect("validated parents")
        })
        .unwrap_or("'none'");
    let csp = format!(
        "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; font-src 'self'; media-src 'self'; connect-src 'self'; base-uri 'none'; object-src 'none'; frame-src 'none'; worker-src 'none'; form-action 'none'; frame-ancestors {ancestors}; sandbox allow-scripts allow-same-origin"
    );
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        csp.parse().expect("validated CSP"),
    );
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    headers.insert(
        "cross-origin-resource-policy",
        "same-origin".parse().unwrap(),
    );
    headers.insert("permissions-policy", "accelerometer=(), autoplay=(), camera=(), display-capture=(), geolocation=(), gyroscope=(), microphone=(), midi=(), payment=(), usb=(), xr-spatial-tracking=()".parse().unwrap());
    response
}
