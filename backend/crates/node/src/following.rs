//! Private signed follows and chronological Following views.
//! Limits: 10,000 active follows, 1..50 results, 256-byte request keys,
//! 256-byte search, 2 KiB cursors. A feed call examines at most 10,000 indexed
//! Objects and 16 MiB of complete text payloads (the first Object always makes
//! progress); a continuation may accompany an empty filtered page. Output is
//! bounded to 16 MiB of serialized Objects, with one Object always allowed.
//! Merge memory is O(follows +
//! page size); the shared profile index is O(total Objects). SQLite cache is
//! 2 MiB per connection. The durable action/receipt history grows with writes
//! and is intentionally not pruned: pruning would break indefinite retry safety.
use crate::LocalNode;
pub use babel_graph::FollowState;
use babel_graph::{
    FollowAction, FollowActionPayload, FollowReceipt, FollowReceiptPayload, FollowRequest,
};
use babel_identity::Identity;
use babel_judgment::JudgmentProvider;
use babel_object::Object;
use babel_types::{Canonical, Error, Hash, IdentityId, ObjectId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, BinaryHeap},
    ops::Bound::{Excluded, Unbounded},
};

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["limit", "cursor", "search"]))]
pub struct FollowingQuery {
    #[schemars(range(min = 1, max = 50))]
    pub limit: usize,
    pub cursor: Option<String>,
    pub search: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AuthorObjectsQuery;
    use babel_identity::IdentityKind;
    use babel_judgment_local::LocalProvider;

