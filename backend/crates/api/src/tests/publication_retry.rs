use crate::ApiState;
use babble_authoring::ObjectDraft;
use babble_identity::IdentityKind;
use babble_judgment_local::LocalProvider;
use babble_node::LocalNode;
use babble_rpc::{RpcBinding, RpcErrorCode, RpcRequestEnvelope, babble_rpc_catalog};
use babble_types::Canonical;
use babble_types::{Hash, IdentityId};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

#[path = "social_media.rs"]
mod social_media;

#[path = "media_album.rs"]
mod media_album;

struct Root(PathBuf);
impl Root {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babble-retry-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn node(&self) -> LocalNode<LocalProvider> {
        LocalNode::open(&self.0, LocalProvider::default()).unwrap()
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn request(method: &str, payload: Value, key: &str) -> RpcRequestEnvelope {
    RpcRequestEnvelope::new(
        &babble_rpc_catalog().unwrap(),
        "retry-test",
        method,
        RpcBinding::host("retry-test", "babble://retry").unwrap(),
        payload,
    )
    .unwrap()
    .with_idempotency_key(key)
}

fn dispatch(state: &ApiState<LocalProvider>, request: &RpcRequestEnvelope) -> Value {
    let result = dispatch_rpc_request(state, request.clone());
    assert!(result.error.is_none(), "{:?}", result.error);
    result.result.unwrap()
}

// Publication corruption/recovery tests exercise the trusted native invocation
// boundary. Authenticated HTTP and document/lifecycle tests live in invocations/tests.rs.
fn prepare_native_invocation(
    state: &ApiState<LocalProvider>,
    request: &RpcRequestEnvelope,
) -> babble_types::Result<babble_capabilities::invocation::InvocationRecord> {
    use babble_capabilities::invocation::{InvocationContext, InvocationOrigin};
    let mut node = state.node.lock().unwrap();
    node.check_ready()?;
    let actor: IdentityId = serde_json::from_value(request.payload["author_id"].clone()).unwrap();
    let object_id =
        babble_types::ObjectId::new_unchecked(request.binding.object_id.clone().unwrap());
    let key = request.idempotency_key.as_deref().unwrap();
    let existing = node.store().list_invocations()?.into_iter().find(|record| {
        record.intent().context.actor == actor && record.intent().request_key == key
    });
    let context = existing
        .map(|record| record.intent().context.clone())
        .unwrap_or(InvocationContext {
            actor,
            login_id: "trusted-native-recovery-test".into(),
            object_version: node.object(&object_id).unwrap().canonical_hash()?,
            object_id,
            origin: InvocationOrigin::HostAction {
                document_id: "native-recovery-document".into(),
            },
            policy_revision: node.invocation_policy_revision()?,
            context_epoch: node.invocation_epoch(),
        });
    let text_method = matches!(
        request.method.as_str(),
        "babble.social.reply" | "babble.social.share"
    );
    let payload = babble_node::SocialInvocationPayload {
        target_object_id: serde_json::from_value(request.payload["target_object_id"].clone())
            .unwrap(),
        text: if text_method {
            Some(request.payload["text"].as_str().unwrap().into())
        } else {
            None
        },
        media: request
            .payload
            .get("media")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| babble_types::Error::Canonical(e.to_string()))?,
    };
    node.prepare_social_invocation(
        context,
        key,
        request.method.as_str(),
        payload,
        babble_types::Timestamp(time::OffsetDateTime::now_utc() + time::Duration::seconds(30)),
    )
}

fn dispatch_rpc_request(
    state: &ApiState<LocalProvider>,
    request: RpcRequestEnvelope,
) -> babble_rpc::RpcResponseEnvelope {
    if !crate::invocations::social_method(request.method.as_str()) {
        return crate::dispatch_rpc_request(state, request);
    }
    let result = (|| -> babble_types::Result<Value> {
        let record = prepare_native_invocation(state, &request)?;
        let ctx = &record.intent().context;
        let mut node = state.node.lock().unwrap();
        node.decide_social_invocation(ctx, &record.intent().request_key, record.id(), true)?;
        let result =
            node.execute_social_invocation(ctx, &record.intent().request_key, record.id())?;
        serde_json::to_value(crate::InvocationSocialResult {
            object: result.object,
            edge: result.edge,
            receipt: result.receipt,
        })
        .map_err(|e| babble_types::Error::Canonical(e.to_string()))
    })();
    let catalog = babble_rpc_catalog().unwrap();
    match result {
        Ok(result) => babble_rpc::RpcResponseEnvelope::ok(&catalog, &request, result),
        Err(error) => babble_rpc::RpcResponseEnvelope::err(
            &catalog,
            &request,
            babble_rpc::RpcError::new(RpcErrorCode::Conflict, error.to_string()),
        ),
    }
}

fn publication_receipt_id(state: &ApiState<LocalProvider>, request: &RpcRequestEnvelope) -> Hash {
    if crate::invocations::social_method(request.method.as_str()) {
        let record = prepare_native_invocation(state, request).unwrap();
        ("babble.invocation.publication.v1", record.id())
            .canonical_hash()
            .unwrap()
    } else {
        json!({"version":1,"author":request.payload["author_id"],"origin":request.binding.origin,
            "object":request.binding.object_id,"key":request.idempotency_key})
        .canonical_hash()
        .unwrap()
    }
}

fn counts(state: &ApiState<LocalProvider>) -> (usize, usize, usize, usize) {
    let node = state.node.lock().unwrap();
    let store = node.store();
    (
        store.list_objects().unwrap().len(),
        store.list_edges().unwrap().len(),
        store.list_events().unwrap().len(),
        store.list_judgments().unwrap().len(),
    )
}

#[test]
fn publication_retry_all_rpc_publication_variants_survive_restart() {
    let root = Root::new();
    let mut node = root.node();
    let author = node
        .create_identity(IdentityKind::Person, "author")
        .unwrap();
    let source = node.publish_text(&author.id, "source").unwrap();
    let target = node.publish_text(&author.id, "target").unwrap();
    let media = node
        .put_media_blob("image/png", b"test-media-bytes")
        .unwrap();
    let mut social = ObjectDraft::text("social controller").unwrap();
    let capabilities: Vec<babble_object::CapabilityRequest> = ["follow", "unfollow", "reply", "share"].into_iter().map(|name| {
        serde_json::from_value(json!({"id":format!("babble.social.{name}"),"version":1,"scope":{"object_id":target.id}})).unwrap()
    }).collect();
    for capability in &capabilities {
        social = social.with_capability(capability.clone()).unwrap();
    }
    let controller = node.publish_draft(&author.id, social).unwrap();
    let social_binding = RpcBinding::object(
        controller.id.to_string(),
        "native-test",
        "retry-test",
        "babble://retry",
        vec![],
    )
    .unwrap();
    let mut requests = vec![
        request(
            "babble.object.publish_text.v1",
            json!({"author_id":author.id,"text":"retry text"}),
            "text",
        ),
        request(
            "babble.object.publish.v1",
            json!({"author_id":author.id,"draft":ObjectDraft::text("retry draft").unwrap()}),
            "draft",
        ),
        request(
            "babble.object.publish_media.v1",
            json!({"author_id":author.id,"title":"media","resources":[media]}),
            "media",
        ),
        request(
            "babble.object.fork.v1",
            json!({"author_id":author.id,"source_object_id":source.id,"draft":ObjectDraft::text("fork").unwrap()}),
            "fork",
        ),
        request(
            "babble.object.remix.v1",
            json!({"author_id":author.id,"source_object_ids":[source.id,target.id],"draft":ObjectDraft::text("remix").unwrap()}),
            "remix",
        ),
        request(
            "babble.graph.edge.publish.v1",
            json!({"author_id":author.id,"source":source.id,"target":target.id,"relation":"references","origin":"HumanAssertion"}),
            "edge",
        ),
    ];
    for action in ["follow", "unfollow", "reply", "share"] {
        let mut req = request(
            &format!("babble.social.{action}"),
            json!({"author_id":author.id,"target_object_id":target.id,"text":"retry social"}),
            action,
        );
        req.binding = social_binding.clone();
        requests.push(req);
    }
    let state = ApiState::new(node);
    let results: Vec<_> = requests.iter().map(|req| dispatch(&state, req)).collect();
    let expected_counts = counts(&state);
    for (req, expected) in requests.iter().zip(&results) {
        assert_eq!(&dispatch(&state, req), expected);
    }
    assert_eq!(counts(&state), expected_counts);
    drop(state);
    let state = ApiState::new(root.node());
    for (req, expected) in requests.iter().zip(&results) {
        assert_eq!(&dispatch(&state, req), expected);
    }
    assert_eq!(counts(&state), expected_counts);
    assert_eq!(
        fs::read_dir(root.0.join("publication_receipts"))
            .unwrap()
            .count(),
        10
    );
    let mut changed = requests[0].clone();
    changed.payload["text"] = json!("different intent");
    assert_eq!(
        dispatch_rpc_request(&state, changed).error.unwrap().code,
        RpcErrorCode::Conflict
    );
    assert_eq!(counts(&state), expected_counts);

    let mut changed_method = requests[0].clone();
    changed_method.method = requests[1].method.clone();
    changed_method.payload =
        json!({"author_id":author.id,"draft":ObjectDraft::text("retry text").unwrap()});
    let mut changed_draft = requests[1].clone();
    changed_draft.payload["draft"] = json!(ObjectDraft::text("different draft").unwrap());
    let mut changed_target = requests[5].clone();
    changed_target.payload["target"] = json!(controller.id);
    for changed in [changed_method, changed_draft, changed_target] {
        let response = dispatch_rpc_request(&state, changed);
        assert!(response.result.is_none());
        assert_eq!(response.error.unwrap().code, RpcErrorCode::Conflict);
        assert_eq!(counts(&state), expected_counts);
    }
}

#[test]
fn publication_retry_is_scoped_and_serializes_concurrent_requests() {
    let root = Root::new();
    let mut node = root.node();
    let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
    let bob = node.create_identity(IdentityKind::Person, "bob").unwrap();
    let state = ApiState::new(node);
    let req = request(
        "babble.object.publish_text.v1",
        json!({"author_id":alice.id,"text":"same intent"}),
        "shared-key",
    );
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let state = state.clone();
            let req = req.clone();
            std::thread::spawn(move || dispatch(&state, &req))
        })
        .collect();
    let results: Vec<_> = threads
        .into_iter()
        .map(|thread| thread.join().unwrap())
        .collect();
    assert!(results.iter().all(|result| result == &results[0]));
    assert_eq!(counts(&state).0, 1);
    let mut other = req.clone();
    other.payload["author_id"] = json!(bob.id);
    assert_ne!(
        dispatch(&state, &other)["object"]["id"],
        results[0]["object"]["id"]
    );
    assert_eq!(counts(&state).0, 2);
    let mut excessive = req;
    excessive.idempotency_key = Some("x".repeat(257));
    assert_eq!(
        dispatch_rpc_request(&state, excessive).error.unwrap().code,
        RpcErrorCode::InvalidInput
    );
}

