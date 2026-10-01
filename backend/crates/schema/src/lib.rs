use schemars::{JsonSchema, Schema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProtocolSchemaBundle {
    pub protocol: String,
    pub version: u32,
    pub generated_by: String,
    pub schemas: BTreeMap<String, Value>,
    pub fixtures: BTreeMap<String, Value>,
}

pub fn protocol_schema_bundle() -> serde_json::Result<ProtocolSchemaBundle> {
    let schemas = protocol_schemas()?;
    let fixtures = protocol_fixtures()?;
    Ok(ProtocolSchemaBundle {
        protocol: babel_types::PROTOCOL_VERSION.to_string(),
        version: 1,
        generated_by: "babel-schema".to_string(),
        schemas,
        fixtures,
    })
}

pub fn protocol_schema_bundle_json() -> serde_json::Result<String> {
    pretty_json(&protocol_schema_bundle()?)
}

pub fn protocol_fixtures_json() -> serde_json::Result<String> {
    pretty_json(&protocol_fixtures()?)
}

pub fn protocol_schemas() -> serde_json::Result<BTreeMap<String, Value>> {
    let mut schemas = BTreeMap::new();
    insert_schema::<babel_graph::moderation::ModerationCase>(&mut schemas, "moderation.ModerationCase")?;
    insert_schema::<babel_graph::moderation::ModerationDecision>(&mut schemas, "moderation.ModerationDecision")?;
    insert_schema::<babel_graph::moderation::ModerationAppeal>(&mut schemas, "moderation.ModerationAppeal")?;
    insert_schema::<babel_graph::moderation::ModerationReason>(&mut schemas, "moderation.ModerationReason")?;
    insert_schema::<babel_graph::moderation::ModerationOutcome>(&mut schemas, "moderation.ModerationOutcome")?;
    insert_schema::<babel_graph::moderation::ModerationStatus>(&mut schemas, "moderation.ModerationStatus")?;
    insert_schema::<babel_graph::moderation::ModerationScope>(&mut schemas, "moderation.ModerationScope")?;
    insert_schema::<babel_graph::moderation::ModerationAccess>(&mut schemas, "moderation.ModerationAccess")?;
    insert_schema::<babel_graph::moderation::ModerationPage>(&mut schemas, "moderation.ModerationPage")?;
    insert_schema::<babel_api::ReportRequest>(&mut schemas, "api.ReportRequest")?;
    insert_schema::<babel_api::DecisionRequest>(&mut schemas, "api.DecisionRequest")?;
    insert_schema::<babel_api::AppealRequest>(&mut schemas, "api.AppealRequest")?;
    insert_schema::<babel_types::Hash>(&mut schemas, "types.Hash")?;
    insert_schema::<babel_types::ObjectId>(&mut schemas, "types.ObjectId")?;
    insert_schema::<babel_types::IdentityId>(&mut schemas, "types.IdentityId")?;
    insert_schema::<babel_types::EventId>(&mut schemas, "types.EventId")?;
    insert_schema::<babel_types::Timestamp>(&mut schemas, "types.Timestamp")?;
    insert_schema::<babel_types::Protocol>(&mut schemas, "types.Protocol")?;
    insert_schema::<babel_crypto::PublicKey>(&mut schemas, "crypto.PublicKey")?;
    insert_schema::<babel_crypto::Signature>(&mut schemas, "crypto.Signature")?;
    insert_schema::<babel_identity::Identity>(&mut schemas, "identity.Identity")?;
    insert_schema::<babel_identity::IdentityKeyTransition>(
        &mut schemas,
        "identity.IdentityKeyTransition",
    )?;
    insert_schema::<babel_object::Object>(&mut schemas, "object.Object")?;
    insert_schema::<babel_object::bundle::BundleManifest>(&mut schemas, "object.BundleManifest")?;
    insert_schema::<babel_object::SchemaRegistry>(&mut schemas, "object.SchemaRegistry")?;
    insert_schema::<babel_graph::Edge>(&mut schemas, "graph.Edge")?;
    insert_schema::<babel_graph::ReactionValue>(&mut schemas, "graph.ReactionValue")?;
    insert_schema::<babel_graph::ReactionState>(&mut schemas, "graph.ReactionState")?;
    insert_schema::<babel_graph::ReactionSummary>(&mut schemas, "graph.ReactionSummary")?;
    insert_schema::<babel_graph::ReactionRecord>(&mut schemas, "graph.ReactionRecord")?;
    insert_schema::<babel_graph::ReactionAction>(&mut schemas, "graph.ReactionAction")?;
    insert_schema::<babel_api::SetReactionRequest>(&mut schemas, "api.SetReactionRequest")?;
    insert_schema::<babel_api::SetReactionRpcRequest>(&mut schemas, "api.SetReactionRpcRequest")?;
    insert_schema::<babel_api::ReactionObjectRequest>(&mut schemas, "api.ReactionObjectRequest")?;
    insert_schema::<babel_api::ReactionRecordRequest>(&mut schemas, "api.ReactionRecordRequest")?;
    insert_schema::<babel_api::AccountSessionInfo>(&mut schemas, "api.AccountSessionInfo")?;
    insert_schema::<babel_api::AccountSessionsResponse>(
        &mut schemas,
        "api.AccountSessionsResponse",
    )?;
    insert_schema::<babel_api::ChangePasswordRequest>(&mut schemas, "api.ChangePasswordRequest")?;
    insert_schema::<babel_graph::GraphTraversal>(&mut schemas, "graph.GraphTraversal")?;
    insert_schema::<babel_graph::GraphTraversalSpec>(&mut schemas, "graph.GraphTraversalSpec")?;
    insert_schema::<babel_state::Event>(&mut schemas, "state.Event")?;
    insert_schema::<babel_media::MediaBlob>(&mut schemas, "media.MediaBlob")?;
    insert_schema::<babel_media::MediaObjectPayload>(&mut schemas, "media.MediaObjectPayload")?;
    insert_schema::<babel_authoring::ObjectDraft>(&mut schemas, "authoring.ObjectDraft")?;
    insert_schema::<babel_authoring::EdgeDraft>(&mut schemas, "authoring.EdgeDraft")?;
    insert_schema::<babel_capabilities::CapabilityManifest>(
        &mut schemas,
        "capabilities.CapabilityManifest",
    )?;
    insert_schema::<babel_capabilities::CapabilityDefinition>(
        &mut schemas,
        "capabilities.CapabilityDefinition",
    )?;
    insert_schema::<babel_capabilities::CapabilityGrant>(
        &mut schemas,
        "capabilities.CapabilityGrant",
    )?;
    insert_schema::<babel_capabilities::CapabilityCall>(
        &mut schemas,
        "capabilities.CapabilityCall",
    )?;
    insert_schema::<babel_capabilities::CapabilityReceipt>(
        &mut schemas,
        "capabilities.CapabilityReceipt",
    )?;
    insert_schema::<babel_capabilities::CapabilityUsageWindow>(
        &mut schemas,
        "capabilities.CapabilityUsageWindow",
    )?;
    insert_schema::<babel_judgment::Judgment>(&mut schemas, "judgment.Judgment")?;
    insert_schema::<babel_judgment::SourceAgreementInput>(
        &mut schemas,
        "judgment.SourceAgreementInput",
    )?;
    insert_schema::<babel_judgment::SourceAgreementOutput>(
        &mut schemas,
        "judgment.SourceAgreementOutput",
    )?;
    insert_schema::<babel_judgment::JudgmentDefinition>(
        &mut schemas,
        "judgment.JudgmentDefinition",
    )?;
    insert_schema::<babel_judgment::JudgmentRegistry>(&mut schemas, "judgment.JudgmentRegistry")?;
    insert_schema::<babel_judgment::JudgmentPrivacyPolicy>(
        &mut schemas,
        "judgment.JudgmentPrivacyPolicy",
    )?;
    insert_schema::<babel_judgment::JudgmentProviderDescriptor>(
        &mut schemas,
        "judgment.JudgmentProviderDescriptor",
    )?;
    insert_schema::<babel_judgment::ProviderDecision>(&mut schemas, "judgment.ProviderDecision")?;
    insert_schema::<babel_judgment::OrchestratedJudgment>(
        &mut schemas,
        "judgment.OrchestratedJudgment",
    )?;
    insert_schema::<babel_judgment::JudgmentBatchResult>(
        &mut schemas,
        "judgment.JudgmentBatchResult",
    )?;
    insert_schema::<babel_lens::LensStack>(&mut schemas, "lens.LensStack")?;
    insert_schema::<babel_lens::LensExecution>(&mut schemas, "lens.LensExecution")?;
    insert_schema::<babel_lens::LensDefinition>(&mut schemas, "lens.LensDefinition")?;
    insert_schema::<babel_lens::Candidate>(&mut schemas, "lens.Candidate")?;
    insert_schema::<babel_lens::RankingRequest>(&mut schemas, "lens.RankingRequest")?;
    insert_schema::<babel_discovery::TemporalRequest>(&mut schemas, "discovery.TemporalRequest")?;
    insert_schema::<babel_discovery::TemporalResult>(&mut schemas, "discovery.TemporalResult")?;
    insert_schema::<babel_discovery::TemporalProviderVersion>(
        &mut schemas,
        "discovery.TemporalProviderVersion",
    )?;
    insert_schema::<babel_lens::RankingResult>(&mut schemas, "lens.RankingResult")?;
    insert_schema::<babel_lens::RankingProviderVersion>(
        &mut schemas,
        "lens.RankingProviderVersion",
    )?;
    insert_schema::<babel_personalization::LocalUserModel>(
        &mut schemas,
        "personalization.LocalUserModel",
    )?;
    insert_schema::<babel_personalization::PersonalizationObjectSummary>(
        &mut schemas,
        "personalization.PersonalizationObjectSummary",
    )?;
    insert_schema::<babel_personalization::PersonalizationTrace>(
        &mut schemas,
        "personalization.PersonalizationTrace",
    )?;
    insert_schema::<babel_personalization::PersonalizationSyncRecipient>(
        &mut schemas,
        "personalization.PersonalizationSyncRecipient",
    )?;
    insert_schema::<babel_personalization::EncryptedLocalUserModel>(
        &mut schemas,
        "personalization.EncryptedLocalUserModel",
    )?;
    insert_schema::<babel_hashgraph::FinalityCheckpoint>(
        &mut schemas,
        "hashgraph.FinalityCheckpoint",
    )?;
    insert_schema::<babel_realtime::RoomSpec>(&mut schemas, "realtime.RoomSpec")?;
    insert_schema::<babel_realtime::RealtimeSchemaRegistry>(
        &mut schemas,
        "realtime.RealtimeSchemaRegistry",
    )?;
    insert_schema::<babel_realtime::RealtimeMessage>(&mut schemas, "realtime.RealtimeMessage")?;
    insert_schema::<babel_realtime::RealtimeSnapshot>(&mut schemas, "realtime.RealtimeSnapshot")?;
    insert_schema::<babel_rpc::RpcCatalog>(&mut schemas, "rpc.RpcCatalog")?;
    insert_schema::<babel_rpc::RpcMethodDefinition>(&mut schemas, "rpc.RpcMethodDefinition")?;
    insert_schema::<babel_rpc::RpcRequestEnvelope>(&mut schemas, "rpc.RpcRequestEnvelope")?;
    insert_schema::<babel_rpc::RpcResponseEnvelope>(&mut schemas, "rpc.RpcResponseEnvelope")?;
    insert_schema::<babel_rpc::RpcError>(&mut schemas, "rpc.RpcError")?;
    insert_schema::<babel_runtime::SurfaceSessionPlan>(&mut schemas, "runtime.SurfaceSessionPlan")?;
    insert_schema::<babel_runtime::SurfaceSession>(&mut schemas, "runtime.SurfaceSession")?;
    insert_schema::<babel_runtime::SurfaceRuntimeEvent>(
        &mut schemas,
        "runtime.SurfaceRuntimeEvent",
    )?;
    insert_schema::<babel_runtime::SurfaceRuntimeHealthSnapshot>(
        &mut schemas,
        "runtime.SurfaceRuntimeHealthSnapshot",
    )?;
    insert_schema::<babel_node::ObservabilitySnapshot>(&mut schemas, "node.ObservabilitySnapshot")?;
    insert_schema::<babel_runtime::SurfaceStateCheckpoint>(
        &mut schemas,
        "runtime.SurfaceStateCheckpoint",
    )?;
    insert_schema::<babel_runtime::SurfaceSchedulingInput>(
        &mut schemas,
        "runtime.SurfaceSchedulingInput",
    )?;
    insert_schema::<babel_runtime::SurfaceScheduleDecision>(
        &mut schemas,
        "runtime.SurfaceScheduleDecision",
    )?;
    insert_schema::<babel_runtime::SurfaceSessionId>(&mut schemas, "runtime.SurfaceSessionId")?;
    insert_schema::<babel_network::Envelope>(&mut schemas, "network.Envelope")?;
    insert_schema::<babel_node::ImportBundle>(&mut schemas, "node.ImportBundle")?;
    insert_schema::<babel_api::CreateIdentityRequest>(&mut schemas, "api.CreateIdentityRequest")?;
    insert_schema::<babel_api::CreateIdentityResponse>(&mut schemas, "api.CreateIdentityResponse")?;
    insert_schema::<babel_api::EmptyRequest>(&mut schemas, "api.EmptyRequest")?;
    insert_schema::<babel_api::IdentityCurrentResponse>(
        &mut schemas,
        "api.IdentityCurrentResponse",
    )?;
    insert_schema::<babel_api::RotateIdentityKeyRequest>(
        &mut schemas,
        "api.RotateIdentityKeyRequest",
    )?;
    insert_schema::<babel_api::RotateIdentityKeyResponse>(
        &mut schemas,
        "api.RotateIdentityKeyResponse",
    )?;
    insert_schema::<babel_api::PublishTextRequest>(&mut schemas, "api.PublishTextRequest")?;
    insert_schema::<babel_api::PublishTextResponse>(&mut schemas, "api.PublishTextResponse")?;
    insert_schema::<babel_api::PublishObjectRequest>(&mut schemas, "api.PublishObjectRequest")?;
    insert_schema::<babel_api::PublishObjectResponse>(&mut schemas, "api.PublishObjectResponse")?;
    insert_schema::<babel_api::ForkObjectRequest>(&mut schemas, "api.ForkObjectRequest")?;
    insert_schema::<babel_api::RemixObjectRequest>(&mut schemas, "api.RemixObjectRequest")?;
    insert_schema::<babel_api::ProvenancePublicationResponse>(
        &mut schemas,
        "api.ProvenancePublicationResponse",
    )?;
    insert_schema::<babel_api::PublishMediaObjectRequest>(
        &mut schemas,
        "api.PublishMediaObjectRequest",
    )?;
    insert_schema::<babel_api::PublishMediaObjectResponse>(
        &mut schemas,
        "api.PublishMediaObjectResponse",
    )?;
    insert_schema::<babel_api::ObjectIdRequest>(&mut schemas, "api.ObjectIdRequest")?;
    insert_schema::<babel_api::PutMediaBlobRequest>(&mut schemas, "api.PutMediaBlobRequest")?;
    insert_schema::<babel_api::GetMediaBlobRequest>(&mut schemas, "api.GetMediaBlobRequest")?;
    insert_schema::<babel_api::MediaBlobResponse>(&mut schemas, "api.MediaBlobResponse")?;
    insert_schema::<babel_api::PublishEdgeRequest>(&mut schemas, "api.PublishEdgeRequest")?;
    insert_schema::<babel_api::PublishEdgeResponse>(&mut schemas, "api.PublishEdgeResponse")?;
    insert_schema::<babel_api::RelationshipInferenceKind>(
        &mut schemas,
        "api.RelationshipInferenceKind",
    )?;
    insert_schema::<babel_api::InferRelationshipRequest>(
        &mut schemas,
        "api.InferRelationshipRequest",
    )?;
    insert_schema::<babel_api::InferRelationshipResponse>(
        &mut schemas,
        "api.InferRelationshipResponse",
    )?;
    insert_schema::<babel_node::EvidenceRelationKind>(&mut schemas, "node.EvidenceRelationKind")?;
    insert_schema::<babel_node::EvidenceProjectionItem>(
        &mut schemas,
        "node.EvidenceProjectionItem",
    )?;
    insert_schema::<babel_node::EvidenceProjectionSummary>(
        &mut schemas,
        "node.EvidenceProjectionSummary",
    )?;
    insert_schema::<babel_node::ClaimEvidenceProjection>(
        &mut schemas,
        "node.ClaimEvidenceProjection",
    )?;
    insert_schema::<babel_api::ClaimEvidenceResponse>(&mut schemas, "api.ClaimEvidenceResponse")?;
    insert_schema::<babel_api::GraphTraverseRequest>(&mut schemas, "api.GraphTraverseRequest")?;
    insert_schema::<babel_api::GraphTraverseRpcRequest>(
        &mut schemas,
        "api.GraphTraverseRpcRequest",
    )?;
    insert_schema::<babel_api::GraphTraversalResponse>(&mut schemas, "api.GraphTraversalResponse")?;
    insert_schema::<babel_api::SocialTargetRequest>(&mut schemas, "api.SocialTargetRequest")?;
    insert_schema::<babel_api::SocialEdgeResponse>(&mut schemas, "api.SocialEdgeResponse")?;
    insert_schema::<babel_api::SocialTextRequest>(&mut schemas, "api.SocialTextRequest")?;
    insert_schema::<babel_api::SocialMediaAttachment>(&mut schemas, "api.SocialMediaAttachment")?;
    insert_schema::<babel_api::SocialTextResponse>(&mut schemas, "api.SocialTextResponse")?;
    insert_schema::<babel_api::PrepareInvocationRequest>(
        &mut schemas,
        "api.PrepareInvocationRequest",
    )?;
    insert_schema::<babel_api::DecideInvocationRequest>(
        &mut schemas,
        "api.DecideInvocationRequest",
    )?;
    insert_schema::<babel_api::RecoverInvocationRequest>(
        &mut schemas,
        "api.RecoverInvocationRequest",
    )?;
    insert_schema::<babel_api::RegisterHostDocumentRequest>(
        &mut schemas,
        "api.RegisterHostDocumentRequest",
    )?;
    insert_schema::<babel_api::HostDocumentResponse>(&mut schemas, "api.HostDocumentResponse")?;
    insert_schema::<babel_api::InvocationResponse>(&mut schemas, "api.InvocationResponse")?;
    insert_schema::<babel_api::BrowserInvocationResponse>(&mut schemas, "api.BrowserInvocationResponse")?;
    insert_schema::<babel_api::BrowserInvocationResult>(&mut schemas, "api.BrowserInvocationResult")?;
    insert_schema::<babel_api::AcknowledgeBrowserInvocationRequest>(&mut schemas, "api.AcknowledgeBrowserInvocationRequest")?;
    insert_schema::<babel_api::BrowserExecutionTicket>(&mut schemas, "api.BrowserExecutionTicket")?;
    insert_schema::<babel_api::InvocationSocialResult>(&mut schemas, "api.InvocationSocialResult")?;
    insert_schema::<babel_api::RepliesListRequest>(&mut schemas, "api.RepliesListRequest")?;
    insert_schema::<babel_node::AuthorObjectsQuery>(&mut schemas, "node.AuthorObjectsQuery")?;
    insert_schema::<babel_node::AuthorObjectsPage>(&mut schemas, "node.AuthorObjectsPage")?;
    insert_schema::<babel_node::FollowState>(&mut schemas, "node.FollowState")?;
    insert_schema::<babel_node::SafetyState>(&mut schemas, "node.SafetyState")?;
    insert_schema::<babel_node::SafetySnapshot>(&mut schemas, "node.SafetySnapshot")?;
    insert_schema::<babel_node::SafetyEntry>(&mut schemas, "node.SafetyEntry")?;
    insert_schema::<babel_api::SetSafetyRequest>(&mut schemas, "api.SetSafetyRequest")?;
    insert_schema::<babel_graph::SafetyAction>(&mut schemas, "graph.SafetyAction")?;
    insert_schema::<babel_graph::SafetyReceipt>(&mut schemas, "graph.SafetyReceipt")?;
    insert_schema::<babel_node::FollowingQuery>(&mut schemas, "node.FollowingQuery")?;
    insert_schema::<babel_node::FollowingPage>(&mut schemas, "node.FollowingPage")?;
    insert_schema::<babel_node::FollowListPage>(&mut schemas, "node.FollowListPage")?;
    insert_schema::<babel_api::SetFollowingRequest>(&mut schemas, "api.SetFollowingRequest")?;
    insert_schema::<babel_api::RepliesListResponse>(&mut schemas, "api.RepliesListResponse")?;
    insert_schema::<babel_api::QuotesListRequest>(&mut schemas, "api.QuotesListRequest")?;
    insert_schema::<babel_api::QuotesListResponse>(&mut schemas, "api.QuotesListResponse")?;
    insert_schema::<babel_api::EventListRequest>(&mut schemas, "api.EventListRequest")?;
    insert_schema::<babel_api::EventListResponse>(&mut schemas, "api.EventListResponse")?;
    insert_schema::<babel_api::EventBundleRequest>(&mut schemas, "api.EventBundleRequest")?;
    insert_schema::<babel_api::EventBundleResponse>(&mut schemas, "api.EventBundleResponse")?;
    insert_schema::<babel_api::EventImportRequest>(&mut schemas, "api.EventImportRequest")?;
    insert_schema::<babel_api::EventImportResponse>(&mut schemas, "api.EventImportResponse")?;
    insert_schema::<babel_api::CheckpointPreviewRequest>(
        &mut schemas,
        "api.CheckpointPreviewRequest",
    )?;
    insert_schema::<babel_api::CheckpointPreviewResponse>(
        &mut schemas,
        "api.CheckpointPreviewResponse",
    )?;
    insert_schema::<babel_api::CheckpointRequest>(&mut schemas, "api.CheckpointRequest")?;
    insert_schema::<babel_api::CheckpointEventResponse>(
        &mut schemas,
        "api.CheckpointEventResponse",
    )?;
    insert_schema::<babel_api::ObjectSearchRequest>(&mut schemas, "api.ObjectSearchRequest")?;
    insert_schema::<babel_api::ObjectSearchResponse>(&mut schemas, "api.ObjectSearchResponse")?;
    insert_schema::<babel_api::JudgeObjectRpcRequest>(&mut schemas, "api.JudgeObjectRpcRequest")?;
    insert_schema::<babel_api::JudgeObjectResponse>(&mut schemas, "api.JudgeObjectResponse")?;
    insert_schema::<babel_api::ObjectJudgmentsResponse>(
        &mut schemas,
        "api.ObjectJudgmentsResponse",
    )?;
    insert_schema::<babel_api::JudgmentDefinitionsResponse>(
        &mut schemas,
        "api.JudgmentDefinitionsResponse",
    )?;
    insert_schema::<babel_api::JudgmentProvidersResponse>(
        &mut schemas,
        "api.JudgmentProvidersResponse",
    )?;
    insert_schema::<babel_api::DiscoveryRequest>(&mut schemas, "api.DiscoveryRequest")?;
    insert_schema::<babel_api::DiscoveryResponse>(&mut schemas, "api.DiscoveryResponse")?;
    insert_schema::<babel_api::LensCatalogResponse>(&mut schemas, "api.LensCatalogResponse")?;
    insert_schema::<babel_api::CapabilityCatalogResponse>(
        &mut schemas,
        "api.CapabilityCatalogResponse",
    )?;
    insert_schema::<babel_api::CapabilitiesResponse>(&mut schemas, "api.CapabilitiesResponse")?;
    insert_schema::<babel_api::GrantCapabilityRequest>(&mut schemas, "api.GrantCapabilityRequest")?;
    insert_schema::<babel_api::GrantCapabilityResponse>(
        &mut schemas,
        "api.GrantCapabilityResponse",
    )?;
    insert_schema::<babel_api::RevokeCapabilityRequest>(
        &mut schemas,
        "api.RevokeCapabilityRequest",
    )?;
    insert_schema::<babel_api::LocalStorageEntry>(&mut schemas, "api.LocalStorageEntry")?;
    insert_schema::<babel_api::LocalStorageGetRequest>(&mut schemas, "api.LocalStorageGetRequest")?;
    insert_schema::<babel_api::LocalStorageGetResponse>(
        &mut schemas,
        "api.LocalStorageGetResponse",
    )?;
    insert_schema::<babel_api::LocalStorageSetRequest>(&mut schemas, "api.LocalStorageSetRequest")?;
    insert_schema::<babel_api::LocalStorageSetResponse>(
        &mut schemas,
        "api.LocalStorageSetResponse",
    )?;
    insert_schema::<babel_api::LocalStorageDeleteRequest>(
        &mut schemas,
        "api.LocalStorageDeleteRequest",
    )?;
    insert_schema::<babel_api::LocalStorageDeleteResponse>(
        &mut schemas,
        "api.LocalStorageDeleteResponse",
    )?;
    insert_schema::<babel_api::LocalStorageListRequest>(
        &mut schemas,
        "api.LocalStorageListRequest",
    )?;
    insert_schema::<babel_api::LocalStorageListResponse>(
        &mut schemas,
        "api.LocalStorageListResponse",
    )?;
    insert_schema::<babel_api::PersonalizationSyncEnvelopeSummary>(
        &mut schemas,
        "api.PersonalizationSyncEnvelopeSummary",
    )?;
    insert_schema::<babel_api::PersonalizationSyncPutRequest>(
        &mut schemas,
        "api.PersonalizationSyncPutRequest",
    )?;
    insert_schema::<babel_api::PersonalizationSyncPutResponse>(
        &mut schemas,
        "api.PersonalizationSyncPutResponse",
    )?;
    insert_schema::<babel_api::PersonalizationSyncListRequest>(
        &mut schemas,
        "api.PersonalizationSyncListRequest",
    )?;
    insert_schema::<babel_api::PersonalizationSyncListResponse>(
        &mut schemas,
        "api.PersonalizationSyncListResponse",
    )?;
    insert_schema::<babel_api::PersonalizationSyncGetRequest>(
        &mut schemas,
        "api.PersonalizationSyncGetRequest",
    )?;
    insert_schema::<babel_api::PersonalizationSyncGetResponse>(
        &mut schemas,
        "api.PersonalizationSyncGetResponse",
    )?;
    insert_schema::<babel_api::PersonalizationSyncDeleteResponse>(
        &mut schemas,
        "api.PersonalizationSyncDeleteResponse",
    )?;
    insert_schema::<babel_api::ObjectStorageEntry>(&mut schemas, "api.ObjectStorageEntry")?;
    insert_schema::<babel_api::ObjectStorageGetRequest>(
        &mut schemas,
        "api.ObjectStorageGetRequest",
    )?;
    insert_schema::<babel_api::ObjectStorageGetResponse>(
        &mut schemas,
        "api.ObjectStorageGetResponse",
    )?;
    insert_schema::<babel_api::ObjectStorageSetRequest>(
        &mut schemas,
        "api.ObjectStorageSetRequest",
    )?;
    insert_schema::<babel_api::ObjectStorageSetResponse>(
        &mut schemas,
        "api.ObjectStorageSetResponse",
    )?;
    insert_schema::<babel_api::ObjectStorageDeleteRequest>(
        &mut schemas,
        "api.ObjectStorageDeleteRequest",
    )?;
    insert_schema::<babel_api::ObjectStorageDeleteResponse>(
        &mut schemas,
        "api.ObjectStorageDeleteResponse",
    )?;
    insert_schema::<babel_api::ObjectStorageListRequest>(
        &mut schemas,
        "api.ObjectStorageListRequest",
    )?;
    insert_schema::<babel_api::ObjectStorageListResponse>(
        &mut schemas,
        "api.ObjectStorageListResponse",
    )?;
    insert_schema::<babel_api::NetworkFetchRequest>(&mut schemas, "api.NetworkFetchRequest")?;
    insert_schema::<babel_api::NetworkFetchResponse>(&mut schemas, "api.NetworkFetchResponse")?;
    insert_schema::<babel_api::PrepareSurfaceRequest>(&mut schemas, "api.PrepareSurfaceRequest")?;
    insert_schema::<babel_api::PrepareSurfaceResponse>(&mut schemas, "api.PrepareSurfaceResponse")?;
    insert_schema::<babel_api::StartSurfaceSessionRequest>(
        &mut schemas,
        "api.StartSurfaceSessionRequest",
    )?;
    insert_schema::<babel_api::SurfaceSessionRequest>(&mut schemas, "api.SurfaceSessionRequest")?;
    insert_schema::<babel_api::SurfaceSessionResponse>(&mut schemas, "api.SurfaceSessionResponse")?;
    insert_schema::<babel_api::SurfaceLeaseResponse>(&mut schemas, "api.SurfaceLeaseResponse")?;
    insert_schema::<babel_api::SurfaceRuntimeHealthResponse>(
        &mut schemas,
        "api.SurfaceRuntimeHealthResponse",
    )?;
    insert_schema::<babel_api::ObservabilitySnapshotResponse>(
        &mut schemas,
        "api.ObservabilitySnapshotResponse",
    )?;
    insert_schema::<babel_api::TransitionSurfaceSessionRequest>(
        &mut schemas,
        "api.TransitionSurfaceSessionRequest",
    )?;
    insert_schema::<babel_api::ChangeSurfaceBudgetRequest>(
        &mut schemas,
        "api.ChangeSurfaceBudgetRequest",
    )?;
    insert_schema::<babel_api::ScheduleSurfaceSessionRequest>(
        &mut schemas,
        "api.ScheduleSurfaceSessionRequest",
    )?;
    insert_schema::<babel_api::ScheduleSurfaceSessionResponse>(
        &mut schemas,
        "api.ScheduleSurfaceSessionResponse",
    )?;
    insert_schema::<babel_api::ApplySurfaceScheduleResponse>(
        &mut schemas,
        "api.ApplySurfaceScheduleResponse",
    )?;
    insert_schema::<babel_api::SurfaceSessionEventResponse>(
        &mut schemas,
        "api.SurfaceSessionEventResponse",
    )?;
    insert_schema::<babel_api::CheckpointSurfaceStateRequest>(
        &mut schemas,
        "api.CheckpointSurfaceStateRequest",
    )?;
    insert_schema::<babel_api::SurfaceStateCheckpointResponse>(
        &mut schemas,
        "api.SurfaceStateCheckpointResponse",
    )?;
    insert_schema::<babel_api::SurfaceStateRestoreResponse>(
        &mut schemas,
        "api.SurfaceStateRestoreResponse",
    )?;
    insert_schema::<babel_api::DefineRealtimeRoomRequest>(
        &mut schemas,
        "api.DefineRealtimeRoomRequest",
    )?;
    insert_schema::<babel_api::DefineRealtimeRoomResponse>(
        &mut schemas,
        "api.DefineRealtimeRoomResponse",
    )?;
    insert_schema::<babel_api::StartRealtimeSessionRequest>(
        &mut schemas,
        "api.StartRealtimeSessionRequest",
    )?;
    insert_schema::<babel_api::StartRealtimeSessionResponse>(
        &mut schemas,
        "api.StartRealtimeSessionResponse",
    )?;
    insert_schema::<babel_api::CloseRealtimeSessionRequest>(
        &mut schemas,
        "api.CloseRealtimeSessionRequest",
    )?;
    insert_schema::<babel_api::CloseRealtimeSessionResponse>(
        &mut schemas,
        "api.CloseRealtimeSessionResponse",
    )?;
    insert_schema::<babel_api::PublishRealtimeMessageRequest>(
        &mut schemas,
        "api.PublishRealtimeMessageRequest",
    )?;
    insert_schema::<babel_api::PublishRealtimeMessageResponse>(
        &mut schemas,
        "api.PublishRealtimeMessageResponse",
    )?;
    insert_schema::<babel_api::RealtimeRoomResponse>(&mut schemas, "api.RealtimeRoomResponse")?;
    Ok(schemas)
}

mod bundles;
mod safety;
mod moderation;
mod temporal;

pub fn protocol_fixtures() -> serde_json::Result<BTreeMap<String, Value>> {
    let mut fixtures = BTreeMap::new();
    fixtures.insert("social_safety".into(), safety::fixtures().map_err(serde_error)?);
    fixtures.insert("moderation".into(), moderation::fixtures().map_err(serde_error)?);
    fixtures.insert("bundle_manifest".into(), bundles::fixture()?);
    fixtures.insert("temporal_scoring".into(), temporal::fixtures()?);
    let canonical_sample = serde_json::json!({
        "zeta": [1, "1", 1.0],
        "alpha": {"nested": true, "empty": null}
    });
    let canonical_hash = babel_types::Canonical::canonical_hash(&canonical_sample)
        .map_err(|err| serde_json::Error::io(std::io::Error::other(err.to_string())))?;
    let canonical_bytes = babel_types::Canonical::canonical_bytes(&canonical_sample)
        .map_err(|err| serde_json::Error::io(std::io::Error::other(err.to_string())))?;
    fixtures.insert(
        "canonical_encoding".to_string(),
        serde_json::json!({
            "version": babel_types::CANONICAL_ENCODING_VERSION,
            "sample": canonical_sample,
            "bytes_hex": hex::encode(canonical_bytes),
            "hash": canonical_hash
        }),
    );
    let blob =
        babel_media::MediaBlob::from_bytes("text/plain", b"babel fixture").map_err(serde_error)?;
    let draft = babel_authoring::ObjectDraft::media(
        "Fixture media",
        Some("A deterministic media draft fixture.".to_string()),
        vec![blob.clone()],
    )
    .map_err(serde_error)?;
    fixtures.insert("media_blob".to_string(), serde_json::to_value(&blob)?);
    fixtures.insert(
        "media_object_draft".to_string(),
        serde_json::to_value(&draft)?,
    );
    fixtures.insert(
        "api_publish_media_object_request".to_string(),
        serde_json::json!({
            "author_id": "id_0000000000000000000000000000000000000000000000000000000000000000",
            "title": "Fixture media",
            "description": "A deterministic media request fixture.",
            "resources": [blob]
        }),
    );
    fixtures.insert(
        "capability_request".to_string(),
        serde_json::json!({
            "id": "babel.realtime.join",
            "version": 1,
            "scope": {"room": "fixture"}
        }),
    );
    fixtures.insert(
        "schema_registry".to_string(),
        serde_json::to_value(babel_object::SchemaRegistry::babel_core())?,
    );
    fixtures.insert(
        "judgment_registry".to_string(),
        serde_json::to_value(babel_judgment::JudgmentRegistry::babel_core())?,
    );
    fixtures.insert(
        "realtime_schema_registry".to_string(),
        serde_json::to_value(babel_realtime::RealtimeSchemaRegistry::babel_core())?,
    );
    fixtures.insert(
        "rpc_catalog".to_string(),
        serde_json::to_value(
            babel_rpc::babel_rpc_catalog()
                .map_err(|err| serde_json::Error::io(std::io::Error::other(err.to_string())))?,
        )?,
    );
    let catalog = babel_rpc::babel_rpc_catalog()
        .map_err(|err| serde_json::Error::io(std::io::Error::other(err.to_string())))?;
    let request = babel_rpc::RpcRequestEnvelope::new(
        &catalog,
        "fixture-request-1",
        "babel.object.get.v1",
        babel_rpc::RpcBinding::host("fixture-runtime", "babel://fixture")
            .map_err(|err| serde_json::Error::io(std::io::Error::other(err.to_string())))?,
        serde_json::json!({"object_id": "obj_0000000000000000000000000000000000000000000000000000000000000000"}),
    )
    .map_err(|err| serde_json::Error::io(std::io::Error::other(err.to_string())))?
    .with_trace_id("trace-fixture-1");
    fixtures.insert("rpc_request".to_string(), serde_json::to_value(&request)?);
    fixtures.insert(
        "rpc_error_response".to_string(),
        serde_json::to_value(babel_rpc::RpcResponseEnvelope::err(
            &catalog,
            &request,
            babel_rpc::RpcError::new(babel_rpc::RpcErrorCode::NotFound, "object not found")
                .with_details(serde_json::json!({"object_id": "obj_0000000000000000000000000000000000000000000000000000000000000000"})),
        ))?,
    );
    Ok(fixtures)
}

fn insert_schema<T: JsonSchema>(
    schemas: &mut BTreeMap<String, Value>,
    name: &'static str,
) -> serde_json::Result<()> {
    let schema: Schema = schemars::schema_for!(T);
    schemas.insert(name.to_string(), serde_json::to_value(schema)?);
    Ok(())
}

fn serde_error(error: babel_types::Error) -> serde_json::Error {
    serde_json::Error::io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        error.to_string(),
    ))
}

