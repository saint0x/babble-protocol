use super::*;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use babble_media::MediaBlob;
use babble_object::Object;
use tower::ServiceExt;

fn upload(state: &ApiState<LocalProvider>, index: usize) -> (MediaBlob, Vec<u8>) {
    let bytes = format!("album-resource-{index}\0\r\n").into_bytes();
    let mime = ["image/png", "audio/wav", "video/webm"][index % 3];
    let req = request(
        "babble.media.blob.put.v1",
        json!({"media_type":mime,"bytes_hex":hex::encode(&bytes)}),
        &format!("upload-{index}"),
    );
    let first = dispatch(state, &req);
    assert_eq!(dispatch(state, &req), first);
    (
        serde_json::from_value(first["blob"].clone()).unwrap(),
        bytes,
    )
}

fn album_request(
    node: &mut LocalNode<LocalProvider>,
    author: &IdentityId,
    target: &babble_types::ObjectId,
    action: &str,
    blobs: &[MediaBlob],
) -> RpcRequestEnvelope {
    match action {
        "publish" => request(
            "babble.object.publish_media.v1",
            json!({"author_id":author,"title":"Album","description":"caption","resources":blobs}),
            action,
        ),
        "draft" => request(
            "babble.object.publish.v1",
            json!({"author_id":author,"draft":ObjectDraft::media("Album",Some("caption".into()),blobs.to_vec()).unwrap()}),
            action,
        ),
        _ => {
            let mut req = social_request(node, author, target, action);
            req.payload["text"] = json!("caption");
            req.payload["media"] = json!({"title":"Album","resources":blobs});
            req
        }
    }
}

fn resources_mut(req: &mut RpcRequestEnvelope) -> &mut Value {
    match req.method.as_str() {
        "babble.object.publish_media.v1" => &mut req.payload["resources"],
        "babble.object.publish.v1" => &mut req.payload["draft"]["payload"]["resources"],
        _ => &mut req.payload["media"]["resources"],
    }
}

fn receipts(root: &Root) -> usize {
    fs::read_dir(root.0.join("publication_receipts"))
        .unwrap()
        .count()
}

