use babel_identity::IdentityKind;
use babel_judgment_local::LocalProvider;
use babel_lens::{
    BuiltInLens, LensStack, LensWeight, NativeRanker, RankingProvider, RankingProviderVersion,
    RankingRequest, RankingResult,
};
use babel_node::{DiscoveryQuery, FollowingQuery, LocalNode};
use babel_types::{Error, Result};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

#[derive(Clone, Copy)]
enum Behavior {
    Valid,
    Fails,
    Corrupt,
}

struct RecordingRanker {
    calls: Arc<Mutex<Vec<RankingRequest>>>,
    behavior: Behavior,
}

impl RankingProvider for RecordingRanker {
    fn version(&self) -> RankingProviderVersion {
        RankingProviderVersion {
            provider: "recording-test".into(),
            model: "lenses-v1".into(),
            version: "1".into(),
        }
    }

    fn rank(&self, request: &RankingRequest) -> Result<RankingResult> {
        self.calls.lock().unwrap().push(request.clone());
        if matches!(self.behavior, Behavior::Fails) {
            return Err(Error::ProviderUnavailable("test outage".into()));
        }
        let mut result = NativeRanker.rank(request)?;
        result.provider = self.version();
        if matches!(self.behavior, Behavior::Corrupt) {
            result.ranked[0].candidate.signals.relevance = 0.987654321;
        }
        Ok(result)
    }
}

struct Fixture {
    root: PathBuf,
    node: LocalNode<LocalProvider>,
    calls: Arc<Mutex<Vec<RankingRequest>>>,
}

impl Fixture {
    fn new(behavior: Behavior) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babel-ranker-{}-{}-{}",
            std::process::id(),
            time::OffsetDateTime::now_utc().unix_timestamp_nanos(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let node = LocalNode::open_with_ranker(
            &root,
            LocalProvider::default(),
            Box::new(RecordingRanker {
                calls: calls.clone(),
                behavior,
            }),
        )
        .unwrap();
        Self { root, node, calls }
    }

    fn publish(&mut self) {
        let author = self
            .node
            .create_identity(IdentityKind::Person, "author")
            .unwrap();
        for text in [
            "Public evidence from a dataset.",
            "A different public observation.",
            "A careful protocol explanation.",
        ] {
            self.node.publish_text(&author.id, text).unwrap();
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn query() -> DiscoveryQuery {
    DiscoveryQuery {
        anchors: vec![],
        search: None,
        followed_objects: BTreeSet::new(),
        limit: 2,
        exploration_slots: 1,
        lens: LensStack::new(
            "test-stack",
            vec![LensWeight {
                lens: BuiltInLens::Research,
                weight: 1.0,
            }],
        ),
    }
}

#[test]
fn discovery_invokes_configured_ranker_with_public_candidates_and_preserves_provenance() {
    let mut f = Fixture::new(Behavior::Valid);
    f.publish();
    let result = f.node.discover_objects(query()).unwrap();
    assert_eq!(result.ranking_provider.provider, "recording-test");
    assert_eq!(result.ranked.len(), 2);
    assert_eq!(result.objects.len(), result.ranked.len());
    for (object, ranked) in result.objects.iter().zip(&result.ranked) {
        assert_eq!(object.id, ranked.candidate.object_id);
    }
    let calls = f.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].candidates.len(), 3);
    let fields = serde_json::to_value(&calls[0]).unwrap();
    assert_eq!(fields.as_object().unwrap().len(), 4);
    assert!(fields.get("history").is_none());
    assert!(fields.get("viewer").is_none());
    assert!(fields.get("following").is_none());
}

#[test]
fn provider_failure_or_modified_input_is_not_replaced_by_native_ranking() {
    for behavior in [Behavior::Fails, Behavior::Corrupt] {
        let mut f = Fixture::new(behavior);
        f.publish();
        assert!(matches!(
            f.node.discover_objects(query()),
            Err(Error::ProviderUnavailable(_))
        ));
        assert_eq!(f.calls.lock().unwrap().len(), 1);
    }
}

#[test]
fn invalid_lenses_and_unbounded_queries_are_rejected_before_provider_work() {
    let mut f = Fixture::new(Behavior::Valid);
    let mut invalid = query();
    invalid.lens.weights[0].weight = f64::NAN;
    assert!(f.node.discover_objects(invalid).is_err());
    let mut invalid = query();
    invalid.search = Some("x".repeat(257));
    assert!(f.node.discover_objects(invalid).is_err());
    assert!(f.calls.lock().unwrap().is_empty());
}

#[test]
fn person_following_remains_chronological_without_invoking_public_ranker() {
    let mut f = Fixture::new(Behavior::Fails);
    let viewer = f
        .node
        .create_identity(IdentityKind::Person, "viewer")
        .unwrap();
    let author = f
        .node
        .create_identity(IdentityKind::Person, "author")
        .unwrap();
    let first = f.node.publish_text(&author.id, "First post").unwrap();
    let second = f.node.publish_text(&author.id, "Second post").unwrap();
    f.node
        .set_following(&viewer.id, &author.id, true, 0, "follow")
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
}
