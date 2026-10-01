use babel_identity::{Identity, IdentityKeyTransition};
use babel_state::{Event, EventKind, EventTarget};
use babel_types::{Canonical, EventId, IdentityId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ValidatorSet {
    weights: BTreeMap<IdentityId, u64>,
    total_weight: u64,
}

impl ValidatorSet {
    pub fn new(weights: BTreeMap<IdentityId, u64>) -> Result<Self> {
        if weights.is_empty() {
            return Err(babel_types::Error::Conflict(
                "validator set cannot be empty".to_string(),
            ));
        }
        let total_weight = weights.values().try_fold(0_u64, |total, weight| {
            if *weight == 0 {
                return Err(babel_types::Error::Conflict(
                    "validator weight must be greater than zero".to_string(),
                ));
            }
            total.checked_add(*weight).ok_or_else(|| {
                babel_types::Error::Conflict("validator weight overflow".to_string())
            })
        })?;
        Ok(Self {
            weights,
            total_weight,
        })
    }

    pub fn equal<I>(validators: I) -> Result<Self>
    where
        I: IntoIterator<Item = IdentityId>,
    {
        Self::new(
            validators
                .into_iter()
                .map(|identity_id| (identity_id, 1))
                .collect(),
        )
    }

    pub fn contains(&self, identity_id: &IdentityId) -> bool {
        self.weights.contains_key(identity_id)
    }

    pub fn weight(&self, identity_id: &IdentityId) -> u64 {
        self.weights.get(identity_id).copied().unwrap_or_default()
    }

    pub fn total_weight(&self) -> u64 {
        self.total_weight
    }

    pub fn commitment_hash(&self) -> Result<babel_types::Hash> {
        self.weights.canonical_hash()
    }

    pub fn supermajority_weight(&self) -> u64 {
        self.total_weight - (self.total_weight / 3)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OrderedEvent {
    pub index: u64,
    pub id: EventId,
    pub generation: u64,
    pub actor: IdentityId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Checkpoint {
    pub event_count: u64,
    pub last_event: Option<EventId>,
    pub order_hash: babel_types::Hash,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FinalityCheckpoint {
    pub checkpoint: Checkpoint,
    pub finalized_count: u64,
    pub last_finalized_event: EventId,
    pub finalized_order_hash: babel_types::Hash,
    pub validator_set_hash: babel_types::Hash,
    pub finality_report_hash: babel_types::Hash,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventRound {
    pub id: EventId,
    pub round: u64,
    pub witness: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FamousWitness {
    pub id: EventId,
    pub round: u64,
    pub decided_by_round: u64,
    pub yes_weight: u64,
    pub no_weight: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FinalizedEvent {
    pub index: u64,
    pub id: EventId,
    pub actor: IdentityId,
    pub event_round: u64,
    pub round_received: u64,
    pub consensus_timestamp: babel_types::Timestamp,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FinalityReport {
    pub validator_weight: u64,
    pub supermajority_weight: u64,
    pub rounds: Vec<EventRound>,
    pub famous_witnesses: Vec<FamousWitness>,
    pub finalized: Vec<FinalizedEvent>,
    pub undecided_witnesses: Vec<EventId>,
}

#[derive(Default)]
pub struct EventDag {
    identities: BTreeMap<IdentityId, Identity>,
    identity_keys: BTreeMap<IdentityId, Vec<IdentityKeyTransition>>,
    events: BTreeMap<EventId, Event>,
    children: BTreeMap<EventId, BTreeSet<EventId>>,
    generations: BTreeMap<EventId, u64>,
}

impl EventDag {
    pub fn add_identity(&mut self, identity: Identity) -> Result<()> {
        identity.verify()?;
        if let Some(existing) = self.identities.get(&identity.id) {
            if existing == &identity {
                return Ok(());
            }
            return Err(babel_types::Error::Conflict(format!(
                "identity id conflict: {}",
                identity.id
            )));
        }
        self.identities.insert(identity.id.clone(), identity);
        Ok(())
    }

    pub fn insert(&mut self, event: Event) -> Result<()> {
        let transition = if event.kind == EventKind::IdentityKeyTransition {
            Some(
                serde_json::from_value::<IdentityKeyTransition>(event.payload.clone())
                    .map_err(|err| babel_types::Error::Canonical(err.to_string()))?,
            )
        } else {
            None
        };
        let actor = if let Some(transition) = &transition {
            let identity = self
                .identities
                .get(&event.actor)
                .ok_or_else(|| babel_types::Error::NotFound(event.actor.to_string()))?;
            identity.with_signing_key(transition.previous_public_key.clone())
        } else {
            self.signing_identity_at(&event.actor, event.created_at)?
        };
        event.verify(&actor)?;

        if let Some(existing) = self.events.get(&event.id) {
            if existing == &event {
                return Ok(());
            }
            return Err(babel_types::Error::Conflict(format!(
                "event id conflict: {}",
                event.id
            )));
        }

        let mut generation = 0;
        for parent in &event.parents {
            let Some(parent_generation) = self.generations.get(parent) else {
                return Err(babel_types::Error::NotFound(format!(
                    "event parent {parent}"
                )));
            };
            generation = generation.max(parent_generation + 1);
        }

        if self.descendants(&event.id).contains(&event.id) {
            return Err(babel_types::Error::Conflict(format!(
                "event cycle detected: {}",
                event.id
            )));
        }

        for parent in &event.parents {
            self.children
                .entry(parent.clone())
                .or_default()
                .insert(event.id.clone());
        }
        if let Some(transition) = transition {
            if transition.identity_id != event.actor {
                return Err(babel_types::Error::Conflict(format!(
                    "identity key transition actor mismatch: actor={} transition={}",
                    event.actor, transition.identity_id
                )));
            }
            let current =
                self.signing_identity_at(&transition.identity_id, transition.effective_at)?;
            let expected_sequence = self
                .identity_keys
                .get(&transition.identity_id)
                .map_or(1, |transitions| transitions.len() as u64 + 1);
            if transition.sequence != expected_sequence {
                return Err(babel_types::Error::Conflict(format!(
                    "identity key transition sequence mismatch for {}: expected {}, got {}",
                    transition.identity_id, expected_sequence, transition.sequence
                )));
            }
            transition.verify(&current.public_key)?;
            self.identity_keys
                .entry(transition.identity_id.clone())
                .or_default()
                .push(transition);
        }
        self.children.entry(event.id.clone()).or_default();
        self.generations.insert(event.id.clone(), generation);
        self.events.insert(event.id.clone(), event);
        Ok(())
    }

    fn signing_identity_at(&self, id: &IdentityId, at: Timestamp) -> Result<Identity> {
        let identity = self
            .identities
            .get(id)
            .ok_or_else(|| babel_types::Error::NotFound(id.to_string()))?;
        let public_key = self
            .identity_keys
            .get(id)
            .into_iter()
            .flatten()
            .filter(|transition| transition.effective_at <= at)
            .filter(|transition| {
                transition
                    .expires_at
                    .is_none_or(|expires_at| expires_at > at)
            })
            .max_by(|left, right| {
                left.effective_at
                    .cmp(&right.effective_at)
                    .then_with(|| left.sequence.cmp(&right.sequence))
            })
            .map(|transition| transition.next_public_key.clone())
            .unwrap_or_else(|| identity.public_key.clone());
        Ok(identity.with_signing_key(public_key))
    }

    pub fn contains(&self, id: &EventId) -> bool {
        self.events.contains_key(id)
    }

    pub fn event(&self, id: &EventId) -> Option<&Event> {
        self.events.get(id)
    }

    pub fn parents(&self, id: &EventId) -> Vec<&EventId> {
        self.events
            .get(id)
            .map(|event| event.parents.iter().collect())
            .unwrap_or_default()
    }

    pub fn children(&self, id: &EventId) -> Vec<&EventId> {
        self.children
            .get(id)
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
    }

    pub fn ancestors(&self, id: &EventId) -> BTreeSet<EventId> {
        let mut seen = BTreeSet::new();
        let mut stack = self
            .events
            .get(id)
            .map(|event| event.parents.clone())
            .unwrap_or_default();
        while let Some(parent) = stack.pop() {
            if !seen.insert(parent.clone()) {
                continue;
            }
            if let Some(event) = self.events.get(&parent) {
                stack.extend(event.parents.clone());
            }
        }
        seen
    }

    pub fn descendants(&self, id: &EventId) -> BTreeSet<EventId> {
        let mut seen = BTreeSet::new();
        let mut stack = self
            .children
            .get(id)
            .map(|children| children.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        while let Some(child) = stack.pop() {
            if !seen.insert(child.clone()) {
                continue;
            }
            if let Some(children) = self.children.get(&child) {
                stack.extend(children.iter().cloned());
            }
        }
        seen
    }

    pub fn deterministic_order(&self) -> Vec<OrderedEvent> {
        self.events
            .values()
            .map(|event| OrderedEvent {
                index: 0,
                id: event.id.clone(),
                generation: *self.generations.get(&event.id).unwrap_or(&0),
                actor: event.actor.clone(),
            })
            .collect::<Vec<_>>()
            .into_iter()
            .enumerate()
            .map(|(index, mut event)| {
                event.index = index as u64;
                event
            })
            .collect()
    }

    pub fn consensus_order(&self) -> Result<Vec<OrderedEvent>> {
        let mut ordered = self
            .events
            .values()
            .map(|event| OrderedEvent {
                index: 0,
                id: event.id.clone(),
                generation: *self.generations.get(&event.id).unwrap_or(&0),
                actor: event.actor.clone(),
            })
            .collect::<Vec<_>>();
        ordered.sort_by(|left, right| {
            left.generation
                .cmp(&right.generation)
                .then_with(|| left.id.cmp(&right.id))
        });

        let mut positions = BTreeMap::new();
        for (index, ordered_event) in ordered.iter_mut().enumerate() {
            ordered_event.index = index as u64;
            positions.insert(ordered_event.id.clone(), index);
        }

        for event in self.events.values() {
            let event_position = positions
                .get(&event.id)
                .ok_or_else(|| babel_types::Error::NotFound(event.id.to_string()))?;
            for parent in &event.parents {
                let parent_position = positions
                    .get(parent)
                    .ok_or_else(|| babel_types::Error::NotFound(parent.to_string()))?;
                if parent_position >= event_position {
                    return Err(babel_types::Error::Conflict(format!(
                        "parent {parent} is not ordered before child {}",
                        event.id
                    )));
                }
            }
        }

        Ok(ordered)
    }

    pub fn checkpoint(&self) -> Result<Checkpoint> {
        let order = self.consensus_order()?;
        let order_hash = order.canonical_hash()?;
        Ok(Checkpoint {
            event_count: order.len() as u64,
            last_event: order.last().map(|event| event.id.clone()),
            order_hash,
        })
    }

    pub fn checkpoint_event(&self, actor: &Identity) -> Result<Event> {
        let checkpoint = self.checkpoint()?;
        Event::new(
            actor,
            EventKind::ConsensusCheckpoint,
            EventTarget::Network,
            serde_json::to_value(checkpoint)
                .map_err(|err| babel_types::Error::Canonical(err.to_string()))?,
            self.consensus_order()?
                .last()
                .map(|event| vec![event.id.clone()])
                .unwrap_or_default(),
        )
    }

    pub fn finality_checkpoint(&self, validators: &ValidatorSet) -> Result<FinalityCheckpoint> {
        let report = self.finality(validators)?;
        let Some(last_finalized) = report.finalized.last() else {
            return Err(babel_types::Error::Conflict(
                "cannot checkpoint without finalized events".to_string(),
            ));
        };
        Ok(FinalityCheckpoint {
            checkpoint: self.checkpoint()?,
            finalized_count: report.finalized.len() as u64,
            last_finalized_event: last_finalized.id.clone(),
            finalized_order_hash: report.finalized.canonical_hash()?,
            validator_set_hash: validators.commitment_hash()?,
            finality_report_hash: report.canonical_hash()?,
        })
    }

    pub fn finality(&self, validators: &ValidatorSet) -> Result<FinalityReport> {
        self.validate_validators(validators)?;
        let rounds = self.assign_rounds(validators)?;
        let witnesses = self.witnesses(&rounds)?;
        let fame = self.decide_fame(validators, &rounds, &witnesses)?;
        let finalized = self.finalized_events(validators, &rounds, &fame)?;

        Ok(FinalityReport {
            validator_weight: validators.total_weight(),
            supermajority_weight: validators.supermajority_weight(),
            rounds: rounds
                .iter()
                .map(|(id, event_round)| EventRound {
                    id: id.clone(),
                    round: event_round.round,
                    witness: event_round.witness,
                })
                .collect(),
            famous_witnesses: fame
                .decided
                .values()
                .filter(|decision| decision.famous)
                .map(|decision| FamousWitness {
                    id: decision.id.clone(),
                    round: decision.round,
                    decided_by_round: decision.decided_by_round,
                    yes_weight: decision.yes_weight,
                    no_weight: decision.no_weight,
                })
                .collect(),
            finalized,
            undecided_witnesses: fame.undecided,
        })
    }

    fn validate_validators(&self, validators: &ValidatorSet) -> Result<()> {
        for validator_id in validators.weights.keys() {
            if !self.identities.contains_key(validator_id) {
                return Err(babel_types::Error::NotFound(format!(
                    "validator {validator_id}"
                )));
            }
        }
        Ok(())
    }

    fn assign_rounds(&self, validators: &ValidatorSet) -> Result<BTreeMap<EventId, RoundInfo>> {
        let order = self.consensus_order()?;
        let mut rounds = BTreeMap::new();
        let mut first_by_actor_round = BTreeSet::new();

        for ordered in order {
            let event = self
                .events
                .get(&ordered.id)
                .ok_or_else(|| babel_types::Error::NotFound(ordered.id.to_string()))?;
            let parent_round = event
                .parents
                .iter()
                .filter_map(|parent| rounds.get(parent).map(|info: &RoundInfo| info.round))
                .max()
                .unwrap_or(1);
            let round =
                if self.strongly_sees_round_witnesses(&event.id, parent_round, validators, &rounds)
                {
                    parent_round + 1
                } else {
                    parent_round
                };
            let witness = validators.contains(&event.actor)
                && first_by_actor_round.insert((event.actor.clone(), round));
            rounds.insert(event.id.clone(), RoundInfo { round, witness });
        }

        Ok(rounds)
    }

    fn witnesses(
        &self,
        rounds: &BTreeMap<EventId, RoundInfo>,
    ) -> Result<BTreeMap<u64, Vec<EventId>>> {
        let mut witnesses: BTreeMap<u64, Vec<EventId>> = BTreeMap::new();
        for ordered in self.consensus_order()? {
            if let Some(round_info) = rounds.get(&ordered.id)
                && round_info.witness
            {
                witnesses
                    .entry(round_info.round)
                    .or_default()
                    .push(ordered.id);
            }
        }
        Ok(witnesses)
    }

    fn decide_fame(
        &self,
        validators: &ValidatorSet,
        rounds: &BTreeMap<EventId, RoundInfo>,
        witnesses: &BTreeMap<u64, Vec<EventId>>,
    ) -> Result<FameReport> {
        let mut decided = BTreeMap::new();
        let mut undecided = Vec::new();
        let max_round = witnesses.keys().next_back().copied().unwrap_or_default();

        for (round, candidates) in witnesses {
            for candidate in candidates {
                let Some(decision) =
                    self.decide_witness_fame(candidate, *round, max_round, validators, witnesses)?
                else {
                    undecided.push(candidate.clone());
                    continue;
                };
                decided.insert(candidate.clone(), decision);
            }
        }

        for witness_id in decided.keys() {
            if !rounds
                .get(witness_id)
                .map(|round_info| round_info.witness)
                .unwrap_or_default()
            {
                return Err(babel_types::Error::Conflict(format!(
                    "fame decided for non-witness event {witness_id}"
                )));
            }
        }

        Ok(FameReport { decided, undecided })
    }

    fn decide_witness_fame(
        &self,
        candidate: &EventId,
        candidate_round: u64,
        max_round: u64,
        validators: &ValidatorSet,
        witnesses: &BTreeMap<u64, Vec<EventId>>,
    ) -> Result<Option<FameDecision>> {
        let mut votes: BTreeMap<EventId, bool> = BTreeMap::new();
        for round in (candidate_round + 1)..=max_round {
            let Some(round_witnesses) = witnesses.get(&round) else {
                continue;
            };
            for witness in round_witnesses {
                let vote = if round == candidate_round + 1 {
                    Some(self.sees(witness, candidate))
                } else {
                    self.derived_vote(witness, round - 1, validators, witnesses, &votes)
                };
                if let Some(vote) = vote {
                    votes.insert(witness.clone(), vote);
                }
            }

            let tally = self.vote_tally(round_witnesses, validators, &votes);
            if tally.yes >= validators.supermajority_weight() {
                return Ok(Some(FameDecision {
                    id: candidate.clone(),
                    round: candidate_round,
                    famous: true,
                    decided_by_round: round,
                    yes_weight: tally.yes,
                    no_weight: tally.no,
                }));
            }
            if tally.no >= validators.supermajority_weight() {
                return Ok(Some(FameDecision {
                    id: candidate.clone(),
                    round: candidate_round,
                    famous: false,
                    decided_by_round: round,
                    yes_weight: tally.yes,
                    no_weight: tally.no,
                }));
            }
        }

        Ok(None)
    }

    fn derived_vote(
        &self,
        witness: &EventId,
        previous_round: u64,
        validators: &ValidatorSet,
        witnesses: &BTreeMap<u64, Vec<EventId>>,
        votes: &BTreeMap<EventId, bool>,
    ) -> Option<bool> {
        let previous_witnesses = witnesses.get(&previous_round)?;
        let mut yes = 0_u64;
        let mut no = 0_u64;

        for previous in previous_witnesses {
            if !self.strongly_sees(witness, previous, validators) {
                continue;
            }
            let previous_vote = votes.get(previous)?;
            let weight = self
                .events
                .get(previous)
                .map(|event| validators.weight(&event.actor))
                .unwrap_or_default();
            if *previous_vote {
                yes += weight;
            } else {
                no += weight;
            }
        }

        match yes.cmp(&no) {
            std::cmp::Ordering::Greater => Some(true),
            std::cmp::Ordering::Less => Some(false),
            std::cmp::Ordering::Equal => None,
        }
    }

    fn vote_tally(
        &self,
        witnesses: &[EventId],
        validators: &ValidatorSet,
        votes: &BTreeMap<EventId, bool>,
    ) -> VoteTally {
        witnesses
            .iter()
            .fold(VoteTally::default(), |mut tally, id| {
                if let Some(vote) = votes.get(id) {
                    let weight = self
                        .events
                        .get(id)
                        .map(|event| validators.weight(&event.actor))
                        .unwrap_or_default();
                    if *vote {
                        tally.yes += weight;
                    } else {
                        tally.no += weight;
                    }
                }
                tally
            })
    }

    fn finalized_events(
        &self,
        validators: &ValidatorSet,
        rounds: &BTreeMap<EventId, RoundInfo>,
        fame: &FameReport,
    ) -> Result<Vec<FinalizedEvent>> {
        let mut famous_by_round: BTreeMap<u64, Vec<EventId>> = BTreeMap::new();
        for decision in fame.decided.values().filter(|decision| decision.famous) {
            famous_by_round
                .entry(decision.round)
                .or_default()
                .push(decision.id.clone());
        }

        let mut finalized = Vec::new();
        for ordered in self.consensus_order()? {
            let event = self
                .events
                .get(&ordered.id)
                .ok_or_else(|| babel_types::Error::NotFound(ordered.id.to_string()))?;
            let Some(event_round) = rounds.get(&ordered.id).map(|round_info| round_info.round)
            else {
                continue;
            };
            let Some((round_received, consensus_timestamp)) =
                self.round_received(&ordered.id, event_round, validators, &famous_by_round)
            else {
                continue;
            };
            finalized.push(FinalizedEvent {
                index: 0,
                id: ordered.id,
                actor: event.actor.clone(),
                event_round,
                round_received,
                consensus_timestamp,
            });
        }

        finalized.sort_by(|left, right| {
            left.round_received
                .cmp(&right.round_received)
                .then_with(|| left.consensus_timestamp.cmp(&right.consensus_timestamp))
                .then_with(|| left.id.cmp(&right.id))
        });
        for (index, event) in finalized.iter_mut().enumerate() {
            event.index = index as u64;
        }
        Ok(finalized)
    }

    fn round_received(
        &self,
        event_id: &EventId,
        event_round: u64,
        validators: &ValidatorSet,
        famous_by_round: &BTreeMap<u64, Vec<EventId>>,
    ) -> Option<(u64, babel_types::Timestamp)> {
        for (round, witnesses) in famous_by_round.range((event_round + 1)..) {
            let mut seen_weight = 0_u64;
            let mut timestamps = Vec::new();
            for witness_id in witnesses {
                let witness = self.events.get(witness_id)?;
                if self.sees(witness_id, event_id) {
                    seen_weight += validators.weight(&witness.actor);
                    timestamps.push(witness.created_at);
                }
            }
            if seen_weight >= validators.supermajority_weight() && !timestamps.is_empty() {
                timestamps.sort();
                return Some((*round, timestamps[timestamps.len() / 2]));
            }
        }
        None
    }

    fn strongly_sees_round_witnesses(
        &self,
        observer: &EventId,
        round: u64,
        validators: &ValidatorSet,
        rounds: &BTreeMap<EventId, RoundInfo>,
    ) -> bool {
        let seen_weight = rounds
            .iter()
            .filter(|(_, round_info)| round_info.round == round && round_info.witness)
            .filter_map(|(witness_id, _)| {
                self.events.get(witness_id).map(|event| (witness_id, event))
            })
            .filter(|(witness_id, _)| self.strongly_sees(observer, witness_id, validators))
            .map(|(_, event)| validators.weight(&event.actor))
            .sum::<u64>();
        seen_weight >= validators.supermajority_weight()
    }

    pub fn strongly_sees(
        &self,
        observer: &EventId,
        target: &EventId,
        validators: &ValidatorSet,
    ) -> bool {
        let mut seen_validators = BTreeSet::new();
        for candidate in self.past(observer) {
            let Some(event) = self.events.get(&candidate) else {
                continue;
            };
            if !validators.contains(&event.actor) || !self.sees(&candidate, target) {
                continue;
            }
            seen_validators.insert(event.actor.clone());
        }
        let seen_weight = seen_validators
            .iter()
            .map(|identity_id| validators.weight(identity_id))
            .sum::<u64>();
        seen_weight >= validators.supermajority_weight()
    }

    pub fn sees(&self, observer: &EventId, target: &EventId) -> bool {
        if observer == target {
            return true;
        }
        if !self.ancestors(observer).contains(target) {
            return false;
        }
        let Some(target_event) = self.events.get(target) else {
            return false;
        };
        self.past(observer)
            .into_iter()
            .filter_map(|id| self.events.get(&id).map(|event| (id, event)))
            .filter(|(id, event)| *id != *target && event.actor == target_event.actor)
            .all(|(id, _)| self.comparable(target, &id))
    }

    fn comparable(&self, left: &EventId, right: &EventId) -> bool {
        left == right
            || self.ancestors(left).contains(right)
            || self.ancestors(right).contains(left)
    }

    fn past(&self, id: &EventId) -> BTreeSet<EventId> {
        let mut past = self.ancestors(id);
        if self.events.contains_key(id) {
            past.insert(id.clone());
        }
        past
    }
}

#[derive(Clone, Debug)]
struct RoundInfo {
    round: u64,
    witness: bool,
}

#[derive(Clone, Debug)]
struct FameDecision {
    id: EventId,
    round: u64,
    famous: bool,
    decided_by_round: u64,
    yes_weight: u64,
    no_weight: u64,
}

#[derive(Clone, Debug, Default)]
struct FameReport {
    decided: BTreeMap<EventId, FameDecision>,
    undecided: Vec<EventId>,
}

#[derive(Clone, Debug, Default)]
struct VoteTally {
    yes: u64,
    no: u64,
}
