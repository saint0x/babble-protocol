//! Public author Objects, newest first. Cursors are snapshot bookmarks: any
//! insertion for this author invalidates the snapshot with a conflict, including
//! backdated imports. Clients restart at page one; no Objects silently drift
//! between pages. The immutable index is rebuilt identically on node restart.
use crate::LocalNode;
use babble_identity::Identity;
use babble_judgment::JudgmentProvider;
use babble_object::Object;
use babble_types::{Error, IdentityId, ObjectId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Bound::{Excluded, Unbounded};

type Position = (Timestamp, ObjectId);

pub(crate) const PAGE_OBJECT_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn object_json_bytes(object: &Object) -> Result<usize> {
    struct Counter(usize);
    impl std::io::Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, object)
        .map_err(|error| Error::Canonical(error.to_string()))?;
    Ok(counter.0)
}

#[derive(Default)]
pub(crate) struct AuthorObjectsIndex(BTreeMap<IdentityId, BTreeSet<Position>>);

impl AuthorObjectsIndex {
    pub(crate) fn positions(&self, author: &IdentityId) -> Option<&BTreeSet<Position>> {
        self.0.get(author)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthorObjectsQuery {
    pub identity_id: IdentityId,
    pub cursor: Option<String>,
    #[schemars(range(min = 1, max = 50))]
    pub limit: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[schemars(extend("required" = ["identity", "objects", "next_cursor"]))]
pub struct AuthorObjectsPage {
    pub identity: Identity,
    pub objects: Vec<Object>,
    pub next_cursor: Option<String>,
}

impl<P: JudgmentProvider> LocalNode<P> {
    pub(crate) fn index_profile_object(&mut self, object: &Object) {
        self.author_objects
            .0
            .entry(object.author.clone())
            .or_default()
            .insert((object.created_at, object.id.clone()));
    }

    pub fn author_objects(&self, query: &AuthorObjectsQuery) -> Result<AuthorObjectsPage> {
        self.author_objects_with_budget(query, PAGE_OBJECT_BYTES)
    }

    pub(crate) fn author_objects_with_budget(
        &self,
        query: &AuthorObjectsQuery,
        byte_budget: usize,
    ) -> Result<AuthorObjectsPage> {
        self.check_ready()?;
        query.identity_id.validate()?;
        if !(1..=50).contains(&query.limit) {
            return Err(Error::Canonical(
                "profile limit must be between 1 and 50".into(),
            ));
        }
        let identity = self
            .identity(&query.identity_id)
            .ok_or_else(|| Error::NotFound(query.identity_id.to_string()))?
            .clone();
        let empty = BTreeSet::new();
        let index = self
            .author_objects
            .0
            .get(&query.identity_id)
            .unwrap_or(&empty);
        let after = query
            .cursor
            .as_deref()
            .map(|cursor| self.profile_cursor(&query.identity_id, index, cursor))
            .transpose()?;
        let mut entries = index
            .range((Unbounded, after.as_ref().map_or(Unbounded, Excluded)))
            .rev()
            .peekable();
        let mut objects = Vec::with_capacity(query.limit);
        let mut bytes = 0_usize;
        while objects.len() < query.limit {
            let Some((_, id)) = entries.peek() else {
                break;
            };
            let object = self.require_object(id)?;
            let size = object_json_bytes(object)?;
            if !objects.is_empty() && bytes.saturating_add(size) > byte_budget {
                break;
            }
            bytes = bytes.saturating_add(size);
            objects.push(object.clone());
            entries.next();
        }
        let has_more = entries.peek().is_some();
        let next_cursor = if has_more {
            Some(format!(
                "v1|{}|{}|{}|{}",
                query.identity_id,
                index.len(),
                index.last().expect("nonempty page").1,
                objects.last().expect("nonempty page").id
            ))
        } else {
            None
        };
        Ok(AuthorObjectsPage {
            identity,
            objects,
            next_cursor,
        })
    }

    fn profile_cursor(
        &self,
        author: &IdentityId,
        index: &BTreeSet<Position>,
        cursor: &str,
    ) -> Result<Position> {
        let invalid = || Error::Canonical("invalid profile cursor for this identity".into());
        if cursor.len() > 256 {
            return Err(invalid());
        }
        let parts = cursor.split('|').collect::<Vec<_>>();
        if parts.len() != 5 || parts[0] != "v1" || parts[1] != author.as_str() {
            return Err(invalid());
        }
        let count = parts[2].parse::<usize>().map_err(|_| invalid())?;
        if count == 0 || count.to_string() != parts[2] {
            return Err(invalid());
        }
        let position = |raw: &str| -> Result<Position> {
            let id = ObjectId::new_unchecked(raw);
            id.validate().map_err(|_| invalid())?;
            let object = self
                .object(&id)
                .filter(|object| &object.author == author)
                .ok_or_else(invalid)?;
            let position = (object.created_at, id);
            if !index.contains(&position) {
                return Err(invalid());
            }
            Ok(position)
        };
        let head = position(parts[3])?;
        let after = position(parts[4])?;
        if after > head {
            return Err(invalid());
        }
        if count != index.len() || index.last() != Some(&head) {
            return Err(Error::Conflict(
                "profile snapshot changed; refresh the profile".into(),
            ));
        }
        Ok(after)
    }
}
