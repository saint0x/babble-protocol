use babble_crypto::Signature;
use babble_identity::Identity;
use babble_types::{Canonical, EdgeId, ObjectId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

mod replies;
pub use replies::{RepliesIndex, ReplyPosition};
mod following;
mod safety;
pub use safety::{SafetyAction, SafetyActionPayload, SafetyReceipt, SafetyReceiptPayload, SafetyRequest, SafetyState, SafetySnapshot, SafetyEntry};
mod reactions;
pub use reactions::{
    Appreciation, Engagement, REACTION_MAX_REVISION, ReactionAction, ReactionActionPayload,
    ReactionReceipt, ReactionReceiptPayload, ReactionRecord, ReactionRequest, ReactionState,
    ReactionSummary, ReactionValue, Stance,
};
pub use following::{FollowAction, FollowActionPayload, FollowReceipt, FollowReceiptPayload, FollowRequest, FollowState};

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    ReplyTo,
    References,
    Quotes,
    Contains,
    Cites,
    Supports,
    Contradicts,
    EvidenceFor,
    EvidenceAgainst,
    Extends,
    DerivesFrom,
    Supersedes,
    Forks,
    Remixes,
    CreatedBy,
    Follows,
    Trusts,
    Custom(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum EdgeOrigin {
    HumanAssertion,
    ApplicationAssertion,
    JudgmentDerived,
    ConsensusDerived,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Edge {
    pub id: EdgeId,
    pub source: ObjectId,
    pub target: ObjectId,
    pub relation: Relation,
    pub origin: EdgeOrigin,
    pub author: Option<babble_types::IdentityId>,
    pub created_at: Timestamp,
    pub metadata: BTreeMap<String, Value>,
    pub signature: Option<Signature>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TraversalDirection {
    Outgoing,
    Incoming,
    Undirected,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GraphTraversalSpec {
    pub root: ObjectId,
    pub direction: TraversalDirection,
    pub relations: Vec<Relation>,
    pub max_depth: u8,
    pub limit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GraphTraversalStep {
    pub depth: u8,
    pub edge: Edge,
    pub next_object: ObjectId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GraphTraversal {
    pub root: ObjectId,
    pub direction: TraversalDirection,
    pub relations: Vec<Relation>,
    pub max_depth: u8,
    pub steps: Vec<GraphTraversalStep>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
struct EdgeCommitment {
    pub source: ObjectId,
    pub target: ObjectId,
    pub relation: Relation,
    pub origin: EdgeOrigin,
    pub author: Option<babble_types::IdentityId>,
    pub created_at: Timestamp,
    pub metadata: BTreeMap<String, Value>,
}

impl Edge {
    pub fn new(
        source: ObjectId,
        target: ObjectId,
        relation: Relation,
        origin: EdgeOrigin,
        author: Option<babble_types::IdentityId>,
    ) -> Result<Self> {
        let commitment = EdgeCommitment {
            source,
            target,
            relation,
            origin,
            author,
            created_at: Timestamp::now(),
            metadata: BTreeMap::new(),
        };
        let id = EdgeId::from_hash(&commitment.canonical_hash()?);
        Ok(Self {
            id,
            source: commitment.source,
            target: commitment.target,
            relation: commitment.relation,
            origin: commitment.origin,
            author: commitment.author,
            created_at: commitment.created_at,
            metadata: commitment.metadata,
            signature: None,
        })
    }

    pub fn sign(mut self, author: &Identity, keypair: &babble_crypto::Keypair) -> Result<Self> {
        let commitment = self.commitment();
        self.author = Some(author.id.clone());
        self.signature = Some(keypair.sign(&commitment.canonical_bytes()?));
        Ok(self)
    }

    pub fn with_metadata(mut self, metadata: BTreeMap<String, Value>) -> Result<Self> {
        self.metadata = metadata;
        self.signature = None;
        self.id = EdgeId::from_hash(&self.commitment().canonical_hash()?);
        Ok(self)
    }

    pub fn verify(&self, author: &Identity) -> Result<()> {
        self.id.validate()?;
        let expected_id = EdgeId::from_hash(&self.commitment().canonical_hash()?);
        if expected_id != self.id {
            return Err(babble_types::Error::Signature);
        }
        let signature = self
            .signature
            .as_ref()
            .ok_or(babble_types::Error::UnsignedEdge)?;
        author
            .public_key
            .verify(&self.commitment().canonical_bytes()?, signature)
    }

    fn commitment(&self) -> EdgeCommitment {
        EdgeCommitment {
            source: self.source.clone(),
            target: self.target.clone(),
            relation: self.relation.clone(),
            origin: self.origin.clone(),
            author: self.author.clone(),
            created_at: self.created_at,
            metadata: self.metadata.clone(),
        }
    }
}

#[derive(Default)]
pub struct GraphIndex {
    edges: BTreeMap<EdgeId, Edge>,
    outgoing: BTreeMap<ObjectId, BTreeSet<EdgeId>>,
    incoming: BTreeMap<ObjectId, BTreeSet<EdgeId>>,
    targets_by_relation: BTreeMap<(ObjectId, Relation), BTreeSet<ObjectId>>,
    sources_by_relation: BTreeMap<(ObjectId, Relation), BTreeSet<ObjectId>>,
    activity_counts: BTreeMap<ObjectId, usize>,
    activity: BTreeSet<(std::cmp::Reverse<usize>, ObjectId)>,
}

impl GraphIndex {
    pub fn insert(&mut self, edge: Edge) {
        // This is a retrieval opportunity count, not temporal engagement: the
        // latter also checks publication/evaluation times after admission.
        if !self.edges.contains_key(&edge.id)
            && edge.signature.is_some()
            && edge.author.is_some()
            && edge.source != edge.target
            && matches!(
                edge.origin,
                EdgeOrigin::HumanAssertion | EdgeOrigin::ApplicationAssertion
            )
            && matches!(
                edge.relation,
                Relation::ReplyTo | Relation::References | Relation::Quotes
                    | Relation::Cites | Relation::Supports | Relation::Contradicts
                    | Relation::EvidenceFor | Relation::EvidenceAgainst | Relation::Extends
                    | Relation::DerivesFrom | Relation::Supersedes | Relation::Forks | Relation::Remixes
            )
        {
            let count = self.activity_counts.entry(edge.target.clone()).or_default();
            self.activity
                .remove(&(std::cmp::Reverse(*count), edge.target.clone()));
            *count += 1;
            self.activity
                .insert((std::cmp::Reverse(*count), edge.target.clone()));
        }
        self.targets_by_relation
            .entry((edge.source.clone(), edge.relation.clone()))
            .or_default()
            .insert(edge.target.clone());
        self.sources_by_relation
            .entry((edge.target.clone(), edge.relation.clone()))
            .or_default()
            .insert(edge.source.clone());
        self.outgoing
            .entry(edge.source.clone())
            .or_default()
            .insert(edge.id.clone());
        self.incoming
            .entry(edge.target.clone())
            .or_default()
            .insert(edge.id.clone());
        self.edges.insert(edge.id.clone(), edge);
    }

    pub fn get(&self, id: &EdgeId) -> Option<&Edge> {
        self.edges.get(id)
    }

    /// Unique neighbors in canonical order, without materializing edge records.
    pub fn relation_neighbors(
        &self,
        root: &ObjectId,
        relation: &Relation,
        incoming: bool,
    ) -> Option<&BTreeSet<ObjectId>> {
        let index = if incoming {
            &self.sources_by_relation
        } else {
            &self.targets_by_relation
        };
        index.get(&(root.clone(), relation.clone()))
    }

    /// Public signed inbound relationship activity, highest count first.
    pub fn active_objects(&self) -> impl Iterator<Item = &ObjectId> {
        self.activity.iter().map(|(_, id)| id)
    }

    pub fn has_public_activity(&self, id: &ObjectId) -> bool {
        self.activity_counts.contains_key(id)
    }

    pub fn outgoing(&self, source: &ObjectId) -> Vec<&Edge> {
        self.outgoing
            .get(source)
            .into_iter()
            .flatten()
            .filter_map(|id| self.edges.get(id))
            .collect()
    }

    pub fn incoming(&self, target: &ObjectId) -> Vec<&Edge> {
        self.incoming_iter(target).collect()
    }

    pub fn incoming_iter<'a>(&'a self, target: &ObjectId) -> impl Iterator<Item = &'a Edge> + use<'a> {
        self.incoming
            .get(target)
            .into_iter()
            .flatten()
            .filter_map(|id| self.edges.get(id))
    }

    pub fn outgoing_relation(&self, source: &ObjectId, relation: &Relation) -> Vec<&Edge> {
        self.outgoing(source)
            .into_iter()
            .filter(|edge| &edge.relation == relation)
            .collect()
    }

    pub fn incoming_relation(&self, target: &ObjectId, relation: &Relation) -> Vec<&Edge> {
        self.incoming(target)
            .into_iter()
            .filter(|edge| &edge.relation == relation)
            .collect()
    }

    pub fn targets(&self, source: &ObjectId, relation: &Relation) -> Vec<&ObjectId> {
        self.outgoing_relation(source, relation)
            .into_iter()
            .map(|edge| &edge.target)
            .collect()
    }

    pub fn sources(&self, target: &ObjectId, relation: &Relation) -> Vec<&ObjectId> {
        self.incoming_relation(target, relation)
            .into_iter()
            .map(|edge| &edge.source)
            .collect()
    }

    pub fn supporting_evidence(&self, claim: &ObjectId) -> Vec<&ObjectId> {
        self.sources(claim, &Relation::EvidenceFor)
    }

    pub fn contradicting_evidence(&self, claim: &ObjectId) -> Vec<&ObjectId> {
        self.sources(claim, &Relation::EvidenceAgainst)
    }

    pub fn traverse(&self, spec: &GraphTraversalSpec) -> GraphTraversal {
        let limit = spec.limit.max(1);
        let mut steps = Vec::new();
        let mut truncated = false;
        let mut queue = VecDeque::from([(spec.root.clone(), 0_u8)]);
        let mut seen_objects = BTreeSet::from([spec.root.clone()]);
        let mut seen_edges = BTreeSet::new();
        let relation_filter = spec.relations.iter().collect::<BTreeSet<_>>();

        while let Some((object_id, depth)) = queue.pop_front() {
            if depth >= spec.max_depth {
                continue;
            }

            for (edge, next_object) in self.traversal_edges(&object_id, &spec.direction) {
                if !relation_filter.is_empty() && !relation_filter.contains(&edge.relation) {
                    continue;
                }
                if !seen_edges.insert(edge.id.clone()) {
                    continue;
                }
                if steps.len() >= limit {
                    truncated = true;
                    break;
                }
                let next_depth = depth.saturating_add(1);
                steps.push(GraphTraversalStep {
                    depth: next_depth,
                    edge: edge.clone(),
                    next_object: next_object.clone(),
                });
                if next_depth < spec.max_depth && seen_objects.insert(next_object.clone()) {
                    queue.push_back((next_object, next_depth));
                }
            }

            if truncated {
                break;
            }
        }

        GraphTraversal {
            root: spec.root.clone(),
            direction: spec.direction.clone(),
            relations: spec.relations.clone(),
            max_depth: spec.max_depth,
            steps,
            truncated,
        }
    }

    fn traversal_edges(
        &self,
        object_id: &ObjectId,
        direction: &TraversalDirection,
    ) -> Vec<(&Edge, ObjectId)> {
        let mut edges = Vec::new();
        if matches!(
            direction,
            TraversalDirection::Outgoing | TraversalDirection::Undirected
        ) {
            edges.extend(
                self.outgoing(object_id)
                    .into_iter()
                    .map(|edge| (edge, edge.target.clone())),
            );
        }
        if matches!(
            direction,
            TraversalDirection::Incoming | TraversalDirection::Undirected
        ) {
            edges.extend(
                self.incoming(object_id)
                    .into_iter()
                    .map(|edge| (edge, edge.source.clone())),
            );
        }
        edges.sort_by(|left, right| {
            left.0
                .relation
                .cmp(&right.0.relation)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.0.id.cmp(&right.0.id))
        });
        edges
    }
}
pub mod moderation;
