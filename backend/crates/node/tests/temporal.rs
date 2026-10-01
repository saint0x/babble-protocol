use babble_crypto::Keypair;
use babble_discovery::{
    NativeTemporalScorer, TemporalClass, TemporalProvider, TemporalProviderVersion,
    TemporalRequest, TemporalResult,
};
use babble_graph::{Edge, EdgeOrigin, Relation};
use babble_identity::{Identity, IdentityKind};
use babble_judgment_local::LocalProvider;
use babble_lens::{
    NativeRanker, RankingProvider, RankingProviderVersion, RankingRequest, RankingResult,
};
use babble_node::{DiscoveryQuery, DiscoveryResult, FollowingQuery, ImportBundle, LocalNode};
use babble_object::Object;
use babble_types::{Error, Result, Timestamp};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
use time::{Duration, OffsetDateTime};

#[derive(Clone, Copy, Debug)]
enum Behavior {
    Native,
    Fixed(f64),
    Fails,
    Missing,
    Duplicate,
    Reordered,
    Foreign,
    WrongProvider,
    WrongTime,
    WrongAge,
    Nan,
    OutOfBounds,
    FailSecondBatch,
}

struct RecordingTemporal {
    calls: Arc<Mutex<Vec<TemporalRequest>>>,
    behavior: Arc<Mutex<Behavior>>,
}

impl TemporalProvider for RecordingTemporal {
    fn version(&self) -> TemporalProviderVersion {
        TemporalProviderVersion {
            provider: "recording-temporal".into(),
            model: "temporal-v1".into(),
            version: "1".into(),
        }
    }

    fn score(&self, request: &TemporalRequest) -> Result<TemporalResult> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(request.clone());
        let behavior = *self.behavior.lock().unwrap();
        if matches!(behavior, Behavior::Fails)
            || matches!(behavior, Behavior::FailSecondBatch) && calls.len() == 2
        {
            return Err(Error::ProviderUnavailable("temporal test outage".into()));
        }
        let mut result = NativeTemporalScorer.score(request)?;
        result.provider = self.version();
        match behavior {
            Behavior::Fixed(value) => result
                .scores
                .iter_mut()
                .for_each(|s| s.survival_score = value),
            Behavior::Missing => {
                result.scores.pop();
            }
            Behavior::Duplicate => result.scores[1] = result.scores[0].clone(),
            Behavior::Reordered => result.scores.reverse(),
            Behavior::Foreign => {
                result.scores[0].object_id =
                    babble_types::ObjectId::new_unchecked(format!("obj_{}", "f".repeat(64)))
            }
            Behavior::WrongProvider => result.provider.version = "999".into(),
            Behavior::WrongTime => {
                result.reference_time = Timestamp(request.reference_time.0 + Duration::seconds(1))
            }
            Behavior::WrongAge => result.scores[0].age_hours += 1.0,
            Behavior::Nan => result.scores[0].survival_score = f64::NAN,
            Behavior::OutOfBounds => result.scores[0].engagement_velocity = 1.1,
            _ => {}
        }
        Ok(result)
    }
}

struct RecordingRanker(Arc<Mutex<Vec<RankingRequest>>>);
impl RankingProvider for RecordingRanker {
    fn version(&self) -> RankingProviderVersion {
        NativeRanker.version()
    }
    fn rank(&self, request: &RankingRequest) -> Result<RankingResult> {
        self.0.lock().unwrap().push(request.clone());
        NativeRanker.rank(request)
    }
}

struct Fixture {
    root: PathBuf,
    node: LocalNode<LocalProvider>,
    author: Identity,
    key: Keypair,
    calls: Arc<Mutex<Vec<TemporalRequest>>>,
    ranks: Arc<Mutex<Vec<RankingRequest>>>,
    behavior: Arc<Mutex<Behavior>>,
}

