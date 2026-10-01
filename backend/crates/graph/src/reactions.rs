//! Public, attributed reaction registers. Axes remain independent; absence is withdrawal.
use babel_crypto::{Keypair, Signature};
use babel_identity::Identity;
use babel_types::{Canonical, Error, Hash, IdentityId, ObjectId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const REACTION_MAX_REVISION: u64 = 9_007_199_254_740_991;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Appreciation {
    Like,
    Dislike,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Engagement {
    Engaging,
    NotEngaging,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    Support,
    Oppose,
    Uncertain,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["appreciation", "engagement", "stance", "certainty"]))]
pub struct ReactionValue {
    #[serde(deserialize_with = "required_nullable")]
    pub appreciation: Option<Appreciation>,
    #[serde(deserialize_with = "required_nullable")]
    pub engagement: Option<Engagement>,
    #[serde(deserialize_with = "required_nullable")]
    pub stance: Option<Stance>,
    #[serde(deserialize_with = "required_nullable")]
    #[schemars(range(max = 100))]
    pub certainty: Option<u8>,
}

fn required_nullable<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::<T>::deserialize(deserializer)
}

impl ReactionValue {
    pub fn validate(&self) -> Result<()> {
        if self.certainty.is_some_and(|value| value > 100)
            || (self.certainty.is_some() && self.stance.is_none())
        {
            return Err(Error::Canonical(
                "certainty must be 0..100 and requires a stance".into(),
            ));
        }
        Ok(())
    }
    pub fn is_empty(&self) -> bool {
        self == &Self::default()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReactionState {
    pub author_id: IdentityId,
    pub object_id: ObjectId,
    pub value: ReactionValue,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub revision: u64,
}

impl ReactionState {
    pub fn absent(author: &IdentityId, object: &ObjectId) -> Self {
        Self {
            author_id: author.clone(),
            object_id: object.clone(),
            value: ReactionValue::default(),
            revision: 0,
        }
    }
    pub fn validate(&self) -> Result<()> {
        self.author_id.validate()?;
        self.object_id.validate()?;
        self.value.validate()?;
        if self.revision > REACTION_MAX_REVISION || (self.revision == 0 && !self.value.is_empty()) {
            return Err(Error::Canonical("invalid reaction revision".into()));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReactionRequest {
    pub author_id: IdentityId,
    pub object_id: ObjectId,
    pub value: ReactionValue,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub expected_revision: u64,
    pub idempotency_key: String,
}

impl ReactionRequest {
    pub fn validate(&self) -> Result<()> {
        self.author_id.validate()?;
        self.object_id.validate()?;
        self.value.validate()?;
        if self.expected_revision > REACTION_MAX_REVISION {
            return Err(Error::Canonical("reaction revision out of range".into()));
        }
        if self.idempotency_key.is_empty()
            || self.idempotency_key.len() > 256
            || !self.idempotency_key.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(Error::Canonical(
                "reaction idempotency key must be 1..256 visible ASCII bytes".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReactionSummary {
    pub object_id: ObjectId,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub participants: u64,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub likes: u64,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub dislikes: u64,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub engaging: u64,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub not_engaging: u64,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub support: u64,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub oppose: u64,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub uncertain: u64,
    #[schemars(range(max = 9_007_199_254_740_991_u64))]
    pub certainty_responses: u64,
}

impl ReactionSummary {
    pub fn validate(&self) -> Result<()> {
        self.object_id.validate()?;
        let axes = [
            self.likes,
            self.dislikes,
            self.engaging,
            self.not_engaging,
            self.support,
            self.oppose,
            self.uncertain,
            self.certainty_responses,
        ];
        if self.participants > REACTION_MAX_REVISION || axes.iter().any(|n| *n > self.participants)
        {
            return Err(Error::Canonical(
                "reaction summary counts out of range".into(),
            ));
        }
        // Bounds above make these sums overflow-safe. Independent axes may overlap;
        // alternatives within a single axis must not, and every participant has an axis.
        let appreciation = self.likes + self.dislikes;
        let engagement = self.engaging + self.not_engaging;
        let stance = self.support + self.oppose + self.uncertain;
        if appreciation > self.participants
            || engagement > self.participants
            || stance > self.participants
            || self.certainty_responses > stance
            || self.participants > appreciation + engagement + stance
        {
            return Err(Error::Canonical(
                "inconsistent reaction summary axes".into(),
            ));
        }
        Ok(())
    }
    pub fn empty(object: &ObjectId) -> Self {
        Self {
            object_id: object.clone(),
            participants: 0,
            likes: 0,
            dislikes: 0,
            engaging: 0,
            not_engaging: 0,
            support: 0,
            oppose: 0,
            uncertain: 0,
            certainty_responses: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["state", "action"]))]
pub struct ReactionRecord {
    pub state: ReactionState,
    pub action: Option<ReactionAction>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["state", "previous_id", "created_at", "request_id"]))]
pub struct ReactionActionPayload {
    pub state: ReactionState,
    pub previous_id: Option<Hash>,
    pub created_at: Timestamp,
    pub request_id: Hash,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReactionAction {
    pub id: Hash,
    pub payload: ReactionActionPayload,
    pub signature: Signature,
}

impl ReactionAction {
    pub fn sign(payload: ReactionActionPayload, key: &Keypair) -> Result<Self> {
        payload.state.validate()?;
        let bytes = ("babel.public.reaction.action.v1", &payload).canonical_bytes()?;
        Ok(Self {
            id: Hash::from_bytes(&bytes),
            payload,
            signature: key.sign(&bytes),
        })
    }
    pub fn verify(&self, signer: &Identity) -> Result<()> {
        let state = &self.payload.state;
        state.validate()?;
        self.payload.request_id.validate()?;
        if let Some(id) = &self.payload.previous_id {
            id.validate()?;
        }
        if signer.id != state.author_id
            || state.revision == 0
            || (state.revision == 1) != self.payload.previous_id.is_none()
        {
            return Err(Error::Signature);
        }
        let bytes = ("babel.public.reaction.action.v1", &self.payload).canonical_bytes()?;
        if self.id != Hash::from_bytes(&bytes) {
            return Err(Error::Signature);
        }
        signer.public_key.verify(&bytes, &self.signature)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["request", "state", "created_at", "sequence", "previous_id"]))]
pub struct ReactionReceiptPayload {
    pub request: ReactionRequest,
    pub state: ReactionState,
    pub created_at: Timestamp,
    pub sequence: u64,
    pub previous_id: Option<Hash>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReactionReceipt {
    pub payload: ReactionReceiptPayload,
    pub signature: Signature,
}

impl ReactionReceipt {
    pub fn id(&self) -> Result<Hash> {
        ("babel.public.reaction.receipt.v1", &self.payload).canonical_hash()
    }
    pub fn sign(payload: ReactionReceiptPayload, key: &Keypair) -> Result<Self> {
        let signature =
            key.sign(&("babel.public.reaction.receipt.v1", &payload).canonical_bytes()?);
        Ok(Self { payload, signature })
    }
    pub fn verify(&self, signer: &Identity) -> Result<()> {
        let request = &self.payload.request;
        let state = &self.payload.state;
        request.validate()?;
        state.validate()?;
        if let Some(id) = &self.payload.previous_id {
            id.validate()?;
        }
        if signer.id != request.author_id
            || self.payload.sequence == 0
            || self.payload.sequence > REACTION_MAX_REVISION
            || (self.payload.sequence == 1) != self.payload.previous_id.is_none()
            || state.author_id != request.author_id
            || state.object_id != request.object_id
            || state.value != request.value
            || !(state.revision == request.expected_revision
                || state.revision == request.expected_revision + 1)
        {
            return Err(Error::Signature);
        }
        signer.public_key.verify(
            &("babel.public.reaction.receipt.v1", &self.payload).canonical_bytes()?,
            &self.signature,
        )
    }
}