fn pretty_json(value: &impl Serialize) -> serde_json::Result<String> {
    let mut output = serde_json::to_string_pretty(value)?;
    output.push('\n');
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn schema_bundle_matches_golden_fixture() {
        assert_golden(
            "schema-bundle.json",
            &protocol_schema_bundle_json().unwrap(),
        );
    }

    #[test]
    fn conformance_values_match_golden_fixture() {
        assert_golden("fixtures.json", &protocol_fixtures_json().unwrap());
    }

    #[test]
    fn schema_bundle_contains_core_contracts() {
        let bundle = protocol_schema_bundle().unwrap();

        assert_eq!(bundle.protocol, babel_types::PROTOCOL_VERSION);
        assert!(bundle.schemas.contains_key("object.Object"));
        assert!(bundle.schemas.contains_key("discovery.TemporalRequest"));
        assert!(bundle.schemas.contains_key("discovery.TemporalResult"));
        assert!(bundle.fixtures.contains_key("temporal_scoring"));
        assert!(bundle.schemas.contains_key("object.SchemaRegistry"));
        assert!(bundle.schemas.contains_key("graph.Edge"));
        assert!(bundle.schemas.contains_key("state.Event"));
        assert!(
            bundle
                .schemas
                .contains_key("identity.IdentityKeyTransition")
        );
        assert!(bundle.schemas.contains_key("api.RotateIdentityKeyResponse"));
        assert!(bundle.schemas.contains_key("api.IdentityCurrentResponse"));
        assert!(bundle.schemas.contains_key("api.PublishMediaObjectRequest"));
        assert!(
            bundle
                .schemas
                .contains_key("api.PublishMediaObjectResponse")
        );
        assert!(bundle.schemas.contains_key("api.GrantCapabilityResponse"));
        assert!(bundle.schemas.contains_key("api.LocalStorageSetResponse"));
        assert!(bundle.schemas.contains_key("api.ObjectStorageSetResponse"));
        assert!(bundle.schemas.contains_key("api.NetworkFetchResponse"));
        assert!(bundle.schemas.contains_key("judgment.JudgmentRegistry"));
        assert!(
            bundle
                .schemas
                .contains_key("judgment.JudgmentPrivacyPolicy")
        );
        assert!(bundle.schemas.contains_key("judgment.ProviderDecision"));
        assert!(bundle.schemas.contains_key("judgment.OrchestratedJudgment"));
        assert!(bundle.schemas.contains_key("judgment.JudgmentBatchResult"));
        assert!(
            bundle
                .schemas
                .contains_key("realtime.RealtimeSchemaRegistry")
        );
        assert!(
            bundle
                .schemas
                .contains_key("api.StartRealtimeSessionResponse")
        );
        assert!(
            bundle
                .schemas
                .contains_key("api.CloseRealtimeSessionResponse")
        );
        assert!(bundle.schemas.contains_key("realtime.RealtimeSnapshot"));
        assert!(bundle.schemas.contains_key("api.RealtimeRoomResponse"));
        assert!(bundle.schemas.contains_key("rpc.RpcCatalog"));
        assert!(bundle.schemas.contains_key("rpc.RpcRequestEnvelope"));
        assert!(bundle.schemas.contains_key("rpc.RpcError"));
        assert!(bundle.fixtures.contains_key("media_object_draft"));
        assert!(bundle.fixtures.contains_key("schema_registry"));
        assert!(bundle.fixtures.contains_key("judgment_registry"));
        assert!(bundle.fixtures.contains_key("realtime_schema_registry"));
        assert!(bundle.fixtures.contains_key("rpc_catalog"));
        assert!(bundle.fixtures.contains_key("rpc_request"));
    }

    #[test]
    fn fixtures_pin_public_judgment_and_rpc_surfaces() {
        let fixtures = protocol_fixtures().unwrap();
        let judgment_registry = fixtures
            .get("judgment_registry")
            .expect("judgment registry fixture must be exported");
        let definitions = judgment_registry["definitions"]
            .as_array()
            .expect("judgment definitions must be an array")
            .iter()
            .filter_map(|definition| definition["id"].as_str())
            .collect::<std::collections::BTreeSet<_>>();

        for required in [
            "babel.judgment.spam.v1",
            "babel.judgment.evidence_quality.v1",
            "babel.judgment.content_analysis.v1",
            "babel.judgment.moderation.v1",
        ] {
            assert!(
                definitions.contains(required),
                "missing exported Judgment definition {required}"
            );
        }

        let rpc_catalog = fixtures
            .get("rpc_catalog")
            .expect("RPC catalog fixture must be exported");
        let methods = rpc_catalog["methods"]
            .as_array()
            .expect("RPC catalog methods must be an array")
            .iter()
            .filter_map(|method| method["method"].as_str())
            .collect::<std::collections::BTreeSet<_>>();

        for required in [
            "babel.identity.current.v1",
            "babel.storage.local.set.v1",
            "babel.social.follow.v1",
            "babel.social.share.v1",
            "babel.social.reply.v1",
            "babel.social.replies.list.v1",
            "babel.social.quotes.list.v1",
            "babel.social.reactions.summary.v1",
            "babel.social.reactions.record.v1",
            "babel.social.reactions.mine.v1",
            "babel.social.reactions.set.v1",
            "babel.object.fork.v1",
            "babel.object.remix.v1",
            "babel.judgment.object.evaluate.v1",
            "babel.runtime.surface.prepare.v1",
            "babel.realtime.room.define.v1",
            "babel.realtime.session.start.v1",
            "babel.realtime.message.publish.v1",
        ] {
            assert!(
                methods.contains(required),
                "missing exported RPC method {required}"
            );
        }
    }

    #[test]
    fn reaction_schema_requires_each_axis_without_erasing_null() {
        fn allows_null(value: &Value) -> bool {
            value["type"] == "null"
                || value["type"]
                    .as_array()
                    .is_some_and(|types| types.iter().any(|kind| kind == "null"))
                || value["anyOf"]
                    .as_array()
                    .is_some_and(|options| options.iter().any(allows_null))
        }
        let schemas = protocol_schemas().unwrap();
        let reaction = &schemas["graph.ReactionValue"];
        let required = reaction["required"].as_array().unwrap();
        for axis in ["appreciation", "engagement", "stance", "certainty"] {
            assert!(
                required.iter().any(|name| name == axis),
                "{axis} must be explicit"
            );
            assert!(
                allows_null(&reaction["properties"][axis]),
                "{axis} must allow withdrawal"
            );
        }
        assert_eq!(reaction["additionalProperties"], false);
    }

    #[test]
    fn account_security_schema_has_no_credential_bearing_session_fields() {
        let schemas = protocol_schemas().unwrap();
        let session = &schemas["api.AccountSessionInfo"];
        let properties = session["properties"].as_object().unwrap();
        assert_eq!(properties.len(), 4);
        for field in ["id", "created_at", "expires_at", "current"] {
            assert!(properties.contains_key(field));
            assert!(
                session["required"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|name| name == field)
            );
        }
        assert_eq!(
            properties["created_at"]["type"],
            serde_json::json!(["string", "null"])
        );
        assert_eq!(session["additionalProperties"], false);
        let change = &schemas["api.ChangePasswordRequest"];
        assert_eq!(change["properties"].as_object().unwrap().len(), 2);
        assert_eq!(
            change["required"],
            serde_json::json!(["current_password", "new_password"])
        );
        assert_eq!(change["additionalProperties"], false);
    }

    #[test]
    fn replies_schema_preserves_required_nullable_cursors_and_limits() {
        let schemas = protocol_schemas().unwrap();
        let request = &schemas["api.RepliesListRequest"];
        assert_eq!(
            request["required"],
            serde_json::json!(["object_id", "cursor", "limit"])
        );
        assert_eq!(
            request["properties"]["cursor"]["type"],
            serde_json::json!(["string", "null"])
        );
        assert_eq!(request["properties"]["limit"]["minimum"], 1);
        assert_eq!(request["properties"]["limit"]["maximum"], 50);
        let response = &schemas["api.RepliesListResponse"];
        assert_eq!(
            response["required"],
            serde_json::json!(["object_id", "replies", "next_cursor"])
        );
        assert_eq!(
            response["properties"]["next_cursor"]["type"],
            serde_json::json!(["string", "null"])
        );
    }

    #[test]
    fn quotes_schema_preserves_required_nullable_fields_and_bounded_limit() {
        let schemas = protocol_schemas().unwrap();
        let request = &schemas["api.QuotesListRequest"];
        assert_eq!(
            request["required"],
            serde_json::json!(["object_id", "cursor", "limit"])
        );
        assert_eq!(request["additionalProperties"], false);
        assert_eq!(
            request["properties"]["cursor"]["type"],
            serde_json::json!(["string", "null"])
        );
        assert_eq!(request["properties"]["limit"]["minimum"], 1);
        assert_eq!(request["properties"]["limit"]["maximum"], 20);
        let response = &schemas["api.QuotesListResponse"];
        assert_eq!(
            response["required"],
            serde_json::json!(["object_id", "quotes", "next_cursor"])
        );
        assert_eq!(
            response["properties"]["next_cursor"]["type"],
            serde_json::json!(["string", "null"])
        );
        let quote = &response["$defs"]["QuotedObject"];
        assert_eq!(quote["required"], serde_json::json!(["edge", "object"]));
        assert!(
            quote["properties"]["object"]["anyOf"]
                .as_array()
                .unwrap()
                .iter()
                .any(|schema| schema["type"] == "null")
        );
    }

    fn assert_golden(file_name: &str, current: &str) {
        let path = fixture_path(file_name);
        let expected = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        assert_eq!(
            expected,
            current,
            "golden fixture drifted: regenerate {} through babel-schema export",
            path.display()
        );
    }

    fn fixture_path(file_name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("fixtures/protocol/v1")
            .join(file_name)
    }
}
