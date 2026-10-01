use crate::{
    dispatch_rpc_request,
    error::ApiError,
    schema::{
        ApplySurfaceScheduleResponse, CapabilitiesResponse, CapabilityCatalogResponse,
        ChangeSurfaceBudgetRequest, CheckpointEventResponse, CheckpointPreviewRequest,
        CheckpointPreviewResponse, CheckpointRequest, CheckpointSurfaceStateRequest,
        ClaimEvidenceResponse, CloseRealtimeSessionRequest, CloseRealtimeSessionResponse,
        CreateIdentityRequest, CreateIdentityResponse, DefineRealtimeRoomRequest,
        DefineRealtimeRoomResponse, DiscoveryRequest, DiscoveryResponse, EdgeListResponse,
        EventBundleRequest, EventBundleResponse, EventImportRequest, EventImportResponse,
        EventListRequest, EventListResponse, EventResponse, ForkObjectRequest,
        GrantCapabilityRequest, GrantCapabilityResponse, GraphTraversalResponse,
        GraphTraverseRequest, InferRelationshipRequest, InferRelationshipResponse,
        JudgeObjectRequest, JudgeObjectResponse, JudgmentDefinitionsResponse,
        JudgmentProvidersResponse, LensCatalogResponse, MediaBlobResponse, ObjectJudgmentsResponse,
        ObjectSearchResponse, ObservabilitySnapshotResponse, PersonalizationSyncDeleteResponse,
        PersonalizationSyncEnvelopeSummary, PersonalizationSyncGetResponse,
        PersonalizationSyncListRequest, PersonalizationSyncListResponse,
        PersonalizationSyncPutRequest, PersonalizationSyncPutResponse, PrepareSurfaceRequest,
        PrepareSurfaceResponse, ProvenancePublicationResponse, PublishEdgeRequest,
        PublishEdgeResponse, PublishMediaObjectRequest, PublishMediaObjectResponse,
        PublishObjectRequest, PublishObjectResponse, PublishRealtimeMessageRequest,
        PublishRealtimeMessageResponse, PublishTextRequest, PublishTextResponse,
        PutMediaBlobRequest, RealtimeRoomResponse, RemixObjectRequest, RevokeCapabilityRequest,
        RotateIdentityKeyRequest, RotateIdentityKeyResponse, ScheduleSurfaceSessionRequest,
        ScheduleSurfaceSessionResponse, StartRealtimeSessionRequest, StartRealtimeSessionResponse,
        StartSurfaceSessionRequest, SurfaceRuntimeHealthResponse, SurfaceSessionEventResponse,
        SurfaceSessionResponse, SurfaceStateCheckpointResponse, SurfaceStateRestoreResponse,
        TransitionSurfaceSessionRequest,
    },
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderName, HeaderValue, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use babble_graph::{GraphTraversalSpec, Relation};
use babble_hashgraph::{FinalityCheckpoint, ValidatorSet};
use babble_judgment::{JudgmentProvider, JudgmentRegistry, ProviderVersion};
use babble_lens::BuiltInLens;
use babble_node::{DiscoveryQuery, EventListQuery, LocalNode, ObjectSearchQuery};
use babble_realtime::RoomSpec;
use babble_rpc::{RpcCatalog, RpcRequestEnvelope, RpcResponseEnvelope, babble_rpc_catalog};
use babble_runtime::SurfaceSessionId;
use babble_state::{Event, EventKind};
use babble_store::PersonalizationSyncRecord;
use babble_types::{EventId, Hash, IdentityId, JudgmentId, ObjectId};
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Arc, Mutex},
};

pub struct ApiState<P> {
    pub(crate) node: Arc<Mutex<LocalNode<P>>>,
    pub(crate) auth: Arc<crate::auth::Auth>,
    pub(crate) gateway: Option<Arc<crate::gateway::Gateway>>,
}

impl<P> Clone for ApiState<P> {
    fn clone(&self) -> Self {
        Self {
            node: self.node.clone(),
            auth: self.auth.clone(),
            gateway: self.gateway.clone(),
        }
    }
}

impl<P> ApiState<P>
where
    P: JudgmentProvider,
{
    pub fn new(node: LocalNode<P>) -> Self {
        let auth = crate::auth::Auth::new(node.store().root())
            .with_operator(std::env::var("BABBLE_OPERATOR_TOKEN").ok().as_deref());
        Self {
            auth: Arc::new(auth),
            node: Arc::new(Mutex::new(node)),
            gateway: None,
        }
    }

    pub fn with_bundle_gateway(mut self, config: crate::gateway::GatewayConfig) -> Self {
        self.gateway = Some(Arc::new(crate::gateway::Gateway::new(config)));
        self
    }

    pub fn with_moderators(self, ids: &str) -> babble_types::Result<Self> {
        self.node.lock().map_err(|_| babble_types::Error::StorageUnavailable("node lock unavailable".into()))?.configure_moderators(ids)?;
        Ok(self)
    }
}

pub fn router<P>(state: ApiState<P>) -> Router
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    crate::execution::bounded(
        trusted_router(state.clone())
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                crate::auth::policy::authorize::<P>,
            ))
            .merge(crate::auth::router(state)),
    )
}

