use crate::{
    ApiState,
    error::ApiError,
    routes::{
        decode_hex, event_id, grant_id, hash_value, identity_id, lock_node, object_id,
        realtime_room_id, realtime_session_id, surface_session_id, validator_set,
    },
    schema::{
        AiEmbedAction, AiEmbedInputModality, AiEmbedRequest, AiEmbedResponse, AiGenerateAction,
        AiGenerateRequest, AiGenerateResponse, AiGenerateTask, AiModality, AiTranscribeAction,
        AiTranscribeRequest, AiTranscribeResponse, ApplySurfaceScheduleResponse,
        CameraCaptureAction, CameraCaptureMode, CameraCaptureRequest, CameraCaptureResponse,
        CameraFacingMode, CapabilitiesResponse, CapabilityCatalogResponse,
        ChangeSurfaceBudgetRequest, CheckpointEventResponse, CheckpointPreviewRequest,
        CheckpointPreviewResponse, CheckpointRequest, CheckpointSurfaceStateRequest,
        ClaimEvidenceResponse,
        CloseRealtimeSessionRequest, CloseRealtimeSessionResponse, CreateIdentityRequest,
        CreateIdentityResponse, DefineRealtimeRoomRequest, DefineRealtimeRoomResponse,
        DiscoveryRequest, DiscoveryResponse, EventBundleRequest, EventBundleResponse,
        EventImportRequest, EventImportResponse, EventListRequest, EventListResponse,
        ForkObjectRequest, GetMediaBlobRequest, GrantCapabilityRequest,
        GrantCapabilityResponse, GraphTraversalResponse, GraphTraverseRpcRequest,
        IdentityCurrentResponse, InferRelationshipRequest, InferRelationshipResponse,
        JudgeObjectResponse, JudgeObjectRpcRequest, JudgmentDefinitionsResponse,
        JudgmentProvidersResponse, LensCatalogResponse, LocalStorageDeleteRequest,
        LocalStorageDeleteResponse, LocalStorageEntry, LocalStorageGetRequest,
        LocalStorageGetResponse, LocalStorageListRequest, LocalStorageListResponse,
        LocalStorageSetRequest, LocalStorageSetResponse, MediaBlobResponse,
        MicrophoneCaptureAction, MicrophoneCaptureMode, MicrophoneCaptureRequest,
        MicrophoneCaptureResponse, NetworkFetchRequest, NetworkFetchResponse,
        NotificationsRequestAction, NotificationsRequestRequest, NotificationsRequestResponse,
        ObjectIdRequest, ObjectJudgmentsResponse, ObjectSearchRequest, ObjectSearchResponse,
        ObjectStorageDeleteRequest, ObjectStorageDeleteResponse, ObjectStorageEntry,
        ObjectStorageGetRequest, ObjectStorageGetResponse, ObjectStorageListRequest,
        ObjectStorageListResponse, ObjectStorageSetRequest, ObjectStorageSetResponse,
        ObservabilitySnapshotResponse, PaymentsCheckoutAction, PaymentsCheckoutRequest,
        PaymentsCheckoutResponse, PersonalizationSyncDeleteResponse,
        PersonalizationSyncEnvelopeSummary, PersonalizationSyncGetRequest,
        PersonalizationSyncGetResponse, PersonalizationSyncListRequest,
        PersonalizationSyncListResponse, PersonalizationSyncPutRequest,
        PersonalizationSyncPutResponse, PrepareSurfaceRequest, PrepareSurfaceResponse,
        ProvenancePublicationResponse, PublishEdgeRequest, PublishEdgeResponse,
        PublishMediaObjectRequest, PublishMediaObjectResponse, PublishObjectRequest,
        PublishObjectResponse, PublishRealtimeMessageRequest, PublishRealtimeMessageResponse,
        PublishTextRequest, PublishTextResponse, PutMediaBlobRequest, RemixObjectRequest,
        RevokeCapabilityRequest, ScheduleSurfaceSessionRequest, ScheduleSurfaceSessionResponse,
        StartRealtimeSessionRequest, StartRealtimeSessionResponse, StartSurfaceSessionRequest,
        SurfaceRuntimeHealthResponse, SurfaceSessionEventResponse, SurfaceSessionRequest,
        SurfaceSessionResponse, SurfaceStateCheckpointResponse, SurfaceStateRestoreResponse,
        TransitionSurfaceSessionRequest,
    },
};
use babble_graph::GraphTraversalSpec;
use babble_hashgraph::FinalityCheckpoint;
use babble_judgment::{JudgmentProvider, JudgmentRegistry};
use babble_lens::BuiltInLens;
use babble_node::{DiscoveryQuery, EventListQuery, ObjectSearchQuery};
use babble_realtime::RoomSpec;
use babble_rpc::{
    RpcCatalog, RpcCatalogError, RpcError, RpcErrorCode, RpcMethodDefinition, RpcRequestEnvelope,
    RpcResponseEnvelope, babble_rpc_catalog,
};
use babble_state::{Event, EventKind};
use babble_store::{LocalStorageRecord, ObjectStorageRecord, PersonalizationSyncRecord};
use babble_types::Canonical;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RpcDispatchReport {
    pub method: String,
    pub authorized_capability: Option<String>,
}

/// Trusted in-process dispatch for native hosts holding their own authorization
/// boundary. HTTP callers must use `router`, which authenticates the principal.
pub fn dispatch_rpc_request<P>(
    state: &ApiState<P>,
    request: RpcRequestEnvelope,
) -> RpcResponseEnvelope
where
    P: JudgmentProvider,
{
    dispatch_request(state, request, None, false)
}

pub(crate) fn dispatch_http_request<P: JudgmentProvider>(
    state: &ApiState<P>,
    request: RpcRequestEnvelope,
    principal: Option<&crate::auth::Principal>,
) -> RpcResponseEnvelope {
    dispatch_request(state, request, principal, true)
}

fn dispatch_request<P: JudgmentProvider>(
    state: &ApiState<P>,
    request: RpcRequestEnvelope,
    principal: Option<&crate::auth::Principal>,
    http: bool,
) -> RpcResponseEnvelope {
    let catalog = match babble_rpc_catalog() {
        Ok(catalog) => catalog,
        Err(error) => {
            return RpcResponseEnvelope::err(
                &RpcCatalog {
                    protocol: request.protocol.clone(),
                    methods: Vec::new(),
                },
                &request,
                catalog_error(error),
            );
        }
    };

    let result = catalog
        .validate_request(&request)
        .map_err(catalog_error)
        .and_then(|()| dispatch_validated(state, &catalog, &request, principal, http));

    match result {
        Ok(value) => RpcResponseEnvelope::ok(&catalog, &request, value),
        Err(error) => RpcResponseEnvelope::err(&catalog, &request, error),
    }
}

