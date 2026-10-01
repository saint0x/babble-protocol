use crate::LocalNode;
use babble_graph::{Edge, Relation, ReplyPosition};
use babble_judgment::JudgmentProvider;
use babble_object::Object;
use babble_types::{Error, ObjectId, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["object_id", "cursor", "limit"]))]
pub struct RepliesListQuery {
    pub object_id: ObjectId,
    #[serde(deserialize_with = "Option::deserialize")]
    pub cursor: Option<String>,
    #[schemars(range(min = 1, max = 50))]
    pub limit: usize,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DirectReply {
    pub object: Object,
    pub edge: Edge,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[schemars(extend("required" = ["object_id", "replies", "next_cursor"]))]
pub struct RepliesListResult {
    pub object_id: ObjectId,
    pub replies: Vec<DirectReply>,
    pub next_cursor: Option<String>,
}

impl<P: JudgmentProvider> LocalNode<P> {
    pub fn list_replies(&self, query: &RepliesListQuery) -> Result<RepliesListResult> {
        self.check_ready()?;
        validate_object_id(&query.object_id)?;
        if !(1..=50).contains(&query.limit) {
            return Err(Error::Canonical(
                "reply list limit must be between 1 and 50".into(),
            ));
        }
        self.require_object(&query.object_id)?;
        let after = query
            .cursor
            .as_deref()
            .map(|cursor| self.reply_cursor(&query.object_id, cursor))
            .transpose()?;
        let (_, restricted) = self.store.moderation_restrictions()?;
        let entries =
            self.replies
                .page_matching(&query.object_id, after.as_ref(), query.limit + 1, |id| {
                    !restricted.contains(id)
                });
        let has_more = entries.len() > query.limit;
        let replies = entries
            .into_iter()
            .take(query.limit)
            .map(|((_, id), edge)| {
                Ok(DirectReply {
                    object: self.require_object(id)?.clone(),
                    edge: edge.clone(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let next_cursor = if has_more {
            replies
                .last()
                .map(|reply| format!("v1|{}|{}", query.object_id, reply.object.id))
        } else {
            None
        };
        Ok(RepliesListResult {
            object_id: query.object_id.clone(),
            replies,
            next_cursor,
        })
    }

    fn reply_cursor(&self, root: &ObjectId, cursor: &str) -> Result<ReplyPosition> {
        let invalid = || Error::Canonical("invalid reply cursor for this object".into());
        // Fixed-size, versioned bookmarks refer to an immutable, existing direct reply.
        if cursor.len() != 140 {
            return Err(invalid());
        }
        let mut parts = cursor.split('|');
        if parts.next() != Some("v1") || parts.next() != Some(root.as_str()) {
            return Err(invalid());
        }
        let id = ObjectId::new_unchecked(parts.next().ok_or_else(invalid)?);
        validate_object_id(&id).map_err(|_| invalid())?;
        if parts.next().is_some() {
            return Err(invalid());
        }
        let object = self.object(&id).ok_or_else(invalid)?;
        let position = (object.created_at, id);
        if !self.replies.contains(root, &position) {
            return Err(invalid());
        }
        Ok(position)
    }

    pub(crate) fn index_reply_edge(&mut self, edge: &Edge) {
        if edge.relation != Relation::ReplyTo || edge.source == edge.target {
            return;
        }
        let Some(source) = self.object(&edge.source) else {
            return;
        };
        let created_at = source.created_at;
        if self.object(&edge.target).is_none() {
            return;
        }
        // Embedded Object relations can enter GraphIndex without edge verification.
        let valid = edge
            .author
            .as_ref()
            .and_then(|id| self.state.signing_identity_at(id, edge.created_at).ok())
            .is_some_and(|author| edge.verify(&author).is_ok());
        if valid {
            self.replies.insert(edge, created_at);
        }
    }

    pub(crate) fn index_object_replies(&mut self, object: &Object) {
        // Revisit both directions when a previously missing endpoint arrives.
        let edges = self
            .state
            .graph()
            .outgoing(&object.id)
            .into_iter()
            .chain(self.state.graph().incoming(&object.id))
            .chain(object.relations.iter())
            .cloned()
            .collect::<Vec<_>>();
        for edge in edges {
            self.index_reply_edge(&edge);
        }
    }
}

fn validate_object_id(id: &ObjectId) -> Result<()> {
    id.validate()?;
    if !id.as_str()[4..]
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::Canonical("invalid object identifier".into()));
    }
    Ok(())
}
