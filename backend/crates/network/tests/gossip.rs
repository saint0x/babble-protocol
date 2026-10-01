use babel_crypto::Keypair;
use babel_hashgraph::{FinalityCheckpoint, ValidatorSet};
use babel_identity::{Identity, IdentityKind};
use babel_judgment_local::LocalProvider;
use babel_network::{Envelope, GossipEngine, Message, NetworkAction, NetworkLimits};
use babel_node::{ImportBundle, LocalNode};
use babel_state::{Event, EventKind, EventTarget};

#[test]
fn gossip_inventory_request_bundle_import_round_trip() {
    let source_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let mut source = LocalNode::open(source_dir.path(), LocalProvider::default()).unwrap();
    let mut target = LocalNode::open(target_dir.path(), LocalProvider::default()).unwrap();
    let source_peer = install_node_identity(&mut source, "source");
    let target_peer = install_node_identity(&mut target, "target");
    let object = source
        .publish_text(&source_peer.identity.id, "Gossip transports signed events.")
        .unwrap();
    let event_id = source
        .store()
        .list_events()
        .unwrap()
        .into_iter()
        .find(|event| {
            matches!(
                &event.target,
                babel_state::EventTarget::Object(object_id) if object_id == &object.id
            )
        })
        .unwrap()
        .id;
    let engine = GossipEngine::new(NetworkLimits::default());

    exchange_hellos(
        &engine,
        &source_peer,
        &mut source,
        &target_peer,
        &mut target,
    );

    let request = engine
        .receive(
            &mut target,
            Envelope::signed(
                &source_peer.identity,
                &source_peer.keypair,
                Message::Inventory {
                    events: vec![event_id.clone()],
                },
            )
            .unwrap(),
        )
        .unwrap();
    let NetworkAction::RequestEvents { events } = request else {
        panic!("target should request missing event");
    };

    let response = engine
        .receive(
            &mut source,
            Envelope::signed(
                &target_peer.identity,
                &target_peer.keypair,
                Message::RequestEvents { events },
            )
            .unwrap(),
        )
        .unwrap();
    let NetworkAction::SendBundle { bundle } = response else {
        panic!("source should return an event bundle");
    };

    let imported = engine
        .receive(
            &mut target,
            Envelope::signed(
                &source_peer.identity,
                &source_peer.keypair,
                Message::EventBundle { bundle },
            )
            .unwrap(),
        )
        .unwrap();
    let NetworkAction::Imported { report } = imported else {
        panic!("target should import event bundle");
    };

    assert_eq!(report.events, 2);
    assert!(target.object(&object.id).is_some());
    assert!(target.event(&event_id).is_some());
}

#[test]
fn gossip_object_inventory_repairs_missing_content_by_hash() {
    let source_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let mut source = LocalNode::open(source_dir.path(), LocalProvider::default()).unwrap();
    let mut target = LocalNode::open(target_dir.path(), LocalProvider::default()).unwrap();
    let source_peer = install_node_identity(&mut source, "source");
    let target_peer = install_node_identity(&mut target, "target");
    let source_object = source
        .publish_text(&source_peer.identity.id, "Source Object")
        .unwrap();
    let target_object = source
        .publish_text(&source_peer.identity.id, "Target Object")
        .unwrap();
    let edge = source
        .publish_edge(
            &source_peer.identity.id,
            source_object.id.clone(),
            target_object.id.clone(),
            babel_graph::Relation::References,
            babel_graph::EdgeOrigin::HumanAssertion,
        )
        .unwrap();
    let engine = GossipEngine::new(NetworkLimits::default());

    exchange_hellos(
        &engine,
        &source_peer,
        &mut source,
        &target_peer,
        &mut target,
    );

    let request = engine
        .receive(
            &mut target,
            Envelope::signed(
                &source_peer.identity,
                &source_peer.keypair,
                Message::ObjectInventory {
                    objects: vec![source_object.id.clone()],
                },
            )
            .unwrap(),
        )
        .unwrap();
    let NetworkAction::RequestObjects { objects } = request else {
        panic!("target should request missing object");
    };

    let response = engine
        .receive(
            &mut source,
            Envelope::signed(
                &target_peer.identity,
                &target_peer.keypair,
                Message::RequestObjects { objects },
            )
            .unwrap(),
        )
        .unwrap();
    let NetworkAction::SendBundle { bundle } = response else {
        panic!("source should return object bundle");
    };
    assert!(
        bundle
            .objects
            .iter()
            .any(|object| object.id == source_object.id)
    );
    assert!(
        bundle
            .objects
            .iter()
            .any(|object| object.id == target_object.id)
    );
    assert!(bundle.edges.iter().any(|candidate| candidate.id == edge.id));

    let imported = engine
        .receive(
            &mut target,
            Envelope::signed(
                &source_peer.identity,
                &source_peer.keypair,
                Message::ObjectBundle { bundle },
            )
            .unwrap(),
        )
        .unwrap();
    let NetworkAction::Imported { report } = imported else {
        panic!("target should import object bundle");
    };

    assert!(report.objects >= 2);
    assert!(target.object(&source_object.id).is_some());
    assert!(target.object(&target_object.id).is_some());
    assert!(target.edge(&edge.id).is_some());
}

