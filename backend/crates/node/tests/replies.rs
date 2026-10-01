use babel_crypto::Keypair;
use babel_graph::{Edge, EdgeOrigin, Relation};
use babel_identity::{Identity, IdentityKind};
use babel_judgment_local::LocalProvider;
use babel_node::{ImportBundle, LocalNode, RepliesListQuery};
use babel_object::Object;
use babel_types::{Hash, ObjectId, Timestamp};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture {
    path: PathBuf,
    node: LocalNode<LocalProvider>,
    author: Identity,
    key: Keypair,
}

impl Fixture {
    fn new() -> Self {
        static NEXT_PATH: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "babel-replies-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            NEXT_PATH.fetch_add(1, Ordering::Relaxed)
        ));
        let mut node = LocalNode::open(&path, LocalProvider::default()).unwrap();
        let key = Keypair::from_ed25519_secret_hex(&"31".repeat(32)).unwrap();
        let author = Identity::create(IdentityKind::Person, "reply-author", &key).unwrap();
        node.import_signing_identity(author.clone(), key.clone())
            .unwrap();
        Self {
            path,
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

    fn edge(&mut self, source: &Object, target: &Object, relation: Relation) -> Edge {
        self.node
            .publish_edge(
                &self.author.id,
                source.id.clone(),
                target.id.clone(),
                relation,
                EdgeOrigin::HumanAssertion,
            )
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.path).unwrap();
    }
}

fn query(root: &Object, cursor: Option<String>, limit: usize) -> RepliesListQuery {
    RepliesListQuery {
        object_id: root.id.clone(),
        cursor,
        limit,
    }
}

#[test]
fn replies_page_by_object_time_and_id_deduplicate_and_survive_restart() {
    let mut f = Fixture::new();
    let root = f.object("root", 1);
    let late = f.object("late", 30);
    let a = f.object("tie-a", 20);
    let b = f.object("tie-b", 20);
    let early = f.object("early", 10);
    let nested = f.object("nested", 15);
    let unrelated = f.object("unrelated", 2);
    f.edge(&late, &root, Relation::ReplyTo);
    let first_edge = f.edge(&a, &root, Relation::ReplyTo);
    let duplicate = f
        .node
        .publish_edge(
            &f.author.id,
            a.id.clone(),
            root.id.clone(),
            Relation::ReplyTo,
            EdgeOrigin::ApplicationAssertion,
        )
        .unwrap();
    f.edge(&early, &root, Relation::ReplyTo);
    f.edge(&b, &root, Relation::ReplyTo);
    f.edge(&nested, &a, Relation::ReplyTo);
    f.edge(&unrelated, &root, Relation::References);
    f.edge(&root, &unrelated, Relation::ReplyTo);
    f.edge(&root, &root, Relation::ReplyTo);
    let mut expected = vec![early, a.clone(), b, late];
    expected.sort_by_key(|object| (object.created_at, object.id.clone()));
    let page = f.node.list_replies(&query(&root, None, 2)).unwrap();
    assert_eq!(
        page.replies
            .iter()
            .map(|reply| &reply.object)
            .collect::<Vec<_>>(),
        expected[..2].iter().collect::<Vec<_>>()
    );
    let cursor = page.next_cursor.clone().unwrap();
    // Add a later reply between page reads, without repeating earlier replies.
    let newest = f.object("newest", 40);
    f.edge(&newest, &root, Relation::ReplyTo);
    expected.push(newest);
    let node = LocalNode::open(&f.path, LocalProvider::default()).unwrap();
    let rest = node
        .list_replies(&query(&root, Some(cursor.clone()), 50))
        .unwrap();
    assert_eq!(
        rest.replies
            .iter()
            .map(|reply| &reply.object)
            .collect::<Vec<_>>(),
        expected[2..].iter().collect::<Vec<_>>()
    );
    assert!(rest.next_cursor.is_none());
    assert_eq!(
        rest,
        f.node
            .list_replies(&query(&root, Some(cursor), 50))
            .unwrap()
    );
    let full = node.list_replies(&query(&root, None, 50)).unwrap();
    let chosen = &full
        .replies
        .iter()
        .find(|reply| reply.object.id == a.id)
        .unwrap()
        .edge;
    assert_eq!(chosen.id, first_edge.id.min(duplicate.id));
    assert_eq!(
        node.list_replies(&query(&unrelated, None, 50))
            .unwrap()
            .replies
            .len(),
        1
    );
}

