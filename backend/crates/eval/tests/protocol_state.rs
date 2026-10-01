use babble_crypto::Keypair;
use babble_eval::{
    ConsensusFinalityEvalCase, ConsensusLoadEvalCase, RealtimeLoadEvalCase, RealtimeRoomEvalCase,
    evaluate_consensus_finality, evaluate_consensus_load, evaluate_realtime_load,
    evaluate_realtime_rooms,
};
use babble_hashgraph::{EventDag, ValidatorSet};
use babble_identity::{Identity, IdentityKind};
use babble_realtime::{
    MembershipPolicy, PersistencePolicy, RealtimeHub, RealtimeOperation, RealtimePayload,
    RoomLimits, RoomSpec,
};
use babble_state::{Event, EventKind, EventTarget};
use babble_types::{EventId, IdentityId, ObjectId};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

#[test]
fn hashgraph_finality_passes_consensus_eval_corpus() {
    let mut dag = EventDag::default();
    let validators = validator_identities(&mut dag, &["alice", "bob", "cara", "drew"]);
    let validator_set =
        ValidatorSet::equal(validators.iter().map(|identity| identity.id.clone())).unwrap();
    let layers = layered_gossip(&mut dag, &validators, 5);
    let finalized_report = dag.finality(&validator_set).unwrap();

    let mut partial = EventDag::default();
    let partial_validators = validator_identities(&mut partial, &["alice", "bob", "cara", "drew"]);
    let partial_validator_set = ValidatorSet::equal(
        partial_validators
            .iter()
            .map(|identity| identity.id.clone()),
    )
    .unwrap();
    for identity in &partial_validators {
        partial
            .insert(signed_event(
                identity,
                &identity.keypair,
                "root",
                Vec::new(),
            ))
            .unwrap();
    }
    let partial_report = partial.finality(&partial_validator_set).unwrap();

    let report = evaluate_consensus_finality(&[
        ConsensusFinalityEvalCase {
            name: "multi-round-four-validator-dag-finalizes-prefix".to_string(),
            report: finalized_report,
            expected_validator_weight: 4,
            expected_supermajority_weight: 3,
            min_rounds: 20,
            min_famous_witnesses: 8,
            min_finalized: layers[0].len(),
            max_undecided_witnesses: 12,
            require_strict_finalized_order: true,
        },
        ConsensusFinalityEvalCase {
            name: "single-round-partial-gossip-stays-undecided".to_string(),
            report: partial_report,
            expected_validator_weight: 4,
            expected_supermajority_weight: 3,
            min_rounds: 4,
            min_famous_witnesses: 0,
            min_finalized: 0,
            max_undecided_witnesses: 4,
            require_strict_finalized_order: true,
        },
    ]);

    report.assert_passed().unwrap();
    assert_eq!(report.cases, 2);
    assert!(report.average_famous_witnesses >= 4.0);
    assert!(report.average_finalized >= 2.0);
}