// Raw handlers are crate-private. The exported router always applies the HTTP
// boundary; native unit tests deliberately use these handlers without it.
pub(crate) fn trusted_router<P>(state: ApiState<P>) -> Router
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    Router::new()
        .merge(crate::invocations::router::<P>())
        .merge(crate::moderation::router::<P>())
        .route("/health", get(health::<P>))
        .route("/observability", get(observability_snapshot::<P>))
        .route("/rpc", post(dispatch_rpc::<P>))
        .route("/rpc/catalog", get(get_rpc_catalog))
        .route("/identities", post(create_identity::<P>))
        .route("/identities/{id}", get(get_identity::<P>))
        .route(
            "/identities/{id}/objects",
            get(crate::profiles::author_objects::<P>),
        )
        .route("/social/following", get(crate::following::list::<P>))
        .route("/social/safety", get(crate::safety::snapshot::<P>))
        .route("/social/safety/{id}", get(crate::safety::state::<P>).put(crate::safety::set::<P>))
        .route(
            "/social/following/{id}",
            get(crate::following::state::<P>).put(crate::following::set::<P>),
        )
        .route("/feed/following", get(crate::following::feed::<P>))
        .route("/objects/{id}/quotes", get(crate::quotes::list::<P>))
        .route(
            "/objects/{id}/reactions",
            get(crate::reactions::summary::<P>),
        )
        .route(
            "/objects/{id}/reactions/mine",
            get(crate::reactions::mine::<P>).put(crate::reactions::set::<P>),
        )
        .route(
            "/objects/{id}/reactions/actors/{actor}",
            get(crate::reactions::record::<P>),
        )
        .route(
            "/identities/{id}/keys/rotate",
            post(rotate_identity_key::<P>),
        )
        .route("/objects", post(publish_object::<P>))
        .route("/objects/text", post(publish_text::<P>))
        .route("/objects/media", post(publish_media_object::<P>))
        .route("/objects/{id}", get(get_object::<P>))
        .route("/objects/{id}/media/{hash}", get(crate::media::get::<P>))
        .route("/objects/{id}/judgments", get(get_object_judgments::<P>))
        .route("/objects/forks", post(fork_object::<P>))
        .route("/objects/remixes", post(remix_object::<P>))
        .route("/objects/{id}/capabilities", get(get_capabilities::<P>))
        .route("/graph/edges", post(publish_edge::<P>))
        .route("/graph/relationships/infer", post(infer_relationship::<P>))
        .route("/graph/edges/{id}", get(get_edge::<P>))
        .route("/graph/objects/{id}/incoming", get(get_incoming_edges::<P>))
        .route("/graph/objects/{id}/outgoing", get(get_outgoing_edges::<P>))
        .route("/graph/objects/{id}/evidence", get(get_evidence::<P>))
        .route("/graph/objects/{id}/traverse", post(traverse_graph::<P>))
        .route("/events", get(list_events::<P>))
        .route("/events/bundle", post(get_event_bundle::<P>))
        .route("/events/import", post(import_events::<P>))
        .route("/events/{id}", get(get_event::<P>))
        .route("/consensus/checkpoints", post(publish_checkpoint::<P>))
        .route(
            "/consensus/checkpoints/preview",
            post(preview_checkpoint::<P>),
        )
        .route("/consensus/checkpoints/{id}", get(get_checkpoint::<P>))
        .route("/judgments/definitions", get(list_judgment_definitions))
        .route("/judgments/providers", get(list_judgment_providers::<P>))
        .route("/judgments/object/{id}", post(judge_object::<P>))
        .route("/judgments/{id}", get(get_judgment::<P>))
        .route("/search/objects", get(search_objects::<P>))
        .route("/lenses", get(list_lenses))
        .route("/discovery/candidates", post(discover_candidates::<P>))
        .route("/capabilities", get(list_capabilities::<P>))
        .route("/capabilities/grants", post(grant_capability::<P>))
        .route("/capabilities/revocations", post(revoke_capability::<P>))
        .route(
            "/personalization/sync/envelopes",
            post(put_personalization_sync_envelope::<P>)
                .get(list_personalization_sync_envelopes::<P>),
        )
        .route(
            "/personalization/sync/envelopes/{hash}",
            get(get_personalization_sync_envelope::<P>)
                .delete(delete_personalization_sync_envelope::<P>),
        )
        .route("/runtime/surfaces/prepare", post(prepare_surface::<P>))
        .route(
            "/runtime/surfaces/sessions",
            post(start_surface_session::<P>),
        )
        .route("/runtime/surfaces/health", get(surface_runtime_health::<P>))
        .route(
            "/runtime/surfaces/sessions/{id}",
            get(get_surface_session::<P>),
        )
        .route(
            "/runtime/surfaces/sessions/{id}/lifecycle",
            post(transition_surface_session::<P>),
        )
        .route(
            "/runtime/surfaces/sessions/{id}/heartbeat",
            post(heartbeat_surface_session::<P>),
        )
        .route(
            "/runtime/surfaces/sessions/{id}/document",
            axum::routing::put(bind_surface_document::<P>),
        )
        .route(
            "/runtime/surfaces/sessions/{id}/budget",
            post(change_surface_session_budget::<P>),
        )
        .route(
            "/runtime/surfaces/sessions/{id}/schedule",
            post(schedule_surface_session::<P>),
        )
        .route(
            "/runtime/surfaces/sessions/{id}/schedule/apply",
            post(apply_surface_schedule::<P>),
        )
        .route(
            "/runtime/surfaces/sessions/{id}/state",
            get(get_surface_state_checkpoint::<P>),
        )
        .route(
            "/runtime/surfaces/sessions/{id}/state/checkpoint",
            post(checkpoint_surface_state::<P>),
        )
        .route("/runtime/surfaces/blobs/{hash}", get(get_surface_blob::<P>))
        .route("/realtime/rooms", post(define_realtime_room::<P>))
        .route("/realtime/rooms/{id}", get(get_realtime_room::<P>))
        .route("/realtime/sessions", post(start_realtime_session::<P>))
        .route(
            "/realtime/sessions/{id}",
            delete(close_realtime_session::<P>),
        )
        .route("/realtime/messages", post(publish_realtime_message::<P>))
        .route("/media/blobs", post(put_media_blob::<P>))
        .route("/media/blobs/{hash}", get(get_media_blob::<P>))
        .with_state(state)
}

async fn dispatch_rpc<P>(
    State(state): State<ApiState<P>>,
    principal: Option<axum::Extension<crate::auth::Principal>>,
    boundary: Option<axum::Extension<crate::auth::HttpBoundary>>,
    Json(request): Json<RpcRequestEnvelope>,
) -> Result<Json<RpcResponseEnvelope>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    if boundary.is_some() {
        Ok(Json(crate::rpc::dispatch_http_request(
            &state,
            request,
            principal.as_ref().map(|value| &value.0),
        )))
    } else {
        Ok(Json(dispatch_rpc_request(&state, request)))
    }
}

async fn get_rpc_catalog() -> Result<Json<RpcCatalog>, ApiError> {
    let catalog = babble_rpc_catalog()
        .map_err(|err| ApiError::internal(format!("invalid RPC catalog: {err}")))?;
    Ok(Json(catalog))
}