#[test]
fn replies_reject_bad_limits_cursors_and_missing_parents() {
    let mut f = Fixture::new();
    let root = f.object("root", 1);
    let other = f.object("other", 2);
    assert!(
        f.node
            .list_replies(&query(&root, None, 50))
            .unwrap()
            .replies
            .is_empty()
    );
    for limit in [0, 51, usize::MAX] {
        assert!(matches!(
            f.node.list_replies(&query(&root, None, limit)),
            Err(babel_types::Error::Canonical(_))
        ));
    }
    for cursor in [
        String::new(),
        "v2|bad".into(),
        "x".repeat(100_000),
        format!("v1|{}|{}", other.id, root.id),
        format!("v1|{}|{}", root.id, other.id),
        format!("v1|{}|obj_{}", root.id, "g".repeat(64)),
    ] {
        assert!(matches!(
            f.node.list_replies(&query(&root, Some(cursor), 1)),
            Err(babel_types::Error::Canonical(_))
        ));
    }
    let missing = RepliesListQuery {
        object_id: ObjectId::from_hash(&Hash::from_bytes(b"missing")),
        cursor: None,
        limit: 1,
    };
    assert!(matches!(
        f.node.list_replies(&missing),
        Err(babel_types::Error::NotFound(_))
    ));
}

#[test]
fn replies_embedded_edges_validate_and_resolve_late_endpoints() {
    let mut f = Fixture::new();
    let root = f.object("root", 1);
    let pending = Object::text(&f.author, "pending source")
        .unwrap()
        .sign(&f.author, &f.key)
        .unwrap();
    let valid = Edge::new(
        pending.id.clone(),
        root.id.clone(),
        Relation::ReplyTo,
        EdgeOrigin::HumanAssertion,
        Some(f.author.id.clone()),
    )
    .unwrap()
    .sign(&f.author, &f.key)
    .unwrap();
    let invalid_source = f.object("invalid signature source", 3);
    let unsigned = Edge::new(
        invalid_source.id.clone(),
        root.id.clone(),
        Relation::ReplyTo,
        EdgeOrigin::HumanAssertion,
        Some(f.author.id.clone()),
    )
    .unwrap();
    let mut forged = unsigned.clone();
    forged.signature = valid.signature.clone();
    let carrier = Object::text(&f.author, "embedded relations")
        .unwrap()
        .with_relations(vec![valid, unsigned, forged])
        .unwrap()
        .sign(&f.author, &f.key)
        .unwrap();
    f.node
        .import_bundle(ImportBundle {
            objects: vec![carrier],
            ..Default::default()
        })
        .unwrap();
    assert!(
        f.node
            .list_replies(&query(&root, None, 50))
            .unwrap()
            .replies
            .is_empty()
    );
    f.node
        .import_bundle(ImportBundle {
            objects: vec![pending.clone()],
            ..Default::default()
        })
        .unwrap();
    let result = f.node.list_replies(&query(&root, None, 50)).unwrap();
    assert_eq!(result.replies.len(), 1);
    assert_eq!(result.replies[0].object, pending);
    let reopened = LocalNode::open(&f.path, LocalProvider::default()).unwrap();
    assert_eq!(
        result,
        reopened.list_replies(&query(&root, None, 50)).unwrap()
    );
}

#[test]
fn replies_over_multiple_full_pages_never_truncate_or_repeat() {
    let mut f = Fixture::new();
    let root = f.object("root", 1);
    let mut objects = Vec::new();
    let mut edges = Vec::new();
    for i in 0..103 {
        let mut object = Object::text(&f.author, format!("reply-{i}")).unwrap();
        object.created_at = Timestamp(time::OffsetDateTime::from_unix_timestamp(2).unwrap());
        let object = object
            .with_relations(vec![])
            .unwrap()
            .sign(&f.author, &f.key)
            .unwrap();
        edges.push(
            Edge::new(
                object.id.clone(),
                root.id.clone(),
                Relation::ReplyTo,
                EdgeOrigin::HumanAssertion,
                Some(f.author.id.clone()),
            )
            .unwrap()
            .sign(&f.author, &f.key)
            .unwrap(),
        );
        objects.push(object);
    }
    f.node
        .import_bundle(ImportBundle {
            objects: objects.clone(),
            edges,
            ..Default::default()
        })
        .unwrap();
    objects.sort_by_key(|object| object.id.clone());
    let mut cursor = None;
    let mut actual = Vec::new();
    let mut sizes = Vec::new();
    loop {
        let result = f.node.list_replies(&query(&root, cursor, 50)).unwrap();
        sizes.push(result.replies.len());
        actual.extend(result.replies.into_iter().map(|reply| reply.object));
        cursor = result.next_cursor;
        if cursor.is_none() {
            break;
        }
        assert!(sizes.len() < 4);
    }
    assert_eq!(sizes, [50, 50, 3]);
    assert_eq!(actual, objects);
}
