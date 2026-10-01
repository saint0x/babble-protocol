use crate as babel_api;
use crate::routes::trusted_router as router;
use crate::{
    ApiState, ApplySurfaceScheduleResponse, CapabilitiesResponse, CapabilityCatalogResponse,
    CheckpointEventResponse, CheckpointPreviewResponse, ClaimEvidenceResponse,
    CloseRealtimeSessionResponse, CreateIdentityResponse, DiscoveryResponse, EdgeListResponse,
    EventBundleResponse, EventImportResponse, EventListResponse, EventResponse,
    GrantCapabilityResponse, GraphTraversalResponse, InferRelationshipResponse,
    JudgeObjectResponse, JudgmentDefinitionsResponse, JudgmentProvidersResponse,
    LensCatalogResponse, MediaBlobResponse, ObjectJudgmentsResponse, ObjectSearchResponse,
    ObservabilitySnapshotResponse, PersonalizationSyncGetResponse, PersonalizationSyncListResponse,
    PersonalizationSyncPutResponse, PrepareSurfaceResponse, ProvenancePublicationResponse,
    PublishEdgeResponse, PublishMediaObjectResponse, PublishObjectResponse,
    PublishRealtimeMessageResponse, PublishTextResponse, RealtimeRoomResponse,
    ScheduleSurfaceSessionResponse, StartRealtimeSessionResponse, SurfaceRuntimeHealthResponse,
    SurfaceSessionEventResponse, SurfaceSessionResponse, SurfaceStateCheckpointResponse,
    SurfaceStateRestoreResponse,
};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Method, Request, StatusCode, header},
};
use babel_authoring::ObjectDraft;
use babel_capabilities::{GrantDecision, PermissionMode};
use babel_crypto::Keypair;
use babel_graph::{EdgeOrigin, Relation, TraversalDirection};
use babel_identity::{Identity, IdentityKeyScope, IdentityKind};
use babel_judgment::{DefinitionId, ProviderRole};
use babel_judgment_local::LocalProvider;
use babel_lens::{BuiltInLens, CandidateSource, LensExecution};
use babel_media::MediaBlob;
use babel_node::{ImportBundle, LocalNode};
use babel_object::{CapabilityRequest, Object, Resource, Surface, SurfaceRole, SurfaceTarget};
use babel_personalization::{
    EncryptedLocalUserModel, LocalUserModel, PersonalizationSyncKey, PersonalizationSyncRecipient,
};
use babel_realtime::{MembershipPolicy, PersistencePolicy, RealtimeOperation, RealtimePayload};
use babel_runtime::{RuntimeAdmissionStatus, SurfaceLifecycle, SurfaceRuntimeEventKind};
use babel_state::{Event, EventKind, EventTarget};
use babel_types::Hash;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;