#[tokio::test]
async fn media_album_upload_publish_reply_share_order_reads_restart_and_idempotency() {
    let root = Root::new();
    let mut node = root.node();
    let author = node
        .create_identity(IdentityKind::Person, "album-author")
        .unwrap();
    let target = node.publish_text(&author.id, "album target").unwrap();
    let state = ApiState::new(node);
    // Upload order deliberately differs from album order, and exceeds the UI limit.
    let mut uploaded: Vec<_> = (0..13).map(|index| upload(&state, index)).collect();
    uploaded.rotate_left(7);
    uploaded.reverse();
    let blobs: Vec<_> = uploaded.iter().map(|(blob, _)| blob.clone()).collect();
    let requests: Vec<_> = ["publish", "draft", "reply", "share"]
        .into_iter()
        .map(|action| {
            album_request(
                &mut state.node.lock().unwrap(),
                &author.id,
                &target.id,
                action,
                &blobs,
            )
        })
        .collect();
    let before = counts(&state);
    let results: Vec<_> = requests.iter().map(|req| dispatch(&state, req)).collect();
    let after = counts(&state);
    assert_eq!(after.0, before.0 + 4);
    assert_eq!(after.1, before.1 + 2);
    assert_eq!(after.2, before.2 + 6);
    assert_eq!(receipts(&root), 4);
    for result in &results {
        let object: Object = serde_json::from_value(result["object"].clone()).unwrap();
        object.verify(&author).unwrap();
        assert_eq!(object.payload["resources"], json!(blobs));
        assert_eq!(object.payload["primary_resource"], json!(blobs[0]));
        assert_eq!(
            object.resources,
            blobs.iter().map(MediaBlob::resource).collect::<Vec<_>>()
        );
        assert_eq!(object.payload["description"], "caption");
        if let Some(edge) = result.get("edge") {
            assert_eq!(edge["source"], json!(object.id));
            assert_eq!(edge["target"], json!(target.id));
        }
    }
    assert_eq!(results[2]["edge"]["relation"], "reply_to");
    assert_eq!(results[3]["edge"]["relation"], "quotes");
    for (req, expected) in requests.iter().zip(&results) {
        assert_eq!(dispatch(&state, req), *expected);
    }
    assert_eq!(counts(&state), after);
    drop(state);
    let state = ApiState::new(root.node());
    let app = crate::router(state.clone());
    for (req, expected) in requests.iter().zip(&results) {
        assert_eq!(dispatch(&state, req), *expected);
        let id = expected["object"]["id"].as_str().unwrap();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/objects/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let read: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert_eq!(read["object"], expected["object"]);
        for (blob, bytes) in &uploaded {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/objects/{id}/media/{}", blob.integrity))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["content-type"], blob.media_type);
            assert_eq!(
                to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
                bytes
            );
        }
        let mut reordered = req.clone();
        resources_mut(&mut reordered)
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        let error = dispatch_rpc_request(&state, reordered).error.unwrap();
        assert_eq!(error.code, RpcErrorCode::Conflict);
        assert!(error.message.contains("key"), "{error:?}");
    }
    let node = state.node.lock().unwrap();
    let replies = node
        .list_replies(&babble_node::RepliesListQuery {
            object_id: target.id.clone(),
            cursor: None,
            limit: 50,
        })
        .unwrap();
    assert_eq!(replies.replies.len(), 1);
    assert_eq!(json!(replies.replies[0].object), results[2]["object"]);
    let quotes = node
        .list_quotes(&babble_node::QuotesListQuery {
            object_id: serde_json::from_value(results[3]["object"]["id"].clone()).unwrap(),
            cursor: None,
            limit: 20,
        })
        .unwrap();
    assert_eq!(quotes.quotes.len(), 1);
    assert_eq!(quotes.quotes[0].object.as_ref().unwrap().id, target.id);
    drop(node);
    assert_eq!(counts(&state), after);
}

#[test]
fn media_album_rejects_inconsistent_metadata_without_publication_or_consuming_retry_key() {
    for action in ["publish", "draft", "reply", "share"] {
        let root = Root::new();
        let mut node = root.node();
        let author = node
            .create_identity(IdentityKind::Person, "author")
            .unwrap();
        let target = node.publish_text(&author.id, "target").unwrap();
        let blobs: Vec<_> = (0..3)
            .map(|i| {
                node.put_media_blob("image/png", format!("album-{i}").as_bytes())
                    .unwrap()
            })
            .collect();
        let req = album_request(&mut node, &author.id, &target.id, action, &blobs);
        let state = ApiState::new(node);
        let before = counts(&state);
        for case in [
            "duplicate",
            "duplicate-mime",
            "size-small",
            "size-large",
            "uri",
            "mime",
            "missing",
        ] {
            let mut bad = req.clone();
            let resources = resources_mut(&mut bad).as_array_mut().unwrap();
            match case {
                "duplicate" | "duplicate-mime" => {
                    resources[2] = resources[0].clone();
                    if case == "duplicate-mime" {
                        resources[2]["media_type"] = json!("video/mp4");
                    }
                }
                "size-small" => resources[2]["size_bytes"] = json!(blobs[2].size_bytes - 1),
                "size-large" => resources[2]["size_bytes"] = json!(blobs[2].size_bytes + 1),
                "uri" => {
                    resources[2]["uri"] = json!(format!("babble://blobs/{}", blobs[1].integrity))
                }
                "mime" => resources[2]["media_type"] = json!("Image/PNG"),
                "missing" => {
                    resources[2] = json!(MediaBlob::from_bytes("image/png", b"absent").unwrap())
                }
                _ => unreachable!(),
            }
            let response = dispatch_rpc_request(&state, bad);
            assert!(
                response.error.is_some(),
                "{action} accepted {case}: {response:?}"
            );
            assert_eq!(counts(&state), before, "{action}/{case}");
            assert_eq!(receipts(&root), 0);
            state.node.lock().unwrap().check_ready().unwrap();
        }
        drop(state);
        let state = ApiState::new(root.node());
        assert_eq!(counts(&state), before);
        let accepted = dispatch(&state, &req);
        assert_eq!(accepted["object"]["payload"]["resources"], json!(blobs));
        assert_eq!(receipts(&root), 1);
    }
}