fn dispatch_validated<P>(
    state: &ApiState<P>,
    catalog: &RpcCatalog,
    request: &RpcRequestEnvelope,
    principal: Option<&crate::auth::Principal>,
    http: bool,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let method = catalog.get(&request.method).ok_or_else(|| {
        RpcError::new(
            RpcErrorCode::UnsupportedVersion,
            format!("unknown RPC method: {}", request.method.as_str()),
        )
    })?;
    if let Some(object) = &request.binding.object_id {
        lock_node(state).map_err(api_error)?.require_moderation_execution(
            &babble_types::ObjectId::new_unchecked(object.clone()),
        ).map_err(|error| api_error(error.into()))?;
    }
    if crate::invocations::social_method(request.method.as_str()) {
        return crate::invocations::intercept(state, request, principal);
    }
    if crate::invocations::browser::browser_method(request.method.as_str()) {
        return crate::invocations::browser::intercept(state, request, principal);
    }
    if matches!(
        request.method.as_str(),
        "babble.social.follow.v1"
            | "babble.social.unfollow.v1"
            | "babble.social.share.v1"
            | "babble.social.reply.v1"
            | "babble.clipboard.write.v1"
            | "babble.fullscreen.enter.v1"
    ) {
        let replacement = format!("{}2", request.method.as_str().trim_end_matches('1'));
        return Err(RpcError::new(
            RpcErrorCode::UnsupportedVersion,
            "one-use consent requires the  invocation contract",
        )
        .with_details(serde_json::json!({"supported_method": replacement})));
    }
    authorize_method(state, method, request)?;

    match request.method.as_str() {
        "babble.identity.create.v1" => create_identity(state, request),
        "babble.observability.snapshot.v1" => observability_snapshot(state, request),
        "babble.identity.current.v1" => current_identity(state, request),
        "babble.object.publish_text.v1" => publish_text(state, request),
        "babble.object.publish.v1" => publish_object(state, request),
        "babble.object.publish_media.v1" => publish_media_object(state, request),
        "babble.object.fork.v1" => fork_object(state, request),
        "babble.object.remix.v1" => remix_object(state, request),
        "babble.object.get.v1" => get_object(state, request),
        "babble.media.blob.put.v1" => put_media_blob(state, request),
        "babble.media.blob.get.v1" => get_media_blob(state, request),
        "babble.graph.edge.publish.v1" => publish_edge(state, request),
        "babble.graph.relationship.infer.v1" => infer_relationship(state, request),
        "babble.graph.evidence.v1" => get_evidence(state, request),
        "babble.graph.traverse.v1" => traverse_graph(state, request),
        "babble.social.replies.list.v1" => list_replies(state, request),
        "babble.social.quotes.list.v1" => list_quotes(state, request),
        "babble.social.reactions.summary.v1" => {
            let input: crate::ReactionObjectRequest = payload(request)?;
            let node = lock_node(state).map_err(api_error)?;
            encode(
                node.reaction_summary(&object_id(input.object_id).map_err(api_error)?)
                    .map_err(babble_error)?,
            )
        }
        "babble.social.reactions.record.v1" => {
            let input: crate::ReactionRecordRequest = payload(request)?;
            let node = lock_node(state).map_err(api_error)?;
            encode(
                node.reaction_record(
                    &identity_id(input.actor_id).map_err(api_error)?,
                    &object_id(input.object_id).map_err(api_error)?,
                )
                .map_err(babble_error)?,
            )
        }
        "babble.social.reactions.mine.v1" => {
            let actor = reaction_actor(request, principal, http)?;
            let input: crate::ReactionObjectRequest = payload(request)?;
            let node = lock_node(state).map_err(api_error)?;
            encode(
                node.reaction_state(&actor, &object_id(input.object_id).map_err(api_error)?)
                    .map_err(babble_error)?,
            )
        }
        "babble.social.reactions.set.v1" => {
            let actor = reaction_actor(request, principal, http)?;
            let input: crate::SetReactionRpcRequest = payload(request)?;
            let key = request
                .idempotency_key
                .as_deref()
                .ok_or_else(|| api_error(ApiError::bad_request("idempotency key required")))?;
            let mut node = lock_node(state).map_err(api_error)?;
            encode(
                node.set_reaction(
                    &actor,
                    &object_id(input.object_id).map_err(api_error)?,
                    input.value,
                    input.expected_revision,
                    key,
                )
                .map_err(babble_error)?,
            )
        }
        "babble.events.list.v1" => list_events(state, request),
        "babble.events.bundle.v1" => get_event_bundle(state, request),
        "babble.events.import.v1" => import_events(state, request),
        "babble.consensus.checkpoint.preview.v1" => preview_checkpoint(state, request),
        "babble.consensus.checkpoint.publish.v1" => publish_checkpoint(state, request),
        "babble.judgment.definitions.list.v1" => list_judgment_definitions(request),
        "babble.judgment.providers.list.v1" => list_judgment_providers(state, request),
        "babble.judgment.object.evaluate.v1" => judge_object(state, request),
        "babble.ai.judge.v1" => ai_judge(state, request),
        "babble.ai.generate.v1" => ai_generate(state, request),
        "babble.ai.embed.v1" => ai_embed(state, request),
        "babble.ai.transcribe.v1" => ai_transcribe(state, request),
        "babble.judgment.object.list.v1" => list_object_judgments(state, request),
        "babble.search.objects.v1" => search_objects(state, request),
        "babble.lenses.list.v1" => list_lenses(request),
        "babble.discovery.candidates.v1" => discover_candidates(state, request),
        "babble.capabilities.list.v1" => list_capabilities(state, request),
        "babble.capabilities.inspect.v1" => inspect_capabilities(state, request, principal, http),
        "babble.capabilities.grant.v1" => grant_capability(state, request),
        "babble.capabilities.revoke.v1" => revoke_capability(state, request),
        "babble.storage.local.get.v1" => local_storage_get(state, request),
        "babble.storage.local.set.v1" => local_storage_set(state, request),
        "babble.storage.local.delete.v1" => local_storage_delete(state, request),
        "babble.storage.local.list.v1" => local_storage_list(state, request),
        "babble.storage.object.get.v1" => object_storage_get(state, request),
        "babble.storage.object.set.v1" => object_storage_set(state, request),
        "babble.storage.object.delete.v1" => object_storage_delete(state, request),
        "babble.storage.object.list.v1" => object_storage_list(state, request),
        "babble.personalization.sync.put.v1" => personalization_sync_put(state, request),
        "babble.personalization.sync.list.v1" => personalization_sync_list(state, request),
        "babble.personalization.sync.get.v1" => personalization_sync_get(state, request),
        "babble.personalization.sync.delete.v1" => personalization_sync_delete(state, request),
        "babble.network.fetch.v1" => network_fetch(state, request),
        "babble.payments.checkout.v1" => payments_checkout(state, request),
        "babble.notifications.request.v1" => notifications_request(state, request),
        "babble.media.camera.request.v1" => media_camera_request(state, request),
        "babble.media.microphone.request.v1" => media_microphone_request(state, request),
        "babble.runtime.surface.prepare.v1" => {
            if http {
                let input: PrepareSurfaceRequest = payload(request)?;
                let identity = principal
                    .map(|p| babble_types::IdentityId::new_unchecked(p.identity_id.clone()));
                let node = lock_node(state).map_err(api_error)?;
                encode(PrepareSurfaceResponse {
                    plan: crate::gateway::prepare(
                        state.gateway.as_ref(),
                        &node,
                        &object_id(input.object_id).map_err(api_error)?,
                        input.role,
                        identity.as_ref(),
                    )
                    .map_err(api_error)?,
                })
            } else {
                prepare_surface(state, request)
            }
        }
        "babble.runtime.surface.session.start.v1" => {
            if let Some(principal) = principal {
                let input = payload(request)?;
                encode(SurfaceSessionResponse {
                    session: crate::auth::start_surface(state, input, principal)
                        .map_err(api_error)?,
                })
            } else if http {
                Err(api_error(ApiError::unauthorized()))
            } else {
                start_surface_session(state, request)
            }
        }
        "babble.runtime.surface.health.v1" => surface_runtime_health(state, request),
        "babble.runtime.surface.session.heartbeat.v1" => {
            require_host_runtime_binding(request)?;
            let _: crate::schema::EmptyRequest = payload(request)?;
            let principal = principal.ok_or_else(|| api_error(ApiError::unauthorized()))?;
            let id = request.binding.surface_session_id.clone().ok_or_else(|| {
                api_error(ApiError::bad_request("Surface session binding is required"))
            })?;
            encode(
                crate::auth::heartbeat_surface(
                    state,
                    surface_session_id(id).map_err(api_error)?,
                    principal,
                )
                .map_err(api_error)?,
            )
        }
        "babble.runtime.surface.session.get.v1" => get_surface_session(state, request),
        "babble.runtime.surface.session.transition.v1" => transition_surface_session(state, request),
        "babble.runtime.surface.session.budget.v1" => change_surface_session_budget(state, request),
        "babble.runtime.surface.session.schedule.v1" => schedule_surface_session(state, request),
        "babble.runtime.surface.session.apply_schedule.v1" => apply_surface_schedule(state, request),
        "babble.runtime.surface.session.state.checkpoint.v1" => {
            checkpoint_surface_state(state, request)
        }
        "babble.runtime.surface.session.state.get.v1" => {
            get_surface_state_checkpoint(state, request)
        }
        "babble.realtime.room.define.v1" => define_realtime_room(state, request),
        "babble.realtime.session.start.v1" => start_realtime_session(state, request),
        "babble.realtime.session.leave.v1" => close_realtime_session(state, request),
        "babble.realtime.message.publish.v1" => publish_realtime_message(state, request),
        _ => Err(RpcError::new(
            RpcErrorCode::UnsupportedVersion,
            format!(
                "RPC method is cataloged but has no local host handler: {}",
                request.method.as_str()
            ),
        )),
    }
}

