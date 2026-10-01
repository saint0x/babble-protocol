use babel_capabilities::invocation::{InvocationId, InvocationState};
use babel_graph::Edge;
use babel_object::Object;
use babel_store::PublicationReceipt;
use babel_types::Timestamp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvocationSource {
    Surface {
        session_id: String,
        document_id: String,
    },
    HostAction {
        document_id: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrepareInvocationRequest {
    pub origin: InvocationSource,
    pub object_id: String,
    pub method: String,
    pub request_key: String,
    pub payload: Value,
    /// Relative budget, bounded by the server method budget on first ingress.
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}
fn default_timeout() -> u64 {
    30_000
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecoverInvocationRequest {
    pub object_id: String,
    pub method: String,
    pub request_key: String,
    pub payload: Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegisterHostDocumentRequest {
    pub object_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct HostDocumentResponse {
    pub document_id: String,
    pub object_id: String,
    pub expires_at: Timestamp,
    pub renew_after_ms: u64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InvocationDecision {
    AllowOnce,
    Deny,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecideInvocationRequest {
    pub decision: InvocationDecision,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct InvocationSocialResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object: Option<Object>,
    pub edge: Edge,
    pub receipt: PublicationReceipt,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct InvocationResponse {
    pub invocation_id: InvocationId,
    pub request_key: String,
    pub actor_id: String,
    pub object_id: String,
    pub origin: InvocationSource,
    pub method: String,
    pub payload: Value,
    pub created_at: Timestamp,
    pub deadline: Timestamp,
    pub state: InvocationState,
    pub revision: u32,
    pub result: Option<InvocationSocialResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct BrowserInvocationResponse {
    pub invocation_id: InvocationId,
    pub request_key: String,
    pub actor_id: String,
    pub object_id: String,
    pub origin: InvocationSource,
    pub method: String,
    pub payload: Value,
    pub created_at: Timestamp,
    pub deadline: Timestamp,
    pub state: InvocationState,
    pub revision: u32,
    pub result: Option<babel_capabilities::invocation::browser::BrowserInvocationResult>,
    pub execution_ticket: Option<babel_capabilities::invocation::browser::BrowserExecutionTicket>,
}