async fn get_event<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
) -> Result<Json<EventResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let id = event_id(id)?;
    let node = lock_node(&state)?;
    let event = node
        .event(&id)
        .ok_or_else(|| ApiError::not_found(format!("event not found: {id}")))?
        .clone();
    Ok(Json(EventResponse { event }))
}

async fn list_events<P>(
    State(state): State<ApiState<P>>,
    Query(request): Query<EventListRequest>,
) -> Result<Json<EventListResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let limit = request.limit.unwrap_or(100);
    if limit > 500 {
        return Err(ApiError::bad_request(
            "event list limit must be at most 500",
        ));
    }
    let after = request.after.map(event_id).transpose()?;
    let node = lock_node(&state)?;
    let listed = node.list_events(EventListQuery { after, limit })?;
    Ok(Json(EventListResponse {
        next_after: listed.next_after.map(|id| id.to_string()),
        events: listed.events,
    }))
}

async fn get_event_bundle<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<EventBundleRequest>,
) -> Result<Json<EventBundleResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    if request.events.is_empty() {
        return Err(ApiError::bad_request(
            "event bundle request must include at least one event",
        ));
    }
    let event_ids = request
        .events
        .into_iter()
        .map(event_id)
        .collect::<Result<BTreeSet<_>, _>>()?;
    let node = lock_node(&state)?;
    let bundle = node.event_bundle(&event_ids)?;
    Ok(Json(EventBundleResponse { bundle }))
}

async fn import_events<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<EventImportRequest>,
) -> Result<Json<EventImportResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    if request.bundle.events.is_empty()
        && request.bundle.objects.is_empty()
        && request.bundle.edges.is_empty()
        && request.bundle.identities.is_empty()
    {
        return Err(ApiError::bad_request(
            "event import bundle must not be empty",
        ));
    }
    let mut node = lock_node(&state)?;
    let report = node.import_bundle(request.bundle)?;
    Ok(Json(EventImportResponse { report }))
}

async fn preview_checkpoint<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<CheckpointPreviewRequest>,
) -> Result<Json<CheckpointPreviewResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let validators = validator_set(request.validators)?;
    let node = lock_node(&state)?;
    let checkpoint = node.finality_checkpoint(&validators)?;
    Ok(Json(CheckpointPreviewResponse { checkpoint }))
}

async fn publish_checkpoint<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<CheckpointRequest>,
) -> Result<Json<CheckpointEventResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let author_id = identity_id(request.author_id)?;
    let validators = validator_set(request.validators)?;
    let mut node = lock_node(&state)?;
    let event = node.publish_checkpoint(&author_id, validators)?;
    checkpoint_response(event)
}

async fn get_checkpoint<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
) -> Result<Json<CheckpointEventResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let id = event_id(id)?;
    let node = lock_node(&state)?;
    let event = node
        .event(&id)
        .ok_or_else(|| ApiError::not_found(format!("checkpoint not found: {id}")))?
        .clone();
    checkpoint_response(event)
}

async fn health<P>(State(state): State<ApiState<P>>) -> Result<Json<HealthResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let node = lock_node(&state)?;
    Ok(Json(HealthResponse {
        ok: true,
        judgment_provider: node.judgment_provider_version(),
        ranking_provider: node.ranking_provider_version(),
        temporal_provider: node.temporal_provider_version(),
    }))
}

async fn observability_snapshot<P>(
    State(state): State<ApiState<P>>,
) -> Result<Json<ObservabilitySnapshotResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let node = lock_node(&state)?;
    Ok(Json(ObservabilitySnapshotResponse {
        snapshot: node.observability_snapshot()?,
    }))
}

async fn put_personalization_sync_envelope<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<PersonalizationSyncPutRequest>,
) -> Result<Json<PersonalizationSyncPutResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let node = lock_node(&state)?;
    let record = node.put_personalization_sync_envelope(request.envelope)?;
    Ok(Json(PersonalizationSyncPutResponse {
        envelope: personalization_sync_summary(record),
    }))
}

async fn list_personalization_sync_envelopes<P>(
    State(state): State<ApiState<P>>,
    Query(request): Query<PersonalizationSyncListRequest>,
) -> Result<Json<PersonalizationSyncListResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let identity_id = identity_id(request.identity_id)?;
    let node = lock_node(&state)?;
    let records = node.list_personalization_sync_envelopes(
        &identity_id,
        request.device_id.as_deref(),
        request.limit.unwrap_or(64),
    )?;
    Ok(Json(PersonalizationSyncListResponse {
        envelopes: records
            .into_iter()
            .map(personalization_sync_summary)
            .collect(),
    }))
}

async fn get_personalization_sync_envelope<P>(
    State(state): State<ApiState<P>>,
    Path(hash): Path<String>,
    Query(request): Query<PersonalizationSyncListRequest>,
) -> Result<Json<PersonalizationSyncGetResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let identity_id = identity_id(request.identity_id)?;
    let device_id = request
        .device_id
        .ok_or_else(|| ApiError::bad_request("device_id query parameter is required"))?;
    let envelope_hash = hash_value(hash)?;
    let node = lock_node(&state)?;
    let record = node.personalization_sync_envelope(&identity_id, &device_id, &envelope_hash)?;
    Ok(Json(PersonalizationSyncGetResponse {
        envelope: record.envelope.clone(),
        summary: personalization_sync_summary(record),
    }))
}

async fn delete_personalization_sync_envelope<P>(
    State(state): State<ApiState<P>>,
    Path(hash): Path<String>,
    Query(request): Query<PersonalizationSyncListRequest>,
) -> Result<Json<PersonalizationSyncDeleteResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let identity_id = identity_id(request.identity_id)?;
    let device_id = request
        .device_id
        .ok_or_else(|| ApiError::bad_request("device_id query parameter is required"))?;
    let envelope_hash = hash_value(hash)?;
    let node = lock_node(&state)?;
    let deleted =
        node.delete_personalization_sync_envelope(&identity_id, &device_id, &envelope_hash)?;
    Ok(Json(PersonalizationSyncDeleteResponse {
        deleted: deleted.map(personalization_sync_summary),
    }))
}