fn authorize_method<P>(
    state: &ApiState<P>,
    method: &RpcMethodDefinition,
    request: &RpcRequestEnvelope,
) -> Result<(), RpcError>
where
    P: JudgmentProvider,
{
    let Some(requirement) = &method.capability else {
        return Ok(());
    };
    if !requirement.required && request.binding.capability_grants.is_empty() {
        return Ok(());
    }
    let object_id = request
        .binding
        .object_id
        .clone()
        .ok_or_else(|| {
            RpcError::new(
                RpcErrorCode::CapabilityDenied,
                "RPC capability checks require an Object binding",
            )
        })
        .and_then(|id| object_id(id).map_err(api_error))?;
    let node = lock_node(state).map_err(api_error)?;
    node.authorize_capability_binding(
        &object_id,
        &requirement.capability,
        requirement.version,
        &request.binding.capability_grants,
    )
    .map_err(capability_error)?;
    Ok(())
}

fn observability_snapshot<P>(
    state: &ApiState<P>,
    _request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let node = lock_node(state).map_err(api_error)?;
    encode(ObservabilitySnapshotResponse {
        snapshot: node.observability_snapshot().map_err(babble_error)?,
    })
}

fn require_host_runtime_binding(request: &RpcRequestEnvelope) -> Result<(), RpcError> {
    if request.binding.object_id.is_some() {
        return Err(RpcError::new(
            RpcErrorCode::CapabilityDenied,
            "Surface runtime session mutation requires a trusted host runtime binding",
        ));
    }
    Ok(())
}

fn reaction_actor(
    request: &RpcRequestEnvelope,
    principal: Option<&crate::auth::Principal>,
    http: bool,
) -> Result<babble_types::IdentityId, RpcError> {
    if request.binding.object_id.is_some() || request.binding.surface_session_id.is_some() {
        return Err(api_error(ApiError::forbidden()));
    }
    let actor = principal
        .map(|p| p.identity_id.as_str())
        .or_else(|| {
            if http {
                None
            } else {
                request.binding.identity_id.as_deref()
            }
        })
        .ok_or_else(|| api_error(ApiError::unauthorized()))?;
    identity_id(actor.to_owned()).map_err(api_error)
}

fn create_identity<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: CreateIdentityRequest = payload(request)?;
    if input.handle.trim().is_empty() {
        return Err(RpcError::new(
            RpcErrorCode::InvalidInput,
            "identity handle must not be empty",
        ));
    }
    let mut node = lock_node(state).map_err(api_error)?;
    encode(CreateIdentityResponse {
        identity: node
            .create_identity(input.kind, input.handle.trim())
            .map_err(babble_error)?,
    })
}

fn current_identity<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let object_id = bound_object_id(request)?;
    let identity_id = request
        .binding
        .identity_id
        .clone()
        .ok_or_else(|| {
            RpcError::new(
                RpcErrorCode::CapabilityDenied,
                "identity.current requires a trusted host identity binding",
            )
        })
        .and_then(|id| identity_id(id).map_err(api_error))?;
    let node = lock_node(state).map_err(api_error)?;
    let (identity, receipt) = node
        .identity_current(&object_id, &identity_id, &request.binding.capability_grants)
        .map_err(babble_error)?;
    encode(IdentityCurrentResponse { identity, receipt })
}

fn publish_text<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: PublishTextRequest = payload(request)?;
    if input.text.trim().is_empty() {
        return Err(RpcError::new(
            RpcErrorCode::InvalidInput,
            "text object must not be empty",
        ));
    }
    let author_id = identity_id(input.author_id).map_err(api_error)?;
    publication(state, request, &author_id, |node| {
        Ok(PublishTextResponse {
            object: node.publish_text(&author_id, input.text.trim())?,
        })
    })
}

fn publish_object<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: PublishObjectRequest = payload(request)?;
    let author_id = identity_id(input.author_id).map_err(api_error)?;
    publication(state, request, &author_id, |node| {
        Ok(PublishObjectResponse {
            object: node.publish_draft(&author_id, input.draft)?,
        })
    })
}

fn publish_media_object<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: PublishMediaObjectRequest = payload(request)?;
    let author_id = identity_id(input.author_id).map_err(api_error)?;
    publication(state, request, &author_id, |node| {
        Ok(PublishMediaObjectResponse {
            object: node.publish_media_object(
                &author_id,
                &input.title,
                input.description,
                input.resources,
            )?,
        })
    })
}

fn get_object<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input = object_id_request(request)?;
    let id = object_id(input.object_id).map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    let object = node
        .object(&id)
        .ok_or_else(|| RpcError::new(RpcErrorCode::NotFound, format!("object not found: {id}")))?
        .clone();
    encode(PublishTextResponse { object })
}

fn fork_object<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: ForkObjectRequest = payload(request)?;
    let author_id = identity_id(input.author_id).map_err(api_error)?;
    let source = object_id(input.source_object_id).map_err(api_error)?;
    publication(state, request, &author_id, |node| {
        Ok(ProvenancePublicationResponse::from(node.fork_object(
            &author_id,
            &source,
            input.draft,
        )?))
    })
}

fn remix_object<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: RemixObjectRequest = payload(request)?;
    let author_id = identity_id(input.author_id).map_err(api_error)?;
    let sources = input
        .source_object_ids
        .into_iter()
        .map(object_id)
        .collect::<Result<Vec<_>, _>>()
        .map_err(api_error)?;
    publication(state, request, &author_id, |node| {
        Ok(ProvenancePublicationResponse::from(node.remix_object(
            &author_id,
            sources,
            input.draft,
        )?))
    })
}

fn put_media_blob<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: PutMediaBlobRequest = payload(request)?;
    let bytes = decode_hex(&input.bytes_hex).map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    encode(MediaBlobResponse {
        blob: node
            .put_media_blob(&input.media_type, &bytes)
            .map_err(babble_error)?,
        bytes_hex: None,
    })
}

fn get_media_blob<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: GetMediaBlobRequest = payload(request)?;
    let hash = hash_value(input.hash).map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    let (blob, bytes) = node
        .media_blob_bounded(
            &hash,
            &input.media_type,
            crate::execution::MAX_BUFFERED_BLOB_BYTES,
        )
        .map_err(|error| api_error(error.into()))?
        .ok_or_else(|| {
            RpcError::new(
                RpcErrorCode::NotFound,
                format!("media blob not found: {hash}"),
            )
        })?;
    encode(MediaBlobResponse {
        blob,
        bytes_hex: Some(hex::encode(bytes)),
    })
}

fn publish_edge<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: PublishEdgeRequest = payload(request)?;
    let author_id = identity_id(input.author_id).map_err(api_error)?;
    let source = object_id(input.source).map_err(api_error)?;
    let target = object_id(input.target).map_err(api_error)?;
    publication(state, request, &author_id, |node| {
        Ok(PublishEdgeResponse {
            edge: node.publish_edge(&author_id, source, target, input.relation, input.origin)?,
        })
    })
}

