use crate::{Edge, Relation};
use babel_types::{ObjectId, Timestamp};
use std::{collections::BTreeMap, ops::Bound};

pub type ReplyPosition = (Timestamp, ObjectId);

/// Materialized adjacency ordered by source Object time, not edge publication time.
/// Callers must validate signatures and both endpoints before inserting.
#[derive(Default)]
pub struct RepliesIndex {
    incoming: BTreeMap<ObjectId, BTreeMap<ReplyPosition, Edge>>,
}

impl RepliesIndex {
    pub fn insert(&mut self, edge: &Edge, source_created_at: Timestamp) {
        if edge.relation != Relation::ReplyTo || edge.source == edge.target {
            return;
        }
        self.incoming
            .entry(edge.target.clone())
            .or_default()
            .entry((source_created_at, edge.source.clone()))
            .and_modify(|existing| {
                if edge.id < existing.id {
                    *existing = edge.clone();
                }
            })
            .or_insert_with(|| edge.clone());
    }

    pub fn contains(&self, root: &ObjectId, position: &ReplyPosition) -> bool {
        self.incoming
            .get(root)
            .is_some_and(|entries| entries.contains_key(position))
    }

    pub fn page(
        &self,
        root: &ObjectId,
        after: Option<&ReplyPosition>,
        limit: usize,
    ) -> Vec<(&ReplyPosition, &Edge)> {
        self.page_matching(root, after, limit, |_| true)
    }

    /// Filter before limiting so excluded replies cannot hide later visible pages.
    pub fn page_matching(
        &self,
        root: &ObjectId,
        after: Option<&ReplyPosition>,
        limit: usize,
        mut include: impl FnMut(&ObjectId) -> bool,
    ) -> Vec<(&ReplyPosition, &Edge)> {
        let start = after.map_or(Bound::Unbounded, Bound::Excluded);
        self.incoming
            .get(root)
            .into_iter()
            .flat_map(|entries| entries.range((start, Bound::Unbounded)))
            .filter(|((_, id), _)| include(id))
            .take(limit)
            .collect()
    }
}
