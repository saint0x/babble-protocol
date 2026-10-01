use super::*;
use crate::router;
use axum::{Router, body::to_bytes, http::Request};
use babel_authoring::ObjectDraft;
use babel_crypto::Keypair;
use babel_identity::{Identity, IdentityKind};
use babel_judgment_local::LocalProvider;
use babel_node::LocalNode;
use babel_object::{Object, Resource};
use babel_types::Hash;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tower::ServiceExt;

struct Fixture {
    state: ApiState<LocalProvider>,
    author: Identity,
    keypair: Keypair,
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babel-binary-media-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let keypair = Keypair::generate();
        let author = Identity::create(IdentityKind::Person, "media-author", &keypair).unwrap();
        node.import_signing_identity(author.clone(), keypair.clone())
            .unwrap();
        Self {
            state: ApiState::new(node),
            author,
            keypair,
            root,
        }
    }

    fn app(&self) -> Router {
        router(self.state.clone())
    }

    fn resource(&self, mime: &str, bytes: &[u8]) -> Resource {
        self.state
            .node
            .lock()
            .unwrap()
            .put_media_blob(mime, bytes)
            .unwrap()
            .resource()
    }

    fn publish(&self, resources: Vec<Resource>) -> String {
        let draft = ObjectDraft::text("Media attachment")
            .unwrap()
            .with_resources(resources)
            .unwrap();
        self.state
            .node
            .lock()
            .unwrap()
            .publish_draft(&self.author.id, draft)
            .unwrap()
            .id
            .to_string()
    }

    fn publish_record(&self, resources: Vec<Resource>) -> String {
        let object = Object::text(&self.author, "Imported media attachment")
            .unwrap()
            .with_resources(resources)
            .unwrap()
            .sign(&self.author, &self.keypair)
            .unwrap();
        self.state
            .node
            .lock()
            .unwrap()
            .publish_object_record(&self.author.id, object)
            .unwrap()
            .id
            .to_string()
    }

    fn media(&self, mime: &str, bytes: &[u8]) -> (String, Resource) {
        let mut node = self.state.node.lock().unwrap();
        let blob = node.put_media_blob(mime, bytes).unwrap();
        let resource = blob.resource();
        let id = node
            .publish_media_object(&self.author.id, "Media post", None, vec![blob])
            .unwrap()
            .id;
        (
            format!("/objects/{id}/media/{}", resource.integrity),
            resource,
        )
    }

    fn blob_path(&self, resource: &Resource) -> PathBuf {
        self.root.join("blobs").join(resource.integrity.as_str())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

async fn request(
    app: Router,
    method: &str,
    uri: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, HeaderMap, Bytes) {
    let mut request = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    (
        parts.status,
        parts.headers,
        to_bytes(body, 9 * 1024 * 1024).await.unwrap(),
    )
}

#[tokio::test]
async fn binary_media_get_and_head_are_public_and_preserve_raw_bytes_and_headers() {
    let fixture = Fixture::new();
    let bytes = b"\0\xffraw\r\nmedia";
    let (uri, resource) = fixture.media("video/mp4", bytes);
    let (status, headers, body) = request(fixture.app(), "GET", &uri, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, &bytes[..]);
    assert_eq!(headers[header::CONTENT_TYPE], "video/mp4");
    assert_eq!(headers[header::CONTENT_LENGTH], bytes.len().to_string());
    assert_eq!(headers[header::ACCEPT_RANGES], "bytes");
    assert_eq!(headers[header::ETAG], format!("\"{}\"", resource.integrity));
    assert_eq!(headers[header::CACHE_CONTROL], "no-store");
    assert_eq!(headers[header::X_CONTENT_TYPE_OPTIONS], "nosniff");
    assert_eq!(
        headers[header::CONTENT_SECURITY_POLICY],
        "default-src 'none'; sandbox"
    );
    for range in [None, Some("bytes=1-2"), Some("invalid")] {
        let extra: Vec<_> = range.map(|value| ("range", value)).into_iter().collect();
        let (status, head, body) = request(fixture.app(), "HEAD", &uri, &extra).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(head, headers);
        assert!(body.is_empty());
    }
}

#[tokio::test]
async fn binary_media_single_ranges_include_open_suffix_clamped_and_large_numbers() {
    let fixture = Fixture::new();
    let (uri, _) = fixture.media("audio/mpeg", b"0123456789");
    for (range, start, end) in [
        ("bytes=0-0", 0, 0),
        ("bytes=2-5", 2, 5),
        ("bytes=7-", 7, 9),
        ("bytes=-3", 7, 9),
        ("bytes=-99", 0, 9),
        ("bytes=2-99", 2, 9),
        ("bytes=0-99999999999999999999999999999999999", 0, 9),
        ("bytes=-99999999999999999999999999999999999", 0, 9),
        ("BYTES=01-02", 1, 2),
    ] {
        let (status, headers, body) =
            request(fixture.app(), "GET", &uri, &[("range", range)]).await;
        assert_eq!(status, StatusCode::PARTIAL_CONTENT, "{range}");
        assert_eq!(
            headers[header::CONTENT_RANGE],
            format!("bytes {start}-{end}/10")
        );
        assert_eq!(
            headers[header::CONTENT_LENGTH],
            (end - start + 1).to_string()
        );
        assert_eq!(body, &b"0123456789"[start..=end]);
    }
}

#[tokio::test]
async fn binary_media_invalid_unsatisfiable_and_multiple_ranges_are_416() {
    let fixture = Fixture::new();
    let (uri, _) = fixture.media("audio/ogg", b"0123456789");
    for range in [
        "bytes=10-",
        "bytes=5-4",
        "bytes=-0",
        "bytes=-",
        "bytes=",
        "items=0-1",
        "bytes=+1-2",
        "bytes=1- 2",
        "bytes=1-2-3",
        "bytes=0-1,5-6",
        "bytes=99999999999999999999999999999999999-",
        "garbage",
    ] {
        let (status, headers, body) =
            request(fixture.app(), "GET", &uri, &[("range", range)]).await;
        assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE, "{range}");
        assert_eq!(headers[header::CONTENT_RANGE], "bytes */10");
        assert_eq!(headers[header::CONTENT_LENGTH], "0");
        assert!(body.is_empty());
    }
    let (status, _, _) = request(
        fixture.app(),
        "GET",
        &uri,
        &[("range", "bytes=0-1"), ("range", "bytes=2-3")],
    )
    .await;
    assert_eq!(status, StatusCode::RANGE_NOT_SATISFIABLE);
}

