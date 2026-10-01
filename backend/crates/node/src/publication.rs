//! Publication preparation has no live index/cache writes. Only the bounded
//! store batch crosses the durable boundary; providers run before that boundary.
use crate::{LocalNode, ProvenancePublication};
use babel_authoring::ObjectDraft;
use babel_crypto::Keypair;
use babel_graph::{Edge, EdgeOrigin, Relation};
use babel_judgment::JudgmentProvider;
use babel_media::MediaObjectPayload;
use babel_object::Object;
use babel_state::{Event, EventKind, EventTarget};
use babel_store::{PublicationBatch, PublicationOutcome};
use babel_types::{Error, IdentityId, ObjectId, Result};

impl<P: JudgmentProvider> LocalNode<P> {
    /// API dispatchers must call this before using infallible snapshot getters.
    /// A recovery-required node must be reopened, never transparently retried.
    pub fn check_ready(&self) -> Result<()> {
        self.store.check_ready()
    }

    pub(crate) fn index_object(&mut self, object: &Object) {
        self.discovery_index.insert(object);
        self.index_object_replies(object);
        self.index_object_quotes(object);
        self.index_profile_object(object);
    }

    pub(crate) fn commit_new_edge(&mut self, edge: Edge, keypair: &Keypair) -> Result<Edge> {
        let author_id = edge.author.as_ref().ok_or(Error::UnsignedEdge)?;
        if let Some(retried) = self.retried_edge(&edge)? {
            return Ok(retried);
        }
        self.check_edge_safety(author_id, &edge, None)?;
        let author = self.state.signing_identity_at(author_id, edge.created_at)?;
        if self.state.graph().get(&edge.id).is_some() || self.store.get_edge(&edge.id)?.is_some() {
            return Err(Error::Conflict(format!("edge already exists: {}", edge.id)));
        }
        let mut batch = PublicationBatch::new();
        batch.edge(&edge, &author)?;
        let event = Event::new(&author, EventKind::EdgePublished, EventTarget::Edge(edge.id.clone()),
            serde_json::json!({"source": edge.source, "target": edge.target, "relation": edge.relation}),
            self.latest_event_ids(2)?)?.sign(&author, keypair)?;
        self.validate_publication_event(&event, &mut batch)?;
        self.attach_publication_receipt(
            &mut batch,
            PublicationOutcome {
                object: None,
                edges: vec![edge.id.clone()],
                event: event.id.clone(),
            },
        )?;
        self.store
            .commit_publication(batch)
            .map_err(publication_error)?;
        self.state
            .apply_edge(edge.clone())
            .map_err(committed_state_error)?;
        self.state
            .apply_event(event)
            .map_err(committed_state_error)?;
        self.index_reply_edge(&edge);
        self.index_quote_edge(&edge);
        Ok(edge)
    }

    pub(crate) fn publish_related_draft(
        &mut self,
        author_id: &IdentityId,
        draft: ObjectDraft,
        kind: EventKind,
        relations: Vec<(ObjectId, Relation, EdgeOrigin)>,
    ) -> Result<ProvenancePublication> {
        draft.validate()?;
        for hash in draft.required_blob_hashes() {
            if !self.store.contains_blob(&hash)? {
                return Err(Error::NotFound(format!("media blob {hash}")));
            }
        }
        let author = self.local_identity(author_id)?;
        let keypair = self.local_keypair(author_id)?.clone();
        let object = draft.build_unsigned(author)?.sign(author, &keypair)?;
        let edges = relations
            .into_iter()
            .map(|(target, relation, origin)| {
                self.require_object(&target)?;
                Edge::new(
                    object.id.clone(),
                    target,
                    relation,
                    origin,
                    Some(author_id.clone()),
                )?
                .sign(author, &keypair)
            })
            .collect::<Result<Vec<_>>>()?;
        self.commit_new_object(object, kind, edges, &keypair)
    }

