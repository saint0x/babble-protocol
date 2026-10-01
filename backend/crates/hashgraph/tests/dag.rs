use babel_crypto::Keypair;
use babel_hashgraph::{EventDag, ValidatorSet};
use babel_identity::{Identity, IdentityKind};
use babel_state::{Event, EventKind, EventTarget};

#[test]
fn hashgraph_orders_events_after_their_parents() {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
    let root = signed_event(&identity, &keypair, "root", Vec::new());
    let child = signed_event(&identity, &keypair, "child", vec![root.id.clone()]);
    let sibling = signed_event(&identity, &keypair, "sibling", vec![root.id.clone()]);

    let mut dag = EventDag::default();
    dag.add_identity(identity).unwrap();
    dag.insert(root.clone()).unwrap();
    dag.insert(sibling.clone()).unwrap();
    dag.insert(child.clone()).unwrap();

    let order = dag.consensus_order().unwrap();
    let root_pos = order.iter().position(|event| event.id == root.id).unwrap();
    let child_pos = order.iter().position(|event| event.id == child.id).unwrap();
    let sibling_pos = order
        .iter()
        .position(|event| event.id == sibling.id)
        .unwrap();

    assert!(root_pos < child_pos);
    assert!(root_pos < sibling_pos);
    assert_eq!(
        dag.ancestors(&child.id).into_iter().collect::<Vec<_>>(),
        vec![root.id]
    );
}

#[test]
fn hashgraph_rejects_missing_parent_and_conflicting_event_id() {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
    let missing_parent = signed_event(
        &identity,
        &keypair,
        "orphan",
        vec![babel_types::EventId::new_unchecked(format!(
            "evt_{}",
            "0".repeat(64)
        ))],
    );
    let root = signed_event(&identity, &keypair, "root", Vec::new());
    let mut conflict = root.clone();
    conflict.payload = serde_json::json!({ "label": "conflict" });

    let mut dag = EventDag::default();
    dag.add_identity(identity).unwrap();

    assert!(dag.insert(missing_parent).is_err());
    dag.insert(root.clone()).unwrap();
    assert!(dag.insert(root).is_ok());
    assert!(dag.insert(conflict).is_err());
}

#[test]
fn hashgraph_checkpoint_commits_deterministic_order() {
    let keypair = Keypair::generate();
    let identity = Identity::create(IdentityKind::Person, "alice", &keypair).unwrap();
    let first = signed_event(&identity, &keypair, "first", Vec::new());
    let second = signed_event(&identity, &keypair, "second", vec![first.id.clone()]);

    let mut dag = EventDag::default();
    dag.add_identity(identity.clone()).unwrap();
    dag.insert(first).unwrap();
    dag.insert(second.clone()).unwrap();

    let checkpoint = dag.checkpoint().unwrap();
    let checkpoint_event = dag.checkpoint_event(&identity).unwrap();

    assert_eq!(checkpoint.event_count, 2);
    assert_eq!(checkpoint.last_event, Some(second.id.clone()));
    assert_eq!(checkpoint_event.target, EventTarget::Network);
    assert_eq!(checkpoint_event.parents, vec![second.id]);
}

#[test]
fn hashgraph_reaches_finality_through_virtual_votes() {
    let mut dag = EventDag::default();
    let validators = validator_identities(&mut dag, &["alice", "bob", "cara", "drew"]);
    let validator_set =
        ValidatorSet::equal(validators.iter().map(|identity| identity.id.clone())).unwrap();

    let roots = validators
        .iter()
        .map(|identity| signed_event(identity, &identity.keypair, "root", Vec::new()))
        .collect::<Vec<_>>();
    for root in &roots {
        dag.insert(root.clone()).unwrap();
    }

    let layer_two = validators
        .iter()
        .map(|identity| {
            signed_event(
                identity,
                &identity.keypair,
                "layer-two",
                roots.iter().map(|event| event.id.clone()).collect(),
            )
        })
        .collect::<Vec<_>>();
    for event in &layer_two {
        dag.insert(event.clone()).unwrap();
    }

    let layer_three = validators
        .iter()
        .map(|identity| {
            signed_event(
                identity,
                &identity.keypair,
                "layer-three",
                layer_two.iter().map(|event| event.id.clone()).collect(),
            )
        })
        .collect::<Vec<_>>();
    for event in &layer_three {
        dag.insert(event.clone()).unwrap();
    }

    let layer_four = validators
        .iter()
        .map(|identity| {
            signed_event(
                identity,
                &identity.keypair,
                "layer-four",
                layer_three.iter().map(|event| event.id.clone()).collect(),
            )
        })
        .collect::<Vec<_>>();
    for event in &layer_four {
        dag.insert(event.clone()).unwrap();
    }

    let layer_five = validators
        .iter()
        .map(|identity| {
            signed_event(
                identity,
                &identity.keypair,
                "layer-five",
                layer_four.iter().map(|event| event.id.clone()).collect(),
            )
        })
        .collect::<Vec<_>>();
    for event in &layer_five {
        dag.insert(event.clone()).unwrap();
    }

    let report = dag.finality(&validator_set).unwrap();

    assert_eq!(report.validator_weight, 4);
    assert_eq!(report.supermajority_weight, 3);
    assert_eq!(report.famous_witnesses.len(), 8);
    assert!(
        roots
            .iter()
            .all(|event| !report.undecided_witnesses.contains(&event.id))
    );
    assert!(roots.iter().all(|event| {
        report
            .finalized
            .iter()
            .any(|finalized| finalized.id == event.id)
    }));
    assert!(
        report
            .finalized
            .windows(2)
            .all(|window| window[0].index < window[1].index)
    );
}

#[test]
fn hashgraph_leaves_partial_gossip_undecided() {
    let mut dag = EventDag::default();
    let validators = validator_identities(&mut dag, &["alice", "bob", "cara", "drew"]);
    let validator_set =
        ValidatorSet::equal(validators.iter().map(|identity| identity.id.clone())).unwrap();

    for identity in &validators {
        dag.insert(signed_event(
            identity,
            &identity.keypair,
            "root",
            Vec::new(),
        ))
        .unwrap();
    }

    let report = dag.finality(&validator_set).unwrap();

    assert!(report.finalized.is_empty());
    assert_eq!(report.undecided_witnesses.len(), 4);
}

fn signed_event(
    identity: &Identity,
    keypair: &Keypair,
    label: &str,
    parents: Vec<babel_types::EventId>,
) -> Event {
    Event::new(
        identity,
        EventKind::CapabilityGranted,
        EventTarget::Network,
        serde_json::json!({ "label": label }),
        parents,
    )
    .unwrap()
    .sign(identity, keypair)
    .unwrap()
}

struct ValidatorIdentity {
    id: babel_types::IdentityId,
    identity: Identity,
    keypair: Keypair,
}

impl std::ops::Deref for ValidatorIdentity {
    type Target = Identity;

    fn deref(&self) -> &Self::Target {
        &self.identity
    }
}

fn validator_identities(dag: &mut EventDag, handles: &[&str]) -> Vec<ValidatorIdentity> {
    handles
        .iter()
        .map(|handle| {
            let keypair = Keypair::generate();
            let identity = Identity::create(IdentityKind::Person, *handle, &keypair).unwrap();
            dag.add_identity(identity.clone()).unwrap();
            ValidatorIdentity {
                id: identity.id.clone(),
                identity,
                keypair,
            }
        })
        .collect()
}