#[test]
fn publication_retry_receipt_install_failure_recovers_without_duplicate() {
    let root = Root::new();
    let mut node = root.node();
    let author = node
        .create_identity(IdentityKind::Person, "author")
        .unwrap();
    let context = babble_store::PublicationRequest {
        id: Hash::from_bytes(b"retry-key"),
        fingerprint: Hash::from_bytes(b"intent"),
        author: author.id.clone(),
    };
    let obstruction = root
        .0
        .join("publication_receipts")
        .join(format!("{}.publication-tmp", context.id));
    fs::create_dir(&obstruction).unwrap();
    let error = node
        .with_publication_request(context.clone(), |node| {
            node.publish_text(&author.id, "committed intent")
        })
        .unwrap_err();
    assert!(
        error.to_string().contains("committed; recovery required"),
        "{error}"
    );
    assert!(node.check_ready().is_err());
    fs::remove_dir(obstruction).unwrap();
    drop(node);
    let mut node = root.node();
    let objects = node.store().list_objects().unwrap();
    assert_eq!(objects.len(), 1);
    let recovered = node
        .with_publication_request(context, |node| {
            node.publish_text(&author.id, "committed intent")
        })
        .unwrap();
    assert_eq!(recovered.id, objects[0].id);
    assert_eq!(node.store().list_objects().unwrap().len(), 1);
}

