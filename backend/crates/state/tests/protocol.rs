use babble_crypto::Keypair;
use babble_graph::{Edge, EdgeOrigin, Relation};
use babble_identity::{Identity, IdentityKind};
use babble_object::Object;
use babble_state::MemoryState;

#[test]
fn identity_object_and_edge_round_trip_through_state() {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
    identity.verify().unwrap();

    let object = Object::text(
        &identity,
        "Babble treats claims, evidence, and software as Objects.",
    )
    .unwrap()
    .sign(&identity, &keypair)
    .unwrap();
    object.verify(&identity).unwrap();

    let evidence = Object::text(
        &identity,
        "The  spec defines the Object -> Graph -> Judgment pipeline.",
    )
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

    let mut state = MemoryState::default();
    let identity_event_id = state.insert_identity(identity.clone(), &keypair).unwrap();
    state
        .event(&identity_event_id)
        .unwrap()
        .verify(&identity)
        .unwrap();
    state.publish_object(object.clone(), &keypair).unwrap();
    state.publish_object(evidence.clone(), &keypair).unwrap();
    state.publish_edge(edge, &keypair).unwrap();

    assert!(state.object(&object.id).is_some());
    assert_eq!(state.graph().outgoing(&evidence.id).len(), 1);
    assert_eq!(state.graph().incoming(&object.id).len(), 1);
}

#[test]
fn tampered_object_fails_verification() {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Pseudonym, "cipher", &keypair).unwrap();
    let mut object = Object::text(&identity, "original")
        .unwrap()
        .sign(&identity, &keypair)
        .unwrap();

    object.payload = serde_json::json!({ "text": "tampered", "metadata": {} });

    assert!(object.verify(&identity).is_err());
}

#[test]
fn unsigned_object_is_rejected_by_state() {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Agent, "jev-shadow", &keypair).unwrap();
    let object = Object::text(&identity, "unsigned observations are not publishable").unwrap();

    let mut state = MemoryState::default();
    state.insert_identity(identity, &keypair).unwrap();

    assert!(state.publish_object(object, &keypair).is_err());
}
