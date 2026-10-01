use babble_crypto::Keypair;
use babble_graph::{Edge, EdgeOrigin, GraphIndex, GraphTraversalSpec, Relation, TraversalDirection};
use babble_identity::{Identity, IdentityKind};
use babble_object::Object;

#[test]
fn claim_evidence_traversal_separates_support_and_contradiction() {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "researcher", &keypair).unwrap();
    let claim = Object::text(
        &identity,
        "Claim: programmable social media can carry executable Objects",
    )
    .unwrap()
    .sign(&identity, &keypair)
    .unwrap();
    let support = Object::text(
        &identity,
        "Evidence: WASM sandboxing can run portable modules",
    )
    .unwrap()
    .sign(&identity, &keypair)
    .unwrap();
    let counter = Object::text(
        &identity,
        "Counterevidence: native APIs differ across platforms",
    )
    .unwrap()
    .sign(&identity, &keypair)
    .unwrap();

    let support_edge = Edge::new(
        support.id.clone(),
        claim.id.clone(),
        Relation::EvidenceFor,
        EdgeOrigin::HumanAssertion,
        Some(identity.id.clone()),
    )
    .unwrap()
    .sign(&identity, &keypair)
    .unwrap();
    let counter_edge = Edge::new(
        counter.id.clone(),
        claim.id.clone(),
        Relation::EvidenceAgainst,
        EdgeOrigin::HumanAssertion,
        Some(identity.id.clone()),
    )
    .unwrap()
    .sign(&identity, &keypair)
    .unwrap();

    let mut graph = GraphIndex::default();
    graph.insert(support_edge);
    graph.insert(counter_edge);

    let incoming = graph.incoming_iter(&claim.id).collect::<Vec<_>>();
    assert_eq!(incoming, graph.incoming(&claim.id));
    assert_eq!(incoming.len(), 2);
    assert!(incoming.windows(2).all(|pair| pair[0].id < pair[1].id));
    assert_eq!(graph.incoming_iter(&support.id).count(), 0);
    assert_eq!(graph.incoming_iter(&claim.id).take(1).count(), 1);

    assert_eq!(graph.supporting_evidence(&claim.id), vec![&support.id]);
    assert_eq!(graph.contradicting_evidence(&claim.id), vec![&counter.id]);
    assert_eq!(
        graph.targets(&support.id, &Relation::EvidenceFor),
        vec![&claim.id]
    );
}

#[test]
fn bounded_traversal_walks_typed_neighborhoods_deterministically() {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "mapper", &keypair).unwrap();
    let root = Object::text(&identity, "Root claim")
        .unwrap()
        .sign(&identity, &keypair)
        .unwrap();
    let evidence = Object::text(&identity, "Direct evidence")
        .unwrap()
        .sign(&identity, &keypair)
        .unwrap();
    let source = Object::text(&identity, "Source dataset")
        .unwrap()
        .sign(&identity, &keypair)
        .unwrap();
    let counter = Object::text(&identity, "Counter evidence")
        .unwrap()
        .sign(&identity, &keypair)
        .unwrap();

    let mut graph = GraphIndex::default();
    for edge in [
        Edge::new(
            evidence.id.clone(),
            root.id.clone(),
            Relation::EvidenceFor,
            EdgeOrigin::HumanAssertion,
            Some(identity.id.clone()),
        )
        .unwrap(),
        Edge::new(
            source.id.clone(),
            evidence.id.clone(),
            Relation::Cites,
            EdgeOrigin::HumanAssertion,
            Some(identity.id.clone()),
        )
        .unwrap(),
        Edge::new(
            counter.id.clone(),
            root.id.clone(),
            Relation::EvidenceAgainst,
            EdgeOrigin::HumanAssertion,
            Some(identity.id.clone()),
        )
        .unwrap(),
    ] {
        graph.insert(edge.sign(&identity, &keypair).unwrap());
    }

    let traversal = graph.traverse(&GraphTraversalSpec {
        root: root.id.clone(),
        direction: TraversalDirection::Incoming,
        relations: vec![Relation::EvidenceFor, Relation::Cites],
        max_depth: 2,
        limit: 8,
    });

    assert!(!traversal.truncated);
    assert_eq!(traversal.steps.len(), 2);
    assert_eq!(traversal.steps[0].depth, 1);
    assert_eq!(traversal.steps[0].next_object, evidence.id);
    assert_eq!(traversal.steps[0].edge.relation, Relation::EvidenceFor);
    assert_eq!(traversal.steps[1].depth, 2);
    assert_eq!(traversal.steps[1].next_object, source.id);
    assert_eq!(traversal.steps[1].edge.relation, Relation::Cites);

    let truncated = graph.traverse(&GraphTraversalSpec {
        root: root.id,
        direction: TraversalDirection::Incoming,
        relations: Vec::new(),
        max_depth: 2,
        limit: 1,
    });
    assert!(truncated.truncated);
    assert_eq!(truncated.steps.len(), 1);
}