#[test]
fn publication_retry_context_clears_after_failure_and_panic() {
    let root = Root::new();
    let mut node = root.node();
    let author = node
        .create_identity(IdentityKind::Person, "author")
        .unwrap();
    let context = babble_store::PublicationRequest {
        id: Hash::from_bytes(b"retry-key"),
        fingerprint: Hash::from_bytes(b"intent"),
        author: author.id.clone(),
    };
    assert!(
        node.with_publication_request(context.clone(), |node| node.publish_text(
            &IdentityId::from_hash(&Hash::from_bytes(b"absent")),
            "no author"
        ))
        .is_err()
    );
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = node.with_publication_request::<()>(context.clone(), |_| panic!("caller panic"));
    }));
    assert!(panic.is_err());
    let result = node
        .with_publication_request(context, |node| node.publish_text(&author.id, "real intent"))
        .unwrap();
    assert_eq!(node.store().list_objects().unwrap(), vec![result]);
}

fn receipt_for_object(root: &Root, object: &Value) -> (PathBuf, Value) {
    assert!(object.is_string());
    let mut matches = fs::read_dir(root.0.join("publication_receipts"))
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let receipt: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            (path, receipt)
        })
        .filter(|(_, receipt)| &receipt["outcome"]["object"] == object);
    let receipt = matches.next().expect("publication receipt exists");
    assert!(matches.next().is_none(), "one receipt per test publication");
    receipt
}