impl Fixture {
    fn new(behavior: Behavior) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babble-temporal-node-{}-{}-{}",
            std::process::id(),
            OffsetDateTime::now_utc().unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let ranks = Arc::new(Mutex::new(Vec::new()));
        let behavior = Arc::new(Mutex::new(behavior));
        let mut node = LocalNode::open_with_algorithms(
            &root,
            LocalProvider::default(),
            Box::new(RecordingRanker(ranks.clone())),
            Box::new(RecordingTemporal {
                calls: calls.clone(),
                behavior: behavior.clone(),
            }),
        )
        .unwrap();
        let key = Keypair::generate();
        let author = Identity::create(IdentityKind::Person, "temporal-author", &key).unwrap();
        node.import_signing_identity(author.clone(), key.clone())
            .unwrap();
        Self {
            root,
            node,
            author,
            key,
            calls,
            ranks,
            behavior,
        }
    }

    fn object(&mut self, text: &str, at: Timestamp) -> Object {
        let mut object = Object::text(&self.author, text).unwrap();
        object.created_at = at;
        let object = object
            .with_relations(vec![])
            .unwrap()
            .sign(&self.author, &self.key)
            .unwrap();
        self.node
            .import_bundle(ImportBundle {
                objects: vec![object.clone()],
                ..Default::default()
            })
            .unwrap();
        object
    }

    fn edge(
        &self,
        source: &Object,
        target: &Object,
        relation: Relation,
        origin: EdgeOrigin,
        at: Timestamp,
    ) -> Edge {
        let mut edge = Edge::new(
            source.id.clone(),
            target.id.clone(),
            relation,
            origin,
            Some(self.author.id.clone()),
        )
        .unwrap();
        edge.created_at = at;
        edge.with_metadata(BTreeMap::new())
            .unwrap()
            .sign(&self.author, &self.key)
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn clock() -> Timestamp {
    Timestamp(OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap())
}
fn query(limit: usize) -> DiscoveryQuery {
    DiscoveryQuery {
        anchors: vec![],
        search: None,
        followed_objects: BTreeSet::new(),
        limit,
        exploration_slots: 1,
        lens: babble_lens::LensStack::new(
            "temporal-test",
            vec![babble_lens::LensWeight {
                lens: babble_lens::BuiltInLens::Research,
                weight: 1.0,
            }],
        ),
    }
}

fn assert_mapping(result: &DiscoveryResult) {
    assert_eq!(result.objects.len(), result.ranked.len());
    assert_eq!(result.temporal.scores.len(), result.ranked.len());
    for ((object, ranked), score) in result
        .objects
        .iter()
        .zip(&result.ranked)
        .zip(&result.temporal.scores)
    {
        assert_eq!(object.id, ranked.candidate.object_id);
        assert_eq!(score.object_id, object.id);
        assert_eq!(ranked.candidate.signals.temporal, score.survival_score);
    }
}

#[test]
fn native_clock_advances_without_publication_and_explicit_replay_survives_reopen() {
    let mut f = Fixture::new(Behavior::Native);
    f.object("Breaking news today", clock());
    f.object("Evergreen reference documentation", clock());
    f.node = LocalNode::open(&f.root, LocalProvider::default()).unwrap();
    let early_time = Timestamp(clock().0 + Duration::hours(1));
    let early = f.node.discover_objects_at(query(2), early_time).unwrap();
    let late = f
        .node
        .discover_objects_at(query(2), Timestamp(clock().0 + Duration::days(31)))
        .unwrap();
    assert_eq!(early.temporal.provider, NativeTemporalScorer.version());
    assert_mapping(&early);
    assert_mapping(&late);
    for first in &early.temporal.scores {
        let last = late
            .temporal
            .scores
            .iter()
            .find(|s| s.object_id == first.object_id)
            .unwrap();
        assert_eq!(first.age_hours, 1.0);
        assert_eq!(last.age_hours, 744.0);
        assert!(last.survival_score < first.survival_score);
        let before = early
            .ranked
            .iter()
            .find(|r| r.candidate.object_id == first.object_id)
            .unwrap();
        let after = late
            .ranked
            .iter()
            .find(|r| r.candidate.object_id == first.object_id)
            .unwrap();
        assert!(after.candidate.signals.exploration < before.candidate.signals.exploration);
    }
    f.node = LocalNode::open_with_ranker(&f.root, LocalProvider::default(), Box::new(NativeRanker))
        .unwrap();
    let replay = f.node.discover_objects_at(query(2), early_time).unwrap();
    assert_eq!(
        serde_json::to_value(&replay).unwrap(),
        serde_json::to_value(&early).unwrap()
    );
    assert_eq!(f.node.store().list_objects().unwrap().len(), 2);
}

#[test]
fn default_discovery_uses_the_evaluation_clock_not_the_newest_publication() {
    let mut f = Fixture::new(Behavior::Native);
    f.object(
        "Old discussion",
        Timestamp(clock().0 - Duration::days(3650)),
    );
    let before = Timestamp::now();
    let result = f.node.discover_objects(query(1)).unwrap();
    let after = Timestamp::now();
    assert!(result.temporal.reference_time >= before && result.temporal.reference_time <= after);
    assert_eq!(
        f.calls.lock().unwrap()[0].reference_time,
        result.temporal.reference_time
    );
    assert!(result.temporal.scores[0].age_hours > 24.0);
}

#[test]
fn explicit_search_only_scores_and_ranks_matches_without_unrelated_fallback() {
    let mut f = Fixture::new(Behavior::Native);
    let expected = f.object("zyxqvtemporalmarker82719", clock());
    f.object(
        "Research evidence analysis dataset reference tutorial",
        clock(),
    );
    let mut search = query(9);
    search.search = Some("zyxqvtemporalmarker82719".into());
    let result = f.node.discover_objects_at(search.clone(), clock()).unwrap();
    assert_eq!(result.objects.len(), 1);
    assert_eq!(result.objects[0].id, expected.id);
    assert_mapping(&result);
    let calls = f.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].items.len(), 1);
    assert_eq!(calls[0].items[0].object_id, expected.id);
    drop(calls);
    search.search = Some("zyxqvabsent82720".into());
    let result = f.node.discover_objects_at(search.clone(), clock()).unwrap();
    assert!(result.objects.is_empty());
    assert!(result.temporal.scores.is_empty());
    assert_eq!(f.calls.lock().unwrap().len(), 1);
    search.search = Some("  ".into());
    assert_eq!(
        f.node
            .discover_objects_at(search, clock())
            .unwrap()
            .objects
            .len(),
        2
    );
}