fn infer_relationship<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: InferRelationshipRequest = payload(request)?;
    let author_id = identity_id(input.author_id).map_err(api_error)?;
    let source = object_id(input.source).map_err(api_error)?;
    let target = object_id(input.target).map_err(api_error)?;
    let mut node = lock_node(state).map_err(api_error)?;
    let (edge, judgment) = node
        .infer_relationship_edge(
            &author_id,
            &source,
            &target,
            input.relation.as_parameter(),
            input.min_score.unwrap_or(0.5),
        )
        .map_err(babble_error)?;
    encode(InferRelationshipResponse { edge, judgment })
}

fn get_evidence<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: ObjectIdRequest = payload(request)?;
    let id = object_id(input.object_id).map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    encode(ClaimEvidenceResponse {
        projection: node.evidence_projection(&id).map_err(babble_error)?,
    })
}

fn traverse_graph<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: GraphTraverseRpcRequest = payload(request)?;
    let root = object_id(input.object_id).map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    encode(GraphTraversalResponse {
        traversal: node
            .traverse_graph(&GraphTraversalSpec {
                root,
                direction: input.direction,
                relations: input.relations,
                max_depth: input.max_depth.clamp(1, 8),
                limit: input.limit.clamp(1, 512),
            })
            .map_err(babble_error)?,
    })
}

fn list_quotes<P: JudgmentProvider>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError> {
    let input: crate::schema::QuotesListRequest = payload(request)?;
    let node = lock_node(state).map_err(api_error)?;
    encode(node.list_quotes(&input).map_err(babble_error)?)
}

fn list_replies<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: crate::schema::RepliesListRequest = payload(request)?;
    let node = lock_node(state).map_err(api_error)?;
    encode(node.list_replies(&input).map_err(babble_error)?)
}

fn publication<P: JudgmentProvider, T: Serialize>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
    author: &babble_types::IdentityId,
    operation: impl FnOnce(&mut babble_node::LocalNode<P>) -> babble_types::Result<T>,
) -> Result<Value, RpcError> {
    let key = request
        .idempotency_key
        .as_deref()
        .filter(|key| !key.trim().is_empty() && key.len() <= 256)
        .ok_or_else(|| {
            RpcError::new(
                RpcErrorCode::InvalidInput,
                "publication idempotency key must be 1-256 bytes",
            )
        })?;
    let id = serde_json::json!({
        "version": 1, "author": author, "origin": request.binding.origin,
        "object": request.binding.object_id, "key": key,
    })
    .canonical_hash()
    .map_err(babble_error)?;
    let mut grants = request.binding.capability_grants.clone();
    grants.sort();
    grants.dedup();
    let fingerprint = serde_json::json!({
        "version": 1, "method": request.method, "payload": request.payload,
        "grants": grants,
    })
    .canonical_hash()
    .map_err(babble_error)?;
    let mut node = lock_node(state).map_err(api_error)?;
    let result = node
        .with_publication_request(
            babble_store::PublicationRequest {
                id,
                fingerprint,
                author: author.clone(),
            },
            operation,
        )
        .map_err(babble_error)?;
    encode(result)
}

fn list_events<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: EventListRequest = payload(request)?;
    let limit = input.limit.unwrap_or(100);
    if limit > 500 {
        return Err(RpcError::new(
            RpcErrorCode::InvalidInput,
            "event list limit must be at most 500",
        ));
    }
    let after = input.after.map(event_id).transpose().map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    let listed = node
        .list_events(EventListQuery { after, limit })
        .map_err(babble_error)?;
    encode(EventListResponse {
        next_after: listed.next_after.map(|id| id.to_string()),
        events: listed.events,
    })
}

fn get_event_bundle<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: EventBundleRequest = payload(request)?;
    if input.events.is_empty() {
        return Err(RpcError::new(
            RpcErrorCode::InvalidInput,
            "event bundle request must include at least one event",
        ));
    }
    let event_ids = input
        .events
        .into_iter()
        .map(event_id)
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    encode(EventBundleResponse {
        bundle: node.event_bundle(&event_ids).map_err(babble_error)?,
    })
}

fn import_events<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: EventImportRequest = payload(request)?;
    if input.bundle.events.is_empty()
        && input.bundle.objects.is_empty()
        && input.bundle.edges.is_empty()
        && input.bundle.identities.is_empty()
    {
        return Err(RpcError::new(
            RpcErrorCode::InvalidInput,
            "event import bundle must not be empty",
        ));
    }
    let mut node = lock_node(state).map_err(api_error)?;
    encode(EventImportResponse {
        report: node.import_bundle(input.bundle).map_err(babble_error)?,
    })
}

fn preview_checkpoint<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: CheckpointPreviewRequest = payload(request)?;
    let validators = validator_set(input.validators).map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    encode(CheckpointPreviewResponse {
        checkpoint: node.finality_checkpoint(&validators).map_err(babble_error)?,
    })
}

fn publish_checkpoint<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: CheckpointRequest = payload(request)?;
    let author_id = identity_id(input.author_id).map_err(api_error)?;
    let validators = validator_set(input.validators).map_err(api_error)?;
    let mut node = lock_node(state).map_err(api_error)?;
    let event = node
        .publish_checkpoint(&author_id, validators)
        .map_err(babble_error)?;
    encode(checkpoint_response(event)?)
}

fn list_judgment_definitions(request: &RpcRequestEnvelope) -> Result<Value, RpcError> {
    let _: crate::EmptyRequest = payload(request)?;
    encode(JudgmentDefinitionsResponse {
        definitions: JudgmentRegistry::babble_core().definitions,
    })
}

fn list_judgment_providers<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let _: crate::EmptyRequest = payload(request)?;
    let node = lock_node(state).map_err(api_error)?;
    encode(JudgmentProvidersResponse {
        providers: node.judgment_provider_descriptors(),
    })
}

fn judge_object<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: JudgeObjectRpcRequest = payload(request)?;
    let object_id = object_id(input.object_id).map_err(api_error)?;
    let mut node = lock_node(state).map_err(api_error)?;
    let orchestration = node
        .judge_object_orchestrated(&object_id, input.definition, input.parameters)
        .map_err(babble_error)?;
    encode(JudgeObjectResponse {
        input: node
            .object_judgment_input(&orchestration.judgment.id)
            .map_err(babble_error)?,
        judgment: orchestration.judgment.clone(),
        orchestration: Some(orchestration),
        receipt: None,
    })
}

fn ai_judge<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: JudgeObjectRpcRequest = payload(request)?;
    let source_object_id = bound_object_id(request)?;
    let target_object_id = object_id(input.object_id).map_err(api_error)?;
    let mut node = lock_node(state).map_err(api_error)?;
    let (orchestration, receipt) = node
        .capability_judge_object(
            &source_object_id,
            &target_object_id,
            input.definition,
            input.parameters,
            &request.binding.capability_grants,
        )
        .map_err(capability_error)?;
    encode(JudgeObjectResponse {
        input: node
            .object_judgment_input(&orchestration.judgment.id)
            .map_err(babble_error)?,
        judgment: orchestration.judgment.clone(),
        orchestration: Some(orchestration),
        receipt: Some(receipt),
    })
}

fn ai_generate<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: AiGenerateRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let task = ai_generate_task(&input.task);
    let output_modalities = input
        .output_modalities
        .iter()
        .map(ai_modality)
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let node = lock_node(state).map_err(api_error)?;
    let receipt = node
        .ai_generate_request(
            &object_id,
            &input.purpose,
            task,
            &input.prompt,
            &output_modalities,
            input.model.as_deref(),
            input.max_output_tokens,
            input.temperature_millis,
            &request.binding.capability_grants,
        )
        .map_err(capability_error)?;
    encode(AiGenerateResponse {
        action: AiGenerateAction {
            kind: "ai.generate".to_string(),
            purpose: input.purpose,
            task: input.task,
            prompt: input.prompt,
            output_modalities: input.output_modalities,
            model: input.model,
            max_output_tokens: input.max_output_tokens,
            temperature_millis: input.temperature_millis,
            requires_user_activation: false,
        },
        receipt,
    })
}

