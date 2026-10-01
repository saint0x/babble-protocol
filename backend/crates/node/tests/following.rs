use babel_crypto::Keypair;
use babel_identity::{Identity, IdentityKeyScope, IdentityKind};
use babel_judgment_local::LocalProvider;
use babel_node::{AuthorObjectsQuery, FollowingQuery, ImportBundle, LocalNode};
use babel_object::Object;
use babel_types::{Error, IdentityId, Timestamp};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
    node: LocalNode<LocalProvider>,
    viewer: Identity,
    first: Identity,
    second: Identity,
    key: Keypair,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babel-follow-node-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let viewer = node
            .create_identity(IdentityKind::Person, "viewer")
            .unwrap();
        let key = Keypair::generate();
        let first = Identity::create(IdentityKind::Person, "first", &key).unwrap();
        let second = Identity::create(IdentityKind::Person, "second", &key).unwrap();
        node.import_signing_identity(first.clone(), key.clone())
            .unwrap();
        node.import_signing_identity(second.clone(), key.clone())
            .unwrap();
        Self {
            root,
            node,
            viewer,
            first,
            second,
            key,
        }
    }
    fn follow(&mut self) {
        self.node
            .set_following(&self.viewer.id, &self.first.id, true, 0, "first")
            .unwrap();
        self.node
            .set_following(&self.viewer.id, &self.second.id, true, 0, "second")
            .unwrap();
    }
    fn object(&mut self, second: bool, text: &str, at: i64) -> Object {
        let author = if second { &self.second } else { &self.first };
        let mut object = Object::text(author, text).unwrap();
        object.created_at = Timestamp(time::OffsetDateTime::from_unix_timestamp(at).unwrap());
        let object = object
            .with_relations(vec![])
            .unwrap()
            .sign(author, &self.key)
            .unwrap();
        self.node.publish_object_record(&author.id, object).unwrap()
    }
    fn query(cursor: Option<String>, limit: usize) -> FollowingQuery {
        FollowingQuery {
            cursor,
            limit,
            search: None,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

#[test]
fn following_merges_authors_ties_paginates_and_survives_restart() {
    let mut f = Fixture::new();
    f.follow();
    let mut expected = (0..9)
        .map(|n| f.object(n % 2 == 0, &format!("post-{n}"), n / 3))
        .collect::<Vec<_>>();
    expected.sort_by(|a, b| (b.created_at, &b.id).cmp(&(a.created_at, &a.id)));
    f.node
        .publish_text(&f.viewer.id, "not in own following feed")
        .unwrap();
    for limit in 1..=10 {
        let mut cursor = None;
        let mut found = Vec::new();
        loop {
            let page = f
                .node
                .following_feed(&f.viewer.id, &Fixture::query(cursor, limit))
                .unwrap();
            found.extend(page.objects);
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
            f.node = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
        }
        assert_eq!(found, expected);
    }
}

#[test]
fn following_snapshot_invalidates_backdated_imports_and_graph_changes() {
    let mut f = Fixture::new();
    f.follow();
    let one = f.object(false, "one", 1);
    f.object(true, "two", 2);
    let cursor = f
        .node
        .following_feed(&f.viewer.id, &Fixture::query(None, 1))
        .unwrap()
        .next_cursor;
    f.node
        .import_bundle(ImportBundle {
            identities: vec![],
            objects: vec![one.clone()],
            edges: vec![],
            events: vec![],
        })
        .unwrap();
    assert_eq!(
        f.node
            .following_feed(&f.viewer.id, &Fixture::query(cursor.clone(), 1))
            .unwrap()
            .objects,
        vec![one]
    );
    let mut imported = Object::text(&f.first, "backdated import").unwrap();
    imported.created_at = Timestamp(time::OffsetDateTime::from_unix_timestamp(0).unwrap());
    let imported = imported
        .with_relations(vec![])
        .unwrap()
        .sign(&f.first, &f.key)
        .unwrap();
    f.node
        .import_bundle(ImportBundle {
            identities: vec![],
            objects: vec![imported],
            edges: vec![],
            events: vec![],
        })
        .unwrap();
    assert!(matches!(
        f.node
            .following_feed(&f.viewer.id, &Fixture::query(cursor, 1)),
        Err(Error::Conflict(_))
    ));
    let cursor = f
        .node
        .following_feed(&f.viewer.id, &Fixture::query(None, 1))
        .unwrap()
        .next_cursor;
    let list = f.node.list_following(&f.viewer.id, None, 1).unwrap();
    f.node
        .set_following(&f.viewer.id, &f.first.id, false, 1, "unfirst")
        .unwrap();
    f.node
        .set_following(&f.viewer.id, &f.second.id, false, 1, "unsecond")
        .unwrap();
    assert!(matches!(
        f.node
            .following_feed(&f.viewer.id, &Fixture::query(cursor, 1)),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        f.node
            .list_following(&f.viewer.id, list.next_cursor.as_deref(), 1),
        Err(Error::Conflict(_))
    ));
}

#[test]
fn following_retry_rotation_restart_and_public_event_privacy() {
    let mut f = Fixture::new();
    let before = f
        .node
        .list_events(babel_node::EventListQuery {
            after: None,
            limit: 100,
        })
        .unwrap()
        .events;
    let on = f
        .node
        .set_following(&f.viewer.id, &f.first.id, true, 0, "on")
        .unwrap();
    assert_eq!(
        f.node
            .list_events(babel_node::EventListQuery {
                after: None,
                limit: 100
            })
            .unwrap()
            .events,
        before
    );
    f.node
        .rotate_identity_key(&f.viewer.id, IdentityKeyScope::Root, None, "rotation")
        .unwrap();
    let off = f
        .node
        .set_following(&f.viewer.id, &f.first.id, false, 1, "off")
        .unwrap();
    f.node = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    assert_eq!(
        f.node
            .set_following(&f.viewer.id, &f.first.id, true, 0, "on")
            .unwrap(),
        on
    );
    assert_eq!(f.node.follow_state(&f.viewer.id, &f.first.id).unwrap(), off);
    assert!(matches!(
        f.node
            .set_following(&f.viewer.id, &f.first.id, true, 1, "stale"),
        Err(Error::Conflict(_))
    ));
    assert!(matches!(
        f.node
            .set_following(&f.viewer.id, &f.second.id, true, 0, "on"),
        Err(Error::Conflict(_))
    ));
    let events = serde_json::to_string(
        &f.node
            .list_events(babel_node::EventListQuery {
                after: None,
                limit: 100,
            })
            .unwrap(),
    )
    .unwrap();
    assert!(!events.contains("idempotency_key"));
    assert!(!events.contains("following"));
}

#[test]
fn following_validates_empty_views_unknown_ids_query_binding_and_search() {
    let mut f = Fixture::new();
    assert!(
        f.node
            .following_feed(&f.viewer.id, &Fixture::query(None, 20))
            .unwrap()
            .objects
            .is_empty()
    );
    for raw in ["", "garbage", &"x".repeat(2049)] {
        assert!(
            f.node
                .following_feed(&f.viewer.id, &Fixture::query(Some(raw.into()), 20))
                .is_err()
        );
        assert!(f.node.list_following(&f.viewer.id, Some(raw), 20).is_err());
    }
    for limit in [0, 51, usize::MAX] {
        assert!(
            f.node
                .following_feed(&f.viewer.id, &Fixture::query(None, limit))
                .is_err()
        );
        assert!(f.node.list_following(&f.viewer.id, None, limit).is_err());
    }
    let unknown = IdentityId::new_unchecked(format!("id_{}", "0".repeat(64)));
    assert!(
        f.node
            .set_following(&f.viewer.id, &unknown, true, 0, "unknown")
            .is_err()
    );
    assert!(
        f.node
            .set_following(&f.viewer.id, &f.viewer.id, true, 0, "self")
            .is_err()
    );
    f.follow();
    f.object(false, "Hello WORLD", 1);
    f.object(true, "world twice", 2);
    f.object(false, "irrelevant", 3);
    let mut query = Fixture::query(None, 1);
    query.search = Some("WORLD".into());
    let first = f.node.following_feed(&f.viewer.id, &query).unwrap();
    assert_eq!(first.objects[0].payload["text"], "world twice");
    query.cursor = first.next_cursor.clone();
    assert_eq!(
        f.node.following_feed(&f.viewer.id, &query).unwrap().objects[0].payload["text"],
        "Hello WORLD"
    );
    query.search = Some("irrelevant".into());
    assert!(f.node.following_feed(&f.viewer.id, &query).is_err());
    assert!(
        f.node
            .following_feed(&f.first.id, &Fixture::query(first.next_cursor.clone(), 1))
            .is_err()
    );
    query = Fixture::query(first.next_cursor, 2);
    query.search = Some("WORLD".into());
    assert!(f.node.following_feed(&f.viewer.id, &query).is_err());
}

#[test]
fn following_lists_real_identities_and_noop_keeps_snapshot() {
    let mut f = Fixture::new();
    f.follow();
    let mut expected = vec![f.first.clone(), f.second.clone()];
    expected.sort_by(|a, b| a.id.cmp(&b.id));
    let first = f.node.list_following(&f.viewer.id, None, 1).unwrap();
    assert_eq!(first.identities, expected[..1]);
    assert_eq!(
        f.node
            .set_following(&f.viewer.id, &f.first.id, true, 1, "noop")
            .unwrap()
            .revision,
        1
    );
    let second = f
        .node
        .list_following(&f.viewer.id, first.next_cursor.as_deref(), 1)
        .unwrap();
    assert_eq!(second.identities, expected[1..]);
    assert!(second.next_cursor.is_none());
}

#[test]
fn following_search_finds_text_after_64k_and_serialized_pages_are_bounded() {
    let mut f = Fixture::new();
    f.follow();
    let large = format!("{} NeedleAtEnd", "x".repeat(70_000));
    let object = f.object(false, &large, 1);
    let page = f
        .node
        .following_feed(
            &f.viewer.id,
            &FollowingQuery {
                limit: 20,
                cursor: None,
                search: Some("needleatend".into()),
            },
        )
        .unwrap();
    assert_eq!(page.objects, vec![object]);
    let page = f
        .node
        .author_objects(&AuthorObjectsQuery {
            identity_id: f.first.id.clone(),
            limit: 20,
            cursor: None,
        })
        .unwrap();
    assert_eq!(page.objects[0].payload["text"], large);
}
