use super::*;
use crate::ImportBundle;
use babble_crypto::Keypair;
use babble_graph::EdgeOrigin;
use babble_identity::{Identity, IdentityKeyScope, IdentityKind};
use babble_judgment_local::LocalProvider;
use babble_types::Hash;
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
            "babble-quotes-{}-{}-{}",
            std::process::id(),
            Timestamp::now().0.unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let key = Keypair::generate();
        let author = Identity::create(IdentityKind::Person, "quote-author", &key).unwrap();
        node.import_signing_identity(author.clone(), key.clone())
            .unwrap();
        Self {
            root,
            node,
            author,
            key,
        }
    }

    fn object(&mut self, text: &str) -> Object {
        self.node.publish_text(&self.author.id, text).unwrap()
    }

    fn edge(&self, source: &Object, target: &Object, at: i64) -> Edge {
        let mut edge = Edge::new(
            source.id.clone(),
            target.id.clone(),
            Relation::Quotes,
            EdgeOrigin::HumanAssertion,
            Some(self.author.id.clone()),
        )
        .unwrap();
        edge.created_at = Timestamp(time::OffsetDateTime::from_unix_timestamp(at).unwrap());
        edge.with_metadata(BTreeMap::new())
            .unwrap()
            .sign(&self.author, &self.key)
            .unwrap()
    }

    fn import(&mut self, objects: Vec<Object>, edges: Vec<Edge>) {
        self.node
            .import_bundle(ImportBundle {
                objects,
                edges,
                ..Default::default()
            })
            .unwrap();
    }

    fn carrier(&self, edges: Vec<Edge>) -> Object {
        Object::text(&self.author, "embedded edge carrier")
            .unwrap()
            .with_relations(edges)
            .unwrap()
            .sign(&self.author, &self.key)
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn query(source: &Object, cursor: Option<String>, limit: usize) -> QuotesListQuery {
    QuotesListQuery {
        object_id: source.id.clone(),
        cursor,
        limit,
    }
}

#[test]
fn quotes_deduplicate_and_page_by_edge_time_then_id_across_import_and_restart() {
    let mut f = Fixture::new();
    let source = f.object("quote collection");
    let mut expected = Vec::new();
    let mut edges = Vec::new();
    for i in 0..43 {
        let target = f.object(&format!("target-{i}"));
        let edge = f.edge(&source, &target, 10 + i % 3);
        let tied = edge
            .clone()
            .with_metadata(BTreeMap::from([(
                "duplicate".into(),
                serde_json::json!(true),
            )]))
            .unwrap()
            .sign(&f.author, &f.key)
            .unwrap();
        let chosen = if edge.id < tied.id { &edge } else { &tied };
        expected.push(QuotedObject {
            object: Some(target.clone()),
            edge: chosen.clone(),
        });
        edges.push(edge);
        edges.push(tied);
        edges.push(f.edge(&source, &target, 50));
    }
    // Reverse arrival order cannot select a later duplicate.
    edges.reverse();
    f.import(vec![], edges);
    expected.sort_by_key(|quote| (quote.edge.created_at, quote.edge.id.clone()));
    let incoming = f.edge(expected[0].object.as_ref().unwrap(), &source, 1);
    f.import(vec![], vec![incoming]);
    let mut cursor = None;
    let mut actual = Vec::new();
    let mut sizes = Vec::new();
    loop {
        let page = f
            .node
            .list_quotes(&query(&source, cursor.clone(), 20))
            .unwrap();
        let reopened = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
        assert_eq!(
            page,
            reopened.list_quotes(&query(&source, cursor, 20)).unwrap()
        );
        sizes.push(page.quotes.len());
        actual.extend(page.quotes);
        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
        assert!(sizes.len() < 4);
    }
    assert_eq!(sizes, [20, 20, 3]);
    assert_eq!(actual, expected);
    let before = f.node.list_quotes(&query(&source, None, 20)).unwrap();
    f.import(vec![], vec![expected[0].edge.clone()]);
    assert_eq!(
        before,
        f.node.list_quotes(&query(&source, None, 20)).unwrap()
    );
}

#[test]
fn quotes_reject_unsigned_forged_wrong_author_and_wrong_relation_claims() {
    let mut f = Fixture::new();
    let source = f.object("source");
    let target = f.object("target");
    let stranger = f
        .node
        .create_identity(IdentityKind::Person, "third-party")
        .unwrap();
    let wrong_author = f
        .node
        .publish_edge(
            &stranger.id,
            source.id.clone(),
            target.id.clone(),
            Relation::Quotes,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();
    let wrong_relation = f
        .node
        .publish_edge(
            &f.author.id,
            source.id.clone(),
            target.id.clone(),
            Relation::References,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();
    let mut unsigned = f.edge(&source, &target, 1);
    unsigned.signature = None;
    let mut forged = f.edge(&source, &target, 2);
    forged.signature = wrong_author.signature.clone();
    let mut tampered = f.edge(&source, &target, 3);
    tampered
        .metadata
        .insert("tampered".into(), serde_json::json!(true));
    let invalid = vec![wrong_author, wrong_relation, unsigned, forged, tampered];
    f.import(vec![f.carrier(invalid.clone())], vec![]);
    for node in [
        &f.node,
        &LocalNode::open(&f.root, LocalProvider::default()).unwrap(),
    ] {
        assert!(
            node.list_quotes(&query(&source, None, 20))
                .unwrap()
                .quotes
                .is_empty()
        );
        for edge in &invalid {
            let cursor = format!("v1|{}|{}", source.id, edge.id);
            assert!(node.list_quotes(&query(&source, Some(cursor), 1)).is_err());
        }
    }
    let valid = f.edge(&source, &target, 4);
    f.import(vec![], vec![valid.clone()]);
    assert_eq!(
        f.node
            .list_quotes(&query(&source, None, 20))
            .unwrap()
            .quotes,
        vec![QuotedObject {
            object: Some(target),
            edge: valid
        }]
    );
}

#[test]
fn quotes_missing_target_is_null_and_embedded_edges_resolve_late_source_and_target() {
    let mut f = Fixture::new();
    let source = Object::text(&f.author, "late source")
        .unwrap()
        .sign(&f.author, &f.key)
        .unwrap();
    let target = Object::text(&f.author, "late target")
        .unwrap()
        .sign(&f.author, &f.key)
        .unwrap();
    let edge = f.edge(&source, &target, 10);
    f.import(vec![f.carrier(vec![edge.clone()])], vec![]);
    assert!(matches!(
        f.node.list_quotes(&query(&source, None, 20)),
        Err(Error::NotFound(_))
    ));
    f.import(vec![source.clone()], vec![]);
    let page = f.node.list_quotes(&query(&source, None, 20)).unwrap();
    assert_eq!(
        page.quotes,
        vec![QuotedObject {
            edge: edge.clone(),
            object: None
        }]
    );
    assert_eq!(
        page,
        LocalNode::open(&f.root, LocalProvider::default())
            .unwrap()
            .list_quotes(&query(&source, None, 20))
            .unwrap()
    );
    f.import(vec![target.clone()], vec![]);
    assert_eq!(
        f.node
            .list_quotes(&query(&source, None, 20))
            .unwrap()
            .quotes,
        vec![QuotedObject {
            edge,
            object: Some(target)
        }]
    );
}

#[test]
fn quotes_validate_source_ids_limits_and_source_bound_existing_edge_cursors() {
    let mut f = Fixture::new();
    let source = f.object("source");
    let other = f.object("other");
    let edge = f.edge(&source, &other, 10);
    f.import(vec![], vec![edge.clone()]);
    assert!(
        f.node
            .list_quotes(&query(&other, None, 20))
            .unwrap()
            .quotes
            .is_empty()
    );
    for limit in [0, 21, usize::MAX] {
        assert!(matches!(
            f.node.list_quotes(&query(&source, None, limit)),
            Err(Error::Canonical(_))
        ));
    }
    for cursor in [
        String::new(),
        "x".repeat(100_000),
        format!("|{}|{}", source.id, edge.id),
        format!("v1|{}|{}", other.id, edge.id),
        format!("v1|{}|edge_{}", source.id, "0".repeat(64)),
        format!("v1|{}|edge_{}", source.id, "G".repeat(64)),
        format!("v1|{}|{}|extra", source.id, edge.id),
    ] {
        assert!(matches!(
            f.node.list_quotes(&query(&source, Some(cursor), 1)),
            Err(Error::Canonical(_))
        ));
    }
    for id in [
        "invalid".into(),
        format!("obj_{}", "G".repeat(64)),
        format!("obj_{}", "é".repeat(32)),
    ] {
        assert!(
            f.node
                .list_quotes(&QuotesListQuery {
                    object_id: ObjectId::new_unchecked(id),
                    cursor: None,
                    limit: 1
                })
                .is_err()
        );
    }
    assert!(matches!(
        f.node.list_quotes(&QuotesListQuery {
            object_id: ObjectId::from_hash(&Hash::from_bytes(b"absent")),
            cursor: None,
            limit: 1
        }),
        Err(Error::NotFound(_))
    ));
    let cursor = format!("v1|{}|{}", source.id, edge.id);
    assert!(
        f.node
            .list_quotes(&query(&source, Some(cursor.clone()), 1))
            .unwrap()
            .quotes
            .is_empty()
    );
    // A previously emitted bookmark remains valid after an earlier duplicate arrives.
    let earlier = f.edge(&source, &other, 1);
    f.import(vec![], vec![earlier]);
    assert!(
        f.node
            .list_quotes(&query(&source, Some(cursor), 1))
            .unwrap()
            .quotes
            .is_empty()
    );
}

#[test]
fn quotes_historical_keys_and_late_rotation_import_rebuild_verified_context() {
    let mut f = Fixture::new();
    let source = f.object("source");
    let a = f.object("old quote");
    let b = f.object("new quote");
    let c = f.object("stale-key claim");
    let old = f
        .node
        .publish_edge(
            &f.author.id,
            source.id.clone(),
            a.id.clone(),
            Relation::Quotes,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();
    let (_, transition_event) = f
        .node
        .rotate_identity_key(
            &f.author.id,
            IdentityKeyScope::Root,
            None,
            "rotation regression",
        )
        .unwrap();
    let new = f
        .node
        .publish_edge(
            &f.author.id,
            source.id.clone(),
            b.id.clone(),
            Relation::Quotes,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();
    assert!(
        old.verify(&f.node.signing_identity(&f.author.id).unwrap())
            .is_err()
    );
    assert!(new.verify(&f.author).is_err());
    let expected = f.node.list_quotes(&query(&source, None, 20)).unwrap();
    assert_eq!(expected.quotes.len(), 2);
    assert_eq!(
        expected,
        LocalNode::open(&f.root, LocalProvider::default())
            .unwrap()
            .list_quotes(&query(&source, None, 20))
            .unwrap()
    );

    let mut imported = Fixture::new();
    // Embed both edges in an old-key Object. Only its own timestamp selects the Object key.
    let mut carrier = Object::text(&f.author, "edge carrier before rotation").unwrap();
    carrier.created_at = source.created_at;
    let mut stale = Edge::new(
        source.id.clone(),
        c.id.clone(),
        Relation::Quotes,
        EdgeOrigin::HumanAssertion,
        Some(f.author.id.clone()),
    )
    .unwrap();
    stale.created_at = new.created_at;
    let stale = stale
        .with_metadata(BTreeMap::new())
        .unwrap()
        .sign(&f.author, &f.key)
        .unwrap();
    let carrier = carrier
        .with_relations(vec![old.clone(), new.clone(), stale])
        .unwrap()
        .sign(&f.author, &f.key)
        .unwrap();
    imported
        .node
        .import_bundle(ImportBundle {
            identities: vec![f.author.clone()],
            objects: vec![carrier, c, b, a, source.clone()],
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        imported
            .node
            .list_quotes(&query(&source, None, 20))
            .unwrap()
            .quotes
            .len(),
        2
    );
    let events = f
        .node
        .store
        .list_events()
        .unwrap()
        .into_iter()
        .filter(|event| event.created_at <= transition_event.created_at)
        .collect();
    imported
        .node
        .import_bundle(ImportBundle {
            events,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        expected,
        imported
            .node
            .list_quotes(&query(&source, None, 20))
            .unwrap()
    );
    assert_eq!(
        expected,
        LocalNode::open(&imported.root, LocalProvider::default())
            .unwrap()
            .list_quotes(&query(&source, None, 20))
            .unwrap()
    );
}

#[test]
fn quotes_text_and_media_social_shares_project_their_atomically_published_edge() {
    use babble_authoring::ObjectDraft;
    let mut f = Fixture::new();
    let target_author = f
        .node
        .create_identity(IdentityKind::Person, "original-author")
        .unwrap();
    let target = f.node.publish_text(&target_author.id, "original").unwrap();
    let capability: babble_object::CapabilityRequest = serde_json::from_value(serde_json::json!({
        "id": "babble.social.share", "version": 1, "scope": {"object_id": target.id}
    }))
    .unwrap();
    let controller = f
        .node
        .publish_draft(
            &f.author.id,
            ObjectDraft::text("share controller")
                .unwrap()
                .with_capability(capability.clone())
                .unwrap(),
        )
        .unwrap();
    let media = crate::SocialMediaAttachment {
        title: "attached photo".into(),
        resources: vec![
            f.node
                .put_media_blob("image/png", b"quote test attachment")
                .unwrap(),
        ],
    };
    for attachment in [None, Some(&media)] {
        let publication = crate::invocations::tests::invoke(
            &mut f.node,
            &f.author.id,
            &controller,
            &target.id,
            "share",
            Some("caption"),
            attachment,
        )
        .unwrap();
        let object = publication.object.unwrap();
        let result = f.node.list_quotes(&query(&object, None, 20)).unwrap();
        assert_eq!(
            result.quotes,
            vec![QuotedObject {
                edge: publication.edge,
                object: Some(target.clone())
            }]
        );
        assert_eq!(
            result,
            LocalNode::open(&f.root, LocalProvider::default())
                .unwrap()
                .list_quotes(&query(&object, None, 20))
                .unwrap()
        );
    }
}