#[test]
fn media_album_rejects_missing_and_corrupt_bytes_at_every_position_atomically() {
    for action in ["publish", "draft", "reply", "share"] {
        let root = Root::new();
        let mut node = root.node();
        let author = node
            .create_identity(IdentityKind::Person, "author")
            .unwrap();
        let target = node.publish_text(&author.id, "target").unwrap();
        let bytes: Vec<_> = (0..3).map(|i| format!("album-{i}").into_bytes()).collect();
        let blobs: Vec<_> = bytes
            .iter()
            .map(|bytes| node.put_media_blob("image/png", bytes).unwrap())
            .collect();
        let req = album_request(&mut node, &author.id, &target.id, action, &blobs);
        let state = ApiState::new(node);
        let before = counts(&state);
        for (index, blob) in blobs.iter().enumerate() {
            let path = root.0.join("blobs").join(blob.integrity.as_str());
            for corrupt in [false, true] {
                if corrupt {
                    fs::write(&path, b"corrupt").unwrap();
                } else {
                    fs::remove_file(&path).unwrap();
                }
                let response = dispatch_rpc_request(&state, req.clone());
                assert!(
                    response.error.is_some(),
                    "{action} accepted index {index}, corrupt={corrupt}"
                );
                assert_eq!(counts(&state), before);
                assert_eq!(receipts(&root), 0);
                fs::write(&path, &bytes[index]).unwrap();
            }
        }
        drop(state);
        let state = ApiState::new(root.node());
        assert_eq!(counts(&state), before);
        let expected = dispatch(&state, &req);
        assert_eq!(dispatch(&state, &req), expected);
        assert_eq!(counts(&state).0, before.0 + 1);
        assert_eq!(
            counts(&state).1,
            before.1 + usize::from(action == "reply" || action == "share")
        );
    }
}

#[test]
fn media_album_draft_rejects_primary_metadata_and_resource_manifest_disagreement() {
    let root = Root::new();
    let mut node = root.node();
    let author = node
        .create_identity(IdentityKind::Person, "author")
        .unwrap();
    let target = node.publish_text(&author.id, "target").unwrap();
    let blobs: Vec<_> = (0..3)
        .map(|i| {
            node.put_media_blob("image/png", format!("album-{i}").as_bytes())
                .unwrap()
        })
        .collect();
    let req = album_request(&mut node, &author.id, &target.id, "draft", &blobs);
    let state = ApiState::new(node);
    let before = counts(&state);
    for case in [
        "primary-size",
        "primary-mime",
        "missing-reference",
        "conflicting-reference",
    ] {
        let mut bad = req.clone();
        let draft = &mut bad.payload["draft"];
        match case {
            "primary-size" => draft["payload"]["primary_resource"]["size_bytes"] = json!(99),
            "primary-mime" => {
                draft["payload"]["primary_resource"]["media_type"] = json!("video/mp4")
            }
            "missing-reference" => {
                draft["resources"].as_array_mut().unwrap().pop();
            }
            "conflicting-reference" => draft["resources"][2]["media_type"] = json!("audio/wav"),
            _ => unreachable!(),
        }
        assert!(
            dispatch_rpc_request(&state, bad).error.is_some(),
            "accepted {case}"
        );
        assert_eq!(counts(&state), before);
        assert_eq!(receipts(&root), 0);
    }
    // An explicitly selected primary may be a later member; preserve its full descriptor.
    let mut selected = req;
    selected.payload["draft"]["payload"]["primary_resource"] = json!(blobs[2]);
    let result = dispatch(&state, &selected);
    assert_eq!(
        result["object"]["payload"]["primary_resource"],
        json!(blobs[2])
    );
    assert_eq!(result["object"]["payload"]["resources"], json!(blobs));
}