fn ai_embed<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: AiEmbedRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let input_modality = ai_embed_modality(&input.input_modality);
    let node = lock_node(state).map_err(api_error)?;
    let receipt = node
        .ai_embed_request(
            &object_id,
            &input.purpose,
            input_modality,
            &input.inputs,
            input.model.as_deref(),
            input.dimensions,
            &request.binding.capability_grants,
        )
        .map_err(capability_error)?;
    encode(AiEmbedResponse {
        action: AiEmbedAction {
            kind: "ai.embed".to_string(),
            purpose: input.purpose,
            input_modality: input.input_modality,
            inputs: input.inputs,
            model: input.model,
            dimensions: input.dimensions,
            requires_user_activation: false,
        },
        receipt,
    })
}

fn ai_transcribe<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: AiTranscribeRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let node = lock_node(state).map_err(api_error)?;
    let receipt = node
        .ai_transcribe_request(
            &object_id,
            &input.purpose,
            &input.media_uri,
            &input.media_type,
            input.model.as_deref(),
            input.language.as_deref(),
            input.max_duration_ms,
            &request.binding.capability_grants,
        )
        .map_err(capability_error)?;
    encode(AiTranscribeResponse {
        action: AiTranscribeAction {
            kind: "ai.transcribe".to_string(),
            purpose: input.purpose,
            media_uri: input.media_uri,
            media_type: input.media_type,
            model: input.model,
            language: input.language,
            max_duration_ms: input.max_duration_ms,
            requires_user_activation: false,
        },
        receipt,
    })
}

fn list_object_judgments<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input = object_id_request(request)?;
    let object_id = object_id(input.object_id).map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    encode(ObjectJudgmentsResponse {
        object_id: object_id.to_string(),
        judgments: node.object_judgments(&object_id).map_err(babble_error)?,
    })
}

fn search_objects<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: ObjectSearchRequest = payload(request)?;
    let author = input
        .author
        .map(identity_id)
        .transpose()
        .map_err(api_error)?;
    let limit = input.limit.unwrap_or(50);
    if limit > 200 {
        return Err(RpcError::new(
            RpcErrorCode::InvalidInput,
            "search limit must be at most 200",
        ));
    }
    let node = lock_node(state).map_err(api_error)?;
    encode(ObjectSearchResponse {
        results: node
            .search_objects(ObjectSearchQuery {
                query: input.q,
                author,
                kind: input.kind,
                limit,
            })
            .map_err(babble_error)?,
    })
}

fn list_lenses(request: &RpcRequestEnvelope) -> Result<Value, RpcError> {
    let _: crate::EmptyRequest = payload(request)?;
    encode(LensCatalogResponse {
        lenses: BuiltInLens::all()
            .into_iter()
            .map(|lens| lens.definition())
            .collect(),
    })
}

fn discover_candidates<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: DiscoveryRequest = payload(request)?;
    let anchors = input
        .anchors
        .into_iter()
        .map(object_id)
        .collect::<Result<Vec<_>, _>>()
        .map_err(api_error)?;
    let followed_objects = input
        .followed_objects
        .into_iter()
        .map(object_id)
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(api_error)?;
    let limit = input.limit.unwrap_or(50);
    if limit > 200 {
        return Err(RpcError::new(
            RpcErrorCode::InvalidInput,
            "discovery limit must be at most 200",
        ));
    }
    let mut query = DiscoveryQuery {
        anchors,
        search: input.search,
        followed_objects,
        limit,
        exploration_slots: input.exploration_slots.unwrap_or(5).min(limit),
        ..Default::default()
    };
    if let Some(lens) = input.lens {
        query.lens = lens;
    }
    let mut node = lock_node(state).map_err(api_error)?;
    encode(DiscoveryResponse {
        discovery: node.discover_objects(query).map_err(babble_error)?,
    })
}

fn list_capabilities<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let _: crate::EmptyRequest = payload(request)?;
    let node = lock_node(state).map_err(api_error)?;
    encode(CapabilityCatalogResponse {
        capabilities: node.capability_definitions(),
    })
}

fn inspect_capabilities<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
    principal: Option<&crate::auth::Principal>,
    http: bool,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input = object_id_request(request)?;
    let id = object_id(input.object_id).map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    let (decisions, grants) = if http {
        let identity =
            principal.map(|p| babble_types::IdentityId::new_unchecked(p.identity_id.clone()));
        node.capability_review_for_identity(&id, identity.as_ref())
            .map_err(babble_error)?
    } else {
        (
            node.capability_decisions(&id).map_err(babble_error)?,
            node.capability_grants(&id).map_err(babble_error)?,
        )
    };
    encode(CapabilitiesResponse {
        manifest: node.capability_manifest(&id).map_err(babble_error)?,
        decisions,
        grants,
    })
}

fn grant_capability<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: GrantCapabilityRequest = payload(request)?;
    let author_id = identity_id(input.author_id).map_err(api_error)?;
    let object_id = object_id(input.object_id).map_err(api_error)?;
    publication(state, request, &author_id, |node| {
        let event =
            node.grant_capability(&author_id, &object_id, input.capability, input.decision)?;
        let grants = node.capability_grants_for_identity(&object_id, &author_id)?;
        Ok(GrantCapabilityResponse { event, grants })
    })
}

fn revoke_capability<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: RevokeCapabilityRequest = payload(request)?;
    let author_id = identity_id(input.author_id).map_err(api_error)?;
    let object_id = object_id(input.object_id).map_err(api_error)?;
    let grant_id = grant_id(input.grant_id).map_err(api_error)?;
    publication(state, request, &author_id, |node| {
        let event = node.revoke_capability(&author_id, &object_id, &grant_id)?;
        let grants = node.capability_grants_for_identity(&object_id, &author_id)?;
        Ok(GrantCapabilityResponse { event, grants })
    })
}

fn local_storage_get<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: LocalStorageGetRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let identity_id = bound_identity_id(request)?;
    let node = lock_node(state).map_err(api_error)?;
    let (entry, receipt) = node
        .local_storage_get(
            &object_id,
            &identity_id,
            &input.key,
            &request.binding.capability_grants,
        )
        .map_err(babble_error)?;
    encode(LocalStorageGetResponse {
        entry: entry.map(local_storage_entry),
        receipt,
    })
}

fn local_storage_set<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: LocalStorageSetRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let identity_id = bound_identity_id(request)?;
    let node = lock_node(state).map_err(api_error)?;
    let (entry, receipt) = node
        .local_storage_set(
            &object_id,
            &identity_id,
            &input.key,
            input.value,
            &request.binding.capability_grants,
        )
        .map_err(babble_error)?;
    encode(LocalStorageSetResponse {
        entry: local_storage_entry(entry),
        receipt,
    })
}

fn local_storage_delete<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: LocalStorageDeleteRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let identity_id = bound_identity_id(request)?;
    let node = lock_node(state).map_err(api_error)?;
    let (deleted, receipt) = node
        .local_storage_delete(
            &object_id,
            &identity_id,
            &input.key,
            &request.binding.capability_grants,
        )
        .map_err(babble_error)?;
    encode(LocalStorageDeleteResponse {
        deleted: deleted.map(local_storage_entry),
        receipt,
    })
}

fn local_storage_list<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: LocalStorageListRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let identity_id = bound_identity_id(request)?;
    let node = lock_node(state).map_err(api_error)?;
    let (entries, receipt) = node
        .local_storage_list(
            &object_id,
            &identity_id,
            input.prefix.as_deref(),
            input.limit.unwrap_or(64),
            &request.binding.capability_grants,
        )
        .map_err(babble_error)?;
    encode(LocalStorageListResponse {
        entries: entries.into_iter().map(local_storage_entry).collect(),
        receipt,
    })
}