    pub(crate) fn commit_new_object(
        &mut self,
        object: Object,
        kind: EventKind,
        edges: Vec<Edge>,
        keypair: &Keypair,
    ) -> Result<ProvenancePublication> {
        if let Some(retried) = self.retried_object(&object, &kind, &edges)? {
            return Ok(retried);
        }
        self.check_object_safety(&object, &edges)?;
        self.validate_media_publication(&object)?;
        if !matches!(
            kind,
            EventKind::ObjectPublished | EventKind::ObjectForked | EventKind::ObjectRemixed
        ) {
            return Err(Error::Conflict(
                "invalid object publication event kind".into(),
            ));
        }
        if self.object(&object.id).is_some() || self.store.get_object(&object.id)?.is_some() {
            return Err(Error::Conflict(format!(
                "object already exists: {}",
                object.id
            )));
        }
        let author = self
            .state
            .signing_identity_at(&object.author, object.created_at)?;
        let mut batch = PublicationBatch::new();
        batch.object(&object, &author)?;
        for edge in &object.relations {
            if self
                .state
                .graph()
                .get(&edge.id)
                .is_some_and(|existing| existing != edge)
            {
                return Err(Error::Conflict(format!(
                    "embedded edge conflict: {}",
                    edge.id
                )));
            }
        }
        // Keep event payloads/signatures compatible with MemoryState publication.
        let event = Event::new(&author, kind, EventTarget::Object(object.id.clone()),
            serde_json::json!({"kind": object.kind.as_str(), "schema": object.schema, "provenance": object.provenance}),
            self.latest_event_ids(2)?)?.sign(&author, keypair)?;
        self.validate_publication_event(&event, &mut batch)?;
        let mut events = vec![event.clone()];
        for edge in &edges {
            let author_id = edge.author.as_ref().ok_or(Error::UnsignedEdge)?;
            let signer = self.state.signing_identity_at(author_id, edge.created_at)?;
            if self.state.graph().get(&edge.id).is_some()
                || object
                    .relations
                    .iter()
                    .any(|embedded| embedded.id == edge.id)
            {
                return Err(Error::Conflict(format!("edge already exists: {}", edge.id)));
            }
            batch.edge(edge, &signer)?;
            let parents = events
                .iter()
                .rev()
                .take(2)
                .map(|e| e.id.clone())
                .chain(event.parents.iter().cloned())
                .take(2)
                .collect();
            let edge_event = Event::new(&signer, EventKind::EdgePublished, EventTarget::Edge(edge.id.clone()),
                serde_json::json!({"source": edge.source, "target": edge.target, "relation": edge.relation}),
                parents)?.sign(&signer, keypair)?;
            self.validate_publication_event(&edge_event, &mut batch)?;
            events.push(edge_event);
        }
        let prepared = self.prepare_object_judgments(&object)?;
        for entry in &prepared {
            if let Some(input) = &entry.input {
                batch.object_judgment(input, &entry.orchestration.judgment)?;
            } else {
                batch.judgment(&entry.orchestration.judgment)?;
            }
        }
        self.attach_publication_receipt(
            &mut batch,
            PublicationOutcome {
                object: Some(object.id.clone()),
                edges: edges.iter().map(|edge| edge.id.clone()).collect(),
                event: event.id.clone(),
            },
        )?;
        self.store
            .commit_publication(batch)
            .map_err(publication_error)?;

        // All apply preconditions were checked against this exclusively borrowed
        // node. No provider or disk operations remain after the durable commit.
        self.state
            .apply_object(object.clone())
            .map_err(committed_state_error)?;
        for edge in &edges {
            self.state
                .apply_edge(edge.clone())
                .map_err(committed_state_error)?;
        }
        for event in events {
            self.state
                .apply_event(event)
                .map_err(committed_state_error)?;
        }
        self.index_object(&object);
        self.cache_object_judgments(prepared);
        Ok(ProvenancePublication {
            object,
            edges,
            event,
        })
    }

    fn validate_media_publication(&self, object: &Object) -> Result<()> {
        if object.kind.as_str() != "babel.media" {
            return Ok(());
        }
        let payload: MediaObjectPayload = serde_json::from_value(object.payload.clone())
            .map_err(|error| Error::Canonical(error.to_string()))?;
        payload.validate_object_resources(&object.resources)?;
        for resource in &payload.resources {
            let max_bytes = usize::try_from(resource.size_bytes)
                .map_err(|_| Error::Conflict("media resource size is not addressable".into()))?;
            let bytes = self
                .store
                .get_blob_bounded(&resource.integrity, max_bytes)
                .map_err(|error| Error::Conflict(error.to_string()))?
                .ok_or_else(|| Error::NotFound(format!("media blob {}", resource.integrity)))?;
            if bytes.len() as u64 != resource.size_bytes {
                return Err(Error::Conflict(format!(
                    "media blob size does not match metadata: {}",
                    resource.integrity
                )));
            }
        }
        Ok(())
    }

    fn validate_publication_event(
        &self,
        event: &Event,
        batch: &mut PublicationBatch,
    ) -> Result<()> {
        let signer = self
            .state
            .signing_identity_at(&event.actor, event.created_at)?;
        if self.state.event(&event.id).is_some() {
            return Err(Error::Conflict(format!(
                "event already exists: {}",
                event.id
            )));
        }
        batch.event(event, &signer)
    }
}

pub(crate) fn publication_error(error: babel_store::PublicationError) -> Error {
    Error::Conflict(error.to_string())
}

fn committed_state_error(error: Error) -> Error {
    Error::Conflict(format!(
        "publication committed; in-memory apply failed, reopen node: {error}"
    ))
}
