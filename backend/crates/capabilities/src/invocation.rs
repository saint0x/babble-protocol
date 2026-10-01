//! Private, immutable invocation intent and legal consent transitions.
//!
//! Callers authenticate the supplied context, resolve defaults and resources,
//! enforce method budgets/quotas, and invalidate old epochs on restart. These
//! types confer no authority from client-supplied identifiers alone.
use crate::CapabilityId;
use babel_types::{Canonical, Error, Hash, IdentityId, ObjectId, Result, Timestamp};
use rand_core::{OsRng, RngCore};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_INVOCATION_PAYLOAD_BYTES: usize = 64 * 1024;
pub const MAX_INVOCATION_SCOPE_BYTES: usize = 8 * 1024;
pub const MAX_INVOCATION_TTL_MS: i128 = 300_000;
pub const MAX_INVOCATION_REVISION: u32 = 4;

/// Social capabilities whose effects require a durable one-use invocation.
pub fn is_social_invocation(capability: &str) -> bool {
    matches!(
        capability,
        "babel.social.follow"
            | "babel.social.unfollow"
            | "babel.social.share"
            | "babel.social.reply"
    )
}

pub fn is_browser_invocation(capability: &str) -> bool {
    matches!(capability, "babel.clipboard.write" | "babel.fullscreen.enter")
}

pub fn is_one_use_invocation(capability: &str) -> bool {
    is_social_invocation(capability) || is_browser_invocation(capability)
}

pub mod browser;

pub fn new_context_epoch() -> Result<Hash> {
    Ok(InvocationId::random()?.0)
}

pub fn invocation_key(context: &InvocationContext, request_key: &str) -> Result<Hash> {
    context.validate()?;
    invocation_key_for_login(&context.actor, &context.login_id, request_key)
}

