//! Private identity relationships. These records never enter the public Object graph.
use babble_crypto::{Keypair, Signature};
use babble_identity::Identity;
use babble_types::{Canonical, Error, Hash, IdentityId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SafetyState {
    pub author_id: IdentityId,
    pub target_id: IdentityId,
    pub blocked: bool,
    pub muted: bool,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
}

impl SafetyState {
    pub fn absent(author: &IdentityId, target: &IdentityId) -> Self {
        Self {
            author_id: author.clone(),
            target_id: target.clone(),
            blocked: false,
            muted: false,
            revision: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SafetyRequest {
    pub author_id: IdentityId,
    pub target_id: IdentityId,
    pub blocked: bool,
    pub muted: bool,
    pub expected_revision: u64,
    pub idempotency_key: String,
}

impl SafetyRequest {
    pub fn validate(&self) -> Result<()> {
        self.author_id.validate()?;
        self.target_id.validate()?;
        if self.author_id == self.target_id {
            return Err(Error::Canonical("cannot mute or block yourself".into()));
        }
        if self.idempotency_key.is_empty()
            || self.idempotency_key.len() > 256
            || !self.idempotency_key.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(Error::Canonical(
                "safety idempotency key must be 1..256 visible ASCII bytes".into(),
            ));
        }
        if self.expected_revision > 9_007_199_254_740_991 {
            return Err(Error::Canonical("safety revision out of range".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["state", "previous_id", "created_at", "request_id"]))]
pub struct SafetyActionPayload {
    pub state: SafetyState,
    pub previous_id: Option<Hash>,
    pub created_at: Timestamp,
    pub request_id: Hash,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SafetyAction {
    pub id: Hash,
    pub payload: SafetyActionPayload,
    pub signature: Signature,
}

impl SafetyAction {
    pub fn sign(payload: SafetyActionPayload, key: &Keypair) -> Result<Self> {
        let bytes = ("babble.private.safety.action.v1", &payload).canonical_bytes()?;
        Ok(Self {
            id: Hash::from_bytes(&bytes),
            payload,
            signature: key.sign(&bytes),
        })
    }

    pub fn verify(&self, signer: &Identity) -> Result<()> {
        let state = &self.payload.state;
        state.author_id.validate()?;
        state.target_id.validate()?;
        if signer.id != state.author_id
            || state.author_id == state.target_id
            || state.revision == 0
            || state.revision > 9_007_199_254_740_991
            || (state.revision == 1) != self.payload.previous_id.is_none()
        {
            return Err(Error::Signature);
        }
        let bytes = ("babble.private.safety.action.v1", &self.payload).canonical_bytes()?;
        if self.id != Hash::from_bytes(&bytes) {
            return Err(Error::Signature);
        }
        signer.public_key.verify(&bytes, &self.signature)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["request", "state", "created_at", "sequence", "previous_id"]))]
pub struct SafetyReceiptPayload {
    pub request: SafetyRequest,
    pub state: SafetyState,
    pub created_at: Timestamp,
    pub sequence: u64,
    pub previous_id: Option<Hash>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SafetyReceipt {
    pub payload: SafetyReceiptPayload,
    pub signature: Signature,
}

impl SafetyReceipt {
    pub fn id(&self) -> Result<Hash> {
        ("babble.private.safety.receipt.v1", &self.payload).canonical_hash()
    }

    pub fn sign(payload: SafetyReceiptPayload, key: &Keypair) -> Result<Self> {
        let signature = key.sign(&("babble.private.safety.receipt.v1", &payload).canonical_bytes()?);
        Ok(Self { payload, signature })
    }

    pub fn verify(&self, signer: &Identity) -> Result<()> {
        let request = &self.payload.request;
        let state = &self.payload.state;
        request.validate()?;
        if signer.id != request.author_id
            || self.payload.sequence == 0
            || self.payload.sequence > 9_007_199_254_740_991
            || (self.payload.sequence == 1) != self.payload.previous_id.is_none()
            || state.author_id != request.author_id
            || state.target_id != request.target_id
            || state.blocked != request.blocked
            || state.muted != request.muted
            || state.revision > 9_007_199_254_740_991
            || !(state.revision == request.expected_revision
                || state.revision == request.expected_revision + 1)
        {
            return Err(Error::Signature);
        }
        signer.public_key.verify(
            &("babble.private.safety.receipt.v1", &self.payload).canonical_bytes()?,
            &self.signature,
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SafetyEntry {
    pub identity: Identity,
    pub state: SafetyState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SafetySnapshot {
    pub author_id: IdentityId,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
    #[schemars(length(max = 1000))]
    pub entries: Vec<SafetyEntry>,
}