fn assert_receipt_rejected_after_restart(
    root: &Root,
    state: ApiState<LocalProvider>,
    req: &RpcRequestEnvelope,
    path: &std::path::Path,
    corrupt: &[u8],
    expected_code: RpcErrorCode,
) {
    let expected_counts = counts(&state);
    let receipt_count = fs::read_dir(root.0.join("publication_receipts"))
        .unwrap()
        .count();
    drop(state);
    fs::write(path, corrupt).unwrap();
    if crate::invocations::social_method(req.method.as_str())
        && LocalNode::open(&root.0, LocalProvider::default()).is_err()
    {
        // Store-detectable receipt corruption is rejected during recovery;
        // method-specific corruption must still fail at result hydration below.
        assert_eq!(fs::read(path).unwrap(), corrupt);
        assert_eq!(
            fs::read_dir(root.0.join("publication_receipts"))
                .unwrap()
                .count(),
            receipt_count
        );
        return;
    }
    let state = ApiState::new(root.node());
    assert_eq!(counts(&state), expected_counts);
    let response = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dispatch_rpc_request(&state, req.clone())
    }));
    assert!(
        !state.node.is_poisoned(),
        "corrupt receipt poisoned the shared node mutex"
    );
    let response = response.expect("corrupt receipts must return structured errors, not panic");
    assert!(
        response.result.is_none(),
        "corrupt receipt returned success"
    );
    let error = response.error.expect("corrupt receipt must be rejected");
    assert_eq!(error.code, expected_code, "{error:?}");
    assert!(!error.message.is_empty());
    state.node.lock().unwrap().check_ready().unwrap();
    assert_eq!(counts(&state), expected_counts);
    assert_eq!(
        fs::read_dir(root.0.join("publication_receipts"))
            .unwrap()
            .count(),
        receipt_count
    );
    assert_eq!(
        fs::read(path).unwrap(),
        corrupt,
        "preserve corrupt evidence"
    );
    drop(state);
    assert_eq!(counts(&ApiState::new(root.node())), expected_counts);
}