async fn create_identity<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<CreateIdentityRequest>,
) -> Result<Json<CreateIdentityResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    if request.handle.trim().is_empty() {
        return Err(ApiError::bad_request("identity handle must not be empty"));
    }
    let mut node = lock_node(&state)?;
    let identity = node.create_identity(request.kind, request.handle.trim())?;
    Ok(Json(CreateIdentityResponse { identity }))
}

async fn get_identity<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
) -> Result<Json<CreateIdentityResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let id = identity_id(id)?;
    let node = lock_node(&state)?;
    let identity = node
        .identity(&id)
        .ok_or_else(|| ApiError::not_found(format!("identity not found: {id}")))?
        .clone();
    Ok(Json(CreateIdentityResponse { identity }))
}

async fn rotate_identity_key<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    Json(request): Json<RotateIdentityKeyRequest>,
) -> Result<Json<RotateIdentityKeyResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let id = identity_id(id)?;
    if request.reason.trim().is_empty() {
        return Err(ApiError::bad_request(
            "identity key transition reason must not be empty",
        ));
    }
    let mut node = lock_node(&state)?;
    let (transition, event) = node.rotate_identity_key(
        &id,
        request.scope,
        request.expires_at,
        request.reason.trim(),
    )?;
    Ok(Json(RotateIdentityKeyResponse { transition, event }))
}

async fn publish_text<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<PublishTextRequest>,
) -> Result<Json<PublishTextResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    if request.text.trim().is_empty() {
        return Err(ApiError::bad_request("text object must not be empty"));
    }
    let author_id = identity_id(request.author_id)?;
    let mut node = lock_node(&state)?;
    let object = node.publish_text(&author_id, request.text.trim())?;
    Ok(Json(PublishTextResponse { object }))
}

async fn publish_object<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<PublishObjectRequest>,
) -> Result<Json<PublishObjectResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let author_id = identity_id(request.author_id)?;
    let mut node = lock_node(&state)?;
    let object = node.publish_draft(&author_id, request.draft)?;
    Ok(Json(PublishObjectResponse { object }))
}

async fn get_object<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
) -> Result<Json<PublishTextResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let id = object_id(id)?;
    let node = lock_node(&state)?;
    let object = node
        .object(&id)
        .ok_or_else(|| ApiError::not_found(format!("object not found: {id}")))?
        .clone();
    Ok(Json(PublishTextResponse { object }))
}

async fn fork_object<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<ForkObjectRequest>,
) -> Result<Json<ProvenancePublicationResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let author_id = identity_id(request.author_id)?;
    let source = object_id(request.source_object_id)?;
    let mut node = lock_node(&state)?;
    let publication = node.fork_object(&author_id, &source, request.draft)?;
    Ok(Json(publication))
}

async fn remix_object<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<RemixObjectRequest>,
) -> Result<Json<ProvenancePublicationResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let author_id = identity_id(request.author_id)?;
    let sources = request
        .source_object_ids
        .into_iter()
        .map(object_id)
        .collect::<Result<Vec<_>, _>>()?;
    let mut node = lock_node(&state)?;
    let publication = node.remix_object(&author_id, sources, request.draft)?;
    Ok(Json(publication))
}

async fn put_media_blob<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<PutMediaBlobRequest>,
) -> Result<Json<MediaBlobResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let bytes = decode_hex(&request.bytes_hex)?;
    let node = lock_node(&state)?;
    let blob = node.put_media_blob(&request.media_type, &bytes)?;
    Ok(Json(MediaBlobResponse {
        blob,
        bytes_hex: None,
    }))
}

async fn get_media_blob<P>(
    State(state): State<ApiState<P>>,
    Path(hash): Path<String>,
    Query(query): Query<MediaBlobQuery>,
) -> Result<Json<MediaBlobResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let hash = hash_value(hash)?;
    let node = lock_node(&state)?;
    let (blob, bytes) = node
        .media_blob_bounded(
            &hash,
            &query.media_type,
            crate::execution::MAX_BUFFERED_BLOB_BYTES,
        )?
        .ok_or_else(|| ApiError::not_found(format!("media blob not found: {hash}")))?;
    Ok(Json(MediaBlobResponse {
        blob,
        bytes_hex: Some(hex::encode(bytes)),
    }))
}

async fn publish_media_object<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<PublishMediaObjectRequest>,
) -> Result<Json<PublishMediaObjectResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let author_id = identity_id(request.author_id)?;
    let mut node = lock_node(&state)?;
    let object = node.publish_media_object(
        &author_id,
        &request.title,
        request.description,
        request.resources,
    )?;
    Ok(Json(PublishMediaObjectResponse { object }))
}

async fn get_capabilities<P>(
    State(state): State<ApiState<P>>,
    principal: Option<axum::Extension<crate::auth::Principal>>,
    Path(id): Path<String>,
) -> Result<Json<CapabilitiesResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let id = object_id(id)?;
    let node = lock_node(&state)?;
    let manifest = node.capability_manifest(&id)?;
    let identity = principal.map(|value| IdentityId::new_unchecked(value.0.identity_id));
    let (decisions, grants) = node.capability_review_for_identity(&id, identity.as_ref())?;
    Ok(Json(CapabilitiesResponse {
        manifest,
        decisions,
        grants,
    }))
}

async fn publish_edge<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<PublishEdgeRequest>,
) -> Result<Json<PublishEdgeResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let author_id = identity_id(request.author_id)?;
    let source = object_id(request.source)?;
    let target = object_id(request.target)?;
    let mut node = lock_node(&state)?;
    let edge = node.publish_edge(&author_id, source, target, request.relation, request.origin)?;
    Ok(Json(PublishEdgeResponse { edge }))
}

async fn infer_relationship<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<InferRelationshipRequest>,
) -> Result<Json<InferRelationshipResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let author_id = identity_id(request.author_id)?;
    let source = object_id(request.source)?;
    let target = object_id(request.target)?;
    let mut node = lock_node(&state)?;
    let (edge, judgment) = node.infer_relationship_edge(
        &author_id,
        &source,
        &target,
        request.relation.as_parameter(),
        request.min_score.unwrap_or(0.5),
    )?;
    Ok(Json(InferRelationshipResponse { edge, judgment }))
}