    #[test]
    fn following_expiry_at_transaction_timestamp_rejects_without_persisting() {
        let root = std::env::temp_dir().join(format!(
            "babel-following-expiry-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let viewer = node
            .create_identity(IdentityKind::Person, "viewer")
            .unwrap();
        let target = node
            .create_identity(IdentityKind::Person, "target")
            .unwrap();
        let expires = Timestamp(Timestamp::now().0 + std::time::Duration::from_secs(60));
        node.rotate_identity_key(
            &viewer.id,
            babel_identity::IdentityKeyScope::Session,
            Some(expires),
            "temporary",
        )
        .unwrap();
        let request = FollowRequest {
            author_id: viewer.id.clone(),
            target_id: target.id.clone(),
            following: true,
            expected_revision: 0,
            idempotency_key: "expiry-boundary".into(),
        };
        // The key is valid when the operation starts but expired at commit time.
        assert_ne!(
            node.signing_identity(&viewer.id).unwrap().public_key,
            viewer.public_key
        );
        assert!(matches!(
            node.commit_follow_request(&request, || expires),
            Err(Error::Signature)
        ));
        assert_eq!(
            node.follow_state(&viewer.id, &target.id).unwrap().revision,
            0
        );
        node.verify_following().unwrap();
        node.set_following(&viewer.id, &target.id, true, 0, "expiry-boundary")
            .unwrap();
        node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        assert_eq!(
            node.follow_state(&viewer.id, &target.id).unwrap().revision,
            1
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn following_budgets_continue_empty_filtered_pages_and_never_skip_large_matches() {
        let root = std::env::temp_dir().join(format!(
            "babel-following-budgets-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos()
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let viewer = node
            .create_identity(IdentityKind::Person, "viewer")
            .unwrap();
        let author = node
            .create_identity(IdentityKind::Person, "author")
            .unwrap();
        node.set_following(&viewer.id, &author.id, true, 0, "follow")
            .unwrap();
        let oldest = node.publish_text(&author.id, "needle").unwrap();
        let middle = node.publish_text(&author.id, "miss-middle").unwrap();
        let newest = node.publish_text(&author.id, "miss-newest").unwrap();
        let mut query = FollowingQuery {
            limit: 50,
            cursor: None,
            search: Some("needle".into()),
        };
        // A byte budget smaller than any text must still examine one whole Object.
        let first = node
            .following_feed_with_budgets(&viewer.id, &query, 100, 1, usize::MAX)
            .unwrap();
        assert!(first.objects.is_empty());
        assert!(first.next_cursor.is_some());
        query.cursor = first.next_cursor;
        let second = node
            .following_feed_with_budgets(&viewer.id, &query, 100, 1, usize::MAX)
            .unwrap();
        assert!(second.objects.is_empty());
        query.cursor = second.next_cursor;
        let third = node
            .following_feed_with_budgets(&viewer.id, &query, 100, 1, usize::MAX)
            .unwrap();
        assert_eq!(third.objects, vec![oldest.clone()]);
        assert!(third.next_cursor.is_none());
        query.cursor = None;
        let first = node
            .following_feed_with_budgets(&viewer.id, &query, 1, usize::MAX, usize::MAX)
            .unwrap();
        assert!(first.objects.is_empty() && first.next_cursor.is_some());
        // Output measurement stops before consuming the unreturned match.
        query.search = None;
        query.cursor = None;
        let mut found = Vec::new();
        loop {
            let page = node
                .following_feed_with_budgets(&viewer.id, &query, 100, usize::MAX, 1)
                .unwrap();
            assert_eq!(page.objects.len(), 1);
            found.extend(page.objects);
            query.cursor = page.next_cursor;
            if query.cursor.is_none() {
                break;
            }
        }
        assert_eq!(found, vec![newest, middle, oldest]);
        let mut profile = AuthorObjectsQuery {
            identity_id: author.id,
            limit: 50,
            cursor: None,
        };
        let mut profile_found = Vec::new();
        loop {
            let page = node.author_objects_with_budget(&profile, 1).unwrap();
            assert_eq!(page.objects.len(), 1);
            profile_found.extend(page.objects);
            profile.cursor = page.next_cursor;
            if profile.cursor.is_none() {
                break;
            }
        }
        assert_eq!(profile_found, found);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["objects", "next_cursor"]))]
pub struct FollowingPage {
    pub objects: Vec<Object>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(extend("required" = ["identities", "next_cursor"]))]
pub struct FollowListPage {
    pub identities: Vec<Identity>,
    pub next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u8,
    viewer: IdentityId,
    view: String,
    limit: usize,
    search: String,
    snapshot: Hash,
    after: String,
    checksum: Hash,
}

impl Cursor {
    fn checksum(&self) -> Result<Hash> {
        (
            self.version,
            &self.viewer,
            &self.view,
            self.limit,
            &self.search,
            &self.snapshot,
            &self.after,
        )
            .canonical_hash()
    }

    fn encode(
        author: &IdentityId,
        view: &str,
        limit: usize,
        search: &str,
        snapshot: &Hash,
        after: String,
    ) -> Result<String> {
        let mut cursor = Self {
            version: 1,
            viewer: author.clone(),
            view: view.into(),
            limit,
            search: search.into(),
            snapshot: snapshot.clone(),
            after,
            checksum: Hash::from_bytes(&[]),
        };
        cursor.checksum = cursor.checksum()?;
        serde_json::to_string(&cursor).map_err(|_| invalid_cursor())
    }

    fn decode(
        raw: &str,
        author: &IdentityId,
        view: &str,
        limit: usize,
        search: &str,
        snapshot: &Hash,
    ) -> Result<Self> {
        if raw.len() > 2048 {
            return Err(invalid_cursor());
        }
        let cursor: Self = serde_json::from_str(raw).map_err(|_| invalid_cursor())?;
        if cursor.version != 1
            || &cursor.viewer != author
            || cursor.view != view
            || cursor.limit != limit
            || cursor.search != search
            || cursor.checksum != cursor.checksum()?
        {
            return Err(invalid_cursor());
        }
        if &cursor.snapshot != snapshot {
            return Err(Error::Conflict(
                "Following snapshot changed; refresh from the first page".into(),
            ));
        }
        Ok(cursor)
    }
}

fn invalid_cursor() -> Error {
    Error::Canonical("invalid Following cursor for this viewer and query".into())
}
fn validate_limit(limit: usize) -> Result<()> {
    if !(1..=50).contains(&limit) {
        return Err(Error::Canonical(
            "following limit must be between 1 and 50".into(),
        ));
    }
    Ok(())
}

impl<P: JudgmentProvider> LocalNode<P> {
    pub(crate) fn verify_following(&self) -> Result<()> {
        self.store
            .verify_following_records(|id, at| self.state.signing_identity_at(id, at))
    }

    fn require_follow_identity(&self, id: &IdentityId) -> Result<&Identity> {
        id.validate()?;
        self.identity(id)
            .ok_or_else(|| Error::NotFound("identity".into()))
    }

    pub fn follow_state(&self, author: &IdentityId, target: &IdentityId) -> Result<FollowState> {
        self.check_ready()?;
        self.require_follow_identity(author)?;
        self.require_follow_identity(target)?;
        if author == target {
            return Err(Error::Canonical("cannot follow yourself".into()));
        }
        self.store.following_state(author, target)
    }

    pub fn set_following(
        &mut self,
        author: &IdentityId,
        target: &IdentityId,
        following: bool,
        expected_revision: u64,
        idempotency_key: &str,
    ) -> Result<FollowState> {
        self.check_ready()?;
        self.require_follow_identity(author)?;
        self.require_follow_identity(target)?;
        let request = FollowRequest {
            author_id: author.clone(),
            target_id: target.clone(),
            following,
            expected_revision,
            idempotency_key: idempotency_key.into(),
        };
        request.validate()?;
        self.commit_follow_request(&request, Timestamp::now)
    }

    fn commit_follow_request(
        &self,
        request: &FollowRequest,
        now: impl FnOnce() -> Timestamp,
    ) -> Result<FollowState> {
        let author = &request.author_id;
        let key = self.local_keypair(author)?;
        let allowed = if request.following {
            self.require_interaction(author, &request.target_id)
        } else { Ok(()) };
        self.store.commit_following(
            &request,
            |state, previous_id, changed, sequence, receipt_previous_id| {
                allowed?;
                let created_at = now();
                let signer = self.state.signing_identity_at(author, created_at)?;
                if key.public_key() != signer.public_key {
                    return Err(Error::Signature);
                }
                let action = changed
                    .then(|| {
                        FollowAction::sign(
                            FollowActionPayload {
                                state: state.clone(),
                                previous_id,
                                created_at,
                                request_id: request.canonical_hash()?,
                            },
                            key,
                        )
                    })
                    .transpose()?;
                let receipt = FollowReceipt::sign(
                    FollowReceiptPayload {
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

    pub fn list_following(
        &self,
        author: &IdentityId,
        cursor: Option<&str>,
        limit: usize,
    ) -> Result<FollowListPage> {
        self.check_ready()?;
        validate_limit(limit)?;
        self.require_follow_identity(author)?;
        let (version, targets) = self.store.following_snapshot(author)?;
        let snapshot = ("following-list-v1", author, version, &targets).canonical_hash()?;
        let after = cursor
            .map(|raw| Cursor::decode(raw, author, "list", limit, "", &snapshot))
            .transpose()?;
        let start = if let Some(cursor) = after {
            targets
                .binary_search(&IdentityId::new_unchecked(cursor.after))
                .map_err(|_| invalid_cursor())?
                + 1
        } else {
            0
        };
        let identities = targets
            .iter()
            .skip(start)
            .take(limit)
            .map(|id| self.require_follow_identity(id).cloned())
            .collect::<Result<Vec<_>>>()?;
        let next_cursor = if start + identities.len() < targets.len() {
            Some(Cursor::encode(
                author,
                "list",
                limit,
                "",
                &snapshot,
                identities.last().expect("positive limit").id.to_string(),
            )?)
        } else {
            None
        };
        Ok(FollowListPage {
            identities,
            next_cursor,
        })
    }

    pub fn following_feed(
        &self,
        author: &IdentityId,
        query: &FollowingQuery,
    ) -> Result<FollowingPage> {
        self.following_feed_with_budgets(
            author,
            query,
            10_000,
            16 * 1024 * 1024,
            crate::profiles::PAGE_OBJECT_BYTES,
        )
    }

    fn following_feed_with_budgets(
        &self,
        author: &IdentityId,
        query: &FollowingQuery,
        scan_limit: usize,
        search_bytes: usize,
        output_bytes: usize,
    ) -> Result<FollowingPage> {
        self.check_ready()?;
        validate_limit(query.limit)?;
        self.require_follow_identity(author)?;
        let raw_search = query.search.as_deref().unwrap_or("");
        if raw_search.len() > 256 || raw_search.chars().any(char::is_control) {
            return Err(Error::Canonical(
                "following search must be at most 256 bytes without control characters".into(),
            ));
        }
        let search = raw_search.trim().to_lowercase();
        let (version, mut targets) = self.store.following_snapshot(author)?;
        let (safety_revision, hidden) = self.store.safety_snapshot(author)?;
        let (_, restricted) = self.store.moderation_restrictions()?;
        let hidden: BTreeSet<_> = hidden.into_iter().map(|state| state.target_id).collect();
        targets.retain(|target| !hidden.contains(target));
        // Immutable Object sets make count + newest position a stable restart-safe
        // snapshot, including when an imported Object predates the current head.
        let stamps = targets
            .iter()
            .map(|id| {
                let positions = self.author_objects.positions(id);
                (
                    id,
                    positions.map_or(0, BTreeSet::len),
                    positions.and_then(BTreeSet::last),
                )
            })
            .collect::<Vec<_>>();
        let snapshot = ("following-feed-v3", author, version, safety_revision, &restricted, &stamps).canonical_hash()?;
        let cursor = query
            .cursor
            .as_deref()
            .map(|raw| Cursor::decode(raw, author, "feed", query.limit, &search, &snapshot))
            .transpose()?;
        let after = cursor
            .map(|cursor| {
                let id = ObjectId::new_unchecked(cursor.after);
                id.validate().map_err(|_| invalid_cursor())?;
                let object = self.object(&id).ok_or_else(invalid_cursor)?;
                if targets.binary_search(&object.author).is_err() {
                    return Err(invalid_cursor());
                }
                Ok((object.created_at, id))
            })
            .transpose()?;
        let mut iterators = targets
            .iter()
            .filter_map(|id| self.author_objects.positions(id))
            .map(|positions| {
                positions
                    .range((Unbounded, after.as_ref().map_or(Unbounded, Excluded)))
                    .rev()
            })
            .collect::<Vec<_>>();
        let mut heap = BinaryHeap::new();
        for (index, iterator) in iterators.iter_mut().enumerate() {
            if let Some(position) = iterator.next() {
                heap.push((position, index));
            }
        }
        let mut objects = Vec::with_capacity(query.limit);
        let mut last = None;
        let mut inspected = 0;
        let mut examined_bytes = 0_usize;
        let mut returned_bytes = 0_usize;
        while objects.len() < query.limit && inspected < scan_limit {
            let Some(&(position, index)) = heap.peek() else {
                break;
            };
            let object = self.require_object(&position.1)?;
            let text = object
                .payload
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let text_bytes = if search.is_empty() { 0 } else { text.len() };
            if inspected > 0 && examined_bytes.saturating_add(text_bytes) > search_bytes {
                break;
            }
            let matches = !restricted.contains(&object.id) && (search.is_empty() || text.to_lowercase().contains(&search));
            if matches {
                let size = crate::profiles::object_json_bytes(object)?;
                if !objects.is_empty() && returned_bytes.saturating_add(size) > output_bytes {
                    break;
                }
                returned_bytes = returned_bytes.saturating_add(size);
                objects.push(object.clone());
            }
            inspected += 1;
            examined_bytes = examined_bytes.saturating_add(text_bytes);
            last = Some(&position.1);
            heap.pop();
            if let Some(position) = iterators[index].next() {
                heap.push((position, index));
            }
        }
        let next_cursor = if !heap.is_empty() {
            Some(Cursor::encode(
                author,
                "feed",
                query.limit,
                &search,
                &snapshot,
                last.expect("positive scan budget").to_string(),
            )?)
        } else {
            None
        };
        Ok(FollowingPage {
            objects,
            next_cursor,
        })
    }
}
