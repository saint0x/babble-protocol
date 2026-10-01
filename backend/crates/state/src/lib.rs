use babel_crypto::{Keypair, Signature};
use babel_graph::{Edge, GraphIndex};
use babel_identity::{Identity, IdentityKeyTransition};
use babel_object::Object;
use babel_types::{Canonical, EventId, IdentityId, ObjectId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventKind {
    IdentityCreated,
    IdentityKeyTransition,
    ObjectPublished,
    EdgePublished,
    ObjectForked,
    ObjectRemixed,
    CapabilityGranted,
    CapabilityRevoked,
    RealtimeRoomDefined,
    RealtimeSessionStarted,
    RealtimeSessionClosed,
    RealtimeMessageCommitted,
    RealtimeSnapshotCommitted,
    ConsensusCheckpoint,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum EventTarget {
    Identity(babel_types::IdentityId),
    Object(ObjectId),
    Edge(babel_types::EdgeId),
    Network,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Event {
    pub id: EventId,
    pub actor: babel_types::IdentityId,
    pub kind: EventKind,
    pub target: EventTarget,
    pub payload: Value,
    pub created_at: Timestamp,
    pub parents: Vec<EventId>,
    pub signature: Option<Signature>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
struct EventCommitment {
    pub actor: babel_types::IdentityId,
    pub kind: EventKind,
    pub target: EventTarget,
    pub payload: Value,
    pub created_at: Timestamp,
    pub parents: Vec<EventId>,
}

impl Event {
    pub fn new(
        actor: &Identity,
        kind: EventKind,
        target: EventTarget,
        payload: Value,
        parents: Vec<EventId>,
    ) -> Result<Self> {
        let commitment = EventCommitment {
            actor: actor.id.clone(),
            kind,
            target,
            payload,
            created_at: Timestamp::now(),
            parents,
        };
        let id = EventId::from_hash(&commitment.canonical_hash()?);
        Ok(Self {
            id,
            actor: commitment.actor,
            kind: commitment.kind,
            target: commitment.target,
            payload: commitment.payload,
            created_at: commitment.created_at,
            parents: commitment.parents,
            signature: None,
        })
    }

    pub fn sign(mut self, actor: &Identity, keypair: &babel_crypto::Keypair) -> Result<Self> {
        if self.actor != actor.id {
            return Err(babel_types::Error::Signature);
        }
        self.signature = Some(keypair.sign(&self.commitment().canonical_bytes()?));
        Ok(self)
    }

    pub fn verify(&self, actor: &Identity) -> Result<()> {
        self.id.validate()?;
        if self.actor != actor.id {
            return Err(babel_types::Error::Signature);
        }
        let expected_id = EventId::from_hash(&self.commitment().canonical_hash()?);
        if expected_id != self.id {
            return Err(babel_types::Error::Signature);
        }
        let signature = self
            .signature
            .as_ref()
            .ok_or(babel_types::Error::UnsignedEvent)?;
        actor
            .public_key
            .verify(&self.commitment().canonical_bytes()?, signature)
    }

    fn commitment(&self) -> EventCommitment {
        EventCommitment {
            actor: self.actor.clone(),
            kind: self.kind.clone(),
            target: self.target.clone(),
            payload: self.payload.clone(),
            created_at: self.created_at,
            parents: self.parents.clone(),
        }
    }
}

#[derive(Default)]
pub struct MemoryState {
    identities: BTreeMap<babel_types::IdentityId, Identity>,
    identity_keys: BTreeMap<babel_types::IdentityId, Vec<IdentityKeyTransition>>,
    objects: BTreeMap<ObjectId, Object>,
    events: BTreeMap<EventId, Event>,
    graph: GraphIndex,
}

impl MemoryState {
    pub fn apply_identity(&mut self, identity: Identity) -> Result<()> {
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

    pub fn apply_identity_key_transition(
        &mut self,
        transition: IdentityKeyTransition,
    ) -> Result<()> {
        let current = self.signing_identity_at(&transition.identity_id, transition.effective_at)?;
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
        Ok(())
    }

    pub fn apply_object(&mut self, object: Object) -> Result<()> {
        let author = self.signing_identity_at(&object.author, object.created_at)?;
        object.verify(&author)?;
        if let Some(existing) = self.objects.get(&object.id) {
            if existing == &object {
                return Ok(());
            }
            return Err(babel_types::Error::Conflict(format!(
                "object id conflict: {}",
                object.id
            )));
        }
        for relation in object.relations.iter().cloned() {
            self.graph.insert(relation);
        }
        self.objects.insert(object.id.clone(), object);
        Ok(())
    }

    pub fn apply_edge(&mut self, edge: Edge) -> Result<()> {
        let author_id = edge
            .author
            .clone()
            .ok_or(babel_types::Error::UnsignedEdge)?;
        let author = self.signing_identity_at(&author_id, edge.created_at)?;
        edge.verify(&author)?;
        if let Some(existing) = self.graph.get(&edge.id) {
            if existing == &edge {
                return Ok(());
            }
            return Err(babel_types::Error::Conflict(format!(
                "edge id conflict: {}",
                edge.id
            )));
        }
        self.graph.insert(edge);
        Ok(())
    }

    pub fn apply_event(&mut self, event: Event) -> Result<()> {
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
        if let Some(transition) = transition {
            if transition.identity_id != event.actor {
                return Err(babel_types::Error::Conflict(format!(
                    "identity key transition actor mismatch: actor={} transition={}",
                    event.actor, transition.identity_id
                )));
            }
            self.apply_identity_key_transition(transition)?;
        }
        self.events.insert(event.id.clone(), event);
        Ok(())
    }

    pub fn insert_identity(&mut self, identity: Identity, keypair: &Keypair) -> Result<EventId> {
        identity.verify()?;
        let event = Event::new(
            &identity,
            EventKind::IdentityCreated,
            EventTarget::Identity(identity.id.clone()),
            serde_json::json!({ "handle": identity.handle }),
            Vec::new(),
        )?
        .sign(&identity, keypair)?;
        let event_id = event.id.clone();
        self.identities.insert(identity.id.clone(), identity);
        self.events.insert(event.id.clone(), event);
        Ok(event_id)
    }

    pub fn publish_object(&mut self, object: Object, keypair: &Keypair) -> Result<EventId> {
        self.publish_object_with_kind(object, EventKind::ObjectPublished, keypair)
    }

    pub fn publish_object_with_kind(
        &mut self,
        object: Object,
        kind: EventKind,
        keypair: &Keypair,
    ) -> Result<EventId> {
        if !matches!(
            kind,
            EventKind::ObjectPublished | EventKind::ObjectForked | EventKind::ObjectRemixed
        ) {
            return Err(babel_types::Error::Conflict(format!(
                "event kind cannot publish an object: {kind:?}"
            )));
        }
        let author = self.signing_identity_at(&object.author, object.created_at)?;
        object.verify(&author)?;
        if self.objects.contains_key(&object.id) {
            return Err(babel_types::Error::Conflict(format!(
                "object already exists: {}",
                object.id
            )));
        }
        for relation in object.relations.iter().cloned() {
            self.graph.insert(relation);
        }
        let event = Event::new(
            &author,
            kind,
            EventTarget::Object(object.id.clone()),
            serde_json::json!({
                "kind": object.kind.as_str(),
                "schema": object.schema,
                "provenance": object.provenance,
            }),
            self.latest_event_ids(2),
        )?
        .sign(&author, keypair)?;
        let event_id = event.id.clone();
        self.objects.insert(object.id.clone(), object);
        self.events.insert(event.id.clone(), event);
        Ok(event_id)
    }

    pub fn publish_edge(&mut self, edge: Edge, keypair: &Keypair) -> Result<EventId> {
        let author_id = edge
            .author
            .clone()
            .ok_or(babel_types::Error::UnsignedEdge)?;
        let author = self.signing_identity_at(&author_id, edge.created_at)?;
        edge.verify(&author)?;
        if self.graph.get(&edge.id).is_some() {
            return Err(babel_types::Error::Conflict(format!(
                "edge already exists: {}",
                edge.id
            )));
        }
        let event = Event::new(
            &author,
            EventKind::EdgePublished,
            EventTarget::Edge(edge.id.clone()),
            serde_json::json!({
                "source": edge.source,
                "target": edge.target,
                "relation": edge.relation,
            }),
            self.latest_event_ids(2),
        )?
        .sign(&author, keypair)?;
        let event_id = event.id.clone();
        self.graph.insert(edge);
        self.events.insert(event.id.clone(), event);
        Ok(event_id)
    }

    pub fn identity(&self, id: &babel_types::IdentityId) -> Option<&Identity> {
        self.identities.get(id)
    }

    pub fn signing_identity(&self, id: &IdentityId) -> Result<Identity> {
        self.identity_at(id, Timestamp::now())
    }

    pub fn signing_identity_at(&self, id: &IdentityId, at: Timestamp) -> Result<Identity> {
        self.identity_at(id, at)
    }

    pub fn identity_key_transitions(&self, id: &IdentityId) -> Vec<IdentityKeyTransition> {
        self.identity_keys.get(id).cloned().unwrap_or_default()
    }

    pub fn object(&self, id: &ObjectId) -> Option<&Object> {
        self.objects.get(id)
    }

    pub fn event(&self, id: &EventId) -> Option<&Event> {
        self.events.get(id)
    }

    pub fn graph(&self) -> &GraphIndex {
        &self.graph
    }

    fn latest_event_ids(&self, count: usize) -> Vec<EventId> {
        let mut events = self.events.values().collect::<Vec<_>>();
        events.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        events
            .into_iter()
            .take(count)
            .map(|event| event.id.clone())
            .collect()
    }

    fn identity_at(&self, id: &IdentityId, at: Timestamp) -> Result<Identity> {
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
}