fn social_request(
    node: &mut LocalNode<LocalProvider>,
    author: &IdentityId,
    target: &babble_types::ObjectId,
    action: &str,
) -> RpcRequestEnvelope {
    let capability: babble_object::CapabilityRequest = serde_json::from_value(json!({
        "id":format!("babble.social.{action}"),"version":1,"scope":{"object_id":target},
    }))
    .unwrap();
    let controller = node
        .publish_draft(
            author,
            ObjectDraft::text("controller")
                .unwrap()
                .with_capability(capability.clone())
                .unwrap(),
        )
        .unwrap();
    let mut req = request(
        &format!("babble.social.{action}"),
        json!({"author_id":author,"target_object_id":target,"text":"original social intent"}),
        action,
    );
    req.binding = RpcBinding::object(
        controller.id.to_string(),
        "retry-surface",
        "retry-test",
        "babble://retry",
        vec![],
    )
    .unwrap();
    req
}

#[test]
fn publication_retry_empty_reply_and_share_receipts_do_not_poison_node() {
    for action in ["reply", "share"] {
        let root = Root::new();
        let mut node = root.node();
        let author = node
            .create_identity(IdentityKind::Person, "author")
            .unwrap();
        let target = node.publish_text(&author.id, "target").unwrap();
        let req = social_request(&mut node, &author.id, &target.id, action);
        let state = ApiState::new(node);
        let result = dispatch(&state, &req);
        let (path, mut receipt) = receipt_for_object(&root, &result["object"]["id"]);
        assert_eq!(receipt["outcome"]["edges"].as_array().unwrap().len(), 1);
        receipt["outcome"]["edges"] = json!([]);
        assert_receipt_rejected_after_restart(
            &root,
            state,
            &req,
            &path,
            &serde_json::to_vec(&receipt).unwrap(),
            RpcErrorCode::Conflict,
        );
    }
}

#[test]
fn publication_retry_rejects_unrelated_same_author_edge() {
    let root = Root::new();
    let mut node = root.node();
    let author = node
        .create_identity(IdentityKind::Person, "author")
        .unwrap();
    let target = node.publish_text(&author.id, "target").unwrap();
    let unrelated_source = node.publish_text(&author.id, "unrelated source").unwrap();
    // Preserve author, relation, and target; only the source is unrelated.
    let unrelated = node
        .publish_edge(
            &author.id,
            unrelated_source.id,
            target.id.clone(),
            babble_graph::Relation::ReplyTo,
            babble_graph::EdgeOrigin::HumanAssertion,
        )
        .unwrap();
    let req = social_request(&mut node, &author.id, &target.id, "reply");
    let state = ApiState::new(node);
    let result = dispatch(&state, &req);
    let (path, mut receipt) = receipt_for_object(&root, &result["object"]["id"]);
    assert_ne!(receipt["outcome"]["edges"][0], json!(unrelated.id));
    receipt["outcome"]["edges"] = json!([unrelated.id]);
    assert_receipt_rejected_after_restart(
        &root,
        state,
        &req,
        &path,
        &serde_json::to_vec(&receipt).unwrap(),
        RpcErrorCode::Conflict,
    );
}