#[test]
fn gossip_transports_signed_consensus_checkpoint() {
    let source_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let mut source = LocalNode::open(source_dir.path(), LocalProvider::default()).unwrap();
    let mut target = LocalNode::open(target_dir.path(), LocalProvider::default()).unwrap();
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
    let target_peer = install_node_identity(&mut target, "target");
    let engine = GossipEngine::new(NetworkLimits::default());

    receive_hello(&engine, &validators[0], &mut target);
    receive_hello(&engine, &target_peer, &mut source);

    let request = engine
        .receive(
            &mut target,
            Envelope::signed(
                &validators[0].identity,
                &validators[0].keypair,
                Message::Inventory {
                    events: vec![checkpoint.id.clone()],
                },
            )
            .unwrap(),
        )
        .unwrap();
    let NetworkAction::RequestEvents { events } = request else {
        panic!("target should request missing checkpoint");
    };

    let response = engine
        .receive(
            &mut source,
            Envelope::signed(
                &target_peer.identity,
                &target_peer.keypair,
                Message::RequestEvents { events },
            )
            .unwrap(),
        )
        .unwrap();
    let NetworkAction::SendBundle { bundle } = response else {
        panic!("source should return checkpoint bundle");
    };

    let imported = engine
        .receive(
            &mut target,
            Envelope::signed(
                &validators[0].identity,
                &validators[0].keypair,
                Message::EventBundle { bundle },
            )
            .unwrap(),
        )
        .unwrap();
    let NetworkAction::Imported { report } = imported else {
        panic!("target should import checkpoint bundle");
    };
    let imported_checkpoint = target.event(&checkpoint.id).unwrap();
    let payload: FinalityCheckpoint =
        serde_json::from_value(imported_checkpoint.payload.clone()).unwrap();

    assert!(report.events > 0);
    assert_eq!(imported_checkpoint.kind, EventKind::ConsensusCheckpoint);
    assert_eq!(
        imported_checkpoint.parents,
        vec![payload.last_finalized_event]
    );
}

#[test]
fn gossip_rejects_oversized_inventory_and_tampered_payload_hash() {
    let dir = tempfile::tempdir().unwrap();
    let mut node = LocalNode::open(dir.path(), LocalProvider::default()).unwrap();
    let peer = install_node_identity(&mut node, "node");
    let engine = GossipEngine::new(NetworkLimits {
        max_inventory_ids: 1,
        max_request_ids: 1,
        max_bundle_events: 1,
        max_bundle_objects: 1,
    });
    let first = node.publish_text(&peer.identity.id, "first").unwrap();
    let second = node.publish_text(&peer.identity.id, "second").unwrap();

    let too_large = Envelope::signed(
        &peer.identity,
        &peer.keypair,
        Message::Inventory {
            events: node
                .store()
                .list_events()
                .unwrap()
                .into_iter()
                .filter(|event| {
                    matches!(
                        &event.target,
                        babel_state::EventTarget::Object(object_id)
                            if object_id == &first.id || object_id == &second.id
                    )
                })
                .map(|event| event.id)
                .collect(),
        },
    )
    .unwrap();
    assert!(engine.receive(&mut node, too_large).is_err());

    let mut tampered = Envelope::signed(
        &peer.identity,
        &peer.keypair,
        Message::Inventory { events: Vec::new() },
    )
    .unwrap();
    tampered.payload_hash = babel_types::Hash::from_bytes(b"wrong");
    assert!(engine.receive(&mut node, tampered).is_err());
}

#[test]
fn gossip_rejects_unsigned_unknown_peer_traffic_before_hello() {
    let source_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let mut source = LocalNode::open(source_dir.path(), LocalProvider::default()).unwrap();
    let mut target = LocalNode::open(target_dir.path(), LocalProvider::default()).unwrap();
    let source_peer = install_node_identity(&mut source, "source");
    let object = source
        .publish_text(&source_peer.identity.id, "Needs authentication first.")
        .unwrap();
    let event_id = source
        .store()
        .list_events()
        .unwrap()
        .into_iter()
        .find(|event| matches!(&event.target, EventTarget::Object(object_id) if object_id == &object.id))
        .unwrap()
        .id;
    let engine = GossipEngine::new(NetworkLimits::default());

    let inventory = Envelope::signed(
        &source_peer.identity,
        &source_peer.keypair,
        Message::Inventory {
            events: vec![event_id],
        },
    )
    .unwrap();
    assert!(engine.receive(&mut target, inventory).is_err());

    receive_hello(&engine, &source_peer, &mut target);
    let inventory = Envelope::signed(
        &source_peer.identity,
        &source_peer.keypair,
        Message::Inventory { events: Vec::new() },
    )
    .unwrap();
    assert_eq!(
        engine.receive(&mut target, inventory).unwrap(),
        NetworkAction::None
    );
}

struct ValidatorFixture {
    identity: Identity,
    keypair: Keypair,
    identity_event: babel_types::EventId,
}

fn install_node_identity(node: &mut LocalNode<LocalProvider>, handle: &str) -> ValidatorFixture {
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
}

fn exchange_hellos(
    engine: &GossipEngine,
    source_peer: &ValidatorFixture,
    source: &mut LocalNode<LocalProvider>,
    target_peer: &ValidatorFixture,
    target: &mut LocalNode<LocalProvider>,
) {
    receive_hello(engine, source_peer, target);
    receive_hello(engine, target_peer, source);
}

fn receive_hello(
    engine: &GossipEngine,
    peer: &ValidatorFixture,
    receiver: &mut LocalNode<LocalProvider>,
) {
    let action = engine
        .receive(
            receiver,
            Envelope::signed(
                &peer.identity,
                &peer.keypair,
                Message::Hello {
                    identity: peer.identity.clone(),
                },
            )
            .unwrap(),
        )
        .unwrap();
    let NetworkAction::PeerAccepted { identity } = action else {
        panic!("hello should authenticate peer");
    };
    assert_eq!(identity.id, peer.identity.id);
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
        .publish_text(
            &validators[0].identity.id,
            "Network checkpoint gossip anchor Object.",
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

    validators
}