async fn get_edge<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
) -> Result<Json<PublishEdgeResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let id = edge_id(id)?;
    let node = lock_node(&state)?;
    let edge = node
        .edge(&id)
        .ok_or_else(|| ApiError::not_found(format!("edge not found: {id}")))?
        .clone();
    Ok(Json(PublishEdgeResponse { edge }))
}

async fn get_incoming_edges<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    Query(query): Query<EdgeQuery>,
) -> Result<Json<EdgeListResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let id = object_id(id)?;
    let relation = query.relation.map(relation_from_param).transpose()?;
    let node = lock_node(&state)?;
    let edges = match relation {
        Some(relation) => node.incoming_relation(&id, &relation),
        None => node.incoming_edges(&id),
    };
    Ok(Json(EdgeListResponse { edges }))
}

async fn get_outgoing_edges<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    Query(query): Query<EdgeQuery>,
) -> Result<Json<EdgeListResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let id = object_id(id)?;
    let relation = query.relation.map(relation_from_param).transpose()?;
    let node = lock_node(&state)?;
    let edges = match relation {
        Some(relation) => node.outgoing_relation(&id, &relation),
        None => node.outgoing_edges(&id),
    };
    Ok(Json(EdgeListResponse { edges }))
}

async fn traverse_graph<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    Json(request): Json<GraphTraverseRequest>,
) -> Result<Json<GraphTraversalResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let root = object_id(id)?;
    let node = lock_node(&state)?;
    let traversal = node.traverse_graph(&GraphTraversalSpec {
        root,
        direction: request.direction,
        relations: request.relations,
        max_depth: request.max_depth.clamp(1, 8),
        limit: request.limit.clamp(1, 512),
    })?;
    Ok(Json(GraphTraversalResponse { traversal }))
}

async fn get_evidence<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
) -> Result<Json<ClaimEvidenceResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let id = object_id(id)?;
    let node = lock_node(&state)?;
    Ok(Json(ClaimEvidenceResponse {
        projection: node.evidence_projection(&id)?,
    }))
}

async fn judge_object<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    Json(request): Json<JudgeObjectRequest>,
) -> Result<Json<JudgeObjectResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let id = object_id(id)?;
    let mut node = lock_node(&state)?;
    let orchestration =
        node.judge_object_orchestrated(&id, request.definition, request.parameters)?;
    Ok(Json(JudgeObjectResponse {
        input: node.object_judgment_input(&orchestration.judgment.id)?,
        judgment: orchestration.judgment.clone(),
        orchestration: Some(orchestration),
        receipt: None,
    }))
}

async fn get_judgment<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
) -> Result<Json<JudgeObjectResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let id = judgment_id(id)?;
    let node = lock_node(&state)?;
    let judgment = node
        .judgment(&id)?
        .ok_or_else(|| ApiError::not_found(format!("judgment not found: {id}")))?;
    Ok(Json(JudgeObjectResponse {
        input: node.object_judgment_input(&id)?,
        judgment,
        orchestration: None,
        receipt: None,
    }))
}

async fn list_judgment_definitions() -> Result<Json<JudgmentDefinitionsResponse>, ApiError> {
    Ok(Json(JudgmentDefinitionsResponse {
        definitions: JudgmentRegistry::babble_core().definitions,
    }))
}

async fn list_judgment_providers<P>(
    State(state): State<ApiState<P>>,
) -> Result<Json<JudgmentProvidersResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let node = lock_node(&state)?;
    Ok(Json(JudgmentProvidersResponse {
        providers: node.judgment_provider_descriptors(),
    }))
}

async fn get_object_judgments<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
) -> Result<Json<ObjectJudgmentsResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let id = object_id(id)?;
    let node = lock_node(&state)?;
    let judgments = node.object_judgments(&id)?;
    Ok(Json(ObjectJudgmentsResponse {
        object_id: id.to_string(),
        judgments,
    }))
}

async fn search_objects<P>(
    State(state): State<ApiState<P>>,
    Query(query): Query<ObjectSearchParams>,
) -> Result<Json<ObjectSearchResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let author = query.author.map(identity_id).transpose()?;
    let limit = query.limit.unwrap_or(50);
    if limit > 200 {
        return Err(ApiError::bad_request("search limit must be at most 200"));
    }
    let node = lock_node(&state)?;
    let results = node.search_objects(ObjectSearchQuery {
        query: query.q,
        author,
        kind: query.kind,
        limit,
    })?;
    Ok(Json(ObjectSearchResponse { results }))
}

async fn discover_candidates<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<DiscoveryRequest>,
) -> Result<Json<DiscoveryResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let anchors = request
        .anchors
        .into_iter()
        .map(object_id)
        .collect::<Result<Vec<_>, _>>()?;
    let followed_objects = request
        .followed_objects
        .into_iter()
        .map(object_id)
        .collect::<Result<BTreeSet<_>, _>>()?;
    let limit = request.limit.unwrap_or(50);
    if limit > 200 {
        return Err(ApiError::bad_request("discovery limit must be at most 200"));
    }
    let mut query = DiscoveryQuery {
        anchors,
        search: request.search,
        followed_objects,
        limit,
        exploration_slots: request.exploration_slots.unwrap_or(5).min(limit),
        ..Default::default()
    };
    if let Some(lens) = request.lens {
        query.lens = lens;
    }

    let mut node = lock_node(&state)?;
    let discovery = node.discover_objects(query)?;
    Ok(Json(DiscoveryResponse { discovery }))
}

async fn list_lenses() -> Result<Json<LensCatalogResponse>, ApiError> {
    Ok(Json(LensCatalogResponse {
        lenses: BuiltInLens::all()
            .into_iter()
            .map(|lens| lens.definition())
            .collect(),
    }))
}

async fn list_capabilities<P>(
    State(state): State<ApiState<P>>,
) -> Result<Json<CapabilityCatalogResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let node = lock_node(&state)?;
    Ok(Json(CapabilityCatalogResponse {
        capabilities: node.capability_definitions(),
    }))
}

