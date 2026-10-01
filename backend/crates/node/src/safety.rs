//! Private account safety. Enforcement belongs to local writes, never public imports or reads.
use crate::LocalNode;
use babble_graph::{
    Edge, EdgeOrigin, Relation, SafetyAction, SafetyActionPayload, SafetyReceipt,
    SafetyReceiptPayload, SafetyRequest,
};
pub use babble_graph::{SafetyEntry, SafetySnapshot, SafetyState};
use babble_identity::Identity;
use babble_judgment::JudgmentProvider;
use babble_object::Object;
use babble_types::{Canonical, Error, IdentityId, ObjectId, Result, Timestamp};

impl<P: JudgmentProvider> LocalNode<P> {
    pub(crate) fn verify_safety(&self) -> Result<()> {
        self.store
            .verify_safety_records(|id, at| self.state.signing_identity_at(id, at))
    }

    fn safety_identity(&self, id: &IdentityId) -> Result<&Identity> {
        id.validate()?;
        self.identity(id)
            .ok_or_else(|| Error::NotFound("identity".into()))
    }

    pub fn safety_state(&self, author: &IdentityId, target: &IdentityId) -> Result<SafetyState> {
        self.check_ready()?;
        self.safety_identity(author)?;
        self.safety_identity(target)?;
        if author == target {
            return Err(Error::Canonical("cannot mute or block yourself".into()));
        }
        self.store.safety_state(author, target)
    }

    pub fn safety_snapshot(&self, author: &IdentityId) -> Result<SafetySnapshot> {
        self.check_ready()?;
        self.safety_identity(author)?;
        let (revision, states) = self.store.safety_snapshot(author)?;
        let entries = states
            .into_iter()
            .map(|state| {
                Ok(SafetyEntry {
                    identity: self.safety_identity(&state.target_id)?.clone(),
                    state,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(SafetySnapshot {
            author_id: author.clone(),
            revision,
            entries,
        })
    }

    pub fn set_safety(
        &mut self,
        author: &IdentityId,
        target: &IdentityId,
        blocked: bool,
        muted: bool,
        expected_revision: u64,
        idempotency_key: &str,
    ) -> Result<SafetyState> {
        self.check_ready()?;
        self.safety_identity(author)?;
        self.safety_identity(target)?;
        let request = SafetyRequest {
            author_id: author.clone(),
            target_id: target.clone(),
            blocked,
            muted,
            expected_revision,
            idempotency_key: idempotency_key.into(),
        };
        request.validate()?;
        let key = self.local_keypair(author)?;
        if key.public_key() != self.state.signing_identity(author)?.public_key {
            return Err(Error::Signature);
        }
        self.store.commit_safety(
            &request,
            |id, at| self.state.signing_identity_at(id, at),
            |state, previous_id, changed, sequence, receipt_previous_id| {
                let created_at = Timestamp::now();
                let signer = self.state.signing_identity_at(author, created_at)?;
                if key.public_key() != signer.public_key {
                    return Err(Error::Signature);
                }
                let action = changed
                    .then(|| {
                        SafetyAction::sign(
                            SafetyActionPayload {
                                state: state.clone(),
                                previous_id,
                                created_at,
                                request_id: request.canonical_hash()?,
                            },
                            key,
                        )
                    })
                    .transpose()?;
                let receipt = SafetyReceipt::sign(
                    SafetyReceiptPayload {
                        request: request.clone(),
                        state: state.clone(),
                        created_at,
                        sequence,
                        previous_id: receipt_previous_id,
                    },
                    key,
                )?;
                Ok((action, receipt))
            },
        )
    }

    pub(crate) fn require_interaction(
        &self,
        author: &IdentityId,
        target: &IdentityId,
    ) -> Result<()> {
        if author != target && self.store.safety_blocked(author, target)? {
            // Do not reveal who blocked whom to the interacting account.
            return Err(Error::Conflict("interaction unavailable".into()));
        }
        Ok(())
    }

    pub(crate) fn require_object_interaction(
        &self,
        author: &IdentityId,
        object: &ObjectId,
    ) -> Result<()> {
        self.require_interaction(author, &self.require_object(object)?.author)
    }

    pub(crate) fn check_edge_safety(
        &self,
        actor: &IdentityId,
        edge: &Edge,
        pending: Option<&Object>,
    ) -> Result<()> {
        if self.is_follow_withdrawal(actor, edge) {
            return Ok(());
        }
        for endpoint in [&edge.source, &edge.target] {
            let owner = match pending.filter(|o| &o.id == endpoint) {
                Some(object) => &object.author,
                None => &self.require_object(endpoint)?.author,
            };
            self.require_interaction(actor, owner)?;
            if let Some(signer) = &edge.author {
                self.require_interaction(signer, owner)?;
            }
        }
        Ok(())
    }

    fn is_follow_withdrawal(&self, actor: &IdentityId, edge: &Edge) -> bool {
        // A label alone cannot authorize a new relationship under a block.
        // A Surface controller can belong to an application. Its actor may still
        // withdraw their own earlier follow through the approved one-use invocation.
        let authorized_source = self
            .object(&edge.source)
            .is_some_and(|source| &source.author == actor)
            || self.executing_invocation.as_ref().is_some_and(|record| {
                let intent = record.intent();
                intent.capability.as_str() == "babble.social.unfollow"
                    && &intent.context.actor == actor
                    && intent.context.object_id == edge.source
                    && intent
                        .payload
                        .get("target_object_id")
                        .and_then(serde_json::Value::as_str)
                        == Some(edge.target.as_str())
            });
        matches!(&edge.relation, Relation::Custom(name) if name == "unfollows")
            && edge.author.as_ref() == Some(actor)
            && edge.origin == EdgeOrigin::HumanAssertion
            && edge.metadata.len() == 1
            && edge
                .metadata
                .get("removes_relation")
                .and_then(serde_json::Value::as_str)
                == Some("follows")
            && authorized_source
            && self
                .outgoing_relation(&edge.source, &Relation::Follows)
                .iter()
                .any(|previous| {
                    previous.target == edge.target
                        && previous.author.as_ref() == Some(actor)
                        && previous.signature.is_some()
                })
    }

    pub(crate) fn check_object_safety(&self, object: &Object, edges: &[Edge]) -> Result<()> {
        for target in object
            .provenance
            .parent
            .iter()
            .chain(object.provenance.forked_from.iter())
            .chain(object.provenance.remixed_from.iter())
        {
            self.require_object_interaction(&object.author, target)?;
        }
        for edge in object.relations.iter().chain(edges) {
            self.check_edge_safety(&object.author, edge, Some(object))?;
        }
        Ok(())
    }
}