/// Lookup namespace for authenticated history reads without live document authority.
pub fn invocation_key_for_login(
    actor: &IdentityId,
    login_id: &str,
    request_key: &str,
) -> Result<Hash> {
    actor.validate()?;
    hex_id(actor.as_str(), IdentityId::PREFIX)?;
    token(login_id)?;
    token(request_key)?;
    ("babel.invocation.key.v1", actor, login_id, request_key).canonical_hash()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct InvocationId(Hash);

impl InvocationId {
    fn random() -> Result<Self> {
        let mut bytes = [0; 32];
        OsRng
            .try_fill_bytes(&mut bytes)
            .map_err(|_| conflict("invocation challenge entropy unavailable"))?;
        Ok(Self(Hash::from_bytes(&bytes)))
    }

    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvocationOrigin {
    Surface {
        session_id: String,
        document_id: String,
        role: String,
        entry: String,
        resource_digest: Hash,
    },
    HostAction {
        document_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InvocationContext {
    pub actor: IdentityId,
    pub login_id: String,
    pub object_id: ObjectId,
    pub object_version: Hash,
    pub origin: InvocationOrigin,
    pub policy_revision: Hash,
    pub context_epoch: Hash,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvocationExecutor {
    LocalPublication,
    External { provider: String, version: String },
}

/// Construct at authoritative ingress. Stored records expose only an immutable
/// reference; retries must use the original server timestamps, never extend them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InvocationIntent {
    pub request_key: String,
    pub context: InvocationContext,
    pub method: String,
    pub method_version: u32,
    pub capability: CapabilityId,
    pub capability_version: u32,
    pub scope: Value,
    pub executor: InvocationExecutor,
    pub payload: Value,
    pub created_at: Timestamp,
    pub deadline: Timestamp,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InvocationInvalidation {
    ContextLost,
    SessionRevoked,
    PolicyChanged,
    Restart,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvocationOutcome {
    Publication { receipt: Hash },
    External { dispatch_id: Hash, result: Value },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvocationState {
    Pending,
    Approved,
    Running { dispatch_id: Hash },
    Completed { outcome: InvocationOutcome },
    Failed { code: String },
    Unknown { dispatch_id: Hash },
    Denied,
    Cancelled,
    Expired,
    Invalidated { reason: InvocationInvalidation },
}

impl InvocationState {
    pub fn is_terminal(&self) -> bool {
        !matches!(
            self,
            Self::Pending | Self::Approved | Self::Running { .. } | Self::Unknown { .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InvocationAction {
    Approve,
    Deny,
    Cancel,
    Expire,
    Invalidate { reason: InvocationInvalidation },
    Dispatch { dispatch_id: Hash },
    CompletePublication { receipt: Hash },
    CompleteExternal { dispatch_id: Hash, result: Value },
    Fail { code: String },
    FailExternal { dispatch_id: Hash, code: String },
    MarkUnknown { dispatch_id: Hash },
}

/// One immutable phase. The store validates the entire chain, including the
/// previous canonical hash, under its writer lock before committing a new phase.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InvocationRecord {
    id: InvocationId,
    intent: InvocationIntent,
    intent_hash: Hash,
    revision: u32,
    previous: Option<Hash>,
    action: Option<InvocationAction>,
    state: InvocationState,
    updated_at: Timestamp,
    consumed_at: Option<Timestamp>,
}

impl InvocationContext {
    pub fn validate(&self) -> Result<()> {
        self.actor.validate()?;
        self.object_id.validate()?;
        hex_id(self.actor.as_str(), IdentityId::PREFIX)?;
        hex_id(self.object_id.as_str(), ObjectId::PREFIX)?;
        token(&self.login_id)?;
        hash(&self.object_version)?;
        hash(&self.policy_revision)?;
        hash(&self.context_epoch)?;
        match &self.origin {
            InvocationOrigin::Surface {
                session_id,
                document_id,
                role,
                entry,
                resource_digest,
            } => {
                for value in [session_id, document_id, role, entry] {
                    token(value)?;
                }
                hash(resource_digest)?;
            }
            InvocationOrigin::HostAction { document_id } => token(document_id)?,
        }
        Ok(())
    }
}

impl InvocationIntent {
    pub fn validate(&self) -> Result<()> {
        self.context.validate()?;
        token(&self.request_key)?;
        token(&self.method)?;
        token(self.capability.as_str())?;
        CapabilityId::new(self.capability.as_str())?;
        if self.method_version == 0 || self.capability_version == 0 {
            return Err(conflict("invocation versions must be positive"));
        }
        bounded_json(&self.payload, MAX_INVOCATION_PAYLOAD_BYTES)?;
        bounded_json(&self.scope, MAX_INVOCATION_SCOPE_BYTES)?;
        if let InvocationExecutor::External { provider, version } = &self.executor {
            token(provider)?;
            token(version)?;
        }
        if is_browser_invocation(self.capability.as_str())
            && (self.method != format!("{}.v2", self.capability.as_str())
                || self.method_version != 2 || self.capability_version != 1
                || self.executor != (InvocationExecutor::External {
                    provider: browser::BROWSER_EXECUTOR.into(), version: "1".into(),
                })) {
            return Err(conflict("browser invocation method or executor mismatch"));
        }
        let ttl = (self.deadline.0 - self.created_at.0).whole_nanoseconds();
        if self.deadline <= self.created_at || ttl > MAX_INVOCATION_TTL_MS * 1_000_000 {
            return Err(conflict("invalid invocation server deadline"));
        }
        Ok(())
    }

    pub fn key(&self) -> Result<Hash> {
        invocation_key(&self.context, &self.request_key)
    }

    pub fn fingerprint(&self) -> Result<Hash> {
        self.validate()?;
        ("babel.invocation.intent.v1", self).canonical_hash()
    }

    pub fn payload_hash(&self) -> Result<Hash> {
        bounded_json(&self.payload, MAX_INVOCATION_PAYLOAD_BYTES)?;
        self.payload.canonical_hash()
    }
}

impl InvocationRecord {
    pub fn pending(intent: InvocationIntent, now: Timestamp) -> Result<Self> {
        intent.validate()?;
        if now < intent.created_at || now >= intent.deadline {
            return Err(conflict("invocation is outside its server deadline"));
        }
        Ok(Self {
            id: InvocationId::random()?,
            intent_hash: intent.fingerprint()?,
            intent,
            revision: 0,
            previous: None,
            action: None,
            state: InvocationState::Pending,
            updated_at: now,
            consumed_at: None,
        })
    }

    pub fn id(&self) -> &InvocationId {
        &self.id
    }
    pub fn intent(&self) -> &InvocationIntent {
        &self.intent
    }
    pub fn revision(&self) -> u32 {
        self.revision
    }
    pub fn state(&self) -> &InvocationState {
        &self.state
    }
    pub fn updated_at(&self) -> Timestamp {
        self.updated_at
    }
    pub fn consumed_at(&self) -> Option<Timestamp> {
        self.consumed_at
    }

    pub fn action(&self) -> Option<&InvocationAction> {
        self.action.as_ref()
    }

    pub fn storage_id(&self) -> Result<String> {
        Ok(format!("{}-{}", self.intent.key()?, self.revision))
    }

    /// Validate a phase's bounded shape. History validation must additionally call
    /// validate_successor for every non-initial phase.
    pub fn validate(&self) -> Result<()> {
        self.intent.validate()?;
        if self.intent.fingerprint()? != self.intent_hash {
            return Err(conflict("invocation intent digest mismatch"));
        }
        hash(&self.id.0)?;
        if self.revision > MAX_INVOCATION_REVISION || self.updated_at < self.intent.created_at {
            return Err(conflict("invalid invocation phase"));
        }
        if self.revision == 0 {
            if self.previous.is_some()
                || self.action.is_some()
                || self.state != InvocationState::Pending
                || self.consumed_at.is_some()
                || self.updated_at >= self.intent.deadline
            {
                return Err(conflict("invalid initial invocation phase"));
            }
        } else {
            hash(
                self.previous
                    .as_ref()
                    .ok_or_else(|| conflict("missing invocation preimage hash"))?,
            )?;
            let action = self
                .action
                .as_ref()
                .ok_or_else(|| conflict("missing invocation action"))?;
            action.validate()?;
            if state_for(action) != self.state {
                return Err(conflict("invocation action/state mismatch"));
            }
        }
        if self.consumed_at.is_some_and(|at| {
            at < self.intent.created_at || at >= self.intent.deadline || at > self.updated_at
        }) {
            return Err(conflict("invalid invocation consumption time"));
        }
        Ok(())
    }

    pub fn validate_successor(&self, next: &Self) -> Result<()> {
        next.validate()?;
        let action = next
            .action
            .clone()
            .ok_or_else(|| conflict("missing invocation action"))?;
        let expected = self.transition(action, &self.intent.context, next.updated_at)?;
        if &expected != next {
            return Err(conflict("invocation history or immutable intent mismatch"));
        }
        Ok(())
    }

    pub fn transition(
        &self,
        action: InvocationAction,
        context: &InvocationContext,
        now: Timestamp,
    ) -> Result<Self> {
        self.validate()?;
        context.validate()?;
        action.validate()?;
        if context != &self.intent.context {
            return Err(conflict("invocation context mismatch"));
        }
        if is_browser_invocation(self.intent.capability.as_str()) {
            match &action {
                InvocationAction::CompleteExternal { result, .. } => {
                    let result: browser::BrowserInvocationResult = serde_json::from_value(result.clone())
                        .map_err(|_| conflict("invalid browser result"))?;
                    if matches!(result, browser::BrowserInvocationResult::Failed { .. }) {
                        return Err(conflict("failed browser result requires failed phase"));
                    }
                    result.validate_for(&self.intent.method)?;
                }
                InvocationAction::FailExternal { code, .. } => {
                    serde_json::from_value::<browser::BrowserFailureCode>(Value::String(code.clone()))
                        .map_err(|_| conflict("invalid browser failure code"))?;
                }
                InvocationAction::Fail { .. } => return Err(conflict("browser failure requires dispatch binding")),
                _ => (),
            }
        }
        if now < self.updated_at || self.revision >= MAX_INVOCATION_REVISION {
            return Err(conflict("stale invocation transition"));
        }
        let before_consumption = matches!(
            self.state,
            InvocationState::Pending | InvocationState::Approved
        );
        let local = self.intent.executor == InvocationExecutor::LocalPublication;
        let legal = match (&self.state, &action) {
            (InvocationState::Pending, InvocationAction::Approve | InvocationAction::Deny) => true,
            (_, InvocationAction::Cancel | InvocationAction::Invalidate { .. }) => {
                before_consumption
            }
            (_, InvocationAction::Expire) => before_consumption && now >= self.intent.deadline,
            (InvocationState::Approved, InvocationAction::Dispatch { .. }) => !local,
            (InvocationState::Approved, InvocationAction::CompletePublication { .. }) => local,
            (
                InvocationState::Running { dispatch_id },
                InvocationAction::CompleteExternal {
                    dispatch_id: actual,
                    ..
                }
                | InvocationAction::MarkUnknown {
                    dispatch_id: actual,
                }
                | InvocationAction::FailExternal {
                    dispatch_id: actual, ..
                },
            ) => !local && dispatch_id == actual,
            (
                InvocationState::Unknown { dispatch_id },
                InvocationAction::CompleteExternal {
                    dispatch_id: actual,
                    ..
                } | InvocationAction::FailExternal {
                    dispatch_id: actual, ..
                },
            ) => !local && dispatch_id == actual,
            (
                InvocationState::Running { .. } | InvocationState::Unknown { .. },
                InvocationAction::Fail { .. },
            ) => !local,
            _ => false,
        };
        if !legal {
            return Err(conflict("illegal invocation transition"));
        }
        // Post-dispatch outcomes may arrive after expiry. They report a past
        // action and cannot create execution authority or dispatch again.
        if matches!(
            action,
            InvocationAction::Approve
                | InvocationAction::Deny
                | InvocationAction::Dispatch { .. }
                | InvocationAction::CompletePublication { .. }
        ) && now >= self.intent.deadline
        {
            return Err(conflict("invocation deadline expired"));
        }
        let consumed_at = if matches!(
            action,
            InvocationAction::Dispatch { .. } | InvocationAction::CompletePublication { .. }
        ) {
            Some(now)
        } else {
            self.consumed_at
        };
        Ok(Self {
            id: self.id.clone(),
            intent: self.intent.clone(),
            intent_hash: self.intent_hash.clone(),
            revision: self.revision + 1,
            previous: Some(self.canonical_hash()?),
            state: state_for(&action),
            action: Some(action),
            updated_at: now,
            consumed_at,
        })
    }
}

impl InvocationAction {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Dispatch { dispatch_id } | Self::MarkUnknown { dispatch_id } => {
                hash(dispatch_id)?
            }
            Self::CompletePublication { receipt } => hash(receipt)?,
            Self::CompleteExternal {
                dispatch_id,
                result,
            } => {
                hash(dispatch_id)?;
                bounded_json(result, MAX_INVOCATION_PAYLOAD_BYTES)?;
            }
            Self::Fail { code } => token(code)?,
            Self::FailExternal { dispatch_id, code } => {
                hash(dispatch_id)?;
                token(code)?;
            }
            _ => (),
        }
        Ok(())
    }
}

fn state_for(action: &InvocationAction) -> InvocationState {
    match action {
        InvocationAction::Approve => InvocationState::Approved,
        InvocationAction::Deny => InvocationState::Denied,
        InvocationAction::Cancel => InvocationState::Cancelled,
        InvocationAction::Expire => InvocationState::Expired,
        InvocationAction::Invalidate { reason } => InvocationState::Invalidated {
            reason: reason.clone(),
        },
        InvocationAction::Dispatch { dispatch_id } => InvocationState::Running {
            dispatch_id: dispatch_id.clone(),
        },
        InvocationAction::CompletePublication { receipt } => InvocationState::Completed {
            outcome: InvocationOutcome::Publication {
                receipt: receipt.clone(),
            },
        },
        InvocationAction::CompleteExternal {
            dispatch_id,
            result,
        } => InvocationState::Completed {
            outcome: InvocationOutcome::External {
                dispatch_id: dispatch_id.clone(),
                result: result.clone(),
            },
        },
        InvocationAction::Fail { code } => InvocationState::Failed { code: code.clone() },
        InvocationAction::FailExternal { code, .. } => InvocationState::Failed { code: code.clone() },
        InvocationAction::MarkUnknown { dispatch_id } => InvocationState::Unknown {
            dispatch_id: dispatch_id.clone(),
        },
    }
}

fn token(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(conflict("invalid bounded invocation identifier"));
    }
    Ok(())
}

fn hex_id(value: &str, prefix: &str) -> Result<()> {
    let suffix = value
        .strip_prefix(prefix)
        .ok_or_else(|| conflict("invalid invocation identifier prefix"))?;
    if suffix.len() != 64
        || !suffix
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(conflict("invalid invocation hash encoding"));
    }
    Ok(())
}

fn hash(value: &Hash) -> Result<()> {
    hex_id(value.as_str(), "")
}

fn bounded_json(value: &Value, limit: usize) -> Result<()> {
    fn visit(value: &Value, depth: usize, budget: &mut usize) -> Result<()> {
        if depth > 32 || *budget == 0 {
            return Err(conflict("invocation JSON complexity limit exceeded"));
        }
        *budget -= 1;
        match value {
            Value::Array(values) => {
                for value in values {
                    visit(value, depth + 1, budget)?;
                }
            }
            Value::Object(values) => {
                for (key, value) in values {
                    if key.len() > MAX_INVOCATION_PAYLOAD_BYTES {
                        return Err(conflict("invocation JSON key limit exceeded"));
                    }
                    visit(value, depth + 1, budget)?;
                }
            }
            Value::String(text) if text.len() > MAX_INVOCATION_PAYLOAD_BYTES => {
                return Err(conflict("invocation JSON string limit exceeded"));
            }
            _ => (),
        }
        Ok(())
    }
    visit(value, 0, &mut 8192)?;
    if value.canonical_bytes()?.len() > limit
        || serde_json::to_vec(value)
            .map_err(|e| Error::Canonical(e.to_string()))?
            .len()
            > limit
    {
        return Err(conflict("invocation JSON byte limit exceeded"));
    }
    Ok(())
}

fn conflict(message: &str) -> Error {
    Error::Conflict(message.into())
}