#[test]
fn publication_retry_rejects_missing_or_reordered_remix_edges() {
    for remove_edge in [true, false] {
        let root = Root::new();
        let mut node = root.node();
        let author = node
            .create_identity(IdentityKind::Person, "author")
            .unwrap();
        let source = node.publish_text(&author.id, "source").unwrap();
        let target = node.publish_text(&author.id, "target").unwrap();
        let req = request(
            "babble.object.remix.v1",
            json!({"author_id":author.id,
            "source_object_ids":[source.id,target.id],"draft":ObjectDraft::text("remix").unwrap()}),
            "remix",
        );
        let state = ApiState::new(node);
        let result = dispatch(&state, &req);
        let (path, mut receipt) = receipt_for_object(&root, &result["object"]["id"]);
        let edges = receipt["outcome"]["edges"].as_array_mut().unwrap();
        assert_eq!(edges.len(), 2);
        if remove_edge {
            edges.pop();
        } else {
            edges.reverse();
        }
        assert_receipt_rejected_after_restart(
            &root,
            state,
            &req,
            &path,
            &serde_json::to_vec(&receipt).unwrap(),
            RpcErrorCode::Conflict,
        );
    }
}

#[test]
fn publication_retry_rejects_swapped_same_author_object_and_event() {
    let root = Root::new();
    let mut node = root.node();
    let author = node
        .create_identity(IdentityKind::Person, "author")
        .unwrap();
    let state = ApiState::new(node);
    let req = request(
        "babble.object.publish_text.v1",
        json!({"author_id":author.id,"text":"original"}),
        "original",
    );
    let other = request(
        "babble.object.publish_text.v1",
        json!({"author_id":author.id,"text":"different content"}),
        "other",
    );
    let original = dispatch(&state, &req);
    let replacement = dispatch(&state, &other);
    let (path, mut receipt) = receipt_for_object(&root, &original["object"]["id"]);
    let (_, other_receipt) = receipt_for_object(&root, &replacement["object"]["id"]);
    assert_ne!(original["object"]["id"], replacement["object"]["id"]);
    receipt["outcome"] = other_receipt["outcome"].clone();
    assert_receipt_rejected_after_restart(
        &root,
        state,
        &req,
        &path,
        &serde_json::to_vec(&receipt).unwrap(),
        RpcErrorCode::Conflict,
    );
}

#[test]
fn publication_retry_malformed_receipts_fail_without_writes() {
    for corruption in ["truncated", "unknown_field", "wrong_id", "missing_event"] {
        let root = Root::new();
        let mut node = root.node();
        let author = node
            .create_identity(IdentityKind::Person, "author")
            .unwrap();
        let state = ApiState::new(node);
        let req = request(
            "babble.object.publish_text.v1",
            json!({"author_id":author.id,"text":"original"}),
            "original",
        );
        let result = dispatch(&state, &req);
        let (path, mut receipt) = receipt_for_object(&root, &result["object"]["id"]);
        let (bytes, code) = match corruption {
            "truncated" => (b"{\"request\":".to_vec(), RpcErrorCode::InvalidInput),
            "unknown_field" => {
                receipt["unexpected"] = json!(true);
                (
                    serde_json::to_vec(&receipt).unwrap(),
                    RpcErrorCode::InvalidInput,
                )
            }
            "wrong_id" => {
                receipt["request"]["id"] = json!(Hash::from_bytes(b"wrong receipt id"));
                (
                    serde_json::to_vec(&receipt).unwrap(),
                    RpcErrorCode::Conflict,
                )
            }
            "missing_event" => {
                receipt["outcome"]["event"] = json!(babble_types::EventId::from_hash(
                    &Hash::from_bytes(b"absent event")
                ));
                (
                    serde_json::to_vec(&receipt).unwrap(),
                    RpcErrorCode::Conflict,
                )
            }
            _ => unreachable!(),
        };
        assert_receipt_rejected_after_restart(&root, state, &req, &path, &bytes, code);
    }
}