async fn grant_capability<P>(
    State(state): State<ApiState<P>>,
    headers: HeaderMap,
    Json(request): Json<GrantCapabilityRequest>,
) -> Result<Json<GrantCapabilityResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let author_id = identity_id(request.author_id.clone())?;
    let retry = crate::consent::rest_request(&headers, &author_id, "grant", &request)?;
    let object_id = object_id(request.object_id)?;
    let mut node = lock_node(&state)?;
    let operation = |node: &mut LocalNode<P>| {
        node.grant_capability(&author_id, &object_id, request.capability, request.decision)
    };
    let event = match retry {
        Some(retry) => node.with_publication_request(retry, operation)?,
        None => operation(&mut node)?,
    };
    let grants = node.capability_grants_for_identity(&object_id, &author_id)?;
    Ok(Json(GrantCapabilityResponse { event, grants }))
}

async fn revoke_capability<P>(
    State(state): State<ApiState<P>>,
    headers: HeaderMap,
    Json(request): Json<RevokeCapabilityRequest>,
) -> Result<Json<GrantCapabilityResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let author_id = identity_id(request.author_id.clone())?;
    let retry = crate::consent::rest_request(&headers, &author_id, "revoke", &request)?;
    let object_id = object_id(request.object_id)?;
    let grant_id = grant_id(request.grant_id)?;
    let mut node = lock_node(&state)?;
    let operation =
        |node: &mut LocalNode<P>| node.revoke_capability(&author_id, &object_id, &grant_id);
    let event = match retry {
        Some(retry) => node.with_publication_request(retry, operation)?,
        None => operation(&mut node)?,
    };
    let grants = node.capability_grants_for_identity(&object_id, &author_id)?;
    Ok(Json(GrantCapabilityResponse { event, grants }))
}

async fn prepare_surface<P>(
    State(state): State<ApiState<P>>,
    principal: Option<axum::Extension<crate::auth::Principal>>,
    boundary: Option<axum::Extension<crate::auth::HttpBoundary>>,
    Json(request): Json<PrepareSurfaceRequest>,
) -> Result<Json<PrepareSurfaceResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let object_id = object_id(request.object_id)?;
    let node = lock_node(&state)?;
    let plan = if boundary.is_some() {
        let identity = principal.map(|value| IdentityId::new_unchecked(value.0.identity_id));
        crate::gateway::prepare(
            state.gateway.as_ref(),
            &node,
            &object_id,
            request.role,
            identity.as_ref(),
        )?
    } else {
        node.prepare_surface(&object_id, request.role)?
    };
    Ok(Json(PrepareSurfaceResponse { plan }))
}

async fn start_surface_session<P>(
    State(state): State<ApiState<P>>,
    principal: Option<axum::Extension<crate::auth::Principal>>,
    boundary: Option<axum::Extension<crate::auth::HttpBoundary>>,
    Json(request): Json<StartSurfaceSessionRequest>,
) -> Result<Json<SurfaceSessionResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    if let Some(axum::Extension(principal)) = principal {
        return Ok(Json(SurfaceSessionResponse {
            session: crate::auth::start_surface(&state, request, &principal)?,
        }));
    }
    if boundary.is_some() {
        return Err(ApiError::unauthorized());
    }
    let object_id = object_id(request.object_id)?;
    let mut node = lock_node(&state)?;
    let session = node.start_surface_session(&object_id, request.role, request.session_id)?;
    Ok(Json(SurfaceSessionResponse { session }))
}

async fn heartbeat_surface_session<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    principal: Option<axum::Extension<crate::auth::Principal>>,
) -> Result<Json<crate::schema::SurfaceLeaseResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let principal = principal.ok_or_else(ApiError::unauthorized)?.0;
    Ok(Json(crate::auth::heartbeat_surface(
        &state,
        surface_session_id(id)?,
        &principal,
    )?))
}

async fn bind_surface_document<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    principal: Option<axum::Extension<crate::auth::Principal>>,
    Json(request): Json<crate::schema::BindSurfaceDocumentRequest>,
) -> Result<Json<crate::schema::SurfaceDocumentResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let principal = principal.ok_or_else(ApiError::unauthorized)?.0;
    Ok(Json(crate::auth::bind_surface_document(
        &state,
        surface_session_id(id)?,
        request.document_id,
        &principal,
    )?))
}

async fn get_surface_session<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
) -> Result<Json<SurfaceSessionResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let session_id = surface_session_id(id)?;
    let node = lock_node(&state)?;
    Ok(Json(SurfaceSessionResponse {
        session: node.surface_session(&session_id)?,
    }))
}

async fn surface_runtime_health<P>(
    State(state): State<ApiState<P>>,
) -> Result<Json<SurfaceRuntimeHealthResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let node = lock_node(&state)?;
    Ok(Json(SurfaceRuntimeHealthResponse {
        health: node.surface_runtime_health(),
    }))
}

async fn transition_surface_session<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    Json(request): Json<TransitionSurfaceSessionRequest>,
) -> Result<Json<SurfaceSessionEventResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let session_id = surface_session_id(id)?;
    let mut node = lock_node(&state)?;
    let (session, event) =
        node.transition_surface_session(&session_id, request.lifecycle, &request.reason)?;
    state.auth.retire_evicted_surface(&session)?;
    Ok(Json(SurfaceSessionEventResponse { session, event }))
}

async fn change_surface_session_budget<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    Json(request): Json<ChangeSurfaceBudgetRequest>,
) -> Result<Json<SurfaceSessionEventResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let session_id = surface_session_id(id)?;
    let mut node = lock_node(&state)?;
    let (session, event) =
        node.reduce_surface_session_budget(&session_id, request.budget, &request.reason)?;
    Ok(Json(SurfaceSessionEventResponse { session, event }))
}

async fn schedule_surface_session<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    Json(request): Json<ScheduleSurfaceSessionRequest>,
) -> Result<Json<ScheduleSurfaceSessionResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let session_id = surface_session_id(id)?;
    let node = lock_node(&state)?;
    let decision = node.schedule_surface_session(&session_id, &request.input)?;
    Ok(Json(ScheduleSurfaceSessionResponse { decision }))
}

async fn apply_surface_schedule<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    Json(request): Json<ScheduleSurfaceSessionRequest>,
) -> Result<Json<ApplySurfaceScheduleResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let session_id = surface_session_id(id)?;
    let mut node = lock_node(&state)?;
    let (session, decision, events) = node.apply_surface_schedule(&session_id, &request.input)?;
    Ok(Json(ApplySurfaceScheduleResponse {
        session,
        decision,
        events,
    }))
}

