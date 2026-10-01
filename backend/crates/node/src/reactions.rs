//! Authenticated node mutations and public readback of signed reaction registers.
use crate::LocalNode;
use babble_graph::{
    ReactionAction, ReactionActionPayload, ReactionReceipt, ReactionReceiptPayload, ReactionRecord,
    ReactionRequest, ReactionState, ReactionSummary, ReactionValue,
};
use babble_judgment::JudgmentProvider;
use babble_types::{Canonical, Error, IdentityId, ObjectId, Result, Timestamp};

impl<P: JudgmentProvider> LocalNode<P> {
    pub(crate) fn verify_reactions(&self) -> Result<()> {
        self.store.verify_reaction_records(
            |id, at| self.state.signing_identity_at(id, at),
            |id, at| self.require_reaction_object_at(id, at),
        )
    }

    fn require_reaction_object_at(&self, object: &ObjectId, at: Timestamp) -> Result<()> {
        object.validate()?;
        let record = self
            .object(object)
            .ok_or_else(|| Error::NotFound("Object".into()))?;
        if record.created_at > at {
            return Err(Error::Canonical("reaction precedes its Object".into()));
        }
        Ok(())
    }

    fn require_reaction_pair(&self, author: &IdentityId, object: &ObjectId) -> Result<()> {
        self.check_ready()?;
        author.validate()?;
        object.validate()?;
        self.identity(author)
            .ok_or_else(|| Error::NotFound("identity".into()))?;
        self.object(object)
            .ok_or_else(|| Error::NotFound("Object".into()))?;
        Ok(())
    }

    pub fn reaction_state(&self, author: &IdentityId, object: &ObjectId) -> Result<ReactionState> {
        self.require_reaction_pair(author, object)?;
        self.store.reaction_state(author, object)
    }

    pub fn reaction_record(
        &self,
        author: &IdentityId,
        object: &ObjectId,
    ) -> Result<ReactionRecord> {
        self.require_reaction_pair(author, object)?;
        self.store.reaction_record(author, object)
    }

    pub fn reaction_summary(&self, object: &ObjectId) -> Result<ReactionSummary> {
        self.check_ready()?;
        object.validate()?;
        self.object(object)
            .ok_or_else(|| Error::NotFound("Object".into()))?;
        self.store.reaction_summary(object)
    }

    pub fn set_reaction(
        &mut self,
        author: &IdentityId,
        object: &ObjectId,
        value: ReactionValue,
        expected_revision: u64,
        idempotency_key: &str,
    ) -> Result<ReactionState> {
        self.require_reaction_pair(author, object)?;
        let request = ReactionRequest {
            author_id: author.clone(),
            object_id: object.clone(),
            value,
            expected_revision,
            idempotency_key: idempotency_key.into(),
        };
        request.validate()?;
        self.commit_reaction_request(&request, Timestamp::now)
    }

    fn commit_reaction_request(
        &self,
        request: &ReactionRequest,
        now: impl FnOnce() -> Timestamp,
    ) -> Result<ReactionState> {
        let author = &request.author_id;
        let key = self.local_keypair(author)?;
        let before = self.store.reaction_state(author, &request.object_id)?.value;
        let after = &request.value;
        let withdrawal = (after.appreciation.is_none() || after.appreciation == before.appreciation)
            && (after.engagement.is_none() || after.engagement == before.engagement)
            && (after.stance.is_none() || after.stance == before.stance)
            && (after.certainty.is_none() || after.certainty == before.certainty);
        let allowed = if withdrawal { Ok(()) } else {
            self.require_object_interaction(author, &request.object_id)
        };
        // Receipt replay is still an authenticated mutation call; expired/stale local
        // credentials may read public state but cannot exercise the signing interface.
        if key.public_key() != self.state.signing_identity(author)?.public_key {
            return Err(Error::Signature);
        }
        self.store.commit_reaction(
            request,
            |id, at| self.state.signing_identity_at(id, at),
            |state, previous_id, changed, sequence, receipt_previous_id| {
                allowed?;
                let created_at = now();
                let signer = self.state.signing_identity_at(author, created_at)?;
                self.require_reaction_object_at(&request.object_id, created_at)?;
                if key.public_key() != signer.public_key {
                    return Err(Error::Signature);
                }
                let action = changed
                    .then(|| {
                        ReactionAction::sign(
                            ReactionActionPayload {
                                state: state.clone(),
                                previous_id,
                                created_at,
                                request_id: request.canonical_hash()?,
                            },
                            key,
                        )
                    })
                    .transpose()?;
                let receipt = ReactionReceipt::sign(
                    ReactionReceiptPayload {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use babble_identity::{IdentityKeyScope, IdentityKind};
    use babble_judgment_local::LocalProvider;

    #[test]
    fn reactions_transaction_timestamp_checks_expiry_and_no_write_on_failure() {
        let root = std::env::temp_dir().join(format!(
            "babble-reaction-expiry-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let author = node
            .create_identity(IdentityKind::Person, "author")
            .unwrap();
        let object = node.publish_text(&author.id, "post").unwrap();
        let expires = Timestamp(Timestamp::now().0 + std::time::Duration::from_secs(60));
        node.rotate_identity_key(
            &author.id,
            IdentityKeyScope::Session,
            Some(expires),
            "temporary",
        )
        .unwrap();
        let request = ReactionRequest {
            author_id: author.id.clone(),
            object_id: object.id.clone(),
            value: ReactionValue {
                appreciation: Some(babble_graph::Appreciation::Like),
                ..Default::default()
            },
            expected_revision: 0,
            idempotency_key: "expiry-boundary".into(),
        };
        assert!(matches!(
            node.commit_reaction_request(&request, || expires),
            Err(Error::Signature)
        ));
        assert_eq!(
            node.reaction_state(&author.id, &object.id)
                .unwrap()
                .revision,
            0
        );
        assert_eq!(node.reaction_summary(&object.id).unwrap().participants, 0);
        node.verify_reactions().unwrap();
        node.set_reaction(
            &author.id,
            &object.id,
            request.value.clone(),
            0,
            "expiry-boundary",
        )
        .unwrap();
        let before = node.reaction_record(&author.id, &object.id).unwrap();
        let now = before.action.as_ref().unwrap().payload.created_at;
        let backward = Timestamp(now.0 - std::time::Duration::from_secs(1));
        let withdraw = ReactionRequest {
            expected_revision: 1,
            value: Default::default(),
            idempotency_key: "withdraw".into(),
            ..request
        };
        assert!(
            node.commit_reaction_request(&withdraw, || backward)
                .is_err()
        );
        assert_eq!(
            node.reaction_record(&author.id, &object.id).unwrap(),
            before
        );
        node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        assert_eq!(
            node.reaction_state(&author.id, &object.id)
                .unwrap()
                .revision,
            1
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
