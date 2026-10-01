use super::*;

#[test]
fn social_media_rpc_contract_accepts_optional_media_and_keeps_text_required() {
    let old = json!({"author_id":"author", "target_object_id":null, "text":"old request"});
    let decoded: crate::SocialTextRequest = serde_json::from_value(old.clone()).unwrap();
    assert!(decoded.media.is_none());
    assert_eq!(serde_json::to_value(decoded).unwrap(), old);
    assert!(
        serde_json::from_value::<crate::SocialTextRequest>(
            json!({"author_id":"author","media":{"title":"media","resources":[]}})
        )
        .is_err()
    );
    assert!(serde_json::from_value::<crate::SocialTextRequest>(json!({"author_id":"author","text":"","media":{"title":"media","resources":[],"capabilities":[]}})).is_err());
}

#[test]
fn social_media_rpc_reply_share_retry_survive_restart_and_reject_changed_intent() {
    let root = Root::new();
    let mut node = root.node();
    let author = node
        .create_identity(IdentityKind::Person, "author")
        .unwrap();
    let target = node.publish_text(&author.id, "target").unwrap();
    let blobs = [
        node.put_media_blob("image/png", b"image").unwrap(),
        node.put_media_blob("audio/wav", b"audio").unwrap(),
        node.put_media_blob("video/webm", b"video").unwrap(),
    ];
    let mut requests = Vec::new();
    for action in ["reply", "share"] {
        for (i, blob) in blobs.iter().enumerate() {
            let mut req = social_request(&mut node, &author.id, &target.id, action);
            req.idempotency_key = Some(format!("{action}-{i}"));
            req.payload["text"] = json!(if i == 0 { "caption" } else { "" });
            req.payload["media"] = json!({"title":"Media", "resources":[blob]});
            requests.push(req);
        }
    }
    let state = ApiState::new(node);
    let before = counts(&state);
    let results: Vec<_> = requests.iter().map(|req| dispatch(&state, req)).collect();
    let after = counts(&state);
    assert_eq!(after.0, before.0 + 6);
    assert_eq!(after.1, before.1 + 6);
    for (i, result) in results.iter().enumerate() {
        assert_eq!(result["object"]["kind"], "babble.media");
        assert_eq!(
            result["object"]["payload"]["resources"],
            requests[i].payload["media"]["resources"]
        );
        assert_eq!(result["edge"]["source"], result["object"]["id"]);
        assert_eq!(result["edge"]["target"], json!(target.id));
        assert_eq!(
            result["edge"]["relation"],
            if i < 3 { "reply_to" } else { "quotes" }
        );
        assert_eq!(dispatch(&state, &requests[i]), *result);
    }
    assert_eq!(counts(&state), after);
    drop(state);
    let state = ApiState::new(root.node());
    for (req, result) in requests.iter().zip(&results) {
        assert_eq!(dispatch(&state, req), *result);
        for field in ["title", "caption", "resource", "remove"] {
            let mut changed = req.clone();
            match field {
                "title" => changed.payload["media"]["title"] = json!("different"),
                "caption" => changed.payload["text"] = json!("different"),
                "resource" => changed.payload["media"]["resources"] = json!([blobs[0], blobs[1]]),
                "remove" => {
                    changed.payload.as_object_mut().unwrap().remove("media");
                    changed.payload["text"] = json!("text instead");
                }
                _ => unreachable!(),
            }
            let error = dispatch_rpc_request(&state, changed).error.unwrap();
            assert_eq!(error.code, RpcErrorCode::Conflict);
            assert!(error.message.contains("key"), "{error:?}");
        }
    }
    assert_eq!(counts(&state), after);
    assert_eq!(
        fs::read_dir(root.0.join("publication_receipts"))
            .unwrap()
            .count(),
        6
    );
}

#[test]
fn social_media_rpc_invalid_media_leaves_key_reusable_and_no_orphan_records() {
    let root = Root::new();
    let mut node = root.node();
    let author = node
        .create_identity(IdentityKind::Person, "author")
        .unwrap();
    let target = node.publish_text(&author.id, "target").unwrap();
    let media = node.put_media_blob("image/png", b"image").unwrap();
    let mut req = social_request(&mut node, &author.id, &target.id, "reply");
    req.payload["text"] = json!("");
    req.payload["media"] = json!({"title":"Image","resources":[media]});
    let state = ApiState::new(node);
    let before = counts(&state);
    for invalid in [
        json!({"title":"","resources":[media]}),
        json!({"title":"Image","resources":[]}),
        json!({"title":"Image","resources":[media,media]}),
    ] {
        let mut bad = req.clone();
        bad.payload["media"] = invalid;
        assert!(dispatch_rpc_request(&state, bad).error.is_some());
        assert_eq!(counts(&state), before);
        assert_eq!(
            fs::read_dir(root.0.join("publication_receipts"))
                .unwrap()
                .count(),
            0
        );
    }
    assert_eq!(dispatch(&state, &req)["object"]["kind"], "babble.media");
    assert_eq!(counts(&state).0, before.0 + 1);
    assert_eq!(counts(&state).1, before.1 + 1);
}

#[test]
fn social_media_rpc_receipt_install_failure_recovers_object_edge_and_retry_together() {
    for action in ["reply", "share"] {
        let root = Root::new();
        let mut node = root.node();
        let author = node
            .create_identity(IdentityKind::Person, "author")
            .unwrap();
        let target = node.publish_text(&author.id, "target").unwrap();
        let media = node.put_media_blob("image/png", b"media").unwrap();
        let mut req = social_request(&mut node, &author.id, &target.id, action);
        req.payload["text"] = json!("");
        req.payload["media"] = json!({"title":"Image","resources":[media]});
        let state = ApiState::new(node);
        let id = publication_receipt_id(&state, &req);
        let obstruction = root
            .0
            .join("publication_receipts")
            .join(format!("{id}.publication-tmp"));
        fs::create_dir(&obstruction).unwrap();
        let error = dispatch_rpc_request(&state, req.clone()).error.unwrap();
        assert!(error.message.contains("recovery required"), "{error:?}");
        assert!(state.node.lock().unwrap().check_ready().is_err());
        drop(state);
        fs::remove_dir(&obstruction).unwrap();
        let state = ApiState::new(root.node());
        let before = counts(&state);
        let result = dispatch(&state, &req);
        assert_eq!(result["object"]["kind"], "babble.media");
        assert_eq!(result["edge"]["source"], result["object"]["id"]);
        assert_eq!(dispatch(&state, &req), result);
        assert_eq!(counts(&state), before);
        assert_eq!(
            fs::read_dir(root.0.join("publication_receipts"))
                .unwrap()
                .count(),
            1
        );
    }
}