async fn checkpoint_surface_state<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    Json(request): Json<CheckpointSurfaceStateRequest>,
) -> Result<Json<SurfaceStateCheckpointResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let session_id = surface_session_id(id)?;
    let mut node = lock_node(&state)?;
    let (session, checkpoint, event) =
        node.checkpoint_surface_state(&session_id, request.state, &request.reason)?;
    Ok(Json(SurfaceStateCheckpointResponse {
        session,
        checkpoint,
        event,
    }))
}

async fn get_surface_state_checkpoint<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
) -> Result<Json<SurfaceStateRestoreResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let session_id = surface_session_id(id)?;
    let node = lock_node(&state)?;
    Ok(Json(SurfaceStateRestoreResponse {
        checkpoint: node.surface_state_checkpoint(&session_id)?,
    }))
}

async fn get_surface_blob<P>(
    State(state): State<ApiState<P>>,
    Path(hash): Path<String>,
    Query(query): Query<SurfaceBlobQuery>,
) -> Result<Response, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let hash = hash_value(hash)?;
    let media_type = babble_media::normalize_media_type(query.media_type)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    if !matches!(
        media_type.as_str(),
        "text/html" | "text/css" | "text/javascript" | "application/javascript"
    ) {
        return Err(ApiError::bad_request(format!(
            "unsupported Surface resource media type: {}",
            media_type
        )));
    }
    let node = lock_node(&state)?;
    let (blob, bytes) = node
        .media_blob_bounded(
            &hash,
            &media_type,
            crate::execution::MAX_BUFFERED_BLOB_BYTES,
        )?
        .ok_or_else(|| ApiError::not_found(format!("surface resource not found: {hash}")))?;
    let mut response = bytes.into_response();
    let content_type = HeaderValue::from_str(&blob.media_type)
        .map_err(|err| ApiError::internal(format!("invalid content type: {err}")))?;
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, content_type);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    response.headers_mut().insert(
        HeaderName::from_static("content-security-policy"),
        surface_resource_csp(&blob.media_type),
    );
    response.headers_mut().insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    Ok(response)
}

fn surface_resource_csp(media_type: &str) -> HeaderValue {
    match media_type {
        "text/html" => HeaderValue::from_static(
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'",
        ),
        "text/css" => HeaderValue::from_static(
            "default-src 'none'; style-src 'self'; worker-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'",
        ),
        "text/javascript" | "application/javascript" => HeaderValue::from_static(
            "default-src 'none'; script-src 'self'; connect-src 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'",
        ),
        _ => HeaderValue::from_static(
            "default-src 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'",
        ),
    }
}

async fn define_realtime_room<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<DefineRealtimeRoomRequest>,
) -> Result<Json<DefineRealtimeRoomResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let author_id = identity_id(request.author_id)?;
    let object_id = object_id(request.object_id)?;
    let spec = RoomSpec::new(
        object_id,
        request.name,
        request.schema,
        request.membership,
        request.persistence,
        request.limits.unwrap_or_default(),
    )?;
    let room = spec.clone();
    let mut node = lock_node(&state)?;
    let event = node.define_realtime_room(&author_id, spec)?;
    Ok(Json(DefineRealtimeRoomResponse { event, room }))
}

async fn get_realtime_room<P>(
    State(state): State<ApiState<P>>,
    principal: Option<axum::Extension<crate::auth::Principal>>,
    Path(id): Path<String>,
) -> Result<Json<RealtimeRoomResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let id = realtime_room_id(id)?;
    let node = lock_node(&state)?;
    let room = node
        .realtime_room(&id)
        .ok_or_else(|| ApiError::not_found(format!("realtime room not found: {id}")))?;
    if let Some(axum::Extension(principal)) = principal
        && let babble_realtime::MembershipPolicy::AllowList(members) = &room.spec.membership
        && !members.contains(&IdentityId::new_unchecked(principal.identity_id))
    {
        return Err(ApiError::forbidden());
    }
    Ok(Json(RealtimeRoomResponse { room }))
}

async fn start_realtime_session<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<StartRealtimeSessionRequest>,
) -> Result<Json<StartRealtimeSessionResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let author_id = identity_id(request.author_id)?;
    let room_id = realtime_room_id(request.room_id)?;
    let mut node = lock_node(&state)?;
    let session = node.start_realtime_session(&author_id, &room_id)?;
    Ok(Json(StartRealtimeSessionResponse {
        session,
        receipt: None,
    }))
}

async fn close_realtime_session<P>(
    State(state): State<ApiState<P>>,
    Path(id): Path<String>,
    Json(request): Json<CloseRealtimeSessionRequest>,
) -> Result<Json<CloseRealtimeSessionResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let author_id = identity_id(request.author_id)?;
    let session_id = realtime_session_id(id)?;
    let request_session_id = realtime_session_id(request.session_id)?;
    if request_session_id != session_id {
        return Err(ApiError::bad_request(format!(
            "session id path/body mismatch: {session_id} != {request_session_id}"
        )));
    }
    let object_id = object_id(request.object_id)?;
    let mut node = lock_node(&state)?;
    let (session, event) = node.close_realtime_session(&author_id, &session_id, &object_id)?;
    Ok(Json(CloseRealtimeSessionResponse {
        session,
        event,
        receipt: None,
    }))
}

async fn publish_realtime_message<P>(
    State(state): State<ApiState<P>>,
    Json(request): Json<PublishRealtimeMessageRequest>,
) -> Result<Json<PublishRealtimeMessageResponse>, ApiError>
where
    P: JudgmentProvider + Send + Sync + 'static,
{
    let author_id = identity_id(request.author_id)?;
    let session_id = realtime_session_id(request.session_id)?;
    let object_id = object_id(request.object_id)?;
    let mut node = lock_node(&state)?;
    let (message, snapshot) = node.publish_realtime_message(
        &author_id,
        &session_id,
        &object_id,
        request.payload,
        request.durable,
    )?;
    Ok(Json(PublishRealtimeMessageResponse {
        message,
        snapshot,
        receipt: None,
    }))
}