fn object_storage_get<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: ObjectStorageGetRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let node = lock_node(state).map_err(api_error)?;
    let (entry, receipt) = node
        .object_storage_get(&object_id, &input.key, &request.binding.capability_grants)
        .map_err(babble_error)?;
    encode(ObjectStorageGetResponse {
        entry: entry.map(object_storage_entry),
        receipt,
    })
}

fn object_storage_set<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: ObjectStorageSetRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let node = lock_node(state).map_err(api_error)?;
    let (entry, receipt) = node
        .object_storage_set(
            &object_id,
            &input.key,
            input.value,
            &request.binding.capability_grants,
        )
        .map_err(babble_error)?;
    encode(ObjectStorageSetResponse {
        entry: object_storage_entry(entry),
        receipt,
    })
}

fn object_storage_delete<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: ObjectStorageDeleteRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let node = lock_node(state).map_err(api_error)?;
    let (deleted, receipt) = node
        .object_storage_delete(&object_id, &input.key, &request.binding.capability_grants)
        .map_err(babble_error)?;
    encode(ObjectStorageDeleteResponse {
        deleted: deleted.map(object_storage_entry),
        receipt,
    })
}

fn object_storage_list<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: ObjectStorageListRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let node = lock_node(state).map_err(api_error)?;
    let (entries, receipt) = node
        .object_storage_list(
            &object_id,
            input.prefix.as_deref(),
            input.limit.unwrap_or(64),
            &request.binding.capability_grants,
        )
        .map_err(babble_error)?;
    encode(ObjectStorageListResponse {
        entries: entries.into_iter().map(object_storage_entry).collect(),
        receipt,
    })
}

fn personalization_sync_put<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: PersonalizationSyncPutRequest = payload(request)?;
    let node = lock_node(state).map_err(api_error)?;
    let record = node
        .put_personalization_sync_envelope(input.envelope)
        .map_err(babble_error)?;
    encode(PersonalizationSyncPutResponse {
        envelope: personalization_sync_summary(record),
    })
}

fn personalization_sync_list<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: PersonalizationSyncListRequest = payload(request)?;
    let identity_id = identity_id(input.identity_id).map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    let records = node
        .list_personalization_sync_envelopes(
            &identity_id,
            input.device_id.as_deref(),
            input.limit.unwrap_or(64),
        )
        .map_err(babble_error)?;
    encode(PersonalizationSyncListResponse {
        envelopes: records
            .into_iter()
            .map(personalization_sync_summary)
            .collect(),
    })
}

fn personalization_sync_get<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: PersonalizationSyncGetRequest = payload(request)?;
    let identity_id = identity_id(input.identity_id).map_err(api_error)?;
    let envelope_hash = hash_value(input.envelope_hash).map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    let record = node
        .personalization_sync_envelope(&identity_id, &input.device_id, &envelope_hash)
        .map_err(babble_error)?;
    encode(PersonalizationSyncGetResponse {
        envelope: record.envelope.clone(),
        summary: personalization_sync_summary(record),
    })
}

fn personalization_sync_delete<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: PersonalizationSyncGetRequest = payload(request)?;
    let identity_id = identity_id(input.identity_id).map_err(api_error)?;
    let envelope_hash = hash_value(input.envelope_hash).map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    let deleted = node
        .delete_personalization_sync_envelope(&identity_id, &input.device_id, &envelope_hash)
        .map_err(babble_error)?;
    encode(PersonalizationSyncDeleteResponse {
        deleted: deleted.map(personalization_sync_summary),
    })
}

fn network_fetch<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: NetworkFetchRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let body = input
        .body_hex
        .map(|value| decode_hex(&value))
        .transpose()
        .map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    let fetched = node
        .network_fetch(
            &object_id,
            &input.method,
            &input.url,
            input.headers,
            body,
            &request.binding.capability_grants,
        )
        .map_err(babble_error)?;
    encode(NetworkFetchResponse {
        status: fetched.status,
        headers: fetched.headers,
        body_hex: hex::encode(fetched.body),
        receipt: fetched.receipt,
    })
}

fn payments_checkout<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: PaymentsCheckoutRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let line_items = input
        .line_items
        .iter()
        .map(|item| (item.label.clone(), item.amount_minor, item.quantity))
        .collect::<Vec<_>>();
    let node = lock_node(state).map_err(api_error)?;
    let receipt = node
        .payments_checkout(
            &object_id,
            input.merchant_id.as_deref(),
            &input.merchant_name,
            &input.currency,
            input.total_amount_minor,
            &line_items,
            input.success_url.as_deref(),
            input.cancel_url.as_deref(),
            input.reference.as_deref(),
            &request.binding.capability_grants,
        )
        .map_err(capability_error)?;
    encode(PaymentsCheckoutResponse {
        action: PaymentsCheckoutAction {
            kind: "payments.checkout".to_string(),
            merchant_id: input.merchant_id,
            merchant_name: input.merchant_name,
            currency: input.currency,
            total_amount_minor: input.total_amount_minor,
            line_items: input.line_items,
            success_url: input.success_url,
            cancel_url: input.cancel_url,
            reference: input.reference,
            requires_user_activation: true,
        },
        receipt,
    })
}

fn notifications_request<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: NotificationsRequestRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let node = lock_node(state).map_err(api_error)?;
    let receipt = node
        .notifications_request(
            &object_id,
            &input.purpose,
            &input.categories,
            &request.binding.capability_grants,
        )
        .map_err(capability_error)?;
    encode(NotificationsRequestResponse {
        action: NotificationsRequestAction {
            kind: "notifications.request".to_string(),
            purpose: input.purpose,
            categories: input.categories,
            requires_user_activation: true,
        },
        receipt,
    })
}

fn media_camera_request<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: CameraCaptureRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let mode = camera_mode(&input.mode);
    let facing_mode = input.facing_mode.unwrap_or(CameraFacingMode::Any);
    let facing_mode_text = camera_facing_mode(&facing_mode);
    let node = lock_node(state).map_err(api_error)?;
    let receipt = node
        .media_camera_request(
            &object_id,
            &input.purpose,
            mode,
            &input.media_types,
            input.max_duration_ms,
            Some(facing_mode_text),
            input.width,
            input.height,
            &request.binding.capability_grants,
        )
        .map_err(capability_error)?;
    encode(CameraCaptureResponse {
        action: CameraCaptureAction {
            kind: "media.camera.request".to_string(),
            purpose: input.purpose,
            mode: input.mode,
            media_types: input.media_types,
            max_duration_ms: input.max_duration_ms,
            facing_mode,
            width: input.width,
            height: input.height,
            requires_user_activation: true,
        },
        receipt,
    })
}

fn media_microphone_request<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: MicrophoneCaptureRequest = payload(request)?;
    let object_id = bound_object_id(request)?;
    let mode = microphone_mode(&input.mode);
    let node = lock_node(state).map_err(api_error)?;
    let receipt = node
        .media_microphone_request(
            &object_id,
            &input.purpose,
            mode,
            &input.media_types,
            input.max_duration_ms,
            &request.binding.capability_grants,
        )
        .map_err(capability_error)?;
    encode(MicrophoneCaptureResponse {
        action: MicrophoneCaptureAction {
            kind: "media.microphone.request".to_string(),
            purpose: input.purpose,
            mode: input.mode,
            media_types: input.media_types,
            max_duration_ms: input.max_duration_ms,
            echo_cancellation: input.echo_cancellation,
            noise_suppression: input.noise_suppression,
            requires_user_activation: true,
        },
        receipt,
    })
}


fn prepare_surface<P>(state: &ApiState<P>, request: &RpcRequestEnvelope) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: PrepareSurfaceRequest = payload(request)?;
    let id = object_id(input.object_id).map_err(api_error)?;
    let node = lock_node(state).map_err(api_error)?;
    encode(PrepareSurfaceResponse {
        plan: node.prepare_surface(&id, input.role).map_err(babble_error)?,
    })
}