#[tokio::test]
async fn binary_media_if_range_requires_exact_strong_validator() {
    let fixture = Fixture::new();
    let (uri, resource) = fixture.media("video/webm", b"0123456789");
    let etag = format!("\"{}\"", resource.integrity);
    for validator in [
        etag.clone(),
        format!("W/{etag}"),
        "\"other\"".into(),
        "Wed, 21 Oct 2015 07:28:00 GMT".into(),
        "*".into(),
    ] {
        let (status, headers, body) = request(
            fixture.app(),
            "GET",
            &uri,
            &[("range", "bytes=2-3"), ("if-range", &validator)],
        )
        .await;
        if validator == etag {
            assert_eq!(status, StatusCode::PARTIAL_CONTENT);
            assert_eq!(body, "23");
        } else {
            assert_eq!(status, StatusCode::OK);
            assert_eq!(body, "0123456789");
            assert!(!headers.contains_key(header::CONTENT_RANGE));
        }
    }
    let (status, _, _) = request(
        fixture.app(),
        "GET",
        &uri,
        &[
            ("range", "bytes=2-3"),
            ("if-range", &etag),
            ("if-range", &etag),
        ],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _, _) = request(
        fixture.app(),
        "GET",
        &uri,
        &[("range", "invalid"), ("if-range", "\"stale\"")],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn binary_media_requires_published_object_and_exact_local_resource_binding() {
    let fixture = Fixture::new();
    let resource = fixture.resource("image/png", b"unpublished");
    let no_resources = fixture.publish(vec![]);
    let unknown = babel_types::ObjectId::from_hash(&Hash::from_bytes(b"missing object"));
    let mut remote = resource.clone();
    remote.uri = "https://example.com/image.png".into();
    let remote_id = fixture.publish(vec![remote]);
    let mut mismatched = resource.clone();
    mismatched.uri = format!("babel://blobs/{}", Hash::from_bytes(b"other"));
    let mismatched_id = fixture.publish_record(vec![mismatched]);
    for id in [no_resources, unknown.to_string(), remote_id, mismatched_id] {
        let uri = format!("/objects/{id}/media/{}", resource.integrity);
        let (status, _, _) = request(fixture.app(), "GET", &uri, &[]).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}");
    }
    let (uri, _) = fixture.media("image/png", b"published");
    let (status, headers, _) = request(
        fixture.app(),
        "GET",
        &format!("{uri}?media_type=text/html"),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
    for uri in [
        "/objects/not-an-id/media/not-a-hash".to_string(),
        format!("/objects/{}/media/invalid", fixture.publish(vec![])),
    ] {
        assert_eq!(
            request(fixture.app(), "GET", &uri, &[]).await.0,
            StatusCode::BAD_REQUEST
        );
    }
}

#[tokio::test]
async fn binary_media_rejects_active_unsupported_and_ambiguous_mime() {
    let fixture = Fixture::new();
    for mime in [
        "image/svg+xml",
        "text/html",
        "application/javascript",
        "application/pdf",
        "application/octet-stream",
        "video/unknown",
        "image/unknown",
    ] {
        let (uri, _) = fixture.media(mime, b"not inline");
        assert_eq!(
            request(fixture.app(), "GET", &uri, &[]).await.0,
            StatusCode::BAD_REQUEST,
            "{mime}"
        );
    }
    let resource = fixture.resource("image/png", b"ambiguous");
    let mut conflicting = resource.clone();
    conflicting.media_type = "video/mp4".into();
    let id = fixture.publish_record(vec![resource.clone(), conflicting]);
    let uri = format!("/objects/{id}/media/{}", resource.integrity);
    assert_eq!(
        request(fixture.app(), "GET", &uri, &[]).await.0,
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn binary_media_supports_the_frontend_inert_mime_allowlist() {
    let fixture = Fixture::new();
    for mime in [
        "image/jpeg",
        "image/png",
        "image/gif",
        "image/webp",
        "image/avif",
        "image/bmp",
        "image/x-icon",
        "image/vnd.microsoft.icon",
        "audio/mpeg",
        "audio/mp4",
        "audio/aac",
        "audio/ogg",
        "audio/wav",
        "audio/x-wav",
        "audio/webm",
        "audio/flac",
        "video/mp4",
        "video/webm",
        "video/ogg",
        "video/quicktime",
    ] {
        let (uri, _) = fixture.media(mime, mime.as_bytes());
        let (status, headers, body) = request(fixture.app(), "GET", &uri, &[]).await;
        assert_eq!(status, StatusCode::OK, "{mime}");
        assert_eq!(headers[header::CONTENT_TYPE], mime);
        assert_eq!(body, mime);
    }
    let mut mixed_case = fixture.resource("image/png", b"case-insensitive type");
    let canonical = mixed_case.clone();
    mixed_case.media_type = "Image/PNG".into();
    let id = fixture.publish_record(vec![mixed_case.clone(), canonical]);
    let uri = format!("/objects/{id}/media/{}", mixed_case.integrity);
    let (status, headers, _) = request(fixture.app(), "GET", &uri, &[]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "image/png");
}

#[tokio::test]
async fn binary_media_missing_tampered_and_oversized_blobs_never_return_bytes() {
    let fixture = Fixture::new();
    let (uri, resource) = fixture.media("video/mp4", b"original");
    let path = fixture.blob_path(&resource);
    fs::remove_file(&path).unwrap();
    assert_eq!(
        request(fixture.app(), "GET", &uri, &[]).await.0,
        StatusCode::NOT_FOUND
    );
    fs::write(&path, b"tampered").unwrap();
    for method in ["GET", "HEAD"] {
        for headers in [vec![], vec![("range", "bytes=0-0")]] {
            let (status, _, body) = request(fixture.app(), method, &uri, &headers).await;
            assert_eq!(status, StatusCode::CONFLICT);
            assert!(!body.windows(8).any(|bytes| bytes == b"tampered"));
        }
    }
    // A tiny Range must not bypass verification of the full stored resource.
    fs::File::create(&path)
        .unwrap()
        .set_len((crate::execution::MAX_BUFFERED_BLOB_BYTES + 1) as u64)
        .unwrap();
    for method in ["GET", "HEAD"] {
        assert_eq!(
            request(fixture.app(), method, &uri, &[("range", "bytes=0-0")])
                .await
                .0,
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }
    let bytes = vec![42; crate::execution::MAX_BUFFERED_BLOB_BYTES];
    let (uri, _) = fixture.media("audio/wav", &bytes);
    let (status, headers, body) =
        request(fixture.app(), "GET", &uri, &[("range", "bytes=-1")]).await;
    assert_eq!(status, StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        headers[header::CONTENT_RANGE],
        "bytes 8388607-8388607/8388608"
    );
    assert_eq!(body, &b"*"[..]);
}

#[test]
fn binary_media_range_parser_exhaustively_matches_small_representations() {
    for length in 0..16usize {
        for start in 0..20usize {
            for end in 0..20usize {
                let mut headers = HeaderMap::new();
                headers.insert(
                    header::RANGE,
                    format!("bytes={start}-{end}").parse().unwrap(),
                );
                let expected = if start < length && start <= end {
                    Ok(Some(start..=end.min(length - 1)))
                } else {
                    Err(())
                };
                assert_eq!(requested_range(&headers, length), expected);
            }
            let mut headers = HeaderMap::new();
            headers.insert(header::RANGE, format!("bytes=-{start}").parse().unwrap());
            let expected = if length > 0 && start > 0 {
                Ok(Some(length.saturating_sub(start)..=length - 1))
            } else {
                Err(())
            };
            assert_eq!(requested_range(&headers, length), expected);
            headers.insert(header::RANGE, format!("bytes={start}-").parse().unwrap());
            let expected = if start < length {
                Ok(Some(start..=length - 1))
            } else {
                Err(())
            };
            assert_eq!(requested_range(&headers, length), expected);
        }
    }
    let mut headers = HeaderMap::new();
    headers.insert(
        header::RANGE,
        axum::http::HeaderValue::from_bytes(b"bytes=\xff-2").unwrap(),
    );
    assert_eq!(requested_range(&headers, 10), Err(()));
}

#[tokio::test]
async fn binary_media_real_http_transport_preserves_head_and_partial_body_lengths() {
    let fixture = Fixture::new();
    let (uri, _) = fixture.media("video/mp4", b"0123456789");
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = fixture.app();
    let (stop, shutdown) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown.await;
            })
            .await
            .unwrap();
    });
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    let url = format!("http://{address}{uri}");
    let response = client.get(&url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.bytes().await.unwrap(), "0123456789");
    let response = client
        .head(&url)
        .header(header::RANGE, "bytes=2-3")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CONTENT_LENGTH], "10");
    assert!(response.bytes().await.unwrap().is_empty());
    let response = client
        .get(&url)
        .header(header::RANGE, "bytes=2-3")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(response.headers()[header::CONTENT_LENGTH], "2");
    assert_eq!(response.bytes().await.unwrap(), "23");
    stop.send(()).unwrap();
    server.await.unwrap();
}