pub(crate) fn lock_node<P>(
    state: &ApiState<P>,
) -> Result<std::sync::MutexGuard<'_, LocalNode<P>>, ApiError>
where
    P: JudgmentProvider,
{
    let mut node = state
        .node
        .lock()
        .map_err(|_| ApiError::internal("local node lock is poisoned"))?;
    node.check_ready()?;
    crate::auth::check_execution(state, &mut node)?;
    Ok(node)
}

pub(crate) fn identity_id(value: String) -> Result<IdentityId, ApiError> {
    let id = IdentityId::new_unchecked(value);
    id.validate()?;
    Ok(id)
}

pub(crate) fn object_id(value: String) -> Result<ObjectId, ApiError> {
    let id = ObjectId::new_unchecked(value);
    id.validate()?;
    Ok(id)
}

fn edge_id(value: String) -> Result<babble_types::EdgeId, ApiError> {
    let id = babble_types::EdgeId::new_unchecked(value);
    id.validate()?;
    Ok(id)
}

fn judgment_id(value: String) -> Result<JudgmentId, ApiError> {
    let id = JudgmentId::new_unchecked(value);
    id.validate()?;
    Ok(id)
}

pub(crate) fn grant_id(value: String) -> Result<babble_types::CapabilityGrantId, ApiError> {
    let id = babble_types::CapabilityGrantId::new_unchecked(value);
    id.validate()?;
    Ok(id)
}

pub(crate) fn realtime_room_id(value: String) -> Result<babble_types::RealtimeRoomId, ApiError> {
    let id = babble_types::RealtimeRoomId::new_unchecked(value);
    id.validate()?;
    Ok(id)
}

pub(crate) fn realtime_session_id(
    value: String,
) -> Result<babble_types::RealtimeSessionId, ApiError> {
    let id = babble_types::RealtimeSessionId::new_unchecked(value);
    id.validate()?;
    Ok(id)
}

pub(crate) fn surface_session_id(value: String) -> Result<SurfaceSessionId, ApiError> {
    SurfaceSessionId::new(value).map_err(ApiError::from)
}

pub(crate) fn event_id(value: String) -> Result<EventId, ApiError> {
    let id = EventId::new_unchecked(value);
    id.validate()?;
    Ok(id)
}

pub(crate) fn hash_value(value: String) -> Result<Hash, ApiError> {
    let hash = Hash::new_unchecked(value);
    hash.validate()?;
    Ok(hash)
}

pub(crate) fn decode_hex(value: &str) -> Result<Vec<u8>, ApiError> {
    let value = value.trim();
    if value.len() > crate::execution::MAX_BUFFERED_BLOB_BYTES * 2 {
        return Err(ApiError::bad_request("decoded bytes exceed 8 MiB"));
    }
    hex::decode(value).map_err(|err| ApiError::bad_request(format!("invalid hex: {err}")))
}

pub(crate) fn validator_set(values: BTreeMap<String, u64>) -> Result<ValidatorSet, ApiError> {
    ValidatorSet::new(
        values
            .into_iter()
            .map(|(value, weight)| {
                let id = IdentityId::new_unchecked(value);
                id.validate()?;
                Ok((id, weight))
            })
            .collect::<Result<BTreeMap<_, _>, babble_types::Error>>()?,
    )
    .map_err(ApiError::from)
}

fn checkpoint_response(event: Event) -> Result<Json<CheckpointEventResponse>, ApiError> {
    if event.kind != EventKind::ConsensusCheckpoint {
        return Err(ApiError::bad_request(format!(
            "event is not a consensus checkpoint: {}",
            event.id
        )));
    }
    let checkpoint: FinalityCheckpoint = serde_json::from_value(event.payload.clone())
        .map_err(|err| ApiError::bad_request(format!("invalid checkpoint payload: {err}")))?;
    Ok(Json(CheckpointEventResponse { event, checkpoint }))
}

fn personalization_sync_summary(
    record: PersonalizationSyncRecord,
) -> PersonalizationSyncEnvelopeSummary {
    PersonalizationSyncEnvelopeSummary {
        envelope_hash: record.envelope_hash.to_string(),
        identity_id: record.identity_id.to_string(),
        device_id: record.device_id,
        uploaded_at: record.uploaded_at,
        size_bytes: record.size_bytes,
    }
}

fn relation_from_param(value: String) -> Result<Relation, ApiError> {
    let normalized = value.trim().to_ascii_lowercase();
    let relation = match normalized.as_str() {
        "reply_to" => Relation::ReplyTo,
        "references" => Relation::References,
        "quotes" => Relation::Quotes,
        "contains" => Relation::Contains,
        "cites" => Relation::Cites,
        "supports" => Relation::Supports,
        "contradicts" => Relation::Contradicts,
        "evidence_for" => Relation::EvidenceFor,
        "evidence_against" => Relation::EvidenceAgainst,
        "extends" => Relation::Extends,
        "derives_from" => Relation::DerivesFrom,
        "supersedes" => Relation::Supersedes,
        "forks" => Relation::Forks,
        "remixes" => Relation::Remixes,
        "created_by" => Relation::CreatedBy,
        "follows" => Relation::Follows,
        "trusts" => Relation::Trusts,
        custom if custom.starts_with("custom:") => {
            Relation::Custom(custom.trim_start_matches("custom:").to_string())
        }
        _ => {
            return Err(ApiError::bad_request(format!(
                "unsupported relation query parameter: {value}"
            )));
        }
    };
    Ok(relation)
}

#[derive(Serialize)]
struct HealthResponse {
    ok: bool,
    judgment_provider: ProviderVersion,
    ranking_provider: babble_lens::RankingProviderVersion,
    temporal_provider: babble_discovery::TemporalProviderVersion,
}

#[derive(serde::Deserialize)]
struct EdgeQuery {
    relation: Option<String>,
}

#[derive(serde::Deserialize)]
struct MediaBlobQuery {
    media_type: String,
}

#[derive(serde::Deserialize)]
struct SurfaceBlobQuery {
    media_type: String,
}

#[derive(serde::Deserialize)]
struct ObjectSearchParams {
    q: Option<String>,
    author: Option<String>,
    kind: Option<String>,
    limit: Option<usize>,
}