#[test]
fn media_album_commit_recovery_installs_complete_album_edge_and_receipt_once() {
    for action in ["publish", "reply", "share"] {
        let root = Root::new();
        let mut node = root.node();
        let author = node
            .create_identity(IdentityKind::Person, "author")
            .unwrap();
        let target = node.publish_text(&author.id, "target").unwrap();
        let blobs: Vec<_> = (0..3)
            .map(|i| {
                node.put_media_blob("image/png", format!("album-{i}").as_bytes())
                    .unwrap()
            })
            .collect();
        let req = album_request(&mut node, &author.id, &target.id, action, &blobs);
        let state = ApiState::new(node);
        let before = counts(&state);
        let receipt_id = publication_receipt_id(&state, &req);
        let obstruction = root
            .0
            .join("publication_receipts")
            .join(format!("{receipt_id}.publication-tmp"));
        fs::create_dir(&obstruction).unwrap();
        let error = dispatch_rpc_request(&state, req.clone()).error.unwrap();
        assert!(error.message.contains("recovery required"), "{error:?}");
        assert!(state.node.lock().unwrap().check_ready().is_err());
        assert!(dispatch_rpc_request(&state, req.clone()).error.is_some());
        drop(state);
        fs::remove_dir(obstruction).unwrap();
        let state = ApiState::new(root.node());
        let after = counts(&state);
        assert_eq!(after.0, before.0 + 1);
        assert_eq!(after.1, before.1 + usize::from(action != "publish"));
        assert_eq!(after.2, before.2 + 1 + usize::from(action != "publish"));
        let result = dispatch(&state, &req);
        assert_eq!(result["object"]["payload"]["resources"], json!(blobs));
        assert_eq!(
            result["object"]["payload"]["primary_resource"],
            json!(blobs[0])
        );
        assert_eq!(dispatch(&state, &req), result);
        assert_eq!(counts(&state), after);
        assert_eq!(receipts(&root), 1);
    }
}

#[test]
fn media_album_signed_record_cannot_bypass_metadata_or_blob_validation() {
    let root = Root::new();
    let mut node = root.node();
    let keypair = babble_crypto::Keypair::generate();
    let author =
        babble_identity::Identity::create(IdentityKind::Person, "author", &keypair).unwrap();
    node.import_signing_identity(author.clone(), keypair.clone())
        .unwrap();
    let blobs: Vec<_> = (0..3)
        .map(|i| {
            node.put_media_blob("image/png", format!("album-{i}").as_bytes())
                .unwrap()
        })
        .collect();
    let payload = babble_media::MediaObjectPayload::new("Album", None, blobs.clone()).unwrap();
    for case in ["primary", "resources", "corrupt"] {
        let mut value = json!(payload);
        let mut resources = payload.object_resources();
        match case {
            "primary" => value["primary_resource"]["size_bytes"] = json!(999),
            "resources" => {
                resources.pop();
            }
            "corrupt" => fs::write(
                root.0.join("blobs").join(blobs[2].integrity.as_str()),
                b"corrupt",
            )
            .unwrap(),
            _ => unreachable!(),
        }
        let object = Object::create(
            &author,
            babble_object::ObjectKind::new("babble.media"),
            "babble.schema.media.v1",
            value,
        )
        .unwrap()
        .with_resources(resources)
        .unwrap()
        .sign(&author, &keypair)
        .unwrap();
        assert!(
            node.publish_object_record(&author.id, object).is_err(),
            "accepted {case}"
        );
        assert!(node.store().list_objects().unwrap().is_empty());
        assert!(node.store().list_edges().unwrap().is_empty());
    }
}
