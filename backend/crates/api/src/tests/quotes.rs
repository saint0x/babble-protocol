use super::*;
use crate::router;
use babel_crypto::Keypair;
use babel_graph::{Edge, EdgeOrigin, Relation};
use babel_identity::{Identity, IdentityKind};
use babel_judgment_local::LocalProvider;
use babel_node::{ImportBundle, LocalNode, QuotedObject};
use babel_object::Object;
use babel_rpc::{
    RpcBinding, RpcErrorCode, RpcIdempotency, RpcRequestEnvelope, RpcResponseEnvelope,
    babel_rpc_catalog,
};
use babel_types::Timestamp;
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

struct Fixture {
    root: PathBuf,
    state: ApiState<LocalProvider>,
    source: Object,
    other: Object,
    expected: Vec<QuotedObject>,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babel-quotes-http-{}-{}-{}",
            std::process::id(),
            Timestamp::now().0.unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut node = LocalNode::open(&root, LocalProvider::default()).unwrap();
        let key = Keypair::generate();
        let author = Identity::create(IdentityKind::Person, "quote-api-author", &key).unwrap();
        node.import_signing_identity(author.clone(), key.clone())
            .unwrap();
        let source = node.publish_text(&author.id, "source").unwrap();
        let other = node.publish_text(&author.id, "unquoted").unwrap();
        let mut expected = Vec::new();
        for text in ["first", "second", "third"] {
            let target = node.publish_text(&author.id, text).unwrap();
            let edge = node
                .publish_edge(
                    &author.id,
                    source.id.clone(),
                    target.id.clone(),
                    Relation::Quotes,
                    EdgeOrigin::HumanAssertion,
                )
                .unwrap();
            expected.push(QuotedObject {
                edge,
                object: Some(target),
            });
        }
        node.publish_edge(
            &author.id,
            source.id.clone(),
            expected[0].edge.target.clone(),
            Relation::Quotes,
            EdgeOrigin::ApplicationAssertion,
        )
        .unwrap();
        let outsider = node
            .create_identity(IdentityKind::Person, "quote-outsider")
            .unwrap();
        node.publish_edge(
            &outsider.id,
            source.id.clone(),
            other.id.clone(),
            Relation::Quotes,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();
        node.publish_edge(
            &author.id,
            other.id.clone(),
            source.id.clone(),
            Relation::Quotes,
            EdgeOrigin::HumanAssertion,
        )
        .unwrap();
        let missing = Object::text(&author, "not imported")
            .unwrap()
            .sign(&author, &key)
            .unwrap();
        let edge = Edge::new(
            source.id.clone(),
            missing.id,
            Relation::Quotes,
            EdgeOrigin::HumanAssertion,
            Some(author.id.clone()),
        )
        .unwrap()
        .sign(&author, &key)
        .unwrap();
        expected.push(QuotedObject {
            edge: edge.clone(),
            object: None,
        });
        let unsigned = Edge::new(
            source.id.clone(),
            other.id.clone(),
            Relation::Quotes,
            EdgeOrigin::HumanAssertion,
            Some(author.id.clone()),
        )
        .unwrap();
        let mut forged = unsigned.clone();
        forged.signature = edge.signature.clone();
        let carrier = Object::text(&author, "carrier")
            .unwrap()
            .with_relations(vec![edge, unsigned, forged])
            .unwrap()
            .sign(&author, &key)
            .unwrap();
        node.import_bundle(ImportBundle {
            objects: vec![carrier],
            ..Default::default()
        })
        .unwrap();
        expected.sort_by_key(|quote| (quote.edge.created_at, quote.edge.id.clone()));
        Self {
            root,
            state: ApiState::new(node),
            source,
            other,
            expected,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).unwrap();
    }
}

fn envelope(payload: Value) -> RpcRequestEnvelope {
    RpcRequestEnvelope::new(
        &babel_rpc_catalog().unwrap(),
        "public-quotes",
        "babel.social.quotes.list.v1",
        RpcBinding::host("quotes-test", "babel://test").unwrap(),
        payload,
    )
    .unwrap()
}

