use babel_authoring::ObjectDraft;
use babel_capabilities::{
    CapabilityDecision, CapabilityDefinition, CapabilityGrant, CapabilityManifest,
    CapabilityReceipt, GrantDecision,
};
use babel_graph::{Edge, EdgeOrigin, GraphTraversal, Relation, TraversalDirection};
use babel_hashgraph::FinalityCheckpoint;
use babel_identity::{Identity, IdentityKeyScope, IdentityKeyTransition, IdentityKind};
use babel_judgment::{
    DefinitionId, Judgment, JudgmentDefinition, JudgmentProviderDescriptor, OrchestratedJudgment,
};
use babel_lens::{LensDefinition, LensStack};
use babel_media::MediaBlob;
use babel_node::{
    ClaimEvidenceProjection, DiscoveryResult, ImportBundle, ImportReport, ObjectSearchResult,
    ObservabilitySnapshot, ProvenancePublication,
};
use babel_object::{CapabilityRequest as ObjectCapabilityRequest, Object, SurfaceRole};
use babel_personalization::EncryptedLocalUserModel;
use babel_realtime::{
    MembershipPolicy, PersistencePolicy, RealtimeMessage, RealtimePayload, RealtimeSession,
    RealtimeSnapshot, RoomLimits, RoomSpec, RoomView,
};
use babel_runtime::{
    ResourceBudget, SurfaceLifecycle, SurfaceRuntimeEvent, SurfaceRuntimeHealthSnapshot,
    SurfaceScheduleDecision, SurfaceSchedulingInput, SurfaceSession, SurfaceSessionId,
    SurfaceSessionPlan, SurfaceStateCheckpoint,
};
use babel_state::Event;
use babel_types::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EmptyRequest {}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CreateIdentityRequest {
    pub kind: IdentityKind,
    pub handle: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CreateIdentityResponse {
    pub identity: Identity,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IdentityCurrentResponse {
    pub identity: Identity,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RotateIdentityKeyRequest {
    pub scope: IdentityKeyScope,
    pub expires_at: Option<Timestamp>,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RotateIdentityKeyResponse {
    pub transition: IdentityKeyTransition,
    pub event: Event,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishTextRequest {
    pub author_id: String,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishTextResponse {
    pub object: Object,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishObjectRequest {
    pub author_id: String,
    pub draft: ObjectDraft,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishObjectResponse {
    pub object: Object,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ForkObjectRequest {
    pub author_id: String,
    pub source_object_id: String,
    pub draft: ObjectDraft,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RemixObjectRequest {
    pub author_id: String,
    pub source_object_ids: Vec<String>,
    pub draft: ObjectDraft,
}

pub type ProvenancePublicationResponse = ProvenancePublication;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectIdRequest {
    pub object_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PutMediaBlobRequest {
    pub media_type: String,
    pub bytes_hex: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GetMediaBlobRequest {
    pub hash: String,
    pub media_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MediaBlobResponse {
    pub blob: MediaBlob,
    pub bytes_hex: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishMediaObjectRequest {
    pub author_id: String,
    pub title: String,
    pub description: Option<String>,
    pub resources: Vec<MediaBlob>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishMediaObjectResponse {
    pub object: Object,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishEdgeRequest {
    pub author_id: String,
    pub source: String,
    pub target: String,
    pub relation: Relation,
    pub origin: EdgeOrigin,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishEdgeResponse {
    pub edge: Edge,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RelationshipInferenceKind {
    Supports,
    Contradicts,
    Related,
}

impl RelationshipInferenceKind {
    pub fn as_parameter(&self) -> &'static str {
        match self {
            Self::Supports => "supports",
            Self::Contradicts => "contradicts",
            Self::Related => "related",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InferRelationshipRequest {
    pub author_id: String,
    pub source: String,
    pub target: String,
    pub relation: RelationshipInferenceKind,
    pub min_score: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct InferRelationshipResponse {
    pub edge: Edge,
    pub judgment: Judgment,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SocialTargetRequest {
    pub author_id: String,
    pub target_object_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SocialEdgeResponse {
    pub edge: Edge,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SocialTextRequest {
    pub author_id: String,
    pub target_object_id: Option<String>,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media: Option<SocialMediaAttachment>,
}

pub type SocialMediaAttachment = babel_node::SocialMediaAttachment;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SocialTextResponse {
    pub object: Object,
    pub edge: Edge,
    pub receipt: CapabilityReceipt,
}

pub type RepliesListRequest = babel_node::RepliesListQuery;
pub type RepliesListResponse = babel_node::RepliesListResult;
pub type QuotesListRequest = babel_node::QuotesListQuery;
pub type QuotesListResponse = babel_node::QuotesListResult;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EdgeListResponse {
    pub edges: Vec<Edge>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GraphTraverseRequest {
    pub direction: TraversalDirection,
    pub relations: Vec<Relation>,
    pub max_depth: u8,
    pub limit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GraphTraverseRpcRequest {
    pub object_id: String,
    pub direction: TraversalDirection,
    pub relations: Vec<Relation>,
    pub max_depth: u8,
    pub limit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GraphTraversalResponse {
    pub traversal: GraphTraversal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClaimEvidenceResponse {
    pub projection: ClaimEvidenceProjection,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventResponse {
    pub event: Event,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventBundleRequest {
    pub events: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventBundleResponse {
    pub bundle: ImportBundle,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventListRequest {
    pub after: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventListResponse {
    pub events: Vec<Event>,
    pub next_after: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventImportRequest {
    pub bundle: ImportBundle,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EventImportResponse {
    pub report: ImportReport,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectSearchResponse {
    pub results: Vec<ObjectSearchResult>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectSearchRequest {
    pub q: Option<String>,
    pub author: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DiscoveryRequest {
    #[serde(default)]
    pub anchors: Vec<String>,
    pub search: Option<String>,
    #[serde(default)]
    pub followed_objects: Vec<String>,
    pub limit: Option<usize>,
    pub exploration_slots: Option<usize>,
    pub lens: Option<LensStack>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DiscoveryResponse {
    pub discovery: DiscoveryResult,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LensCatalogResponse {
    pub lenses: Vec<LensDefinition>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilityCatalogResponse {
    pub capabilities: Vec<CapabilityDefinition>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgmentDefinitionsResponse {
    pub definitions: Vec<JudgmentDefinition>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgmentProvidersResponse {
    pub providers: Vec<JudgmentProviderDescriptor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CapabilitiesResponse {
    pub manifest: CapabilityManifest,
    pub decisions: Vec<CapabilityDecision>,
    pub grants: Vec<CapabilityGrant>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GrantCapabilityRequest {
    pub author_id: String,
    pub object_id: String,
    pub capability: ObjectCapabilityRequest,
    pub decision: GrantDecision,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct GrantCapabilityResponse {
    pub event: Event,
    pub grants: Vec<CapabilityGrant>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RevokeCapabilityRequest {
    pub author_id: String,
    pub object_id: String,
    pub grant_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectStorageEntry {
    pub key: String,
    pub value: Value,
    pub updated_at: Timestamp,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectStorageGetRequest {
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectStorageGetResponse {
    pub entry: Option<ObjectStorageEntry>,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectStorageSetRequest {
    pub key: String,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectStorageSetResponse {
    pub entry: ObjectStorageEntry,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectStorageDeleteRequest {
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectStorageDeleteResponse {
    pub deleted: Option<ObjectStorageEntry>,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectStorageListRequest {
    pub prefix: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectStorageListResponse {
    pub entries: Vec<ObjectStorageEntry>,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LocalStorageEntry {
    pub key: String,
    pub value: Value,
    pub updated_at: Timestamp,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LocalStorageGetRequest {
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LocalStorageGetResponse {
    pub entry: Option<LocalStorageEntry>,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LocalStorageSetRequest {
    pub key: String,
    pub value: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LocalStorageSetResponse {
    pub entry: LocalStorageEntry,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LocalStorageDeleteRequest {
    pub key: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LocalStorageDeleteResponse {
    pub deleted: Option<LocalStorageEntry>,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LocalStorageListRequest {
    pub prefix: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LocalStorageListResponse {
    pub entries: Vec<LocalStorageEntry>,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersonalizationSyncEnvelopeSummary {
    pub envelope_hash: String,
    pub identity_id: String,
    pub device_id: String,
    pub uploaded_at: Timestamp,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersonalizationSyncPutRequest {
    pub envelope: EncryptedLocalUserModel,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersonalizationSyncPutResponse {
    pub envelope: PersonalizationSyncEnvelopeSummary,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersonalizationSyncListRequest {
    pub identity_id: String,
    pub device_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersonalizationSyncListResponse {
    pub envelopes: Vec<PersonalizationSyncEnvelopeSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersonalizationSyncGetRequest {
    pub identity_id: String,
    pub device_id: String,
    pub envelope_hash: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersonalizationSyncGetResponse {
    pub envelope: EncryptedLocalUserModel,
    pub summary: PersonalizationSyncEnvelopeSummary,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PersonalizationSyncDeleteResponse {
    pub deleted: Option<PersonalizationSyncEnvelopeSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NetworkFetchRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body_hex: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NetworkFetchResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body_hex: String,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaymentLineItem {
    pub label: String,
    pub amount_minor: u64,
    pub quantity: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaymentsCheckoutRequest {
    pub merchant_id: Option<String>,
    pub merchant_name: String,
    pub currency: String,
    pub total_amount_minor: u64,
    pub line_items: Vec<PaymentLineItem>,
    pub success_url: Option<String>,
    pub cancel_url: Option<String>,
    pub reference: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaymentsCheckoutAction {
    pub kind: String,
    pub merchant_id: Option<String>,
    pub merchant_name: String,
    pub currency: String,
    pub total_amount_minor: u64,
    pub line_items: Vec<PaymentLineItem>,
    pub success_url: Option<String>,
    pub cancel_url: Option<String>,
    pub reference: Option<String>,
    pub requires_user_activation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PaymentsCheckoutResponse {
    pub action: PaymentsCheckoutAction,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NotificationsRequestRequest {
    pub purpose: String,
    pub categories: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NotificationsRequestAction {
    pub kind: String,
    pub purpose: String,
    pub categories: Vec<String>,
    pub requires_user_activation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct NotificationsRequestResponse {
    pub action: NotificationsRequestAction,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CameraCaptureMode {
    Photo,
    Video,
    Stream,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CameraFacingMode {
    Any,
    User,
    Environment,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CameraCaptureRequest {
    pub purpose: String,
    pub mode: CameraCaptureMode,
    pub media_types: Vec<String>,
    pub max_duration_ms: Option<u64>,
    pub facing_mode: Option<CameraFacingMode>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CameraCaptureAction {
    pub kind: String,
    pub purpose: String,
    pub mode: CameraCaptureMode,
    pub media_types: Vec<String>,
    pub max_duration_ms: Option<u64>,
    pub facing_mode: CameraFacingMode,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub requires_user_activation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CameraCaptureResponse {
    pub action: CameraCaptureAction,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MicrophoneCaptureMode {
    AudioClip,
    Stream,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MicrophoneCaptureRequest {
    pub purpose: String,
    pub mode: MicrophoneCaptureMode,
    pub media_types: Vec<String>,
    pub max_duration_ms: Option<u64>,
    pub echo_cancellation: Option<bool>,
    pub noise_suppression: Option<bool>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MicrophoneCaptureAction {
    pub kind: String,
    pub purpose: String,
    pub mode: MicrophoneCaptureMode,
    pub media_types: Vec<String>,
    pub max_duration_ms: Option<u64>,
    pub echo_cancellation: Option<bool>,
    pub noise_suppression: Option<bool>,
    pub requires_user_activation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MicrophoneCaptureResponse {
    pub action: MicrophoneCaptureAction,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipboardWriteRequest {
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipboardWriteAction {
    pub kind: String,
    pub text: String,
    pub media_type: String,
    pub requires_user_activation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ClipboardWriteResponse {
    pub action: ClipboardWriteAction,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FullscreenNavigationUi {
    Auto,
    Show,
    Hide,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FullscreenEnterRequest {
    pub target_hint: Option<String>,
    pub navigation_ui: Option<FullscreenNavigationUi>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FullscreenEnterAction {
    pub kind: String,
    pub target_hint: Option<String>,
    pub navigation_ui: FullscreenNavigationUi,
    pub requires_user_activation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FullscreenEnterResponse {
    pub action: FullscreenEnterAction,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PrepareSurfaceRequest {
    pub object_id: String,
    pub role: SurfaceRole,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PrepareSurfaceResponse {
    pub plan: SurfaceSessionPlan,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StartSurfaceSessionRequest {
    pub object_id: String,
    pub role: SurfaceRole,
    pub session_id: Option<SurfaceSessionId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceSessionResponse {
    pub session: SurfaceSession,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BindSurfaceDocumentRequest {
    pub document_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceDocumentResponse {
    pub session_id: SurfaceSessionId,
    pub document_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceLease {
    pub session_id: SurfaceSessionId,
    pub expires_at: String,
    pub ttl_ms: u64,
    pub renew_after_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceLeaseResponse {
    pub lease: SurfaceLease,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceRuntimeHealthResponse {
    pub health: SurfaceRuntimeHealthSnapshot,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObservabilitySnapshotResponse {
    pub snapshot: ObservabilitySnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceSessionRequest {
    pub session_id: SurfaceSessionId,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TransitionSurfaceSessionRequest {
    pub lifecycle: SurfaceLifecycle,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChangeSurfaceBudgetRequest {
    pub budget: ResourceBudget,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScheduleSurfaceSessionRequest {
    pub input: SurfaceSchedulingInput,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ScheduleSurfaceSessionResponse {
    pub decision: SurfaceScheduleDecision,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ApplySurfaceScheduleResponse {
    pub session: SurfaceSession,
    pub decision: SurfaceScheduleDecision,
    pub events: Vec<SurfaceRuntimeEvent>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceSessionEventResponse {
    pub session: SurfaceSession,
    pub event: SurfaceRuntimeEvent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CheckpointSurfaceStateRequest {
    pub state: Value,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceStateCheckpointResponse {
    pub session: SurfaceSession,
    pub checkpoint: SurfaceStateCheckpoint,
    pub event: SurfaceRuntimeEvent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SurfaceStateRestoreResponse {
    pub checkpoint: SurfaceStateCheckpoint,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DefineRealtimeRoomRequest {
    pub author_id: String,
    pub object_id: String,
    pub name: String,
    pub schema: String,
    pub membership: MembershipPolicy,
    pub persistence: PersistencePolicy,
    pub limits: Option<RoomLimits>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DefineRealtimeRoomResponse {
    pub event: Event,
    pub room: RoomSpec,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StartRealtimeSessionRequest {
    pub author_id: String,
    pub room_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StartRealtimeSessionResponse {
    pub session: RealtimeSession,
    pub receipt: Option<CapabilityReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CloseRealtimeSessionRequest {
    pub author_id: String,
    pub session_id: String,
    pub object_id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CloseRealtimeSessionResponse {
    pub session: RealtimeSession,
    pub event: Event,
    pub receipt: Option<CapabilityReceipt>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishRealtimeMessageRequest {
    pub author_id: String,
    pub session_id: String,
    pub object_id: String,
    pub payload: RealtimePayload,
    pub durable: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PublishRealtimeMessageResponse {
    pub message: RealtimeMessage,
    pub snapshot: Option<RealtimeSnapshot>,
    pub receipt: Option<CapabilityReceipt>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RealtimeRoomResponse {
    pub room: RoomView,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgeObjectRequest {
    pub definition: DefinitionId,
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgeObjectRpcRequest {
    pub object_id: String,
    pub definition: DefinitionId,
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgeObjectResponse {
    pub judgment: Judgment,
    pub input: Option<babel_store::ObjectJudgmentInput>,
    pub orchestration: Option<OrchestratedJudgment>,
    pub receipt: Option<CapabilityReceipt>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiGenerateTask {
    Text,
    Image,
    Audio,
    Code,
    Json,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiModality {
    Text,
    Image,
    Audio,
    Json,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AiGenerateRequest {
    pub purpose: String,
    pub task: AiGenerateTask,
    pub prompt: String,
    pub output_modalities: Vec<AiModality>,
    pub model: Option<String>,
    pub max_output_tokens: Option<u32>,
    pub temperature_millis: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AiGenerateAction {
    pub kind: String,
    pub purpose: String,
    pub task: AiGenerateTask,
    pub prompt: String,
    pub output_modalities: Vec<AiModality>,
    pub model: Option<String>,
    pub max_output_tokens: Option<u32>,
    pub temperature_millis: Option<u32>,
    pub requires_user_activation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AiGenerateResponse {
    pub action: AiGenerateAction,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AiEmbedInputModality {
    Text,
    Image,
    Audio,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AiEmbedRequest {
    pub purpose: String,
    pub input_modality: AiEmbedInputModality,
    pub inputs: Vec<String>,
    pub model: Option<String>,
    pub dimensions: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AiEmbedAction {
    pub kind: String,
    pub purpose: String,
    pub input_modality: AiEmbedInputModality,
    pub inputs: Vec<String>,
    pub model: Option<String>,
    pub dimensions: Option<u32>,
    pub requires_user_activation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AiEmbedResponse {
    pub action: AiEmbedAction,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AiTranscribeRequest {
    pub purpose: String,
    pub media_uri: String,
    pub media_type: String,
    pub model: Option<String>,
    pub language: Option<String>,
    pub max_duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AiTranscribeAction {
    pub kind: String,
    pub purpose: String,
    pub media_uri: String,
    pub media_type: String,
    pub model: Option<String>,
    pub language: Option<String>,
    pub max_duration_ms: Option<u64>,
    pub requires_user_activation: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AiTranscribeResponse {
    pub action: AiTranscribeAction,
    pub receipt: CapabilityReceipt,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ObjectJudgmentsResponse {
    pub object_id: String,
    pub judgments: Vec<Judgment>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CheckpointRequest {
    pub author_id: String,
    pub validators: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CheckpointPreviewRequest {
    pub validators: BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CheckpointPreviewResponse {
    pub checkpoint: FinalityCheckpoint,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CheckpointEventResponse {
    pub event: Event,
    pub checkpoint: FinalityCheckpoint,
}
