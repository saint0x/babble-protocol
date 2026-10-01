use babel_crypto::{Keypair, PublicKey, Signature};
use babel_types::{Canonical, IdentityId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum IdentityKind {
    Person,
    Pseudonym,
    Organization,
    Service,
    Application,
    Agent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Identity {
    pub id: IdentityId,
    pub kind: IdentityKind,
    pub handle: String,
    pub public_key: PublicKey,
    pub created_at: Timestamp,
    pub signature: Signature,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum IdentityKeyScope {
    Root,
    Device,
    Session,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct IdentityKeyTransition {
    pub identity_id: IdentityId,
    pub sequence: u64,
    pub scope: IdentityKeyScope,
    pub previous_public_key: PublicKey,
    pub next_public_key: PublicKey,
    pub effective_at: Timestamp,
    pub expires_at: Option<Timestamp>,
    pub reason: String,
    pub previous_signature: Signature,
    pub next_signature: Signature,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
struct IdentityKeyTransitionCommitment {
    pub identity_id: IdentityId,
    pub sequence: u64,
    pub scope: IdentityKeyScope,
    pub previous_public_key: PublicKey,
    pub next_public_key: PublicKey,
    pub effective_at: Timestamp,
    pub expires_at: Option<Timestamp>,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
struct IdentityCommitment {
    pub kind: IdentityKind,
    pub handle: String,
    pub public_key: PublicKey,
    pub created_at: Timestamp,
}

impl Identity {
    pub fn create(
        kind: IdentityKind,
        handle: impl Into<String>,
        keypair: &Keypair,
    ) -> Result<Self> {
        let commitment = IdentityCommitment {
            kind,
            handle: handle.into(),
            public_key: keypair.public_key(),
            created_at: Timestamp::now(),
        };
        let hash = commitment.canonical_hash()?;
        let signature = keypair.sign(&commitment.canonical_bytes()?);
        Ok(Self {
            id: IdentityId::from_hash(&hash),
            kind: commitment.kind,
            handle: commitment.handle,
            public_key: commitment.public_key,
            created_at: commitment.created_at,
            signature,
        })
    }

    pub fn verify(&self) -> Result<()> {
        self.id.validate()?;
        let commitment = IdentityCommitment {
            kind: self.kind.clone(),
            handle: self.handle.clone(),
            public_key: self.public_key.clone(),
            created_at: self.created_at,
        };
        let expected_id = IdentityId::from_hash(&commitment.canonical_hash()?);
        if expected_id != self.id {
            return Err(babel_types::Error::Signature);
        }
        self.public_key
            .verify(&commitment.canonical_bytes()?, &self.signature)
    }

    pub fn with_signing_key(&self, public_key: PublicKey) -> Self {
        let mut identity = self.clone();
        identity.public_key = public_key;
        identity
    }
}

impl IdentityKeyTransition {
    pub fn create(
        identity_id: IdentityId,
        sequence: u64,
        scope: IdentityKeyScope,
        previous_keypair: &Keypair,
        next_keypair: &Keypair,
        expires_at: Option<Timestamp>,
        reason: impl Into<String>,
    ) -> Result<Self> {
        let commitment = IdentityKeyTransitionCommitment {
            identity_id,
            sequence,
            scope,
            previous_public_key: previous_keypair.public_key(),
            next_public_key: next_keypair.public_key(),
            effective_at: Timestamp::now(),
            expires_at,
            reason: reason.into(),
        };
        let bytes = commitment.canonical_bytes()?;
        Ok(Self {
            identity_id: commitment.identity_id,
            sequence: commitment.sequence,
            scope: commitment.scope,
            previous_public_key: commitment.previous_public_key,
            next_public_key: commitment.next_public_key,
            effective_at: commitment.effective_at,
            expires_at: commitment.expires_at,
            reason: commitment.reason,
            previous_signature: previous_keypair.sign(&bytes),
            next_signature: next_keypair.sign(&bytes),
        })
    }

    pub fn verify(&self, expected_previous_key: &PublicKey) -> Result<()> {
        self.identity_id.validate()?;
        if &self.previous_public_key != expected_previous_key {
            return Err(babel_types::Error::Signature);
        }
        if self
            .expires_at
            .is_some_and(|expires_at| expires_at <= self.effective_at)
        {
            return Err(babel_types::Error::Conflict(
                "identity key transition expires before it becomes effective".to_string(),
            ));
        }
        let bytes = self.commitment().canonical_bytes()?;
        self.previous_public_key
            .verify(&bytes, &self.previous_signature)?;
        self.next_public_key.verify(&bytes, &self.next_signature)
    }

    fn commitment(&self) -> IdentityKeyTransitionCommitment {
        IdentityKeyTransitionCommitment {
            identity_id: self.identity_id.clone(),
            sequence: self.sequence,
            scope: self.scope.clone(),
            previous_public_key: self.previous_public_key.clone(),
            next_public_key: self.next_public_key.clone(),
            effective_at: self.effective_at,
            expires_at: self.expires_at,
            reason: self.reason.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_transition_requires_previous_and_next_key_signatures() {
        let root_keypair = Keypair::generate();
        let next_keypair = Keypair::generate();
        let identity = Identity::create(IdentityKind::Person, "alice", &root_keypair).unwrap();
        let transition = IdentityKeyTransition::create(
            identity.id,
            1,
            IdentityKeyScope::Device,
            &root_keypair,
            &next_keypair,
            None,
            "new workstation",
        )
        .unwrap();

        transition.verify(&root_keypair.public_key()).unwrap();

        let wrong_keypair = Keypair::generate();
        assert!(transition.verify(&wrong_keypair.public_key()).is_err());
    }
}