#[tokio::test]
async fn quotes_public_http_and_rpc_verify_page_context_without_sessions_or_authors() {
    let f = Fixture::new();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = router(f.state.clone());
    let (stop, shutdown) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown.await;
            })
            .await
            .unwrap();
    });
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let url = format!("{origin}/objects/{}/quotes", f.source.id);
    let response = client.get(&url).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let full: QuotesListResult = response.json().await.unwrap();
    assert_eq!(full.object_id, f.source.id);
    assert_eq!(full.quotes, f.expected);
    assert_eq!(full.next_cursor, None);
    let first: QuotesListResult = client
        .get(&url)
        .query(&[("limit", "2")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(first.quotes, f.expected[..2]);
    let cursor = first.next_cursor.unwrap();
    let response = client
        .post(format!("{origin}/rpc"))
        .json(&envelope(json!({
            "object_id": f.source.id, "cursor": cursor, "limit": 2
        })))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let response: RpcResponseEnvelope = response.json().await.unwrap();
    response.validate(&babel_rpc_catalog().unwrap()).unwrap();
    assert!(response.error.is_none(), "{:?}", response.error);
    let last: QuotesListResult = serde_json::from_value(response.result.unwrap()).unwrap();
    assert_eq!(last.quotes, f.expected[2..]);
    assert_eq!(last.next_cursor, None);
    let rest_last: QuotesListResult = client
        .get(&url)
        .query(&[("cursor", cursor.as_str()), ("limit", "2")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(last, rest_last);

    for input in [
        json!({"object_id": f.other.id, "cursor": cursor, "limit": 1}),
        json!({"object_id": f.source.id, "cursor": "malformed", "limit": 1}),
        json!({"object_id": f.source.id, "cursor": null, "limit": 0}),
        json!({"object_id": f.source.id, "cursor": null, "limit": 21}),
        json!({"object_id": f.source.id, "cursor": null, "limit": -1}),
        json!({"object_id": f.source.id, "cursor": null, "limit": 1.5}),
        json!({"object_id": f.source.id, "limit": 1}),
        json!({"object_id": f.source.id, "cursor": null}),
        json!({"object_id": "invalid", "cursor": null, "limit": 1}),
        json!({"object_id": format!("obj_{}", "G".repeat(64)), "cursor": null, "limit": 1}),
        json!({"object_id": f.source.id, "cursor": null, "limit": 1, "author_id": f.source.author}),
    ] {
        let response: RpcResponseEnvelope = client
            .post(format!("{origin}/rpc"))
            .json(&envelope(input.clone()))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(
            response.error.unwrap().code,
            RpcErrorCode::InvalidInput,
            "{input}"
        );
    }
    let missing = format!("obj_{}", "0".repeat(64));
    let response: RpcResponseEnvelope = client
        .post(format!("{origin}/rpc"))
        .json(&envelope(json!({
            "object_id": missing, "cursor": null, "limit": 1
        })))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(response.error.unwrap().code, RpcErrorCode::NotFound);
    for query in [
        "limit=0",
        "limit=21",
        "limit=-1",
        "limit=1.5",
        "cursor=bad",
        "limit=1&unknown=x",
        "limit=1&limit=2",
    ] {
        assert_eq!(
            client
                .get(format!("{url}?{query}"))
                .send()
                .await
                .unwrap()
                .status(),
            400,
            "{query}"
        );
    }
    for (id, status) in [("invalid", 400), (&missing, 404)] {
        assert_eq!(
            client
                .get(format!("{origin}/objects/{id}/quotes"))
                .send()
                .await
                .unwrap()
                .status(),
            status
        );
    }
    assert_eq!(
        client
            .get(format!("{origin}/objects/{}/quotes", f.other.id))
            .query(&[("cursor", cursor)])
            .send()
            .await
            .unwrap()
            .status(),
        400
    );
    let catalog: babel_rpc::RpcCatalog = client
        .get(format!("{origin}/rpc/catalog"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let method = catalog
        .methods
        .iter()
        .find(|method| method.method.as_str() == "babel.social.quotes.list.v1")
        .unwrap();
    assert_eq!(method.idempotency, RpcIdempotency::ReadOnly);
    stop.send(()).unwrap();
    server.await.unwrap();

    let reopened = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    assert_eq!(
        full,
        reopened
            .list_quotes(&QuotesListQuery {
                object_id: f.source.id.clone(),
                cursor: None,
                limit: 20
            })
            .unwrap()
    );
}
