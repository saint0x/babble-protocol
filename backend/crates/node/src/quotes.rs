use crate::LocalNode;
use babble_graph::{Edge, Relation};
use babble_judgment::JudgmentProvider;
use babble_object::Object;
use babble_types::{EdgeId, Error, IdentityId, ObjectId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, ops::Bound};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["object_id", "cursor", "limit"]))]
pub struct QuotesListQuery {
    pub object_id: ObjectId,
    #[serde(deserialize_with = "Option::deserialize")]
    pub cursor: Option<String>,
    #[schemars(range(min = 1, max = 20))]
    pub limit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(extend("required" = ["edge", "object"]))]
pub struct QuotedObject {
    pub edge: Edge,
    pub object: Option<Object>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(extend("required" = ["object_id", "quotes", "next_cursor"]))]
pub struct QuotesListResult {
    pub object_id: ObjectId,
    pub quotes: Vec<QuotedObject>,
    pub next_cursor: Option<String>,
}

type QuotePosition = (Timestamp, EdgeId);

#[derive(Default)]
pub(crate) struct QuotesIndex {
    sources: BTreeMap<ObjectId, SourceQuotes>,
}

#[derive(Default)]
struct SourceQuotes {
    targets: BTreeMap<ObjectId, QuotePosition>,
    ordered: BTreeMap<QuotePosition, Edge>,
}

impl<P: JudgmentProvider> LocalNode<P> {
    pub fn list_quotes(&self, query: &QuotesListQuery) -> Result<QuotesListResult> {
        self.check_ready()?;
        validate_object_id(&query.object_id)?;
        if !(1..=20).contains(&query.limit) {
            return Err(Error::Canonical(
                "quote list limit must be between 1 and 20".into(),
            ));
        }
        self.require_object(&query.object_id)?;
        let after = query
            .cursor
            .as_deref()
            .map(|cursor| self.quote_cursor(&query.object_id, cursor))
            .transpose()?;
        let start = after.as_ref().map_or(Bound::Unbounded, Bound::Excluded);
        let (_, restricted) = self.store.moderation_restrictions()?;
        // Verification and target deduplication happen on ingestion, not on every read.
        let mut quotes = self
            .quotes
            .sources
            .get(&query.object_id)
            .into_iter()
            .flat_map(|source| source.ordered.range((start, Bound::Unbounded)))
            .take(query.limit + 1)
            .map(|(_, edge)| QuotedObject {
                edge: edge.clone(),
                object: self
                    .object(&edge.target)
                    .filter(|object| !restricted.contains(&object.id))
                    .cloned(),
            })
            .collect::<Vec<_>>();
        let has_more = quotes.len() > query.limit;
        quotes.truncate(query.limit);
        let next_cursor = if has_more {
            quotes
                .last()
                .map(|quote| format!("v1|{}|{}", query.object_id, quote.edge.id))
        } else {
            None
        };
        Ok(QuotesListResult {
            object_id: query.object_id.clone(),
            quotes,
            next_cursor,
        })
    }

    fn quote_cursor(&self, source: &ObjectId, cursor: &str) -> Result<QuotePosition> {
        let invalid = || Error::Canonical("invalid quote cursor for this object".into());
        if cursor.len() != 141 {
            return Err(invalid());
        }
        let mut parts = cursor.split('|');
        if parts.next() != Some("v1") || parts.next() != Some(source.as_str()) {
            return Err(invalid());
        }
        let id = EdgeId::new_unchecked(parts.next().ok_or_else(invalid)?);
        id.validate().map_err(|_| invalid())?;
        if parts.next().is_some() || !lower_hex(&id.as_str()[5..]) {
            return Err(invalid());
        }
        let edge = self.edge(&id).ok_or_else(invalid)?;
        if &edge.source != source || !self.is_authoritative_quote(edge) {
            return Err(invalid());
        }
        // An older valid bookmark remains usable if an earlier duplicate is imported later.
        Ok((edge.created_at, edge.id.clone()))
    }

    fn is_authoritative_quote(&self, edge: &Edge) -> bool {
        if edge.relation != Relation::Quotes || validate_object_id(&edge.target).is_err() {
            return false;
        }
        let Some(source) = self.object(&edge.source) else {
            return false;
        };
        if edge.author.as_ref() != Some(&source.author) {
            return false;
        }
        self.state
            .signing_identity_at(&source.author, edge.created_at)
            .is_ok_and(|author| edge.verify(&author).is_ok())
    }

    pub(crate) fn index_quote_edge(&mut self, edge: &Edge) {
        if edge.relation != Relation::Quotes || self.object(&edge.source).is_none() {
            return;
        }
        // Retain candidate sources even before their historical signing keys arrive.
        self.quotes.sources.entry(edge.source.clone()).or_default();
        if !self.is_authoritative_quote(edge) {
            return;
        }
        let source = self
            .quotes
            .sources
            .get_mut(&edge.source)
            .expect("source inserted");
        let position = (edge.created_at, edge.id.clone());
        if let Some(existing) = source.targets.get(&edge.target) {
            if existing <= &position {
                return;
            }
            source.ordered.remove(existing);
        }
        source.targets.insert(edge.target.clone(), position.clone());
        source.ordered.insert(position, edge.clone());
    }

    pub(crate) fn index_object_quotes(&mut self, object: &Object) {
        let edges = self
            .state
            .graph()
            .outgoing(&object.id)
            .into_iter()
            .chain(object.relations.iter())
            .cloned()
            .collect::<Vec<_>>();
        for edge in edges {
            self.index_quote_edge(&edge);
        }
    }

    pub(crate) fn reindex_author_quotes(&mut self, author: &IdentityId) {
        let sources = self
            .quotes
            .sources
            .keys()
            .filter_map(|id| {
                self.object(id)
                    .filter(|object| &object.author == author)
                    .cloned()
            })
            .collect::<Vec<_>>();
        for source in sources {
            self.quotes.sources.remove(&source.id);
            self.index_object_quotes(&source);
        }
    }
}

fn validate_object_id(id: &ObjectId) -> Result<()> {
    id.validate()?;
    if !lower_hex(&id.as_str()[4..]) {
        return Err(Error::Canonical("invalid object identifier".into()));
    }
    Ok(())
}

fn lower_hex(value: &str) -> bool {
    value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests;
