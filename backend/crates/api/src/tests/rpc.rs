use crate as babble_api;
use crate::routes::trusted_router as router;
use crate::{
    AiEmbedResponse, AiGenerateResponse, AiTranscribeResponse, ApiState,
    ApplySurfaceScheduleResponse, CameraCaptureResponse, CapabilityCatalogResponse,
    ClaimEvidenceResponse, CloseRealtimeSessionResponse,
    CreateIdentityResponse, DefineRealtimeRoomResponse, EventBundleResponse, EventImportResponse,
    EventListResponse, GrantCapabilityResponse, GraphTraversalResponse,
    IdentityCurrentResponse, InferRelationshipResponse, JudgeObjectResponse,
    JudgmentDefinitionsResponse, JudgmentProvidersResponse, LensCatalogResponse,
    LocalStorageDeleteResponse, LocalStorageGetResponse, LocalStorageListResponse,
    LocalStorageSetResponse, MicrophoneCaptureResponse, NetworkFetchResponse,
    NotificationsRequestResponse, ObjectJudgmentsResponse, ObjectStorageDeleteResponse,
    ObjectStorageGetResponse, ObjectStorageListResponse, ObjectStorageSetResponse,
    ObservabilitySnapshotResponse, PaymentsCheckoutResponse, PersonalizationSyncDeleteResponse,
    PersonalizationSyncGetResponse, PersonalizationSyncListResponse,
    PersonalizationSyncPutResponse, ProvenancePublicationResponse, PublishEdgeResponse,
    PublishObjectResponse, PublishRealtimeMessageResponse, PublishTextResponse,
    ScheduleSurfaceSessionResponse, StartRealtimeSessionResponse, SurfaceRuntimeHealthResponse,
    SurfaceSessionResponse, SurfaceStateCheckpointResponse, SurfaceStateRestoreResponse,
    dispatch_rpc_request,
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode, header},
};
use babble_authoring::ObjectDraft;
use babble_capabilities::PermissionMode;
use babble_graph::{EdgeOrigin, Relation, TraversalDirection};
use babble_identity::IdentityKind;
use babble_judgment::{DefinitionId, ProviderRole};
use babble_judgment_local::LocalProvider;
use babble_lens::{BuiltInLens, CandidateSource, LensExecution};
use babble_node::LocalNode;
use babble_object::{Resource, Surface, SurfaceRole, SurfaceTarget};
use babble_personalization::{
    EncryptedLocalUserModel, LocalUserModel, PersonalizationSyncKey, PersonalizationSyncRecipient,
};
use babble_realtime::{MembershipPolicy, PersistencePolicy, RealtimeOperation, RealtimePayload};
use babble_rpc::{
    RpcBinding, RpcCatalog, RpcErrorCode, RpcRequestEnvelope, RpcResponseEnvelope,
    babble_rpc_catalog,
};
use babble_runtime::{SurfaceLifecycle, SurfaceRuntimeEventKind};
use babble_state::EventKind;
use babble_types::Hash;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