fn start_surface_session<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    require_host_runtime_binding(request)?;
    let input: StartSurfaceSessionRequest = payload(request)?;
    let object_id = object_id(input.object_id).map_err(api_error)?;
    let mut node = lock_node(state).map_err(api_error)?;
    encode(SurfaceSessionResponse {
        session: node
            .start_surface_session(&object_id, input.role, input.session_id)
            .map_err(babble_error)?,
    })
}

fn surface_runtime_health<P>(
    state: &ApiState<P>,
    _request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let node = lock_node(state).map_err(api_error)?;
    encode(SurfaceRuntimeHealthResponse {
        health: node.surface_runtime_health(),
    })
}

fn get_surface_session<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: SurfaceSessionRequest = payload(request)?;
    if let Some(bound_session) = request.binding.surface_session_id.as_ref()
        && bound_session != input.session_id.as_str()
    {
        return Err(RpcError::new(
            RpcErrorCode::CapabilityDenied,
            "Object-bound runtime calls may only inspect their bound Surface session",
        ));
    }
    let node = lock_node(state).map_err(api_error)?;
    encode(SurfaceSessionResponse {
        session: node
            .surface_session(&input.session_id)
            .map_err(babble_error)?,
    })
}

fn transition_surface_session<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    require_host_runtime_binding(request)?;
    let session_id = request
        .binding
        .surface_session_id
        .clone()
        .ok_or_else(|| {
            RpcError::new(
                RpcErrorCode::InvalidInput,
                "host runtime binding must include the Surface session id",
            )
        })
        .and_then(|id| surface_session_id(id).map_err(api_error))?;
    let input: TransitionSurfaceSessionRequest = payload(request)?;
    let mut node = lock_node(state).map_err(api_error)?;
    let (session, event) = node
        .transition_surface_session(&session_id, input.lifecycle, &input.reason)
        .map_err(babble_error)?;
    state
        .auth
        .retire_evicted_surface(&session)
        .map_err(api_error)?;
    encode(SurfaceSessionEventResponse { session, event })
}

fn change_surface_session_budget<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    require_host_runtime_binding(request)?;
    let session_id = request
        .binding
        .surface_session_id
        .clone()
        .ok_or_else(|| {
            RpcError::new(
                RpcErrorCode::InvalidInput,
                "host runtime binding must include the Surface session id",
            )
        })
        .and_then(|id| surface_session_id(id).map_err(api_error))?;
    let input: ChangeSurfaceBudgetRequest = payload(request)?;
    let mut node = lock_node(state).map_err(api_error)?;
    let (session, event) = node
        .reduce_surface_session_budget(&session_id, input.budget, &input.reason)
        .map_err(babble_error)?;
    encode(SurfaceSessionEventResponse { session, event })
}

fn schedule_surface_session<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    require_host_runtime_binding(request)?;
    let session_id = request
        .binding
        .surface_session_id
        .clone()
        .ok_or_else(|| {
            RpcError::new(
                RpcErrorCode::InvalidInput,
                "host runtime binding must include the Surface session id",
            )
        })
        .and_then(|id| surface_session_id(id).map_err(api_error))?;
    let input: ScheduleSurfaceSessionRequest = payload(request)?;
    let node = lock_node(state).map_err(api_error)?;
    encode(ScheduleSurfaceSessionResponse {
        decision: node
            .schedule_surface_session(&session_id, &input.input)
            .map_err(babble_error)?,
    })
}

fn apply_surface_schedule<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    require_host_runtime_binding(request)?;
    let session_id = request
        .binding
        .surface_session_id
        .clone()
        .ok_or_else(|| {
            RpcError::new(
                RpcErrorCode::InvalidInput,
                "host runtime binding must include the Surface session id",
            )
        })
        .and_then(|id| surface_session_id(id).map_err(api_error))?;
    let input: ScheduleSurfaceSessionRequest = payload(request)?;
    let mut node = lock_node(state).map_err(api_error)?;
    let (session, decision, events) = node
        .apply_surface_schedule(&session_id, &input.input)
        .map_err(babble_error)?;
    encode(ApplySurfaceScheduleResponse {
        session,
        decision,
        events,
    })
}

fn checkpoint_surface_state<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    require_host_runtime_binding(request)?;
    let session_id = request
        .binding
        .surface_session_id
        .clone()
        .ok_or_else(|| {
            RpcError::new(
                RpcErrorCode::InvalidInput,
                "host runtime binding must include the Surface session id",
            )
        })
        .and_then(|id| surface_session_id(id).map_err(api_error))?;
    let input: CheckpointSurfaceStateRequest = payload(request)?;
    let mut node = lock_node(state).map_err(api_error)?;
    let (session, checkpoint, event) = node
        .checkpoint_surface_state(&session_id, input.state, &input.reason)
        .map_err(babble_error)?;
    encode(SurfaceStateCheckpointResponse {
        session,
        checkpoint,
        event,
    })
}

fn get_surface_state_checkpoint<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: SurfaceSessionRequest = payload(request)?;
    if let Some(bound_session) = request.binding.surface_session_id.as_ref()
        && bound_session != input.session_id.as_str()
    {
        return Err(RpcError::new(
            RpcErrorCode::CapabilityDenied,
            "Object-bound runtime calls may only inspect their bound Surface session state",
        ));
    }
    let node = lock_node(state).map_err(api_error)?;
    encode(SurfaceStateRestoreResponse {
        checkpoint: node
            .surface_state_checkpoint(&input.session_id)
            .map_err(babble_error)?,
    })
}

fn define_realtime_room<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: DefineRealtimeRoomRequest = payload(request)?;
    let author_id = identity_id(input.author_id).map_err(api_error)?;
    let object_id = object_id(input.object_id).map_err(api_error)?;
    let spec = RoomSpec::new(
        object_id,
        input.name,
        input.schema,
        input.membership,
        input.persistence,
        input.limits.unwrap_or_default(),
    )
    .map_err(babble_error)?;
    let room = spec.clone();
    let mut node = lock_node(state).map_err(api_error)?;
    encode(DefineRealtimeRoomResponse {
        event: node
            .define_realtime_room(&author_id, spec)
            .map_err(babble_error)?,
        room,
    })
}

fn start_realtime_session<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: StartRealtimeSessionRequest = payload(request)?;
    let author_id = identity_id(input.author_id).map_err(api_error)?;
    let room_id = realtime_room_id(input.room_id).map_err(api_error)?;
    let mut node = lock_node(state).map_err(api_error)?;
    let object_id = bound_object_id(request)?;
    let room = node
        .realtime_room(&room_id)
        .ok_or_else(|| RpcError::new(RpcErrorCode::NotFound, "realtime room not found"))?;
    if room.spec.object_id != object_id {
        return Err(RpcError::new(
            RpcErrorCode::CapabilityDenied,
            "realtime join RPC must target a room owned by the bound Object",
        ));
    }
    let (session, receipt) = node
        .start_realtime_session_with_capability(
            &author_id,
            &room_id,
            &request.binding.capability_grants,
        )
        .map_err(babble_error)?;
    encode(StartRealtimeSessionResponse {
        session,
        receipt: Some(receipt),
    })
}

fn close_realtime_session<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: CloseRealtimeSessionRequest = payload(request)?;
    let author_id = identity_id(input.author_id).map_err(api_error)?;
    let session_id = realtime_session_id(input.session_id).map_err(api_error)?;
    let input_object_id = object_id(input.object_id).map_err(api_error)?;
    let object_id = bound_object_id(request)?;
    if input_object_id != object_id {
        return Err(RpcError::new(
            RpcErrorCode::CapabilityDenied,
            "realtime leave RPC must target the bound Object",
        ));
    }
    let mut node = lock_node(state).map_err(api_error)?;
    let (session, event, receipt) = node
        .close_realtime_session_with_capability(
            &author_id,
            &session_id,
            &object_id,
            &request.binding.capability_grants,
        )
        .map_err(babble_error)?;
    encode(CloseRealtimeSessionResponse {
        session,
        event,
        receipt: Some(receipt),
    })
}