#[test]
fn admits_before_temporal_work_and_returns_selected_scores_in_rank_order() {
    let mut f = Fixture::new(Behavior::Fixed(0.271828));
    let mut expected = BTreeSet::new();
    for n in 0..201 {
        expected.insert(f.object(&format!("Public discussion {n}"), clock()).id);
    }
    let result = f.node.discover_objects_at(query(3), clock()).unwrap();
    assert_eq!(result.ranked.len(), 3);
    assert_eq!(result.temporal.provider.provider, "recording-temporal");
    assert_mapping(&result);
    assert!(
        result
            .temporal
            .scores
            .iter()
            .all(|s| s.survival_score == 0.271828)
    );
    let calls = f.calls.lock().unwrap();
    assert_eq!(
        calls.iter().map(|r| r.items.len()).collect::<Vec<_>>(),
        vec![200]
    );
    let mut seen = BTreeSet::new();
    for call in calls.iter() {
        call.validate().unwrap();
        assert_eq!(call.reference_time, clock());
        for item in &call.items {
            assert!(seen.insert(item.object_id.clone()));
        }
    }
    assert_eq!(seen.len(), 200);
    assert!(seen.is_subset(&expected));
    let ranks = f.ranks.lock().unwrap();
    assert_eq!(ranks.len(), 1);
    assert!(
        ranks[0]
            .candidates
            .iter()
            .all(|c| c.signals.temporal == 0.271828)
    );
}