#[test]
fn rpc_dispatch_publishes_and_fetches_text_objects() {
    let root = unique_root("dispatch-text");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();

    let created = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "request-identity",
            "babble.identity.create.v1",
            RpcBinding::host("test-runtime", "babble://test").unwrap(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("identity-key"),
    );
    assert!(created.error.is_none());
    let identity: CreateIdentityResponse = serde_json::from_value(created.result.unwrap()).unwrap();

    let published = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "request-publish",
            "babble.object.publish_text.v1",
            RpcBinding::host("test-runtime", "babble://test").unwrap(),
            json!({
                "author_id": identity.identity.id,
                "text": "RPC dispatch should share the real local node publication path."
            }),
        )
        .unwrap()
        .with_idempotency_key("publish-key"),
    );
    assert!(published.error.is_none());
    let published: PublishTextResponse = serde_json::from_value(published.result.unwrap()).unwrap();

    let fetched = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "request-get",
            "babble.object.get.v1",
            RpcBinding::host("test-runtime", "babble://test").unwrap(),
            json!({"object_id": published.object.id}),
        )
        .unwrap(),
    );
    assert!(fetched.error.is_none());
    let fetched: PublishTextResponse = serde_json::from_value(fetched.result.unwrap()).unwrap();
    assert_eq!(fetched.object, published.object);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_lists_lens_catalog() {
    let root = unique_root("dispatch-lenses");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();

    let response = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "request-lenses",
            "babble.lenses.list.v1",
            RpcBinding::host("test-runtime", "babble://test").unwrap(),
            json!({}),
        )
        .unwrap(),
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    let catalog: LensCatalogResponse = serde_json::from_value(response.result.unwrap()).unwrap();

    assert_eq!(catalog.lenses.len(), BuiltInLens::all().len());
    let research = catalog
        .lenses
        .iter()
        .find(|lens| lens.lens == BuiltInLens::Research)
        .expect("Research Lens should be advertised");
    assert_eq!(research.id, "babble.lens.research.v1");
    assert_eq!(research.execution, LensExecution::LocalDeterministic);
    assert!(
        research
            .required_sources
            .contains(&CandidateSource::Evidence)
    );
    assert!(
        research
            .required_signals
            .iter()
            .any(|signal| signal == "evidence_quality")
    );
    assert!(research.required_permissions.is_empty());

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn publication_recovery_required_rejects_infallible_api_readers() {
    let root = unique_root("publication-recovery-required");
    let keypair = babble_crypto::Keypair::generate();
    let author =
        babble_identity::Identity::create(IdentityKind::Person, "publisher", &keypair).unwrap();
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    node.import_signing_identity(author.clone(), keypair.clone())
        .unwrap();
    let existing = node
        .publish_text(&author.id, "Existing readable Object")
        .unwrap();
    let pending = babble_object::Object::text(&author, "Recover this committed publication")
        .unwrap()
        .sign(&author, &keypair)
        .unwrap();
    let catalog = babble_rpc_catalog().unwrap();
    let request = RpcRequestEnvelope::new(
        &catalog,
        "read-after-publication-failure",
        "babble.object.get.v1",
        RpcBinding::host("test-runtime", "babble://test").unwrap(),
        json!({"object_id": existing.id}),
    )
    .unwrap();

    // A genuine preparation failure is retry-safe and must not poison readers.
    fs::create_dir(root.join(".publication-prepared")).unwrap();
    let error = node
        .publish_object_record(&author.id, pending.clone())
        .unwrap_err();
    assert!(
        error.to_string().contains("publication not committed"),
        "{error}"
    );
    fs::remove_dir(root.join(".publication-prepared")).unwrap();
    let state = ApiState::new(node);
    assert!(dispatch(&state, request.clone()).error.is_none());

    // Block a real record installation, after the redo commit decision is durable.
    let obstruction = root
        .join("objects")
        .join(format!("{}.publication-tmp", pending.id));
    fs::create_dir(&obstruction).unwrap();
    {
        let mut node = state.node.lock().unwrap();
        let error = node
            .publish_object_record(&author.id, pending.clone())
            .unwrap_err();
        assert!(
            error.to_string().contains("committed; recovery required"),
            "{error}"
        );
        assert!(!error.to_string().contains("publication not committed"));
        assert!(root.join(".publication-committed").is_file());
        assert_eq!(node.object(&existing.id), Some(&existing));
        assert!(node.object(&pending.id).is_none());
    }

    let response = dispatch(&state, request.clone());
    assert!(response.result.is_none());
    let error = response.error.unwrap();
    assert_eq!(error.code, RpcErrorCode::Conflict);
    assert!(error.message.contains("publication recovery required"));

    let app = crate::router(state.clone());
    let response = request_json(
        app.clone(),
        Method::GET,
        &format!("/objects/{}", existing.id),
        Value::Null,
    )
    .await;
    assert_eq!(response.status, StatusCode::CONFLICT);
    assert_eq!(response.body["code"], "conflict");
    assert!(
        response.body["message"]
            .as_str()
            .unwrap()
            .contains("publication recovery required")
    );
    assert!(response.body.get("object").is_none());
    let response = request_json(
        app,
        Method::POST,
        "/rpc",
        serde_json::to_value(&request).unwrap(),
    )
    .await;
    let response: RpcResponseEnvelope = serde_json::from_value(response.body).unwrap();
    assert!(response.result.is_none());
    assert_eq!(response.error.unwrap().code, RpcErrorCode::Conflict);

    fs::remove_dir(obstruction).unwrap();
    let recovered = LocalNode::open(&root, LocalProvider::default()).unwrap();
    assert_eq!(recovered.object(&pending.id), Some(&pending));
    assert_eq!(recovered.object_judgments(&pending.id).unwrap().len(), 4);
    assert!(!root.join(".publication-committed").exists());
    // Recovery through another handle must not re-enable the stale API node.
    assert_eq!(
        dispatch(&state, request.clone()).error.unwrap().code,
        RpcErrorCode::Conflict
    );
    let recovered = ApiState::new(recovered);
    assert!(dispatch(&recovered, request).error.is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_lists_judgment_definitions() {
    let root = unique_root("dispatch-judgment-definitions");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();

    let response = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "request-judgment-definitions",
            "babble.judgment.definitions.list.v1",
            RpcBinding::host("test-runtime", "babble://test").unwrap(),
            json!({}),
        )
        .unwrap(),
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    let definitions: JudgmentDefinitionsResponse =
        serde_json::from_value(response.result.unwrap()).unwrap();

    assert_eq!(definitions.definitions.len(), 7);
    assert!(
        definitions
            .definitions
            .iter()
            .any(|definition| definition.id == DefinitionId::source_agreement_v1())
    );
    let evidence = definitions
        .definitions
        .iter()
        .find(|definition| definition.id == DefinitionId::evidence_quality_v1())
        .expect("evidence quality Judgment definition should be listed");
    assert_eq!(
        evidence.output_schema,
        "babble.judgment.output.bounded_score.v1"
    );
    assert!(evidence.meaning.contains("evidence"));
    assert!(evidence.calibration.contains("higher means"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_lists_judgment_providers() {
    let root = unique_root("dispatch-judgment-providers");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();

    let response = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "request-judgment-providers",
            "babble.judgment.providers.list.v1",
            RpcBinding::host("test-runtime", "babble://test").unwrap(),
            json!({}),
        )
        .unwrap(),
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    let providers: JudgmentProvidersResponse =
        serde_json::from_value(response.result.unwrap()).unwrap();

    assert_eq!(providers.providers.len(), 1);
    let provider = &providers.providers[0];
    assert_eq!(provider.provider.provider, "babble-local");
    assert_eq!(provider.role, ProviderRole::Local);
    assert!(provider.enabled);
    assert!(provider.privacy_policy.include_subject);
    assert!(
        provider
            .supported_definitions
            .contains(&DefinitionId::evidence_quality_v1())
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_lists_capability_catalog() {
    let root = unique_root("dispatch-capability-catalog");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();

    let response = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "request-capabilities",
            "babble.capabilities.list.v1",
            RpcBinding::host("test-runtime", "babble://test").unwrap(),
            json!({}),
        )
        .unwrap(),
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    let catalog: CapabilityCatalogResponse =
        serde_json::from_value(response.result.unwrap()).unwrap();

    assert!(catalog.capabilities.len() >= 20);
    let network = catalog
        .capabilities
        .iter()
        .find(|capability| capability.id.as_str() == "babble.network.fetch")
        .expect("network.fetch capability should be cataloged");
    assert_eq!(network.version, 1);
    assert_eq!(network.permission, PermissionMode::AskOnce);
    assert_eq!(network.quota.calls_per_minute, 60);
    assert_eq!(network.quota.bytes_per_minute, 2 * 1024 * 1024);

    let files = catalog
        .capabilities
        .iter()
        .find(|capability| capability.id.as_str() == "babble.files")
        .expect("denied file access capability should be cataloged");
    assert_eq!(files.permission, PermissionMode::DeniedByDefault);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_traverses_typed_graph_neighborhoods() {
    let root = unique_root("dispatch-graph-traverse");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let binding = RpcBinding::host("test-runtime", "babble://test").unwrap();

    let created = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "graph-traverse-identity",
            "babble.identity.create.v1",
            binding.clone(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("graph-traverse-identity-key"),
    );
    assert!(created.error.is_none(), "{:?}", created.error);
    let identity: CreateIdentityResponse = serde_json::from_value(created.result.unwrap()).unwrap();

    let publish_text = |id: &str, key: &str, text: &str| -> PublishTextResponse {
        let response = dispatch(
            &state,
            RpcRequestEnvelope::new(
                &catalog,
                id,
                "babble.object.publish_text.v1",
                binding.clone(),
                json!({"author_id": identity.identity.id, "text": text}),
            )
            .unwrap()
            .with_idempotency_key(key),
        );
        assert!(response.error.is_none(), "{:?}", response.error);
        serde_json::from_value(response.result.unwrap()).unwrap()
    };

    let claim = publish_text("graph-traverse-claim", "graph-traverse-claim-key", "Claim");
    let evidence = publish_text(
        "graph-traverse-evidence",
        "graph-traverse-evidence-key",
        "Evidence",
    );
    let source = publish_text(
        "graph-traverse-source",
        "graph-traverse-source-key",
        "Source",
    );

    let inferred_edge = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "graph-traverse-inferred-edge",
            "babble.graph.relationship.infer.v1",
            binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "source": evidence.object.id,
                "target": claim.object.id,
                "relation": "supports",
                "min_score": 0.2
            }),
        )
        .unwrap()
        .with_idempotency_key("graph-traverse-inferred-edge-key"),
    );
    assert!(inferred_edge.error.is_none(), "{:?}", inferred_edge.error);
    let inferred_edge: InferRelationshipResponse =
        serde_json::from_value(inferred_edge.result.unwrap()).unwrap();
    assert_eq!(inferred_edge.edge.origin, EdgeOrigin::JudgmentDerived);
    assert_eq!(inferred_edge.edge.relation, Relation::Supports);
    assert_eq!(
        inferred_edge.edge.metadata.get("judgment_id"),
        Some(&json!(inferred_edge.judgment.id.to_string()))
    );

    let evidence_projection = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "graph-evidence-projection",
            "babble.graph.evidence.v1",
            binding.clone(),
            json!({"object_id": claim.object.id}),
        )
        .unwrap(),
    );
    assert!(
        evidence_projection.error.is_none(),
        "{:?}",
        evidence_projection.error
    );
    let evidence_projection: ClaimEvidenceResponse =
        serde_json::from_value(evidence_projection.result.unwrap()).unwrap();
    assert_eq!(evidence_projection.projection.claim, claim.object);
    assert_eq!(evidence_projection.projection.supporting.len(), 1);
    assert_eq!(evidence_projection.projection.summary.judgment_support, 1);
    assert_eq!(
        evidence_projection.projection.supporting[0]
            .relationship_judgment
            .as_ref()
            .map(|judgment| &judgment.id),
        Some(&inferred_edge.judgment.id)
    );

    let evidence_edge = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "graph-traverse-evidence-edge",
            "babble.graph.edge.publish.v1",
            binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "source": evidence.object.id,
                "target": claim.object.id,
                "relation": Relation::EvidenceFor,
                "origin": EdgeOrigin::HumanAssertion
            }),
        )
        .unwrap()
        .with_idempotency_key("graph-traverse-evidence-edge-key"),
    );
    assert!(evidence_edge.error.is_none(), "{:?}", evidence_edge.error);
    let evidence_edge: PublishEdgeResponse =
        serde_json::from_value(evidence_edge.result.unwrap()).unwrap();

    let source_edge = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "graph-traverse-source-edge",
            "babble.graph.edge.publish.v1",
            binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "source": source.object.id,
                "target": evidence.object.id,
                "relation": Relation::Cites,
                "origin": EdgeOrigin::HumanAssertion
            }),
        )
        .unwrap()
        .with_idempotency_key("graph-traverse-source-edge-key"),
    );
    assert!(source_edge.error.is_none(), "{:?}", source_edge.error);
    let source_edge: PublishEdgeResponse =
        serde_json::from_value(source_edge.result.unwrap()).unwrap();

    let traversal = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "graph-traverse",
            "babble.graph.traverse.v1",
            binding,
            json!({
                "object_id": claim.object.id,
                "direction": TraversalDirection::Incoming,
                "relations": [Relation::EvidenceFor, Relation::Cites],
                "max_depth": 2,
                "limit": 8
            }),
        )
        .unwrap(),
    );
    assert!(traversal.error.is_none(), "{:?}", traversal.error);
    let traversal: GraphTraversalResponse =
        serde_json::from_value(traversal.result.unwrap()).unwrap();
    assert_eq!(traversal.traversal.steps.len(), 2);
    assert_eq!(traversal.traversal.steps[0].edge, evidence_edge.edge);
    assert_eq!(traversal.traversal.steps[0].next_object, evidence.object.id);
    assert_eq!(traversal.traversal.steps[1].edge, source_edge.edge);
    assert_eq!(traversal.traversal.steps[1].next_object, source.object.id);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_publishes_general_object_drafts() {
    let root = unique_root("dispatch-object-draft");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let binding = RpcBinding::host("test-runtime", "babble://test").unwrap();

    let created = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "draft-identity",
            "babble.identity.create.v1",
            binding.clone(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("draft-identity-key"),
    );
    assert!(created.error.is_none(), "{:?}", created.error);
    let identity: CreateIdentityResponse = serde_json::from_value(created.result.unwrap()).unwrap();
    let draft = ObjectDraft::new(
        babble_object::ObjectKind::new("babble.canvas"),
        "example.canvas.v1",
        json!({
            "title": "Collaborative canvas",
            "layers": [{"id": "base", "locked": false}]
        }),
    )
    .unwrap()
    .with_capability(
        serde_json::from_value(json!({
            "id": "babble.storage.object",
            "version": 1,
            "scope": {"namespace": "canvas"}
        }))
        .unwrap(),
    )
    .unwrap()
    .with_state(json!({"revision": 1}))
    .unwrap();

    let missing_idempotency = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "draft-missing-idempotency",
            "babble.object.publish.v1",
            binding.clone(),
            json!({"author_id": identity.identity.id, "draft": draft}),
        )
        .unwrap(),
    );
    assert_eq!(
        missing_idempotency.error.unwrap().code,
        RpcErrorCode::InvalidInput
    );

    let published = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "draft-publish",
            "babble.object.publish.v1",
            binding.clone(),
            json!({"author_id": identity.identity.id, "draft": draft}),
        )
        .unwrap()
        .with_idempotency_key("draft-publish-key"),
    );
    assert!(published.error.is_none(), "{:?}", published.error);
    let published: PublishObjectResponse =
        serde_json::from_value(published.result.unwrap()).unwrap();
    assert_eq!(published.object.kind.as_str(), "babble.canvas");
    assert_eq!(published.object.schema, "example.canvas.v1");
    assert_eq!(published.object.capabilities.len(), 1);
    assert_eq!(published.object.state, Some(json!({"revision": 1})));

    let fetched = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "draft-fetch",
            "babble.object.get.v1",
            binding,
            json!({"object_id": published.object.id}),
        )
        .unwrap(),
    );
    assert!(fetched.error.is_none(), "{:?}", fetched.error);
    let fetched: PublishTextResponse = serde_json::from_value(fetched.result.unwrap()).unwrap();
    assert_eq!(fetched.object, published.object);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_lists_bundles_and_imports_protocol_events() {
    let source_root = unique_root("dispatch-events-source");
    let target_root = unique_root("dispatch-events-target");
    let source = test_state(&source_root);
    let target = test_state(&target_root);
    let catalog = babble_rpc_catalog().unwrap();
    let binding = RpcBinding::host("test-runtime", "babble://test").unwrap();

    let created = dispatch(
        &source,
        RpcRequestEnvelope::new(
            &catalog,
            "events-identity",
            "babble.identity.create.v1",
            binding.clone(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("events-identity-key"),
    );
    assert!(created.error.is_none(), "{:?}", created.error);
    let identity: CreateIdentityResponse = serde_json::from_value(created.result.unwrap()).unwrap();

    let published = dispatch(
        &source,
        RpcRequestEnvelope::new(
            &catalog,
            "events-publish",
            "babble.object.publish_text.v1",
            binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "text": "Event sync should replicate signed protocol records."
            }),
        )
        .unwrap()
        .with_idempotency_key("events-publish-key"),
    );
    assert!(published.error.is_none(), "{:?}", published.error);
    let published: PublishTextResponse = serde_json::from_value(published.result.unwrap()).unwrap();

    let first_page = dispatch(
        &source,
        RpcRequestEnvelope::new(
            &catalog,
            "events-list-first",
            "babble.events.list.v1",
            binding.clone(),
            json!({"limit": 1}),
        )
        .unwrap(),
    );
    assert!(first_page.error.is_none(), "{:?}", first_page.error);
    let first_page: EventListResponse = serde_json::from_value(first_page.result.unwrap()).unwrap();
    assert_eq!(first_page.events.len(), 1);
    let cursor = first_page
        .next_after
        .expect("first event page should include a cursor");

    let second_page = dispatch(
        &source,
        RpcRequestEnvelope::new(
            &catalog,
            "events-list-second",
            "babble.events.list.v1",
            binding.clone(),
            json!({"after": cursor, "limit": 50}),
        )
        .unwrap(),
    );
    assert!(second_page.error.is_none(), "{:?}", second_page.error);
    let second_page: EventListResponse =
        serde_json::from_value(second_page.result.unwrap()).unwrap();
    assert!(second_page.events.iter().any(|event| {
        matches!(&event.target, babble_state::EventTarget::Object(object_id) if object_id == &published.object.id)
    }));

    let object_event_id = second_page
        .events
        .iter()
        .find(|event| {
            matches!(&event.target, babble_state::EventTarget::Object(object_id) if object_id == &published.object.id)
        })
        .map(|event| event.id.to_string())
        .expect("published object should have an event");
    let bundled = dispatch(
        &source,
        RpcRequestEnvelope::new(
            &catalog,
            "events-bundle",
            "babble.events.bundle.v1",
            binding.clone(),
            json!({"events": [object_event_id]}),
        )
        .unwrap(),
    );
    assert!(bundled.error.is_none(), "{:?}", bundled.error);
    let bundled: EventBundleResponse = serde_json::from_value(bundled.result.unwrap()).unwrap();
    assert!(bundled.bundle.identities.contains(&identity.identity));
    assert!(bundled.bundle.objects.contains(&published.object));
    assert!(bundled.bundle.events.len() >= 2);

    let missing_idempotency = dispatch(
        &target,
        RpcRequestEnvelope::new(
            &catalog,
            "events-import-missing-key",
            "babble.events.import.v1",
            binding.clone(),
            json!({"bundle": bundled.bundle.clone()}),
        )
        .unwrap(),
    );
    assert_eq!(
        missing_idempotency.error.unwrap().code,
        RpcErrorCode::InvalidInput
    );

    let imported = dispatch(
        &target,
        RpcRequestEnvelope::new(
            &catalog,
            "events-import",
            "babble.events.import.v1",
            binding.clone(),
            json!({"bundle": bundled.bundle}),
        )
        .unwrap()
        .with_idempotency_key("events-import-key"),
    );
    assert!(imported.error.is_none(), "{:?}", imported.error);
    let imported: EventImportResponse = serde_json::from_value(imported.result.unwrap()).unwrap();
    assert_eq!(imported.report.identities, 1);
    assert_eq!(imported.report.objects, 1);
    assert_eq!(imported.report.events, 2);

    let fetched = dispatch(
        &target,
        RpcRequestEnvelope::new(
            &catalog,
            "events-import-fetch-object",
            "babble.object.get.v1",
            binding,
            json!({"object_id": published.object.id}),
        )
        .unwrap(),
    );
    assert!(fetched.error.is_none(), "{:?}", fetched.error);
    let fetched: PublishTextResponse = serde_json::from_value(fetched.result.unwrap()).unwrap();
    assert_eq!(fetched.object, published.object);

    fs::remove_dir_all(source_root).unwrap();
    fs::remove_dir_all(target_root).unwrap();
}

