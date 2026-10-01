use super::*;

async fn register_host(app: &Router, account: &Account, object: &str) {
    let (status, body) = request(
        app,
        "PUT",
        &format!("/invocations/v1/documents/{DOCUMENT}"),
        Some(&account.token),
        json!({"object_id":object}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

fn prepare(
    object: &str,
    account: &Account,
    target: &str,
    action: &str,
    key: &str,
    blobs: &[Value],
) -> Value {
    json!({"origin":{"kind":"host_action","document_id":DOCUMENT},"object_id":object,
        "method":format!("babble.social.{action}"),"request_key":key,
        "payload":{"author_id":account.id,"target_object_id":target,"text":"",
            "media":{"title":"Album","resources":blobs}}})
}

async fn host_call(
    app: &Router,
    account: Option<&Account>,
    method: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    request_with_headers(
        app,
        method,
        path,
        account.map(|a| a.token.as_str()),
        body,
        &[("x-babble-host-document", DOCUMENT)],
    )
    .await
}

async fn invoke(app: &Router, account: &Account, input: Value) -> Value {
    let (status, prepared) =
        host_call(app, Some(account), "POST", "/invocations/v1/prepare", input).await;
    assert_eq!(status, StatusCode::OK, "{prepared}");
    let id = prepared["invocation_id"].as_str().unwrap();
    let (status, decided) = host_call(
        app,
        Some(account),
        "POST",
        &format!("/invocations/v1/{id}/decision"),
        json!({"decision":"allow_once"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{decided}");
    let (status, executed) = host_call(
        app,
        Some(account),
        "POST",
        &format!("/invocations/v1/{id}/execute"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{executed}");
    assert_eq!(executed["state"]["kind"], "completed");
    assert!(executed["result"]["receipt"]["request"]["fingerprint"].is_string());
    executed["result"].clone()
}

#[tokio::test]
async fn media_album_authenticated_upload_publication_social_delivery_and_restart() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let alice = register(&app, "album-alice").await;
    let bob = register(&app, "album-bob").await;
    let target = publish(&app, &bob, "album target").await;
    let (controller_id, _) = controller(&app, &alice, &target, false).await;
    register_host(&app, &alice, &controller_id).await;
    let mut uploaded = Vec::new();
    for index in 0..13 {
        let bytes = format!("authenticated-album-{index}").into_bytes();
        let mime = ["image/png", "audio/wav", "video/webm"][index % 3];
        let operation = envelope(
            "babble.media.blob.put.v1",
            host(),
            json!({"media_type":mime,"bytes_hex":hex::encode(&bytes)}),
        );
        let (status, result) =
            request(&app, "POST", "/rpc", Some(&alice.token), operation.clone()).await;
        assert_eq!(status, StatusCode::OK, "{result}");
        assert!(result["error"].is_null(), "{result}");
        let (_, retry) = request(&app, "POST", "/rpc", Some(&alice.token), operation).await;
        assert_eq!(result["result"], retry["result"]);
        uploaded.push((result["result"]["blob"].clone(), bytes));
    }
    uploaded.rotate_left(5);
    let blobs: Vec<_> = uploaded.iter().map(|(blob, _)| blob.clone()).collect();
    let direct = envelope(
        "babble.object.publish_media.v1",
        host(),
        json!({"author_id":alice.id,"title":"Album","resources":blobs}),
    );
    let (_, direct_result) =
        request(&app, "POST", "/rpc", Some(&alice.token), direct.clone()).await;
    assert!(direct_result["error"].is_null(), "{direct_result}");
    let mut results = vec![direct_result["result"].clone()];
    let mut operations = Vec::new();
    for action in ["reply", "share"] {
        let operation = prepare(&controller_id, &alice, &target, action, action, &blobs);
        assert_eq!(
            host_call(
                &app,
                None,
                "POST",
                "/invocations/v1/prepare",
                operation.clone()
            )
            .await
            .0,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            host_call(
                &app,
                Some(&bob),
                "POST",
                "/invocations/v1/prepare",
                operation.clone()
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        let result = invoke(&app, &alice, operation.clone()).await;
        assert_eq!(invoke(&app, &alice, operation.clone()).await, result);
        assert_eq!(result["object"]["payload"]["resources"], json!(blobs));
        assert_eq!(result["object"]["payload"]["primary_resource"], blobs[0]);
        operations.push(operation);
        results.push(result);
    }
    drop(app);
    let app = fixture.app();
    let (_, retried) = request(&app, "POST", "/rpc", Some(&alice.token), direct).await;
    assert_eq!(retried["result"], results[0]);
    // A restarted host document cannot execute or retrieve late bridge output.
    // Published signed Objects and media remain publicly readable.
    for operation in operations {
        assert_eq!(
            host_call(
                &app,
                Some(&alice),
                "POST",
                "/invocations/v1/prepare",
                operation
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    for expected in &results {
        let id = expected["object"]["id"].as_str().unwrap();
        let (status, read) =
            request(&app, "GET", &format!("/objects/{id}"), None, Value::Null).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(read["object"], expected["object"]);
        for (blob, bytes) in &uploaded {
            let uri = format!(
                "/objects/{id}/media/{}",
                blob["integrity"].as_str().unwrap()
            );
            let response = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()["content-type"],
                blob["media_type"].as_str().unwrap()
            );
            assert_eq!(
                to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
                bytes
            );
        }
    }
    let node = LocalNode::open(&fixture.root, LocalProvider::default()).unwrap();
    let albums: Vec<_> = node
        .store()
        .list_objects()
        .unwrap()
        .into_iter()
        .filter(|object| object.kind.as_str() == "babble.media")
        .map(|object| object.id)
        .collect();
    assert_eq!(albums.len(), 3);
    assert_eq!(
        node.store()
            .list_edges()
            .unwrap()
            .iter()
            .filter(|edge| albums.contains(&edge.source))
            .count(),
        2
    );
}

#[tokio::test]
async fn social_media_authenticated_invocations_reject_forgery_and_retire_documents() {
    let fixture = Fixture::new();
    let app = fixture.app();
    let alice = register(&app, "media-alice").await;
    let bob = register(&app, "media-bob").await;
    let target = publish(&app, &bob, "attachment target").await;
    let other = publish(&app, &bob, "other target").await;
    let (object, _) = controller(&app, &alice, &target, false).await;
    register_host(&app, &alice, &object).await;
    for (mime, bytes, action) in [
        ("image/png", b"social-media-image".as_slice(), "reply"),
        (
            "text/html",
            b"<script>alert(1)</script>".as_slice(),
            "share",
        ),
    ] {
        let (_, upload) = rpc(
            &app,
            &alice,
            "babble.media.blob.put.v1",
            host(),
            json!({"media_type":mime,"bytes_hex":hex::encode(bytes)}),
        )
        .await;
        assert!(upload["error"].is_null(), "{upload}");
        let blob = upload["result"]["blob"].clone();
        let operation = prepare(&object, &alice, &target, action, action, &[blob.clone()]);
        let mut wrong_target = operation.clone();
        wrong_target["payload"]["target_object_id"] = json!(other);
        assert_eq!(
            host_call(
                &app,
                Some(&alice),
                "POST",
                "/invocations/v1/prepare",
                wrong_target
            )
            .await
            .0,
            StatusCode::CONFLICT
        );
        let mut forged = operation.clone();
        forged["payload"]["author_id"] = json!(bob.id);
        assert_eq!(
            host_call(&app, Some(&bob), "POST", "/invocations/v1/prepare", forged)
                .await
                .0,
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(
                &app,
                "POST",
                "/invocations/v1/prepare",
                Some(&alice.token),
                operation.clone()
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        let published = invoke(&app, &alice, operation).await;
        assert_eq!(published["object"]["surfaces"], json!([]));
        assert_eq!(published["object"]["capabilities"], json!([]));
        let uri = format!(
            "/objects/{}/media/{}",
            published["object"]["id"].as_str().unwrap(),
            blob["integrity"].as_str().unwrap()
        );
        let response = app
            .clone()
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            if mime == "text/html" {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::OK
            }
        );
    }
    assert_eq!(
        host_call(
            &app,
            Some(&alice),
            "DELETE",
            &format!("/invocations/v1/documents/{DOCUMENT}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (status, _) = request(
        &app,
        "PUT",
        &format!("/invocations/v1/documents/{DOCUMENT}"),
        Some(&alice.token),
        json!({"object_id":object}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
}