#[test]
fn configured_survival_score_changes_exploration_as_well_as_the_temporal_signal() {
    let mut f = Fixture::new(Behavior::Fixed(0.1));
    f.object("Public discussion", clock());
    let low = f.node.discover_objects_at(query(1), clock()).unwrap();
    *f.behavior.lock().unwrap() = Behavior::Fixed(0.9);
    let high = f.node.discover_objects_at(query(1), clock()).unwrap();
    assert_mapping(&low);
    assert_mapping(&high);
    assert_eq!(low.ranked[0].candidate.signals.temporal, 0.1);
    assert_eq!(high.ranked[0].candidate.signals.temporal, 0.9);
    assert!(
        high.ranked[0].candidate.signals.exploration > low.ranked[0].candidate.signals.exploration
    );
}

#[test]
fn signed_public_activity_and_content_mapping_exclude_private_and_ineligible_signals() {
    let mut f = Fixture::new(Behavior::Native);
    let target = f.object(
        "Breaking time-sensitive news",
        Timestamp(clock().0 - Duration::days(30)),
    );
    let source = f.object(
        "Evergreen API reference",
        Timestamp(clock().0 - Duration::days(29)),
    );
    let tutorial = f.object("How to build a tutorial", clock());
    let analysis = f.object("Dataset analysis methodology", clock());
    let discussion = f.object("A public discussion", clock());
    let recent = f.edge(
        &source,
        &target,
        Relation::ReplyTo,
        EdgeOrigin::HumanAssertion,
        Timestamp(clock().0 - Duration::days(1)),
    );
    let edges = vec![
        recent.clone(),
        recent,
        f.edge(
            &source,
            &target,
            Relation::Cites,
            EdgeOrigin::HumanAssertion,
            Timestamp(clock().0 - Duration::days(8)),
        ),
        f.edge(
            &source,
            &target,
            Relation::Quotes,
            EdgeOrigin::ApplicationAssertion,
            Timestamp(clock().0 - Duration::days(7)),
        ),
        f.edge(
            &target,
            &target,
            Relation::References,
            EdgeOrigin::HumanAssertion,
            clock(),
        ),
        f.edge(
            &source,
            &target,
            Relation::Follows,
            EdgeOrigin::HumanAssertion,
            clock(),
        ),
        f.edge(
            &source,
            &target,
            Relation::Supports,
            EdgeOrigin::JudgmentDerived,
            clock(),
        ),
        f.edge(
            &source,
            &target,
            Relation::Supports,
            EdgeOrigin::ConsensusDerived,
            clock(),
        ),
        f.edge(
            &source,
            &target,
            Relation::Quotes,
            EdgeOrigin::HumanAssertion,
            Timestamp(clock().0 + Duration::seconds(1)),
        ),
        f.edge(
            &source,
            &target,
            Relation::References,
            EdgeOrigin::HumanAssertion,
            Timestamp(clock().0 - Duration::days(31)),
        ),
        f.edge(
            &target,
            &source,
            Relation::ReplyTo,
            EdgeOrigin::HumanAssertion,
            clock(),
        ),
    ];
    f.node
        .import_bundle(ImportBundle {
            edges,
            ..Default::default()
        })
        .unwrap();
    let mut unsigned = f.edge(
        &source,
        &target,
        Relation::Extends,
        EdgeOrigin::HumanAssertion,
        clock(),
    );
    unsigned.signature = None;
    assert!(
        f.node
            .import_bundle(ImportBundle {
                edges: vec![unsigned],
                ..Default::default()
            })
            .is_err()
    );
    let viewer = f
        .node
        .create_identity(IdentityKind::Person, "private-viewer-marker")
        .unwrap();
    f.node
        .set_following(&viewer.id, &f.author.id, true, 0, "private-follow-marker")
        .unwrap();
    let first = f.node.discover_objects_at(query(5), clock()).unwrap();
    f.node
        .set_following(
            &viewer.id,
            &f.author.id,
            false,
            1,
            "private-unfollow-marker",
        )
        .unwrap();
    let second = f.node.discover_objects_at(query(5), clock()).unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(second).unwrap()
    );
    let calls = f.calls.lock().unwrap();
    assert_eq!(calls[0], calls[1]);
    let input = &calls[0];
    for (id, class) in [
        (&target.id, TemporalClass::News),
        (&source.id, TemporalClass::Reference),
        (&tutorial.id, TemporalClass::Tutorial),
        (&analysis.id, TemporalClass::Analysis),
        (&discussion.id, TemporalClass::Discussion),
    ] {
        let item = input.items.iter().find(|i| &i.object_id == id).unwrap();
        assert_eq!(item.content_class, class);
        assert_eq!(item.published_at, f.node.object(id).unwrap().created_at);
        assert!((0.0..=1.0).contains(&item.quality_score));
        assert_eq!(
            (item.engagement.total_views, item.engagement.recent_views),
            (0, 0)
        );
    }
    let item = input
        .items
        .iter()
        .find(|i| i.object_id == target.id)
        .unwrap();
    assert_eq!(item.tags, vec!["breaking", "time-sensitive"]);
    assert_eq!(item.engagement.total_interactions, 3);
    assert_eq!(item.engagement.recent_interactions, 2);
    let wire = serde_json::to_value(input).unwrap();
    assert_eq!(
        wire.as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["items", "reference_time"])
    );
    for item in wire["items"].as_array().unwrap() {
        assert_eq!(
            item.as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([
                "object_id",
                "published_at",
                "content_class",
                "quality_score",
                "tags",
                "engagement"
            ])
        );
    }
    let text = wire.to_string();
    for forbidden in [
        viewer.id.as_str(),
        "private-viewer-marker",
        "private-follow-marker",
        "telemetry",
        "history",
        "password",
    ] {
        assert!(
            !text.contains(forbidden),
            "unexpected private field: {forbidden}"
        );
    }
}