#[test]
fn rpc_dispatch_evaluates_judgment_with_orchestration_trace() {
    let root = unique_root("dispatch-judgment");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let binding = RpcBinding::host("test-runtime", "babble://test").unwrap();

    let created = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "judgment-identity",
            "babble.identity.create.v1",
            binding.clone(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("judgment-identity-key"),
    );
    assert!(created.error.is_none());
    let identity: CreateIdentityResponse = serde_json::from_value(created.result.unwrap()).unwrap();

    let published = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "judgment-publish",
            "babble.object.publish_text.v1",
            binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "text": "The dataset and methodology support a reusable semantic Judgment trace."
            }),
        )
        .unwrap()
        .with_idempotency_key("judgment-publish-key"),
    );
    assert!(published.error.is_none());
    let published: PublishTextResponse = serde_json::from_value(published.result.unwrap()).unwrap();

    let judged = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "judgment-evaluate",
            "babble.judgment.object.evaluate.v1",
            binding,
            json!({
                "object_id": published.object.id,
                "definition": DefinitionId::evidence_quality_v1(),
                "parameters": {}
            }),
        )
        .unwrap(),
    );
    assert!(judged.error.is_none());
    let judged: JudgeObjectResponse = serde_json::from_value(judged.result.unwrap()).unwrap();
    let orchestration = judged
        .orchestration
        .expect("RPC Judgment evaluation should return provider trace");

    assert_eq!(orchestration.judgment, judged.judgment);
    assert_eq!(orchestration.decisions.len(), 1);
    assert_eq!(orchestration.decisions[0].provider.provider, "babble-local");
    assert!(orchestration.decisions[0].accepted);
    assert!(orchestration.decisions[0].privacy.include_subject);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_handles_capability_bound_ai_judgment() {
    let root = unique_root("dispatch-ai-judge");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let host_binding = RpcBinding::host("test-runtime", "babble://test").unwrap();

    let created = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "ai-judge-identity",
            "babble.identity.create.v1",
            host_binding.clone(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("ai-judge-identity-key"),
    );
    assert!(created.error.is_none());
    let identity: CreateIdentityResponse = serde_json::from_value(created.result.unwrap()).unwrap();

    let target = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "ai-judge-target",
            "babble.object.publish_text.v1",
            host_binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "text": "The protocol evidence explains the reproducible methodology."
            }),
        )
        .unwrap()
        .with_idempotency_key("ai-judge-target-key"),
    );
    assert!(target.error.is_none());
    let target: PublishTextResponse = serde_json::from_value(target.result.unwrap()).unwrap();

    let definition = DefinitionId::evidence_quality_v1();
    let capability = json!({
        "id": "babble.ai.judge",
        "version": 1,
        "scope": {
            "definition": definition,
            "object_id": target.object.id
        }
    });
    let draft = ObjectDraft::text("AI Judgment capability caller")
        .unwrap()
        .with_capability(serde_json::from_value(capability.clone()).unwrap())
        .unwrap();
    let source = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "ai-judge-source",
            "babble.object.publish.v1",
            host_binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "draft": draft
            }),
        )
        .unwrap()
        .with_idempotency_key("ai-judge-source-key"),
    );
    assert!(source.error.is_none());
    let source: PublishObjectResponse = serde_json::from_value(source.result.unwrap()).unwrap();

    let object_binding = RpcBinding::object(
        source.object.id.to_string(),
        "surface",
        "test-runtime",
        "babble://test",
        vec![],
    )
    .unwrap();
    let missing_grant = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "ai-judge-missing-grant",
            "babble.ai.judge.v1",
            object_binding.clone(),
            json!({
                "object_id": target.object.id,
                "definition": definition,
                "parameters": {}
            }),
        )
        .unwrap(),
    );
    let error = missing_grant.error.unwrap();
    assert_eq!(error.code, RpcErrorCode::CapabilityDenied);
    assert!(error.message.contains("missing capability grant binding"));

    let grant_id = grant_rpc_capability(
        &state,
        &catalog,
        host_binding.clone(),
        identity.identity.id.as_str(),
        source.object.id.as_str(),
        capability,
        "ai-judge-grant-key",
    );

    let wrong_definition = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "ai-judge-wrong-definition",
            "babble.ai.judge.v1",
            RpcBinding::object(
                source.object.id.to_string(),
                "surface",
                "test-runtime",
                "babble://test",
                vec![grant_id.clone()],
            )
            .unwrap(),
            json!({
                "object_id": target.object.id,
                "definition": DefinitionId::spam_v1(),
                "parameters": {}
            }),
        )
        .unwrap(),
    );
    let error = wrong_definition.error.unwrap();
    assert_eq!(error.code, RpcErrorCode::CapabilityDenied);
    assert!(
        error
            .message
            .contains("no active babble.ai.judge grant permits")
    );

    let judged = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "ai-judge-ok",
            "babble.ai.judge.v1",
            RpcBinding::object(
                source.object.id.to_string(),
                "surface",
                "test-runtime",
                "babble://test",
                vec![grant_id],
            )
            .unwrap(),
            json!({
                "object_id": target.object.id,
                "definition": definition,
                "parameters": {}
            }),
        )
        .unwrap(),
    );
    assert!(judged.error.is_none());
    let judged: JudgeObjectResponse = serde_json::from_value(judged.result.unwrap()).unwrap();
    let receipt = judged
        .receipt
        .expect("ai.judge must return a capability receipt");
    assert_eq!(receipt.capability.as_str(), "babble.ai.judge");
    assert_eq!(
        receipt.scope["definition"],
        json!(DefinitionId::evidence_quality_v1())
    );
    assert_eq!(
        judged.judgment.definition,
        DefinitionId::evidence_quality_v1()
    );
    assert!(judged.orchestration.is_some());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_handles_capability_bound_ai_host_actions() {
    let root = unique_root("dispatch-ai-host-actions");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let host_binding = RpcBinding::host("test-runtime", "babble://test").unwrap();

    let identity = create_rpc_identity(&state, &catalog, host_binding.clone(), "ai-host-author");
    let generate_capability = json!({
        "id": "babble.ai.generate",
        "version": 1,
        "scope": {
            "tasks": ["text"],
            "output_modalities": ["text", "json"],
            "models": ["local/text-v1"],
            "max_input_bytes": 4096,
            "max_output_tokens": 2048
        }
    });
    let embed_capability = json!({
        "id": "babble.ai.embed",
        "version": 1,
        "scope": {
            "input_modalities": ["text"],
            "models": ["local/embed-v1"],
            "max_input_bytes": 4096,
            "dimensions": 384
        }
    });
    let transcribe_capability = json!({
        "id": "babble.ai.transcribe",
        "version": 1,
        "scope": {
            "media_types": ["audio/webm"],
            "models": ["local/transcribe-v1"],
            "languages": ["en-US"],
            "max_duration_ms": 60000
        }
    });
    let draft = ObjectDraft::text("AI host action caller")
        .unwrap()
        .with_capability(serde_json::from_value(generate_capability.clone()).unwrap())
        .unwrap()
        .with_capability(serde_json::from_value(embed_capability.clone()).unwrap())
        .unwrap()
        .with_capability(serde_json::from_value(transcribe_capability.clone()).unwrap())
        .unwrap();
    let source = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "ai-host-source",
            "babble.object.publish.v1",
            host_binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "draft": draft
            }),
        )
        .unwrap()
        .with_idempotency_key("ai-host-source-key"),
    );
    assert!(source.error.is_none());
    let source: PublishObjectResponse = serde_json::from_value(source.result.unwrap()).unwrap();

    let generate_grant = grant_rpc_capability(
        &state,
        &catalog,
        host_binding.clone(),
        identity.identity.id.as_str(),
        source.object.id.as_str(),
        generate_capability,
        "ai-generate-grant-key",
    );
    let embed_grant = grant_rpc_capability(
        &state,
        &catalog,
        host_binding.clone(),
        identity.identity.id.as_str(),
        source.object.id.as_str(),
        embed_capability,
        "ai-embed-grant-key",
    );
    let transcribe_grant = grant_rpc_capability(
        &state,
        &catalog,
        host_binding,
        identity.identity.id.as_str(),
        source.object.id.as_str(),
        transcribe_capability,
        "ai-transcribe-grant-key",
    );

    let object_binding = |grants: Vec<String>| {
        RpcBinding::object(
            source.object.id.to_string(),
            "surface",
            "test-runtime",
            "babble://test",
            grants,
        )
        .unwrap()
    };

    let denied_generate = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "ai-generate-denied",
            "babble.ai.generate.v1",
            object_binding(vec![generate_grant.clone()]),
            json!({
                "purpose": "Draft a feed label",
                "task": "image",
                "prompt": "Make a poster",
                "output_modalities": ["image"],
                "model": "local/text-v1",
                "max_output_tokens": 256,
                "temperature_millis": 700
            }),
        )
        .unwrap(),
    );
    let error = denied_generate.error.unwrap();
    assert_eq!(error.code, RpcErrorCode::CapabilityDenied);
    assert!(
        error
            .message
            .contains("no active babble.ai.generate grant permits")
    );

    let generated = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "ai-generate-ok",
            "babble.ai.generate.v1",
            object_binding(vec![generate_grant]),
            json!({
                "purpose": "Draft a feed label",
                "task": "text",
                "prompt": "Summarize this Object for a preview card.",
                "output_modalities": ["text"],
                "model": "local/text-v1",
                "max_output_tokens": 256,
                "temperature_millis": 700
            }),
        )
        .unwrap(),
    );
    assert!(generated.error.is_none());
    let generated: AiGenerateResponse = serde_json::from_value(generated.result.unwrap()).unwrap();
    assert_eq!(generated.action.kind, "ai.generate");
    assert_eq!(generated.action.task, babble_api::AiGenerateTask::Text);
    assert_eq!(generated.receipt.capability.as_str(), "babble.ai.generate");

    let denied_embed = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "ai-embed-denied",
            "babble.ai.embed.v1",
            object_binding(vec![embed_grant.clone()]),
            json!({
                "purpose": "Rank related Objects",
                "input_modality": "image",
                "inputs": ["babble://blobs/not-a-real-hash"],
                "model": "local/embed-v1",
                "dimensions": 384
            }),
        )
        .unwrap(),
    );
    let error = denied_embed.error.unwrap();
    assert_eq!(error.code, RpcErrorCode::CapabilityDenied);
    assert!(
        error
            .message
            .contains("no active babble.ai.embed grant permits")
    );

    let embedded = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "ai-embed-ok",
            "babble.ai.embed.v1",
            object_binding(vec![embed_grant]),
            json!({
                "purpose": "Rank related Objects",
                "input_modality": "text",
                "inputs": ["Babble Objects are executable social media."],
                "model": "local/embed-v1",
                "dimensions": 384
            }),
        )
        .unwrap(),
    );
    assert!(embedded.error.is_none());
    let embedded: AiEmbedResponse = serde_json::from_value(embedded.result.unwrap()).unwrap();
    assert_eq!(embedded.action.kind, "ai.embed");
    assert_eq!(embedded.receipt.capability.as_str(), "babble.ai.embed");

    let media_uri = format!(
        "babble://blobs/{}",
        babble_types::Hash::from_bytes(b"caption me")
    );
    let denied_transcribe = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "ai-transcribe-denied",
            "babble.ai.transcribe.v1",
            object_binding(vec![transcribe_grant.clone()]),
            json!({
                "purpose": "Caption audio Object",
                "media_uri": media_uri.clone(),
                "media_type": "audio/webm",
                "model": "local/transcribe-v1",
                "language": "es-ES",
                "max_duration_ms": 30000
            }),
        )
        .unwrap(),
    );
    let error = denied_transcribe.error.unwrap();
    assert_eq!(error.code, RpcErrorCode::CapabilityDenied);
    assert!(
        error
            .message
            .contains("no active babble.ai.transcribe grant permits")
    );

    let transcribed = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "ai-transcribe-ok",
            "babble.ai.transcribe.v1",
            object_binding(vec![transcribe_grant]),
            json!({
                "purpose": "Caption audio Object",
                "media_uri": media_uri,
                "media_type": "audio/webm",
                "model": "local/transcribe-v1",
                "language": "en-US",
                "max_duration_ms": 30000
            }),
        )
        .unwrap(),
    );
    assert!(transcribed.error.is_none());
    let transcribed: AiTranscribeResponse =
        serde_json::from_value(transcribed.result.unwrap()).unwrap();
    assert_eq!(transcribed.action.kind, "ai.transcribe");
    assert_eq!(
        transcribed.receipt.capability.as_str(),
        "babble.ai.transcribe"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_lists_persisted_object_judgments() {
    let root = unique_root("dispatch-object-judgments");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let binding = RpcBinding::host("test-runtime", "babble://test").unwrap();

    let created = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "object-judgments-identity",
            "babble.identity.create.v1",
            binding.clone(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("object-judgments-identity-key"),
    );
    assert!(created.error.is_none());
    let identity: CreateIdentityResponse = serde_json::from_value(created.result.unwrap()).unwrap();

    let published = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "object-judgments-publish",
            "babble.object.publish_text.v1",
            binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "text": "A cited dataset and reproducible method should surface content analysis and moderation judgments."
            }),
        )
        .unwrap()
        .with_idempotency_key("object-judgments-publish-key"),
    );
    assert!(published.error.is_none());
    let published: PublishTextResponse = serde_json::from_value(published.result.unwrap()).unwrap();

    let listed = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "object-judgments-list",
            "babble.judgment.object.list.v1",
            binding,
            json!({"object_id": published.object.id}),
        )
        .unwrap(),
    );
    assert!(listed.error.is_none());
    let listed: ObjectJudgmentsResponse = serde_json::from_value(listed.result.unwrap()).unwrap();
    let definitions = listed
        .judgments
        .iter()
        .map(|judgment| judgment.definition.as_str())
        .collect::<std::collections::BTreeSet<_>>();

    assert_eq!(listed.object_id, published.object.id.to_string());
    assert!(definitions.contains("babble.judgment.spam.v1"));
    assert!(definitions.contains("babble.judgment.evidence_quality.v1"));
    assert!(definitions.contains("babble.judgment.content_analysis.v1"));
    assert!(definitions.contains("babble.judgment.moderation.v1"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_rejects_mutations_without_idempotency_keys() {
    let root = unique_root("dispatch-idempotency");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();

    let response = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "request-missing-key",
            "babble.identity.create.v1",
            RpcBinding::host("test-runtime", "babble://test").unwrap(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap(),
    );

    let error = response.error.unwrap();
    assert_eq!(error.code, RpcErrorCode::InvalidInput);
    assert!(error.message.contains("missing idempotency key"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_publishes_fork_and_remix_provenance() {
    let root = unique_root("dispatch-provenance");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let binding = RpcBinding::host("test-runtime", "babble://test").unwrap();

    let created = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "request-identity",
            "babble.identity.create.v1",
            binding.clone(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("identity-key"),
    );
    let identity: CreateIdentityResponse = serde_json::from_value(created.result.unwrap()).unwrap();

    let first = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "request-source-a",
            "babble.object.publish_text.v1",
            binding.clone(),
            json!({"author_id": identity.identity.id, "text": "source one"}),
        )
        .unwrap()
        .with_idempotency_key("source-a-key"),
    );
    let first: PublishTextResponse = serde_json::from_value(first.result.unwrap()).unwrap();

    let second = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "request-source-b",
            "babble.object.publish_text.v1",
            binding.clone(),
            json!({"author_id": identity.identity.id, "text": "source two"}),
        )
        .unwrap()
        .with_idempotency_key("source-b-key"),
    );
    let second: PublishTextResponse = serde_json::from_value(second.result.unwrap()).unwrap();

    let fork = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "request-fork",
            "babble.object.fork.v1",
            binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "source_object_id": first.object.id,
                "draft": ObjectDraft::text("forked source").unwrap()
            }),
        )
        .unwrap()
        .with_idempotency_key("fork-key"),
    );
    let fork: ProvenancePublicationResponse = serde_json::from_value(fork.result.unwrap()).unwrap();
    assert_eq!(fork.event.kind, EventKind::ObjectForked);
    assert_eq!(
        fork.object.provenance.forked_from,
        Some(first.object.id.clone())
    );
    assert_eq!(fork.edges[0].relation, Relation::Forks);

    let remix = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "request-remix",
            "babble.object.remix.v1",
            binding,
            json!({
                "author_id": identity.identity.id,
                "source_object_ids": [first.object.id, second.object.id],
                "draft": ObjectDraft::text("remixed sources").unwrap()
            }),
        )
        .unwrap()
        .with_idempotency_key("remix-key"),
    );
    let remix: ProvenancePublicationResponse =
        serde_json::from_value(remix.result.unwrap()).unwrap();
    assert_eq!(remix.event.kind, EventKind::ObjectRemixed);
    assert_eq!(remix.object.provenance.remixed_from.len(), 2);
    assert!(
        remix
            .edges
            .iter()
            .all(|edge| edge.relation == Relation::Remixes)
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_denies_required_capability_without_bound_grant() {
    let root = unique_root("dispatch-capability");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let object_id = "obj_0000000000000000000000000000000000000000000000000000000000000000";

    let response = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "request-start-session",
            "babble.realtime.session.start.v1",
            RpcBinding::object(
                object_id,
                "surface-session",
                "test-runtime",
                "babble://test",
                Vec::new(),
            )
            .unwrap(),
            json!({
                "author_id": "id_0000000000000000000000000000000000000000000000000000000000000000",
                "room_id": "room_0000000000000000000000000000000000000000000000000000000000000000"
            }),
        )
        .unwrap(),
    );

    let error = response.error.unwrap();
    assert_eq!(error.code, RpcErrorCode::CapabilityDenied);
    assert!(error.message.contains("missing capability grant binding"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_denies_object_bound_surface_session_mutation() {
    let root = unique_root("dispatch-runtime-session");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();

    let response = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "surface-transition",
            "babble.runtime.surface.session.transition.v1",
            RpcBinding::object(
                "obj_0000000000000000000000000000000000000000000000000000000000000000",
                "surf_0000000000000000000000000000000000000000000000000000000000000000",
                "test-runtime",
                "babble://test",
                Vec::new(),
            )
            .unwrap(),
            json!({"lifecycle": "active", "reason": "object tried to self-activate"}),
        )
        .unwrap(),
    );

    let error = response.error.unwrap();
    assert_eq!(error.code, RpcErrorCode::CapabilityDenied);
    assert!(error.message.contains("trusted host runtime binding"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_schedules_host_owned_surface_sessions() {
    let root = unique_root("dispatch-runtime-schedule");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let host_binding = RpcBinding::host("test-runtime", "babble://test").unwrap();

    let identity = create_rpc_identity(&state, &catalog, host_binding.clone(), "surface-author");
    let bundle_hash = Hash::from_bytes(b"export default function mount(host) { host.ready(); }");
    let draft = ObjectDraft::text("scheduled executable surface")
        .unwrap()
        .with_resource(Resource {
            uri: "surface.js".to_string(),
            media_type: "text/javascript".to_string(),
            integrity: bundle_hash.clone(),
        })
        .unwrap()
        .with_surface(Surface {
            bundle: None,
            role: SurfaceRole::Feed,
            target: SurfaceTarget::Web,
            entry: "surface.js".to_string(),
            integrity: Some(bundle_hash),
        })
        .unwrap();
    let published = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "schedule-publish",
            "babble.object.publish.v1",
            host_binding.clone(),
            json!({"author_id": identity.identity.id, "draft": draft}),
        )
        .unwrap()
        .with_idempotency_key("schedule-publish-key"),
    );
    assert!(published.error.is_none(), "{:?}", published.error);
    let published: PublishObjectResponse =
        serde_json::from_value(published.result.unwrap()).unwrap();

    let started = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "schedule-start",
            "babble.runtime.surface.session.start.v1",
            host_binding.clone(),
            json!({"object_id": published.object.id, "role": "Feed"}),
        )
        .unwrap(),
    );
    assert!(started.error.is_none(), "{:?}", started.error);
    let started: SurfaceSessionResponse = serde_json::from_value(started.result.unwrap()).unwrap();

    let scheduled = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "schedule-decision",
            "babble.runtime.surface.session.schedule.v1",
            RpcBinding {
                object_id: None,
                surface_session_id: Some(started.session.id.to_string()),
                runtime_id: "test-runtime".to_string(),
                origin: "babble://test".to_string(),
                capability_grants: Vec::new(),
                identity_id: None,
            },
            json!({
                "input": {
                    "viewport_distance_px": 40,
                    "approaching_viewport": true,
                    "interaction_score": 900,
                    "memory_pressure": "normal",
                    "gpu_pressure": "normal",
                    "battery_saver": false,
                    "metered_network": false,
                    "device_class": "desktop"
                }
            }),
        )
        .unwrap(),
    );
    assert!(scheduled.error.is_none(), "{:?}", scheduled.error);
    let scheduled: ScheduleSurfaceSessionResponse =
        serde_json::from_value(scheduled.result.unwrap()).unwrap();
    assert_eq!(scheduled.decision.lifecycle, SurfaceLifecycle::Active);
    assert!(!scheduled.decision.zero_cpu_required);

    let applied = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "schedule-apply",
            "babble.runtime.surface.session.apply_schedule.v1",
            RpcBinding {
                object_id: None,
                surface_session_id: Some(started.session.id.to_string()),
                runtime_id: "test-runtime".to_string(),
                origin: "babble://test".to_string(),
                capability_grants: Vec::new(),
                identity_id: None,
            },
            json!({
                "input": {
                    "viewport_distance_px": 40,
                    "approaching_viewport": true,
                    "interaction_score": 900,
                    "memory_pressure": "normal",
                    "gpu_pressure": "normal",
                    "battery_saver": false,
                    "metered_network": false,
                    "device_class": "desktop"
                }
            }),
        )
        .unwrap(),
    );
    assert!(applied.error.is_none(), "{:?}", applied.error);
    let applied: ApplySurfaceScheduleResponse =
        serde_json::from_value(applied.result.unwrap()).unwrap();
    assert_eq!(applied.decision.lifecycle, SurfaceLifecycle::Active);
    assert_eq!(applied.session.lifecycle, SurfaceLifecycle::Active);
    assert_eq!(applied.events.len(), 1);

    let health = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "schedule-health",
            "babble.runtime.surface.health.v1",
            host_binding.clone(),
            json!({}),
        )
        .unwrap(),
    );
    assert!(health.error.is_none(), "{:?}", health.error);
    let health: SurfaceRuntimeHealthResponse =
        serde_json::from_value(health.result.unwrap()).unwrap();
    assert_eq!(health.health.session_count, 1);
    assert_eq!(health.health.lifecycle_counts.active, 1);
    assert_eq!(health.health.sessions[0].event_count, 2);
    assert_eq!(
        health.health.sessions[0].lifecycle,
        SurfaceLifecycle::Active
    );

    let checkpointed = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "surface-state-checkpoint",
            "babble.runtime.surface.session.state.checkpoint.v1",
            RpcBinding {
                object_id: None,
                surface_session_id: Some(started.session.id.to_string()),
                runtime_id: "test-runtime".to_string(),
                origin: "babble://test".to_string(),
                capability_grants: Vec::new(),
                identity_id: None,
            },
            json!({
                "state": {
                    "route": "/feed/obj_surface",
                    "scroll": 144,
                    "active_panel": "conversation"
                },
                "reason": "serialize before backgrounding"
            }),
        )
        .unwrap(),
    );
    assert!(checkpointed.error.is_none(), "{:?}", checkpointed.error);
    let checkpointed: SurfaceStateCheckpointResponse =
        serde_json::from_value(checkpointed.result.unwrap()).unwrap();
    assert_eq!(checkpointed.checkpoint.session_id, started.session.id);
    assert_eq!(
        checkpointed.event.kind,
        SurfaceRuntimeEventKind::StateCheckpointed
    );
    assert_eq!(checkpointed.checkpoint.state["scroll"], json!(144));

    let restored = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "surface-state-restore",
            "babble.runtime.surface.session.state.get.v1",
            RpcBinding::object(
                published.object.id.to_string(),
                started.session.id.to_string(),
                "test-runtime",
                "babble://test",
                Vec::new(),
            )
            .unwrap(),
            json!({"session_id": started.session.id}),
        )
        .unwrap(),
    );
    assert!(restored.error.is_none(), "{:?}", restored.error);
    let restored: SurfaceStateRestoreResponse =
        serde_json::from_value(restored.result.unwrap()).unwrap();
    assert_eq!(
        restored.checkpoint.state_hash,
        checkpointed.checkpoint.state_hash
    );
    assert_eq!(
        restored.checkpoint.state["active_panel"],
        json!("conversation")
    );

    let denied_checkpoint = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "surface-state-checkpoint-object-denied",
            "babble.runtime.surface.session.state.checkpoint.v1",
            RpcBinding::object(
                published.object.id.to_string(),
                started.session.id.to_string(),
                "test-runtime",
                "babble://test",
                Vec::new(),
            )
            .unwrap(),
            json!({
                "state": {"attempt": "object-write"},
                "reason": "object-owned checkpoint attempt"
            }),
        )
        .unwrap(),
    );
    let error = denied_checkpoint.error.unwrap();
    assert_eq!(error.code, RpcErrorCode::CapabilityDenied);
    assert!(error.message.contains("trusted host runtime binding"));

    let denied = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "schedule-apply-object-denied",
            "babble.runtime.surface.session.apply_schedule.v1",
            RpcBinding::object(
                published.object.id.to_string(),
                started.session.id.to_string(),
                "test-runtime",
                "babble://test",
                Vec::new(),
            )
            .unwrap(),
            json!({
                "input": {
                    "viewport_distance_px": 40,
                    "approaching_viewport": true,
                    "interaction_score": 900,
                    "memory_pressure": "normal",
                    "gpu_pressure": "normal",
                    "battery_saver": false,
                    "metered_network": false,
                    "device_class": "desktop"
                }
            }),
        )
        .unwrap(),
    );
    let error = denied.error.unwrap();
    assert_eq!(error.code, RpcErrorCode::CapabilityDenied);
    assert!(error.message.contains("trusted host runtime binding"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_reports_observability_snapshot() {
    let root = unique_root("rpc-observability");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
    let source = node
        .publish_text(
            &alice.id,
            "RPC observability should expose aggregate protocol health.",
        )
        .unwrap();
    let target = node
        .publish_text(
            &alice.id,
            "Judgment metadata should be counted without raw private logs.",
        )
        .unwrap();
    node.publish_edge(
        &alice.id,
        source.id,
        target.id,
        Relation::References,
        EdgeOrigin::HumanAssertion,
    )
    .unwrap();
    let state = ApiState::new(node);
    let catalog = babble_rpc_catalog().unwrap();

    let response = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "observability-snapshot",
            "babble.observability.snapshot.v1",
            RpcBinding::host("test-runtime", "babble://test").unwrap(),
            json!({}),
        )
        .unwrap(),
    );
    assert!(response.error.is_none(), "{:?}", response.error);
    let response: ObservabilitySnapshotResponse =
        serde_json::from_value(response.result.unwrap()).unwrap();

    assert_eq!(response.snapshot.protocol.objects, 2);
    assert_eq!(response.snapshot.protocol.edges, 1);
    assert!(response.snapshot.protocol.event_dag_buildable);
    assert_eq!(response.snapshot.semantic.stored_judgments, 8);
    assert_eq!(response.snapshot.discovery.searchable_text_objects, 2);
    assert_eq!(response.snapshot.runtime.session_count, 0);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_handles_capability_bound_object_storage() {
    let root = unique_root("dispatch-object-storage");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let host_binding = RpcBinding::host("test-runtime", "babble://test").unwrap();
    let capability = json!({
        "id": "babble.storage.object",
        "version": 1,
        "scope": {"namespace": "self"}
    });

    let created = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "storage-identity",
            "babble.identity.create.v1",
            host_binding.clone(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("storage-identity-key"),
    );
    assert!(created.error.is_none());
    let identity: CreateIdentityResponse = serde_json::from_value(created.result.unwrap()).unwrap();

    let source = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "storage-source",
            "babble.object.publish_text.v1",
            host_binding.clone(),
            json!({"author_id": identity.identity.id, "text": "storage source"}),
        )
        .unwrap()
        .with_idempotency_key("storage-source-key"),
    );
    let source: PublishTextResponse = serde_json::from_value(source.result.unwrap()).unwrap();

    let draft = ObjectDraft::text("object-owned storage")
        .unwrap()
        .with_capability(serde_json::from_value(capability.clone()).unwrap())
        .unwrap();
    let storage_object = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "storage-object",
            "babble.object.fork.v1",
            host_binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "source_object_id": source.object.id,
                "draft": draft
            }),
        )
        .unwrap()
        .with_idempotency_key("storage-object-key"),
    );
    assert!(storage_object.error.is_none());
    let storage_object: ProvenancePublicationResponse =
        serde_json::from_value(storage_object.result.unwrap()).unwrap();

    let other = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "storage-other",
            "babble.object.publish_text.v1",
            host_binding.clone(),
            json!({"author_id": identity.identity.id, "text": "other object"}),
        )
        .unwrap()
        .with_idempotency_key("storage-other-key"),
    );
    let other: PublishTextResponse = serde_json::from_value(other.result.unwrap()).unwrap();

    let grant = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "storage-grant",
            "babble.capabilities.grant.v1",
            host_binding,
            json!({
                "author_id": identity.identity.id,
                "object_id": storage_object.object.id,
                "capability": capability,
                "decision": "approved"
            }),
        )
        .unwrap()
        .with_idempotency_key("storage-grant-key"),
    );
    assert!(grant.error.is_none());
    let grant: GrantCapabilityResponse = serde_json::from_value(grant.result.unwrap()).unwrap();
    let grant_id = grant.grants[0].id.to_string();

    let binding = RpcBinding::object(
        storage_object.object.id.to_string(),
        "surface-storage",
        "test-runtime",
        "babble://test",
        vec![grant_id.clone()],
    )
    .unwrap();

    let missing_idempotency = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "storage-missing-idempotency",
            "babble.storage.object.set.v1",
            binding.clone(),
            json!({"key": "settings/theme", "value": {"mode": "dark"}}),
        )
        .unwrap(),
    );
    assert_eq!(
        missing_idempotency.error.unwrap().code,
        RpcErrorCode::InvalidInput
    );

    let stored = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "storage-set",
            "babble.storage.object.set.v1",
            binding.clone(),
            json!({"key": "settings/theme", "value": {"mode": "dark"}}),
        )
        .unwrap()
        .with_idempotency_key("storage-set-key"),
    );
    assert!(stored.error.is_none());
    let stored: ObjectStorageSetResponse = serde_json::from_value(stored.result.unwrap()).unwrap();
    assert_eq!(stored.entry.value, json!({"mode": "dark"}));
    assert_eq!(stored.receipt.capability.as_str(), "babble.storage.object");

    let fetched = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "storage-get",
            "babble.storage.object.get.v1",
            binding.clone(),
            json!({"key": "settings/theme"}),
        )
        .unwrap(),
    );
    let fetched: ObjectStorageGetResponse =
        serde_json::from_value(fetched.result.unwrap()).unwrap();
    assert_eq!(fetched.entry.unwrap().value, json!({"mode": "dark"}));

    let listed = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "storage-list",
            "babble.storage.object.list.v1",
            binding.clone(),
            json!({"prefix": "settings/", "limit": 32}),
        )
        .unwrap(),
    );
    let listed: ObjectStorageListResponse = serde_json::from_value(listed.result.unwrap()).unwrap();
    assert_eq!(listed.entries.len(), 1);
    assert_eq!(listed.entries[0].key, "settings/theme");

    let other_binding = RpcBinding::object(
        other.object.id.to_string(),
        "surface-other",
        "test-runtime",
        "babble://test",
        vec![grant_id],
    )
    .unwrap();
    let cross_object = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "storage-cross-object",
            "babble.storage.object.get.v1",
            other_binding,
            json!({"key": "settings/theme"}),
        )
        .unwrap(),
    );
    assert_eq!(
        cross_object.error.unwrap().code,
        RpcErrorCode::CapabilityDenied
    );

    let deleted = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "storage-delete",
            "babble.storage.object.delete.v1",
            binding,
            json!({"key": "settings/theme"}),
        )
        .unwrap()
        .with_idempotency_key("storage-delete-key"),
    );
    let deleted: ObjectStorageDeleteResponse =
        serde_json::from_value(deleted.result.unwrap()).unwrap();
    assert_eq!(deleted.deleted.unwrap().value, json!({"mode": "dark"}));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_handles_capability_bound_local_storage() {
    let root = unique_root("dispatch-local-storage");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let host_binding = RpcBinding::host("test-runtime", "babble://test").unwrap();
    let capability = json!({
        "id": "babble.storage.local",
        "version": 1,
        "scope": {"namespace": "prefs"}
    });

    let alice = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "local-storage-alice",
            "babble.identity.create.v1",
            host_binding.clone(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("local-storage-alice-key"),
    );
    assert!(alice.error.is_none());
    let alice: CreateIdentityResponse = serde_json::from_value(alice.result.unwrap()).unwrap();

    let bob = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "local-storage-bob",
            "babble.identity.create.v1",
            host_binding.clone(),
            json!({"kind": "Person", "handle": "bob"}),
        )
        .unwrap()
        .with_idempotency_key("local-storage-bob-key"),
    );
    assert!(bob.error.is_none());
    let bob: CreateIdentityResponse = serde_json::from_value(bob.result.unwrap()).unwrap();

    let source = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "local-storage-source",
            "babble.object.publish_text.v1",
            host_binding.clone(),
            json!({"author_id": alice.identity.id, "text": "local storage source"}),
        )
        .unwrap()
        .with_idempotency_key("local-storage-source-key"),
    );
    let source: PublishTextResponse = serde_json::from_value(source.result.unwrap()).unwrap();

    let draft = ObjectDraft::text("local state object")
        .unwrap()
        .with_capability(serde_json::from_value(capability.clone()).unwrap())
        .unwrap();
    let local_object = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "local-storage-object",
            "babble.object.fork.v1",
            host_binding.clone(),
            json!({
                "author_id": alice.identity.id,
                "source_object_id": source.object.id,
                "draft": draft
            }),
        )
        .unwrap()
        .with_idempotency_key("local-storage-object-key"),
    );
    assert!(local_object.error.is_none());
    let local_object: ProvenancePublicationResponse =
        serde_json::from_value(local_object.result.unwrap()).unwrap();
    let grant_id = grant_rpc_capability(
        &state,
        &catalog,
        host_binding,
        alice.identity.id.as_str(),
        local_object.object.id.as_str(),
        capability,
        "local-storage-grant-key",
    );
    let mut alice_binding = RpcBinding::object(
        local_object.object.id.to_string(),
        "surface-local-storage",
        "test-runtime",
        "babble://test",
        vec![grant_id.clone()],
    )
    .unwrap();

    let missing_identity = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "local-storage-missing-identity",
            "babble.storage.local.get.v1",
            alice_binding.clone(),
            json!({"key": "settings/theme"}),
        )
        .unwrap(),
    );
    assert_eq!(
        missing_identity.error.unwrap().code,
        RpcErrorCode::CapabilityDenied
    );

    alice_binding.identity_id = Some(alice.identity.id.to_string());
    let missing_idempotency = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "local-storage-missing-idempotency",
            "babble.storage.local.set.v1",
            alice_binding.clone(),
            json!({"key": "settings/theme", "value": {"mode": "dark"}}),
        )
        .unwrap(),
    );
    assert_eq!(
        missing_idempotency.error.unwrap().code,
        RpcErrorCode::InvalidInput
    );

    let stored = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "local-storage-set",
            "babble.storage.local.set.v1",
            alice_binding.clone(),
            json!({"key": "settings/theme", "value": {"mode": "dark"}}),
        )
        .unwrap()
        .with_idempotency_key("local-storage-set-key"),
    );
    assert!(stored.error.is_none(), "{:?}", stored.error);
    let stored: LocalStorageSetResponse = serde_json::from_value(stored.result.unwrap()).unwrap();
    assert_eq!(stored.entry.key, "settings/theme");
    assert_eq!(stored.entry.value, json!({"mode": "dark"}));
    assert_eq!(stored.receipt.capability.as_str(), "babble.storage.local");

    let mut bob_binding = alice_binding.clone();
    bob_binding.identity_id = Some(bob.identity.id.to_string());
    let bob_stored = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "local-storage-bob-set",
            "babble.storage.local.set.v1",
            bob_binding.clone(),
            json!({"key": "settings/theme", "value": {"mode": "light"}}),
        )
        .unwrap()
        .with_idempotency_key("local-storage-bob-set-key"),
    );
    assert!(bob_stored.error.is_none(), "{:?}", bob_stored.error);

    let fetched = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "local-storage-get",
            "babble.storage.local.get.v1",
            alice_binding.clone(),
            json!({"key": "settings/theme"}),
        )
        .unwrap(),
    );
    let fetched: LocalStorageGetResponse = serde_json::from_value(fetched.result.unwrap()).unwrap();
    assert_eq!(fetched.entry.unwrap().value, json!({"mode": "dark"}));

    let bob_fetched = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "local-storage-bob-get",
            "babble.storage.local.get.v1",
            bob_binding,
            json!({"key": "settings/theme"}),
        )
        .unwrap(),
    );
    let bob_fetched: LocalStorageGetResponse =
        serde_json::from_value(bob_fetched.result.unwrap()).unwrap();
    assert_eq!(bob_fetched.entry.unwrap().value, json!({"mode": "light"}));

    let listed = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "local-storage-list",
            "babble.storage.local.list.v1",
            alice_binding.clone(),
            json!({"prefix": "settings/", "limit": 32}),
        )
        .unwrap(),
    );
    let listed: LocalStorageListResponse = serde_json::from_value(listed.result.unwrap()).unwrap();
    assert_eq!(listed.entries.len(), 1);
    assert_eq!(listed.entries[0].key, "settings/theme");

    let deleted = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "local-storage-delete",
            "babble.storage.local.delete.v1",
            alice_binding.clone(),
            json!({"key": "settings/theme"}),
        )
        .unwrap()
        .with_idempotency_key("local-storage-delete-key"),
    );
    let deleted: LocalStorageDeleteResponse =
        serde_json::from_value(deleted.result.unwrap()).unwrap();
    assert_eq!(deleted.deleted.unwrap().value, json!({"mode": "dark"}));

    let fetched_after_delete = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "local-storage-get-after-delete",
            "babble.storage.local.get.v1",
            alice_binding,
            json!({"key": "settings/theme"}),
        )
        .unwrap(),
    );
    let fetched_after_delete: LocalStorageGetResponse =
        serde_json::from_value(fetched_after_delete.result.unwrap()).unwrap();
    assert!(fetched_after_delete.entry.is_none());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_invocation_social_versioned_methods_are_retired_and_unversioned_requires_authenticated_document() {
    let root = unique_root("dispatch-social");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    for action in ["follow", "unfollow", "share", "reply"] {
        for version in [1, 2] {
            let response = dispatch(&state, RpcRequestEnvelope::new(
                &catalog, format!("{action}-{version}"), format!("babble.social.{action}.v{version}"),
                RpcBinding::host("test", "babble://test").unwrap(),
                json!({"author_id":"untrusted","target_object_id":"untrusted","text":"untrusted"}),
            ).unwrap().with_idempotency_key(format!("{action}-{version}")));
            assert_eq!(
                response.error.unwrap().code,
                if version == 1 {
                    RpcErrorCode::UnsupportedVersion
                } else {
                    RpcErrorCode::CapabilityDenied
                }
            );
        }
    }
    assert!(
        state
            .node
            .lock()
            .unwrap()
            .store()
            .list_edges()
            .unwrap()
            .is_empty()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_handles_capability_bound_current_identity() {
    let root = unique_root("dispatch-identity-current");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let host_binding = RpcBinding::host("test-runtime", "babble://test").unwrap();
    let capability = json!({
        "id": "babble.identity.current",
        "version": 1,
        "scope": {}
    });

    let created = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "identity-current-author",
            "babble.identity.create.v1",
            host_binding.clone(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("identity-current-author-key"),
    );
    assert!(created.error.is_none());
    let identity: CreateIdentityResponse = serde_json::from_value(created.result.unwrap()).unwrap();

    let source = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "identity-current-source",
            "babble.object.publish_text.v1",
            host_binding.clone(),
            json!({"author_id": identity.identity.id, "text": "identity source"}),
        )
        .unwrap()
        .with_idempotency_key("identity-current-source-key"),
    );
    let source: PublishTextResponse = serde_json::from_value(source.result.unwrap()).unwrap();

    let draft = ObjectDraft::text("identity-aware surface")
        .unwrap()
        .with_capability(serde_json::from_value(capability.clone()).unwrap())
        .unwrap();
    let identity_object = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "identity-current-object",
            "babble.object.fork.v1",
            host_binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "source_object_id": source.object.id,
                "draft": draft
            }),
        )
        .unwrap()
        .with_idempotency_key("identity-current-object-key"),
    );
    assert!(identity_object.error.is_none());
    let identity_object: ProvenancePublicationResponse =
        serde_json::from_value(identity_object.result.unwrap()).unwrap();
    let grant_id = grant_rpc_capability(
        &state,
        &catalog,
        host_binding,
        identity.identity.id.as_str(),
        identity_object.object.id.as_str(),
        capability,
        "identity-current-grant-key",
    );
    let mut binding = RpcBinding::object(
        identity_object.object.id.to_string(),
        "surface-identity",
        "test-runtime",
        "babble://test",
        vec![grant_id],
    )
    .unwrap();

    let missing_identity = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "identity-current-missing-binding",
            "babble.identity.current.v1",
            binding.clone(),
            json!({}),
        )
        .unwrap(),
    );
    assert_eq!(
        missing_identity.error.unwrap().code,
        RpcErrorCode::CapabilityDenied
    );

    binding.identity_id = Some(identity.identity.id.to_string());
    let current = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "identity-current",
            "babble.identity.current.v1",
            binding,
            json!({}),
        )
        .unwrap(),
    );
    assert!(current.error.is_none());
    let current: IdentityCurrentResponse = serde_json::from_value(current.result.unwrap()).unwrap();
    assert_eq!(current.identity.id, identity.identity.id);
    assert_eq!(current.identity.handle, "alice");
    assert_eq!(
        current.receipt.capability.as_str(),
        "babble.identity.current"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_handles_capability_bound_network_fetch() {
    let root = unique_root("dispatch-network-fetch");
    let (url, server) = spawn_http_response("hello rpc");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let host_binding = RpcBinding::host("test-runtime", "babble://test").unwrap();
    let origin = url.rsplit_once('/').unwrap().0.to_string();
    let capability = json!({
        "id": "babble.network.fetch",
        "version": 1,
        "scope": {"origins": [origin]}
    });

    let created = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "network-identity",
            "babble.identity.create.v1",
            host_binding.clone(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("network-identity-key"),
    );
    assert!(created.error.is_none());
    let identity: CreateIdentityResponse = serde_json::from_value(created.result.unwrap()).unwrap();

    let source = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "network-source",
            "babble.object.publish_text.v1",
            host_binding.clone(),
            json!({"author_id": identity.identity.id, "text": "network source"}),
        )
        .unwrap()
        .with_idempotency_key("network-source-key"),
    );
    let source: PublishTextResponse = serde_json::from_value(source.result.unwrap()).unwrap();

    let draft = ObjectDraft::text("object-owned network fetch")
        .unwrap()
        .with_capability(serde_json::from_value(capability.clone()).unwrap())
        .unwrap();
    let network_object = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "network-object",
            "babble.object.fork.v1",
            host_binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "source_object_id": source.object.id,
                "draft": draft
            }),
        )
        .unwrap()
        .with_idempotency_key("network-object-key"),
    );
    assert!(network_object.error.is_none());
    let network_object: ProvenancePublicationResponse =
        serde_json::from_value(network_object.result.unwrap()).unwrap();
    let grant_id = grant_rpc_capability(
        &state,
        &catalog,
        host_binding,
        identity.identity.id.as_str(),
        network_object.object.id.as_str(),
        capability,
        "network-grant-key",
    );
    let binding = RpcBinding::object(
        network_object.object.id.to_string(),
        "surface-network",
        "test-runtime",
        "babble://test",
        vec![grant_id],
    )
    .unwrap();

    let missing_idempotency = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "network-missing-idempotency",
            "babble.network.fetch.v1",
            binding.clone(),
            json!({"method": "GET", "url": url, "headers": {}}),
        )
        .unwrap(),
    );
    assert_eq!(
        missing_idempotency.error.unwrap().code,
        RpcErrorCode::InvalidInput
    );

    let forbidden_header = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "network-forbidden-header",
            "babble.network.fetch.v1",
            binding.clone(),
            json!({"method": "GET", "url": url, "headers": {"Cookie": "secret=1"}}),
        )
        .unwrap()
        .with_idempotency_key("network-forbidden-header-key"),
    );
    assert_eq!(forbidden_header.error.unwrap().code, RpcErrorCode::Conflict);

    let fetched = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "network-fetch",
            "babble.network.fetch.v1",
            binding,
            json!({"method": "GET", "url": url, "headers": {"X-Babble-Test": "request"}}),
        )
        .unwrap()
        .with_idempotency_key("network-fetch-key"),
    );
    assert!(fetched.error.is_none());
    let fetched: NetworkFetchResponse = serde_json::from_value(fetched.result.unwrap()).unwrap();
    assert_eq!(fetched.status, 200);
    assert_eq!(fetched.body_hex, hex::encode("hello rpc"));
    assert_eq!(fetched.headers.get("x-babble-test"), Some(&"ok".to_string()));
    assert!(!fetched.headers.contains_key("set-cookie"));
    assert_eq!(fetched.receipt.capability.as_str(), "babble.network.fetch");
    server.join().unwrap();

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_handles_capability_bound_payments_and_notifications() {
    let root = unique_root("dispatch-payments-notifications");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let host_binding = RpcBinding::host("test-runtime", "babble://test").unwrap();
    let payment = json!({
        "id": "babble.payments.checkout",
        "version": 1,
        "scope": {
            "currencies": ["USD"],
            "max_amount_minor": 5000,
            "merchant_id": "merchant.babble"
        }
    });
    let notifications = json!({
        "id": "babble.notifications.request",
        "version": 1,
        "scope": {
            "categories": ["game.turn", "creator.update"],
            "purpose": "Notify players and followers about Object activity."
        }
    });

    let created = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "payments-notifications-identity",
            "babble.identity.create.v1",
            host_binding.clone(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("payments-notifications-identity-key"),
    );
    assert!(created.error.is_none(), "{:?}", created.error);
    let identity: CreateIdentityResponse = serde_json::from_value(created.result.unwrap()).unwrap();

    let draft = ObjectDraft::text("checkout and notification object")
        .unwrap()
        .with_capability(serde_json::from_value(payment.clone()).unwrap())
        .unwrap()
        .with_capability(serde_json::from_value(notifications.clone()).unwrap())
        .unwrap();
    let published = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "payments-notifications-object",
            "babble.object.publish.v1",
            host_binding.clone(),
            json!({"author_id": identity.identity.id, "draft": draft}),
        )
        .unwrap()
        .with_idempotency_key("payments-notifications-object-key"),
    );
    assert!(published.error.is_none(), "{:?}", published.error);
    let published: PublishObjectResponse =
        serde_json::from_value(published.result.unwrap()).unwrap();

    let payment_grant = grant_rpc_capability(
        &state,
        &catalog,
        host_binding.clone(),
        identity.identity.id.as_str(),
        published.object.id.as_str(),
        payment,
        "payments-notifications-payment-grant",
    );
    let notification_grant = grant_rpc_capability(
        &state,
        &catalog,
        host_binding,
        identity.identity.id.as_str(),
        published.object.id.as_str(),
        notifications,
        "payments-notifications-notification-grant",
    );
    let binding = RpcBinding::object(
        published.object.id.to_string(),
        "surface-payments-notifications",
        "test-runtime",
        "babble://test",
        vec![payment_grant, notification_grant],
    )
    .unwrap();

    let oversized_payment = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "payments-notifications-too-large",
            "babble.payments.checkout.v1",
            binding.clone(),
            json!({
                "merchant_id": "merchant.babble",
                "merchant_name": "Babble Merchant",
                "currency": "USD",
                "total_amount_minor": 6000,
                "line_items": [{"label": "Pass", "amount_minor": 6000, "quantity": 1}],
                "success_url": "https://example.com/success",
                "cancel_url": "https://example.com/cancel",
                "reference": "order_1"
            }),
        )
        .unwrap()
        .with_idempotency_key("payments-notifications-too-large-key"),
    );
    assert_eq!(
        oversized_payment.error.unwrap().code,
        RpcErrorCode::CapabilityDenied
    );

    let checkout = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "payments-notifications-checkout",
            "babble.payments.checkout.v1",
            binding.clone(),
            json!({
                "merchant_id": "merchant.babble",
                "merchant_name": "Babble Merchant",
                "currency": "USD",
                "total_amount_minor": 2500,
                "line_items": [{"label": "Creator pass", "amount_minor": 2500, "quantity": 1}],
                "success_url": "https://example.com/success",
                "cancel_url": "https://example.com/cancel",
                "reference": "order_2"
            }),
        )
        .unwrap()
        .with_idempotency_key("payments-notifications-checkout-key"),
    );
    assert!(checkout.error.is_none(), "{:?}", checkout.error);
    let checkout: PaymentsCheckoutResponse =
        serde_json::from_value(checkout.result.unwrap()).unwrap();
    assert_eq!(checkout.action.kind, "payments.checkout");
    assert_eq!(checkout.action.currency, "USD");
    assert_eq!(checkout.action.total_amount_minor, 2500);
    assert!(checkout.action.requires_user_activation);
    assert_eq!(
        checkout.receipt.capability.as_str(),
        "babble.payments.checkout"
    );

    let wrong_category = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "payments-notifications-wrong-category",
            "babble.notifications.request.v1",
            binding.clone(),
            json!({
                "purpose": "Notify players and followers about Object activity.",
                "categories": ["marketing"]
            }),
        )
        .unwrap(),
    );
    assert_eq!(
        wrong_category.error.unwrap().code,
        RpcErrorCode::CapabilityDenied
    );

    let notification = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "payments-notifications-notification",
            "babble.notifications.request.v1",
            binding,
            json!({
                "purpose": "Notify players and followers about Object activity.",
                "categories": ["game.turn"]
            }),
        )
        .unwrap(),
    );
    assert!(notification.error.is_none(), "{:?}", notification.error);
    let notification: NotificationsRequestResponse =
        serde_json::from_value(notification.result.unwrap()).unwrap();
    assert_eq!(notification.action.kind, "notifications.request");
    assert_eq!(
        notification.action.categories,
        vec!["game.turn".to_string()]
    );
    assert!(notification.action.requires_user_activation);
    assert_eq!(
        notification.receipt.capability.as_str(),
        "babble.notifications.request"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_handles_capability_bound_media_capture() {
    let root = unique_root("dispatch-media-capture");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let host_binding = RpcBinding::host("test-runtime", "babble://test").unwrap();
    let camera = json!({
        "id": "babble.media.camera",
        "version": 1,
        "scope": {
            "modes": ["photo", "video"],
            "media_types": ["image/jpeg", "video/webm"],
            "max_duration_ms": 30_000,
            "facing_modes": ["user"]
        }
    });
    let microphone = json!({
        "id": "babble.media.microphone",
        "version": 1,
        "scope": {
            "modes": ["audio_clip"],
            "media_types": ["audio/webm"],
            "max_duration_ms": 30_000
        }
    });

    let created = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "media-capture-identity",
            "babble.identity.create.v1",
            host_binding.clone(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("media-capture-identity-key"),
    );
    assert!(created.error.is_none(), "{:?}", created.error);
    let identity: CreateIdentityResponse = serde_json::from_value(created.result.unwrap()).unwrap();

    let draft = ObjectDraft::text("media capture object")
        .unwrap()
        .with_capability(serde_json::from_value(camera.clone()).unwrap())
        .unwrap()
        .with_capability(serde_json::from_value(microphone.clone()).unwrap())
        .unwrap();
    let published = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "media-capture-object",
            "babble.object.publish.v1",
            host_binding.clone(),
            json!({"author_id": identity.identity.id, "draft": draft}),
        )
        .unwrap()
        .with_idempotency_key("media-capture-object-key"),
    );
    assert!(published.error.is_none(), "{:?}", published.error);
    let published: PublishObjectResponse =
        serde_json::from_value(published.result.unwrap()).unwrap();

    let camera_grant = grant_rpc_capability(
        &state,
        &catalog,
        host_binding.clone(),
        identity.identity.id.as_str(),
        published.object.id.as_str(),
        camera,
        "media-capture-camera-grant",
    );
    let microphone_grant = grant_rpc_capability(
        &state,
        &catalog,
        host_binding,
        identity.identity.id.as_str(),
        published.object.id.as_str(),
        microphone,
        "media-capture-microphone-grant",
    );
    let binding = RpcBinding::object(
        published.object.id.to_string(),
        "surface-media-capture",
        "test-runtime",
        "babble://test",
        vec![camera_grant, microphone_grant],
    )
    .unwrap();

    let wrong_facing_mode = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "media-capture-wrong-facing",
            "babble.media.camera.request.v1",
            binding.clone(),
            json!({
                "purpose": "Attach a profile photo to this Object.",
                "mode": "photo",
                "media_types": ["image/jpeg"],
                "max_duration_ms": 5_000,
                "facing_mode": "environment",
                "width": 1280,
                "height": 720
            }),
        )
        .unwrap(),
    );
    assert_eq!(
        wrong_facing_mode.error.unwrap().code,
        RpcErrorCode::CapabilityDenied
    );

    let camera_capture = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "media-capture-camera",
            "babble.media.camera.request.v1",
            binding.clone(),
            json!({
                "purpose": "Attach a profile photo to this Object.",
                "mode": "photo",
                "media_types": ["image/jpeg"],
                "max_duration_ms": 5_000,
                "facing_mode": "user",
                "width": 1280,
                "height": 720
            }),
        )
        .unwrap(),
    );
    assert!(camera_capture.error.is_none(), "{:?}", camera_capture.error);
    let camera_capture: CameraCaptureResponse =
        serde_json::from_value(camera_capture.result.unwrap()).unwrap();
    assert_eq!(camera_capture.action.kind, "media.camera.request");
    assert_eq!(
        serde_json::to_value(&camera_capture.action.mode).unwrap(),
        "photo"
    );
    assert_eq!(
        camera_capture.action.media_types,
        vec!["image/jpeg".to_string()]
    );
    assert!(camera_capture.action.requires_user_activation);
    assert_eq!(
        camera_capture.receipt.capability.as_str(),
        "babble.media.camera"
    );

    let wrong_audio_type = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "media-capture-wrong-audio",
            "babble.media.microphone.request.v1",
            binding.clone(),
            json!({
                "purpose": "Record a short spoken reply.",
                "mode": "audio_clip",
                "media_types": ["audio/wav"],
                "max_duration_ms": 5_000,
                "echo_cancellation": true,
                "noise_suppression": true
            }),
        )
        .unwrap(),
    );
    assert_eq!(
        wrong_audio_type.error.unwrap().code,
        RpcErrorCode::CapabilityDenied
    );

    let microphone_capture = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "media-capture-microphone",
            "babble.media.microphone.request.v1",
            binding,
            json!({
                "purpose": "Record a short spoken reply.",
                "mode": "audio_clip",
                "media_types": ["audio/webm"],
                "max_duration_ms": 5_000,
                "echo_cancellation": true,
                "noise_suppression": true
            }),
        )
        .unwrap(),
    );
    assert!(
        microphone_capture.error.is_none(),
        "{:?}",
        microphone_capture.error
    );
    let microphone_capture: MicrophoneCaptureResponse =
        serde_json::from_value(microphone_capture.result.unwrap()).unwrap();
    assert_eq!(microphone_capture.action.kind, "media.microphone.request");
    assert_eq!(
        serde_json::to_value(&microphone_capture.action.mode).unwrap(),
        "audio_clip"
    );
    assert_eq!(
        microphone_capture.action.media_types,
        vec!["audio/webm".to_string()]
    );
    assert!(microphone_capture.action.requires_user_activation);
    assert_eq!(
        microphone_capture.receipt.capability.as_str(),
        "babble.media.microphone"
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_browser_versioned_methods_retired_and_unversioned_cannot_use_native_grants() {
    let root = unique_root("dispatch-host-actions");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let object = {
        let mut node = state.node.lock().unwrap();
        let actor = node.create_identity(babble_identity::IdentityKind::Person, "browser").unwrap();
        let draft = ObjectDraft::text("browser controller").unwrap()
            .with_capability(serde_json::from_value(json!({"id":"babble.clipboard.write","version":1,"scope":{}})).unwrap()).unwrap()
            .with_capability(serde_json::from_value(json!({"id":"babble.fullscreen.enter","version":1,"scope":{}})).unwrap()).unwrap();
        node.publish_draft(&actor.id, draft).unwrap()
    };
    for capability in ["babble.clipboard.write", "babble.fullscreen.enter"] {
        for version in [1, 2] {
            let name = format!("{capability}.v{version}");
            let request = RpcRequestEnvelope::new(&catalog, &name, &name,
                RpcBinding::object(object.id.to_string(), "surface", "runtime", "babble://test",
                    vec!["historical-approved-grant".into()]).unwrap(),
                if capability.contains("clipboard") {json!({"text":"copy"})} else {json!({})})
                .unwrap().with_idempotency_key(format!("{capability}-{version}"));
            let result = dispatch(&state,request);
            assert_eq!(result.error.unwrap().code, if version == 1 {
                RpcErrorCode::UnsupportedVersion
            } else { RpcErrorCode::CapabilityDenied });
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_handles_capability_bound_realtime_leave() {
    let root = unique_root("dispatch-realtime-leave");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let host_binding = RpcBinding::host("test-runtime", "babble://test").unwrap();
    let join = json!({"id": "babble.realtime.join", "version": 1, "scope": {"room": "main"}});
    let send = json!({"id": "babble.realtime.send", "version": 1, "scope": {"room": "main"}});
    let leave = json!({"id": "babble.realtime.leave", "version": 1, "scope": {"room": "main"}});

    let created = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "realtime-identity",
            "babble.identity.create.v1",
            host_binding.clone(),
            json!({"kind": "Person", "handle": "alice"}),
        )
        .unwrap()
        .with_idempotency_key("realtime-identity-key"),
    );
    let identity: CreateIdentityResponse = serde_json::from_value(created.result.unwrap()).unwrap();

    let source = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "realtime-source",
            "babble.object.publish_text.v1",
            host_binding.clone(),
            json!({"author_id": identity.identity.id, "text": "realtime source"}),
        )
        .unwrap()
        .with_idempotency_key("realtime-source-key"),
    );
    let source: PublishTextResponse = serde_json::from_value(source.result.unwrap()).unwrap();

    let draft = ObjectDraft::text("realtime object")
        .unwrap()
        .with_capability(serde_json::from_value(join.clone()).unwrap())
        .unwrap()
        .with_capability(serde_json::from_value(send.clone()).unwrap())
        .unwrap()
        .with_capability(serde_json::from_value(leave.clone()).unwrap())
        .unwrap();
    let object = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "realtime-object",
            "babble.object.fork.v1",
            host_binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "source_object_id": source.object.id,
                "draft": draft
            }),
        )
        .unwrap()
        .with_idempotency_key("realtime-object-key"),
    );
    let object: ProvenancePublicationResponse =
        serde_json::from_value(object.result.unwrap()).unwrap();

    let room = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "realtime-room",
            "babble.realtime.room.define.v1",
            host_binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "object_id": object.object.id,
                "name": "main",
                "schema": "babble.realtime.state.v1",
                "membership": MembershipPolicy::Open,
                "persistence": PersistencePolicy::DurableMessages,
                "limits": null
            }),
        )
        .unwrap()
        .with_idempotency_key("realtime-room-key"),
    );
    let room: DefineRealtimeRoomResponse = serde_json::from_value(room.result.unwrap()).unwrap();
    let side_room = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "realtime-side-room",
            "babble.realtime.room.define.v1",
            host_binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "object_id": object.object.id,
                "name": "side",
                "schema": "babble.realtime.state.v1",
                "membership": MembershipPolicy::Open,
                "persistence": PersistencePolicy::DurableMessages,
                "limits": null
            }),
        )
        .unwrap()
        .with_idempotency_key("realtime-side-room-key"),
    );
    let side_room: DefineRealtimeRoomResponse =
        serde_json::from_value(side_room.result.unwrap()).unwrap();

    let grant_join = grant_rpc_capability(
        &state,
        &catalog,
        host_binding.clone(),
        &identity.identity.id.to_string(),
        &object.object.id.to_string(),
        join,
        "realtime-grant-join",
    );
    let grant_send = grant_rpc_capability(
        &state,
        &catalog,
        host_binding.clone(),
        &identity.identity.id.to_string(),
        &object.object.id.to_string(),
        send,
        "realtime-grant-send",
    );
    let grant_leave = grant_rpc_capability(
        &state,
        &catalog,
        host_binding,
        &identity.identity.id.to_string(),
        &object.object.id.to_string(),
        leave,
        "realtime-grant-leave",
    );
    let grant_ids = vec![grant_join, grant_send, grant_leave];
    let binding = RpcBinding::object(
        object.object.id.to_string(),
        "surface-realtime",
        "test-runtime",
        "babble://test",
        grant_ids,
    )
    .unwrap();

    let wrong_room = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "realtime-wrong-room",
            "babble.realtime.session.start.v1",
            binding.clone(),
            json!({"author_id": identity.identity.id, "room_id": side_room.room.id}),
        )
        .unwrap(),
    );
    assert_eq!(wrong_room.error.unwrap().code, RpcErrorCode::Conflict);

    let session = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "realtime-start",
            "babble.realtime.session.start.v1",
            binding.clone(),
            json!({"author_id": identity.identity.id, "room_id": room.room.id}),
        )
        .unwrap(),
    );
    let session: StartRealtimeSessionResponse =
        serde_json::from_value(session.result.unwrap()).unwrap();
    let session_receipt = session.receipt.as_ref().unwrap();
    assert_eq!(session_receipt.capability.as_str(), "babble.realtime.join");
    assert_eq!(session_receipt.scope, json!({"room": "main"}));
    assert_eq!(session_receipt.remaining_realtime_connections, 1);

    let message = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "realtime-send",
            "babble.realtime.message.publish.v1",
            binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "session_id": session.session.id,
                "object_id": object.object.id,
                "payload": RealtimePayload::State(RealtimeOperation::IncrementCounter {
                    key: "brush_strokes".to_string(),
                    by: 2,
                }),
                "durable": true
            }),
        )
        .unwrap()
        .with_idempotency_key("realtime-send-key"),
    );
    assert!(message.error.is_none(), "{:?}", message.error);
    let message: PublishRealtimeMessageResponse =
        serde_json::from_value(message.result.unwrap()).unwrap();
    assert_eq!(message.message.sequence, 1);
    let message_receipt = message.receipt.as_ref().unwrap();
    assert_eq!(message_receipt.capability.as_str(), "babble.realtime.send");
    assert!(message_receipt.remaining_bytes_per_minute < message_receipt.quota.bytes_per_minute);

    let missing_idempotency = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "realtime-leave-missing-key",
            "babble.realtime.session.leave.v1",
            binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "session_id": session.session.id,
                "object_id": object.object.id
            }),
        )
        .unwrap(),
    );
    assert_eq!(
        missing_idempotency.error.unwrap().code,
        RpcErrorCode::InvalidInput
    );

    let closed = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "realtime-leave",
            "babble.realtime.session.leave.v1",
            binding.clone(),
            json!({
                "author_id": identity.identity.id,
                "session_id": session.session.id,
                "object_id": object.object.id
            }),
        )
        .unwrap()
        .with_idempotency_key("realtime-leave-key"),
    );
    assert!(closed.error.is_none(), "{:?}", closed.error);
    let closed: CloseRealtimeSessionResponse =
        serde_json::from_value(closed.result.unwrap()).unwrap();
    assert_eq!(closed.event.kind, EventKind::RealtimeSessionClosed);
    assert_eq!(
        closed.receipt.unwrap().capability.as_str(),
        "babble.realtime.leave"
    );

    let rejected_send = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "realtime-send-after-leave",
            "babble.realtime.message.publish.v1",
            binding,
            json!({
                "author_id": identity.identity.id,
                "session_id": session.session.id,
                "object_id": object.object.id,
                "payload": RealtimePayload::State(RealtimeOperation::IncrementCounter {
                    key: "brush_strokes".to_string(),
                    by: 1,
                }),
                "durable": true
            }),
        )
        .unwrap()
        .with_idempotency_key("realtime-send-after-leave-key"),
    );
    assert_eq!(rejected_send.error.unwrap().code, RpcErrorCode::Conflict);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn rpc_http_transport_exposes_catalog_and_dispatches_envelopes() {
    let root = unique_root("http-rpc");
    let app = router(test_state(&root));
    let catalog = babble_rpc_catalog().unwrap();

    let catalog_response =
        request_json(app.clone(), Method::GET, "/rpc/catalog", Value::Null).await;
    assert_eq!(catalog_response.status, StatusCode::OK);
    let remote_catalog: RpcCatalog = serde_json::from_value(catalog_response.body).unwrap();
    assert_eq!(remote_catalog, catalog);

    let request = RpcRequestEnvelope::new(
        &catalog,
        "http-rpc-identity",
        "babble.identity.create.v1",
        RpcBinding::host("browser-runtime", "https://babble.local").unwrap(),
        json!({"kind": "Person", "handle": "mira"}),
    )
    .unwrap()
    .with_idempotency_key("http-rpc-identity-key")
    .with_trace_id("trace-http-rpc");

    let response = request_json(
        app,
        Method::POST,
        "/rpc",
        serde_json::to_value(request).unwrap(),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    let response: RpcResponseEnvelope = serde_json::from_value(response.body).unwrap();
    response.validate(&catalog).unwrap();
    assert_eq!(response.trace_id.as_deref(), Some("trace-http-rpc"));
    assert!(response.error.is_none());
    let created: CreateIdentityResponse = serde_json::from_value(response.result.unwrap()).unwrap();
    assert_eq!(created.identity.handle, "mira");

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rpc_dispatch_handles_encrypted_personalization_sync_vault() {
    let root = unique_root("personalization-sync");
    let state = test_state(&root);
    let catalog = babble_rpc_catalog().unwrap();
    let binding = RpcBinding::host("test-runtime", "babble://test").unwrap();
    let created = create_rpc_identity(&state, &catalog, binding.clone(), "syncer-rpc");
    let key = PersonalizationSyncKey::from_hex(
        "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd",
    )
    .unwrap();
    let recipient =
        PersonalizationSyncRecipient::new(created.identity.id.clone(), "desktop-main").unwrap();
    let envelope = EncryptedLocalUserModel::seal(
        &LocalUserModel {
            model_revision: Some("rpc-sync-rev-1".to_string()),
            interests: vec!["private rpc interest".to_string()],
            novelty_tolerance: 0.7,
            exploration_preference: 0.5,
            evidence_preference: 0.8,
            contradiction_tolerance: 0.2,
            ..Default::default()
        },
        recipient.clone(),
        &key,
    )
    .unwrap();

    let put = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "personalization-sync-put",
            "babble.personalization.sync.put.v1",
            binding.clone(),
            json!({ "envelope": envelope }),
        )
        .unwrap()
        .with_idempotency_key("personalization-sync-put-key"),
    );
    assert!(put.error.is_none());
    let put: PersonalizationSyncPutResponse = serde_json::from_value(put.result.unwrap()).unwrap();

    let listed = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "personalization-sync-list",
            "babble.personalization.sync.list.v1",
            binding.clone(),
            json!({
                "identity_id": created.identity.id,
                "device_id": "desktop-main"
            }),
        )
        .unwrap(),
    );
    assert!(listed.error.is_none());
    let listed_json = serde_json::to_string(&listed.result).unwrap();
    assert!(!listed_json.contains("ciphertext"));
    assert!(!listed_json.contains("private rpc interest"));
    let listed: PersonalizationSyncListResponse =
        serde_json::from_value(listed.result.unwrap()).unwrap();
    assert_eq!(listed.envelopes, vec![put.envelope.clone()]);

    let fetched = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "personalization-sync-get",
            "babble.personalization.sync.get.v1",
            binding.clone(),
            json!({
                "identity_id": created.identity.id,
                "device_id": "desktop-main",
                "envelope_hash": put.envelope.envelope_hash
            }),
        )
        .unwrap(),
    );
    assert!(fetched.error.is_none());
    let fetched: PersonalizationSyncGetResponse =
        serde_json::from_value(fetched.result.unwrap()).unwrap();
    assert_eq!(fetched.summary, put.envelope);
    assert_eq!(
        fetched.envelope.open(&recipient, &key).unwrap().interests,
        vec![
            "interest".to_string(),
            "private".to_string(),
            "rpc".to_string()
        ]
    );

    let deleted = dispatch(
        &state,
        RpcRequestEnvelope::new(
            &catalog,
            "personalization-sync-delete",
            "babble.personalization.sync.delete.v1",
            binding,
            json!({
                "identity_id": created.identity.id,
                "device_id": "desktop-main",
                "envelope_hash": fetched.summary.envelope_hash
            }),
        )
        .unwrap()
        .with_idempotency_key("personalization-sync-delete-key"),
    );
    assert!(deleted.error.is_none());
    let deleted: PersonalizationSyncDeleteResponse =
        serde_json::from_value(deleted.result.unwrap()).unwrap();
    assert!(deleted.deleted.is_some());

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn replies_rpc_pages_over_http_and_validates_inputs() {
    let root = unique_root("replies");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let author = node
        .create_identity(IdentityKind::Person, "reply-author")
        .unwrap();
    let parent = node.publish_text(&author.id, "parent").unwrap();
    let other = node.publish_text(&author.id, "other parent").unwrap();
    let mut expected = Vec::new();
    for text in ["reply one", "reply two", "reply three"] {
        let object = node.publish_text(&author.id, text).unwrap();
        node.publish_edge(
            &author.id,
            object.id.clone(),
            parent.id.clone(),
            Relation::ReplyTo,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();
        expected.push(object);
    }
    expected.sort_by_key(|object| (object.created_at, object.id.clone()));
    let state = ApiState::new(node);
    let app = router(state.clone());
    let catalog = babble_rpc_catalog().unwrap();
    let envelope = |value| {
        serde_json::to_value(
            RpcRequestEnvelope::new(
                &catalog,
                "replies-test",
                "babble.social.replies.list.v1",
                RpcBinding::host("test-runtime", "babble://test").unwrap(),
                value,
            )
            .unwrap(),
        )
        .unwrap()
    };
    let first = request_json(
        app.clone(),
        Method::POST,
        "/rpc",
        envelope(json!({"object_id": parent.id, "cursor": null, "limit": 2})),
    )
    .await;
    assert_eq!(first.status, StatusCode::OK);
    let response: RpcResponseEnvelope = serde_json::from_value(first.body).unwrap();
    assert!(response.error.is_none(), "{:?}", response.error);
    let first: babble_api::RepliesListResponse =
        serde_json::from_value(response.result.unwrap()).unwrap();
    assert_eq!(first.object_id, parent.id);
    assert_eq!(
        first
            .replies
            .iter()
            .map(|reply| &reply.object)
            .collect::<Vec<_>>(),
        expected[..2].iter().collect::<Vec<_>>()
    );
    let cursor = first.next_cursor.unwrap();
    let last = request_json(
        app.clone(),
        Method::POST,
        "/rpc",
        envelope(json!({"object_id": parent.id, "cursor": cursor, "limit": 2})),
    )
    .await;
    let response: RpcResponseEnvelope = serde_json::from_value(last.body).unwrap();
    let last: babble_api::RepliesListResponse =
        serde_json::from_value(response.result.unwrap()).unwrap();
    assert_eq!(last.replies[0].object, expected[2]);
    assert!(last.next_cursor.is_none());
    for invalid in [
        json!({"object_id": other.id, "cursor": cursor, "limit": 1}),
        json!({"object_id": parent.id, "cursor": "malformed", "limit": 1}),
        json!({"object_id": parent.id, "cursor": null, "limit": 0}),
        json!({"object_id": parent.id, "cursor": null, "limit": 51}),
        json!({"object_id": parent.id, "cursor": null, "limit": -1}),
        json!({"object_id": parent.id, "cursor": null, "limit": 1.5}),
        json!({"object_id": parent.id, "limit": 1}),
        json!({"object_id": parent.id, "cursor": null}),
        json!({"object_id": "invalid", "cursor": null, "limit": 1}),
        json!({"object_id": parent.id, "cursor": null, "limit": 1, "unknown": true}),
    ] {
        let response =
            request_json(app.clone(), Method::POST, "/rpc", envelope(invalid.clone())).await;
        let response: RpcResponseEnvelope = serde_json::from_value(response.body).unwrap();
        assert_eq!(
            response.error.unwrap().code,
            RpcErrorCode::InvalidInput,
            "{invalid}"
        );
    }
    let response = request_json(
        app,
        Method::POST,
        "/rpc",
        envelope(
            json!({"object_id": format!("obj_{}", "0".repeat(64)), "cursor": null, "limit": 1}),
        ),
    )
    .await;
    let response: RpcResponseEnvelope = serde_json::from_value(response.body).unwrap();
    assert_eq!(response.error.unwrap().code, RpcErrorCode::NotFound);
    fs::remove_dir_all(root).unwrap();
}

fn dispatch(
    state: &ApiState<LocalProvider>,
    request: RpcRequestEnvelope,
) -> babble_rpc::RpcResponseEnvelope {
    let response = dispatch_rpc_request(state, request);
    let catalog = babble_rpc_catalog().unwrap();
    response.validate(&catalog).unwrap();
    response
}

fn grant_rpc_capability(
    state: &ApiState<LocalProvider>,
    catalog: &RpcCatalog,
    binding: RpcBinding,
    author_id: &str,
    object_id: &str,
    capability: Value,
    idempotency_key: &str,
) -> String {
    let capability_id = capability
        .get("id")
        .and_then(Value::as_str)
        .expect("test capability must include id")
        .to_string();
    let response = dispatch(
        state,
        RpcRequestEnvelope::new(
            catalog,
            idempotency_key,
            "babble.capabilities.grant.v1",
            binding,
            json!({
                "author_id": author_id,
                "object_id": object_id,
                "capability": capability,
                "decision": "approved"
            }),
        )
        .unwrap()
        .with_idempotency_key(idempotency_key),
    );
    assert!(response.error.is_none());
    let response: GrantCapabilityResponse =
        serde_json::from_value(response.result.unwrap()).unwrap();
    response
        .grants
        .iter()
        .find(|grant| grant.capability.as_str() == capability_id)
        .expect("capability grant should be returned")
        .id
        .to_string()
}

fn create_rpc_identity(
    state: &ApiState<LocalProvider>,
    catalog: &RpcCatalog,
    binding: RpcBinding,
    handle: &str,
) -> CreateIdentityResponse {
    let response = dispatch(
        state,
        RpcRequestEnvelope::new(
            catalog,
            &format!("identity-{handle}"),
            "babble.identity.create.v1",
            binding,
            json!({"kind": "Person", "handle": handle}),
        )
        .unwrap()
        .with_idempotency_key(format!("identity-{handle}-key")),
    );
    assert!(response.error.is_none());
    serde_json::from_value(response.result.unwrap()).unwrap()
}

fn test_state(root: &PathBuf) -> ApiState<LocalProvider> {
    ApiState::new(LocalNode::open(root, LocalProvider::default()).unwrap())
}

async fn request_json(app: Router, method: Method, uri: &str, body: Value) -> TestResponse {
    let request = if body == Value::Null {
        Request::builder()
            .method(method)
            .uri(uri)
            .body(Body::empty())
            .unwrap()
    } else {
        Request::builder()
            .method(method)
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap()
    };
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    TestResponse {
        status,
        body: serde_json::from_slice(&body).unwrap(),
    }
}

struct TestResponse {
    status: StatusCode,
    body: Value,
}

fn unique_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("babble-api-rpc-{name}-{nanos}"))
}

fn spawn_http_response(body: &'static str) -> (String, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let url = format!("http://127.0.0.1:{}/resource", address.port());
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 1024];
        let _ = stream.read(&mut request).unwrap();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nX-Babble-Test: ok\r\nSet-Cookie: secret=1\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).unwrap();
    });
    (url, server)
}