#[test]
fn realtime_rooms_pass_state_eval_corpus() {
    let mut hub = RealtimeHub::default();
    let spec = RoomSpec::new(
        object_id(),
        "object-surface",
        "babble.realtime.state.v1",
        MembershipPolicy::Open,
        PersistencePolicy::DurableMessages,
        RoomLimits::default(),
    )
    .unwrap();
    let room_id = hub.create_room(spec).unwrap().id;
    let alice = hub.start_session(&room_id, identity_id('a')).unwrap();
    let bob = hub.start_session(&room_id, identity_id('b')).unwrap();

    hub.publish(
        &alice.id,
        RealtimePayload::State(RealtimeOperation::IncrementCounter {
            key: "score".to_string(),
            by: 2,
        }),
        true,
    )
    .unwrap();
    hub.publish(
        &bob.id,
        RealtimePayload::State(RealtimeOperation::IncrementCounter {
            key: "score".to_string(),
            by: 3,
        }),
        true,
    )
    .unwrap();
    hub.publish(
        &alice.id,
        RealtimePayload::State(RealtimeOperation::SetRegister {
            key: "mode".to_string(),
            value: json!("playing"),
        }),
        true,
    )
    .unwrap();
    hub.publish(
        &alice.id,
        RealtimePayload::State(RealtimeOperation::AddToSet {
            key: "selected".to_string(),
            value: "portal-card".to_string(),
        }),
        true,
    )
    .unwrap();
    hub.publish(
        &bob.id,
        RealtimePayload::State(RealtimeOperation::AddToSet {
            key: "selected".to_string(),
            value: "game-card".to_string(),
        }),
        true,
    )
    .unwrap();
    hub.publish(
        &alice.id,
        RealtimePayload::State(RealtimeOperation::RemoveFromSet {
            key: "selected".to_string(),
            value: "portal-card".to_string(),
        }),
        true,
    )
    .unwrap();
    let active_view = hub.room(&room_id).unwrap();

    hub.close_session(&bob.id).unwrap();
    let after_close_view = hub.room(&room_id).unwrap();

    let report = evaluate_realtime_rooms(&[
        RealtimeRoomEvalCase {
            name: "active-object-surface-room".to_string(),
            room: active_view,
            min_durable_messages: 6,
            expected_active_sessions: 2,
            expected_participants: 2,
            expected_counters: BTreeMap::from([("score".to_string(), 5)]),
            expected_registers: BTreeMap::from([("mode".to_string(), json!("playing"))]),
            expected_sets: BTreeMap::from([(
                "selected".to_string(),
                BTreeSet::from(["game-card".to_string()]),
            )]),
        },
        RealtimeRoomEvalCase {
            name: "closed-session-presence-update".to_string(),
            room: after_close_view,
            min_durable_messages: 6,
            expected_active_sessions: 1,
            expected_participants: 1,
            expected_counters: BTreeMap::from([("score".to_string(), 5)]),
            expected_registers: BTreeMap::from([("mode".to_string(), json!("playing"))]),
            expected_sets: BTreeMap::from([(
                "selected".to_string(),
                BTreeSet::from(["game-card".to_string()]),
            )]),
        },
    ]);

    report.assert_passed().unwrap();
    assert_eq!(report.cases, 2);
    assert_eq!(report.average_durable_messages, 6.0);
}

#[test]
fn consensus_and_realtime_load_suites_pass_scaled_regression_corpus() {
    let consensus = consensus_load_case();
    let realtime = realtime_load_case();

    let consensus_report = evaluate_consensus_load(&[consensus]);
    consensus_report.assert_passed().unwrap();
    assert_eq!(consensus_report.cases, 1);
    assert!(consensus_report.total_events >= 80);
    assert!(consensus_report.average_finalized >= 8.0);

    let realtime_report = evaluate_realtime_load(&[realtime]);
    realtime_report.assert_passed().unwrap();
    assert_eq!(realtime_report.cases, 1);
    assert!(realtime_report.total_sessions >= 24);
    assert!(realtime_report.total_messages >= 480);
}

fn consensus_load_case() -> ConsensusLoadEvalCase {
    let mut dag = EventDag::default();
    let handles = (0..8)
        .map(|index| format!("validator-{index}"))
        .collect::<Vec<_>>();
    let handle_refs = handles.iter().map(String::as_str).collect::<Vec<_>>();
    let validators = validator_identities(&mut dag, &handle_refs);
    let validator_set =
        ValidatorSet::equal(validators.iter().map(|identity| identity.id.clone())).unwrap();

    let started_at = Instant::now();
    let layers = layered_gossip(&mut dag, &validators, 10);
    let report = dag.finality(&validator_set).unwrap();
    let elapsed_ms = elapsed_ms(started_at);
    let inserted_events = validators.len() + layers.iter().map(Vec::len).sum::<usize>();

    ConsensusLoadEvalCase {
        name: "eight-validator-ten-layer-all-to-all-finality-load".to_string(),
        report,
        validator_count: validators.len(),
        inserted_events,
        elapsed_ms,
        min_rounds: inserted_events / 2,
        min_famous_witnesses: validators.len() * 2,
        min_finalized: validators.len(),
        max_undecided_witnesses: validators.len() * 6,
        max_elapsed_ms: 10_000,
        require_strict_finalized_order: true,
    }
}

