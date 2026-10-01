use babel_authoring::{CapabilityGrantDraft, EdgeDraft, ObjectDraft};
use babel_capabilities::{
    CapabilityBroker, CapabilityCall, CapabilityDecision, CapabilityDefinition, CapabilityGrant,
    CapabilityId, CapabilityManifest, CapabilityReceipt, CapabilityUsageWindow, GrantDecision,
};
use babel_crypto::Keypair;
use babel_discovery::{
    NativeTemporalScorer, TemporalProvider, TemporalProviderVersion,
    TemporalResult,
};
use babel_graph::{Edge, EdgeOrigin, GraphTraversal, GraphTraversalSpec, Relation};
use babel_hashgraph::{EventDag, FinalityCheckpoint, ValidatorSet};
use babel_identity::{Identity, IdentityKeyScope, IdentityKeyTransition, IdentityKind};
use babel_judgment::{
    DefinitionId, Judgment, JudgmentCache, JudgmentOrchestrator, JudgmentProvider,
    JudgmentProviderDescriptor, JudgmentRequest, OrchestratedJudgment, ProviderVersion,
};
use babel_lens::{
    BuiltInLens, DiversityPolicy, DiversityTrace, EvidenceSignals, LensStack, LensWeight,
    NativeRanker, RankedCandidate, RankingProvider, RankingProviderVersion, RankingRequest,
    RankingTrace, ReputationSignals,
};
use babel_media::{MediaBlob, MediaObjectPayload};
use babel_object::{CapabilityRequest, Object, ObjectKind, Provenance, SurfaceRole};
use babel_personalization::EncryptedLocalUserModel;
use babel_realtime::{
    RealtimeHub, RealtimeMessage, RealtimePayload, RealtimeSession, RealtimeSnapshot, RoomSpec,
    RoomView,
};
use babel_runtime::{
    ResourceBudget, SurfaceLifecycle, SurfaceRuntime, SurfaceRuntimeEvent,
    SurfaceRuntimeHealthSnapshot, SurfaceScheduleDecision, SurfaceScheduler,
    SurfaceSchedulingInput, SurfaceSession, SurfaceSessionId, SurfaceSessionPlan,
    SurfaceStateCheckpoint,
};
use babel_state::{Event, EventKind, EventTarget, MemoryState};
use babel_store::{FileStore, PersonalizationSyncRecord};
use babel_types::{
    Canonical, CapabilityGrantId, EdgeId, EventId, Hash, IdentityId, JudgmentId, ObjectId, Result,
    Timestamp,
};
use schemars::JsonSchema;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};
mod bundles;
mod discovery;
mod retrieval;
mod following;
mod safety;
pub mod moderation;
pub use safety::{SafetyState, SafetySnapshot, SafetyEntry};
mod reactions;
pub use babel_graph::{
    Appreciation, Engagement, ReactionAction, ReactionActionPayload, ReactionReceipt,
    ReactionReceiptPayload, ReactionRecord, ReactionRequest, ReactionState, ReactionSummary,
    ReactionValue, Stance,
};
mod agreement;
mod grants;
mod identity_capability;
pub mod invocations;
pub use invocations::{SocialInvocationPayload, SocialInvocationResult};
mod judgments;
mod keys;
mod local_storage;
mod network_fetch;
mod profiles;
mod publication;
mod retry;
pub use following::{FollowListPage, FollowState, FollowingPage, FollowingQuery};
pub use profiles::{AuthorObjectsPage, AuthorObjectsQuery};
mod realtime_capability;
mod replies;
pub use replies::{DirectReply, RepliesListQuery, RepliesListResult};
mod quotes;
pub use quotes::{QuotedObject, QuotesListQuery, QuotesListResult};
mod social;
pub use social::SocialMediaAttachment;
mod storage;

#[derive(
    Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema,
)]
pub struct ImportBundle {
    pub identities: Vec<Identity>,
    pub objects: Vec<Object>,
    pub edges: Vec<Edge>,
    pub events: Vec<Event>,
}

#[derive(
    Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema,
)]
pub struct ImportReport {
    pub identities: usize,
    pub objects: usize,
    pub edges: usize,
    pub events: usize,
    pub duplicate_events: usize,
}