fn publish_realtime_message<P>(
    state: &ApiState<P>,
    request: &RpcRequestEnvelope,
) -> Result<Value, RpcError>
where
    P: JudgmentProvider,
{
    let input: PublishRealtimeMessageRequest = payload(request)?;
    let author_id = identity_id(input.author_id).map_err(api_error)?;
    let session_id = realtime_session_id(input.session_id).map_err(api_error)?;
    let input_object_id = object_id(input.object_id).map_err(api_error)?;
    let object_id = bound_object_id(request)?;
    if input_object_id != object_id {
        return Err(RpcError::new(
            RpcErrorCode::CapabilityDenied,
            "realtime send RPC must target the bound Object",
        ));
    }
    let mut node = lock_node(state).map_err(api_error)?;
    let (message, snapshot, receipt) = node
        .publish_realtime_message_with_capability(
            &author_id,
            &session_id,
            &object_id,
            input.payload,
            input.durable,
            &request.binding.capability_grants,
        )
        .map_err(babble_error)?;
    encode(PublishRealtimeMessageResponse {
        message,
        snapshot,
        receipt: Some(receipt),
    })
}

fn object_id_request(request: &RpcRequestEnvelope) -> Result<ObjectIdRequest, RpcError> {
    if let Some(value) = request.payload.as_str() {
        return Ok(ObjectIdRequest {
            object_id: value.to_string(),
        });
    }
    payload(request)
}

fn bound_object_id(request: &RpcRequestEnvelope) -> Result<babble_types::ObjectId, RpcError> {
    let value = request.binding.object_id.clone().ok_or_else(|| {
        RpcError::new(
            RpcErrorCode::CapabilityDenied,
            "capability-bound RPC requires an Object binding",
        )
    })?;
    object_id(value).map_err(api_error)
}

fn bound_identity_id(request: &RpcRequestEnvelope) -> Result<babble_types::IdentityId, RpcError> {
    let value = request.binding.identity_id.clone().ok_or_else(|| {
        RpcError::new(
            RpcErrorCode::CapabilityDenied,
            "storage.local requires a trusted host identity binding",
        )
    })?;
    identity_id(value).map_err(api_error)
}

fn ai_generate_task(task: &AiGenerateTask) -> &'static str {
    match task {
        AiGenerateTask::Text => "text",
        AiGenerateTask::Image => "image",
        AiGenerateTask::Audio => "audio",
        AiGenerateTask::Code => "code",
        AiGenerateTask::Json => "json",
    }
}

fn ai_modality(modality: &AiModality) -> &'static str {
    match modality {
        AiModality::Text => "text",
        AiModality::Image => "image",
        AiModality::Audio => "audio",
        AiModality::Json => "json",
    }
}

fn ai_embed_modality(modality: &AiEmbedInputModality) -> &'static str {
    match modality {
        AiEmbedInputModality::Text => "text",
        AiEmbedInputModality::Image => "image",
        AiEmbedInputModality::Audio => "audio",
    }
}

fn camera_mode(mode: &CameraCaptureMode) -> &'static str {
    match mode {
        CameraCaptureMode::Photo => "photo",
        CameraCaptureMode::Video => "video",
        CameraCaptureMode::Stream => "stream",
    }
}

fn camera_facing_mode(mode: &CameraFacingMode) -> &'static str {
    match mode {
        CameraFacingMode::Any => "any",
        CameraFacingMode::User => "user",
        CameraFacingMode::Environment => "environment",
    }
}

fn microphone_mode(mode: &MicrophoneCaptureMode) -> &'static str {
    match mode {
        MicrophoneCaptureMode::AudioClip => "audio_clip",
        MicrophoneCaptureMode::Stream => "stream",
    }
}

fn local_storage_entry(record: LocalStorageRecord) -> LocalStorageEntry {
    LocalStorageEntry {
        key: record.key,
        value: record.value,
        updated_at: record.updated_at,
        size_bytes: record.size_bytes,
    }
}

fn object_storage_entry(record: ObjectStorageRecord) -> ObjectStorageEntry {
    ObjectStorageEntry {
        key: record.key,
        value: record.value,
        updated_at: record.updated_at,
        size_bytes: record.size_bytes,
    }
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

fn checkpoint_response(event: Event) -> Result<CheckpointEventResponse, RpcError> {
    if event.kind != EventKind::ConsensusCheckpoint {
        return Err(RpcError::new(
            RpcErrorCode::InvalidInput,
            format!("event is not a consensus checkpoint: {}", event.id),
        ));
    }
    let checkpoint: FinalityCheckpoint =
        serde_json::from_value(event.payload.clone()).map_err(|err| {
            RpcError::new(
                RpcErrorCode::InvalidInput,
                format!("invalid checkpoint payload: {err}"),
            )
        })?;
    Ok(CheckpointEventResponse { event, checkpoint })
}

fn payload<T: DeserializeOwned>(request: &RpcRequestEnvelope) -> Result<T, RpcError> {
    serde_json::from_value(request.payload.clone()).map_err(|err| {
        RpcError::new(
            RpcErrorCode::InvalidInput,
            format!("invalid RPC payload for {}: {err}", request.method.as_str()),
        )
    })
}

fn encode(value: impl Serialize) -> Result<Value, RpcError> {
    serde_json::to_value(value).map_err(|err| {
        RpcError::new(
            RpcErrorCode::Internal,
            format!("failed to encode RPC response: {err}"),
        )
    })
}

fn catalog_error(error: RpcCatalogError) -> RpcError {
    let code = match error {
        RpcCatalogError::MissingIdempotencyKey(_)
        | RpcCatalogError::InvalidBinding
        | RpcCatalogError::InvalidDeadline
        | RpcCatalogError::InvalidMethodName(_)
        | RpcCatalogError::InvalidRequestId(_)
        | RpcCatalogError::InvalidResponse => RpcErrorCode::InvalidInput,
        RpcCatalogError::UnsupportedProtocol(_)
        | RpcCatalogError::UnknownMethod(_)
        | RpcCatalogError::InvalidVersion(_)
        | RpcCatalogError::DuplicateMethod(_) => RpcErrorCode::UnsupportedVersion,
    };
    RpcError::new(code, error.to_string())
}

fn api_error(error: ApiError) -> RpcError {
    let code = match error.code() {
        "bad_request" => RpcErrorCode::InvalidInput,
        "not_found" => RpcErrorCode::NotFound,
        "conflict" => RpcErrorCode::Conflict,
        "provider_unavailable" => RpcErrorCode::ProviderUnavailable,
        "storage_unavailable" => RpcErrorCode::StorageUnavailable,
        "payload_too_large" => RpcErrorCode::QuotaExceeded,
        _ => RpcErrorCode::Internal,
    };
    RpcError::new(code, error.message().to_string())
}

fn babble_error(error: babble_types::Error) -> RpcError {
    let code = match error {
        babble_types::Error::InvalidPrefix { .. }
        | babble_types::Error::InvalidHashLength { .. }
        | babble_types::Error::Canonical(_)
        | babble_types::Error::Signature
        | babble_types::Error::UnsignedObject
        | babble_types::Error::UnsignedEdge
        | babble_types::Error::UnsignedEvent => RpcErrorCode::InvalidInput,
        babble_types::Error::NotFound(_) => RpcErrorCode::NotFound,
        babble_types::Error::Conflict(_) => RpcErrorCode::Conflict,
        babble_types::Error::ProviderUnavailable(_) => RpcErrorCode::ProviderUnavailable,
        babble_types::Error::StorageUnavailable(_) => RpcErrorCode::StorageUnavailable,
    };
    RpcError::new(code, error.to_string())
}

fn capability_error(error: babble_types::Error) -> RpcError {
    match error {
        babble_types::Error::NotFound(_) => babble_error(error),
        _ => RpcError::new(RpcErrorCode::CapabilityDenied, error.to_string()),
    }
}