fn realtime_load_case() -> RealtimeLoadEvalCase {
    let session_count = 24usize;
    let messages_per_session = 20usize;
    let mut hub = RealtimeHub::default();
    let spec = RoomSpec::new(
        object_id(),
        "scaled-object-surface",
        "babble.realtime.state.v1",
        MembershipPolicy::Open,
        PersistencePolicy::DurableMessages,
        RoomLimits {
            max_members: session_count,
            max_payload_bytes: 4096,
            max_messages_per_session: (messages_per_session + 1) as u64,
        },
    )
    .unwrap();
    let room_id = hub.create_room(spec).unwrap().id;

    let started_at = Instant::now();
    let sessions = (0..session_count)
        .map(|index| {
            hub.start_session(&room_id, identity_id_number(index))
                .unwrap()
        })
        .collect::<Vec<_>>();

    for (session_index, session) in sessions.iter().enumerate() {
        for message_index in 0..messages_per_session {
            hub.publish(
                &session.id,
                RealtimePayload::State(RealtimeOperation::IncrementCounter {
                    key: "ticks".to_string(),
                    by: 1,
                }),
                true,
            )
            .unwrap();
            if message_index == messages_per_session - 1 {
                hub.publish(
                    &session.id,
                    RealtimePayload::State(RealtimeOperation::SetRegister {
                        key: format!("last-session-{session_index}"),
                        value: json!(message_index),
                    }),
                    true,
                )
                .unwrap();
            }
        }
    }

    let room = hub.room(&room_id).unwrap();
    let elapsed_ms = elapsed_ms(started_at);
    let published_messages = session_count * (messages_per_session + 1);

    RealtimeLoadEvalCase {
        name: "twenty-four-session-durable-state-room-load".to_string(),
        room,
        opened_sessions: session_count,
        published_messages,
        elapsed_ms,
        min_durable_messages: published_messages,
        expected_active_sessions: session_count,
        expected_participants: session_count,
        expected_counter_totals: BTreeMap::from([(
            "ticks".to_string(),
            (session_count * messages_per_session) as i64,
        )]),
        max_elapsed_ms: 10_000,
    }
}

fn layered_gossip(
    dag: &mut EventDag,
    validators: &[ValidatorIdentity],
    count: usize,
) -> Vec<Vec<Event>> {
    let mut layers = Vec::new();
    for index in 0..count {
        let parents: Vec<EventId> = layers
            .last()
            .map(|events: &Vec<Event>| events.iter().map(|event| event.id.clone()).collect())
            .unwrap_or_default();
        let layer = validators
            .iter()
            .map(|identity| {
                signed_event(
                    identity,
                    &identity.keypair,
                    &format!("layer-{}", index + 1),
                    parents.clone(),
                )
            })
            .collect::<Vec<_>>();
        for event in &layer {
            dag.insert(event.clone()).unwrap();
        }
        layers.push(layer);
    }
    layers
}

fn signed_event(
    identity: &Identity,
    keypair: &Keypair,
    label: &str,
    parents: Vec<EventId>,
) -> Event {
    Event::new(
        identity,
        EventKind::CapabilityGranted,
        EventTarget::Network,
        json!({ "label": label }),
        parents,
    )
    .unwrap()
    .sign(identity, keypair)
    .unwrap()
}

struct ValidatorIdentity {
    id: IdentityId,
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

fn object_id() -> ObjectId {
    ObjectId::new_unchecked(format!("obj_{}", "1".repeat(64)))
}

fn identity_id(value: char) -> IdentityId {
    IdentityId::new_unchecked(format!("id_{}", value.to_string().repeat(64)))
}

fn identity_id_number(value: usize) -> IdentityId {
    IdentityId::new_unchecked(format!("id_{value:064x}"))
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at
        .elapsed()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
