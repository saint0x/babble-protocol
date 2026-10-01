use crate::{LocalNode, ProvenancePublication};
use babble_graph::Edge;
use babble_judgment::JudgmentProvider;
use babble_object::Object;
use babble_state::{Event, EventKind, EventTarget};
use babble_store::{PublicationBatch, PublicationOutcome, PublicationReceipt, PublicationRequest};
use babble_types::{Error, IdentityId, ObjectId, Result};

impl<P: JudgmentProvider> LocalNode<P> {
    /// One exclusively borrowed publication operation. Authorization and validation
    /// still run before the commit helper either publishes or restores its result.
    pub fn with_publication_request<T>(
        &mut self,
        request: PublicationRequest,
        operation: impl FnOnce(&mut Self) -> Result<T>,
    ) -> Result<T> {
        self.check_ready()?;
        request.validate()?;
        if self.publication_request.is_some() {
            return Err(Error::Conflict("nested publication request".into()));
        }
        self.publication_request = Some(request);
        // Reset scoped metadata even when a trusted native caller catches a panic.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(self)));
        self.publication_request = None;
        match result {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }

    fn retried_outcome(&self, author: &IdentityId) -> Result<Option<PublicationOutcome>> {
        let Some(request) = &self.publication_request else {
            return Ok(None);
        };
        if &request.author != author {
            return Err(Error::Conflict(
                "publication request author mismatch".into(),
            ));
        }
        let Some(receipt) = self.store.publication_receipt(&request.id)? else {
            return Ok(None);
        };
        if receipt.request != *request {
            return Err(Error::Conflict(
                "idempotency key was already used for a different publication".into(),
            ));
        }
        Ok(Some(receipt.outcome))
    }

    pub(crate) fn retried_consent(
        &self,
        author: &IdentityId,
        object: &ObjectId,
        kind: EventKind,
    ) -> Result<Option<Event>> {
        let Some(outcome) = self.retried_outcome(author)? else {
            return Ok(None);
        };
        let event = self
            .event(&outcome.event)
            .ok_or_else(|| Error::Conflict("retry consent event missing; restore store".into()))?;
        if outcome.object.is_some()
            || !outcome.edges.is_empty()
            || &event.actor != author
            || event.target != EventTarget::Object(object.clone())
            || event.kind != kind
            || !matches!(
                kind,
                EventKind::CapabilityGranted | EventKind::CapabilityRevoked
            )
        {
            return Err(Error::Conflict("invalid consent retry receipt".into()));
        }
        Ok(Some(event.clone()))
    }

    pub(crate) fn retried_object(
        &self,
        expected: &Object,
        kind: &EventKind,
        expected_edges: &[Edge],
    ) -> Result<Option<ProvenancePublication>> {
        let author = &expected.author;
        let Some(outcome) = self.retried_outcome(author)? else {
            return Ok(None);
        };
        let id = outcome
            .object
            .ok_or_else(|| Error::Conflict("expected Object retry receipt".into()))?;
        let object = self
            .object(&id)
            .ok_or_else(|| Error::Conflict("retry Object missing; restore store".into()))?
            .clone();
        let event = self
            .event(&outcome.event)
            .ok_or_else(|| Error::Conflict("retry event missing; restore store".into()))?
            .clone();
        let edges = outcome
            .edges
            .iter()
            .map(|id| {
                self.state
                    .graph()
                    .get(id)
                    .cloned()
                    .ok_or_else(|| Error::Conflict("retry edge missing; restore store".into()))
            })
            .collect::<Result<Vec<_>>>()?;
        // A retry constructs fresh signed candidates. Only their generated identity,
        // timestamp, and signature may differ from the originally committed intent.
        let mut candidate = expected.clone();
        candidate.id = object.id.clone();
        candidate.created_at = object.created_at;
        candidate.signature = object.signature.clone();
        if candidate != object
            || &event.actor != author
            || &event.kind != kind
            || event.target != EventTarget::Object(object.id.clone())
            || edges.len() != expected_edges.len()
            || edges.iter().zip(expected_edges).any(|(edge, expected)| {
                let mut candidate = expected.clone();
                candidate.source = object.id.clone();
                !same_edge_intent(edge, &candidate)
            })
        {
            return Err(Error::Conflict("invalid Object retry receipt".into()));
        }
        Ok(Some(ProvenancePublication {
            object,
            edges,
            event,
        }))
    }

    pub(crate) fn retried_edge(&self, expected: &Edge) -> Result<Option<Edge>> {
        let author = expected.author.as_ref().ok_or(Error::UnsignedEdge)?;
        let Some(outcome) = self.retried_outcome(author)? else {
            return Ok(None);
        };
        if outcome.object.is_some() || outcome.edges.len() != 1 {
            return Err(Error::Conflict("expected edge retry receipt".into()));
        }
        let edge = self
            .state
            .graph()
            .get(&outcome.edges[0])
            .ok_or_else(|| Error::Conflict("retry edge missing; restore store".into()))?;
        let event = self
            .event(&outcome.event)
            .ok_or_else(|| Error::Conflict("retry event missing; restore store".into()))?;
        if !same_edge_intent(edge, expected)
            || &event.actor != author
            || event.kind != EventKind::EdgePublished
            || event.target != EventTarget::Edge(edge.id.clone())
        {
            return Err(Error::Conflict("invalid edge retry receipt".into()));
        }
        Ok(Some(edge.clone()))
    }

    pub(crate) fn attach_publication_receipt(
        &self,
        batch: &mut PublicationBatch,
        outcome: PublicationOutcome,
    ) -> Result<()> {
        self.attach_invocation_consumption(batch)?;
        if let Some(request) = &self.publication_request {
            batch.receipt(&PublicationReceipt {
                request: request.clone(),
                outcome,
            })?;
        }
        Ok(())
    }
}

fn same_edge_intent(stored: &Edge, expected: &Edge) -> bool {
    let mut candidate = expected.clone();
    candidate.id = stored.id.clone();
    candidate.created_at = stored.created_at;
    candidate.signature = stored.signature.clone();
    candidate == *stored
}