#[tokio::test]
async fn api_publishes_and_judges_text_objects() {
    let root = unique_root("api");
    let app = test_app(&root);

    let created = request_json(
        app.clone(),
        Method::POST,
        "/identities",
        json!({"kind": "Person", "handle": "alice"}),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK);
    let identity: CreateIdentityResponse = serde_json::from_value(created.body).unwrap();

    let claim = request_json(
        app.clone(),
        Method::POST,
        "/objects/text",
        json!({
            "author_id": identity.identity.id,
            "text": "According to the dataset, Babel keeps evidence attached to discovery."
        }),
    )
    .await;
    assert_eq!(claim.status, StatusCode::OK);
    let claim: PublishTextResponse = serde_json::from_value(claim.body).unwrap();

    let evidence = request_json(
        app.clone(),
        Method::POST,
        "/objects/text",
        json!({
            "author_id": identity.identity.id,
            "text": "The source methodology reproduces the result."
        }),
    )
    .await;
    assert_eq!(evidence.status, StatusCode::OK);
    let evidence: PublishTextResponse = serde_json::from_value(evidence.body).unwrap();

    let edge = request_json(
        app.clone(),
        Method::POST,
        "/graph/edges",
        json!({
            "author_id": identity.identity.id,
            "source": evidence.object.id,
            "target": claim.object.id,
            "relation": Relation::EvidenceFor,
            "origin": EdgeOrigin::HumanAssertion
        }),
    )
    .await;
    assert_eq!(edge.status, StatusCode::OK);

    let object_judgments = request_json(
        app.clone(),
        Method::GET,
        &format!("/objects/{}/judgments", claim.object.id),
        Value::Null,
    )
    .await;
    assert_eq!(object_judgments.status, StatusCode::OK);
    let object_judgments: ObjectJudgmentsResponse =
        serde_json::from_value(object_judgments.body).unwrap();
    let definitions = object_judgments
        .judgments
        .iter()
        .map(|judgment| judgment.definition.as_str().to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(object_judgments.object_id, claim.object.id.to_string());
    assert!(definitions.contains("babel.judgment.spam.v1"));
    assert!(definitions.contains("babel.judgment.evidence_quality.v1"));
    assert!(definitions.contains("babel.judgment.content_analysis.v1"));
    assert!(definitions.contains("babel.judgment.moderation.v1"));

    let judged = request_json(
        app.clone(),
        Method::POST,
        &format!("/judgments/object/{}", claim.object.id),
        json!({"definition": DefinitionId::evidence_quality_v1()}),
    )
    .await;
    assert_eq!(judged.status, StatusCode::OK);
    let judged: JudgeObjectResponse = serde_json::from_value(judged.body).unwrap();
    let orchestration = judged
        .orchestration
        .as_ref()
        .expect("fresh Judgment evaluation should include orchestration trace");
    assert_eq!(orchestration.judgment, judged.judgment);
    assert_eq!(orchestration.decisions.len(), 1);
    assert_eq!(orchestration.decisions[0].provider.provider, "babel-local");
    assert!(orchestration.decisions[0].cache_hit);

    let fetched = request_json(
        app,
        Method::GET,
        &format!("/judgments/{}", judged.judgment.id),
        Value::Null,
    )
    .await;
    assert_eq!(fetched.status, StatusCode::OK);
    let fetched: JudgeObjectResponse = serde_json::from_value(fetched.body).unwrap();
    assert_eq!(fetched.judgment, judged.judgment);
    assert!(fetched.orchestration.is_none());

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_rejects_invalid_ids() {
    let root = unique_root("api-invalid");
    let app = test_app(&root);

    let response = request_json(
        app,
        Method::POST,
        "/objects/text",
        json!({"author_id": "not-an-identity", "text": "hello"}),
    )
    .await;

    assert_eq!(response.status, StatusCode::BAD_REQUEST);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_lists_judgment_definitions() {
    let root = unique_root("api-judgment-definitions");
    let app = test_app(&root);

    let response = request_json(app, Method::GET, "/judgments/definitions", Value::Null).await;
    assert_eq!(response.status, StatusCode::OK);
    let definitions: JudgmentDefinitionsResponse = serde_json::from_value(response.body).unwrap();

    assert_eq!(definitions.definitions.len(), 7);
    assert!(
        definitions
            .definitions
            .iter()
            .any(|definition| definition.id == DefinitionId::source_agreement_v1())
    );
    let relationship = definitions
        .definitions
        .iter()
        .find(|definition| definition.id == DefinitionId::relationship_v1())
        .expect("relationship Judgment definition should be listed");
    assert_eq!(
        relationship.input_schema,
        "babel.judgment.input.object_text.v1"
    );
    assert_eq!(
        relationship.output_schema,
        "babel.judgment.output.relationship.v1"
    );
    assert!(relationship.meaning.contains("supports"));
    assert!(relationship.calibration.contains("[0, 1]"));

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_lists_judgment_providers() {
    let root = unique_root("api-judgment-providers");
    let app = test_app(&root);

    let response = request_json(app, Method::GET, "/judgments/providers", Value::Null).await;
    assert_eq!(response.status, StatusCode::OK);
    let providers: JudgmentProvidersResponse = serde_json::from_value(response.body).unwrap();

    assert_eq!(providers.providers.len(), 1);
    let provider = &providers.providers[0];
    assert_eq!(provider.provider.provider, "babel-local");
    assert_eq!(provider.provider.model, "rules-v1");
    assert_eq!(provider.role, ProviderRole::Local);
    assert!(provider.enabled);
    assert!(provider.privacy_policy.include_subject);
    assert!(provider.privacy_policy.max_text_bytes.is_none());
    assert!(
        provider
            .supported_definitions
            .contains(&DefinitionId::relationship_v1())
    );
    assert!(
        provider
            .supported_definitions
            .contains(&DefinitionId::moderation_v1())
    );

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_rotates_identity_key_and_continues_publishing() {
    let root = unique_root("api-identity-key-rotation");
    let app = test_app(&root);

    let created = request_json(
        app.clone(),
        Method::POST,
        "/identities",
        json!({"kind": "Person", "handle": "alice"}),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK);
    let identity: CreateIdentityResponse = serde_json::from_value(created.body).unwrap();

    let rotated = request_json(
        app.clone(),
        Method::POST,
        &format!("/identities/{}/keys/rotate", identity.identity.id),
        json!({
            "scope": IdentityKeyScope::Device,
            "expires_at": null,
            "reason": "replace local device signing key"
        }),
    )
    .await;
    assert_eq!(rotated.status, StatusCode::OK);
    let rotated: babel_api::RotateIdentityKeyResponse =
        serde_json::from_value(rotated.body).unwrap();
    assert_eq!(rotated.event.kind, EventKind::IdentityKeyTransition);
    assert_eq!(
        rotated.event.target,
        EventTarget::Identity(identity.identity.id.clone())
    );
    assert_eq!(rotated.transition.identity_id, identity.identity.id);
    assert_ne!(
        rotated.transition.previous_public_key,
        rotated.transition.next_public_key
    );

    let published = request_json(
        app,
        Method::POST,
        "/objects/text",
        json!({
            "author_id": identity.identity.id,
            "text": "Rotated identity keys still publish durable protocol Objects."
        }),
    )
    .await;
    assert_eq!(published.status, StatusCode::OK);
    let published: PublishTextResponse = serde_json::from_value(published.body).unwrap();
    assert_eq!(published.object.author, identity.identity.id);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_publishes_fork_and_remix_lineage() {
    let root = unique_root("api-provenance");
    let app = test_app(&root);

    let created = request_json(
        app.clone(),
        Method::POST,
        "/identities",
        json!({"kind": "Person", "handle": "alice"}),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK);
    let identity: CreateIdentityResponse = serde_json::from_value(created.body).unwrap();

    let first = request_json(
        app.clone(),
        Method::POST,
        "/objects/text",
        json!({
            "author_id": identity.identity.id,
            "text": "Original post for a fork."
        }),
    )
    .await;
    assert_eq!(first.status, StatusCode::OK);
    let first: PublishTextResponse = serde_json::from_value(first.body).unwrap();

    let second = request_json(
        app.clone(),
        Method::POST,
        "/objects/text",
        json!({
            "author_id": identity.identity.id,
            "text": "Second source for a remix."
        }),
    )
    .await;
    assert_eq!(second.status, StatusCode::OK);
    let second: PublishTextResponse = serde_json::from_value(second.body).unwrap();

    let fork = request_json(
        app.clone(),
        Method::POST,
        "/objects/forks",
        json!({
            "author_id": identity.identity.id,
            "source_object_id": first.object.id,
            "draft": ObjectDraft::text("A fork with server-authored provenance.").unwrap()
        }),
    )
    .await;
    assert_eq!(fork.status, StatusCode::OK);
    let fork: ProvenancePublicationResponse = serde_json::from_value(fork.body).unwrap();
    assert_eq!(fork.event.kind, EventKind::ObjectForked);
    assert_eq!(
        fork.object.provenance.forked_from,
        Some(first.object.id.clone())
    );
    assert_eq!(fork.edges.len(), 1);
    assert_eq!(fork.edges[0].relation, Relation::Forks);
    assert_eq!(fork.edges[0].target, first.object.id);

    let remix = request_json(
        app.clone(),
        Method::POST,
        "/objects/remixes",
        json!({
            "author_id": identity.identity.id,
            "source_object_ids": [first.object.id, second.object.id],
            "draft": ObjectDraft::text("A remix with graph lineage.").unwrap()
        }),
    )
    .await;
    assert_eq!(remix.status, StatusCode::OK);
    let remix: ProvenancePublicationResponse = serde_json::from_value(remix.body).unwrap();
    assert_eq!(remix.event.kind, EventKind::ObjectRemixed);
    assert_eq!(remix.object.provenance.remixed_from.len(), 2);
    assert_eq!(remix.edges.len(), 2);
    assert!(
        remix
            .edges
            .iter()
            .all(|edge| edge.relation == Relation::Remixes)
    );

    let outgoing = request_json(
        app,
        Method::GET,
        &format!(
            "/graph/objects/{}/outgoing?relation=remixes",
            remix.object.id
        ),
        Value::Null,
    )
    .await;
    assert_eq!(outgoing.status, StatusCode::OK);
    let outgoing: EdgeListResponse = serde_json::from_value(outgoing.body).unwrap();
    assert_eq!(outgoing.edges.len(), 2);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_serves_records_after_node_reopen() {
    let root = unique_root("api-reopen");
    let app = test_app(&root);

    let created = request_json(
        app.clone(),
        Method::POST,
        "/identities",
        json!({"kind": "Person", "handle": "alice"}),
    )
    .await;
    let identity: CreateIdentityResponse = serde_json::from_value(created.body).unwrap();
    let published = request_json(
        app,
        Method::POST,
        "/objects/text",
        json!({
            "author_id": identity.identity.id,
            "text": "Reloaded API instances should serve durable Object records."
        }),
    )
    .await;
    let published: PublishTextResponse = serde_json::from_value(published.body).unwrap();

    let reopened = test_app(&root);
    let fetched = request_json(
        reopened,
        Method::GET,
        &format!("/objects/{}", published.object.id),
        Value::Null,
    )
    .await;

    assert_eq!(fetched.status, StatusCode::OK);
    let fetched: PublishTextResponse = serde_json::from_value(fetched.body).unwrap();
    assert_eq!(fetched.object, published.object);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_stores_media_blobs_and_publishes_resource_objects() {
    let root = unique_root("api-media");
    let app = test_app(&root);

    let created = request_json(
        app.clone(),
        Method::POST,
        "/identities",
        json!({"kind": "Person", "handle": "alice"}),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK);
    let identity: CreateIdentityResponse = serde_json::from_value(created.body).unwrap();

    let bytes = b"not really png, but real content-addressed bytes";
    let stored = request_json(
        app.clone(),
        Method::POST,
        "/media/blobs",
        json!({
            "media_type": "Image/PNG",
            "bytes_hex": hex::encode(bytes)
        }),
    )
    .await;
    assert_eq!(stored.status, StatusCode::OK);
    let stored: MediaBlobResponse = serde_json::from_value(stored.body).unwrap();
    assert_eq!(stored.blob.media_type, "image/png");
    assert_eq!(stored.blob.size_bytes, bytes.len() as u64);
    assert_eq!(stored.blob.integrity, Hash::from_bytes(bytes));
    assert_eq!(stored.bytes_hex, None);

    let fetched = request_json(
        app.clone(),
        Method::GET,
        &format!(
            "/media/blobs/{}?media_type=image/png",
            stored.blob.integrity
        ),
        Value::Null,
    )
    .await;
    assert_eq!(fetched.status, StatusCode::OK);
    let fetched: MediaBlobResponse = serde_json::from_value(fetched.body).unwrap();
    assert_eq!(fetched.blob, stored.blob);
    assert_eq!(fetched.bytes_hex, Some(hex::encode(bytes)));

    let published = request_json(
        app.clone(),
        Method::POST,
        "/objects/media",
        json!({
            "author_id": identity.identity.id,
            "title": "First image",
            "description": "A content-addressed media Object.",
            "resources": [stored.blob]
        }),
    )
    .await;
    assert_eq!(published.status, StatusCode::OK);
    let published: PublishMediaObjectResponse = serde_json::from_value(published.body).unwrap();
    assert_eq!(published.object.kind.as_str(), "babel.media");
    assert_eq!(published.object.resources, vec![fetched.blob.resource()]);
    assert_eq!(published.object.payload["title"], "First image");
    assert_eq!(
        published.object.payload["primary_resource"]["integrity"],
        fetched.blob.integrity.to_string()
    );

    let invalid_hex = request_json(
        app.clone(),
        Method::POST,
        "/media/blobs",
        json!({"media_type": "image/png", "bytes_hex": "zz"}),
    )
    .await;
    assert_eq!(invalid_hex.status, StatusCode::BAD_REQUEST);

    let missing =
        MediaBlob::from_hash("image/png", Hash::from_bytes(b"missing resource"), 16).unwrap();
    let rejected = request_json(
        app,
        Method::POST,
        "/objects/media",
        json!({
            "author_id": identity.identity.id,
            "title": "Missing resource",
            "resources": [missing]
        }),
    )
    .await;
    assert_eq!(rejected.status, StatusCode::NOT_FOUND);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_publishes_general_object_drafts_with_executable_surfaces() {
    let root = unique_root("api-object-draft");
    let app = test_app(&root);

    let created = request_json(
        app.clone(),
        Method::POST,
        "/identities",
        json!({"kind": "Person", "handle": "alice"}),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK);
    let identity: CreateIdentityResponse = serde_json::from_value(created.body).unwrap();

    let surface_bytes = b"export default function mount(host) { host.ready = true; }";
    let stored = request_json(
        app.clone(),
        Method::POST,
        "/media/blobs",
        json!({
            "media_type": "text/javascript",
            "bytes_hex": hex::encode(surface_bytes)
        }),
    )
    .await;
    assert_eq!(stored.status, StatusCode::OK);
    let stored: MediaBlobResponse = serde_json::from_value(stored.body).unwrap();
    let surface_uri = format!("babel://blobs/{}", stored.blob.integrity);
    let capability = json!({
        "id": "babel.network.fetch",
        "version": 1,
        "scope": {"origins": ["https://example.com"]}
    });

    let published = request_json(
        app.clone(),
        Method::POST,
        "/objects",
        json!({
            "author_id": identity.identity.id,
            "draft": {
                "kind": "babel.application",
                "schema": "example.application.v1",
                "payload": {
                    "title": "Interactive Lens Toy",
                    "description": "A real executable Surface Object draft."
                },
                "surfaces": [{
                    "role": "Feed",
                    "target": "Web",
                    "entry": surface_uri,
                    "integrity": stored.blob.integrity
                }],
                "resources": [{
                    "uri": surface_uri,
                    "media_type": "text/javascript",
                    "integrity": stored.blob.integrity
                }],
                "capabilities": [capability],
                "state": {"launch_count": 0},
                "provenance": {
                    "parent": null,
                    "forked_from": null,
                    "remixed_from": []
                }
            }
        }),
    )
    .await;
    assert_eq!(published.status, StatusCode::OK);
    let published: PublishObjectResponse = serde_json::from_value(published.body).unwrap();
    assert_eq!(published.object.kind.as_str(), "babel.application");
    assert_eq!(published.object.surfaces.len(), 1);
    assert_eq!(published.object.capabilities.len(), 1);
    assert_eq!(published.object.resources[0].uri, surface_uri);

    let prepared = request_json(
        app.clone(),
        Method::POST,
        "/runtime/surfaces/prepare",
        json!({"object_id": published.object.id, "role": SurfaceRole::Feed}),
    )
    .await;
    assert_eq!(prepared.status, StatusCode::OK);
    let prepared: PrepareSurfaceResponse = serde_json::from_value(prepared.body).unwrap();
    assert_eq!(
        prepared.plan.admission,
        RuntimeAdmissionStatus::NeedsPermission
    );
    assert_eq!(prepared.plan.surface.entry, surface_uri);

    let missing_hash = Hash::from_bytes(b"not uploaded");
    let rejected = request_json(
        app,
        Method::POST,
        "/objects",
        json!({
            "author_id": identity.identity.id,
            "draft": {
                "kind": "babel.application",
                "schema": "example.application.v1",
                "payload": {"title": "Broken app"},
                "surfaces": [{
                    "role": "Feed",
                    "target": "Web",
                    "entry": format!("babel://blobs/{missing_hash}"),
                    "integrity": missing_hash
                }],
                "resources": [{
                    "uri": format!("babel://blobs/{missing_hash}"),
                    "media_type": "text/javascript",
                    "integrity": missing_hash
                }],
                "capabilities": [],
                "state": null,
                "provenance": {
                    "parent": null,
                    "forked_from": null,
                    "remixed_from": []
                }
            }
        }),
    )
    .await;
    assert_eq!(rejected.status, StatusCode::NOT_FOUND);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_serves_surface_blobs_with_worker_locked_csp() {
    let root = unique_root("api-surface-blob-csp");
    let app = test_app(&root);
    let html = b"<!doctype html><title>Babel Surface</title><script type=\"module\" src=\"./surface.js\"></script>";

    let stored = request_json(
        app.clone(),
        Method::POST,
        "/media/blobs",
        json!({
            "media_type": "text/html",
            "bytes_hex": hex::encode(html)
        }),
    )
    .await;
    assert_eq!(stored.status, StatusCode::OK);
    let stored: MediaBlobResponse = serde_json::from_value(stored.body).unwrap();

    let fetched = request_bytes(
        app,
        Method::GET,
        &format!(
            "/runtime/surfaces/blobs/{}?media_type=text/html",
            stored.blob.integrity
        ),
        Value::Null,
    )
    .await;

    assert_eq!(fetched.status, StatusCode::OK);
    assert_eq!(fetched.body.as_ref(), html);
    let csp = fetched
        .headers
        .get("content-security-policy")
        .and_then(|value| value.to_str().ok())
        .expect("surface blob responses must carry a CSP");
    assert!(csp.contains("worker-src 'none'"));
    assert!(csp.contains("object-src 'none'"));
    assert!(csp.contains("base-uri 'none'"));
    assert!(csp.contains("form-action 'none'"));

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_previews_publishes_and_fetches_consensus_checkpoints() {
    let root = unique_root("api-checkpoint");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let validators = install_validator_mesh(&mut node);
    let validator_weights = validators
        .iter()
        .map(|validator| (validator.identity.id.to_string(), 1_u64))
        .collect::<BTreeMap<_, _>>();
    let author_id = validators[0].identity.id.to_string();
    let app = router(ApiState::new(node));

    let preview = request_json(
        app.clone(),
        Method::POST,
        "/consensus/checkpoints/preview",
        json!({ "validators": validator_weights }),
    )
    .await;
    assert_eq!(preview.status, StatusCode::OK);
    let preview: CheckpointPreviewResponse = serde_json::from_value(preview.body).unwrap();

    let published = request_json(
        app.clone(),
        Method::POST,
        "/consensus/checkpoints",
        json!({
            "author_id": author_id,
            "validators": validators
                .iter()
                .map(|validator| (validator.identity.id.to_string(), 1_u64))
                .collect::<BTreeMap<_, _>>()
        }),
    )
    .await;
    assert_eq!(published.status, StatusCode::OK);
    let published: CheckpointEventResponse = serde_json::from_value(published.body).unwrap();

    let fetched = request_json(
        app,
        Method::GET,
        &format!("/consensus/checkpoints/{}", published.event.id),
        Value::Null,
    )
    .await;
    assert_eq!(fetched.status, StatusCode::OK);
    let fetched: CheckpointEventResponse = serde_json::from_value(fetched.body).unwrap();

    assert_eq!(published.checkpoint, preview.checkpoint);
    assert_eq!(published.event, fetched.event);
    assert_eq!(published.checkpoint, fetched.checkpoint);
    assert_eq!(published.event.kind, EventKind::ConsensusCheckpoint);
    assert_eq!(
        published.event.parents,
        vec![published.checkpoint.last_finalized_event.clone()]
    );

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_queries_graph_edges_and_event_bundles() {
    let root = unique_root("api-graph-events");
    let app = test_app(&root);

    let created = request_json(
        app.clone(),
        Method::POST,
        "/identities",
        json!({"kind": "Person", "handle": "alice"}),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK);
    let identity: CreateIdentityResponse = serde_json::from_value(created.body).unwrap();

    let claim = request_json(
        app.clone(),
        Method::POST,
        "/objects/text",
        json!({
            "author_id": identity.identity.id,
            "text": "Graph APIs should expose evidence relationships."
        }),
    )
    .await;
    let claim: PublishTextResponse = serde_json::from_value(claim.body).unwrap();

    let evidence = request_json(
        app.clone(),
        Method::POST,
        "/objects/text",
        json!({
            "author_id": identity.identity.id,
            "text": "The relationship is persisted as a signed edge."
        }),
    )
    .await;
    let evidence: PublishTextResponse = serde_json::from_value(evidence.body).unwrap();

    let edge = request_json(
        app.clone(),
        Method::POST,
        "/graph/edges",
        json!({
            "author_id": identity.identity.id,
            "source": evidence.object.id,
            "target": claim.object.id,
            "relation": Relation::EvidenceFor,
            "origin": EdgeOrigin::HumanAssertion
        }),
    )
    .await;
    assert_eq!(edge.status, StatusCode::OK);
    let edge: PublishEdgeResponse = serde_json::from_value(edge.body).unwrap();

    let fetched_edge = request_json(
        app.clone(),
        Method::GET,
        &format!("/graph/edges/{}", edge.edge.id),
        Value::Null,
    )
    .await;
    assert_eq!(fetched_edge.status, StatusCode::OK);
    let fetched_edge: PublishEdgeResponse = serde_json::from_value(fetched_edge.body).unwrap();
    assert_eq!(fetched_edge.edge, edge.edge);

    let incoming = request_json(
        app.clone(),
        Method::GET,
        &format!(
            "/graph/objects/{}/incoming?relation=evidence_for",
            claim.object.id
        ),
        Value::Null,
    )
    .await;
    assert_eq!(incoming.status, StatusCode::OK);
    let incoming: EdgeListResponse = serde_json::from_value(incoming.body).unwrap();
    assert_eq!(incoming.edges, vec![edge.edge.clone()]);

    let outgoing = request_json(
        app.clone(),
        Method::GET,
        &format!("/graph/objects/{}/outgoing", evidence.object.id),
        Value::Null,
    )
    .await;
    assert_eq!(outgoing.status, StatusCode::OK);
    let outgoing: EdgeListResponse = serde_json::from_value(outgoing.body).unwrap();
    assert_eq!(outgoing.edges, vec![edge.edge.clone()]);

    let inferred = request_json(
        app.clone(),
        Method::POST,
        "/graph/relationships/infer",
        json!({
            "author_id": identity.identity.id,
            "source": evidence.object.id,
            "target": claim.object.id,
            "relation": "supports",
            "min_score": 0.2
        }),
    )
    .await;
    assert_eq!(inferred.status, StatusCode::OK);
    let inferred: InferRelationshipResponse = serde_json::from_value(inferred.body).unwrap();
    assert_eq!(inferred.edge.origin, EdgeOrigin::JudgmentDerived);
    assert_eq!(inferred.edge.relation, Relation::Supports);
    assert_eq!(
        inferred.edge.metadata.get("judgment_id"),
        Some(&json!(inferred.judgment.id.to_string()))
    );
    assert_eq!(
        inferred.edge.metadata.get("definition"),
        Some(&json!("babel.judgment.relationship.v1"))
    );
    assert!(
        inferred
            .judgment
            .output
            .get("score")
            .and_then(Value::as_f64)
            .unwrap()
            >= 0.2
    );

    let evidence_projection = request_json(
        app.clone(),
        Method::GET,
        &format!("/graph/objects/{}/evidence", claim.object.id),
        Value::Null,
    )
    .await;
    assert_eq!(evidence_projection.status, StatusCode::OK);
    let evidence_projection: ClaimEvidenceResponse =
        serde_json::from_value(evidence_projection.body).unwrap();
    assert_eq!(evidence_projection.projection.claim, claim.object);
    assert_eq!(evidence_projection.projection.supporting.len(), 2);
    assert_eq!(evidence_projection.projection.contradicting.len(), 0);
    assert_eq!(evidence_projection.projection.summary.human_support, 1);
    assert_eq!(evidence_projection.projection.summary.judgment_support, 1);
    let model_support = evidence_projection
        .projection
        .supporting
        .iter()
        .find(|item| item.edge.id == inferred.edge.id)
        .expect("model-derived support edge should appear in projection");
    assert_eq!(model_support.evidence, evidence.object);
    assert_eq!(
        model_support.relationship_judgment.as_ref().map(|j| &j.id),
        Some(&inferred.judgment.id)
    );
    assert!(
        model_support
            .evidence_judgments
            .iter()
            .any(|judgment| judgment.definition.as_str() == "babel.judgment.evidence_quality.v1")
    );

    let source = request_json(
        app.clone(),
        Method::POST,
        "/objects/text",
        json!({
            "author_id": identity.identity.id,
            "text": "The signed edge was derived from a source dataset."
        }),
    )
    .await;
    let source: PublishTextResponse = serde_json::from_value(source.body).unwrap();

    let source_edge = request_json(
        app.clone(),
        Method::POST,
        "/graph/edges",
        json!({
            "author_id": identity.identity.id,
            "source": source.object.id,
            "target": evidence.object.id,
            "relation": Relation::Cites,
            "origin": EdgeOrigin::HumanAssertion
        }),
    )
    .await;
    assert_eq!(source_edge.status, StatusCode::OK);
    let source_edge: PublishEdgeResponse = serde_json::from_value(source_edge.body).unwrap();

    let traversal = request_json(
        app.clone(),
        Method::POST,
        &format!("/graph/objects/{}/traverse", claim.object.id),
        json!({
            "direction": TraversalDirection::Incoming,
            "relations": [Relation::EvidenceFor, Relation::Cites],
            "max_depth": 2,
            "limit": 8
        }),
    )
    .await;
    assert_eq!(traversal.status, StatusCode::OK);
    let traversal: GraphTraversalResponse = serde_json::from_value(traversal.body).unwrap();
    assert_eq!(traversal.traversal.root, claim.object.id);
    assert!(!traversal.traversal.truncated);
    assert_eq!(traversal.traversal.steps.len(), 2);
    assert_eq!(traversal.traversal.steps[0].edge, edge.edge);
    assert_eq!(traversal.traversal.steps[0].next_object, evidence.object.id);
    assert_eq!(traversal.traversal.steps[1].edge, source_edge.edge);
    assert_eq!(traversal.traversal.steps[1].next_object, source.object.id);

    let edge_event = {
        let reopened = LocalNode::open(&root, LocalProvider::default()).unwrap();
        reopened
            .store()
            .list_events()
            .unwrap()
            .into_iter()
            .find(|event| {
                matches!(
                    &event.target,
                    EventTarget::Edge(edge_id) if edge_id == &edge.edge.id
                )
            })
            .unwrap()
    };
    let fetched_event = request_json(
        app.clone(),
        Method::GET,
        &format!("/events/{}", edge_event.id),
        Value::Null,
    )
    .await;
    assert_eq!(fetched_event.status, StatusCode::OK);
    let fetched_event: EventResponse = serde_json::from_value(fetched_event.body).unwrap();
    assert_eq!(fetched_event.event, edge_event);

    let bundle = request_json(
        app,
        Method::POST,
        "/events/bundle",
        json!({ "events": [edge_event.id] }),
    )
    .await;
    assert_eq!(bundle.status, StatusCode::OK);
    let bundle: EventBundleResponse = serde_json::from_value(bundle.body).unwrap();
    assert!(bundle.bundle.events.len() >= 2);
    assert!(bundle.bundle.edges.contains(&edge.edge));
    assert!(bundle.bundle.objects.contains(&claim.object));
    assert!(bundle.bundle.objects.contains(&evidence.object));

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_lists_and_imports_event_sync_bundles() {
    let source_root = unique_root("api-events-source");
    let target_root = unique_root("api-events-target");
    let source = test_app(&source_root);
    let target = test_app(&target_root);

    let alice = request_json(
        source.clone(),
        Method::POST,
        "/identities",
        json!({"kind": "Person", "handle": "alice"}),
    )
    .await;
    assert_eq!(alice.status, StatusCode::OK);
    let alice: CreateIdentityResponse = serde_json::from_value(alice.body).unwrap();

    let published = request_json(
        source.clone(),
        Method::POST,
        "/objects/text",
        json!({
            "author_id": alice.identity.id,
            "text": "HTTP event sync should carry signed Object state."
        }),
    )
    .await;
    assert_eq!(published.status, StatusCode::OK);
    let published: PublishTextResponse = serde_json::from_value(published.body).unwrap();

    let listed = request_json(source.clone(), Method::GET, "/events?limit=50", Value::Null).await;
    assert_eq!(listed.status, StatusCode::OK);
    let listed: EventListResponse = serde_json::from_value(listed.body).unwrap();
    let object_event = listed
        .events
        .iter()
        .find(|event| {
            matches!(&event.target, babel_state::EventTarget::Object(object_id) if object_id == &published.object.id)
        })
        .expect("published Object should have an event");
    assert_eq!(
        listed.next_after.as_deref(),
        listed.events.last().map(|event| event.id.as_str())
    );

    let bundle = request_json(
        source,
        Method::POST,
        "/events/bundle",
        json!({ "events": [object_event.id.to_string()] }),
    )
    .await;
    assert_eq!(bundle.status, StatusCode::OK);
    let bundle: EventBundleResponse = serde_json::from_value(bundle.body).unwrap();
    assert!(bundle.bundle.identities.contains(&alice.identity));
    assert!(bundle.bundle.objects.contains(&published.object));

    let imported = request_json(
        target.clone(),
        Method::POST,
        "/events/import",
        json!({ "bundle": bundle.bundle }),
    )
    .await;
    assert_eq!(imported.status, StatusCode::OK);
    let imported: EventImportResponse = serde_json::from_value(imported.body).unwrap();
    assert_eq!(imported.report.identities, 1);
    assert_eq!(imported.report.objects, 1);
    assert_eq!(imported.report.events, 2);

    let fetched = request_json(
        target,
        Method::GET,
        &format!("/objects/{}", published.object.id),
        Value::Null,
    )
    .await;
    assert_eq!(fetched.status, StatusCode::OK);
    let fetched: PublishTextResponse = serde_json::from_value(fetched.body).unwrap();
    assert_eq!(fetched.object, published.object);

    fs::remove_dir_all(source_root).unwrap();
    fs::remove_dir_all(target_root).unwrap();
}

#[tokio::test]
async fn api_searches_objects_with_filters_and_scores() {
    let root = unique_root("api-search");
    let app = test_app(&root);

    let alice = request_json(
        app.clone(),
        Method::POST,
        "/identities",
        json!({"kind": "Person", "handle": "alice"}),
    )
    .await;
    let alice: CreateIdentityResponse = serde_json::from_value(alice.body).unwrap();
    let bob = request_json(
        app.clone(),
        Method::POST,
        "/identities",
        json!({"kind": "Person", "handle": "bob"}),
    )
    .await;
    let bob: CreateIdentityResponse = serde_json::from_value(bob.body).unwrap();

    let first = request_json(
        app.clone(),
        Method::POST,
        "/objects/text",
        json!({
            "author_id": alice.identity.id,
            "text": "Babel search indexes evidence-rich protocol records."
        }),
    )
    .await;
    let first: PublishTextResponse = serde_json::from_value(first.body).unwrap();
    let second = request_json(
        app.clone(),
        Method::POST,
        "/objects/text",
        json!({
            "author_id": alice.identity.id,
            "text": "Protocol records carry signed graph context."
        }),
    )
    .await;
    let second: PublishTextResponse = serde_json::from_value(second.body).unwrap();
    let third = request_json(
        app.clone(),
        Method::POST,
        "/objects/text",
        json!({
            "author_id": bob.identity.id,
            "text": "A private note about gardens."
        }),
    )
    .await;
    let third: PublishTextResponse = serde_json::from_value(third.body).unwrap();

    let search = request_json(
        app.clone(),
        Method::GET,
        "/search/objects?q=protocol%20records&kind=babel.text&limit=10",
        Value::Null,
    )
    .await;
    assert_eq!(search.status, StatusCode::OK);
    let search: ObjectSearchResponse = serde_json::from_value(search.body).unwrap();

    assert_eq!(search.results.len(), 2);
    assert_eq!(search.results[0].object, second.object);
    assert_eq!(search.results[1].object, first.object);
    assert!(search.results[0].score >= search.results[1].score);
    assert!(
        search.results[0]
            .reasons
            .iter()
            .any(|reason| reason == "phrase_match")
    );

    let alice_only = request_json(
        app.clone(),
        Method::GET,
        &format!("/search/objects?author={}&limit=10", alice.identity.id),
        Value::Null,
    )
    .await;
    assert_eq!(alice_only.status, StatusCode::OK);
    let alice_only: ObjectSearchResponse = serde_json::from_value(alice_only.body).unwrap();
    assert_eq!(alice_only.results.len(), 2);
    assert!(
        alice_only
            .results
            .iter()
            .all(|result| result.object.author == alice.identity.id)
    );
    assert!(
        !alice_only
            .results
            .iter()
            .any(|result| result.object == third.object)
    );

    let invalid_limit =
        request_json(app, Method::GET, "/search/objects?limit=201", Value::Null).await;
    assert_eq!(invalid_limit.status, StatusCode::BAD_REQUEST);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_lists_lens_catalog() {
    let root = unique_root("api-lenses");
    let app = test_app(&root);

    let response = request_json(app, Method::GET, "/lenses", Value::Null).await;
    assert_eq!(response.status, StatusCode::OK);
    let catalog: LensCatalogResponse = serde_json::from_value(response.body).unwrap();

    assert_eq!(catalog.lenses.len(), BuiltInLens::all().len());
    let research = catalog
        .lenses
        .iter()
        .find(|lens| lens.lens == BuiltInLens::Research)
        .expect("Research Lens should be advertised");
    assert_eq!(research.id, "babel.lens.research.v1");
    assert_eq!(research.version, 1);
    assert_eq!(research.execution, LensExecution::LocalDeterministic);
    assert!(
        research
            .required_signals
            .iter()
            .any(|signal| signal == "evidence_quality")
    );
    assert!(
        research
            .required_sources
            .contains(&CandidateSource::Evidence)
    );
    assert!(research.required_permissions.is_empty());

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_discovers_candidates_from_graph_search_judgment_and_lenses() {
    let root = unique_root("api-discovery");
    let app = test_app(&root);

    let created = request_json(
        app.clone(),
        Method::POST,
        "/identities",
        json!({"kind": "Person", "handle": "alice"}),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK);
    let identity: CreateIdentityResponse = serde_json::from_value(created.body).unwrap();

    let claim = publish_test_text(
        app.clone(),
        &identity.identity.id.to_string(),
        "Babel discovery should combine graph evidence, local judgment, search relevance, and lens traces.",
    )
    .await;
    let supporting = publish_test_text(
        app.clone(),
        &identity.identity.id.to_string(),
        "The reproducible evidence says protocol discovery works from persisted graph context.",
    )
    .await;
    let counter = publish_test_text(
        app.clone(),
        &identity.identity.id.to_string(),
        "A conflicting source claims the protocol discovery signal is incomplete.",
    )
    .await;
    let related = publish_test_text(
        app.clone(),
        &identity.identity.id.to_string(),
        "Related semantic notes reference ranked candidate exploration.",
    )
    .await;
    let social = publish_test_text(
        app.clone(),
        &identity.identity.id.to_string(),
        "A followed curator points toward this social graph candidate.",
    )
    .await;
    let unrelated = publish_test_text(
        app.clone(),
        &identity.identity.id.to_string(),
        "A deliberately strange unrelated object gives the exploration slot something real to rank.",
    )
    .await;

    for (source, relation) in [
        (&supporting.object.id, Relation::EvidenceFor),
        (&counter.object.id, Relation::EvidenceAgainst),
        (&claim.object.id, Relation::References),
    ] {
        let edge = request_json(
            app.clone(),
            Method::POST,
            "/graph/edges",
            json!({
                "author_id": identity.identity.id,
                "source": source,
                "target": if relation == Relation::References {
                    &related.object.id
                } else {
                    &claim.object.id
                },
                "relation": relation,
                "origin": EdgeOrigin::HumanAssertion
            }),
        )
        .await;
        assert_eq!(edge.status, StatusCode::OK);
    }

    let social_edge = request_json(
        app.clone(),
        Method::POST,
        "/graph/edges",
        json!({
            "author_id": identity.identity.id,
            "source": supporting.object.id,
            "target": social.object.id,
            "relation": Relation::Follows,
            "origin": EdgeOrigin::HumanAssertion
        }),
    )
    .await;
    assert_eq!(social_edge.status, StatusCode::OK);

    let discovered = request_json(
        app.clone(),
        Method::POST,
        "/discovery/candidates",
        json!({
            "anchors": [claim.object.id],
            "search": null,
            "followed_objects": [supporting.object.id],
            "limit": 10,
            "exploration_slots": 2
        }),
    )
    .await;
    assert_eq!(discovered.status, StatusCode::OK);
    let discovered: DiscoveryResponse = serde_json::from_value(discovered.body).unwrap();
    let ranked_ids = discovered
        .discovery
        .ranked
        .iter()
        .map(|ranked| ranked.candidate.object_id.clone())
        .collect::<Vec<_>>();
    let object_ids = discovered
        .discovery
        .objects
        .iter()
        .map(|object| object.id.clone())
        .collect::<Vec<_>>();

    assert_eq!(ranked_ids, object_ids);
    assert!(ranked_ids.contains(&claim.object.id));
    assert!(ranked_ids.contains(&supporting.object.id));
    assert!(ranked_ids.contains(&counter.object.id));
    assert!(ranked_ids.contains(&related.object.id));
    assert!(ranked_ids.contains(&social.object.id));
    assert!(ranked_ids.contains(&unrelated.object.id));
    let searched = request_json(
        app.clone(),
        Method::POST,
        "/discovery/candidates",
        json!({
            "anchors": [claim.object.id],
            "search": "protocol discovery evidence",
            "followed_objects": [supporting.object.id, unrelated.object.id],
            "limit": 10,
            "exploration_slots": 2
        }),
    )
    .await;
    assert_eq!(searched.status, StatusCode::OK);
    let searched: DiscoveryResponse = serde_json::from_value(searched.body).unwrap();
    let searched_ids = searched
        .discovery
        .objects
        .iter()
        .map(|object| object.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(searched_ids.len(), 3);
    assert!(searched_ids.contains(&claim.object.id));
    assert!(searched_ids.contains(&supporting.object.id));
    assert!(searched_ids.contains(&counter.object.id));
    assert!(!searched_ids.contains(&related.object.id));
    assert!(!searched_ids.contains(&social.object.id));
    assert!(!searched_ids.contains(&unrelated.object.id));
    assert_eq!(
        discovered.discovery.trace.stack_id,
        "babel.lens.stack.balanced.v1"
    );
    assert_eq!(
        discovered.discovery.trace.candidates.len(),
        discovered.discovery.ranked.len()
    );
    assert!(
        discovered
            .discovery
            .trace
            .candidates
            .iter()
            .all(|candidate| !candidate.lens_contributions.is_empty())
    );
    assert!(
        discovered
            .discovery
            .trace
            .candidates
            .iter()
            .any(|candidate| {
                candidate.object_id == social.object.id
                    && candidate
                        .sources
                        .iter()
                        .any(|source| source.source == CandidateSource::SocialGraph)
            })
    );
    assert!(discovered.discovery.ranked.iter().any(|ranked| {
        ranked
            .reasons
            .iter()
            .any(|reason| reason.signal.contains("evidence_quality"))
    }));

    let invalid_limit = request_json(
        app,
        Method::POST,
        "/discovery/candidates",
        json!({"limit": 201}),
    )
    .await;
    assert_eq!(invalid_limit.status, StatusCode::BAD_REQUEST);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_lists_capability_catalog() {
    let root = unique_root("api-capability-catalog");
    let app = test_app(&root);

    let response = request_json(app, Method::GET, "/capabilities", Value::Null).await;
    assert_eq!(response.status, StatusCode::OK);
    let catalog: CapabilityCatalogResponse = serde_json::from_value(response.body).unwrap();

    assert!(catalog.capabilities.len() >= 20);
    let network = catalog
        .capabilities
        .iter()
        .find(|capability| capability.id.as_str() == "babel.network.fetch")
        .expect("network.fetch capability should be cataloged");
    assert_eq!(network.version, 1);
    assert_eq!(network.permission, PermissionMode::AskOnce);
    assert_eq!(network.quota.calls_per_minute, 60);
    assert_eq!(network.quota.bytes_per_minute, 2 * 1024 * 1024);
    assert_eq!(network.request_schema, json!({"type": "object"}));
    assert_eq!(network.response_schema, json!({"type": "object"}));

    let location = catalog
        .capabilities
        .iter()
        .find(|capability| capability.id.as_str() == "babel.location")
        .expect("denied platform capabilities should remain inspectable");
    assert_eq!(location.permission, PermissionMode::DeniedByDefault);

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_reports_observability_snapshot_from_node_state() {
    let root = unique_root("api-observability");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
    let first = node
        .publish_text(
            &alice.id,
            "Observability should summarize protocol health without private payload logs.",
        )
        .unwrap();
    let second = node
        .publish_text(
            &alice.id,
            "A second Object gives graph density something to count.",
        )
        .unwrap();
    node.publish_edge(
        &alice.id,
        first.id.clone(),
        second.id.clone(),
        Relation::References,
        EdgeOrigin::HumanAssertion,
    )
    .unwrap();
    let app = router(ApiState::new(node));

    let response = request_json(app, Method::GET, "/observability", Value::Null).await;
    assert_eq!(response.status, StatusCode::OK);
    let response: ObservabilitySnapshotResponse = serde_json::from_value(response.body).unwrap();

    assert_eq!(response.snapshot.protocol.identities, 1);
    assert_eq!(response.snapshot.protocol.objects, 2);
    assert_eq!(response.snapshot.protocol.edges, 1);
    assert!(response.snapshot.protocol.events >= 4);
    assert!(response.snapshot.protocol.event_dag_buildable);
    assert_eq!(response.snapshot.semantic.stored_judgments, 8);
    assert_eq!(response.snapshot.discovery.indexed_objects, 2);
    assert_eq!(response.snapshot.discovery.indexed_edges, 1);
    assert_eq!(response.snapshot.runtime.session_count, 0);
    assert!(
        response
            .snapshot
            .privacy_notes
            .iter()
            .any(|note| note.contains("private local personalization"))
    );

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_stores_personalization_sync_as_encrypted_opaque_envelopes() {
    let root = unique_root("api-personalization-sync");
    let app = test_app(&root);
    let created = request_json(
        app.clone(),
        Method::POST,
        "/identities",
        json!({"kind": "Person", "handle": "syncer"}),
    )
    .await;
    let identity: CreateIdentityResponse = serde_json::from_value(created.body).unwrap();
    let key = PersonalizationSyncKey::from_hex(
        "abababababababababababababababababababababababababababababababab",
    )
    .unwrap();
    let recipient =
        PersonalizationSyncRecipient::new(identity.identity.id.clone(), "desktop-main").unwrap();
    let envelope = EncryptedLocalUserModel::seal(
        &LocalUserModel {
            model_revision: Some("sync-rev-1".to_string()),
            interests: vec!["ultra private semantic taste".to_string()],
            novelty_tolerance: 0.8,
            exploration_preference: 0.6,
            evidence_preference: 0.7,
            contradiction_tolerance: 0.3,
            ..Default::default()
        },
        recipient.clone(),
        &key,
    )
    .unwrap();

    let put = request_json(
        app.clone(),
        Method::POST,
        "/personalization/sync/envelopes",
        json!({ "envelope": envelope }),
    )
    .await;
    assert_eq!(put.status, StatusCode::OK);
    let put: PersonalizationSyncPutResponse = serde_json::from_value(put.body).unwrap();
    assert_eq!(put.envelope.identity_id, identity.identity.id.to_string());
    assert_eq!(put.envelope.device_id, "desktop-main");

    let list_uri = format!(
        "/personalization/sync/envelopes?identity_id={}&device_id=desktop-main",
        identity.identity.id
    );
    let listed = request_json(app.clone(), Method::GET, &list_uri, Value::Null).await;
    let listed_json = serde_json::to_string(&listed.body).unwrap();
    assert_eq!(listed.status, StatusCode::OK);
    assert!(!listed_json.contains("ciphertext"));
    assert!(!listed_json.contains("ultra private semantic taste"));
    let listed: PersonalizationSyncListResponse = serde_json::from_value(listed.body).unwrap();
    assert_eq!(listed.envelopes, vec![put.envelope.clone()]);

    let stored_text = read_tree_to_string(root.join("personalization_sync"));
    assert!(!stored_text.contains("ultra private semantic taste"));
    assert!(stored_text.contains("ciphertext"));

    let get_uri = format!(
        "/personalization/sync/envelopes/{}?identity_id={}&device_id=desktop-main",
        put.envelope.envelope_hash, identity.identity.id
    );
    let fetched = request_json(app.clone(), Method::GET, &get_uri, Value::Null).await;
    assert_eq!(fetched.status, StatusCode::OK);
    let fetched: PersonalizationSyncGetResponse = serde_json::from_value(fetched.body).unwrap();
    assert_eq!(fetched.summary, put.envelope);
    assert_eq!(
        fetched.envelope.open(&recipient, &key).unwrap().interests,
        vec![
            "private".to_string(),
            "semantic".to_string(),
            "taste".to_string(),
            "ultra".to_string(),
        ]
    );

    let observed = request_json(app.clone(), Method::GET, "/observability", Value::Null).await;
    let observed: ObservabilitySnapshotResponse = serde_json::from_value(observed.body).unwrap();
    assert_eq!(
        observed.snapshot.personalization_sync.encrypted_envelopes,
        1
    );
    assert!(
        observed
            .snapshot
            .privacy_notes
            .iter()
            .any(|note| { note.contains("encrypted personalization sync envelope bodies") })
    );

    let deleted = request_json(app.clone(), Method::DELETE, &get_uri, Value::Null).await;
    assert_eq!(deleted.status, StatusCode::OK);
    let listed = request_json(app, Method::GET, &list_uri, Value::Null).await;
    let listed: PersonalizationSyncListResponse = serde_json::from_value(listed.body).unwrap();
    assert!(listed.envelopes.is_empty());
}

#[tokio::test]
async fn api_inspects_grants_revokes_and_prepares_surface_runtime() {
    let root = unique_root("api-capabilities");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
    node.import_signing_identity(identity.clone(), keypair.clone())
        .unwrap();
    let bundle_hash = Hash::from_bytes(b"export default function surface() {}");
    let capability = CapabilityRequest {
        id: "babel.network.fetch".to_string(),
        version: 1,
        scope: json!({"origins": ["https://example.com"]}),
    };
    let object = Object::text(&identity, "Executable Babel Object")
        .unwrap()
        .with_resources(vec![Resource {
            uri: "surface.js".to_string(),
            media_type: "text/javascript".to_string(),
            integrity: bundle_hash.clone(),
        }])
        .unwrap()
        .with_surfaces(vec![Surface {
            bundle: None,
            role: SurfaceRole::Feed,
            target: SurfaceTarget::Web,
            entry: "surface.js".to_string(),
            integrity: Some(bundle_hash),
        }])
        .unwrap()
        .with_capabilities(vec![capability.clone()])
        .unwrap()
        .sign(&identity, &keypair)
        .unwrap();
    let object = node.publish_object_record(&identity.id, object).unwrap();
    let app = router(ApiState::new(node));

    let capabilities = request_json(
        app.clone(),
        Method::GET,
        &format!("/objects/{}/capabilities", object.id),
        Value::Null,
    )
    .await;
    assert_eq!(capabilities.status, StatusCode::OK);
    let capabilities: CapabilitiesResponse = serde_json::from_value(capabilities.body).unwrap();
    assert_eq!(capabilities.manifest.requests, vec![capability.clone()]);
    assert!(capabilities.grants.is_empty());

    let pending = request_json(
        app.clone(),
        Method::POST,
        "/runtime/surfaces/prepare",
        json!({"object_id": object.id, "role": SurfaceRole::Feed}),
    )
    .await;
    assert_eq!(pending.status, StatusCode::OK);
    let pending: PrepareSurfaceResponse = serde_json::from_value(pending.body).unwrap();
    assert_eq!(
        pending.plan.admission,
        RuntimeAdmissionStatus::NeedsPermission
    );

    let granted = request_json(
        app.clone(),
        Method::POST,
        "/capabilities/grants",
        json!({
            "author_id": identity.id,
            "object_id": object.id,
            "capability": capability,
            "decision": GrantDecision::Approved,
        }),
    )
    .await;
    assert_eq!(granted.status, StatusCode::OK);
    let granted: GrantCapabilityResponse = serde_json::from_value(granted.body).unwrap();
    assert_eq!(granted.event.kind, EventKind::CapabilityGranted);
    assert_eq!(granted.grants.len(), 1);

    let ready = request_json(
        app.clone(),
        Method::POST,
        "/runtime/surfaces/prepare",
        json!({"object_id": object.id, "role": SurfaceRole::Feed}),
    )
    .await;
    assert_eq!(ready.status, StatusCode::OK);
    let ready: PrepareSurfaceResponse = serde_json::from_value(ready.body).unwrap();
    assert_eq!(ready.plan.admission, RuntimeAdmissionStatus::Ready);
    assert!(ready.plan.sandbox.capability_bridge);

    let started = request_json(
        app.clone(),
        Method::POST,
        "/runtime/surfaces/sessions",
        json!({"object_id": object.id, "role": SurfaceRole::Feed}),
    )
    .await;
    assert_eq!(started.status, StatusCode::OK);
    let started: SurfaceSessionResponse = serde_json::from_value(started.body).unwrap();
    assert_eq!(started.session.plan.object_id, object.id);
    assert_eq!(started.session.lifecycle, SurfaceLifecycle::Prefetched);
    assert_eq!(started.session.events.len(), 1);

    let scheduled = request_json(
        app.clone(),
        Method::POST,
        &format!("/runtime/surfaces/sessions/{}/schedule", started.session.id),
        json!({
            "input": {
                "viewport_distance_px": 2400,
                "approaching_viewport": false,
                "interaction_score": 25,
                "memory_pressure": "critical",
                "gpu_pressure": "normal",
                "battery_saver": true,
                "metered_network": true,
                "device_class": "mobile"
            }
        }),
    )
    .await;
    assert_eq!(scheduled.status, StatusCode::OK);
    let scheduled: ScheduleSurfaceSessionResponse = serde_json::from_value(scheduled.body).unwrap();
    assert_eq!(scheduled.decision.lifecycle, SurfaceLifecycle::Evicted);
    assert!(scheduled.decision.zero_cpu_required);
    assert_eq!(scheduled.decision.budget.cpu_ms_per_minute, 0);

    let applied_schedule = request_json(
        app.clone(),
        Method::POST,
        &format!(
            "/runtime/surfaces/sessions/{}/schedule/apply",
            started.session.id
        ),
        json!({
            "input": {
                "viewport_distance_px": 900,
                "approaching_viewport": true,
                "interaction_score": 320,
                "memory_pressure": "normal",
                "gpu_pressure": "normal",
                "battery_saver": false,
                "metered_network": true,
                "device_class": "desktop"
            }
        }),
    )
    .await;
    assert_eq!(applied_schedule.status, StatusCode::OK);
    let applied_schedule: ApplySurfaceScheduleResponse =
        serde_json::from_value(applied_schedule.body).unwrap();
    assert_eq!(
        applied_schedule.decision.lifecycle,
        SurfaceLifecycle::Prefetched
    );
    assert_eq!(
        applied_schedule.session.lifecycle,
        SurfaceLifecycle::Prefetched
    );
    assert_eq!(applied_schedule.events.len(), 1);
    assert_eq!(
        applied_schedule.events[0].kind,
        SurfaceRuntimeEventKind::BudgetChanged
    );
    assert_eq!(applied_schedule.session.budget.cpu_ms_per_minute, 250);
    assert_eq!(
        applied_schedule.session.budget.network_bytes_per_minute,
        64 * 1024
    );

    let health = request_json(
        app.clone(),
        Method::GET,
        "/runtime/surfaces/health",
        Value::Null,
    )
    .await;
    assert_eq!(health.status, StatusCode::OK);
    let health: SurfaceRuntimeHealthResponse = serde_json::from_value(health.body).unwrap();
    assert_eq!(health.health.session_count, 1);
    assert_eq!(health.health.lifecycle_counts.prefetched, 1);
    assert_eq!(health.health.totals.zero_cpu_sessions, 0);
    assert_eq!(health.health.sessions[0].event_count, 2);
    assert!(
        health.health.sessions[0]
            .last_event_reason
            .as_deref()
            .unwrap_or_default()
            .contains("runtime scheduler")
    );

    let checkpointed = request_json(
        app.clone(),
        Method::POST,
        &format!(
            "/runtime/surfaces/sessions/{}/state/checkpoint",
            started.session.id
        ),
        json!({
            "state": {
                "scroll": 420,
                "focused_object": started.session.plan.object_id,
                "expanded": false
            },
            "reason": "serialize before offscreen suspension"
        }),
    )
    .await;
    assert_eq!(checkpointed.status, StatusCode::OK);
    let checkpointed: SurfaceStateCheckpointResponse =
        serde_json::from_value(checkpointed.body).unwrap();
    assert_eq!(checkpointed.checkpoint.session_id, started.session.id);
    assert_eq!(checkpointed.checkpoint.object_id, object.id);
    assert_eq!(
        checkpointed.event.kind,
        SurfaceRuntimeEventKind::StateCheckpointed
    );
    assert_eq!(checkpointed.checkpoint.state["scroll"], json!(420));
    assert!(checkpointed.checkpoint.size_bytes > 0);

    let restored = request_json(
        app.clone(),
        Method::GET,
        &format!("/runtime/surfaces/sessions/{}/state", started.session.id),
        Value::Null,
    )
    .await;
    assert_eq!(restored.status, StatusCode::OK);
    let restored: SurfaceStateRestoreResponse = serde_json::from_value(restored.body).unwrap();
    assert_eq!(
        restored.checkpoint.state_hash,
        checkpointed.checkpoint.state_hash
    );
    assert_eq!(restored.checkpoint.state["expanded"], json!(false));

    let activated = request_json(
        app.clone(),
        Method::POST,
        &format!(
            "/runtime/surfaces/sessions/{}/lifecycle",
            started.session.id
        ),
        json!({"lifecycle": SurfaceLifecycle::Warm, "reason": "viewport nearing"}),
    )
    .await;
    assert_eq!(activated.status, StatusCode::OK);
    let activated: SurfaceSessionEventResponse = serde_json::from_value(activated.body).unwrap();
    assert_eq!(activated.session.lifecycle, SurfaceLifecycle::Warm);
    assert_eq!(
        activated.event.kind,
        SurfaceRuntimeEventKind::LifecycleTransition
    );

    let lowered_budget = json!({
        "memory_bytes": ready.plan.budget.memory_bytes / 2,
        "cpu_ms_per_minute": 125,
        "gpu_expected": false,
        "network_bytes_per_minute": 32 * 1024,
        "persistent_storage_bytes": ready.plan.budget.persistent_storage_bytes,
        "realtime_connections": 0,
        "background_eligible": false,
    });
    let budgeted = request_json(
        app.clone(),
        Method::POST,
        &format!("/runtime/surfaces/sessions/{}/budget", started.session.id),
        json!({"budget": lowered_budget, "reason": "thermal pressure"}),
    )
    .await;
    assert_eq!(budgeted.status, StatusCode::OK);
    let budgeted: SurfaceSessionEventResponse = serde_json::from_value(budgeted.body).unwrap();
    assert_eq!(budgeted.event.kind, SurfaceRuntimeEventKind::BudgetChanged);
    assert_eq!(
        budgeted.session.budget.memory_bytes,
        ready.plan.budget.memory_bytes / 2
    );

    let raised_budget = json!({
        "memory_bytes": ready.plan.budget.memory_bytes,
        "cpu_ms_per_minute": ready.plan.budget.cpu_ms_per_minute,
        "gpu_expected": false,
        "network_bytes_per_minute": ready.plan.budget.network_bytes_per_minute,
        "persistent_storage_bytes": ready.plan.budget.persistent_storage_bytes,
        "realtime_connections": ready.plan.budget.realtime_connections,
        "background_eligible": false,
    });
    let raised = request_json(
        app.clone(),
        Method::POST,
        &format!("/runtime/surfaces/sessions/{}/budget", started.session.id),
        json!({"budget": raised_budget, "reason": "raise"}),
    )
    .await;
    assert_eq!(raised.status, StatusCode::CONFLICT);

    let revoked = request_json(
        app,
        Method::POST,
        "/capabilities/revocations",
        json!({
            "author_id": identity.id,
            "object_id": object.id,
            "grant_id": granted.grants[0].id,
        }),
    )
    .await;
    assert_eq!(revoked.status, StatusCode::OK);
    let revoked: GrantCapabilityResponse = serde_json::from_value(revoked.body).unwrap();
    assert_eq!(revoked.event.kind, EventKind::CapabilityRevoked);
    assert!(revoked.grants[0].revoked_at.is_some());

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_defines_realtime_room_and_commits_state_messages() {
    let root = unique_root("api-realtime");
    let app = test_app(&root);

    let created = request_json(
        app.clone(),
        Method::POST,
        "/identities",
        json!({"kind": "Person", "handle": "alice"}),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK);
    let identity: CreateIdentityResponse = serde_json::from_value(created.body).unwrap();
    let object = publish_test_text(
        app.clone(),
        &identity.identity.id.to_string(),
        "Collaborative canvas Object",
    )
    .await;

    let room = request_json(
        app.clone(),
        Method::POST,
        "/realtime/rooms",
        json!({
            "author_id": identity.identity.id,
            "object_id": object.object.id,
            "name": "canvas-main",
            "schema": "babel.realtime.state.v1",
            "membership": MembershipPolicy::Open,
            "persistence": PersistencePolicy::DurableMessages
        }),
    )
    .await;
    assert_eq!(room.status, StatusCode::OK);
    let room: babel_api::DefineRealtimeRoomResponse = serde_json::from_value(room.body).unwrap();
    assert_eq!(room.event.kind, EventKind::RealtimeRoomDefined);

    let session = request_json(
        app.clone(),
        Method::POST,
        "/realtime/sessions",
        json!({
            "author_id": identity.identity.id,
            "room_id": room.room.id,
        }),
    )
    .await;
    assert_eq!(session.status, StatusCode::OK);
    let session: StartRealtimeSessionResponse = serde_json::from_value(session.body).unwrap();

    let counter_message = request_json(
        app.clone(),
        Method::POST,
        "/realtime/messages",
        json!({
            "author_id": identity.identity.id,
            "session_id": session.session.id,
            "object_id": object.object.id,
            "payload": RealtimePayload::State(RealtimeOperation::IncrementCounter {
                key: "brush_strokes".to_string(),
                by: 5,
            }),
            "durable": true
        }),
    )
    .await;
    assert_eq!(counter_message.status, StatusCode::OK);
    let counter_message: PublishRealtimeMessageResponse =
        serde_json::from_value(counter_message.body).unwrap();

    let add_message = request_json(
        app.clone(),
        Method::POST,
        "/realtime/messages",
        json!({
            "author_id": identity.identity.id,
            "session_id": session.session.id,
            "object_id": object.object.id,
            "payload": RealtimePayload::State(RealtimeOperation::AddToSet {
                key: "selected_layers".to_string(),
                value: "ink".to_string(),
            }),
            "durable": true
        }),
    )
    .await;
    assert_eq!(add_message.status, StatusCode::OK);
    let add_message: PublishRealtimeMessageResponse =
        serde_json::from_value(add_message.body).unwrap();

    let remove_message = request_json(
        app.clone(),
        Method::POST,
        "/realtime/messages",
        json!({
            "author_id": identity.identity.id,
            "session_id": session.session.id,
            "object_id": object.object.id,
            "payload": RealtimePayload::State(RealtimeOperation::RemoveFromSet {
                key: "selected_layers".to_string(),
                value: "ink".to_string(),
            }),
            "durable": true
        }),
    )
    .await;
    assert_eq!(remove_message.status, StatusCode::OK);
    let remove_message: PublishRealtimeMessageResponse =
        serde_json::from_value(remove_message.body).unwrap();

    let restore_message = request_json(
        app.clone(),
        Method::POST,
        "/realtime/messages",
        json!({
            "author_id": identity.identity.id,
            "session_id": session.session.id,
            "object_id": object.object.id,
            "payload": RealtimePayload::State(RealtimeOperation::AddToSet {
                key: "selected_layers".to_string(),
                value: "ink".to_string(),
            }),
            "durable": true
        }),
    )
    .await;
    assert_eq!(restore_message.status, StatusCode::OK);
    let restore_message: PublishRealtimeMessageResponse =
        serde_json::from_value(restore_message.body).unwrap();

    let closed = request_json(
        app.clone(),
        Method::DELETE,
        &format!("/realtime/sessions/{}", session.session.id),
        json!({
            "author_id": identity.identity.id,
            "session_id": session.session.id,
            "object_id": object.object.id,
        }),
    )
    .await;
    assert_eq!(closed.status, StatusCode::OK);
    let closed: CloseRealtimeSessionResponse = serde_json::from_value(closed.body).unwrap();
    assert_eq!(closed.event.kind, EventKind::RealtimeSessionClosed);

    let rejected_after_close = request_json(
        app.clone(),
        Method::POST,
        "/realtime/messages",
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
    .await;
    assert_eq!(rejected_after_close.status, StatusCode::CONFLICT);

    let view = request_json(
        app,
        Method::GET,
        &format!("/realtime/rooms/{}", room.room.id),
        Value::Null,
    )
    .await;
    assert_eq!(view.status, StatusCode::OK);
    let view: RealtimeRoomResponse = serde_json::from_value(view.body).unwrap();
    assert_eq!(view.room.state.counters.get("brush_strokes"), Some(&5));
    assert!(
        view.room
            .state
            .sets
            .get("selected_layers")
            .is_some_and(|values| values.contains("ink"))
    );
    let version = view
        .room
        .state
        .set_versions
        .get("selected_layers")
        .and_then(|values| values.get("ink"))
        .expect("selected layer version should be present");
    assert_eq!(version.added_by.as_ref(), Some(&restore_message.message.id));
    assert_eq!(
        version.removed_by.as_ref(),
        Some(&remove_message.message.id)
    );
    assert_eq!(
        view.room.durable_messages,
        vec![
            counter_message.message,
            add_message.message,
            remove_message.message,
            restore_message.message
        ]
    );
    assert!(
        !view
            .room
            .presence
            .participants
            .contains(&identity.identity.id)
    );
    assert!(
        !view
            .room
            .presence
            .active_sessions
            .contains(&closed.session.id)
    );

    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn api_commits_realtime_snapshots_for_snapshot_rooms() {
    let root = unique_root("api-realtime-snapshot");
    let app = test_app(&root);

    let created = request_json(
        app.clone(),
        Method::POST,
        "/identities",
        json!({"kind": "Person", "handle": "alice"}),
    )
    .await;
    assert_eq!(created.status, StatusCode::OK);
    let identity: CreateIdentityResponse = serde_json::from_value(created.body).unwrap();
    let object = publish_test_text(
        app.clone(),
        &identity.identity.id.to_string(),
        "Realtime snapshot Object",
    )
    .await;

    let room = request_json(
        app.clone(),
        Method::POST,
        "/realtime/rooms",
        json!({
            "author_id": identity.identity.id,
            "object_id": object.object.id,
            "name": "snapshot-main",
            "schema": "babel.realtime.state.v1",
            "membership": MembershipPolicy::Open,
            "persistence": PersistencePolicy::SnapshotEvery { messages: 2 }
        }),
    )
    .await;
    assert_eq!(room.status, StatusCode::OK);
    let room: babel_api::DefineRealtimeRoomResponse = serde_json::from_value(room.body).unwrap();

    let session = request_json(
        app.clone(),
        Method::POST,
        "/realtime/sessions",
        json!({
            "author_id": identity.identity.id,
            "room_id": room.room.id,
        }),
    )
    .await;
    assert_eq!(session.status, StatusCode::OK);
    let session: StartRealtimeSessionResponse = serde_json::from_value(session.body).unwrap();

    let first = request_json(
        app.clone(),
        Method::POST,
        "/realtime/messages",
        json!({
            "author_id": identity.identity.id,
            "session_id": session.session.id,
            "object_id": object.object.id,
            "payload": RealtimePayload::State(RealtimeOperation::IncrementCounter {
                key: "ticks".to_string(),
                by: 1,
            }),
            "durable": false
        }),
    )
    .await;
    assert_eq!(first.status, StatusCode::OK);
    let first: PublishRealtimeMessageResponse = serde_json::from_value(first.body).unwrap();
    assert!(first.snapshot.is_none());
    assert!(!first.message.durable);

    let second = request_json(
        app.clone(),
        Method::POST,
        "/realtime/messages",
        json!({
            "author_id": identity.identity.id,
            "session_id": session.session.id,
            "object_id": object.object.id,
            "payload": RealtimePayload::State(RealtimeOperation::IncrementCounter {
                key: "ticks".to_string(),
                by: 2,
            }),
            "durable": false
        }),
    )
    .await;
    assert_eq!(second.status, StatusCode::OK);
    let second: PublishRealtimeMessageResponse = serde_json::from_value(second.body).unwrap();
    let snapshot = second
        .snapshot
        .expect("second message should create snapshot");
    assert_eq!(snapshot.sequence, second.message.sequence);
    assert_eq!(snapshot.state.counters.get("ticks"), Some(&3));

    let view = request_json(
        app,
        Method::GET,
        &format!("/realtime/rooms/{}", room.room.id),
        Value::Null,
    )
    .await;
    assert_eq!(view.status, StatusCode::OK);
    let view: RealtimeRoomResponse = serde_json::from_value(view.body).unwrap();
    assert!(view.room.durable_messages.is_empty());
    assert_eq!(view.room.snapshots, vec![snapshot]);

    fs::remove_dir_all(root).unwrap();
}

fn test_app(root: &PathBuf) -> Router {
    let node = LocalNode::open(root, LocalProvider::default()).unwrap();
    router(ApiState::new(node))
}

async fn publish_test_text(app: Router, author_id: &str, text: &str) -> PublishTextResponse {
    let response = request_json(
        app,
        Method::POST,
        "/objects/text",
        json!({
            "author_id": author_id,
            "text": text,
        }),
    )
    .await;
    assert_eq!(response.status, StatusCode::OK);
    serde_json::from_value(response.body).unwrap()
}

struct ValidatorFixture {
    identity: Identity,
    keypair: Keypair,
    identity_event: babel_types::EventId,
}

fn install_validator_mesh(node: &mut LocalNode<LocalProvider>) -> Vec<ValidatorFixture> {
    let validators = ["alice", "bob", "cara", "drew"]
        .into_iter()
        .map(|handle| {
            let keypair = Keypair::generate();
            let identity = Identity::create(IdentityKind::Person, handle, &keypair).unwrap();
            let identity_event = node
                .import_signing_identity(identity.clone(), keypair.clone())
                .unwrap();
            ValidatorFixture {
                identity,
                keypair,
                identity_event,
            }
        })
        .collect::<Vec<_>>();

    let anchor = node
        .publish_text(&validators[0].identity.id, "API checkpoint anchor Object.")
        .unwrap();
    let mut parents = validators
        .iter()
        .map(|validator| validator.identity_event.clone())
        .collect::<Vec<_>>();

    for layer in 0..5 {
        let next = validators
            .iter()
            .map(|validator| {
                Event::new(
                    &validator.identity,
                    EventKind::CapabilityGranted,
                    EventTarget::Object(anchor.id.clone()),
                    serde_json::json!({ "layer": layer }),
                    parents.clone(),
                )
                .unwrap()
                .sign(&validator.identity, &validator.keypair)
                .unwrap()
            })
            .collect::<Vec<_>>();
        node.import_bundle(ImportBundle {
            identities: Vec::new(),
            objects: Vec::new(),
            edges: Vec::new(),
            events: next.clone(),
        })
        .unwrap();
        parents = next.into_iter().map(|event| event.id).collect();
    }

    validators
}

async fn request_json(app: Router, method: Method, uri: &str, body: Value) -> TestResponse {
    let response = request_bytes(app, method, uri, body).await;
    TestResponse {
        status: response.status,
        body: serde_json::from_slice(&response.body).unwrap(),
    }
}

async fn request_bytes(app: Router, method: Method, uri: &str, body: Value) -> BinaryResponse {
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
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    BinaryResponse {
        status,
        headers,
        body,
    }
}

fn read_tree_to_string(path: PathBuf) -> String {
    if path.is_file() {
        return fs::read_to_string(path).unwrap();
    }
    if !path.exists() {
        return String::new();
    }
    let mut output = String::new();
    let mut entries = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    entries.sort();
    for entry in entries {
        output.push_str(&read_tree_to_string(entry));
    }
    output
}

struct TestResponse {
    status: StatusCode,
    body: Value,
}

struct BinaryResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: axum::body::Bytes,
}

fn unique_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("babel-api-{name}-{nanos}"))
}