#[test]
fn failures_and_malformed_temporal_results_never_reach_ranking_or_native_fallback() {
    for behavior in [
        Behavior::Fails,
        Behavior::Missing,
        Behavior::Duplicate,
        Behavior::Reordered,
        Behavior::Foreign,
        Behavior::WrongProvider,
        Behavior::WrongTime,
        Behavior::WrongAge,
        Behavior::Nan,
        Behavior::OutOfBounds,
    ] {
        let mut f = Fixture::new(behavior);
        f.object("First discussion", clock());
        f.object("Second discussion", clock());
        assert!(
            matches!(
                f.node.discover_objects_at(query(2), clock()),
                Err(Error::ProviderUnavailable(_))
            ),
            "{behavior:?}"
        );
        assert_eq!(f.calls.lock().unwrap().len(), 1);
        assert!(f.ranks.lock().unwrap().is_empty(), "{behavior:?}");
    }
}

#[test]
fn corpus_over_budget_never_reaches_a_second_temporal_batch() {
    let mut f = Fixture::new(Behavior::FailSecondBatch);
    for n in 0..201 {
        f.object(&format!("Public discussion {n}"), clock());
    }
    assert!(f.node.discover_objects_at(query(2), clock()).is_ok());
    assert_eq!(f.calls.lock().unwrap().len(), 1);
    assert_eq!(f.ranks.lock().unwrap().len(), 1);
}

#[test]
fn private_person_following_is_chronological_and_bypasses_temporal_provider() {
    let mut f = Fixture::new(Behavior::Fails);
    let first = f.object("Older discussion", clock());
    let second = f.object(
        "Newer discussion",
        Timestamp(clock().0 + Duration::seconds(1)),
    );
    let viewer = f
        .node
        .create_identity(IdentityKind::Person, "viewer")
        .unwrap();
    f.node
        .set_following(&viewer.id, &f.author.id, true, 0, "follow")
        .unwrap();
    let page = f
        .node
        .following_feed(
            &viewer.id,
            &FollowingQuery {
                limit: 20,
                cursor: None,
                search: None,
            },
        )
        .unwrap();
    assert_eq!(page.objects, vec![second, first]);
    assert!(f.calls.lock().unwrap().is_empty());
    assert!(f.ranks.lock().unwrap().is_empty());
}