#[derive(
    Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema,
)]
pub struct EventListQuery {
    pub after: Option<EventId>,
    pub limit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct EventListResult {
    pub events: Vec<Event>,
    pub next_after: Option<EventId>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct ObjectSearchQuery {
    pub query: Option<String>,
    pub author: Option<IdentityId>,
    pub kind: Option<String>,
    pub limit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct ObjectSearchResult {
    pub object: Object,
    pub score: u64,
    pub reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct DiscoveryQuery {
    pub anchors: Vec<ObjectId>,
    pub search: Option<String>,
    pub followed_objects: BTreeSet<ObjectId>,
    pub limit: usize,
    pub exploration_slots: usize,
    pub lens: LensStack,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct DiscoveryResult {
    pub ranked: Vec<RankedCandidate>,
    pub objects: Vec<Object>,
    pub trace: RankingTrace,
    pub diversity_trace: DiversityTrace,
    pub ranking_provider: RankingProviderVersion,
    pub temporal: TemporalResult,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct ProvenancePublication {
    pub object: Object,
    pub edges: Vec<Edge>,
    pub event: Event,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceRelationKind {
    Supports,
    Contradicts,
    Related,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct EvidenceProjectionItem {
    pub kind: EvidenceRelationKind,
    pub edge: Edge,
    pub evidence: Object,
    pub relationship_judgment: Option<Judgment>,
    pub evidence_judgments: Vec<Judgment>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct EvidenceProjectionSummary {
    pub human_support: u64,
    pub judgment_support: u64,
    pub human_contradiction: u64,
    pub judgment_contradiction: u64,
    pub related: u64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct ClaimEvidenceProjection {
    pub claim: Object,
    pub supporting: Vec<EvidenceProjectionItem>,
    pub contradicting: Vec<EvidenceProjectionItem>,
    pub related: Vec<EvidenceProjectionItem>,
    pub summary: EvidenceProjectionSummary,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct MetricCount {
    pub name: String,
    pub count: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct ProtocolHealthMetrics {
    pub identities: u64,
    pub objects: u64,
    pub edges: u64,
    pub events: u64,
    pub judgments: u64,
    pub event_kinds: Vec<MetricCount>,
    pub object_kinds: Vec<MetricCount>,
    pub graph_density_per_object_micros: u64,
    pub event_dag_buildable: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct SemanticHealthMetrics {
    pub providers: Vec<JudgmentProviderDescriptor>,
    pub stored_judgments: u64,
    pub definitions: Vec<MetricCount>,
    pub providers_by_version: Vec<MetricCount>,
    pub average_confidence_micros: u64,
    pub provider_disagreement_groups: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct DiscoveryHealthMetrics {
    pub indexed_objects: u64,
    pub indexed_edges: u64,
    pub searchable_text_objects: u64,
    pub claim_objects: u64,
    pub evidence_edges: u64,
    pub social_edges: u64,
    pub derived_judgment_edges: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct CapabilityHealthMetrics {
    pub definitions: u64,
    pub grants_issued: u64,
    pub active_grants: u64,
    pub revoked_grants: u64,
    pub grants_by_capability: Vec<MetricCount>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct PersonalizationSyncHealthMetrics {
    pub encrypted_envelopes: u64,
    pub recipient_devices: u64,
    pub ciphertext_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, JsonSchema)]
pub struct ObservabilitySnapshot {
    pub at: Timestamp,
    pub protocol: ProtocolHealthMetrics,
    pub runtime: SurfaceRuntimeHealthSnapshot,
    pub semantic: SemanticHealthMetrics,
    pub discovery: DiscoveryHealthMetrics,
    pub capabilities: CapabilityHealthMetrics,
    pub personalization_sync: PersonalizationSyncHealthMetrics,
    pub privacy_notes: Vec<String>,
}

impl Default for DiscoveryQuery {
    fn default() -> Self {
        Self {
            anchors: Vec::new(),
            search: None,
            followed_objects: BTreeSet::new(),
            limit: 50,
            exploration_slots: 5,
            lens: LensStack::new(
                "babel.lens.stack.balanced.v1",
                vec![
                    LensWeight {
                        lens: BuiltInLens::Research,
                        weight: 0.36,
                    },
                    LensWeight {
                        lens: BuiltInLens::IntellectualSerendipity,
                        weight: 0.24,
                    },
                    LensWeight {
                        lens: BuiltInLens::Emerging,
                        weight: 0.16,
                    },
                    LensWeight {
                        lens: BuiltInLens::Contradictions,
                        weight: 0.10,
                    },
                    LensWeight {
                        lens: BuiltInLens::Following,
                        weight: 0.08,
                    },
                    LensWeight {
                        lens: BuiltInLens::Weird,
                        weight: 0.06,
                    },
                ],
            ),
        }
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CapabilityBindingUsage {
    pub requested_bytes: u64,
    pub realtime_connections: u32,
    pub windows: Vec<CapabilityUsageWindow>,
}

pub struct LocalNode<P> {
    state: MemoryState,
    store: FileStore,
    judgment_cache: JudgmentCache,
    judgment_provider: P,
    ranking_provider: Box<dyn RankingProvider>,
    temporal_provider: Box<dyn TemporalProvider>,
    capability_broker: CapabilityBroker,
    realtime: RealtimeHub,
    surface_sessions: BTreeMap<SurfaceSessionId, SurfaceSession>,
    keyring: BTreeMap<IdentityId, Keypair>,
    replies: babel_graph::RepliesIndex,
    quotes: quotes::QuotesIndex,
    publication_request: Option<babel_store::PublicationRequest>,
    invocation_epoch: Hash,
    executing_invocation: Option<babel_capabilities::invocation::InvocationRecord>,
    author_objects: profiles::AuthorObjectsIndex,
    discovery_index: retrieval::DiscoveryIndex,
    moderators: BTreeSet<IdentityId>,
}

impl<P> LocalNode<P>
where
    P: JudgmentProvider,
{
    pub fn open(store_root: impl Into<PathBuf>, judgment_provider: P) -> Result<Self> {
        Self::open_with_ranker(store_root, judgment_provider, Box::new(NativeRanker))
    }

    pub fn open_with_ranker(
        store_root: impl Into<PathBuf>,
        judgment_provider: P,
        ranking_provider: Box<dyn RankingProvider>,
    ) -> Result<Self> {
        Self::open_with_algorithms(
            store_root,
            judgment_provider,
            ranking_provider,
            Box::new(NativeTemporalScorer),
        )
    }

    pub fn open_with_algorithms(
        store_root: impl Into<PathBuf>,
        judgment_provider: P,
        ranking_provider: Box<dyn RankingProvider>,
        temporal_provider: Box<dyn TemporalProvider>,
    ) -> Result<Self> {
        let store = FileStore::open(store_root)?;
        let mut state = MemoryState::default();
        for identity in store.list_identities()? {
            state.apply_identity(identity)?;
        }
        let mut stored_events = store.list_events()?;
        stored_events.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        for event in stored_events {
            state.apply_event(event)?;
        }
        let objects = store.list_objects()?;
        for object in &objects {
            state.apply_object(object.clone())?;
        }
        for edge in store.list_edges()? {
            state.apply_edge(edge)?;
        }
        let mut realtime = RealtimeHub::default();
        let mut realtime_events = store.list_events()?;
        realtime_events.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        for event in realtime_events {
            apply_realtime_event(&mut realtime, &event)?;
        }

        let mut node = Self {
            state,
            store,
            judgment_cache: JudgmentCache::default(),
            judgment_provider,
            ranking_provider,
            temporal_provider,
            capability_broker: CapabilityBroker::babel_default(),
            realtime,
            surface_sessions: BTreeMap::new(),
            keyring: BTreeMap::new(),
            replies: babel_graph::RepliesIndex::default(),
            quotes: quotes::QuotesIndex::default(),
            publication_request: None,
            invocation_epoch: babel_capabilities::invocation::new_context_epoch()?,
            executing_invocation: None,
            author_objects: profiles::AuthorObjectsIndex::default(),
            discovery_index: retrieval::DiscoveryIndex::default(),
            moderators: BTreeSet::new(),
        };
        for object in &objects {
            node.index_object(object);
        }
        node.restore_signing_keys()?;
        node.invalidate_restarted_invocations()?;
        node.verify_following()?;
        node.verify_safety()?;
        node.verify_moderation()?;
        node.verify_reactions()?;
        Ok(node)
    }

    pub fn create_identity(&mut self, kind: IdentityKind, handle: &str) -> Result<Identity> {
        self.check_ready()?;
        let keypair = Keypair::generate();
        let identity = Identity::create(kind, handle, &keypair)?;
        let event_id = self.state.insert_identity(identity.clone(), &keypair)?;
        let event = self
            .state
            .event(&event_id)
            .ok_or_else(|| babel_types::Error::NotFound(event_id.to_string()))?;

        self.store.put_identity(&identity)?;
        self.persist_signing_key(&keypair)?;
        self.put_event_record(event)?;
        self.keyring.insert(identity.id.clone(), keypair);
        Ok(identity)
    }

    pub fn import_signing_identity(
        &mut self,
        identity: Identity,
        keypair: Keypair,
    ) -> Result<EventId> {
        self.check_ready()?;
        identity.verify()?;
        let event_id = self.state.insert_identity(identity.clone(), &keypair)?;
        let event = self
            .state
            .event(&event_id)
            .ok_or_else(|| babel_types::Error::NotFound(event_id.to_string()))?;

        self.store.put_identity(&identity)?;
        self.persist_signing_key(&keypair)?;
        self.put_event_record(event)?;
        self.keyring.insert(identity.id.clone(), keypair);
        Ok(event_id)
    }

    pub fn attach_signing_keypair(
        &mut self,
        identity_id: &IdentityId,
        keypair: Keypair,
    ) -> Result<()> {
        self.check_ready()?;
        let identity = self.state.signing_identity(identity_id)?;
        if keypair.public_key() != identity.public_key {
            return Err(babel_types::Error::Signature);
        }
        self.keyring.insert(identity_id.clone(), keypair);
        Ok(())
    }

    pub fn rotate_identity_key(
        &mut self,
        identity_id: &IdentityId,
        scope: IdentityKeyScope,
        expires_at: Option<babel_types::Timestamp>,
        reason: &str,
    ) -> Result<(IdentityKeyTransition, Event)> {
        self.check_ready()?;
        let actor_before = self.state.signing_identity(identity_id)?;
        let previous_keypair = self.local_keypair(identity_id)?.clone();
        if previous_keypair.public_key() != actor_before.public_key {
            return Err(babel_types::Error::Signature);
        }
        let next_keypair = Keypair::generate();
        let transition = IdentityKeyTransition::create(
            identity_id.clone(),
            self.identity_key_transitions(identity_id).len() as u64 + 1,
            scope,
            &previous_keypair,
            &next_keypair,
            expires_at,
            reason,
        )?;
        let event = Event::new(
            &actor_before,
            EventKind::IdentityKeyTransition,
            EventTarget::Identity(identity_id.clone()),
            serde_json::to_value(&transition)
                .map_err(|err| babel_types::Error::Canonical(err.to_string()))?,
            self.latest_event_ids(2)?,
        )?
        .sign(&actor_before, &previous_keypair)?;

        self.persist_signing_key(&next_keypair)?;
        self.store.put_event(&event, &actor_before)?;
        self.state.apply_event(event.clone())?;
        self.keyring.insert(identity_id.clone(), next_keypair);
        self.reindex_author_quotes(identity_id);
        Ok((transition, event))
    }

    pub fn import_bundle(&mut self, bundle: ImportBundle) -> Result<ImportReport> {
        self.check_ready()?;
        let mut report = ImportReport::default();
        let mut events = bundle.events;

        for identity in bundle.identities {
            let is_new = self.identity(&identity.id).is_none();
            self.state.apply_identity(identity.clone())?;
            self.store.put_identity(&identity)?;
            if is_new {
                report.identities += 1;
            }
        }

        events.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        let existing_events = self
            .store
            .list_events()?
            .into_iter()
            .map(|event| event.id)
            .collect::<BTreeSet<_>>();
        let incoming_events = events
            .iter()
            .map(|event| event.id.clone())
            .collect::<BTreeSet<_>>();
        let mut preapplied_events = BTreeSet::new();
        for event in events
            .iter()
            .filter(|event| event.kind == EventKind::IdentityKeyTransition)
        {
            self.validate_import_event(event, &existing_events, &incoming_events)?;
            let duplicate = self.state.event(&event.id).is_some();
            self.state.apply_event(event.clone())?;
            self.reindex_author_quotes(&event.actor);
            if !duplicate {
                preapplied_events.insert(event.id.clone());
            }
        }

        for object in bundle.objects {
            let is_new = self.object(&object.id).is_none();
            if self
                .object(&object.id)
                .is_some_and(|existing| existing != &object)
            {
                return Err(babel_types::Error::Conflict(format!(
                    "object id conflict: {}", object.id
                )));
            }
            self.put_object_record(&object)?;
            self.state.apply_object(object.clone())?;
            self.index_object(&object);
            if is_new {
                report.objects += 1;
            }
        }

        for edge in bundle.edges {
            edge.author
                .as_ref()
                .ok_or(babel_types::Error::UnsignedEdge)?;
            let is_new = self.edge(&edge.id).is_none();
            self.require_object(&edge.source)?;
            self.require_object(&edge.target)?;
            if self.edge(&edge.id).is_some_and(|existing| existing != &edge) {
                return Err(babel_types::Error::Conflict(format!(
                    "edge id conflict: {}", edge.id
                )));
            }
            self.put_edge_record(&edge)?;
            self.state.apply_edge(edge.clone())?;
            self.index_reply_edge(&edge);
            self.index_quote_edge(&edge);
            if is_new {
                report.edges += 1;
            }
        }

        for event in events {
            self.validate_import_event(&event, &existing_events, &incoming_events)?;
            let duplicate =
                self.state.event(&event.id).is_some() && !preapplied_events.contains(&event.id);
            self.state.apply_event(event.clone())?;
            apply_realtime_event(&mut self.realtime, &event)?;
            self.put_event_record(&event)?;
            if matches!(event.kind, EventKind::CapabilityGranted | EventKind::CapabilityRevoked) {
                self.reconcile_surface_permissions()?;
            }
            if duplicate {
                report.duplicate_events += 1;
            } else {
                report.events += 1;
            }
        }

        Ok(report)
    }

    pub fn publish_text(&mut self, author_id: &IdentityId, text: &str) -> Result<Object> {
        self.check_ready()?;
        let (keypair, object) = {
            let author = self.local_identity(author_id)?;
            let keypair = self.local_keypair(author_id)?;
            let object = Object::text(author, text)?.sign(author, keypair)?;
            (keypair.clone(), object)
        };
        Ok(self
            .commit_new_object(object, EventKind::ObjectPublished, Vec::new(), &keypair)?
            .object)
    }

    pub fn publish_object_record(
        &mut self,
        author_id: &IdentityId,
        object: Object,
    ) -> Result<Object> {
        self.check_ready()?;
        if &object.author != author_id {
            return Err(babel_types::Error::Signature);
        }
        let keypair = {
            let author = self
                .state
                .signing_identity_at(author_id, object.created_at)?;
            let keypair = self.local_keypair(author_id)?;
            object.verify(&author)?;
            keypair.clone()
        };
        Ok(self
            .commit_new_object(object, EventKind::ObjectPublished, Vec::new(), &keypair)?
            .object)
    }

    pub fn publish_draft(&mut self, author_id: &IdentityId, draft: ObjectDraft) -> Result<Object> {
        self.check_ready()?;
        draft.validate()?;
        for hash in draft.required_blob_hashes() {
            if !self.store.contains_blob(&hash)? {
                return Err(babel_types::Error::NotFound(format!("media blob {}", hash)));
            }
        }
        let (keypair, object) = {
            let author = self.local_identity(author_id)?;
            let keypair = self.local_keypair(author_id)?;
            let object = draft.build_unsigned(author)?.sign(author, keypair)?;
            (keypair.clone(), object)
        };
        Ok(self
            .commit_new_object(object, EventKind::ObjectPublished, Vec::new(), &keypair)?
            .object)
    }

    pub fn fork_object(
        &mut self,
        author_id: &IdentityId,
        source: &ObjectId,
        draft: ObjectDraft,
    ) -> Result<ProvenancePublication> {
        self.check_ready()?;
        self.require_object(source)?;
        let draft = self.provenance_draft(
            draft,
            Provenance {
                parent: Some(source.clone()),
                forked_from: Some(source.clone()),
                remixed_from: Vec::new(),
            },
        )?;
        self.publish_related_draft(
            author_id,
            draft,
            EventKind::ObjectForked,
            vec![(
                source.clone(),
                Relation::Forks,
                EdgeOrigin::ApplicationAssertion,
            )],
        )
    }

    pub fn remix_object(
        &mut self,
        author_id: &IdentityId,
        sources: Vec<ObjectId>,
        draft: ObjectDraft,
    ) -> Result<ProvenancePublication> {
        self.check_ready()?;
        let sources = unique_sources(sources)?;
        for source in &sources {
            self.require_object(source)?;
        }
        let draft = self.provenance_draft(
            draft,
            Provenance {
                parent: None,
                forked_from: None,
                remixed_from: sources.clone(),
            },
        )?;
        self.publish_related_draft(
            author_id,
            draft,
            EventKind::ObjectRemixed,
            sources
                .into_iter()
                .map(|source| (source, Relation::Remixes, EdgeOrigin::ApplicationAssertion))
                .collect(),
        )
    }

    fn provenance_draft(&self, draft: ObjectDraft, provenance: Provenance) -> Result<ObjectDraft> {
        if draft.provenance.parent.is_some()
            || draft.provenance.forked_from.is_some()
            || !draft.provenance.remixed_from.is_empty()
        {
            return Err(babel_types::Error::Conflict(
                "fork and remix requests must not predeclare provenance".to_string(),
            ));
        }
        draft.with_provenance(provenance)
    }

    pub fn put_media_blob(&self, media_type: &str, bytes: &[u8]) -> Result<MediaBlob> {
        self.check_ready()?;
        if bytes.is_empty() {
            return Err(babel_types::Error::Conflict(
                "media blob must not be empty".to_string(),
            ));
        }
        let blob = MediaBlob::from_bytes(media_type, bytes)?;
        let stored = self.store.put_blob(bytes)?;
        if stored != blob.integrity {
            return Err(babel_types::Error::Conflict(format!(
                "stored blob hash mismatch: expected {} got {}",
                blob.integrity, stored
            )));
        }
        Ok(blob)
    }

    pub fn media_blob(
        &self,
        hash: &Hash,
        media_type: &str,
    ) -> Result<Option<(MediaBlob, Vec<u8>)>> {
        self.check_ready()?;
        let Some(bytes) = self.store.get_blob(hash)? else {
            return Ok(None);
        };
        let blob = MediaBlob::from_bytes(media_type, &bytes)?;
        if &blob.integrity != hash {
            return Err(babel_types::Error::Conflict(format!(
                "media blob integrity mismatch: {}",
                hash
            )));
        }
        Ok(Some((blob, bytes)))
    }

    pub fn media_blob_bounded(
        &self,
        hash: &Hash,
        media_type: &str,
        max_bytes: usize,
    ) -> std::result::Result<Option<(MediaBlob, Vec<u8>)>, babel_store::BlobReadError> {
        self.check_ready()?;
        let media_type = babel_media::normalize_media_type(media_type.to_owned())?;
        let Some(bytes) = self.store.get_blob_bounded(hash, max_bytes)? else {
            return Ok(None);
        };
        // FileStore has verified these exact bytes against the requested hash.
        let blob = MediaBlob::from_hash(media_type, hash.clone(), bytes.len() as u64)?;
        Ok(Some((blob, bytes)))
    }

    pub fn publish_media_object(
        &mut self,
        author_id: &IdentityId,
        title: &str,
        description: Option<String>,
        resources: Vec<MediaBlob>,
    ) -> Result<Object> {
        self.check_ready()?;
        for resource in &resources {
            if !self.store.contains_blob(&resource.integrity)? {
                return Err(babel_types::Error::NotFound(format!(
                    "media blob {}",
                    resource.integrity
                )));
            }
        }
        let payload = MediaObjectPayload::new(title, description, resources)?;
        let object_resources = payload.object_resources();
        let (keypair, object) = {
            let author = self.local_identity(author_id)?;
            let keypair = self.local_keypair(author_id)?;
            let object = Object::create(
                author,
                ObjectKind::new("babel.media"),
                "babel.schema.media.v1",
                serde_json::to_value(&payload)
                    .map_err(|err| babel_types::Error::Canonical(err.to_string()))?,
            )?
            .with_resources(object_resources)?
            .sign(author, keypair)?;
            (keypair.clone(), object)
        };
        Ok(self
            .commit_new_object(object, EventKind::ObjectPublished, Vec::new(), &keypair)?
            .object)
    }

    pub fn publish_edge(
        &mut self,
        author_id: &IdentityId,
        source: ObjectId,
        target: ObjectId,
        relation: Relation,
        origin: EdgeOrigin,
    ) -> Result<Edge> {
        self.check_ready()?;
        self.require_object(&source)?;
        self.require_object(&target)?;

        let (keypair, edge) = {
            let author = self.local_identity(author_id)?;
            let keypair = self.local_keypair(author_id)?;
            let edge = Edge::new(source, target, relation, origin, Some(author.id.clone()))?
                .sign(author, keypair)?;
            (keypair.clone(), edge)
        };
        self.commit_new_edge(edge, &keypair)
    }

    pub fn publish_edge_draft(&mut self, author_id: &IdentityId, draft: EdgeDraft) -> Result<Edge> {
        self.check_ready()?;
        self.require_object(&draft.source)?;
        self.require_object(&draft.target)?;

        let (keypair, edge) = {
            let author = self.local_identity(author_id)?;
            let keypair = self.local_keypair(author_id)?;
            let edge = draft.sign(author, keypair)?;
            (keypair.clone(), edge)
        };
        self.commit_new_edge(edge, &keypair)
    }

    pub fn infer_relationship_edge(
        &mut self,
        author_id: &IdentityId,
        source: &ObjectId,
        target: &ObjectId,
        relation: &str,
        min_score: f64,
    ) -> Result<(Edge, Judgment)> {
        self.check_ready()?;
        let source_object = self.require_object(source)?.clone();
        let target_object = self.require_object(target)?.clone();
        self.require_interaction(author_id, &source_object.author)?;
        self.require_interaction(author_id, &target_object.author)?;
        let relation = normalize_relationship_relation(relation)?;
        let mut parameters = BTreeMap::new();
        parameters.insert(
            "relation".to_string(),
            Value::String(relationship_parameter(&relation).to_string()),
        );
        let request = JudgmentRequest {
            definition: DefinitionId::relationship_v1(),
            state: relationship_judgment_state(&source_object, &target_object),
            parameters,
        };
        let orchestration = JudgmentOrchestrator::single(&self.judgment_provider)
            .evaluate(&mut self.judgment_cache, &request)?;
        let judgment = orchestration.judgment;
        let score = judgment
            .output
            .get("score")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        let min_score = min_score.clamp(0.0, 1.0);
        if score < min_score {
            return Err(babel_types::Error::Conflict(format!(
                "relationship Judgment score {score:.3} below required threshold {min_score:.3}"
            )));
        }
        self.store.put_judgment(&judgment)?;
        let mut metadata = BTreeMap::new();
        metadata.insert(
            "judgment_id".to_string(),
            Value::String(judgment.id.to_string()),
        );
        metadata.insert(
            "definition".to_string(),
            Value::String(judgment.definition.as_str().to_string()),
        );
        metadata.insert("score".to_string(), serde_json::json!(score));
        metadata.insert(
            "confidence".to_string(),
            serde_json::json!(judgment.confidence),
        );
        metadata.insert(
            "provider".to_string(),
            Value::String(judgment.provider.provider.clone()),
        );
        metadata.insert(
            "model".to_string(),
            Value::String(judgment.provider.model.clone()),
        );
        metadata.insert(
            "model_version".to_string(),
            Value::String(judgment.provider.version.clone()),
        );
        metadata.insert(
            "evaluated_relation".to_string(),
            Value::String(relationship_parameter(&relation).to_string()),
        );

        let (keypair, edge) = {
            let author = self.local_identity(author_id)?;
            let keypair = self.local_keypair(author_id)?;
            let edge = Edge::new(
                source.clone(),
                target.clone(),
                relation,
                EdgeOrigin::JudgmentDerived,
                Some(author.id.clone()),
            )?
            .with_metadata(metadata)?
            .sign(author, keypair)?;
            (keypair.clone(), edge)
        };
        let event_id = self.state.publish_edge(edge.clone(), &keypair)?;
        self.index_reply_edge(&edge);
        self.index_quote_edge(&edge);
        let event = self
            .state
            .event(&event_id)
            .ok_or_else(|| babel_types::Error::NotFound(event_id.to_string()))?;

        self.put_edge_record(&edge)?;
        self.put_event_record(event)?;
        Ok((edge, judgment))
    }

    pub fn judge_object(
        &mut self,
        object_id: &ObjectId,
        definition: DefinitionId,
        parameters: BTreeMap<String, Value>,
    ) -> Result<Judgment> {
        self.check_ready()?;
        Ok(self
            .judge_object_orchestrated(object_id, definition, parameters)?
            .judgment)
    }

    pub fn judge_object_orchestrated(
        &mut self,
        object_id: &ObjectId,
        definition: DefinitionId,
        parameters: BTreeMap<String, Value>,
    ) -> Result<OrchestratedJudgment> {
        self.check_ready()?;
        let object = self.require_object(object_id)?;
        let prepared = self.prepare_object_judgment(object, definition, parameters)?;
        let orchestration = prepared.orchestration.clone();
        self.commit_object_judgments(vec![prepared])?;
        Ok(orchestration)
    }

    pub fn capability_judge_object(
        &mut self,
        source_object_id: &ObjectId,
        target_object_id: &ObjectId,
        definition: DefinitionId,
        parameters: BTreeMap<String, Value>,
        grant_ids: &[String],
    ) -> Result<(OrchestratedJudgment, CapabilityReceipt)> {
        self.check_ready()?;
        let receipt = self.authorize_ai_judge(
            source_object_id,
            target_object_id,
            &definition,
            &parameters,
            grant_ids,
        )?;
        let orchestration =
            self.judge_object_orchestrated(target_object_id, definition, parameters)?;
        Ok((orchestration, receipt))
    }

    fn authorize_ai_judge(
        &self,
        source_object_id: &ObjectId,
        target_object_id: &ObjectId,
        definition: &DefinitionId,
        parameters: &BTreeMap<String, Value>,
        grant_ids: &[String],
    ) -> Result<CapabilityReceipt> {
        self.require_moderation_execution(source_object_id)?;
        self.require_object(source_object_id)?;
        self.require_object(target_object_id)?;
        if grant_ids.is_empty() {
            return Err(babel_types::Error::Conflict(
                "missing capability grant binding for babel.ai.judge@1".to_string(),
            ));
        }
        let bound_grants = grant_ids
            .iter()
            .map(|id| {
                let id = CapabilityGrantId::new_unchecked(id.clone());
                id.validate()?;
                Ok(id)
            })
            .collect::<Result<BTreeSet<_>>>()?;
        let capability = CapabilityId::new("babel.ai.judge")?;
        let grants = self.capability_grants(source_object_id)?;
        let now = babel_types::Timestamp::now();
        for grant in &grants {
            if bound_grants.contains(&grant.id)
                && &grant.object_id == source_object_id
                && grant.capability == capability
                && grant.version == 1
                && grant.decision == GrantDecision::Approved
                && grant.revoked_at.is_none()
                && grant.expires_at.is_none_or(|expires_at| expires_at > now)
                && ai_judge_scope_allows(&grant.scope, target_object_id, definition)
            {
                return self.capability_broker.authorize_call_with_usage(
                    CapabilityCall {
                        object_id: source_object_id.clone(),
                        capability,
                        version: 1,
                        scope: grant.scope.clone(),
                        requested_bytes: ai_judge_request_bytes(
                            target_object_id,
                            definition,
                            parameters,
                        )?,
                        realtime_connections: 0,
                    },
                    std::slice::from_ref(grant),
                    &[],
                    now,
                );
            }
        }
        Err(babel_types::Error::Conflict(format!(
            "no active babel.ai.judge grant permits {} for {}",
            definition.as_str(),
            target_object_id
        )))
    }

    pub fn ingest_object_judgments(&mut self, object_id: &ObjectId) -> Result<Vec<Judgment>> {
        self.check_ready()?;
        let prepared = self.prepare_object_judgments(self.require_object(object_id)?)?;
        let mut judgments = self.commit_object_judgments(prepared)?;
        judgments.sort_by(|left, right| {
            left.definition
                .cmp(&right.definition)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(judgments)
    }

    pub fn publish_checkpoint(
        &mut self,
        author_id: &IdentityId,
        validators: ValidatorSet,
    ) -> Result<Event> {
        self.check_ready()?;
        let checkpoint = self.finality_checkpoint(&validators)?;
        let parent = checkpoint.last_finalized_event.clone();
        let event = {
            let author = self.local_identity(author_id)?;
            let keypair = self.local_keypair(author_id)?;
            Event::new(
                author,
                EventKind::ConsensusCheckpoint,
                EventTarget::Network,
                serde_json::to_value(&checkpoint)
                    .map_err(|err| babel_types::Error::Canonical(err.to_string()))?,
                vec![parent],
            )?
            .sign(author, keypair)?
        };

        let mut dag = self.event_dag()?;
        dag.insert(event.clone())?;
        self.put_event_record(&event)?;
        self.state.apply_event(event.clone())?;
        Ok(event)
    }

    pub fn finality_checkpoint(&self, validators: &ValidatorSet) -> Result<FinalityCheckpoint> {
        self.check_ready()?;
        self.event_dag()?.finality_checkpoint(validators)
    }

    pub fn identity(&self, id: &IdentityId) -> Option<&Identity> {
        self.state.identity(id)
    }

    pub fn signing_identity(&self, id: &IdentityId) -> Result<Identity> {
        self.check_ready()?;
        self.state.signing_identity(id)
    }

    pub fn identity_key_transitions(&self, id: &IdentityId) -> Vec<IdentityKeyTransition> {
        self.state.identity_key_transitions(id)
    }

    pub fn put_personalization_sync_envelope(
        &self,
        envelope: EncryptedLocalUserModel,
    ) -> Result<PersonalizationSyncRecord> {
        self.check_ready()?;
        envelope.validate()?;
        if self.identity(&envelope.recipient.identity_id).is_none() {
            return Err(babel_types::Error::NotFound(
                envelope.recipient.identity_id.to_string(),
            ));
        }
        self.store.put_personalization_sync_envelope(envelope)
    }

    pub fn list_personalization_sync_envelopes(
        &self,
        identity_id: &IdentityId,
        device_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PersonalizationSyncRecord>> {
        self.check_ready()?;
        if self.identity(identity_id).is_none() {
            return Err(babel_types::Error::NotFound(identity_id.to_string()));
        }
        self.store
            .list_personalization_sync_envelopes(identity_id, device_id, limit.min(500))
    }

    pub fn personalization_sync_envelope(
        &self,
        identity_id: &IdentityId,
        device_id: &str,
        envelope_hash: &Hash,
    ) -> Result<PersonalizationSyncRecord> {
        self.check_ready()?;
        if self.identity(identity_id).is_none() {
            return Err(babel_types::Error::NotFound(identity_id.to_string()));
        }
        self.store
            .get_personalization_sync_envelope(identity_id, device_id, envelope_hash)?
            .ok_or_else(|| babel_types::Error::NotFound(envelope_hash.to_string()))
    }

    pub fn delete_personalization_sync_envelope(
        &self,
        identity_id: &IdentityId,
        device_id: &str,
        envelope_hash: &Hash,
    ) -> Result<Option<PersonalizationSyncRecord>> {
        self.check_ready()?;
        if self.identity(identity_id).is_none() {
            return Err(babel_types::Error::NotFound(identity_id.to_string()));
        }
        self.store
            .delete_personalization_sync_envelope(identity_id, device_id, envelope_hash)
    }

    pub fn object(&self, id: &ObjectId) -> Option<&Object> {
        self.state.object(id)
    }

    pub fn search_objects(&self, query: ObjectSearchQuery) -> Result<Vec<ObjectSearchResult>> {
        self.check_ready()?;
        let (_, restricted) = self.store.moderation_restrictions()?;
        self.discovery_index
            .search(&query, &restricted)
            .into_iter()
            .map(|(id, score, reasons)| {
                Ok(ObjectSearchResult {
                    object: self.require_object(&id)?.clone(),
                    score,
                    reasons,
                })
            })
            .collect()
    }

    pub fn discover_objects(&mut self, query: DiscoveryQuery) -> Result<DiscoveryResult> {
        self.discover_objects_at(query, Timestamp::now())
    }

    /// Explicit evaluation time makes replay independent of publication activity.
    pub fn discover_objects_at(
        &mut self,
        query: DiscoveryQuery,
        reference_time: Timestamp,
    ) -> Result<DiscoveryResult> {
        self.check_ready()?;
        babel_discovery::TemporalRequest {
            reference_time,
            items: Vec::new(),
        }
        .validate()?;
        if query.anchors.len() > 64
            || query.followed_objects.len() > 200
            || query
                .search
                .as_ref()
                .is_some_and(|q| q.len() > 256 || q.chars().any(char::is_control))
        {
            return Err(babel_types::Error::Canonical(
                "discovery query exceeds limits".into(),
            ));
        }
        for id in query.anchors.iter().chain(&query.followed_objects) {
            id.validate()?;
        }
        let limit = if query.limit == 0 {
            50
        } else {
            query.limit.min(200)
        };
        RankingRequest {
            candidates: Vec::new(),
            lens: query.lens.clone(),
            diversity: DiversityPolicy::default(),
            limit,
        }
        .validate()?;
        let mut search_relevance = BTreeMap::new();
        let mut search_objects = None;
        if query
            .search
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            let search_results = self.search_objects(ObjectSearchQuery {
                query: query.search.clone(),
                author: None,
                kind: None,
                limit: 200,
            })?;
            let max_score = search_results
                .iter()
                .map(|result| result.score)
                .max()
                .unwrap_or(1)
                .max(1) as f64;
            let mut matches = Vec::with_capacity(search_results.len());
            for result in search_results {
                search_relevance.insert(
                    result.object.id.clone(),
                    (result.score as f64 / max_score).clamp(0.0, 1.0),
                );
                matches.push(result.object);
            }
            search_objects = Some(matches);
        }

        let (_, restricted) = self.store.moderation_restrictions()?;
        let admission = self.retrieve(
            &query, &restricted, search_objects.as_deref(), query.exploration_slots.min(limit),
        );
        let objects = admission.members.keys()
            .map(|id| self.require_object(id).cloned())
            .collect::<Result<Vec<_>>>()?;
        let population = self.discovery_index.population(&objects, &restricted);
        let (summaries, temporal_scores) =
            self.discovery_signals(&objects, &query, &search_relevance, reference_time, &population)?;

        let mut candidates = admission.candidates(&summaries);
        babel_discovery::CandidateEngine.annotate_admitted(
            &mut candidates, &summaries, query.exploration_slots.min(limit),
        );
        let ranking_request = RankingRequest {
            candidates,
            lens: query.lens,
            diversity: DiversityPolicy::default(),
            limit,
        };
        ranking_request.validate()?;
        let result = self.ranking_provider.rank(&ranking_request)?;
        result
            .validate_for(&ranking_request, &self.ranking_provider.version())
            .map_err(|_| {
                babel_types::Error::ProviderUnavailable(
                    "ranking provider returned invalid output".into(),
                )
            })?;
        let babel_lens::RankingResult {
            ranked,
            trace,
            diversity_trace,
            provider,
        } = result;
        let by_id = objects
            .into_iter()
            .map(|object| (object.id.clone(), object))
            .collect::<BTreeMap<_, _>>();
        let temporal = TemporalResult {
            provider: self.temporal_provider.version(),
            reference_time,
            scores: ranked
                .iter()
                .map(|ranked| temporal_scores[&ranked.candidate.object_id].clone())
                .collect(),
        };
        let objects = ranked
            .iter()
            .filter_map(|ranked| by_id.get(&ranked.candidate.object_id).cloned())
            .collect();

        Ok(DiscoveryResult {
            ranked,
            objects,
            trace,
            diversity_trace,
            ranking_provider: provider,
            temporal,
        })
    }

    pub fn capability_manifest(&self, object_id: &ObjectId) -> Result<CapabilityManifest> {
        self.check_ready()?;
        let object = self.require_object(object_id)?;
        Ok(self.capability_broker.manifest(object))
    }

    pub fn capability_definitions(&self) -> Vec<CapabilityDefinition> {
        self.capability_broker.definitions()
    }

    pub fn capability_decisions(&self, object_id: &ObjectId) -> Result<Vec<CapabilityDecision>> {
        self.check_ready()?;
        let object = self.require_object(object_id)?;
        let grants = self.capability_grants(object_id)?;
        Ok(self.capability_broker.evaluate_object(object, &grants))
    }

    pub fn capability_grants(&self, object_id: &ObjectId) -> Result<Vec<CapabilityGrant>> {
        self.check_ready()?;
        Ok(grants::project_grants(&self.store.list_events()?)?
            .into_values()
            .filter(|(_, grant)| &grant.object_id == object_id)
            .map(|(_, grant)| grant)
            .collect())
    }

    pub fn authorize_capability_binding(
        &self,
        object_id: &ObjectId,
        capability: &str,
        version: u32,
        grant_ids: &[String],
    ) -> Result<CapabilityReceipt> {
        self.check_ready()?;
        self.authorize_capability_binding_with_usage(
            object_id,
            capability,
            version,
            grant_ids,
            CapabilityBindingUsage::default(),
        )
    }

    pub fn authorize_capability_binding_with_usage(
        &self,
        object_id: &ObjectId,
        capability: &str,
        version: u32,
        grant_ids: &[String],
        usage: CapabilityBindingUsage,
    ) -> Result<CapabilityReceipt> {
        self.check_ready()?;
        self.require_moderation_execution(object_id)?;
        if grant_ids.is_empty() {
            return Err(babel_types::Error::Conflict(format!(
                "missing capability grant binding for {capability}@{version}"
            )));
        }
        let capability = CapabilityId::new(capability)?;
        let grant_ids = grant_ids
            .iter()
            .map(|id| {
                let id = CapabilityGrantId::new_unchecked(id.clone());
                id.validate()?;
                Ok(id)
            })
            .collect::<Result<BTreeSet<_>>>()?;
        let grants = self.capability_grants(object_id)?;
        let now = babel_types::Timestamp::now();
        for grant in &grants {
            if grant_ids.contains(&grant.id)
                && &grant.object_id == object_id
                && grant.capability == capability
                && grant.version == version
                && grant.decision == GrantDecision::Approved
                && grant.revoked_at.is_none()
                && grant.expires_at.is_none_or(|expires_at| expires_at > now)
            {
                return self.capability_broker.authorize_call_with_usage(
                    CapabilityCall {
                        object_id: object_id.clone(),
                        capability,
                        version,
                        scope: grant.scope.clone(),
                        requested_bytes: usage.requested_bytes,
                        realtime_connections: usage.realtime_connections,
                    },
                    std::slice::from_ref(grant),
                    &usage.windows,
                    now,
                );
            }
        }
        Err(babel_types::Error::Conflict(format!(
            "no active grant binding satisfies {}@{version}",
            capability.as_str()
        )))
    }

    fn authorize_sensitive_capability(
        &self,
        object_id: &ObjectId,
        capability: &str,
        version: u32,
        grant_ids: &[String],
        scope_allows: impl Fn(&Value) -> bool,
        requested_bytes: u64,
        realtime_connections: u32,
        denied_message: String,
    ) -> Result<CapabilityReceipt> {
        self.require_moderation_execution(object_id)?;
        if grant_ids.is_empty() {
            return Err(babel_types::Error::Conflict(format!(
                "missing capability grant binding for {capability}@{version}"
            )));
        }
        let capability = CapabilityId::new(capability)?;
        let grant_ids = grant_ids
            .iter()
            .map(|id| {
                let id = CapabilityGrantId::new_unchecked(id.clone());
                id.validate()?;
                Ok(id)
            })
            .collect::<Result<BTreeSet<_>>>()?;
        let grants = self.capability_grants(object_id)?;
        let now = babel_types::Timestamp::now();
        for grant in &grants {
            if grant_ids.contains(&grant.id)
                && &grant.object_id == object_id
                && grant.capability == capability
                && grant.version == version
                && grant.decision == GrantDecision::Approved
                && grant.revoked_at.is_none()
                && grant.expires_at.is_none_or(|expires_at| expires_at > now)
                && scope_allows(&grant.scope)
            {
                return self.capability_broker.authorize_call_with_usage(
                    CapabilityCall {
                        object_id: object_id.clone(),
                        capability,
                        version,
                        scope: grant.scope.clone(),
                        requested_bytes,
                        realtime_connections,
                    },
                    std::slice::from_ref(grant),
                    &[],
                    now,
                );
            }
        }
        Err(babel_types::Error::Conflict(denied_message))
    }

    pub fn clipboard_write(
        &self,
        _object_id: &ObjectId,
        _text: &str,
        _grant_ids: &[String],
    ) -> Result<CapabilityReceipt> {
        self.check_ready()?;
        Err(babel_types::Error::Conflict("clipboard requires a one-use browser invocation".into()))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn payments_checkout(
        &self,
        object_id: &ObjectId,
        merchant_id: Option<&str>,
        merchant_name: &str,
        currency: &str,
        total_amount_minor: u64,
        line_items: &[(String, u64, u32)],
        success_url: Option<&str>,
        cancel_url: Option<&str>,
        reference: Option<&str>,
        grant_ids: &[String],
    ) -> Result<CapabilityReceipt> {
        self.check_ready()?;
        self.require_object(object_id)?;
        validate_payment_checkout(
            merchant_id,
            merchant_name,
            currency,
            total_amount_minor,
            line_items,
            success_url,
            cancel_url,
            reference,
        )?;
        self.authorize_sensitive_capability(
            object_id,
            "babel.payments.checkout",
            1,
            grant_ids,
            |scope| payment_scope_allows(scope, merchant_id, currency, total_amount_minor),
            payment_request_bytes(
                merchant_id,
                merchant_name,
                currency,
                total_amount_minor,
                line_items,
                success_url,
                cancel_url,
                reference,
            )?,
            0,
            format!(
                "no active babel.payments.checkout grant permits {currency} {total_amount_minor}"
            ),
        )
    }

    pub fn notifications_request(
        &self,
        object_id: &ObjectId,
        purpose: &str,
        categories: &[String],
        grant_ids: &[String],
    ) -> Result<CapabilityReceipt> {
        self.check_ready()?;
        self.require_object(object_id)?;
        validate_notifications_request(purpose, categories)?;
        self.authorize_sensitive_capability(
            object_id,
            "babel.notifications.request",
            1,
            grant_ids,
            |scope| notification_scope_allows(scope, purpose, categories),
            notification_request_bytes(purpose, categories),
            0,
            "no active babel.notifications.request grant permits requested categories".to_string(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn media_camera_request(
        &self,
        object_id: &ObjectId,
        purpose: &str,
        mode: &str,
        media_types: &[String],
        max_duration_ms: Option<u64>,
        facing_mode: Option<&str>,
        width: Option<u32>,
        height: Option<u32>,
        grant_ids: &[String],
    ) -> Result<CapabilityReceipt> {
        self.check_ready()?;
        self.require_object(object_id)?;
        validate_camera_request(
            purpose,
            mode,
            media_types,
            max_duration_ms,
            facing_mode,
            width,
            height,
        )?;
        self.authorize_sensitive_capability(
            object_id,
            "babel.media.camera",
            1,
            grant_ids,
            |scope| camera_scope_allows(scope, mode, media_types, max_duration_ms, facing_mode),
            media_capture_request_bytes(purpose, mode, media_types, max_duration_ms, facing_mode),
            0,
            "no active babel.media.camera grant permits requested capture".to_string(),
        )
    }

    pub fn media_microphone_request(
        &self,
        object_id: &ObjectId,
        purpose: &str,
        mode: &str,
        media_types: &[String],
        max_duration_ms: Option<u64>,
        grant_ids: &[String],
    ) -> Result<CapabilityReceipt> {
        self.check_ready()?;
        self.require_object(object_id)?;
        validate_microphone_request(purpose, mode, media_types, max_duration_ms)?;
        self.authorize_sensitive_capability(
            object_id,
            "babel.media.microphone",
            1,
            grant_ids,
            |scope| microphone_scope_allows(scope, mode, media_types, max_duration_ms),
            media_capture_request_bytes(purpose, mode, media_types, max_duration_ms, None),
            0,
            "no active babel.media.microphone grant permits requested capture".to_string(),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn ai_generate_request(
        &self,
        object_id: &ObjectId,
        purpose: &str,
        task: &str,
        prompt: &str,
        output_modalities: &[String],
        model: Option<&str>,
        max_output_tokens: Option<u32>,
        temperature_millis: Option<u32>,
        grant_ids: &[String],
    ) -> Result<CapabilityReceipt> {
        self.check_ready()?;
        self.require_object(object_id)?;
        validate_ai_generate_request(
            purpose,
            task,
            prompt,
            output_modalities,
            model,
            max_output_tokens,
            temperature_millis,
        )?;
        self.authorize_sensitive_capability(
            object_id,
            "babel.ai.generate",
            1,
            grant_ids,
            |scope| {
                ai_generate_scope_allows(
                    scope,
                    task,
                    prompt,
                    output_modalities,
                    model,
                    max_output_tokens,
                )
            },
            ai_generate_request_bytes(
                purpose,
                task,
                prompt,
                output_modalities,
                model,
                max_output_tokens,
                temperature_millis,
            ),
            0,
            "no active babel.ai.generate grant permits requested generation".to_string(),
        )
    }

    pub fn ai_embed_request(
        &self,
        object_id: &ObjectId,
        purpose: &str,
        input_modality: &str,
        inputs: &[String],
        model: Option<&str>,
        dimensions: Option<u32>,
        grant_ids: &[String],
    ) -> Result<CapabilityReceipt> {
        self.check_ready()?;
        self.require_object(object_id)?;
        validate_ai_embed_request(purpose, input_modality, inputs, model, dimensions)?;
        self.authorize_sensitive_capability(
            object_id,
            "babel.ai.embed",
            1,
            grant_ids,
            |scope| ai_embed_scope_allows(scope, input_modality, inputs, model, dimensions),
            ai_embed_request_bytes(purpose, input_modality, inputs, model, dimensions),
            0,
            "no active babel.ai.embed grant permits requested embedding".to_string(),
        )
    }

    pub fn ai_transcribe_request(
        &self,
        object_id: &ObjectId,
        purpose: &str,
        media_uri: &str,
        media_type: &str,
        model: Option<&str>,
        language: Option<&str>,
        max_duration_ms: Option<u64>,
        grant_ids: &[String],
    ) -> Result<CapabilityReceipt> {
        self.check_ready()?;
        self.require_object(object_id)?;
        validate_ai_transcribe_request(
            purpose,
            media_uri,
            media_type,
            model,
            language,
            max_duration_ms,
        )?;
        self.authorize_sensitive_capability(
            object_id,
            "babel.ai.transcribe",
            1,
            grant_ids,
            |scope| ai_transcribe_scope_allows(scope, media_type, model, language, max_duration_ms),
            ai_transcribe_request_bytes(
                purpose,
                media_uri,
                media_type,
                model,
                language,
                max_duration_ms,
            ),
            0,
            "no active babel.ai.transcribe grant permits requested transcription".to_string(),
        )
    }

    pub fn fullscreen_enter(
        &self,
        _object_id: &ObjectId,
        _target_hint: Option<&str>,
        _grant_ids: &[String],
    ) -> Result<CapabilityReceipt> {
        self.check_ready()?;
        Err(babel_types::Error::Conflict("fullscreen requires a one-use browser invocation".into()))
    }

    pub fn grant_capability(
        &mut self,
        author_id: &IdentityId,
        object_id: &ObjectId,
        request: CapabilityRequest,
        decision: GrantDecision,
    ) -> Result<Event> {
        self.grant_capability_draft(author_id, CapabilityGrantDraft {
            object_id: object_id.clone(), request, decision, expires_at: None,
        })
    }

    pub fn grant_capability_draft(
        &mut self,
        author_id: &IdentityId,
        draft: CapabilityGrantDraft,
    ) -> Result<Event> {
        self.check_ready()?;
        let object_id = {
            let object = self.require_object(&draft.object_id)?;
            if !object
                .capabilities
                .iter()
                .any(|declared| declared == &draft.request)
            {
                return Err(babel_types::Error::Conflict(format!(
                    "object {} did not declare capability {}@{} with requested scope",
                    object.id, draft.request.id, draft.request.version
                )));
            }
            object.id.clone()
        };
        self.local_identity(author_id)?;
        self.local_keypair(author_id)?;
        if let Some(event) = self.retried_consent(author_id, &object_id, EventKind::CapabilityGranted)? {
            let grant = event_grant(&event)?;
            if grant.object_id != object_id || grant.capability.as_str() != draft.request.id
                || grant.version != draft.request.version || grant.scope != draft.request.scope
                || grant.decision != draft.decision || grant.expires_at != draft.expires_at
            {
                return Err(babel_types::Error::Conflict("consent retry intent mismatch".into()));
            }
            return Ok(event);
        }
        let grant = self.capability_broker.issue_grant(
            object_id,
            draft.request,
            draft.decision,
            draft.expires_at,
        )?;
        let event = {
            let author = self.local_identity(author_id)?;
            let keypair = self.local_keypair(author_id)?;
            Event::new(
                author,
                EventKind::CapabilityGranted,
                EventTarget::Object(draft.object_id),
                serde_json::json!({ "grant": grant }),
                self.latest_event_ids(2)?,
            )?
            .sign(author, keypair)?
        };

        self.commit_consent_event(event)
    }

    pub fn revoke_capability(
        &mut self,
        author_id: &IdentityId,
        object_id: &ObjectId,
        grant_id: &CapabilityGrantId,
    ) -> Result<Event> {
        self.check_ready()?;
        if !self.grants_belong_to(object_id, author_id, &[grant_id.to_string()])? {
            return Err(babel_types::Error::Signature);
        }
        self.require_object(object_id)?;
        self.local_identity(author_id)?;
        self.local_keypair(author_id)?;
        if let Some(event) = self.retried_consent(author_id, object_id, EventKind::CapabilityRevoked)? {
            if event_grant_id(&event)? != *grant_id {
                return Err(babel_types::Error::Conflict("revocation retry intent mismatch".into()));
            }
            self.reconcile_surface_permissions()?;
            return Ok(event);
        }
        let grants = self.capability_grants(object_id)?;
        let grant = grants
            .iter()
            .find(|grant| &grant.id == grant_id && grant.revoked_at.is_none())
            .ok_or_else(|| babel_types::Error::NotFound(grant_id.to_string()))?;
        let event = {
            let author = self.local_identity(author_id)?;
            let keypair = self.local_keypair(author_id)?;
            Event::new(
                author,
                EventKind::CapabilityRevoked,
                EventTarget::Object(object_id.clone()),
                serde_json::json!({
                    "grant_id": grant.id,
                    "capability": grant.capability,
                    "version": grant.version,
                }),
                self.latest_event_ids(2)?,
            )?
            .sign(author, keypair)?
        };

        self.commit_consent_event(event)
    }

    pub fn prepare_surface(
        &self,
        object_id: &ObjectId,
        role: SurfaceRole,
    ) -> Result<SurfaceSessionPlan> {
        self.check_ready()?;
        self.require_moderation_execution(object_id)?;
        let object = self.require_object(object_id)?;
        let grants = self.capability_grants(object_id)?;
        SurfaceRuntime::new(self.capability_broker.clone()).prepare_surface(object, role, &grants)
    }

    pub fn start_surface_session(
        &mut self,
        object_id: &ObjectId,
        role: SurfaceRole,
        requested_id: Option<SurfaceSessionId>,
    ) -> Result<SurfaceSession> {
        self.check_ready()?;
        let plan = self.prepare_surface(object_id, role)?;
        let session = SurfaceRuntime::new(self.capability_broker.clone())
            .start_session(plan, requested_id)?;
        if self.surface_sessions.contains_key(&session.id) {
            return Err(babel_types::Error::Conflict(format!(
                "Surface session already exists: {}",
                session.id
            )));
        }
        self.surface_sessions
            .insert(session.id.clone(), session.clone());
        Ok(session)
    }

    pub fn surface_session(&self, session_id: &SurfaceSessionId) -> Result<SurfaceSession> {
        self.check_ready()?;
        self.surface_sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| babel_types::Error::NotFound(session_id.to_string()))
    }

    pub fn surface_runtime_health(&self) -> SurfaceRuntimeHealthSnapshot {
        SurfaceRuntimeHealthSnapshot::from_sessions(self.surface_sessions.values())
    }

    pub fn observability_snapshot(&self) -> Result<ObservabilitySnapshot> {
        self.check_ready()?;
        let identities = self.store.list_identities()?;
        let objects = self.store.list_objects()?;
        let edges = self.store.list_edges()?;
        let events = self.store.list_events()?;
        let judgments = self.store.list_judgments()?;
        let personalization_sync_records =
            identities
                .iter()
                .try_fold(Vec::new(), |mut records, identity| {
                    records.extend(self.store.list_personalization_sync_envelopes(
                        &identity.id,
                        None,
                        usize::MAX,
                    )?);
                    Ok::<_, babel_types::Error>(records)
                })?;
        let recipient_devices = personalization_sync_records
            .iter()
            .map(|record| (record.identity_id.clone(), record.device_id.clone()))
            .collect::<BTreeSet<_>>()
            .len() as u64;
        let personalization_sync_bytes =
            personalization_sync_records
                .iter()
                .try_fold(0_u64, |total, record| {
                    total.checked_add(record.size_bytes).ok_or_else(|| {
                        babel_types::Error::Conflict(
                            "personalization sync byte accounting overflow".to_string(),
                        )
                    })
                })?;
        let grants = capability_grants_from_events(&events)?;
        let active_grants = grants
            .iter()
            .filter(|grant| {
                grant.decision == GrantDecision::Approved
                    && grant.revoked_at.is_none()
                    && grant
                        .expires_at
                        .is_none_or(|expires_at| expires_at > Timestamp::now())
            })
            .count() as u64;
        let revoked_grants = grants
            .iter()
            .filter(|grant| grant.revoked_at.is_some())
            .count() as u64;
        let event_dag_buildable = self.event_dag().is_ok();

        Ok(ObservabilitySnapshot {
            at: Timestamp::now(),
            protocol: ProtocolHealthMetrics {
                identities: identities.len() as u64,
                objects: objects.len() as u64,
                edges: edges.len() as u64,
                events: events.len() as u64,
                judgments: judgments.len() as u64,
                event_kinds: count_by(events.iter().map(|event| format!("{:?}", event.kind))),
                object_kinds: count_by(
                    objects
                        .iter()
                        .map(|object| object.kind.as_str().to_string()),
                ),
                graph_density_per_object_micros: ratio_micros(edges.len(), objects.len()),
                event_dag_buildable,
            },
            runtime: self.surface_runtime_health(),
            semantic: SemanticHealthMetrics {
                providers: self.judgment_provider_descriptors(),
                stored_judgments: judgments.len() as u64,
                definitions: count_by(
                    judgments
                        .iter()
                        .map(|judgment| judgment.definition.as_str().to_string()),
                ),
                providers_by_version: count_by(judgments.iter().map(|judgment| {
                    format!(
                        "{}:{}:{}",
                        judgment.provider.provider,
                        judgment.provider.model,
                        judgment.provider.version
                    )
                })),
                average_confidence_micros: average_confidence_micros(&judgments),
                provider_disagreement_groups: provider_disagreement_groups(&judgments),
            },
            discovery: DiscoveryHealthMetrics {
                indexed_objects: objects.len() as u64,
                indexed_edges: edges.len() as u64,
                searchable_text_objects: objects
                    .iter()
                    .filter(|object| object.kind.as_str() == "babel.text")
                    .count() as u64,
                claim_objects: objects
                    .iter()
                    .filter(|object| object.kind.as_str() == "babel.claim")
                    .count() as u64,
                evidence_edges: edges
                    .iter()
                    .filter(|edge| {
                        matches!(edge.relation, Relation::EvidenceFor | Relation::EvidenceAgainst)
                    })
                    .count() as u64,
                social_edges: edges
                    .iter()
                    .filter(|edge| matches!(edge.relation, Relation::Follows))
                    .count() as u64,
                derived_judgment_edges: edges
                    .iter()
                    .filter(|edge| edge.origin == EdgeOrigin::JudgmentDerived)
                    .count() as u64,
            },
            capabilities: CapabilityHealthMetrics {
                definitions: self.capability_definitions().len() as u64,
                grants_issued: grants.len() as u64,
                active_grants,
                revoked_grants,
                grants_by_capability: count_by(
                    grants
                        .iter()
                        .map(|grant| grant.capability.as_str().to_string()),
                ),
            },
            personalization_sync: PersonalizationSyncHealthMetrics {
                encrypted_envelopes: personalization_sync_records.len() as u64,
                recipient_devices,
                ciphertext_bytes: personalization_sync_bytes,
            },
            privacy_notes: vec![
                "snapshot contains aggregate protocol, runtime, semantic, discovery, and capability metrics only".to_string(),
                "private local personalization state, encrypted personalization sync envelope bodies, and raw Object payload text are not included".to_string(),
                "remote Judgment provider boundaries are represented by provider descriptors and stored Judgment metadata".to_string(),
            ],
        })
    }

    pub fn transition_surface_session(
        &mut self,
        session_id: &SurfaceSessionId,
        lifecycle: SurfaceLifecycle,
        reason: &str,
    ) -> Result<(SurfaceSession, SurfaceRuntimeEvent)> {
        self.reconcile_surface_permissions()?;
        let session = self
            .surface_sessions
            .get_mut(session_id)
            .ok_or_else(|| babel_types::Error::NotFound(session_id.to_string()))?;
        let event = session.transition(lifecycle, reason)?;
        let session = session.clone();
        if session.lifecycle != SurfaceLifecycle::Active {
            self.invalidate_social_session(session_id.as_str(), babel_capabilities::invocation::InvocationInvalidation::ContextLost)?;
        }
        Ok((session, event))
    }

    pub fn reduce_surface_session_budget(
        &mut self,
        session_id: &SurfaceSessionId,
        budget: ResourceBudget,
        reason: &str,
    ) -> Result<(SurfaceSession, SurfaceRuntimeEvent)> {
        self.check_ready()?;
        let session = self
            .surface_sessions
            .get_mut(session_id)
            .ok_or_else(|| babel_types::Error::NotFound(session_id.to_string()))?;
        let event = session.reduce_budget(budget, reason)?;
        Ok((session.clone(), event))
    }

    pub fn schedule_surface_session(
        &self,
        session_id: &SurfaceSessionId,
        input: &SurfaceSchedulingInput,
    ) -> Result<SurfaceScheduleDecision> {
        self.check_ready()?;
        let session = self.surface_session(session_id)?;
        let mut plan = session.plan.clone();
        plan.lifecycle = session.lifecycle;
        plan.budget = session.budget;
        Ok(SurfaceScheduler::new().recommend(&plan, input))
    }

    pub fn apply_surface_schedule(
        &mut self,
        session_id: &SurfaceSessionId,
        input: &SurfaceSchedulingInput,
    ) -> Result<(
        SurfaceSession,
        SurfaceScheduleDecision,
        Vec<SurfaceRuntimeEvent>,
    )> {
        self.check_ready()?;
        let decision = self.schedule_surface_session(session_id, input)?;
        let session = self
            .surface_sessions
            .get_mut(session_id)
            .ok_or_else(|| babel_types::Error::NotFound(session_id.to_string()))?;
        let mut events = Vec::new();
        if decision.budget != session.budget {
            events.push(session.reduce_budget(
                decision.budget.clone(),
                format!("runtime scheduler: {}", decision.reason),
            )?);
        }
        if decision.lifecycle != session.lifecycle {
            events.push(session.transition(
                decision.lifecycle.clone(),
                format!("runtime scheduler: {}", decision.reason),
            )?);
        }
        let session = session.clone();
        if session.lifecycle != SurfaceLifecycle::Active {
            self.invalidate_social_session(session_id.as_str(), babel_capabilities::invocation::InvocationInvalidation::ContextLost)?;
        }
        Ok((session, decision, events))
    }

    pub fn checkpoint_surface_state(
        &mut self,
        session_id: &SurfaceSessionId,
        state: Value,
        reason: &str,
    ) -> Result<(SurfaceSession, SurfaceStateCheckpoint, SurfaceRuntimeEvent)> {
        self.reconcile_surface_permissions()?;
        let session = self
            .surface_sessions
            .get_mut(session_id)
            .ok_or_else(|| babel_types::Error::NotFound(session_id.to_string()))?;
        if session.lifecycle == SurfaceLifecycle::Evicted {
            return Err(babel_types::Error::Conflict(format!(
                "cannot checkpoint evicted Surface session {}",
                session.id
            )));
        }
        let checkpoint = SurfaceStateCheckpoint::new(
            session.id.clone(),
            session.plan.object_id.clone(),
            session.lifecycle.clone(),
            reason,
            state,
        )?;
        if checkpoint.size_bytes > session.budget.persistent_storage_bytes {
            return Err(babel_types::Error::Conflict(format!(
                "Surface state checkpoint exceeds session persistent storage budget: {} > {}",
                checkpoint.size_bytes, session.budget.persistent_storage_bytes
            )));
        }
        let checkpoint_value = serde_json::to_value(&checkpoint).map_err(|err| {
            babel_types::Error::Canonical(format!("encode Surface state checkpoint: {err}"))
        })?;
        self.store.put_object_storage(
            &checkpoint.object_id,
            surface_state_checkpoint_key(session_id),
            checkpoint_value,
        )?;
        let event = session.checkpoint_state(&checkpoint)?;
        Ok((session.clone(), checkpoint, event))
    }

    pub fn surface_state_checkpoint(
        &self,
        session_id: &SurfaceSessionId,
    ) -> Result<SurfaceStateCheckpoint> {
        self.check_ready()?;
        let session = self.surface_session(session_id)?;
        let record = self
            .store
            .get_object_storage(
                &session.plan.object_id,
                &surface_state_checkpoint_key(session_id),
            )?
            .ok_or_else(|| babel_types::Error::NotFound(session_id.to_string()))?;
        let checkpoint: SurfaceStateCheckpoint =
            serde_json::from_value(record.value).map_err(|err| {
                babel_types::Error::Canonical(format!("decode Surface state checkpoint: {err}"))
            })?;
        if checkpoint.session_id != *session_id || checkpoint.object_id != session.plan.object_id {
            return Err(babel_types::Error::Conflict(format!(
                "Surface state checkpoint is not bound to session {}",
                session_id
            )));
        }
        checkpoint.verify()?;
        Ok(checkpoint)
    }

    pub fn define_realtime_room(
        &mut self,
        author_id: &IdentityId,
        spec: RoomSpec,
    ) -> Result<Event> {
        self.check_ready()?;
        self.require_object(&spec.object_id)?;
        let spec = self.realtime.create_room(spec)?;
        let event = {
            let author = self.local_identity(author_id)?;
            let keypair = self.local_keypair(author_id)?;
            Event::new(
                author,
                EventKind::RealtimeRoomDefined,
                EventTarget::Object(spec.object_id.clone()),
                serde_json::json!({ "room": spec }),
                self.latest_event_ids(2)?,
            )?
            .sign(author, keypair)?
        };
        self.put_event_record(&event)?;
        self.state.apply_event(event.clone())?;
        Ok(event)
    }

    pub fn start_realtime_session(
        &mut self,
        author_id: &IdentityId,
        room_id: &babel_types::RealtimeRoomId,
    ) -> Result<RealtimeSession> {
        self.check_ready()?;
        let session = self.realtime.start_session(room_id, author_id.clone())?;
        let event = {
            let author = self.local_identity(author_id)?;
            let keypair = self.local_keypair(author_id)?;
            Event::new(
                author,
                EventKind::RealtimeSessionStarted,
                EventTarget::Object(session.object_id.clone()),
                serde_json::json!({ "session": session }),
                self.latest_event_ids(2)?,
            )?
            .sign(author, keypair)?
        };
        self.put_event_record(&event)?;
        self.state.apply_event(event)?;
        Ok(session)
    }

    pub fn close_realtime_session(
        &mut self,
        author_id: &IdentityId,
        session_id: &babel_types::RealtimeSessionId,
        object_id: &ObjectId,
    ) -> Result<(RealtimeSession, Event)> {
        self.check_ready()?;
        let session = self
            .realtime
            .session(session_id)
            .ok_or_else(|| babel_types::Error::NotFound(session_id.to_string()))?;
        if &session.participant != author_id {
            return Err(babel_types::Error::Signature);
        }
        if &session.object_id != object_id {
            return Err(babel_types::Error::Conflict(format!(
                "realtime session {} belongs to Object {}, not {}",
                session.id, session.object_id, object_id
            )));
        }
        let session = self.realtime.close_session(session_id)?;
        let event = {
            let author = self.local_identity(author_id)?;
            let keypair = self.local_keypair(author_id)?;
            Event::new(
                author,
                EventKind::RealtimeSessionClosed,
                EventTarget::Object(session.object_id.clone()),
                serde_json::json!({ "session": session }),
                self.latest_event_ids(2)?,
            )?
            .sign(author, keypair)?
        };
        self.put_event_record(&event)?;
        self.state.apply_event(event.clone())?;
        Ok((session, event))
    }

    pub fn publish_realtime_message(
        &mut self,
        author_id: &IdentityId,
        session_id: &babel_types::RealtimeSessionId,
        object_id: &ObjectId,
        payload: RealtimePayload,
        durable: bool,
    ) -> Result<(RealtimeMessage, Option<RealtimeSnapshot>)> {
        self.check_ready()?;
        let session = self
            .realtime
            .session(session_id)
            .ok_or_else(|| babel_types::Error::NotFound(session_id.to_string()))?;
        if &session.participant != author_id {
            return Err(babel_types::Error::Signature);
        }
        if &session.object_id != object_id {
            return Err(babel_types::Error::Conflict(format!(
                "realtime session {} belongs to Object {}, not {}",
                session.id, session.object_id, object_id
            )));
        }
        let publication = self.realtime.publish(session_id, payload, durable)?;
        if publication.message.durable {
            let object_id = self
                .realtime
                .room(&publication.message.room_id)
                .ok_or_else(|| {
                    babel_types::Error::NotFound(publication.message.room_id.to_string())
                })?
                .spec
                .object_id;
            let event = {
                let author = self.local_identity(author_id)?;
                let keypair = self.local_keypair(author_id)?;
                Event::new(
                    author,
                    EventKind::RealtimeMessageCommitted,
                    EventTarget::Object(object_id),
                    serde_json::json!({ "message": publication.message }),
                    self.latest_event_ids(2)?,
                )?
                .sign(author, keypair)?
            };
            self.put_event_record(&event)?;
            self.state.apply_event(event)?;
        }
        if let Some(snapshot) = &publication.snapshot {
            let event = {
                let author = self.local_identity(author_id)?;
                let keypair = self.local_keypair(author_id)?;
                Event::new(
                    author,
                    EventKind::RealtimeSnapshotCommitted,
                    EventTarget::Object(snapshot.object_id.clone()),
                    serde_json::json!({ "snapshot": snapshot }),
                    self.latest_event_ids(2)?,
                )?
                .sign(author, keypair)?
            };
            self.put_event_record(&event)?;
            self.state.apply_event(event)?;
        }
        Ok((publication.message, publication.snapshot))
    }

    pub fn realtime_room(&self, room_id: &babel_types::RealtimeRoomId) -> Option<RoomView> {
        self.realtime.room(room_id)
    }

    pub fn edge(&self, id: &EdgeId) -> Option<&Edge> {
        self.state.graph().get(id)
    }

    pub fn outgoing_edges(&self, source: &ObjectId) -> Vec<Edge> {
        self.state
            .graph()
            .outgoing(source)
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn incoming_edges(&self, target: &ObjectId) -> Vec<Edge> {
        self.state
            .graph()
            .incoming(target)
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn outgoing_relation(&self, source: &ObjectId, relation: &Relation) -> Vec<Edge> {
        self.state
            .graph()
            .outgoing_relation(source, relation)
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn incoming_relation(&self, target: &ObjectId, relation: &Relation) -> Vec<Edge> {
        self.state
            .graph()
            .incoming_relation(target, relation)
            .into_iter()
            .cloned()
            .collect()
    }

    pub fn traverse_graph(&self, spec: &GraphTraversalSpec) -> Result<GraphTraversal> {
        self.check_ready()?;
        self.require_object(&spec.root)?;
        Ok(self.state.graph().traverse(spec))
    }

    pub fn evidence_projection(&self, claim_id: &ObjectId) -> Result<ClaimEvidenceProjection> {
        self.check_ready()?;
        let claim = self.require_object(claim_id)?.clone();
        let mut supporting = Vec::new();
        let mut contradicting = Vec::new();
        let mut related = Vec::new();
        let mut summary = EvidenceProjectionSummary {
            human_support: 0,
            judgment_support: 0,
            human_contradiction: 0,
            judgment_contradiction: 0,
            related: 0,
        };

        for edge in self.incoming_edges(claim_id) {
            let Some(kind) = evidence_relation_kind(&edge.relation) else {
                continue;
            };
            let evidence = self.require_object(&edge.source)?.clone();
            let relationship_judgment = self.relationship_judgment_for_edge(&edge)?;
            let evidence_judgments = self.object_judgments(&evidence.id)?;
            let item = EvidenceProjectionItem {
                kind: kind.clone(),
                edge,
                evidence,
                relationship_judgment,
                evidence_judgments,
            };
            match kind {
                EvidenceRelationKind::Supports => {
                    if matches!(item.edge.origin, EdgeOrigin::JudgmentDerived) {
                        summary.judgment_support += 1;
                    } else {
                        summary.human_support += 1;
                    }
                    supporting.push(item);
                }
                EvidenceRelationKind::Contradicts => {
                    if matches!(item.edge.origin, EdgeOrigin::JudgmentDerived) {
                        summary.judgment_contradiction += 1;
                    } else {
                        summary.human_contradiction += 1;
                    }
                    contradicting.push(item);
                }
                EvidenceRelationKind::Related => {
                    summary.related += 1;
                    related.push(item);
                }
            }
        }

        sort_projection_items(&mut supporting);
        sort_projection_items(&mut contradicting);
        sort_projection_items(&mut related);

        Ok(ClaimEvidenceProjection {
            claim,
            supporting,
            contradicting,
            related,
            summary,
        })
    }

    fn evidence_signals(edges: &[Edge]) -> EvidenceSignals {
        let mut signals = EvidenceSignals::default();
        for edge in edges {
            match (&edge.relation, &edge.origin) {
                (Relation::EvidenceFor | Relation::Supports, EdgeOrigin::JudgmentDerived) => {
                    signals.judgment_support += 1.0;
                }
                (Relation::EvidenceFor | Relation::Supports, _) => {
                    signals.human_support += 1.0;
                }
                (
                    Relation::EvidenceAgainst | Relation::Contradicts,
                    EdgeOrigin::JudgmentDerived,
                ) => {
                    signals.judgment_contradiction += 1.0;
                }
                (Relation::EvidenceAgainst | Relation::Contradicts, _) => {
                    signals.human_contradiction += 1.0;
                }
                _ => {}
            }
        }
        signals
    }

    fn reputation_signals(
        incoming: &[Edge],
        outgoing: &[Edge],
        evidence: &EvidenceSignals,
        judged_quality: f64,
        moderation: f64,
    ) -> ReputationSignals {
        let support_total = evidence.human_support + evidence.judgment_support;
        let contradiction_total = evidence.human_contradiction + evidence.judgment_contradiction;
        let epistemic_accuracy = if support_total + contradiction_total == 0.0 {
            0.0
        } else {
            (evidence.human_support + 0.75 * evidence.judgment_support)
                / (support_total + contradiction_total).max(1.0)
        };

        let constructive = incoming
            .iter()
            .chain(outgoing.iter())
            .filter(|edge| {
                !matches!(edge.origin, EdgeOrigin::JudgmentDerived)
                    && matches!(
                        edge.relation,
                        Relation::References
                            | Relation::Cites
                            | Relation::Quotes
                            | Relation::Supports
                            | Relation::EvidenceFor
                            | Relation::Extends
                            | Relation::DerivesFrom
                            | Relation::Supersedes
                    )
            })
            .count() as f64;
        let creative = incoming
            .iter()
            .chain(outgoing.iter())
            .filter(|edge| {
                matches!(
                    edge.relation,
                    Relation::References
                        | Relation::Quotes
                        | Relation::Contains
                        | Relation::Extends
                        | Relation::DerivesFrom
                        | Relation::Forks
                        | Relation::Remixes
                )
            })
            .count() as f64;
        let expertise = incoming
            .iter()
            .chain(outgoing.iter())
            .filter(|edge| {
                matches!(
                    edge.relation,
                    Relation::Cites | Relation::EvidenceFor | Relation::EvidenceAgainst
                )
            })
            .count() as f64;

        ReputationSignals {
            epistemic_accuracy,
            evidence_quality: judged_quality.max(evidence.support_score()),
            social_constructiveness: (constructive / 6.0).min(1.0),
            creative_contribution: (creative / 6.0).min(1.0),
            moderation,
            domain_expertise: (expertise / 4.0).min(1.0),
        }
        .bounded()
    }

    pub fn judgment(&self, id: &babel_types::JudgmentId) -> Result<Option<Judgment>> {
        self.check_ready()?;
        self.store.get_judgment(id)
    }

    pub fn object_judgment_input(
        &self,
        id: &JudgmentId,
    ) -> Result<Option<babel_store::ObjectJudgmentInput>> {
        self.check_ready()?;
        self.store.get_object_judgment_input(id)
    }

    pub fn object_judgments(&self, object_id: &ObjectId) -> Result<Vec<Judgment>> {
        self.check_ready()?;
        let object = self.require_object(object_id)?;
        let input_hash = object_provider_judgment_state(object).canonical_hash()?;
        let mut judgments = self
            .store
            .list_judgments()?
            .into_iter()
            .filter(|judgment| {
                ingestion_definitions().contains(&judgment.definition)
                    && judgment.input_hash == input_hash
            })
            .collect::<Vec<_>>();
        for input in self.store.object_judgment_inputs(object_id)? {
            if judgments
                .iter()
                .any(|judgment| judgment.id == input.judgment_id)
            {
                continue;
            }
            let judgment = self
                .store
                .get_judgment(&input.judgment_id)?
                .ok_or_else(|| babel_types::Error::NotFound(input.judgment_id.to_string()))?;
            judgments.push(judgment);
        }
        judgments.sort_by(|left, right| {
            left.definition
                .cmp(&right.definition)
                .then_with(|| left.created_at.cmp(&right.created_at))
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(judgments)
    }

    fn relationship_judgment_for_edge(&self, edge: &Edge) -> Result<Option<Judgment>> {
        let Some(value) = edge.metadata.get("judgment_id") else {
            return Ok(None);
        };
        let Some(id) = value.as_str() else {
            return Ok(None);
        };
        let judgment_id = JudgmentId::new_unchecked(id.to_string());
        judgment_id.validate()?;
        self.store.get_judgment(&judgment_id)
    }

    pub fn judgment_provider_version(&self) -> ProviderVersion {
        self.judgment_provider.version()
    }

    pub fn ranking_provider_version(&self) -> RankingProviderVersion {
        self.ranking_provider.version()
    }

    pub fn temporal_provider_version(&self) -> TemporalProviderVersion {
        self.temporal_provider.version()
    }

    pub fn judgment_provider_descriptors(&self) -> Vec<JudgmentProviderDescriptor> {
        vec![self.judgment_provider.descriptor()]
    }

    pub fn event(&self, id: &EventId) -> Option<&Event> {
        self.state.event(id)
    }

    pub fn list_events(&self, query: EventListQuery) -> Result<EventListResult> {
        self.check_ready()?;
        let limit = if query.limit == 0 {
            100
        } else {
            query.limit.min(500)
        };
        let mut events = self.store.list_events()?;
        events.sort_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        if let Some(after) = &query.after {
            after.validate()?;
            let after_timestamp = self.event_cursor_timestamp(after)?;
            events.retain(|event| {
                event.created_at > after_timestamp
                    || (event.created_at == after_timestamp && &event.id > after)
            });
        }
        events.truncate(limit);
        let next_after = events.last().map(|event| event.id.clone());
        Ok(EventListResult { events, next_after })
    }

    pub fn event_dag(&self) -> Result<EventDag> {
        self.check_ready()?;
        let mut dag = EventDag::default();
        for identity in self.store.list_identities()? {
            dag.add_identity(identity)?;
        }
        insert_events_by_parent(dag, self.store.list_events()?)
    }

    pub fn event_bundle(&self, requested: &BTreeSet<EventId>) -> Result<ImportBundle> {
        self.check_ready()?;
        let mut event_ids = requested.clone();
        let mut cursor: Vec<_> = requested.iter().cloned().collect();

        while let Some(event_id) = cursor.pop() {
            let Some(event) = self.store.get_event(&event_id)? else {
                continue;
            };
            for parent in event.parents {
                if event_ids.insert(parent.clone()) {
                    cursor.push(parent);
                }
            }
        }

        let mut identities = BTreeMap::new();
        let mut objects = BTreeMap::new();
        let mut edges = BTreeMap::new();
        let mut events = Vec::new();

        for event_id in event_ids {
            let Some(event) = self.store.get_event(&event_id)? else {
                continue;
            };
            if let Some(identity) = self.store.get_identity(&event.actor)? {
                identities.insert(identity.id.clone(), identity);
            }
            match &event.target {
                EventTarget::Identity(identity_id) => {
                    if let Some(identity) = self.store.get_identity(identity_id)? {
                        identities.insert(identity.id.clone(), identity);
                    }
                }
                EventTarget::Object(object_id) => {
                    if let Some(object) = self.store.get_object(object_id)? {
                        if let Some(author) = self.store.get_identity(&object.author)? {
                            identities.insert(author.id.clone(), author);
                        }
                        objects.insert(object.id.clone(), object);
                    }
                }
                EventTarget::Edge(edge_id) => {
                    if let Some(edge) = self.store.get_edge(edge_id)? {
                        if let Some(author_id) = &edge.author
                            && let Some(author) = self.store.get_identity(author_id)?
                        {
                            identities.insert(author.id.clone(), author);
                        }
                        if let Some(source) = self.store.get_object(&edge.source)? {
                            if let Some(author) = self.store.get_identity(&source.author)? {
                                identities.insert(author.id.clone(), author);
                            }
                            objects.insert(source.id.clone(), source);
                        }
                        if let Some(target) = self.store.get_object(&edge.target)? {
                            if let Some(author) = self.store.get_identity(&target.author)? {
                                identities.insert(author.id.clone(), author);
                            }
                            objects.insert(target.id.clone(), target);
                        }
                        edges.insert(edge.id.clone(), edge);
                    }
                }
                EventTarget::Network => {}
            }
            events.push(event);
        }

        Ok(ImportBundle {
            identities: identities.into_values().collect(),
            objects: objects.into_values().collect(),
            edges: edges.into_values().collect(),
            events,
        })
    }

    pub fn object_bundle(&self, requested: &BTreeSet<ObjectId>) -> Result<ImportBundle> {
        self.check_ready()?;
        let mut object_ids = requested.clone();
        let mut cursor: Vec<_> = requested.iter().cloned().collect();

        while let Some(object_id) = cursor.pop() {
            let Some(object) = self.store.get_object(&object_id)? else {
                continue;
            };
            for related in object
                .provenance
                .parent
                .iter()
                .chain(object.provenance.forked_from.iter())
                .chain(object.provenance.remixed_from.iter())
            {
                if object_ids.insert(related.clone()) {
                    cursor.push(related.clone());
                }
            }
        }

        let mut edge_ids = BTreeSet::new();
        for edge in self.store.list_edges()? {
            if object_ids.contains(&edge.source) || object_ids.contains(&edge.target) {
                object_ids.insert(edge.source.clone());
                object_ids.insert(edge.target.clone());
                edge_ids.insert(edge.id.clone());
            }
        }

        let event_ids = self
            .store
            .list_events()?
            .into_iter()
            .filter_map(|event| match &event.target {
                EventTarget::Object(object_id) if object_ids.contains(object_id) => Some(event.id),
                EventTarget::Edge(edge_id) if edge_ids.contains(edge_id) => Some(event.id),
                _ => None,
            })
            .collect::<BTreeSet<_>>();

        let mut bundle = self.event_bundle(&event_ids)?;
        let mut identities = bundle
            .identities
            .into_iter()
            .map(|identity| (identity.id.clone(), identity))
            .collect::<BTreeMap<_, _>>();
        let mut objects = bundle
            .objects
            .into_iter()
            .map(|object| (object.id.clone(), object))
            .collect::<BTreeMap<_, _>>();
        let mut edges = bundle
            .edges
            .into_iter()
            .map(|edge| (edge.id.clone(), edge))
            .collect::<BTreeMap<_, _>>();

        for object_id in object_ids {
            if let Some(object) = self.store.get_object(&object_id)? {
                if let Some(author) = self.store.get_identity(&object.author)? {
                    identities.insert(author.id.clone(), author);
                }
                objects.insert(object.id.clone(), object);
            }
        }
        for edge_id in edge_ids {
            if let Some(edge) = self.store.get_edge(&edge_id)? {
                if let Some(author_id) = &edge.author
                    && let Some(author) = self.store.get_identity(author_id)?
                {
                    identities.insert(author.id.clone(), author);
                }
                edges.insert(edge.id.clone(), edge);
            }
        }

        bundle.identities = identities.into_values().collect();
        bundle.objects = objects.into_values().collect();
        bundle.edges = edges.into_values().collect();
        Ok(bundle)
    }

    pub fn store(&self) -> &FileStore {
        &self.store
    }

    fn event_cursor_timestamp(&self, id: &EventId) -> Result<babel_types::Timestamp> {
        self.store
            .get_event(id)?
            .map(|event| event.created_at)
            .ok_or_else(|| babel_types::Error::NotFound(format!("event cursor {id}")))
    }

    fn local_identity(&self, id: &IdentityId) -> Result<&Identity> {
        self.state
            .identity(id)
            .ok_or_else(|| babel_types::Error::NotFound(id.to_string()))
    }

    fn local_keypair(&self, id: &IdentityId) -> Result<&Keypair> {
        self.keyring
            .get(id)
            .ok_or_else(|| babel_types::Error::NotFound(format!("local keypair for {id}")))
    }

    fn put_object_record(&self, object: &Object) -> Result<()> {
        let author = self
            .state
            .signing_identity_at(&object.author, object.created_at)?;
        self.store.put_object(object, &author)
    }

    fn put_edge_record(&self, edge: &Edge) -> Result<()> {
        let author_id = edge
            .author
            .as_ref()
            .ok_or(babel_types::Error::UnsignedEdge)?;
        let author = self.state.signing_identity_at(author_id, edge.created_at)?;
        self.store.put_edge(edge, &author)
    }

    fn put_event_record(&self, event: &Event) -> Result<()> {
        let actor = if event.kind == EventKind::IdentityKeyTransition {
            let transition: IdentityKeyTransition =
                serde_json::from_value(event.payload.clone())
                    .map_err(|err| babel_types::Error::Canonical(err.to_string()))?;
            let identity = self
                .state
                .identity(&event.actor)
                .ok_or_else(|| babel_types::Error::NotFound(event.actor.to_string()))?;
            identity.with_signing_key(transition.previous_public_key)
        } else {
            self.state
                .signing_identity_at(&event.actor, event.created_at)?
        };
        self.store.put_event(event, &actor)
    }

    fn require_object(&self, id: &ObjectId) -> Result<&Object> {
        self.state
            .object(id)
            .ok_or_else(|| babel_types::Error::NotFound(id.to_string()))
    }

    fn validate_import_event(
        &self,
        event: &Event,
        existing_events: &BTreeSet<EventId>,
        incoming_events: &BTreeSet<EventId>,
    ) -> Result<()> {
        let actor = if event.kind == EventKind::IdentityKeyTransition {
            let transition: IdentityKeyTransition =
                serde_json::from_value(event.payload.clone())
                    .map_err(|err| babel_types::Error::Canonical(err.to_string()))?;
            let identity = self
                .state
                .identity(&event.actor)
                .ok_or_else(|| babel_types::Error::NotFound(event.actor.to_string()))?;
            identity.with_signing_key(transition.previous_public_key)
        } else {
            self.state
                .signing_identity_at(&event.actor, event.created_at)?
        };
        event.verify(&actor)?;

        for parent in &event.parents {
            if !existing_events.contains(parent) && !incoming_events.contains(parent) {
                return Err(babel_types::Error::NotFound(format!(
                    "event parent {parent}"
                )));
            }
        }

        match &event.kind {
            EventKind::IdentityCreated => match &event.target {
                EventTarget::Identity(identity_id)
                    if identity_id == &event.actor && self.identity(identity_id).is_some() =>
                {
                    Ok(())
                }
                EventTarget::Identity(identity_id) => Err(babel_types::Error::Conflict(format!(
                    "identity event target mismatch: actor={} target={identity_id}",
                    event.actor
                ))),
                target => Err(babel_types::Error::Conflict(format!(
                    "identity event has invalid target: {target:?}"
                ))),
            },
            EventKind::IdentityKeyTransition => match &event.target {
                EventTarget::Identity(identity_id) if identity_id == &event.actor => {
                    let transition: IdentityKeyTransition =
                        serde_json::from_value(event.payload.clone())
                            .map_err(|err| babel_types::Error::Canonical(err.to_string()))?;
                    if &transition.identity_id == identity_id {
                        Ok(())
                    } else {
                        Err(babel_types::Error::Conflict(format!(
                            "identity key transition target mismatch: target={identity_id} transition={}",
                            transition.identity_id
                        )))
                    }
                }
                EventTarget::Identity(identity_id) => Err(babel_types::Error::Conflict(format!(
                    "identity key transition actor mismatch: actor={} target={identity_id}",
                    event.actor
                ))),
                target => Err(babel_types::Error::Conflict(format!(
                    "identity key transition has invalid target: {target:?}"
                ))),
            },
            EventKind::ObjectPublished => match &event.target {
                EventTarget::Object(object_id) if self.object(object_id).is_some() => Ok(()),
                EventTarget::Object(object_id) => {
                    Err(babel_types::Error::NotFound(object_id.to_string()))
                }
                target => Err(babel_types::Error::Conflict(format!(
                    "object event has invalid target: {target:?}"
                ))),
            },
            EventKind::EdgePublished => match &event.target {
                EventTarget::Edge(edge_id) if self.edge(edge_id).is_some() => Ok(()),
                EventTarget::Edge(edge_id) => {
                    Err(babel_types::Error::NotFound(edge_id.to_string()))
                }
                target => Err(babel_types::Error::Conflict(format!(
                    "edge event has invalid target: {target:?}"
                ))),
            },
            EventKind::ObjectForked
            | EventKind::ObjectRemixed
            | EventKind::CapabilityGranted
            | EventKind::CapabilityRevoked
            | EventKind::RealtimeRoomDefined
            | EventKind::RealtimeSessionStarted
            | EventKind::RealtimeSessionClosed
            | EventKind::RealtimeMessageCommitted
            | EventKind::RealtimeSnapshotCommitted => match &event.target {
                EventTarget::Object(object_id) if self.object(object_id).is_some() => Ok(()),
                EventTarget::Object(object_id) => {
                    Err(babel_types::Error::NotFound(object_id.to_string()))
                }
                target => Err(babel_types::Error::Conflict(format!(
                    "object-scoped event has invalid target: {target:?}"
                ))),
            },
            EventKind::ConsensusCheckpoint => match &event.target {
                EventTarget::Network => Ok(()),
                target => Err(babel_types::Error::Conflict(format!(
                    "checkpoint event has invalid target: {target:?}"
                ))),
            },
        }
    }

    fn latest_event_ids(&self, count: usize) -> Result<Vec<EventId>> {
        let mut events = self.store.list_events()?;
        events.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(events
            .into_iter()
            .take(count)
            .map(|event| event.id)
            .collect())
    }
}

fn event_grant(event: &Event) -> Result<CapabilityGrant> {
    serde_json::from_value(
        event
            .payload
            .get("grant")
            .cloned()
            .ok_or_else(|| babel_types::Error::Conflict("missing grant payload".to_string()))?,
    )
    .map_err(|err| babel_types::Error::Canonical(format!("decode capability grant: {err}")))
}

fn event_grant_id(event: &Event) -> Result<CapabilityGrantId> {
    let value = event
        .payload
        .get("grant_id")
        .and_then(Value::as_str)
        .ok_or_else(|| babel_types::Error::Conflict("missing grant_id payload".to_string()))?;
    let id = CapabilityGrantId::new_unchecked(value);
    id.validate()?;
    Ok(id)
}

fn capability_grants_from_events(events: &[Event]) -> Result<Vec<CapabilityGrant>> {
    Ok(grants::project_grants(events)?
        .into_values()
        .map(|(_, grant)| grant)
        .collect())
}

fn count_by(values: impl IntoIterator<Item = String>) -> Vec<MetricCount> {
    let mut counts = BTreeMap::<String, u64>::new();
    for value in values {
        *counts.entry(value).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(name, count)| MetricCount { name, count })
        .collect()
}

fn ratio_micros(numerator: usize, denominator: usize) -> u64 {
    if denominator == 0 {
        return 0;
    }
    ((numerator as u128) * 1_000_000_u128 / denominator as u128) as u64
}

fn average_confidence_micros(judgments: &[Judgment]) -> u64 {
    if judgments.is_empty() {
        return 0;
    }
    let total = judgments
        .iter()
        .map(|judgment| judgment.confidence.clamp(0.0, 1.0))
        .sum::<f64>();
    ((total / judgments.len() as f64) * 1_000_000.0).round() as u64
}

fn provider_disagreement_groups(judgments: &[Judgment]) -> u64 {
    let mut groups = BTreeMap::<(DefinitionId, Hash), BTreeSet<String>>::new();
    for judgment in judgments {
        groups
            .entry((judgment.definition.clone(), judgment.input_hash.clone()))
            .or_default()
            .insert(format!(
                "{}:{}:{}",
                judgment.provider.provider, judgment.provider.model, judgment.provider.version
            ));
    }
    groups
        .values()
        .filter(|providers| providers.len() > 1)
        .count() as u64
}

#[allow(clippy::too_many_arguments)]
fn validate_payment_checkout(
    merchant_id: Option<&str>,
    merchant_name: &str,
    currency: &str,
    total_amount_minor: u64,
    line_items: &[(String, u64, u32)],
    success_url: Option<&str>,
    cancel_url: Option<&str>,
    reference: Option<&str>,
) -> Result<()> {
    if let Some(merchant_id) = merchant_id
        && !valid_external_token(merchant_id, 128)
    {
        return Err(babel_types::Error::Conflict(
            "payment merchant_id must be a non-empty token at most 128 bytes".to_string(),
        ));
    }
    if merchant_name.trim().is_empty() || merchant_name.len() > 120 {
        return Err(babel_types::Error::Conflict(
            "payment merchant_name must be non-empty and at most 120 bytes".to_string(),
        ));
    }
    if !valid_currency(currency) {
        return Err(babel_types::Error::Conflict(
            "payment currency must be a three-letter uppercase code".to_string(),
        ));
    }
    if total_amount_minor == 0 {
        return Err(babel_types::Error::Conflict(
            "payment total_amount_minor must be positive".to_string(),
        ));
    }
    if line_items.is_empty() || line_items.len() > 64 {
        return Err(babel_types::Error::Conflict(
            "payment line_items must contain 1 to 64 entries".to_string(),
        ));
    }
    let mut computed_total = 0_u64;
    for (label, amount_minor, quantity) in line_items {
        if label.trim().is_empty() || label.len() > 160 {
            return Err(babel_types::Error::Conflict(
                "payment line item label must be non-empty and at most 160 bytes".to_string(),
            ));
        }
        if *amount_minor == 0 || *quantity == 0 {
            return Err(babel_types::Error::Conflict(
                "payment line item amount and quantity must be positive".to_string(),
            ));
        }
        let line_total = amount_minor
            .checked_mul(u64::from(*quantity))
            .ok_or_else(|| {
                babel_types::Error::Conflict("payment line item total overflow".to_string())
            })?;
        computed_total = computed_total
            .checked_add(line_total)
            .ok_or_else(|| babel_types::Error::Conflict("payment total overflow".to_string()))?;
    }
    if computed_total != total_amount_minor {
        return Err(babel_types::Error::Conflict(
            "payment line item total must equal total_amount_minor".to_string(),
        ));
    }
    validate_optional_return_url(success_url, "payment success_url")?;
    validate_optional_return_url(cancel_url, "payment cancel_url")?;
    if let Some(reference) = reference
        && !valid_reference(reference, 128)
    {
        return Err(babel_types::Error::Conflict(
            "payment reference must be a non-empty printable token at most 128 bytes".to_string(),
        ));
    }
    Ok(())
}

fn payment_scope_allows(
    scope: &Value,
    merchant_id: Option<&str>,
    currency: &str,
    total_amount_minor: u64,
) -> bool {
    let Some(scope) = scope.as_object() else {
        return false;
    };
    let currency_allowed = scope
        .get("currencies")
        .and_then(Value::as_array)
        .is_some_and(|currencies| {
            currencies
                .iter()
                .filter_map(Value::as_str)
                .any(|scoped_currency| scoped_currency == currency)
        });
    if !currency_allowed {
        return false;
    }
    if let Some(max_amount_minor) = scope.get("max_amount_minor").and_then(Value::as_u64)
        && total_amount_minor > max_amount_minor
    {
        return false;
    }
    scope
        .get("merchant_id")
        .and_then(Value::as_str)
        .is_none_or(|scoped_merchant| merchant_id == Some(scoped_merchant))
}

#[allow(clippy::too_many_arguments)]
fn payment_request_bytes(
    merchant_id: Option<&str>,
    merchant_name: &str,
    currency: &str,
    total_amount_minor: u64,
    line_items: &[(String, u64, u32)],
    success_url: Option<&str>,
    cancel_url: Option<&str>,
    reference: Option<&str>,
) -> Result<u64> {
    let mut bytes = merchant_id.map(str::len).unwrap_or(0)
        + merchant_name.len()
        + currency.len()
        + std::mem::size_of_val(&total_amount_minor)
        + success_url.map(str::len).unwrap_or(0)
        + cancel_url.map(str::len).unwrap_or(0)
        + reference.map(str::len).unwrap_or(0);
    for (label, _, _) in line_items {
        bytes = bytes.checked_add(label.len() + 16).ok_or_else(|| {
            babel_types::Error::Conflict("payment request byte accounting overflow".to_string())
        })?;
    }
    Ok(bytes as u64)
}

fn validate_notifications_request(purpose: &str, categories: &[String]) -> Result<()> {
    if purpose.trim().is_empty() || purpose.len() > 200 {
        return Err(babel_types::Error::Conflict(
            "notification purpose must be non-empty and at most 200 bytes".to_string(),
        ));
    }
    if categories.is_empty() || categories.len() > 16 {
        return Err(babel_types::Error::Conflict(
            "notification categories must contain 1 to 16 entries".to_string(),
        ));
    }
    let mut seen = BTreeSet::new();
    for category in categories {
        if !valid_category(category) || !seen.insert(category.as_str()) {
            return Err(babel_types::Error::Conflict(format!(
                "invalid or duplicate notification category: {category}"
            )));
        }
    }
    Ok(())
}

fn notification_scope_allows(scope: &Value, purpose: &str, categories: &[String]) -> bool {
    let Some(scope) = scope.as_object() else {
        return false;
    };
    if let Some(scoped_purpose) = scope.get("purpose").and_then(Value::as_str)
        && scoped_purpose != purpose
    {
        return false;
    }
    let Some(scoped_categories) = scope.get("categories").and_then(Value::as_array) else {
        return false;
    };
    let scoped = scoped_categories
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    categories
        .iter()
        .all(|category| scoped.contains(category.as_str()))
}

fn notification_request_bytes(purpose: &str, categories: &[String]) -> u64 {
    (purpose.len() + categories.iter().map(String::len).sum::<usize>()) as u64
}

fn validate_camera_request(
    purpose: &str,
    mode: &str,
    media_types: &[String],
    max_duration_ms: Option<u64>,
    facing_mode: Option<&str>,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<()> {
    validate_capture_purpose(purpose)?;
    if !valid_camera_mode(mode) {
        return Err(babel_types::Error::Conflict(
            "camera mode must be photo, video, or stream".to_string(),
        ));
    }
    validate_capture_media_types(media_types, |media_type| {
        media_type.starts_with("image/") || media_type.starts_with("video/")
    })?;
    if mode == "photo"
        && media_types
            .iter()
            .any(|media_type| !media_type.starts_with("image/"))
    {
        return Err(babel_types::Error::Conflict(
            "photo camera capture must request image media types".to_string(),
        ));
    }
    if matches!(mode, "video" | "stream")
        && media_types
            .iter()
            .any(|media_type| !media_type.starts_with("video/"))
    {
        return Err(babel_types::Error::Conflict(
            "video or stream camera capture must request video media types".to_string(),
        ));
    }
    validate_capture_duration(max_duration_ms)?;
    if let Some(facing_mode) = facing_mode
        && !valid_camera_facing_mode(facing_mode)
    {
        return Err(babel_types::Error::Conflict(
            "camera facing_mode must be any, user, or environment".to_string(),
        ));
    }
    if let Some(width) = width
        && !(1..=7680).contains(&width)
    {
        return Err(babel_types::Error::Conflict(
            "camera width must be between 1 and 7680".to_string(),
        ));
    }
    if let Some(height) = height
        && !(1..=4320).contains(&height)
    {
        return Err(babel_types::Error::Conflict(
            "camera height must be between 1 and 4320".to_string(),
        ));
    }
    Ok(())
}

fn validate_microphone_request(
    purpose: &str,
    mode: &str,
    media_types: &[String],
    max_duration_ms: Option<u64>,
) -> Result<()> {
    validate_capture_purpose(purpose)?;
    if !valid_microphone_mode(mode) {
        return Err(babel_types::Error::Conflict(
            "microphone mode must be audio_clip or stream".to_string(),
        ));
    }
    validate_capture_media_types(media_types, |media_type| media_type.starts_with("audio/"))?;
    validate_capture_duration(max_duration_ms)?;
    Ok(())
}

fn validate_capture_purpose(purpose: &str) -> Result<()> {
    if purpose.trim().is_empty() || purpose.len() > 200 {
        return Err(babel_types::Error::Conflict(
            "media capture purpose must be non-empty and at most 200 bytes".to_string(),
        ));
    }
    Ok(())
}

fn validate_capture_media_types(
    media_types: &[String],
    allowed: impl Fn(&str) -> bool,
) -> Result<()> {
    if media_types.is_empty() || media_types.len() > 8 {
        return Err(babel_types::Error::Conflict(
            "media capture media_types must contain 1 to 8 entries".to_string(),
        ));
    }
    let mut seen = BTreeSet::new();
    for media_type in media_types {
        if !allowed(media_type)
            || !seen.insert(media_type.as_str())
            || !valid_media_type(media_type)
        {
            return Err(babel_types::Error::Conflict(format!(
                "invalid or duplicate media capture media_type: {media_type}"
            )));
        }
    }
    Ok(())
}

fn validate_capture_duration(max_duration_ms: Option<u64>) -> Result<()> {
    if let Some(duration) = max_duration_ms
        && !(1..=3_600_000).contains(&duration)
    {
        return Err(babel_types::Error::Conflict(
            "media capture max_duration_ms must be between 1 and 3600000".to_string(),
        ));
    }
    Ok(())
}

fn camera_scope_allows(
    scope: &Value,
    mode: &str,
    media_types: &[String],
    max_duration_ms: Option<u64>,
    facing_mode: Option<&str>,
) -> bool {
    capture_scope_allows(scope, mode, media_types, max_duration_ms)
        && scope
            .as_object()
            .and_then(|scope| scope.get("facing_modes"))
            .and_then(Value::as_array)
            .is_none_or(|facing_modes| {
                let requested = facing_mode.unwrap_or("any");
                facing_modes
                    .iter()
                    .filter_map(Value::as_str)
                    .any(|candidate| candidate == requested || candidate == "any")
            })
}

fn microphone_scope_allows(
    scope: &Value,
    mode: &str,
    media_types: &[String],
    max_duration_ms: Option<u64>,
) -> bool {
    capture_scope_allows(scope, mode, media_types, max_duration_ms)
}

fn capture_scope_allows(
    scope: &Value,
    mode: &str,
    media_types: &[String],
    max_duration_ms: Option<u64>,
) -> bool {
    let Some(scope) = scope.as_object() else {
        return false;
    };
    let mode_allowed = scope
        .get("modes")
        .and_then(Value::as_array)
        .is_some_and(|modes| {
            modes
                .iter()
                .filter_map(Value::as_str)
                .any(|value| value == mode)
        });
    if !mode_allowed {
        return false;
    }
    let Some(scoped_media_types) = scope.get("media_types").and_then(Value::as_array) else {
        return false;
    };
    let scoped = scoped_media_types
        .iter()
        .filter_map(Value::as_str)
        .collect::<BTreeSet<_>>();
    if !media_types
        .iter()
        .all(|media_type| scoped.contains(media_type.as_str()))
    {
        return false;
    }
    if let Some(scoped_duration) = scope.get("max_duration_ms").and_then(Value::as_u64)
        && max_duration_ms.unwrap_or(scoped_duration) > scoped_duration
    {
        return false;
    }
    true
}

fn media_capture_request_bytes(
    purpose: &str,
    mode: &str,
    media_types: &[String],
    max_duration_ms: Option<u64>,
    facing_mode: Option<&str>,
) -> u64 {
    (purpose.len()
        + mode.len()
        + media_types.iter().map(String::len).sum::<usize>()
        + facing_mode.map(str::len).unwrap_or(0)
        + max_duration_ms.map(|_| 8).unwrap_or(0)) as u64
}

fn validate_ai_generate_request(
    purpose: &str,
    task: &str,
    prompt: &str,
    output_modalities: &[String],
    model: Option<&str>,
    max_output_tokens: Option<u32>,
    temperature_millis: Option<u32>,
) -> Result<()> {
    validate_ai_purpose(purpose)?;
    if !valid_ai_generate_task(task) {
        return Err(babel_types::Error::Conflict(
            "AI generate task must be text, image, audio, code, or json".to_string(),
        ));
    }
    if prompt.trim().is_empty() || prompt.len() > 64 * 1024 {
        return Err(babel_types::Error::Conflict(
            "AI generate prompt must be non-empty and at most 65536 bytes".to_string(),
        ));
    }
    validate_ai_string_set(
        output_modalities,
        "AI generate output_modalities",
        valid_ai_modality,
        4,
    )?;
    if let Some(model) = model {
        validate_ai_model(model)?;
    }
    if let Some(tokens) = max_output_tokens
        && !(1..=262_144).contains(&tokens)
    {
        return Err(babel_types::Error::Conflict(
            "AI generate max_output_tokens must be between 1 and 262144".to_string(),
        ));
    }
    if let Some(temperature) = temperature_millis
        && temperature > 2_000
    {
        return Err(babel_types::Error::Conflict(
            "AI generate temperature_millis must be at most 2000".to_string(),
        ));
    }
    Ok(())
}

fn validate_ai_embed_request(
    purpose: &str,
    input_modality: &str,
    inputs: &[String],
    model: Option<&str>,
    dimensions: Option<u32>,
) -> Result<()> {
    validate_ai_purpose(purpose)?;
    if !valid_ai_embed_modality(input_modality) {
        return Err(babel_types::Error::Conflict(
            "AI embed input_modality must be text, image, or audio".to_string(),
        ));
    }
    if inputs.is_empty() || inputs.len() > 128 {
        return Err(babel_types::Error::Conflict(
            "AI embed inputs must contain 1 to 128 entries".to_string(),
        ));
    }
    let total_bytes = inputs.iter().map(String::len).sum::<usize>();
    if total_bytes == 0 || total_bytes > 256 * 1024 {
        return Err(babel_types::Error::Conflict(
            "AI embed inputs must contain 1 to 262144 total bytes".to_string(),
        ));
    }
    if inputs.iter().any(|input| input.trim().is_empty()) {
        return Err(babel_types::Error::Conflict(
            "AI embed inputs must not contain empty entries".to_string(),
        ));
    }
    if let Some(model) = model {
        validate_ai_model(model)?;
    }
    if let Some(dimensions) = dimensions
        && !(1..=65_536).contains(&dimensions)
    {
        return Err(babel_types::Error::Conflict(
            "AI embed dimensions must be between 1 and 65536".to_string(),
        ));
    }
    Ok(())
}

fn validate_ai_transcribe_request(
    purpose: &str,
    media_uri: &str,
    media_type: &str,
    model: Option<&str>,
    language: Option<&str>,
    max_duration_ms: Option<u64>,
) -> Result<()> {
    validate_ai_purpose(purpose)?;
    validate_ai_media_uri(media_uri)?;
    if !(media_type.starts_with("audio/") || media_type.starts_with("video/"))
        || !valid_media_type(media_type)
    {
        return Err(babel_types::Error::Conflict(
            "AI transcribe media_type must be valid audio/* or video/*".to_string(),
        ));
    }
    if let Some(model) = model {
        validate_ai_model(model)?;
    }
    if let Some(language) = language
        && !valid_language(language)
    {
        return Err(babel_types::Error::Conflict(
            "AI transcribe language must be a valid BCP-47-like token".to_string(),
        ));
    }
    validate_capture_duration(max_duration_ms)?;
    Ok(())
}

fn validate_ai_purpose(purpose: &str) -> Result<()> {
    if purpose.trim().is_empty() || purpose.len() > 200 {
        return Err(babel_types::Error::Conflict(
            "AI capability purpose must be non-empty and at most 200 bytes".to_string(),
        ));
    }
    Ok(())
}

fn validate_ai_string_set(
    values: &[String],
    label: &str,
    valid: impl Fn(&str) -> bool,
    max_len: usize,
) -> Result<()> {
    if values.is_empty() || values.len() > max_len {
        return Err(babel_types::Error::Conflict(format!(
            "{label} must contain 1 to {max_len} entries"
        )));
    }
    let mut seen = BTreeSet::new();
    for value in values {
        if !valid(value) || !seen.insert(value.as_str()) {
            return Err(babel_types::Error::Conflict(format!(
                "invalid or duplicate {label} entry: {value}"
            )));
        }
    }
    Ok(())
}

fn validate_ai_model(model: &str) -> Result<()> {
    if valid_ai_model(model) {
        Ok(())
    } else {
        Err(babel_types::Error::Conflict(format!(
            "invalid AI model identifier: {model}"
        )))
    }
}

fn validate_ai_media_uri(media_uri: &str) -> Result<()> {
    if let Some(hash) = media_uri.strip_prefix("babel://blobs/") {
        Hash::new_unchecked(hash.to_string()).validate()?;
        return Ok(());
    }
    validate_optional_return_url(Some(media_uri), "AI media_uri")
}

fn ai_generate_scope_allows(
    scope: &Value,
    task: &str,
    prompt: &str,
    output_modalities: &[String],
    model: Option<&str>,
    max_output_tokens: Option<u32>,
) -> bool {
    let Some(scope) = scope.as_object() else {
        return false;
    };
    if !scope_array_contains(scope, "tasks", task) {
        return false;
    }
    if !scope_array_contains_all(scope, "output_modalities", output_modalities) {
        return false;
    }
    if let Some(model) = model
        && scope
            .get("models")
            .and_then(Value::as_array)
            .is_some_and(|models| !value_array_contains(models, model))
    {
        return false;
    }
    if let Some(limit) = scope.get("max_input_bytes").and_then(Value::as_u64)
        && prompt.len() as u64 > limit
    {
        return false;
    }
    if let Some(limit) = scope.get("max_output_tokens").and_then(Value::as_u64)
        && u64::from(max_output_tokens.unwrap_or(0)) > limit
    {
        return false;
    }
    true
}

fn ai_embed_scope_allows(
    scope: &Value,
    input_modality: &str,
    inputs: &[String],
    model: Option<&str>,
    dimensions: Option<u32>,
) -> bool {
    let Some(scope) = scope.as_object() else {
        return false;
    };
    if !scope_array_contains(scope, "input_modalities", input_modality) {
        return false;
    }
    if let Some(model) = model
        && scope
            .get("models")
            .and_then(Value::as_array)
            .is_some_and(|models| !value_array_contains(models, model))
    {
        return false;
    }
    if let Some(limit) = scope.get("max_input_bytes").and_then(Value::as_u64)
        && inputs.iter().map(String::len).sum::<usize>() as u64 > limit
    {
        return false;
    }
    if let Some(scoped_dimensions) = scope.get("dimensions").and_then(Value::as_u64)
        && dimensions.is_some_and(|requested| u64::from(requested) != scoped_dimensions)
    {
        return false;
    }
    true
}

fn ai_transcribe_scope_allows(
    scope: &Value,
    media_type: &str,
    model: Option<&str>,
    language: Option<&str>,
    max_duration_ms: Option<u64>,
) -> bool {
    let Some(scope) = scope.as_object() else {
        return false;
    };
    if !scope_array_contains(scope, "media_types", media_type) {
        return false;
    }
    if let Some(model) = model
        && scope
            .get("models")
            .and_then(Value::as_array)
            .is_some_and(|models| !value_array_contains(models, model))
    {
        return false;
    }
    if let Some(language) = language
        && scope
            .get("languages")
            .and_then(Value::as_array)
            .is_some_and(|languages| !value_array_contains(languages, language))
    {
        return false;
    }
    if let Some(limit) = scope.get("max_duration_ms").and_then(Value::as_u64)
        && max_duration_ms.unwrap_or(limit) > limit
    {
        return false;
    }
    true
}

fn scope_array_contains(scope: &serde_json::Map<String, Value>, field: &str, value: &str) -> bool {
    scope
        .get(field)
        .and_then(Value::as_array)
        .is_some_and(|values| value_array_contains(values, value))
}

fn scope_array_contains_all(
    scope: &serde_json::Map<String, Value>,
    field: &str,
    requested: &[String],
) -> bool {
    let Some(values) = scope.get(field).and_then(Value::as_array) else {
        return false;
    };
    requested
        .iter()
        .all(|value| value_array_contains(values, value))
}

fn value_array_contains(values: &[Value], value: &str) -> bool {
    values
        .iter()
        .filter_map(Value::as_str)
        .any(|item| item == value)
}

fn ai_generate_request_bytes(
    purpose: &str,
    task: &str,
    prompt: &str,
    output_modalities: &[String],
    model: Option<&str>,
    max_output_tokens: Option<u32>,
    temperature_millis: Option<u32>,
) -> u64 {
    (purpose.len()
        + task.len()
        + prompt.len()
        + output_modalities.iter().map(String::len).sum::<usize>()
        + model.map(str::len).unwrap_or(0)
        + max_output_tokens.map(|_| 4).unwrap_or(0)
        + temperature_millis.map(|_| 4).unwrap_or(0)) as u64
}

fn ai_embed_request_bytes(
    purpose: &str,
    input_modality: &str,
    inputs: &[String],
    model: Option<&str>,
    dimensions: Option<u32>,
) -> u64 {
    (purpose.len()
        + input_modality.len()
        + inputs.iter().map(String::len).sum::<usize>()
        + model.map(str::len).unwrap_or(0)
        + dimensions.map(|_| 4).unwrap_or(0)) as u64
}

fn ai_transcribe_request_bytes(
    purpose: &str,
    media_uri: &str,
    media_type: &str,
    model: Option<&str>,
    language: Option<&str>,
    max_duration_ms: Option<u64>,
) -> u64 {
    (purpose.len()
        + media_uri.len()
        + media_type.len()
        + model.map(str::len).unwrap_or(0)
        + language.map(str::len).unwrap_or(0)
        + max_duration_ms.map(|_| 8).unwrap_or(0)) as u64
}

fn validate_optional_return_url(value: Option<&str>, label: &str) -> Result<()> {
    let Some(value) = value else {
        return Ok(());
    };
    let url = reqwest::Url::parse(value)
        .map_err(|err| babel_types::Error::Conflict(format!("invalid {label}: {err}")))?;
    let allowed = url.scheme() == "https"
        || (url.scheme() == "http"
            && matches!(
                url.host_str(),
                Some("localhost" | "127.0.0.1" | "::1" | "[::1]")
            ));
    if allowed {
        Ok(())
    } else {
        Err(babel_types::Error::Conflict(format!(
            "{label} must use https or localhost http"
        )))
    }
}

fn valid_currency(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_uppercase())
}

fn valid_category(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'_' | b'-')
        })
}

fn valid_camera_mode(value: &str) -> bool {
    matches!(value, "photo" | "video" | "stream")
}

fn valid_microphone_mode(value: &str) -> bool {
    matches!(value, "audio_clip" | "stream")
}

fn valid_camera_facing_mode(value: &str) -> bool {
    matches!(value, "any" | "user" | "environment")
}

fn valid_ai_generate_task(value: &str) -> bool {
    matches!(value, "text" | "image" | "audio" | "code" | "json")
}

fn valid_ai_modality(value: &str) -> bool {
    matches!(value, "text" | "image" | "audio" | "json")
}

fn valid_ai_embed_modality(value: &str) -> bool {
    matches!(value, "text" | "image" | "audio")
}

fn valid_ai_model(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/')
        })
}

fn valid_language(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 35
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn valid_media_type(value: &str) -> bool {
    let Some((kind, subtype)) = value.split_once('/') else {
        return false;
    };
    valid_media_token(kind) && valid_media_token(subtype)
}

fn valid_media_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'+' | b'-' | b'.')
        })
}

fn valid_external_token(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':'))
}

fn valid_reference(value: &str, max_len: usize) -> bool {
    !value.trim().is_empty()
        && value.len() <= max_len
        && value
            .bytes()
            .all(|byte| byte.is_ascii_graphic() && byte != b'"' && byte != b'\\')
}

fn ai_judge_scope_allows(scope: &Value, target: &ObjectId, definition: &DefinitionId) -> bool {
    let Some(scope) = scope.as_object() else {
        return false;
    };
    let Some(scoped_definition) = scope.get("definition").and_then(Value::as_str) else {
        return false;
    };
    if scoped_definition != definition.as_str() {
        return false;
    }
    scope
        .get("object_id")
        .and_then(Value::as_str)
        .is_none_or(|object_id| object_id == target.as_str())
}

fn ai_judge_request_bytes(
    target: &ObjectId,
    definition: &DefinitionId,
    parameters: &BTreeMap<String, Value>,
) -> Result<u64> {
    let parameters = serde_json::to_vec(parameters).map_err(|err| {
        babel_types::Error::Canonical(format!("encode Judgment parameters: {err}"))
    })?;
    Ok((target.as_str().len() + definition.as_str().len() + parameters.len()) as u64)
}

fn apply_realtime_event(realtime: &mut RealtimeHub, event: &Event) -> Result<()> {
    match event.kind {
        EventKind::RealtimeRoomDefined => {
            let spec: RoomSpec =
                serde_json::from_value(event.payload.get("room").cloned().ok_or_else(|| {
                    babel_types::Error::Conflict("missing room payload".to_string())
                })?)
                .map_err(|err| babel_types::Error::Canonical(format!("decode room: {err}")))?;
            realtime.create_room(spec)?;
        }
        EventKind::RealtimeSessionStarted => {
            let session: RealtimeSession =
                serde_json::from_value(event.payload.get("session").cloned().ok_or_else(|| {
                    babel_types::Error::Conflict("missing session payload".to_string())
                })?)
                .map_err(|err| babel_types::Error::Canonical(format!("decode session: {err}")))?;
            realtime.apply_session(session)?;
        }
        EventKind::RealtimeSessionClosed => {
            let session: RealtimeSession =
                serde_json::from_value(event.payload.get("session").cloned().ok_or_else(|| {
                    babel_types::Error::Conflict("missing session payload".to_string())
                })?)
                .map_err(|err| babel_types::Error::Canonical(format!("decode session: {err}")))?;
            realtime.apply_session(session)?;
        }
        EventKind::RealtimeMessageCommitted => {
            let message: RealtimeMessage =
                serde_json::from_value(event.payload.get("message").cloned().ok_or_else(|| {
                    babel_types::Error::Conflict("missing realtime message payload".to_string())
                })?)
                .map_err(|err| {
                    babel_types::Error::Canonical(format!("decode realtime message: {err}"))
                })?;
            realtime.apply_message(message)?;
        }
        EventKind::RealtimeSnapshotCommitted => {
            let snapshot: RealtimeSnapshot = serde_json::from_value(
                event.payload.get("snapshot").cloned().ok_or_else(|| {
                    babel_types::Error::Conflict("missing realtime snapshot payload".to_string())
                })?,
            )
            .map_err(|err| {
                babel_types::Error::Canonical(format!("decode realtime snapshot: {err}"))
            })?;
            realtime.apply_snapshot(snapshot)?;
        }
        _ => {}
    }
    Ok(())
}

fn insert_events_by_parent(mut dag: EventDag, events: Vec<Event>) -> Result<EventDag> {
    let mut pending = events
        .into_iter()
        .map(|event| (event.id.clone(), event))
        .collect::<BTreeMap<_, _>>();

    while !pending.is_empty() {
        let ready = pending
            .iter()
            .filter(|(_, event)| event.parents.iter().all(|parent| dag.contains(parent)))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        if ready.is_empty() {
            let missing = pending
                .values()
                .flat_map(|event| event.parents.iter())
                .filter(|parent| !dag.contains(parent) && !pending.contains_key(*parent))
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            return Err(babel_types::Error::Conflict(format!(
                "event log has unresolved parent closure: {}",
                missing.join(", ")
            )));
        }

        for id in ready {
            let event = pending
                .remove(&id)
                .ok_or_else(|| babel_types::Error::NotFound(id.to_string()))?;
            dag.insert(event)?;
        }
    }

    Ok(dag)
}

fn ingestion_definitions() -> Vec<DefinitionId> {
    vec![
        DefinitionId::spam_v1(),
        DefinitionId::evidence_quality_v1(),
        DefinitionId::content_analysis_v1(),
        DefinitionId::moderation_v1(),
    ]
}

fn object_judgment_state(object: &Object) -> babel_judgment::JudgmentState {
    let mut context = BTreeMap::new();
    context.insert("object".to_string(), object.payload.clone());
    context.insert(
        "kind".to_string(),
        Value::String(object.kind.as_str().to_string()),
    );
    context.insert("schema".to_string(), Value::String(object.schema.clone()));
    context.insert(
        "author".to_string(),
        Value::String(object.author.to_string()),
    );
    context.insert("text".to_string(), Value::String(object_text(object)));
    babel_judgment::JudgmentState {
        subject: object.id.to_string(),
        context,
    }
}

fn object_provider_judgment_state(object: &Object) -> babel_judgment::JudgmentState {
    let mut context = BTreeMap::new();
    context.insert("object".to_string(), object.payload.clone());
    context.insert("text".to_string(), Value::String(object_text(object)));
    babel_judgment::JudgmentState {
        subject: object.id.to_string(),
        context,
    }
}

fn relationship_judgment_state(source: &Object, target: &Object) -> babel_judgment::JudgmentState {
    let source_text = object_text(source);
    let target_text = object_text(target);
    let mut context = BTreeMap::new();
    context.insert("source_object".to_string(), source.payload.clone());
    context.insert("target_object".to_string(), target.payload.clone());
    context.insert(
        "source_text".to_string(),
        Value::String(source_text.clone()),
    );
    context.insert(
        "target_text".to_string(),
        Value::String(target_text.clone()),
    );
    context.insert(
        "text".to_string(),
        Value::String(format!("source: {source_text}\ntarget: {target_text}")),
    );
    babel_judgment::JudgmentState {
        subject: format!("{}->{}", source.id, target.id),
        context,
    }
}

fn normalize_relationship_relation(value: &str) -> Result<Relation> {
    match value {
        "supports" => Ok(Relation::Supports),
        "contradicts" => Ok(Relation::Contradicts),
        "related" => Ok(Relation::References),
        other => Err(babel_types::Error::Conflict(format!(
            "unsupported inferred relationship: {other}"
        ))),
    }
}

fn relationship_parameter(relation: &Relation) -> &'static str {
    match relation {
        Relation::Supports | Relation::EvidenceFor => "supports",
        Relation::Contradicts | Relation::EvidenceAgainst => "contradicts",
        _ => "related",
    }
}

fn evidence_relation_kind(relation: &Relation) -> Option<EvidenceRelationKind> {
    match relation {
        Relation::EvidenceFor | Relation::Supports => Some(EvidenceRelationKind::Supports),
        Relation::EvidenceAgainst | Relation::Contradicts => {
            Some(EvidenceRelationKind::Contradicts)
        }
        Relation::References | Relation::Cites => Some(EvidenceRelationKind::Related),
        _ => None,
    }
}

fn sort_projection_items(items: &mut [EvidenceProjectionItem]) {
    items.sort_by(|left, right| {
        left.edge
            .created_at
            .cmp(&right.edge.created_at)
            .then_with(|| left.edge.id.cmp(&right.edge.id))
    });
}

fn novelty_signal(
    author_count: usize,
    max_author_objects: usize,
    relation_diversity: f64,
    support: f64,
    contradiction: f64,
) -> f64 {
    let author_rarity = if max_author_objects <= 1 {
        1.0
    } else {
        1.0 - ((author_count.saturating_sub(1)) as f64 / max_author_objects as f64)
    };
    clamp01(
        0.35 * author_rarity
            + 0.30 * relation_diversity
            + 0.20 * contradiction
            + 0.15 * (1.0 - clamp01(support)),
    )
}

fn exploration_signal(
    relevance: f64,
    novelty: f64,
    temporal: f64,
    relation_diversity: f64,
    creative_reputation: f64,
    total_objects: usize,
) -> f64 {
    let sparse_pool_bonus = if total_objects < 20 {
        0.12
    } else if total_objects < 100 {
        0.06
    } else {
        0.0
    };
    clamp01(
        0.34 * novelty
            + 0.24 * (1.0 - clamp01(relevance))
            + 0.18 * relation_diversity
            + 0.14 * temporal
            + 0.10 * creative_reputation
            + sparse_pool_bonus,
    )
}

fn relation_diversity(incoming: &[Edge], outgoing: &[Edge]) -> f64 {
    let relations = incoming
        .iter()
        .chain(outgoing.iter())
        .map(|edge| edge.relation.clone())
        .collect::<BTreeSet<_>>();
    normalized_count(relations.len(), 6)
}

fn normalized_count(count: usize, max_count: usize) -> f64 {
    if max_count == 0 {
        0.0
    } else {
        (count as f64 / max_count as f64).clamp(0.0, 1.0)
    }
}

fn clamp01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

fn score_search_text(
    haystack: &str,
    phrase: Option<&str>,
    terms: &[String],
) -> (u64, Vec<String>) {
    let mut score = 0_u64;
    let mut reasons = Vec::new();

    if let Some(phrase) = phrase
        && haystack.contains(phrase)
    {
        score += 20;
        reasons.push("phrase_match".to_string());
    }

    for term in terms {
        let matches = haystack.matches(term).count() as u64;
        if matches > 0 {
            score += matches;
            reasons.push(format!("term:{term}"));
        }
    }

    if terms.is_empty() && phrase.is_none() {
        score = 1;
        reasons.push("unfiltered".to_string());
    }

    (score, reasons)
}

fn searchable_object_text(object: &Object) -> String {
    let mut parts = vec![
        object.id.to_string(),
        object.author.to_string(),
        object.kind.as_str().to_string(),
        object.schema.clone(),
    ];
    collect_json_text(&object.payload, &mut parts);
    if let Some(state) = &object.state {
        collect_json_text(state, &mut parts);
    }
    parts.join(" ").to_ascii_lowercase()
}

fn object_text(object: &Object) -> String {
    for key in ["text", "title", "description", "name", "summary"] {
        if let Some(value) = object.payload.get(key).and_then(Value::as_str)
            && !value.trim().is_empty()
        {
            return value.trim().to_string();
        }
    }
    let mut parts = Vec::new();
    collect_json_text(&object.payload, &mut parts);
    if parts.is_empty() {
        object.kind.as_str().to_string()
    } else {
        parts.join(" ")
    }
}

fn collect_json_text(value: &Value, parts: &mut Vec<String>) {
    match value {
        Value::String(value) => parts.push(value.clone()),
        Value::Array(values) => {
            for value in values {
                collect_json_text(value, parts);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                parts.push(key.clone());
                collect_json_text(value, parts);
            }
        }
        Value::Number(value) => parts.push(value.to_string()),
        Value::Bool(value) => parts.push(value.to_string()),
        Value::Null => {}
    }
}

fn search_terms(query: &str) -> Vec<String> {
    query
        .split(|character: char| !character.is_alphanumeric())
        .map(str::trim)
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn surface_state_checkpoint_key(session_id: &SurfaceSessionId) -> String {
    format!("runtime/surface_sessions/{session_id}/state")
}

fn unique_sources(sources: Vec<ObjectId>) -> Result<Vec<ObjectId>> {
    if sources.is_empty() {
        return Err(babel_types::Error::Conflict(
            "remix must reference at least one source object".to_string(),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut unique = Vec::new();
    for source in sources {
        source.validate()?;
        if !seen.insert(source.clone()) {
            return Err(babel_types::Error::Conflict(format!(
                "duplicate remix source object: {source}"
            )));
        }
        unique.push(source);
    }
    Ok(unique)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exploration_signal_rewards_novel_low_relevance_candidates() {
        let familiar = exploration_signal(0.9, 0.1, 0.5, 0.0, 0.0, 100);
        let serendipitous = exploration_signal(0.1, 0.9, 0.5, 0.6, 0.4, 100);

        assert!(serendipitous > familiar);
    }
}
