use babel_crypto::Keypair;
use babel_discovery::{
    NativeTemporalScorer, TemporalProvider, TemporalProviderVersion, TemporalRequest,
    TemporalResult,
};
use babel_graph::{Edge, EdgeOrigin, Relation};
use babel_identity::{Identity, IdentityKind};
use babel_judgment::{Judgment, JudgmentProvider, JudgmentRequest, ProviderVersion};
use babel_judgment_local::LocalProvider;
use babel_lens::{
    CandidateSource, NativeRanker, RankingProvider, RankingProviderVersion, RankingRequest,
    RankingResult,
};
use babel_node::{DiscoveryQuery, ImportBundle, LocalNode, ObjectSearchQuery, moderation::*};
use babel_object::Object;
use babel_types::{Canonical, IdentityId, Result, Timestamp};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use time::{Duration, OffsetDateTime};

#[derive(Default)]
struct Calls {
    fail: AtomicBool,
    judgments: AtomicUsize,
    temporal: Mutex<Vec<TemporalRequest>>,
    ranking: Mutex<Vec<RankingRequest>>,
}
struct Judge(Arc<Calls>);
impl JudgmentProvider for Judge {
    fn version(&self) -> ProviderVersion {
        LocalProvider::default().version()
    }
    fn judge(&self, request: &JudgmentRequest) -> Result<Judgment> {
        self.0.judgments.fetch_add(1, Ordering::Relaxed);
        if self.0.fail.load(Ordering::Relaxed) {
            return Err(babel_types::Error::ProviderUnavailable(
                "injected publication failure".into(),
            ));
        }
        LocalProvider::default().judge(request)
    }
}
struct Temporal(Arc<Calls>);
impl TemporalProvider for Temporal {
    fn version(&self) -> TemporalProviderVersion {
        NativeTemporalScorer.version()
    }
    fn score(&self, request: &TemporalRequest) -> Result<TemporalResult> {
        self.0.temporal.lock().unwrap().push(request.clone());
        NativeTemporalScorer.score(request)
    }
}
struct Ranker(Arc<Calls>);
impl RankingProvider for Ranker {
    fn version(&self) -> RankingProviderVersion {
        NativeRanker.version()
    }
    fn rank(&self, request: &RankingRequest) -> Result<RankingResult> {
        self.0.ranking.lock().unwrap().push(request.clone());
        NativeRanker.rank(request)
    }
}
fn clock() -> Timestamp {
    Timestamp(OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap())
}
fn identity(seed: u8) -> (Identity, Keypair) {
    let key = Keypair::from_ed25519_secret_hex(&format!("{seed:02x}").repeat(32)).unwrap();
    let commitment = serde_json::json!({"kind": IdentityKind::Person, "handle": format!("author-{seed}"),
        "public_key": key.public_key(), "created_at": Timestamp(clock().0 - Duration::days(3000))});
    let id = Identity {
        id: IdentityId::from_hash(&commitment.canonical_hash().unwrap()),
        kind: IdentityKind::Person,
        handle: format!("author-{seed}"),
        public_key: key.public_key(),
        created_at: Timestamp(clock().0 - Duration::days(3000)),
        signature: key.sign(&commitment.canonical_bytes().unwrap()),
    };
    id.verify().unwrap();
    (id, key)
}
struct Fixture {
    root: PathBuf,
    node: LocalNode<Judge>,
    calls: Arc<Calls>,
    author: Identity,
    key: Keypair,
}
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "babel-indexed-discovery-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let calls = Arc::new(Calls::default());
        let mut node = open(&root, &calls);
        let (author, key) = identity(1);
        node.import_signing_identity(author.clone(), key.clone())
            .unwrap();
        Self {
            root,
            node,
            calls,
            author,
            key,
        }
    }
    fn object(&self, number: usize, text: &str) -> Object {
        let mut object = Object::text(&self.author, text).unwrap();
        object.created_at =
            Timestamp(clock().0 - Duration::days(100) + Duration::seconds(number as i64));
        object
            .with_relations(vec![])
            .unwrap()
            .sign(&self.author, &self.key)
            .unwrap()
    }
    fn corpus(&mut self, count: usize) -> Vec<Object> {
        let objects: Vec<_> = (0..count)
            .map(|n| self.object(n, &format!("public corpus item {n}")))
            .collect();
        self.node
            .import_bundle(ImportBundle {
                objects: objects.clone(),
                ..Default::default()
            })
            .unwrap();
        objects
    }
    fn edge(&self, from: &Object, to: &Object, relation: Relation) -> Edge {
        let mut edge = Edge::new(
            from.id.clone(),
            to.id.clone(),
            relation,
            EdgeOrigin::HumanAssertion,
            Some(self.author.id.clone()),
        )
        .unwrap();
        edge.created_at = clock();
        edge.with_metadata(BTreeMap::new())
            .unwrap()
            .sign(&self.author, &self.key)
            .unwrap()
    }
    fn discover(&mut self, query: DiscoveryQuery) -> RankingRequest {
        self.node.discover_objects_at(query, clock()).unwrap();
        self.calls.ranking.lock().unwrap().last().unwrap().clone()
    }
}
fn open(root: &PathBuf, calls: &Arc<Calls>) -> LocalNode<Judge> {
    LocalNode::open_with_algorithms(
        root,
        Judge(calls.clone()),
        Box::new(Ranker(calls.clone())),
        Box::new(Temporal(calls.clone())),
    )
    .unwrap()
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
fn query() -> DiscoveryQuery {
    DiscoveryQuery {
        limit: 200,
        exploration_slots: 7,
        ..Default::default()
    }
}
fn search(text: &str) -> ObjectSearchQuery {
    ObjectSearchQuery {
        query: Some(text.into()),
        author: None,
        kind: None,
        limit: 200,
    }
}

#[test]
fn expensive_work_is_constant_at_200_admitted_objects_for_256_and_1024_corpus() {
    for count in [256, 1024] {
        let mut f = Fixture::new();
        f.corpus(count);
        assert_eq!(f.calls.judgments.load(Ordering::Relaxed), 0);
        let ranking = f.discover(query());
        assert_eq!(ranking.candidates.len(), 200);
        assert_eq!(
            f.calls.judgments.load(Ordering::Relaxed),
            400,
            "exactly two judgments per admitted Object"
        );
        let temporal = f.calls.temporal.lock().unwrap();
        assert_eq!(temporal.len(), 1);
        assert_eq!(temporal[0].items.len(), 200);
        assert_eq!(
            temporal[0]
                .items
                .iter()
                .map(|x| &x.object_id)
                .collect::<BTreeSet<_>>(),
            ranking.candidates.iter().map(|x| &x.object_id).collect()
        );
        // One author owns the whole corpus: novelty must use count, not pool=200.
        for candidate in &ranking.candidates {
            assert!((candidate.signals.novelty - (0.35 / count as f64 + 0.15)).abs() < 1e-12);
        }
        eprintln!(
            "corpus={count} admitted=200 judgments=400 temporal_batches=1 temporal_items=200"
        );
    }
}

#[test]
fn old_sources_complete_overlap_provenance_recent_exploration_and_output_limit_independence() {
    let mut f = Fixture::new();
    let objects = f.corpus(512);
    let mut edges = vec![
        f.edge(&objects[1], &objects[0], Relation::EvidenceFor),
        f.edge(&objects[1], &objects[0], Relation::EvidenceAgainst),
        f.edge(&objects[1], &objects[0], Relation::References),
        f.edge(&objects[1], &objects[0], Relation::Follows),
        f.edge(&objects[2], &objects[0], Relation::EvidenceAgainst),
    ];
    // The hot target has no anchor/follow relationship and is older than 500
    // newer Objects. Only indexed inbound activity (or sampling) can admit it.
    for source in &objects[10..30] {
        edges.push(f.edge(source, &objects[3], Relation::ReplyTo));
    }
    for source in &objects[40..400] {
        edges.push(f.edge(source, &objects[0], Relation::EvidenceFor));
    }
    edges.push(f.edge(&objects[5], &objects[4], Relation::EvidenceFor));
    f.node
        .import_bundle(ImportBundle {
            edges: edges.clone(),
            ..Default::default()
        })
        .unwrap();
    let mut q = query();
    q.anchors = vec![
        objects[0].id.clone(),
        objects[0].id.clone(),
        objects[4].id.clone(),
    ];
    q.followed_objects.insert(objects[1].id.clone());
    let overflow = objects[40..400].iter().max_by_key(|o| &o.id).unwrap();
    q.followed_objects.insert(overflow.id.clone());
    let ranked = f.discover(q.clone());
    let selected: BTreeSet<_> = ranked
        .candidates
        .iter()
        .map(|c| c.object_id.clone())
        .collect();
    for n in [0, 1, 2, 3, 4, 5, 511] {
        assert!(selected.contains(&objects[n].id));
    }
    let beyond_prefix = ranked
        .candidates
        .iter()
        .find(|c| c.object_id == overflow.id)
        .unwrap();
    assert!(
        beyond_prefix
            .sources
            .iter()
            .any(|s| s.source == CandidateSource::Evidence)
    );
    let hot = ranked
        .candidates
        .iter()
        .find(|c| c.object_id == objects[3].id)
        .unwrap();
    assert!(
        hot.sources
            .iter()
            .any(|s| s.source == CandidateSource::SocialGraph)
    );
    let newest = ranked
        .candidates
        .iter()
        .find(|c| c.object_id == objects[511].id)
        .unwrap();
    assert!(
        newest
            .sources
            .iter()
            .any(|s| s.source == CandidateSource::Temporal)
    );
    let mut by_emergence = ranked.candidates.iter().collect::<Vec<_>>();
    by_emergence.sort_by(|a, b| {
        let score = |c: &babel_lens::Candidate| {
            0.55 * c.signals.temporal
                + 0.25 * c.signals.novelty
                + 0.20 * c.signals.reputation.research_score()
        };
        score(b)
            .total_cmp(&score(a))
            .then_with(|| b.created_at.cmp(&a.created_at))
            .then_with(|| a.object_id.cmp(&b.object_id))
    });
    let expected: BTreeSet<_> = by_emergence.iter().take(10).map(|c| &c.object_id).collect();
    let actual: BTreeSet<_> = ranked
        .candidates
        .iter()
        .filter(|c| {
            c.sources
                .iter()
                .any(|s| s.source == CandidateSource::Emerging)
        })
        .map(|c| &c.object_id)
        .collect();
    assert_eq!(actual, expected);
    let overlap = ranked
        .candidates
        .iter()
        .find(|c| c.object_id == objects[1].id)
        .unwrap();
    for source in [
        CandidateSource::Following,
        CandidateSource::Evidence,
        CandidateSource::Contradiction,
        CandidateSource::SemanticNeighborhood,
        CandidateSource::SocialGraph,
    ] {
        assert!(overlap.sources.iter().any(|s| s.source == source));
    }
    let explored = ranked
        .candidates
        .iter()
        .filter(|c| {
            c.sources
                .iter()
                .any(|s| s.source == CandidateSource::Exploration)
        })
        .count();
    assert!((7..=14).contains(&explored));
    q.limit = 9;
    assert_eq!(f.discover(q.clone()).candidates, ranked.candidates);
    f.node
        .import_bundle(ImportBundle {
            objects,
            edges,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(f.discover(q.clone()).candidates, ranked.candidates);
    f.node = open(&f.root, &f.calls);
    assert_eq!(f.discover(q).candidates, ranked.candidates);
}

#[test]
fn indexed_search_keeps_ascii_any_term_substrings_unicode_empty_and_strict_pool() {
    let mut f = Fixture::new();
    f.corpus(260);
    let extra: Vec<_> = [
        "Mixed ALPHA alpha beta",
        "alphabet betamax",
        "Caf\u{e9} \u{6771}\u{4eac}",
        "CAF\u{c9}",
    ]
    .iter()
    .enumerate()
    .map(|(n, t)| f.object(1000 + n, t))
    .collect();
    f.node
        .import_bundle(ImportBundle {
            objects: extra.clone(),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(
        f.node
            .search_objects(search("ALPHA beta"))
            .unwrap()
            .iter()
            .map(|r| r.object.id.clone())
            .collect::<Vec<_>>(),
        vec![extra[0].id.clone(), extra[1].id.clone()]
    );
    assert_eq!(
        f.node.search_objects(search("\u{6771}\u{4eac}")).unwrap()[0]
            .object
            .id,
        extra[2].id
    );
    assert_eq!(
        f.node.search_objects(search("caf\u{e9}")).unwrap()[0]
            .object
            .id,
        extra[2].id
    );
    assert_eq!(
        f.node.search_objects(search("caf\u{c9}")).unwrap()[0]
            .object
            .id,
        extra[3].id
    );
    let mut q = query();
    q.search = Some("ALPHA beta".into());
    q.anchors = vec![extra[3].id.clone()];
    q.followed_objects.insert(extra[2].id.clone());
    assert_eq!(f.discover(q.clone()).candidates.len(), 2);
    q.search = Some("absent_xyzzy".into());
    let before = f.calls.judgments.load(Ordering::Relaxed);
    let batches = f.calls.temporal.lock().unwrap().len();
    assert!(f.discover(q.clone()).candidates.is_empty());
    assert_eq!(before, f.calls.judgments.load(Ordering::Relaxed));
    assert_eq!(batches, f.calls.temporal.lock().unwrap().len());
    q.search = Some("  ".into());
    assert_eq!(f.discover(q).candidates.len(), 200);
}

#[test]
fn failed_import_and_publication_never_enter_indexes_and_retry_survives_restart() {
    let mut f = Fixture::new();
    let object = f.object(0, "durable searchable import");
    let path = f.root.join("objects").join(format!("{}.json", object.id));
    std::fs::create_dir(&path).unwrap();
    assert!(
        f.node
            .import_bundle(ImportBundle {
                objects: vec![object.clone()],
                ..Default::default()
            })
            .is_err()
    );
    assert!(f.node.object(&object.id).is_none());
    assert!(f.node.search_objects(search("durable")).unwrap().is_empty());
    std::fs::remove_dir(path).unwrap();
    f.node
        .import_bundle(ImportBundle {
            objects: vec![object.clone()],
            ..Default::default()
        })
        .unwrap();
    let edges_dir = f.root.join("edges");
    let edge = f.edge(&object, &object, Relation::Follows);
    let path = edges_dir.join(format!("{}.json", edge.id));
    std::fs::create_dir(&path).unwrap();
    assert!(
        f.node
            .import_bundle(ImportBundle {
                edges: vec![edge.clone()],
                ..Default::default()
            })
            .is_err()
    );
    assert!(f.node.edge(&edge.id).is_none());
    std::fs::remove_dir(path).unwrap();
    f.node
        .import_bundle(ImportBundle {
            edges: vec![edge],
            ..Default::default()
        })
        .unwrap();
    let published = f
        .node
        .publish_text(&f.author.id, "fresh publication indexed")
        .unwrap();
    assert_eq!(
        f.node
            .search_objects(search("publication indexed"))
            .unwrap()[0]
            .object
            .id,
        published.id
    );
    f.calls.fail.store(true, Ordering::Relaxed);
    assert!(
        f.node
            .publish_text(&f.author.id, "failedpublicationunique")
            .is_err()
    );
    assert!(
        f.node
            .search_objects(search("failedpublicationunique"))
            .unwrap()
            .is_empty()
    );
    f.calls.fail.store(false, Ordering::Relaxed);
    f.node = open(&f.root, &f.calls);
    assert_eq!(
        f.node.search_objects(search("durable")).unwrap()[0]
            .object
            .id,
        object.id
    );
    assert_eq!(
        f.node
            .search_objects(search("publication indexed"))
            .unwrap()[0]
            .object
            .id,
        published.id
    );
    assert!(
        f.node
            .search_objects(search("failedpublicationunique"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn restrictions_apply_before_slots_and_reversal_restores_indexed_sources() {
    let mut f = Fixture::new();
    let objects = f.corpus(210);
    let reporter = f
        .node
        .create_identity(IdentityKind::Person, "reporter")
        .unwrap();
    let first = f
        .node
        .create_identity(IdentityKind::Person, "first")
        .unwrap();
    let second = f
        .node
        .create_identity(IdentityKind::Person, "second")
        .unwrap();
    f.node
        .configure_moderators(&format!("{},{}", first.id, second.id))
        .unwrap();
    let mut cases = Vec::new();
    for object in &objects[200..] {
        let case = f
            .node
            .moderation_report(
                &reporter.id,
                ReportRequest {
                    object_id: object.id.clone(),
                    reason: ModerationReason::Fraud,
                    details: "Private reporter evidence for review".into(),
                    idempotency_key: object.id.to_string(),
                },
            )
            .unwrap();
        f.node
            .moderation_decide(
                &first.id,
                case.id.clone(),
                decision(ModerationOutcome::Restrict, 1, &object.id.to_string()),
            )
            .unwrap();
        cases.push(case);
    }
    let admitted = f.discover(query());
    assert_eq!(admitted.candidates.len(), 200);
    assert!(
        admitted
            .candidates
            .iter()
            .all(|c| objects[..200].iter().any(|o| o.id == c.object_id))
    );
    assert_eq!(f.calls.judgments.load(Ordering::Relaxed), 400);
    for c in &admitted.candidates {
        assert!((c.signals.novelty - (0.35 / 200.0 + 0.15)).abs() < 1e-12);
    }
    let case = &cases[9];
    f.node
        .moderation_appeal(
            &f.author.id,
            case.id.clone(),
            AppealRequest {
                details: "Detailed appeal for independent review".into(),
                expected_revision: 2,
                idempotency_key: "appeal".into(),
            },
        )
        .unwrap();
    f.node
        .moderation_decide(
            &second.id,
            case.id.clone(),
            decision(ModerationOutcome::NoAction, 3, "reverse"),
        )
        .unwrap();
    let mut q = query();
    q.followed_objects.insert(objects[209].id.clone());
    assert!(
        f.discover(q.clone())
            .candidates
            .iter()
            .any(|c| c.object_id == objects[209].id)
    );
    f.node = open(&f.root, &f.calls);
    assert!(
        f.discover(q)
            .candidates
            .iter()
            .any(|c| c.object_id == objects[209].id)
    );
}
fn decision(outcome: ModerationOutcome, revision: u64, key: &str) -> DecisionRequest {
    DecisionRequest {
        outcome,
        reason: ModerationReason::Fraud,
        explanation: "Reviewed evidence against the integrity policy".into(),
        policy_version: POLICY.into(),
        source_signals: vec![],
        expected_revision: revision,
        idempotency_key: key.into(),
    }
}

#[test]
fn roots_are_bounded_and_empty_store_never_calls_enrichment() {
    let mut f = Fixture::new();
    assert!(f.discover(query()).candidates.is_empty());
    let mut q = query();
    q.anchors = (0..65)
        .map(|n| babel_types::ObjectId::new_unchecked(format!("obj_{n:064x}")))
        .collect();
    assert!(f.node.discover_objects_at(q, clock()).is_err());
    let mut q = query();
    q.followed_objects = (0..201)
        .map(|n| babel_types::ObjectId::new_unchecked(format!("obj_{n:064x}")))
        .collect();
    assert!(f.node.discover_objects_at(q, clock()).is_err());
    assert_eq!(f.calls.judgments.load(Ordering::Relaxed), 0);
    assert!(f.calls.temporal.lock().unwrap().is_empty());
    assert_eq!(f.calls.ranking.lock().unwrap().len(), 1);
}
