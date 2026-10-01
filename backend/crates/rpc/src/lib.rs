use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct RpcMethodName(String);

impl RpcMethodName {
    pub fn new(value: impl Into<String>) -> Result<Self, RpcCatalogError> {
        let value = value.into();
        validate_method_name(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RpcIdempotency {
    ReadOnly,
    IdempotentByInput,
    RequiresIdempotencyKey,
    NonIdempotent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RpcCapabilityRequirement {
    pub capability: String,
    pub version: u32,
    pub required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RpcMethodDefinition {
    pub method: RpcMethodName,
    pub version: u32,
    pub input: String,
    pub output: String,
    pub capability: Option<RpcCapabilityRequirement>,
    pub idempotency: RpcIdempotency,
    pub timeout_ms: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RpcCatalog {
    pub protocol: String,
    pub methods: Vec<RpcMethodDefinition>,
}

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct RpcRequestId(String);

impl RpcRequestId {
    pub fn new(value: impl Into<String>) -> Result<Self, RpcCatalogError> {
        let value = value.into();
        let valid = !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'));
        if valid {
            Ok(Self(value))
        } else {
            Err(RpcCatalogError::InvalidRequestId(value))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RpcBinding {
    pub object_id: Option<String>,
    pub surface_session_id: Option<String>,
    pub runtime_id: String,
    pub origin: String,
    pub capability_grants: Vec<String>,
    #[serde(default)]
    pub identity_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RpcDeadline {
    pub timeout_ms: u64,
    pub client_started_at: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RpcRequestEnvelope {
    pub protocol: String,
    pub id: RpcRequestId,
    pub method: RpcMethodName,
    pub binding: RpcBinding,
    pub payload: Value,
    pub idempotency_key: Option<String>,
    pub deadline: RpcDeadline,
    pub trace_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RpcResponseEnvelope {
    pub protocol: String,
    pub id: RpcRequestId,
    pub result: Option<Value>,
    pub error: Option<RpcError>,
    pub trace_id: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RpcErrorCode {
    CapabilityDenied,
    CapabilityUnavailable,
    PermissionRequired,
    InvalidInput,
    NotFound,
    Conflict,
    RateLimited,
    QuotaExceeded,
    Timeout,
    Cancelled,
    Offline,
    ProviderUnavailable,
    StorageUnavailable,
    UnsupportedVersion,
    IntegrityFailure,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RpcError {
    pub code: RpcErrorCode,
    pub message: String,
    pub retryable: bool,
    pub retry_after_ms: Option<u64>,
    pub details: Value,
}

#[derive(Debug, thiserror::Error)]
pub enum RpcCatalogError {
    #[error("RPC method name must be a non-empty namespaced versioned method: {0}")]
    InvalidMethodName(String),
    #[error("RPC method version must be positive: {0}")]
    InvalidVersion(String),
    #[error("duplicate RPC method: {0}")]
    DuplicateMethod(String),
    #[error("invalid RPC request id: {0}")]
    InvalidRequestId(String),
    #[error("unsupported RPC protocol: {0}")]
    UnsupportedProtocol(String),
    #[error("unknown RPC method: {0}")]
    UnknownMethod(String),
    #[error("missing idempotency key for RPC method: {0}")]
    MissingIdempotencyKey(String),
    #[error("RPC binding runtime id and origin must not be empty")]
    InvalidBinding,
    #[error("RPC deadline must be positive and no greater than method timeout")]
    InvalidDeadline,
    #[error("RPC response must contain exactly one of result or error")]
    InvalidResponse,
}

impl RpcCatalog {
    pub fn new(
        protocol: impl Into<String>,
        methods: Vec<RpcMethodDefinition>,
    ) -> Result<Self, RpcCatalogError> {
        let mut seen = BTreeSet::new();
        for method in &methods {
            method.validate()?;
            if !seen.insert(method.method.as_str().to_string()) {
                return Err(RpcCatalogError::DuplicateMethod(
                    method.method.as_str().to_string(),
                ));
            }
        }
        Ok(Self {
            protocol: protocol.into(),
            methods,
        })
    }

    pub fn get(&self, method: &RpcMethodName) -> Option<&RpcMethodDefinition> {
        self.methods
            .iter()
            .find(|definition| &definition.method == method)
    }

    pub fn validate_request(&self, request: &RpcRequestEnvelope) -> Result<(), RpcCatalogError> {
        if request.protocol != self.protocol {
            return Err(RpcCatalogError::UnsupportedProtocol(
                request.protocol.clone(),
            ));
        }
        request.binding.validate()?;
        let Some(method) = self.get(&request.method) else {
            return Err(RpcCatalogError::UnknownMethod(
                request.method.as_str().to_string(),
            ));
        };
        if method.idempotency == RpcIdempotency::RequiresIdempotencyKey
            && request
                .idempotency_key
                .as_ref()
                .is_none_or(|key| key.trim().is_empty())
        {
            return Err(RpcCatalogError::MissingIdempotencyKey(
                method.method.as_str().to_string(),
            ));
        }
        if request.deadline.timeout_ms == 0 || request.deadline.timeout_ms > method.timeout_ms {
            return Err(RpcCatalogError::InvalidDeadline);
        }
        Ok(())
    }
}

impl RpcMethodDefinition {
    pub fn new(
        method: impl Into<String>,
        input: impl Into<String>,
        output: impl Into<String>,
        idempotency: RpcIdempotency,
    ) -> Result<Self, RpcCatalogError> {
        let method = RpcMethodName::new(method)?;
        let version = infer_method_version(method.as_str())?;
        Ok(Self {
            method,
            version,
            input: input.into(),
            output: output.into(),
            capability: None,
            idempotency,
            timeout_ms: 30_000,
        })
    }

    pub fn with_capability(
        mut self,
        capability: impl Into<String>,
        version: u32,
        required: bool,
    ) -> Result<Self, RpcCatalogError> {
        if version == 0 {
            return Err(RpcCatalogError::InvalidVersion(capability.into()));
        }
        self.capability = Some(RpcCapabilityRequirement {
            capability: capability.into(),
            version,
            required,
        });
        Ok(self)
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms.max(1);
        self
    }

    pub fn validate(&self) -> Result<(), RpcCatalogError> {
        validate_method_name(self.method.as_str())?;
        let inferred_version = infer_method_version(self.method.as_str())?;
        if self.version == 0 || self.version != inferred_version {
            return Err(RpcCatalogError::InvalidVersion(
                self.method.as_str().to_string(),
            ));
        }
        if let Some(capability) = &self.capability
            && capability.version == 0
        {
            return Err(RpcCatalogError::InvalidVersion(
                capability.capability.clone(),
            ));
        }
        Ok(())
    }
}

impl RpcBinding {
    pub fn host(
        runtime_id: impl Into<String>,
        origin: impl Into<String>,
    ) -> Result<Self, RpcCatalogError> {
        let binding = Self {
            object_id: None,
            surface_session_id: None,
            runtime_id: runtime_id.into(),
            origin: origin.into(),
            capability_grants: Vec::new(),
            identity_id: None,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn object(
        object_id: impl Into<String>,
        surface_session_id: impl Into<String>,
        runtime_id: impl Into<String>,
        origin: impl Into<String>,
        capability_grants: Vec<String>,
    ) -> Result<Self, RpcCatalogError> {
        let binding = Self {
            object_id: Some(object_id.into()),
            surface_session_id: Some(surface_session_id.into()),
            runtime_id: runtime_id.into(),
            origin: origin.into(),
            capability_grants,
            identity_id: None,
        };
        binding.validate()?;
        Ok(binding)
    }

    pub fn validate(&self) -> Result<(), RpcCatalogError> {
        if self.runtime_id.trim().is_empty()
            || self.origin.trim().is_empty()
            || self
                .object_id
                .as_ref()
                .is_some_and(|value| value.trim().is_empty())
            || self
                .surface_session_id
                .as_ref()
                .is_some_and(|value| value.trim().is_empty())
            || self
                .identity_id
                .as_ref()
                .is_some_and(|value| value.trim().is_empty())
            || self
                .capability_grants
                .iter()
                .any(|grant| grant.trim().is_empty())
        {
            return Err(RpcCatalogError::InvalidBinding);
        }
        Ok(())
    }
}

impl RpcRequestEnvelope {
    pub fn new(
        catalog: &RpcCatalog,
        id: impl Into<String>,
        method: impl Into<String>,
        binding: RpcBinding,
        payload: Value,
    ) -> Result<Self, RpcCatalogError> {
        let method = RpcMethodName::new(method)?;
        let definition = catalog
            .get(&method)
            .ok_or_else(|| RpcCatalogError::UnknownMethod(method.as_str().to_string()))?;
        let request = Self {
            protocol: catalog.protocol.clone(),
            id: RpcRequestId::new(id)?,
            method,
            binding,
            payload,
            idempotency_key: None,
            deadline: RpcDeadline {
                timeout_ms: definition.timeout_ms,
                client_started_at: None,
            },
            trace_id: None,
        };
        Ok(request)
    }

    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    pub fn with_trace_id(mut self, trace_id: impl Into<String>) -> Self {
        self.trace_id = Some(trace_id.into());
        self
    }
}

impl RpcResponseEnvelope {
    pub fn ok(catalog: &RpcCatalog, request: &RpcRequestEnvelope, result: Value) -> Self {
        Self {
            protocol: catalog.protocol.clone(),
            id: request.id.clone(),
            result: Some(result),
            error: None,
            trace_id: request.trace_id.clone(),
        }
    }

    pub fn err(catalog: &RpcCatalog, request: &RpcRequestEnvelope, error: RpcError) -> Self {
        Self {
            protocol: catalog.protocol.clone(),
            id: request.id.clone(),
            result: None,
            error: Some(error),
            trace_id: request.trace_id.clone(),
        }
    }

    pub fn validate(&self, catalog: &RpcCatalog) -> Result<(), RpcCatalogError> {
        if self.protocol != catalog.protocol {
            return Err(RpcCatalogError::UnsupportedProtocol(self.protocol.clone()));
        }
        if self.result.is_some() == self.error.is_some() {
            return Err(RpcCatalogError::InvalidResponse);
        }
        Ok(())
    }
}

impl RpcError {
    pub fn new(code: RpcErrorCode, message: impl Into<String>) -> Self {
        let retryable = matches!(
            code,
            RpcErrorCode::RateLimited
                | RpcErrorCode::Timeout
                | RpcErrorCode::Offline
                | RpcErrorCode::ProviderUnavailable
                | RpcErrorCode::StorageUnavailable
                | RpcErrorCode::Internal
        );
        Self {
            code,
            message: message.into(),
            retryable,
            retry_after_ms: None,
            details: Value::Null,
        }
    }

    pub fn with_retry_after_ms(mut self, retry_after_ms: u64) -> Self {
        self.retry_after_ms = Some(retry_after_ms);
        self.retryable = true;
        self
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }
}

pub fn babble_rpc_catalog() -> Result<RpcCatalog, RpcCatalogError> {
    RpcCatalog::new(
        "babble.rpc.v1",
        vec![
            method(
                "babble.observability.snapshot.v1",
                "api.EmptyRequest",
                "api.ObservabilitySnapshotResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.identity.create.v1",
                "api.CreateIdentityRequest",
                "api.CreateIdentityResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.identity.current.v1",
                "api.EmptyRequest",
                "api.IdentityCurrentResponse",
                RpcIdempotency::ReadOnly,
            )?
            .with_capability("babble.identity.current", 1, true)?,
            method(
                "babble.object.publish_text.v1",
                "api.PublishTextRequest",
                "api.PublishTextResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.object.publish.v1",
                "api.PublishObjectRequest",
                "api.PublishObjectResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.object.publish_media.v1",
                "api.PublishMediaObjectRequest",
                "api.PublishMediaObjectResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.object.fork.v1",
                "api.ForkObjectRequest",
                "api.ProvenancePublicationResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.object.remix.v1",
                "api.RemixObjectRequest",
                "api.ProvenancePublicationResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.object.get.v1",
                "api.ObjectIdRequest",
                "api.PublishTextResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.media.blob.put.v1",
                "api.PutMediaBlobRequest",
                "api.MediaBlobResponse",
                RpcIdempotency::IdempotentByInput,
            )?,
            method(
                "babble.media.blob.get.v1",
                "api.GetMediaBlobRequest",
                "api.MediaBlobResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.graph.edge.publish.v1",
                "api.PublishEdgeRequest",
                "api.PublishEdgeResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.graph.relationship.infer.v1",
                "api.InferRelationshipRequest",
                "api.InferRelationshipResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.graph.evidence.v1",
                "api.ObjectIdRequest",
                "api.ClaimEvidenceResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.graph.traverse.v1",
                "api.GraphTraverseRpcRequest",
                "api.GraphTraversalResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.social.follow.v1",
                "api.SocialTargetRequest",
                "api.SocialEdgeResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.social.follow", 1, true)?,
            method(
                "babble.social.follow",
                "api.SocialTargetRequest",
                "api.InvocationSocialResult",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.social.follow", 1, true)?,
            method(
                "babble.social.unfollow",
                "api.SocialTargetRequest",
                "api.InvocationSocialResult",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.social.unfollow", 1, true)?,
            method(
                "babble.social.share",
                "api.SocialTextRequest",
                "api.InvocationSocialResult",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.social.share", 1, true)?,
            method(
                "babble.social.reply",
                "api.SocialTextRequest",
                "api.InvocationSocialResult",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.social.reply", 1, true)?,
            method(
                "babble.social.unfollow.v1",
                "api.SocialTargetRequest",
                "api.SocialEdgeResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.social.unfollow", 1, true)?,
            method(
                "babble.social.share.v1",
                "api.SocialTextRequest",
                "api.SocialTextResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.social.share", 1, true)?,
            method(
                "babble.social.reply.v1",
                "api.SocialTextRequest",
                "api.SocialTextResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.social.reply", 1, true)?,
            method(
                "babble.social.replies.list.v1",
                "api.RepliesListRequest",
                "api.RepliesListResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.social.quotes.list.v1",
                "api.QuotesListRequest",
                "api.QuotesListResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.social.reactions.summary.v1",
                "api.ReactionObjectRequest",
                "graph.ReactionSummary",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.social.reactions.record.v1",
                "api.ReactionRecordRequest",
                "graph.ReactionRecord",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.social.reactions.mine.v1",
                "api.ReactionObjectRequest",
                "graph.ReactionState",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.social.reactions.set.v1",
                "api.SetReactionRpcRequest",
                "graph.ReactionState",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.events.list.v1",
                "api.EventListRequest",
                "api.EventListResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.events.bundle.v1",
                "api.EventBundleRequest",
                "api.EventBundleResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.events.import.v1",
                "api.EventImportRequest",
                "api.EventImportResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.consensus.checkpoint.preview.v1",
                "api.CheckpointPreviewRequest",
                "api.CheckpointPreviewResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.consensus.checkpoint.publish.v1",
                "api.CheckpointRequest",
                "api.CheckpointEventResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.judgment.object.evaluate.v1",
                "api.JudgeObjectRpcRequest",
                "api.JudgeObjectResponse",
                RpcIdempotency::IdempotentByInput,
            )?
            .with_capability("babble.ai.judge", 1, false)?,
            method(
                "babble.ai.judge.v1",
                "api.JudgeObjectRpcRequest",
                "api.JudgeObjectResponse",
                RpcIdempotency::IdempotentByInput,
            )?
            .with_capability("babble.ai.judge", 1, true)?,
            method(
                "babble.ai.generate.v1",
                "api.AiGenerateRequest",
                "api.AiGenerateResponse",
                RpcIdempotency::NonIdempotent,
            )?
            .with_capability("babble.ai.generate", 1, true)?,
            method(
                "babble.ai.embed.v1",
                "api.AiEmbedRequest",
                "api.AiEmbedResponse",
                RpcIdempotency::IdempotentByInput,
            )?
            .with_capability("babble.ai.embed", 1, true)?,
            method(
                "babble.ai.transcribe.v1",
                "api.AiTranscribeRequest",
                "api.AiTranscribeResponse",
                RpcIdempotency::NonIdempotent,
            )?
            .with_capability("babble.ai.transcribe", 1, true)?,
            method(
                "babble.judgment.object.list.v1",
                "api.ObjectIdRequest",
                "api.ObjectJudgmentsResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.judgment.definitions.list.v1",
                "api.EmptyRequest",
                "api.JudgmentDefinitionsResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.judgment.providers.list.v1",
                "api.EmptyRequest",
                "api.JudgmentProvidersResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.search.objects.v1",
                "api.ObjectSearchRequest",
                "api.ObjectSearchResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.lenses.list.v1",
                "api.EmptyRequest",
                "api.LensCatalogResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.discovery.candidates.v1",
                "api.DiscoveryRequest",
                "api.DiscoveryResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.capabilities.list.v1",
                "api.EmptyRequest",
                "api.CapabilityCatalogResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.capabilities.inspect.v1",
                "api.ObjectIdRequest",
                "api.CapabilitiesResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.capabilities.grant.v1",
                "api.GrantCapabilityRequest",
                "api.GrantCapabilityResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.capabilities.revoke.v1",
                "api.RevokeCapabilityRequest",
                "api.GrantCapabilityResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.storage.object.get.v1",
                "api.ObjectStorageGetRequest",
                "api.ObjectStorageGetResponse",
                RpcIdempotency::ReadOnly,
            )?
            .with_capability("babble.storage.object", 1, true)?,
            method(
                "babble.storage.local.get.v1",
                "api.LocalStorageGetRequest",
                "api.LocalStorageGetResponse",
                RpcIdempotency::ReadOnly,
            )?
            .with_capability("babble.storage.local", 1, true)?,
            method(
                "babble.storage.local.set.v1",
                "api.LocalStorageSetRequest",
                "api.LocalStorageSetResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.storage.local", 1, true)?,
            method(
                "babble.storage.local.delete.v1",
                "api.LocalStorageDeleteRequest",
                "api.LocalStorageDeleteResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.storage.local", 1, true)?,
            method(
                "babble.storage.local.list.v1",
                "api.LocalStorageListRequest",
                "api.LocalStorageListResponse",
                RpcIdempotency::ReadOnly,
            )?
            .with_capability("babble.storage.local", 1, true)?,
            method(
                "babble.storage.object.set.v1",
                "api.ObjectStorageSetRequest",
                "api.ObjectStorageSetResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.storage.object", 1, true)?,
            method(
                "babble.storage.object.delete.v1",
                "api.ObjectStorageDeleteRequest",
                "api.ObjectStorageDeleteResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.storage.object", 1, true)?,
            method(
                "babble.storage.object.list.v1",
                "api.ObjectStorageListRequest",
                "api.ObjectStorageListResponse",
                RpcIdempotency::ReadOnly,
            )?
            .with_capability("babble.storage.object", 1, true)?,
            method(
                "babble.personalization.sync.put.v1",
                "api.PersonalizationSyncPutRequest",
                "api.PersonalizationSyncPutResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.personalization.sync.list.v1",
                "api.PersonalizationSyncListRequest",
                "api.PersonalizationSyncListResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.personalization.sync.get.v1",
                "api.PersonalizationSyncGetRequest",
                "api.PersonalizationSyncGetResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.personalization.sync.delete.v1",
                "api.PersonalizationSyncGetRequest",
                "api.PersonalizationSyncDeleteResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?,
            method(
                "babble.network.fetch.v1",
                "api.NetworkFetchRequest",
                "api.NetworkFetchResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.network.fetch", 1, true)?,
            method(
                "babble.payments.checkout.v1",
                "api.PaymentsCheckoutRequest",
                "api.PaymentsCheckoutResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.payments.checkout", 1, true)?,
            method(
                "babble.notifications.request.v1",
                "api.NotificationsRequestRequest",
                "api.NotificationsRequestResponse",
                RpcIdempotency::NonIdempotent,
            )?
            .with_capability("babble.notifications.request", 1, true)?,
            method(
                "babble.media.camera.request.v1",
                "api.CameraCaptureRequest",
                "api.CameraCaptureResponse",
                RpcIdempotency::NonIdempotent,
            )?
            .with_capability("babble.media.camera", 1, true)?,
            method(
                "babble.media.microphone.request.v1",
                "api.MicrophoneCaptureRequest",
                "api.MicrophoneCaptureResponse",
                RpcIdempotency::NonIdempotent,
            )?
            .with_capability("babble.media.microphone", 1, true)?,
            method(
                "babble.clipboard.write.v1",
                "api.ClipboardWriteRequest",
                "api.ClipboardWriteResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.clipboard.write", 1, true)?,
            method(
                "babble.fullscreen.enter.v1",
                "api.FullscreenEnterRequest",
                "api.FullscreenEnterResponse",
                RpcIdempotency::NonIdempotent,
            )?
            .with_capability("babble.fullscreen.enter", 1, true)?,
            method(
                "babble.clipboard.write",
                "api.ClipboardWriteRequest",
                "api.BrowserInvocationResult",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.clipboard.write", 1, true)?,
            method(
                "babble.fullscreen.enter",
                "api.FullscreenEnterRequest",
                "api.BrowserInvocationResult",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.fullscreen.enter", 1, true)?,
            method(
                "babble.runtime.surface.prepare.v1",
                "api.PrepareSurfaceRequest",
                "api.PrepareSurfaceResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.runtime.surface.session.start.v1",
                "api.StartSurfaceSessionRequest",
                "api.SurfaceSessionResponse",
                RpcIdempotency::NonIdempotent,
            )?,
            method(
                "babble.runtime.surface.health.v1",
                "api.EmptyRequest",
                "api.SurfaceRuntimeHealthResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.runtime.surface.session.get.v1",
                "api.SurfaceSessionRequest",
                "api.SurfaceSessionResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.runtime.surface.session.transition.v1",
                "api.TransitionSurfaceSessionRequest",
                "api.SurfaceSessionEventResponse",
                RpcIdempotency::NonIdempotent,
            )?,
            method(
                "babble.runtime.surface.session.heartbeat.v1",
                "api.EmptyRequest",
                "api.SurfaceLeaseResponse",
                RpcIdempotency::NonIdempotent,
            )?,
            method(
                "babble.runtime.surface.session.budget.v1",
                "api.ChangeSurfaceBudgetRequest",
                "api.SurfaceSessionEventResponse",
                RpcIdempotency::NonIdempotent,
            )?,
            method(
                "babble.runtime.surface.session.schedule.v1",
                "api.ScheduleSurfaceSessionRequest",
                "api.ScheduleSurfaceSessionResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.runtime.surface.session.apply_schedule.v1",
                "api.ScheduleSurfaceSessionRequest",
                "api.ApplySurfaceScheduleResponse",
                RpcIdempotency::NonIdempotent,
            )?,
            method(
                "babble.runtime.surface.session.state.checkpoint.v1",
                "api.CheckpointSurfaceStateRequest",
                "api.SurfaceStateCheckpointResponse",
                RpcIdempotency::NonIdempotent,
            )?,
            method(
                "babble.runtime.surface.session.state.get.v1",
                "api.SurfaceSessionRequest",
                "api.SurfaceStateRestoreResponse",
                RpcIdempotency::ReadOnly,
            )?,
            method(
                "babble.realtime.room.define.v1",
                "api.DefineRealtimeRoomRequest",
                "api.DefineRealtimeRoomResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.realtime.join", 1, false)?,
            method(
                "babble.realtime.session.start.v1",
                "api.StartRealtimeSessionRequest",
                "api.StartRealtimeSessionResponse",
                RpcIdempotency::NonIdempotent,
            )?
            .with_capability("babble.realtime.join", 1, true)?,
            method(
                "babble.realtime.session.leave.v1",
                "api.CloseRealtimeSessionRequest",
                "api.CloseRealtimeSessionResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.realtime.leave", 1, true)?,
            method(
                "babble.realtime.message.publish.v1",
                "api.PublishRealtimeMessageRequest",
                "api.PublishRealtimeMessageResponse",
                RpcIdempotency::RequiresIdempotencyKey,
            )?
            .with_capability("babble.realtime.send", 1, true)?,
        ],
    )
}

fn method(
    name: impl Into<String>,
    input: impl Into<String>,
    output: impl Into<String>,
    idempotency: RpcIdempotency,
) -> Result<RpcMethodDefinition, RpcCatalogError> {
    RpcMethodDefinition::new(name, input, output, idempotency)
}

fn infer_method_version(value: &str) -> Result<u32, RpcCatalogError> {
    if value.is_empty()
        || !value.starts_with("babble.")
        || value.ends_with('.')
        || value.contains("..")
        || !value.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'.' || byte == b'_'
        })
    {
        return Err(RpcCatalogError::InvalidMethodName(value.to_string()));
    }

    let Some(last_segment) = value.rsplit('.').next() else {
        return Err(RpcCatalogError::InvalidMethodName(value.to_string()));
    };
    if let Some(version) = last_segment.strip_prefix('v') {
        if version.is_empty() || !version.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(RpcCatalogError::InvalidMethodName(value.to_string()));
        }
        return version
            .parse::<u32>()
            .ok()
            .filter(|version| *version > 0)
            .ok_or_else(|| RpcCatalogError::InvalidVersion(value.to_string()));
    }

    Ok(2)
}

fn validate_method_name(value: &str) -> Result<(), RpcCatalogError> {
    infer_method_version(value).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invocation_social_catalog_has_honest_receipt_contract_and_capability_v1() {
        let catalog = babble_rpc_catalog().unwrap();
        for action in ["follow", "unfollow", "share", "reply"] {
            let name = RpcMethodName::new(format!("babble.social.{action}")).unwrap();
            let definition = catalog.get(&name).unwrap();
            assert_eq!(definition.version, 2);
            assert_eq!(
                definition.method.as_str(),
                format!("babble.social.{action}")
            );
            assert_eq!(definition.output, "api.InvocationSocialResult");
            assert_eq!(definition.capability.as_ref().unwrap().version, 1);
            assert_eq!(definition.timeout_ms, 30_000);
        }
    }

    #[test]
    fn storage_failure_is_retryable_but_conflicting_intent_is_not() {
        let error = RpcError::new(RpcErrorCode::StorageUnavailable, "storage unavailable");
        assert!(error.retryable);
        assert_eq!(
            serde_json::to_value(&error).unwrap()["code"],
            "STORAGE_UNAVAILABLE"
        );
        assert!(!RpcError::new(RpcErrorCode::Conflict, "conflicting intent").retryable);
    }

    #[test]
    fn default_catalog_has_unique_versioned_methods() {
        let catalog = babble_rpc_catalog().unwrap();
        let names = catalog
            .methods
            .iter()
            .map(|method| method.method.as_str())
            .collect::<BTreeSet<_>>();

        assert_eq!(names.len(), catalog.methods.len());
        assert!(names.contains("babble.object.publish_media.v1"));
        assert!(names.contains("babble.object.fork.v1"));
        assert!(names.contains("babble.object.remix.v1"));
        assert!(names.contains("babble.judgment.object.list.v1"));
        assert!(names.contains("babble.realtime.message.publish.v1"));
        assert!(catalog.methods.iter().all(|method| {
            let name = method.method.as_str();
            name.ends_with(&format!(".v{}", method.version))
                || (!name.rsplit('.').next().unwrap().starts_with('v') && method.version == 2)
        }));
    }

    #[test]
    fn method_definition_version_matches_method_suffix() {
        assert_eq!(
            method(
                "babble.object.get.v1",
                "Input",
                "Output",
                RpcIdempotency::ReadOnly
            )
            .unwrap()
            .version,
            1
        );
        assert_eq!(
            method(
                "babble.social.follow",
                "Input",
                "Output",
                RpcIdempotency::RequiresIdempotencyKey
            )
            .unwrap()
            .version,
            2
        );

        let mut definition = method(
            "babble.object.get.v1",
            "Input",
            "Output",
            RpcIdempotency::ReadOnly,
        )
        .unwrap();
        definition.version = 2;
        assert!(matches!(
            definition.validate(),
            Err(RpcCatalogError::InvalidVersion(_))
        ));
        assert!(RpcMethodName::new("babble.object.get.vx").is_err());
        assert!(RpcMethodName::new("babble.object.get.v0").is_err());
    }

    #[test]
    fn mutating_methods_declare_idempotency() {
        let catalog = babble_rpc_catalog().unwrap();

        assert_eq!(
            idempotency(&catalog, "babble.object.get.v1"),
            RpcIdempotency::ReadOnly
        );
        assert_eq!(
            idempotency(&catalog, "babble.runtime.surface.prepare.v1"),
            RpcIdempotency::ReadOnly
        );
        assert_eq!(
            idempotency(&catalog, "babble.judgment.object.list.v1"),
            RpcIdempotency::ReadOnly
        );
        assert_eq!(
            idempotency(&catalog, "babble.object.publish_media.v1"),
            RpcIdempotency::RequiresIdempotencyKey
        );
        assert_eq!(
            idempotency(&catalog, "babble.realtime.session.start.v1"),
            RpcIdempotency::NonIdempotent
        );
        assert_eq!(
            idempotency(&catalog, "babble.realtime.session.leave.v1"),
            RpcIdempotency::RequiresIdempotencyKey
        );
    }

    #[test]
    fn invalid_method_names_are_rejected() {
        assert!(RpcMethodName::new("object.publish").is_err());
        assert!(RpcMethodName::new("babble.object.publish.v0").is_err());
        assert!(RpcMethodName::new("babble.social.reply").is_ok());
        assert!(RpcMethodName::new("babble.object.Publish.v1").is_err());
    }

    #[test]
    fn request_envelopes_bind_callers_deadlines_and_idempotency() {
        let catalog = babble_rpc_catalog().unwrap();
        let binding = RpcBinding::object(
            "obj_abc",
            "sess_abc",
            "runtime-1",
            "https://app.example",
            vec![],
        )
        .unwrap();
        let request = RpcRequestEnvelope::new(
            &catalog,
            "req-1",
            "babble.object.publish_media.v1",
            binding,
            serde_json::json!({"title": "draft"}),
        )
        .unwrap();

        assert!(matches!(
            catalog.validate_request(&request),
            Err(RpcCatalogError::MissingIdempotencyKey(_))
        ));
        let request = request.with_idempotency_key("publish-media-1");
        catalog.validate_request(&request).unwrap();
    }

    #[test]
    fn response_envelopes_require_exactly_one_outcome() {
        let catalog = babble_rpc_catalog().unwrap();
        let request = RpcRequestEnvelope::new(
            &catalog,
            "req-2",
            "babble.object.get.v1",
            RpcBinding::host("host-runtime", "babble://host").unwrap(),
            serde_json::json!({"id": "obj"}),
        )
        .unwrap();
        let ok = RpcResponseEnvelope::ok(&catalog, &request, serde_json::json!({"ok": true}));
        let err = RpcResponseEnvelope::err(
            &catalog,
            &request,
            RpcError::new(RpcErrorCode::NotFound, "object not found"),
        );

        ok.validate(&catalog).unwrap();
        err.validate(&catalog).unwrap();
        assert!(
            RpcResponseEnvelope {
                protocol: catalog.protocol,
                id: request.id,
                result: None,
                error: None,
                trace_id: None,
            }
            .validate(&babble_rpc_catalog().unwrap())
            .is_err()
        );
    }

    fn idempotency(catalog: &RpcCatalog, method: &str) -> RpcIdempotency {
        let name = RpcMethodName::new(method).unwrap();
        catalog.get(&name).unwrap().idempotency.clone()
    }
}
