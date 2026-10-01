use babel_crypto::Keypair;
use babel_identity::{Identity, IdentityKind};
use babel_judgment_local::LocalProvider;
use babel_node::{AuthorObjectsQuery, ImportBundle, LocalNode};
use babel_object::Object;
use babel_types::{Error, IdentityId, Timestamp};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture {
    root: PathBuf,
    node: LocalNode<LocalProvider>,
    author: Identity,
    key: Keypair,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babel-profiles-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let key = Keypair::from_ed25519_secret_hex(&"31".repeat(32)).unwrap();
        let author = Identity::create(IdentityKind::Person, "profile-author", &key).unwrap();
        node.import_signing_identity(author.clone(), key.clone())
            .unwrap();
        Self {
            root,
            node,
            author,
            key,
        }
    }
    fn object(&mut self, text: &str, at: i64) -> Object {
        let mut object = Object::text(&self.author, text).unwrap();
        object.created_at = Timestamp(time::OffsetDateTime::from_unix_timestamp(at).unwrap());
        let object = object
            .with_relations(vec![])
            .unwrap()
            .sign(&self.author, &self.key)
            .unwrap();
        self.node
            .publish_object_record(&self.author.id, object)
            .unwrap()
    }
    fn query(&self, cursor: Option<String>, limit: usize) -> AuthorObjectsQuery {
        AuthorObjectsQuery {
            identity_id: self.author.id.clone(),
            cursor,
            limit,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

#[test]
fn profiles_order_ties_paginate_and_resume_after_restart() {
    let mut f = Fixture::new();
    let mut expected = vec![
        f.object("early", 1),
        f.object("tie-a", 20),
        f.object("tie-b", 20),
        f.object("late", 30),
    ];
    expected.sort_by(|a, b| (b.created_at, &b.id).cmp(&(a.created_at, &a.id)));
    let page = f.node.author_objects(&f.query(None, 2)).unwrap();
    assert_eq!(page.identity, f.author);
    assert_eq!(page.objects, expected[..2]);
    f.node = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    let page = f
        .node
        .author_objects(&f.query(page.next_cursor, 2))
        .unwrap();
    assert_eq!(page.objects, expected[2..]);
    assert!(page.next_cursor.is_none());
}

#[test]
fn profiles_snapshot_rejects_newer_and_backdated_objects_without_duplicates() {
    for at in [0, 100] {
        let mut f = Fixture::new();
        f.object("one", 1);
        f.object("two", 2);
        let page = f.node.author_objects(&f.query(None, 1)).unwrap();
        f.object("new", at);
        assert!(matches!(
            f.node.author_objects(&f.query(page.next_cursor, 1)),
            Err(Error::Conflict(_))
        ));
        assert_eq!(
            f.node
                .author_objects(&f.query(None, 50))
                .unwrap()
                .objects
                .len(),
            3
        );
    }
}

#[test]
fn profiles_validate_limits_cursor_author_scope_and_empty_identities() {
    let mut f = Fixture::new();
    let empty = f.node.author_objects(&f.query(None, 20)).unwrap();
    assert!(empty.objects.is_empty() && empty.next_cursor.is_none());
    for limit in [0, 51, usize::MAX] {
        assert!(matches!(
            f.node.author_objects(&f.query(None, limit)),
            Err(Error::Canonical(_))
        ));
    }
    for cursor in ["", "v2|bad", &"x".repeat(257)] {
        assert!(matches!(
            f.node.author_objects(&f.query(Some(cursor.into()), 20)),
            Err(Error::Canonical(_))
        ));
    }
    let unknown = AuthorObjectsQuery {
        identity_id: IdentityId::new_unchecked(format!("id_{}", "0".repeat(64))),
        cursor: None,
        limit: 20,
    };
    assert!(f.node.author_objects(&unknown).is_err());
    f.object("one", 1);
    f.object("two", 2);
    let page = f.node.author_objects(&f.query(None, 1)).unwrap();
    let other = f
        .node
        .create_identity(IdentityKind::Person, "other")
        .unwrap();
    assert!(matches!(
        f.node.author_objects(&AuthorObjectsQuery {
            identity_id: other.id,
            cursor: page.next_cursor,
            limit: 1,
        }),
        Err(Error::Canonical(_))
    ));
}

#[test]
fn profiles_import_indexes_only_the_author_and_duplicate_import_preserves_cursor() {
    let mut f = Fixture::new();
    let one = f.object("one", 1);
    f.object("two", 2);
    let page = f.node.author_objects(&f.query(None, 1)).unwrap();
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
            .author_objects(&f.query(page.next_cursor, 1))
            .unwrap()
            .objects,
        vec![one]
    );
    let other = f
        .node
        .create_identity(IdentityKind::Person, "another-author")
        .unwrap();
    f.node
        .publish_text(&other.id, "Other author's Object")
        .unwrap();
    assert_eq!(
        f.node
            .author_objects(&f.query(None, 50))
            .unwrap()
            .objects
            .len(),
        2
    );
}

#[test]
fn profiles_all_page_sizes_are_complete_and_cursor_corruption_is_rejected() {
    let mut f = Fixture::new();
    let mut expected = (0..9)
        .map(|n| f.object(&format!("Object {n}"), n / 3))
        .collect::<Vec<_>>();
    expected.sort_by(|a, b| (b.created_at, &b.id).cmp(&(a.created_at, &a.id)));
    for limit in 1..=10 {
        let mut cursor = None;
        let mut found = Vec::new();
        loop {
            let page = f.node.author_objects(&f.query(cursor, limit)).unwrap();
            assert!(page.objects.len() <= limit);
            found.extend(page.objects);
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
            assert!(found.len() < expected.len());
        }
        assert_eq!(found, expected);
    }
    let cursor = f
        .node
        .author_objects(&f.query(None, 1))
        .unwrap()
        .next_cursor
        .unwrap();
    for position in 0..cursor.len() {
        let mut corrupted = cursor.as_bytes().to_vec();
        corrupted[position] = b'~';
        let result = f
            .node
            .author_objects(&f.query(Some(String::from_utf8(corrupted).unwrap()), 1));
        assert!(result.is_err(), "corruption at {position} was accepted");
    }
}
