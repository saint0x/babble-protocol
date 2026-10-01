use babel_graph::{Edge, EdgeOrigin, Relation, RepliesIndex};
use babel_types::{Hash, ObjectId, Timestamp};

#[test]
fn replies_index_seeks_orders_and_deduplicates_independently_of_insertion_order() {
    let root = ObjectId::from_hash(&Hash::from_bytes(b"root"));
    let other = ObjectId::from_hash(&Hash::from_bytes(b"other"));
    let at = Timestamp::now();
    let mut edges = (0..200)
        .map(|i| {
            Edge::new(
                ObjectId::from_hash(&Hash::from_bytes(&[i])),
                root.clone(),
                Relation::ReplyTo,
                EdgeOrigin::HumanAssertion,
                None,
            )
            .unwrap()
        })
        .collect::<Vec<_>>();
    let mut index = RepliesIndex::default();
    for edge in edges.iter().rev() {
        index.insert(edge, at);
        index.insert(edge, at);
    }
    let mut ignored = edges[0].clone();
    ignored.source = root.clone();
    index.insert(&ignored, at);
    ignored.source = other.clone();
    ignored.relation = Relation::References;
    index.insert(&ignored, at);
    edges.sort_by_key(|edge| edge.source.clone());
    let first = index.page(&root, None, 50);
    assert_eq!(first.len(), 50);
    assert_eq!(first[0].1, &edges[0]);
    let last = index.page(&root, Some(&(at, edges[149].source.clone())), 51);
    assert_eq!(
        last.iter().map(|(_, edge)| *edge).collect::<Vec<_>>(),
        edges[150..].iter().collect::<Vec<_>>()
    );
    assert!(
        index
            .page(&root, Some(&(at, edges[199].source.clone())), 50)
            .is_empty()
    );
    assert!(index.page(&other, None, 50).is_empty());
    assert!(index.page(&root, None, 0).is_empty());
}
