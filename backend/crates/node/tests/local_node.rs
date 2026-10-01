use babble_authoring::{CapabilityGrantDraft, EdgeDraft, ObjectDraft};
use babble_capabilities::{CapabilityUsageWindow, GrantDecision};
use babble_crypto::Keypair;
use babble_graph::{Edge, EdgeOrigin, Relation};
use babble_hashgraph::ValidatorSet;
use babble_identity::{Identity, IdentityKeyScope, IdentityKind};
use babble_judgment::DefinitionId;
use babble_judgment_local::LocalProvider;
use babble_lens::CandidateSource;
use babble_media::MediaBlob;
use babble_node::{CapabilityBindingUsage, DiscoveryQuery, ImportBundle, LocalNode};
use babble_object::{CapabilityRequest, Object, Resource, Surface, SurfaceRole, SurfaceTarget};
use babble_realtime::{
    MembershipPolicy, PersistencePolicy, RealtimeOperation, RealtimePayload, RoomLimits, RoomSpec,
    SessionState,
};
use babble_runtime::RuntimeAdmissionStatus;
use babble_state::{Event, EventKind, EventTarget};
use babble_types::Hash;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    net::TcpListener,
    path::PathBuf,
    thread::{self, JoinHandle},
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn local_node_publishes_graph_and_persists_judgment() {
    let root = unique_root("local-node");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();

    let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
    let claim = node
        .publish_text(
            &alice.id,
            "According to the dataset, Babble discovery should preserve evidence context.",
        )
        .unwrap();
    let evidence = node
        .publish_text(
            &alice.id,
            "The cited methodology reproduces the result with source data.",
        )
        .unwrap();
    let edge = node
        .publish_edge(
            &alice.id,
            evidence.id.clone(),
            claim.id.clone(),
            Relation::EvidenceFor,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();
    let judgment = node
        .judge_object(
            &claim.id,
            DefinitionId::evidence_quality_v1(),
            BTreeMap::new(),
        )
        .unwrap();

    assert_eq!(node.object(&claim.id).unwrap(), &claim);
    assert_eq!(node.edge(&edge.id).unwrap(), &edge);
    assert_eq!(
        node.store().get_judgment(&judgment.id).unwrap().unwrap(),
        judgment
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_ingests_semantic_and_moderation_judgments_on_publish() {
    let root = unique_root("local-node-ingestion-judgments");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();

    let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
    let object = node
        .publish_text(
            &alice.id,
            "According to the dataset and methodology, Babble runtime surfaces preserve source evidence.",
        )
        .unwrap();

    let judgments = node.object_judgments(&object.id).unwrap();
    let definitions = judgments
        .iter()
        .map(|judgment| judgment.definition.as_str().to_string())
        .collect::<BTreeSet<_>>();

    assert!(definitions.contains("babble.judgment.spam.v1"));
    assert!(definitions.contains("babble.judgment.evidence_quality.v1"));
    assert!(definitions.contains("babble.judgment.content_analysis.v1"));
    assert!(definitions.contains("babble.judgment.moderation.v1"));
    let content = judgments
        .iter()
        .find(|judgment| judgment.definition == DefinitionId::content_analysis_v1())
        .unwrap();
    assert_eq!(content.output["kind"], "content_analysis");
    assert!(
        content
            .output
            .get("topics")
            .and_then(serde_json::Value::as_array)
            .unwrap()
            .iter()
            .any(|topic| topic == "evidence" || topic == "runtime")
    );
    let moderation = judgments
        .iter()
        .find(|judgment| judgment.definition == DefinitionId::moderation_v1())
        .unwrap();
    assert_eq!(moderation.output["kind"], "moderation");
    assert_eq!(moderation.output["action"], "allow");

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_discovery_derives_reputation_and_preserves_evidence_origin() {
    let root = unique_root("local-node-discovery-reputation");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();

    let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
    let claim = node
        .publish_text(
            &alice.id,
            "According to the dataset, Babble discovery should preserve source accounting.",
        )
        .unwrap();
    let support = node
        .publish_text(
            &alice.id,
            "The cited source supports the claim with reproducible methodology.",
        )
        .unwrap();
    let counter = node
        .publish_text(
            &alice.id,
            "However, this local Judgment-derived relationship disputes one part of the claim.",
        )
        .unwrap();

    node.publish_edge(
        &alice.id,
        support.id.clone(),
        claim.id.clone(),
        Relation::EvidenceFor,
        EdgeOrigin::HumanAssertion,
    )
    .unwrap();
    node.publish_edge(
        &alice.id,
        counter.id.clone(),
        claim.id.clone(),
        Relation::EvidenceAgainst,
        EdgeOrigin::JudgmentDerived,
    )
    .unwrap();

    let discovered = node
        .discover_objects(DiscoveryQuery {
            anchors: vec![claim.id.clone()],
            limit: 10,
            ..DiscoveryQuery::default()
        })
        .unwrap();
    let claim_candidate = discovered
        .ranked
        .iter()
        .find(|ranked| ranked.candidate.object_id == claim.id)
        .unwrap();

    assert_eq!(
        claim_candidate.candidate.signals.evidence.human_support,
        1.0
    );
    assert_eq!(
        claim_candidate
            .candidate
            .signals
            .evidence
            .judgment_contradiction,
        1.0
    );
    assert_eq!(
        claim_candidate
            .candidate
            .signals
            .evidence
            .human_contradiction,
        0.0
    );
    assert!(
        claim_candidate
            .candidate
            .sources
            .iter()
            .any(|source| source.source == CandidateSource::Temporal)
    );
    assert!(
        claim_candidate
            .candidate
            .signals
            .reputation
            .evidence_quality
            > 0.0
    );
    assert!(claim_candidate.candidate.signals.reputation.moderation > 0.5);
    assert_ne!(
        claim_candidate
            .candidate
            .signals
            .reputation
            .research_score(),
        0.5
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_discovery_uses_semantic_novelty_and_exploration_signals() {
    let root = unique_root("local-node-discovery-signals");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();

    let prolific = node
        .create_identity(IdentityKind::Person, "prolific")
        .unwrap();
    let rare = node.create_identity(IdentityKind::Person, "rare").unwrap();
    let common = node
        .publish_text(
            &prolific.id,
            "Routine discussion object in a saturated author stream.",
        )
        .unwrap();
    for index in 0..4 {
        node.publish_text(
            &prolific.id,
            &format!("Additional routine discussion object number {index}."),
        )
        .unwrap();
    }
    let unusual = node
        .publish_text(
            &rare.id,
            "Strange adjacent idea for deliberate exploration outside the saturated author stream.",
        )
        .unwrap();

    let discovered = node
        .discover_objects(DiscoveryQuery {
            limit: 20,
            exploration_slots: 10,
            ..DiscoveryQuery::default()
        })
        .unwrap();
    let common_candidate = discovered
        .ranked
        .iter()
        .find(|ranked| ranked.candidate.object_id == common.id)
        .unwrap();
    let unusual_candidate = discovered
        .ranked
        .iter()
        .find(|ranked| ranked.candidate.object_id == unusual.id)
        .unwrap();

    assert!(
        unusual_candidate.candidate.signals.novelty > common_candidate.candidate.signals.novelty,
        "rare-author Object should carry more novelty than saturated-author Objects"
    );
    assert!(
        unusual_candidate.candidate.signals.exploration
            > common_candidate.candidate.signals.exploration,
        "exploration should be driven by semantic novelty and relevance, not object-id hashing"
    );
    assert!(
        unusual_candidate
            .candidate
            .sources
            .iter()
            .any(|source| source.source == CandidateSource::Exploration)
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_rebuilds_indexes_from_file_store() {
    let root = unique_root("local-node-reload");
    let (alice_id, claim_id, edge_id, judgment_id) = {
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
        let claim = node
            .publish_text(
                &alice.id,
                "According to the dataset, durable nodes reload signed Objects.",
            )
            .unwrap();
        let evidence = node
            .publish_text(&alice.id, "The source methodology is persisted on disk.")
            .unwrap();
        let edge = node
            .publish_edge(
                &alice.id,
                evidence.id.clone(),
                claim.id.clone(),
                Relation::EvidenceFor,
                EdgeOrigin::HumanAssertion,
            )
            .unwrap();
        let judgment = node
            .judge_object(
                &claim.id,
                DefinitionId::evidence_quality_v1(),
                BTreeMap::new(),
            )
            .unwrap();
        (alice.id, claim.id, edge.id, judgment.id)
    };

    let reloaded = LocalNode::open(&root, LocalProvider::default()).unwrap();

    assert!(reloaded.identity(&alice_id).is_some());
    assert!(reloaded.object(&claim_id).is_some());
    assert!(reloaded.edge(&edge_id).is_some());
    assert!(reloaded.judgment(&judgment_id).unwrap().is_some());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_imports_signed_event_bundle_and_deduplicates_replay() {
    let root = unique_root("local-node-import");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let (identity, object, evidence, edge, identity_event, object_event, edge_event) =
        signed_bundle_records();
    let bundle = ImportBundle {
        identities: vec![identity.clone()],
        objects: vec![object.clone(), evidence],
        edges: vec![edge.clone()],
        events: vec![
            identity_event.clone(),
            object_event.clone(),
            edge_event.clone(),
        ],
    };

    let first = node.import_bundle(bundle.clone()).unwrap();
    let second = node.import_bundle(bundle).unwrap();

    assert_eq!(first.identities, 1);
    assert_eq!(first.objects, 2);
    assert_eq!(first.edges, 1);
    assert_eq!(first.events, 3);
    assert_eq!(second.duplicate_events, 3);
    assert_eq!(node.object(&object.id).unwrap(), &object);
    assert_eq!(node.edge(&edge.id).unwrap(), &edge);
    assert!(node.event(&edge_event.id).is_some());
    assert_eq!(
        node.store().get_event(&edge_event.id).unwrap().unwrap(),
        edge_event
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_rotates_identity_key_and_replays_signed_records() {
    let source_root = unique_root("local-node-key-rotation-source");
    let target_root = unique_root("local-node-key-rotation-target");
    let (alice_id, first_id, second_id, second_event_id, transition_event_id) = {
        let mut source = LocalNode::open(&source_root, LocalProvider::default()).unwrap();
        let alice = source
            .create_identity(IdentityKind::Person, "alice")
            .unwrap();
        let first = source
            .publish_text(&alice.id, "Root key signed the first durable Object.")
            .unwrap();
        let root_key = source.signing_identity(&alice.id).unwrap().public_key;
        let (transition, transition_event) = source
            .rotate_identity_key(
                &alice.id,
                IdentityKeyScope::Device,
                None,
                "replace local device signing key",
            )
            .unwrap();
        let rotated_key = source.signing_identity(&alice.id).unwrap().public_key;
        assert_ne!(root_key, rotated_key);
        assert_eq!(transition.previous_public_key, root_key);
        assert_eq!(transition.next_public_key, rotated_key);
        assert_eq!(source.identity_key_transitions(&alice.id), vec![transition]);

        let second = source
            .publish_text(&alice.id, "Rotated key signed the second durable Object.")
            .unwrap();
        assert!(source.object(&first.id).is_some());
        assert!(source.object(&second.id).is_some());

        let second_event_id = source
            .store()
            .list_events()
            .unwrap()
            .into_iter()
            .find(|event| {
                event.kind == EventKind::ObjectPublished
                    && event.target == EventTarget::Object(second.id.clone())
            })
            .unwrap()
            .id;

        (
            alice.id,
            first.id,
            second.id,
            second_event_id,
            transition_event.id,
        )
    };

    let reloaded = LocalNode::open(&source_root, LocalProvider::default()).unwrap();
    assert!(reloaded.object(&first_id).is_some());
    assert!(reloaded.object(&second_id).is_some());
    assert!(reloaded.event(&transition_event_id).is_some());
    assert_eq!(reloaded.identity_key_transitions(&alice_id).len(), 1);

    let source = LocalNode::open(&source_root, LocalProvider::default()).unwrap();
    let bundle = source
        .event_bundle(&BTreeSet::from([second_event_id]))
        .unwrap();
    assert!(
        bundle
            .events
            .iter()
            .any(|event| event.kind == EventKind::IdentityKeyTransition)
    );
    let mut target = LocalNode::open(&target_root, LocalProvider::default()).unwrap();
    let report = target.import_bundle(bundle).unwrap();
    assert!(report.events >= 2);
    assert!(target.object(&second_id).is_some());
    assert_eq!(target.identity_key_transitions(&alice_id).len(), 1);

    fs::remove_dir_all(source_root).unwrap();
    fs::remove_dir_all(target_root).unwrap();
}

#[test]
fn local_node_forks_and_remixes_objects_with_signed_lineage() {
    let source_root = unique_root("local-node-provenance-source");
    let target_root = unique_root("local-node-provenance-target");
    let (fork_event_id, remix_event_id, fork_id, remix_id, source_ids) = {
        let mut node = LocalNode::open(&source_root, LocalProvider::default()).unwrap();
        let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
        let first = node
            .publish_text(&alice.id, "Original composable Object")
            .unwrap();
        let second = node
            .publish_text(&alice.id, "Second Object with compatible context")
            .unwrap();

        let fork = node
            .fork_object(
                &alice.id,
                &first.id,
                ObjectDraft::text("Forked Object with preserved lineage").unwrap(),
            )
            .unwrap();
        assert_eq!(fork.event.kind, EventKind::ObjectForked);
        assert_eq!(fork.object.provenance.parent, Some(first.id.clone()));
        assert_eq!(fork.object.provenance.forked_from, Some(first.id.clone()));
        assert_eq!(fork.object.provenance.remixed_from, Vec::new());
        assert_eq!(fork.edges.len(), 1);
        assert_eq!(fork.edges[0].source, fork.object.id);
        assert_eq!(fork.edges[0].target, first.id);
        assert_eq!(fork.edges[0].relation, Relation::Forks);

        let remix = node
            .remix_object(
                &alice.id,
                vec![first.id.clone(), second.id.clone()],
                ObjectDraft::text("Remixed Object joining two sources").unwrap(),
            )
            .unwrap();
        assert_eq!(remix.event.kind, EventKind::ObjectRemixed);
        assert_eq!(
            remix.object.provenance.remixed_from,
            vec![first.id.clone(), second.id.clone()]
        );
        assert_eq!(remix.edges.len(), 2);
        assert!(
            remix
                .edges
                .iter()
                .all(|edge| edge.source == remix.object.id && edge.relation == Relation::Remixes)
        );

        (
            fork.event.id,
            remix.event.id,
            fork.object.id,
            remix.object.id,
            vec![first.id, second.id],
        )
    };

    let reloaded = LocalNode::open(&source_root, LocalProvider::default()).unwrap();
    assert_eq!(
        reloaded.object(&fork_id).unwrap().provenance.forked_from,
        Some(source_ids[0].clone())
    );
    assert_eq!(
        reloaded.object(&remix_id).unwrap().provenance.remixed_from,
        source_ids
    );

    let source = LocalNode::open(&source_root, LocalProvider::default()).unwrap();
    let bundle = source
        .event_bundle(&BTreeSet::from([fork_event_id, remix_event_id]))
        .unwrap();
    assert!(
        bundle
            .events
            .iter()
            .any(|event| event.kind == EventKind::ObjectForked)
    );
    assert!(
        bundle
            .events
            .iter()
            .any(|event| event.kind == EventKind::ObjectRemixed)
    );
    let mut target = LocalNode::open(&target_root, LocalProvider::default()).unwrap();
    let report = target.import_bundle(bundle).unwrap();
    assert!(report.objects >= 4);
    assert!(target.object(&fork_id).is_some());
    assert!(target.object(&remix_id).is_some());

    fs::remove_dir_all(source_root).unwrap();
    fs::remove_dir_all(target_root).unwrap();
}

#[test]
fn local_node_rejects_import_event_with_missing_parent() {
    let root = unique_root("local-node-missing-parent");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let (identity, object, _evidence, _edge, _identity_event, object_event, _edge_event) =
        signed_bundle_records();

    let rejected = node.import_bundle(ImportBundle {
        identities: vec![identity],
        objects: vec![object],
        edges: Vec::new(),
        events: vec![object_event],
    });

    assert!(rejected.is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_publishes_signed_finality_checkpoint() {
    let root = unique_root("local-node-checkpoint");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let validators = install_validator_mesh(&mut node);
    let validator_set = ValidatorSet::equal(
        validators
            .iter()
            .map(|validator| validator.identity.id.clone()),
    )
    .unwrap();
    let author = &validators[0].identity;

    let checkpoint = node.publish_checkpoint(&author.id, validator_set).unwrap();
    let stored = node.store().get_event(&checkpoint.id).unwrap().unwrap();
    let payload: babble_hashgraph::FinalityCheckpoint =
        serde_json::from_value(checkpoint.payload.clone()).unwrap();

    assert_eq!(checkpoint.kind, EventKind::ConsensusCheckpoint);
    assert_eq!(checkpoint.target, EventTarget::Network);
    assert_eq!(
        checkpoint.parents,
        vec![payload.last_finalized_event.clone()]
    );
    assert_eq!(stored, checkpoint);
    assert!(node.event(&payload.last_finalized_event).is_some());
    checkpoint.verify(author).unwrap();

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn signed_checkpoint_imports_through_event_bundle() {
    let source_root = unique_root("local-node-checkpoint-source");
    let target_root = unique_root("local-node-checkpoint-target");
    let mut source = LocalNode::open(&source_root, LocalProvider::default()).unwrap();
    let mut target = LocalNode::open(&target_root, LocalProvider::default()).unwrap();
    let validators = install_validator_mesh(&mut source);
    let validator_set = ValidatorSet::equal(
        validators
            .iter()
            .map(|validator| validator.identity.id.clone()),
    )
    .unwrap();

    let checkpoint = source
        .publish_checkpoint(&validators[0].identity.id, validator_set)
        .unwrap();
    let bundle = source
        .event_bundle(&BTreeSet::from([checkpoint.id.clone()]))
        .unwrap();
    let report = target.import_bundle(bundle).unwrap();

    assert!(report.events > 0);
    assert!(target.event(&checkpoint.id).is_some());

    fs::remove_dir_all(source_root).unwrap();
    fs::remove_dir_all(target_root).unwrap();
}

#[test]
fn local_node_persists_capability_grants_and_prepares_surfaces() {
    let root = unique_root("local-node-capabilities");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
    node.import_signing_identity(identity.clone(), keypair.clone())
        .unwrap();
    let bundle_hash = Hash::from_bytes(b"export default function surface() {}");
    let capability = CapabilityRequest {
        id: "babble.network.fetch".to_string(),
        version: 1,
        scope: json!({"origins": ["https://example.com"]}),
    };
    let object = Object::text(&identity, "Executable Babble Object")
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

    let pending = node.prepare_surface(&object.id, SurfaceRole::Feed).unwrap();
    assert_eq!(pending.admission, RuntimeAdmissionStatus::NeedsPermission);
    assert_eq!(pending.capability_decisions.len(), 1);

    let grant_event = node
        .grant_capability(
            &identity.id,
            &object.id,
            capability,
            GrantDecision::Approved,
        )
        .unwrap();
    assert_eq!(grant_event.kind, EventKind::CapabilityGranted);
    let ready = node.prepare_surface(&object.id, SurfaceRole::Feed).unwrap();
    assert_eq!(ready.admission, RuntimeAdmissionStatus::Ready);
    let grant_id = node.capability_grants(&object.id).unwrap()[0].id.clone();
    let receipt = node
        .authorize_capability_binding_with_usage(
            &object.id,
            "babble.network.fetch",
            1,
            &[grant_id.to_string()],
            CapabilityBindingUsage {
                requested_bytes: 4096,
                realtime_connections: 0,
                windows: Vec::new(),
            },
        )
        .unwrap();
    assert_eq!(receipt.remaining_calls_per_minute, 59);
    assert_eq!(receipt.remaining_bytes_per_minute, (2 * 1024 * 1024) - 4096);
    let exhausted = node.authorize_capability_binding_with_usage(
        &object.id,
        "babble.network.fetch",
        1,
        &[grant_id.to_string()],
        CapabilityBindingUsage {
            requested_bytes: 1,
            realtime_connections: 0,
            windows: vec![CapabilityUsageWindow {
                grant_id: grant_id.clone(),
                window_started_at: babble_types::Timestamp::now(),
                calls: 60,
                bytes: 0,
                realtime_connections: 0,
            }],
        },
    );
    assert!(exhausted.is_err());

    let revoked = node
        .revoke_capability(&identity.id, &object.id, &grant_id)
        .unwrap();
    assert_eq!(revoked.kind, EventKind::CapabilityRevoked);
    let grants = node.capability_grants(&object.id).unwrap();
    assert!(grants[0].revoked_at.is_some());
    let pending_again = node.prepare_surface(&object.id, SurfaceRole::Feed).unwrap();
    assert_eq!(
        pending_again.admission,
        RuntimeAdmissionStatus::NeedsPermission
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_enforces_capability_backed_object_storage() {
    let root = unique_root("local-node-object-storage");
    let object_id;
    let second_object_id;
    let grant_id;
    {
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
        node.import_signing_identity(identity.clone(), keypair.clone())
            .unwrap();
        let capability = CapabilityRequest {
            id: "babble.storage.object".to_string(),
            version: 1,
            scope: json!({"namespace": "self"}),
        };
        let object = Object::text(&identity, "Object-owned persistent state")
            .unwrap()
            .with_capabilities(vec![capability.clone()])
            .unwrap()
            .sign(&identity, &keypair)
            .unwrap();
        let object = node.publish_object_record(&identity.id, object).unwrap();
        let second = node
            .publish_text(&identity.id, "Object without the storage grant")
            .unwrap();

        node.grant_capability(
            &identity.id,
            &object.id,
            capability,
            GrantDecision::Approved,
        )
        .unwrap();
        grant_id = node.capability_grants(&object.id).unwrap()[0]
            .id
            .to_string();
        object_id = object.id.clone();
        second_object_id = second.id.clone();

        let missing_grant =
            node.object_storage_set(&object.id, "settings/theme", json!({"mode": "dark"}), &[]);
        assert!(missing_grant.is_err());

        let cross_object_grant = node.object_storage_set(
            &second.id,
            "settings/theme",
            json!({"mode": "dark"}),
            &[grant_id.to_string()],
        );
        assert!(cross_object_grant.is_err());

        let (stored, receipt) = node
            .object_storage_set(
                &object.id,
                "settings/theme",
                json!({"mode": "dark"}),
                &[grant_id.to_string()],
            )
            .unwrap();
        assert_eq!(stored.value, json!({"mode": "dark"}));
        assert_eq!(receipt.capability.as_str(), "babble.storage.object");
        assert_eq!(receipt.remaining_calls_per_minute, 119);

        node.object_storage_set(
            &object.id,
            "settings/name",
            json!("alice"),
            &[grant_id.to_string()],
        )
        .unwrap();
        let (entries, _) = node
            .object_storage_list(&object.id, Some("settings/"), 16, &[grant_id.to_string()])
            .unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.key.as_str())
                .collect::<Vec<_>>(),
            vec!["settings/name", "settings/theme"]
        );

        let oversized = "x".repeat(128 * 1024);
        let rejected = node.object_storage_set(
            &object.id,
            "settings/oversized",
            json!(oversized),
            &[grant_id.to_string()],
        );
        assert!(rejected.is_err());
    }

    let node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let (persisted, _) = node
        .object_storage_get(&object_id, "settings/theme", &[grant_id.to_string()])
        .unwrap();
    assert_eq!(persisted.unwrap().value, json!({"mode": "dark"}));
    assert!(
        node.object_storage_get(&second_object_id, "settings/theme", &[grant_id.to_string()])
            .is_err()
    );
    let (deleted, _) = node
        .object_storage_delete(&object_id, "settings/theme", &[grant_id])
        .unwrap();
    assert_eq!(deleted.unwrap().value, json!({"mode": "dark"}));
    assert!(
        node.object_storage_get(&object_id, "settings/theme", &[])
            .is_err()
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_enforces_capability_bound_local_storage() {
    let root = unique_root("local-node-local-storage");
    let object_id;
    let alice_id;
    let bob_id;
    let grant_id;
    {
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
        let bob = node.create_identity(IdentityKind::Person, "bob").unwrap();
        let capability = CapabilityRequest {
            id: "babble.storage.local".to_string(),
            version: 1,
            scope: json!({"namespace": "prefs"}),
        };
        let object = ObjectDraft::text("Local state Object")
            .unwrap()
            .with_capability(capability.clone())
            .unwrap();
        let object = node.publish_draft(&alice.id, object).unwrap();

        let missing_grant = node.local_storage_set(
            &object.id,
            &alice.id,
            "settings/theme",
            json!({"mode": "dark"}),
            &[],
        );
        assert!(missing_grant.is_err());

        node.grant_capability(&alice.id, &object.id, capability, GrantDecision::Approved)
            .unwrap();
        grant_id = node.capability_grants(&object.id).unwrap()[0]
            .id
            .to_string();

        let (stored, receipt) = node
            .local_storage_set(
                &object.id,
                &alice.id,
                "settings/theme",
                json!({"mode": "dark"}),
                &[grant_id.clone()],
            )
            .unwrap();
        assert_eq!(stored.identity_id, alice.id);
        assert_eq!(stored.namespace, "prefs");
        assert_eq!(stored.value, json!({"mode": "dark"}));
        assert_eq!(receipt.capability.as_str(), "babble.storage.local");

        let (fetched, _) = node
            .local_storage_get(&object.id, &alice.id, "settings/theme", &[grant_id.clone()])
            .unwrap();
        assert_eq!(fetched.unwrap().value, json!({"mode": "dark"}));

        node.local_storage_set(
            &object.id,
            &alice.id,
            "settings/name",
            json!("alice"),
            &[grant_id.clone()],
        )
        .unwrap();
        let (entries, _) = node
            .local_storage_list(
                &object.id,
                &alice.id,
                Some("settings/"),
                16,
                &[grant_id.clone()],
            )
            .unwrap();
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.key.as_str())
                .collect::<Vec<_>>(),
            vec!["settings/name", "settings/theme"]
        );

        node.local_storage_set(
            &object.id,
            &bob.id,
            "settings/theme",
            json!({"mode": "light"}),
            &[grant_id.clone()],
        )
        .unwrap();
        let (alice_theme, _) = node
            .local_storage_get(&object.id, &alice.id, "settings/theme", &[grant_id.clone()])
            .unwrap();
        let (bob_theme, _) = node
            .local_storage_get(&object.id, &bob.id, "settings/theme", &[grant_id.clone()])
            .unwrap();
        assert_eq!(alice_theme.unwrap().value, json!({"mode": "dark"}));
        assert_eq!(bob_theme.unwrap().value, json!({"mode": "light"}));

        let (deleted, _) = node
            .local_storage_delete(&object.id, &alice.id, "settings/theme", &[grant_id.clone()])
            .unwrap();
        assert_eq!(deleted.unwrap().value, json!({"mode": "dark"}));
        let (deleted_alice_theme, _) = node
            .local_storage_get(&object.id, &alice.id, "settings/theme", &[grant_id.clone()])
            .unwrap();
        assert!(deleted_alice_theme.is_none());

        object_id = object.id;
        alice_id = alice.id;
        bob_id = bob.id;
    }

    let node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let (alice_theme, _) = node
        .local_storage_get(&object_id, &alice_id, "settings/theme", &[grant_id.clone()])
        .unwrap();
    assert!(alice_theme.is_none());
    let (bob_theme, _) = node
        .local_storage_get(&object_id, &bob_id, "settings/theme", &[grant_id])
        .unwrap();
    assert_eq!(bob_theme.unwrap().value, json!({"mode": "light"}));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_enforces_capability_bound_current_identity() {
    let root = unique_root("local-node-identity-current");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
    let capability = CapabilityRequest {
        id: "babble.identity.current".to_string(),
        version: 1,
        scope: json!({}),
    };
    let object = ObjectDraft::text("Identity-aware Object")
        .unwrap()
        .with_capability(capability.clone())
        .unwrap();
    let object = node.publish_draft(&alice.id, object).unwrap();

    let missing_grant = node.identity_current(&object.id, &alice.id, &[]);
    assert!(missing_grant.is_err());

    node.grant_capability(&alice.id, &object.id, capability, GrantDecision::Approved)
        .unwrap();
    let grant_id = node.capability_grants(&object.id).unwrap()[0]
        .id
        .to_string();
    let (identity, receipt) = node
        .identity_current(&object.id, &alice.id, &[grant_id])
        .unwrap();
    assert_eq!(identity.id, alice.id);
    assert_eq!(identity.handle, "alice");
    assert_eq!(receipt.capability.as_str(), "babble.identity.current");

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_enforces_capability_bound_network_fetch() {
    let root = unique_root("local-node-network-fetch");
    let (url, server) = spawn_http_response("hello babble");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
    node.import_signing_identity(identity.clone(), keypair.clone())
        .unwrap();
    let origin = url.rsplit_once('/').unwrap().0.to_string();
    let capability = CapabilityRequest {
        id: "babble.network.fetch".to_string(),
        version: 1,
        scope: json!({"origins": [origin]}),
    };
    let object = Object::text(&identity, "Network Object")
        .unwrap()
        .with_capabilities(vec![capability.clone()])
        .unwrap()
        .sign(&identity, &keypair)
        .unwrap();
    let object = node.publish_object_record(&identity.id, object).unwrap();
    node.grant_capability(
        &identity.id,
        &object.id,
        capability,
        GrantDecision::Approved,
    )
    .unwrap();
    let grant_id = node.capability_grants(&object.id).unwrap()[0]
        .id
        .to_string();

    let missing_grant = node.network_fetch(&object.id, "GET", &url, BTreeMap::new(), None, &[]);
    assert!(missing_grant.is_err());
    let disallowed_origin = node.network_fetch(
        &object.id,
        "GET",
        "https://example.com/resource",
        BTreeMap::new(),
        None,
        &[grant_id.clone()],
    );
    assert!(disallowed_origin.is_err());
    let forbidden_header = node.network_fetch(
        &object.id,
        "GET",
        &url,
        BTreeMap::from([("Cookie".to_string(), "session=secret".to_string())]),
        None,
        &[grant_id.clone()],
    );
    assert!(forbidden_header.is_err());

    let fetched = node
        .network_fetch(
            &object.id,
            "GET",
            &url,
            BTreeMap::from([("X-Babble-Test".to_string(), "request".to_string())]),
            None,
            &[grant_id],
        )
        .unwrap();
    assert_eq!(fetched.status, 200);
    assert_eq!(fetched.body, b"hello babble");
    assert_eq!(fetched.headers.get("x-babble-test"), Some(&"ok".to_string()));
    assert!(!fetched.headers.contains_key("set-cookie"));
    assert_eq!(fetched.receipt.capability.as_str(), "babble.network.fetch");
    server.join().unwrap();

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_commits_realtime_messages_closes_sessions_and_rebuilds_room_state() {
    let root = unique_root("local-node-realtime");
    let (room_id, message_id, session_id) = {
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
        let object = node
            .publish_text(&alice.id, "Collaborative canvas Object")
            .unwrap();
        let room = RoomSpec::new(
            object.id.clone(),
            "canvas-main",
            "babble.realtime.state.v1",
            MembershipPolicy::Open,
            PersistencePolicy::DurableMessages,
            RoomLimits::default(),
        )
        .unwrap();
        let room_id = room.id.clone();
        let event = node.define_realtime_room(&alice.id, room).unwrap();
        assert_eq!(event.kind, EventKind::RealtimeRoomDefined);

        let session = node.start_realtime_session(&alice.id, &room_id).unwrap();
        let (message, snapshot) = node
            .publish_realtime_message(
                &alice.id,
                &session.id,
                &object.id,
                RealtimePayload::State(RealtimeOperation::IncrementCounter {
                    key: "brush_strokes".to_string(),
                    by: 3,
                }),
                true,
            )
            .unwrap();
        assert!(snapshot.is_none());

        let view = node.realtime_room(&room_id).unwrap();
        assert_eq!(
            view.presence.participants,
            BTreeSet::from([alice.id.clone()])
        );
        assert_eq!(view.state.counters.get("brush_strokes"), Some(&3));
        assert_eq!(view.durable_messages, vec![message.clone()]);
        let (closed, close_event) = node
            .close_realtime_session(&alice.id, &session.id, &object.id)
            .unwrap();
        assert_eq!(closed.state, SessionState::Closed);
        assert_eq!(close_event.kind, EventKind::RealtimeSessionClosed);
        assert!(
            node.publish_realtime_message(
                &alice.id,
                &session.id,
                &object.id,
                RealtimePayload::State(RealtimeOperation::IncrementCounter {
                    key: "brush_strokes".to_string(),
                    by: 1,
                }),
                true,
            )
            .is_err()
        );
        (room_id, message.id, session.id)
    };

    let reopened = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let view = reopened.realtime_room(&room_id).unwrap();
    assert_eq!(view.state.counters.get("brush_strokes"), Some(&3));
    assert_eq!(view.durable_messages[0].id, message_id);
    assert!(!view.presence.active_sessions.contains(&session_id));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_enforces_capability_bound_realtime_calls() {
    let root = unique_root("local-node-realtime-capability");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
    let join = CapabilityRequest {
        id: "babble.realtime.join".to_string(),
        version: 1,
        scope: json!({"room": "main"}),
    };
    let send = CapabilityRequest {
        id: "babble.realtime.send".to_string(),
        version: 1,
        scope: json!({"room": "main"}),
    };
    let leave = CapabilityRequest {
        id: "babble.realtime.leave".to_string(),
        version: 1,
        scope: json!({"room": "main"}),
    };
    let wrong_join = CapabilityRequest {
        id: "babble.realtime.join".to_string(),
        version: 1,
        scope: json!({"room": "side"}),
    };
    let object = ObjectDraft::text("Realtime Object")
        .unwrap()
        .with_capability(join.clone())
        .unwrap()
        .with_capability(send.clone())
        .unwrap()
        .with_capability(leave.clone())
        .unwrap()
        .with_capability(wrong_join.clone())
        .unwrap();
    let object = node.publish_draft(&alice.id, object).unwrap();
    let room = RoomSpec::new(
        object.id.clone(),
        "main",
        "babble.realtime.state.v1",
        MembershipPolicy::Open,
        PersistencePolicy::DurableMessages,
        RoomLimits::default(),
    )
    .unwrap();
    let room_id = room.id.clone();
    node.define_realtime_room(&alice.id, room).unwrap();

    let missing_grant = node.start_realtime_session_with_capability(&alice.id, &room_id, &[]);
    assert!(missing_grant.is_err());

    node.grant_capability(&alice.id, &object.id, wrong_join, GrantDecision::Approved)
        .unwrap();
    let wrong_join_grant = node.capability_grants(&object.id).unwrap()[0]
        .id
        .to_string();
    let wrong_room =
        node.start_realtime_session_with_capability(&alice.id, &room_id, &[wrong_join_grant]);
    assert!(wrong_room.is_err());

    node.grant_capability(&alice.id, &object.id, join, GrantDecision::Approved)
        .unwrap();
    node.grant_capability(&alice.id, &object.id, send, GrantDecision::Approved)
        .unwrap();
    node.grant_capability(&alice.id, &object.id, leave, GrantDecision::Approved)
        .unwrap();
    let grants = node.capability_grants(&object.id).unwrap();
    let join_grant = grants
        .iter()
        .find(|grant| {
            grant.capability.as_str() == "babble.realtime.join"
                && grant.scope == json!({"room": "main"})
        })
        .unwrap()
        .id
        .to_string();
    let send_grant = grants
        .iter()
        .find(|grant| grant.capability.as_str() == "babble.realtime.send")
        .unwrap()
        .id
        .to_string();
    let leave_grant = grants
        .iter()
        .find(|grant| grant.capability.as_str() == "babble.realtime.leave")
        .unwrap()
        .id
        .to_string();

    let (session, join_receipt) = node
        .start_realtime_session_with_capability(&alice.id, &room_id, &[join_grant])
        .unwrap();
    assert_eq!(join_receipt.capability.as_str(), "babble.realtime.join");
    assert_eq!(join_receipt.scope, json!({"room": "main"}));
    assert_eq!(join_receipt.remaining_realtime_connections, 1);

    let (message, snapshot, send_receipt) = node
        .publish_realtime_message_with_capability(
            &alice.id,
            &session.id,
            &object.id,
            RealtimePayload::State(RealtimeOperation::IncrementCounter {
                key: "brush_strokes".to_string(),
                by: 2,
            }),
            true,
            &[send_grant],
        )
        .unwrap();
    assert_eq!(message.sequence, 1);
    assert!(snapshot.is_none());
    assert_eq!(send_receipt.capability.as_str(), "babble.realtime.send");
    assert!(send_receipt.remaining_bytes_per_minute < send_receipt.quota.bytes_per_minute);

    let (closed, _, leave_receipt) = node
        .close_realtime_session_with_capability(&alice.id, &session.id, &object.id, &[leave_grant])
        .unwrap();
    assert_eq!(closed.state, SessionState::Closed);
    assert_eq!(leave_receipt.capability.as_str(), "babble.realtime.leave");

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_enforces_capability_bound_social_actions() {
    let root = unique_root("local-node-social-capability");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
    let target = node.publish_text(&alice.id, "Target Object").unwrap();
    let follow = CapabilityRequest {
        id: "babble.social.follow".to_string(),
        version: 1,
        scope: json!({"object_id": target.id}),
    };
    let unfollow = CapabilityRequest {
        id: "babble.social.unfollow".to_string(),
        version: 1,
        scope: json!({"object_id": target.id}),
    };
    let reply = CapabilityRequest {
        id: "babble.social.reply".to_string(),
        version: 1,
        scope: json!({"object_id": target.id}),
    };
    let share = CapabilityRequest {
        id: "babble.social.share".to_string(),
        version: 1,
        scope: json!({"object_id": target.id}),
    };
    let caller = ObjectDraft::text("Social app Object")
        .unwrap()
        .with_capability(follow.clone())
        .unwrap()
        .with_capability(unfollow.clone())
        .unwrap()
        .with_capability(reply.clone())
        .unwrap()
        .with_capability(share.clone())
        .unwrap();
    let caller = node.publish_draft(&alice.id, caller).unwrap();

    let missing_grant = node.social_follow(&alice.id, &caller.id, &target.id, &[]);
    assert!(missing_grant.is_err());

    for capability in [follow, unfollow, reply, share] {
        assert!(node.grant_capability(&alice.id, &caller.id, capability, GrantDecision::Approved).is_err());
    }
    assert!(node.capability_grants(&caller.id).unwrap().is_empty());

    let unscoped = CapabilityRequest {
        id: "babble.social.follow".to_string(),
        version: 1,
        scope: json!({}),
    };
    let unscoped_caller = ObjectDraft::text("Unscoped social Object")
        .unwrap()
        .with_capability(unscoped.clone())
        .unwrap();
    let unscoped_caller = node.publish_draft(&alice.id, unscoped_caller).unwrap();
    use babble_capabilities::invocation::{InvocationContext, InvocationOrigin};
    use babble_types::{Canonical, Timestamp};
    let context = |node: &LocalNode<LocalProvider>, object: &babble_object::Object| InvocationContext {
        actor: alice.id.clone(), login_id: "origin-login".into(), object_id: object.id.clone(),
        object_version: object.canonical_hash().unwrap(),
        origin: InvocationOrigin::HostAction { document_id: "host-document".into() },
        policy_revision: node.invocation_policy_revision().unwrap(), context_epoch: node.invocation_epoch(),
    };
    let deadline = Timestamp(Timestamp::now().0 + time::Duration::seconds(60));
    assert!(node.prepare_social_invocation(context(&node, &unscoped_caller), "unscoped", "babble.social.follow",
        babble_node::SocialInvocationPayload { target_object_id: target.id.clone(), text: None, media: None }, deadline).is_err());
    for (action, relation) in [("follow", Relation::Follows), ("unfollow", Relation::Custom("unfollows".into())),
        ("reply", Relation::ReplyTo), ("share", Relation::Quotes)] {
        let context = context(&node, &caller);
        let record = node.prepare_social_invocation(context.clone(), action, &format!("babble.social.{action}"),
            babble_node::SocialInvocationPayload { target_object_id: target.id.clone(),
                text: matches!(action, "reply" | "share").then(|| "Signed content".into()), media: None }, deadline).unwrap();
        node.decide_social_invocation(&context, action, record.id(), true).unwrap();
        let result = node.execute_social_invocation(&context, action, record.id()).unwrap();
        assert_eq!(result.edge.target, target.id);
        assert_eq!(result.edge.relation, relation);
        assert_eq!(result.edge.source, result.object.as_ref().map_or(&caller.id, |o| &o.id).clone());
        assert_eq!(result.receipt.request.fingerprint, record.intent().fingerprint().unwrap());
        if action == "unfollow" { assert_eq!(result.edge.metadata.get("removes_relation"), Some(&json!("follows"))); }
    }

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_commits_realtime_snapshots_and_rebuilds_snapshot_state() {
    let root = unique_root("local-node-realtime-snapshot");
    let (room_id, snapshot_id) = {
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
        let object = node
            .publish_text(&alice.id, "Realtime world Object")
            .unwrap();
        let room = RoomSpec::new(
            object.id.clone(),
            "world-main",
            "babble.realtime.state.v1",
            MembershipPolicy::Open,
            PersistencePolicy::SnapshotEvery { messages: 2 },
            RoomLimits::default(),
        )
        .unwrap();
        let room_id = room.id.clone();
        node.define_realtime_room(&alice.id, room).unwrap();

        let session = node.start_realtime_session(&alice.id, &room_id).unwrap();
        let (_, first_snapshot) = node
            .publish_realtime_message(
                &alice.id,
                &session.id,
                &object.id,
                RealtimePayload::State(RealtimeOperation::IncrementCounter {
                    key: "ticks".to_string(),
                    by: 1,
                }),
                false,
            )
            .unwrap();
        assert!(first_snapshot.is_none());
        let (_, second_snapshot) = node
            .publish_realtime_message(
                &alice.id,
                &session.id,
                &object.id,
                RealtimePayload::State(RealtimeOperation::IncrementCounter {
                    key: "ticks".to_string(),
                    by: 2,
                }),
                false,
            )
            .unwrap();
        let snapshot = second_snapshot.expect("second message should create snapshot");
        assert_eq!(snapshot.state.counters.get("ticks"), Some(&3));

        let view = node.realtime_room(&room_id).unwrap();
        assert!(view.durable_messages.is_empty());
        assert_eq!(view.snapshots, vec![snapshot.clone()]);
        (room_id, snapshot.id)
    };

    let reopened = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let view = reopened.realtime_room(&room_id).unwrap();
    assert_eq!(view.state.counters.get("ticks"), Some(&3));
    assert_eq!(view.snapshots[0].id, snapshot_id);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_publishes_media_objects_from_content_addressed_blobs() {
    let root = unique_root("local-node-media");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
    let blob = node.put_media_blob("Image/PNG", b"not really png").unwrap();

    let object = node
        .publish_media_object(
            &alice.id,
            "First image",
            Some("A tiny test image resource.".to_string()),
            vec![blob.clone()],
        )
        .unwrap();
    let (loaded, bytes) = node
        .media_blob(&blob.integrity, &blob.media_type)
        .unwrap()
        .unwrap();

    assert_eq!(loaded, blob);
    assert_eq!(bytes, b"not really png");
    assert_eq!(object.kind.as_str(), "babble.media");
    assert_eq!(object.resources, vec![blob.resource()]);
    assert!(node.object(&object.id).is_some());

    let missing = MediaBlob::from_hash(
        "image/png".to_string(),
        Hash::from_bytes(b"missing resource"),
        16,
    )
    .unwrap();
    let rejected = node.publish_media_object(&alice.id, "Missing", None, vec![missing]);
    assert!(rejected.is_err());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn local_node_publishes_validated_authoring_drafts() {
    let root = unique_root("local-node-authoring");
    let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
    let alice = node.create_identity(IdentityKind::Person, "alice").unwrap();
    let blob = node
        .put_media_blob("text/javascript", b"export function mount() {}")
        .unwrap();
    let capability = CapabilityRequest {
        id: "babble.realtime.join".to_string(),
        version: 1,
        scope: json!({"room": "media-object"}),
    };
    let draft = ObjectDraft::media(
        "Executable media",
        Some("A media Object with a feed surface.".to_string()),
        vec![blob.clone()],
    )
    .unwrap()
    .with_surface(Surface {
        bundle: None,
        role: SurfaceRole::Feed,
        target: SurfaceTarget::Web,
        entry: blob.uri.clone(),
        integrity: Some(blob.integrity.clone()),
    })
    .unwrap()
    .with_capability(capability.clone())
    .unwrap();

    let object = node.publish_draft(&alice.id, draft).unwrap();
    object.verify(&alice).unwrap();
    assert_eq!(object.kind.as_str(), "babble.media");
    assert_eq!(object.resources, vec![blob.resource()]);
    assert_eq!(object.surfaces.len(), 1);
    assert_eq!(object.capabilities, vec![capability.clone()]);

    let note = node
        .publish_draft(&alice.id, ObjectDraft::text("draft-authored note").unwrap())
        .unwrap();
    let edge = node
        .publish_edge_draft(
            &alice.id,
            EdgeDraft::new(
                object.id.clone(),
                note.id.clone(),
                Relation::References,
                EdgeOrigin::ApplicationAssertion,
            )
            .unwrap(),
        )
        .unwrap();
    edge.verify(&alice).unwrap();
    assert_eq!(edge.source, object.id);
    assert_eq!(edge.target, note.id);

    let grant = node
        .grant_capability_draft(
            &alice.id,
            CapabilityGrantDraft::new(object.id.clone(), capability, GrantDecision::Approved, None)
                .unwrap(),
        )
        .unwrap();
    assert_eq!(grant.kind, EventKind::CapabilityGranted);

    let missing = ObjectDraft::media(
        "Missing blob",
        None,
        vec![
            MediaBlob::from_hash("text/javascript", Hash::from_bytes(b"missing bundle"), 16)
                .unwrap(),
        ],
    )
    .unwrap();
    assert!(node.publish_draft(&alice.id, missing).is_err());

    fs::remove_dir_all(root).unwrap();
}

fn signed_bundle_records() -> (Identity, Object, Object, Edge, Event, Event, Event) {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "remote-alice", &keypair).unwrap();
    let object = Object::text(&identity, "Remote signed Object imported through gossip.")
        .unwrap()
        .sign(&identity, &keypair)
        .unwrap();
    let evidence = Object::text(&identity, "Imported edge references known Objects.")
        .unwrap()
        .sign(&identity, &keypair)
        .unwrap();
    let edge = Edge::new(
        evidence.id.clone(),
        object.id.clone(),
        Relation::EvidenceFor,
        EdgeOrigin::HumanAssertion,
        Some(identity.id.clone()),
    )
    .unwrap()
    .sign(&identity, &keypair)
    .unwrap();
    let identity_event = Event::new(
        &identity,
        EventKind::IdentityCreated,
        EventTarget::Identity(identity.id.clone()),
        serde_json::json!({ "handle": identity.handle }),
        Vec::new(),
    )
    .unwrap()
    .sign(&identity, &keypair)
    .unwrap();
    let object_event = Event::new(
        &identity,
        EventKind::ObjectPublished,
        EventTarget::Object(object.id.clone()),
        serde_json::json!({ "kind": object.kind.as_str(), "schema": object.schema }),
        vec![identity_event.id.clone()],
    )
    .unwrap()
    .sign(&identity, &keypair)
    .unwrap();
    let edge_event = Event::new(
        &identity,
        EventKind::EdgePublished,
        EventTarget::Edge(edge.id.clone()),
        serde_json::json!({
            "source": edge.source,
            "target": edge.target,
            "relation": edge.relation,
        }),
        vec![object_event.id.clone()],
    )
    .unwrap()
    .sign(&identity, &keypair)
    .unwrap();

    (
        identity,
        object,
        evidence,
        edge,
        identity_event,
        object_event,
        edge_event,
    )
}

struct ValidatorFixture {
    identity: Identity,
    keypair: Keypair,
    identity_event: babble_types::EventId,
}

fn install_validator_mesh(node: &mut LocalNode<LocalProvider>) -> Vec<ValidatorFixture> {
    let mut validators = ["alice", "bob", "cara", "drew"]
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
        .publish_text(
            &validators[0].identity.id,
            "Consensus checkpoint anchor Object.",
        )
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

    validators.sort_by(|left, right| left.identity.id.cmp(&right.identity.id));
    validators
}

fn unique_root(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("babble-node-{name}-{nanos}"))
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
