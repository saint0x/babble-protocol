use babble_network::{Message, NetworkLimits};
use babble_sim::{DeliveryStatus, NetworkSimulator, event_sets_by_node};

#[test]
fn simulation_converges_through_relay_after_direct_partition() {
    let alice_dir = tempfile::tempdir().unwrap();
    let bob_dir = tempfile::tempdir().unwrap();
    let cara_dir = tempfile::tempdir().unwrap();
    let mut sim = NetworkSimulator::new(NetworkLimits::default());
    let alice = sim.add_node(alice_dir.path(), "alice").unwrap();
    let bob = sim.add_node(bob_dir.path(), "bob").unwrap();
    let cara = sim.add_node(cara_dir.path(), "cara").unwrap();

    let object = sim
        .publish_text(alice, "Relay gossip preserves signed protocol records.")
        .unwrap();
    sim.partition(alice, cara).unwrap();

    assert_eq!(
        sim.sync_pair(alice, cara).unwrap().status,
        DeliveryStatus::DroppedPartition
    );
    assert_eq!(
        sim.sync_pair(alice, bob).unwrap().status,
        DeliveryStatus::Delivered
    );
    assert_eq!(
        sim.sync_pair(bob, cara).unwrap().status,
        DeliveryStatus::Delivered
    );

    assert!(sim.node(cara).unwrap().object(&object.id).is_some());
    assert!(
        event_sets_by_node(&sim)
            .unwrap()
            .values()
            .all(|events| !events.is_empty())
    );
    assert!(sim.stats().imported_events >= 2);
    assert_eq!(sim.stats().rejected, 0);
}

#[test]
fn simulation_rejects_tampered_envelope_without_import() {
    let source_dir = tempfile::tempdir().unwrap();
    let target_dir = tempfile::tempdir().unwrap();
    let mut sim = NetworkSimulator::new(NetworkLimits::default());
    let source = sim.add_node(source_dir.path(), "source").unwrap();
    let target = sim.add_node(target_dir.path(), "target").unwrap();
    let object = sim
        .publish_text(source, "Tampered network envelopes cannot import state.")
        .unwrap();
    let events = sim.events(source).unwrap();

    let report = sim
        .deliver_tampered_hash(source, target, Message::Inventory { events })
        .unwrap();

    assert_eq!(report.status, DeliveryStatus::Rejected);
    assert!(sim.node(target).unwrap().object(&object.id).is_none());
    assert_eq!(sim.stats().rejected, 1);
}
