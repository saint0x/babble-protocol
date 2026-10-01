use super::*;
use crate::ImportBundle;
use babble_crypto::Keypair;
use babble_graph::Edge;
use babble_identity::Identity;
use babble_judgment::{ConstantProvider, Judgment, JudgmentRequest, ProviderVersion};
use babble_judgment_local::LocalProvider;
use babble_object::ObjectKind;
use babble_store::{ObjectJudgmentInput, PublicationBatch};
use babble_types::{Canonical, IdentityId};
use std::{
    cell::RefCell,
    collections::BTreeSet,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use time::{Duration, OffsetDateTime};

#[derive(Default)]
struct RecordingLocal {
    local: LocalProvider,
    calls: RefCell<Vec<JudgmentRequest>>,
    deadlines: RefCell<Vec<Instant>>,
    expire_on: RefCell<Option<DefinitionId>>,
}

impl JudgmentProvider for RecordingLocal {
    fn version(&self) -> ProviderVersion {
        self.local.version()
    }
    fn supported_definitions(&self) -> Vec<DefinitionId> {
        self.local.supported_definitions()
    }
    fn privacy_policy(&self) -> babble_judgment::JudgmentPrivacyPolicy {
        let mut policy = self.local.privacy_policy();
        policy
            .allowed_context_keys
            .insert("source_agreement".into());
        policy
    }
    fn judge(&self, request: &JudgmentRequest) -> Result<Judgment> {
        self.calls.borrow_mut().push(request.clone());
        self.local.judge(request)
    }
    fn judge_before(&self, request: &JudgmentRequest, deadline: Instant) -> Result<Judgment> {
        self.deadlines.borrow_mut().push(deadline);
        let result = self.judge(request);
        if self.expire_on.borrow().as_ref() == Some(&request.definition) {
            std::thread::sleep(deadline.saturating_duration_since(Instant::now()));
        }
        result
    }
}

fn at(seconds: i64) -> Timestamp {
    Timestamp(OffsetDateTime::from_unix_timestamp(1_800_000_000 + seconds).unwrap())
}

struct Fixture {
    root: PathBuf,
    node: LocalNode<RecordingLocal>,
    author: Identity,
    key: Keypair,
    claim: Object,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "babble-source-agreement-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let mut node = LocalNode::open(&root, RecordingLocal::default()).unwrap();
        let key = Keypair::from_ed25519_secret_hex(&"42".repeat(32)).unwrap();
        // Freeze the signed identity commitment as well as Object/Edge timestamps.
        let commitment = json!({"kind": "Person", "handle": "agreement-author",
            "public_key": key.public_key(), "created_at": at(-1000)});
        let mut identity = commitment.clone();
        identity["id"] = json!(IdentityId::from_hash(&commitment.canonical_hash().unwrap()));
        identity["signature"] = json!(key.sign(&commitment.canonical_bytes().unwrap()));
        let author: Identity = serde_json::from_value(identity).unwrap();
        author.verify().unwrap();
        let claim = Self::signed_object(&author, &key, "The claim being compared", at(0));
        node.import_bundle(ImportBundle {
            identities: vec![author.clone()],
            objects: vec![claim.clone()],
            ..Default::default()
        })
        .unwrap();
        Self {
            root,
            node,
            author,
            key,
            claim,
        }
    }

    fn signed_object(author: &Identity, key: &Keypair, text: &str, time: Timestamp) -> Object {
        let mut object = Object::text(author, text).unwrap();
        object.created_at = time;
        object
            .with_relations(vec![])
            .unwrap()
            .sign(author, key)
            .unwrap()
    }

    fn object(&self, text: &str, time: Timestamp) -> Object {
        Self::signed_object(&self.author, &self.key, text, time)
    }

    fn custom_object(&self, payload: Value, time: Timestamp) -> Object {
        let mut object = Object::create(
            &self.author,
            ObjectKind::new("test.agreement"),
            "test.agreement.v1",
            payload,
        )
        .unwrap();
        object.created_at = time;
        object
            .with_relations(vec![])
            .unwrap()
            .sign(&self.author, &self.key)
            .unwrap()
    }

    fn edge(
        &self,
        source: &Object,
        relation: Relation,
        origin: EdgeOrigin,
        time: Timestamp,
    ) -> Edge {
        let mut edge = Edge::new(
            source.id.clone(),
            self.claim.id.clone(),
            relation,
            origin,
            Some(self.author.id.clone()),
        )
        .unwrap();
        edge.created_at = time;
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

    fn add(&mut self, text: &str, time: Timestamp) -> Object {
        let object = self.object(text, time);
        let edge = self.edge(
            &object,
            Relation::Supports,
            EdgeOrigin::HumanAssertion,
            at(10),
        );
        self.import(vec![object.clone()], vec![edge]);
        object
    }

    fn state(&self, time: Timestamp) -> JudgmentState {
        self.node
            .source_agreement_state_at(&self.claim, time)
            .unwrap()
    }

    fn assert_limit_without_scoring(&self) {
        self.node.judgment_provider.calls.borrow_mut().clear();
        let error = self
            .node
            .source_agreement_state_at(&self.claim, at(10))
            .unwrap_err();
        assert!(
            matches!(error, Error::Conflict(ref message) if message.contains("evaluation limits")),
            "{error}"
        );
        assert!(self.node.judgment_provider.calls.borrow().is_empty());
        assert!(self.node.store.list_judgments().unwrap().is_empty());
    }

    fn remember(
        &self,
        state: JudgmentState,
        score: f64,
        created: Timestamp,
        version: ProviderVersion,
    ) -> Judgment {
        let input = agreement(&state);
        let ids: Vec<_> = sources(&state)
            .iter()
            .map(|s| s["source_id"].clone())
            .collect();
        let contributions: BTreeMap<_, _> = sources(&state)
            .iter()
            .filter(|source| !source["vote"].is_null())
            .filter_map(|source| source["user_id"].as_str().map(|user| (user, 0.0)))
            .collect();
        // Seed validated persisted history; LocalProvider does not implement aggregate scoring.
        let provider = ConstantProvider::new(
            version,
            json!({
                "kind": "source_agreement", "confidence": 0.0, "confidence_status": "uncalibrated",
                "reference_time": input["reference_time"], "source_ids": ids,
                "content_id": self.claim.id, "consensus_score": score, "reliability_score": 0.0,
                "validation_count": ids.len(), "state": "insufficient", "temporal_weight": 0.0,
                "term_agreement": 0.0, "fact_agreement": 0.0, "user_contributions": contributions,
                "limitations": ["Persisted history fixture; aggregate scoring is not exercised"]
            }),
            0.0,
        );
        let request = JudgmentRequest {
            definition: DefinitionId::source_agreement_v1(),
            state,
            parameters: BTreeMap::new(),
        };
        let mut judgment = provider.judge(&request).unwrap();
        judgment.created_at = created;
        let input = ObjectJudgmentInput {
            object_id: self.claim.id.clone(),
            judgment_id: judgment.id.clone(),
            request,
        };
        let mut batch = PublicationBatch::new();
        batch.object_judgment(&input, &judgment).unwrap();
        self.node.store.commit_publication(batch).unwrap();
        judgment
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn agreement(state: &JudgmentState) -> &Value {
    &state.context["source_agreement"]
}
fn sources(state: &JudgmentState) -> &[Value] {
    agreement(state)["sources"].as_array().unwrap()
}

#[test]
fn source_count_accepts_200_and_rejects_201_before_scoring() {
    let mut f = Fixture::new();
    let objects: Vec<_> = (0..200)
        .map(|i| f.object(&format!("Independent source {i}"), at(1)))
        .collect();
    let edges = objects
        .iter()
        .map(|o| f.edge(o, Relation::Supports, EdgeOrigin::HumanAssertion, at(10)))
        .collect();
    f.import(objects, edges);
    assert!(f.node.judgment_provider.calls.borrow().is_empty());
    assert_eq!(sources(&f.state(at(10))).len(), 200);
    assert_eq!(f.node.judgment_provider.calls.borrow().len(), 400);
    f.add("Source 201", at(1));
    f.assert_limit_without_scoring();
}

#[test]
fn per_source_limit_counts_utf8_bytes_and_accepts_exactly_64_kib() {
    let mut f = Fixture::new();
    let text = "\u{00e9}".repeat(32 * 1024);
    f.add(&text, at(1));
    assert_eq!(
        sources(&f.state(at(10)))[0]["text"].as_str().unwrap().len(),
        64 * 1024
    );
    f.add(&(text + "x"), at(1));
    f.assert_limit_without_scoring();
}

#[test]
fn total_limit_accepts_512_kib_and_rejects_one_more_byte() {
    let mut f = Fixture::new();
    for i in 0..8 {
        f.add(&format!("{i}{}", "x".repeat(64 * 1024 - 1)), at(1));
    }
    let state = f.state(at(10));
    assert_eq!(sources(&state).len(), 8);
    assert_eq!(
        sources(&state)
            .iter()
            .map(|s| s["text"].as_str().unwrap().len())
            .sum::<usize>(),
        512 * 1024
    );
    f.add("z", at(1));
    f.assert_limit_without_scoring();
}

#[test]
fn exact_copies_are_collapsed_before_count_and_byte_limits() {
    let mut f = Fixture::new();
    let text = "x".repeat(64 * 1024);
    let objects: Vec<_> = (0..201)
        .map(|i| f.object(&text, Timestamp(at(1).0 + Duration::milliseconds(i))))
        .collect();
    let edges = objects
        .iter()
        .map(|o| f.edge(o, Relation::Supports, EdgeOrigin::HumanAssertion, at(10)))
        .collect();
    f.import(objects, edges);
    assert_eq!(sources(&f.state(at(10))).len(), 1);
    assert_eq!(f.node.judgment_provider.calls.borrow().len(), 2);
}

#[test]
fn temporal_selection_requires_claim_source_and_edge_to_exist_at_reference() {
    let mut f = Fixture::new();
    let cases = [
        ("old source is eligible", -50, 0, true),
        ("source equals edge", 5, 5, true),
        ("source and edge equal reference", 10, 10, true),
        ("edge predates claim", -5, -1, false),
        ("future edge", 2, 11, false),
        ("future source", 11, 10, false),
        ("source postdates edge", 5, 4, false),
    ];
    let mut expected = BTreeSet::new();
    for (text, source_at, edge_at, eligible) in cases {
        let object = f.object(text, at(source_at));
        let edge = f.edge(
            &object,
            Relation::Supports,
            EdgeOrigin::HumanAssertion,
            at(edge_at),
        );
        if eligible {
            expected.insert(object.id.to_string());
        }
        f.import(vec![object], vec![edge]);
    }
    let state = f.state(at(10));
    assert_eq!(
        sources(&state)
            .iter()
            .map(|s| s["source_id"].as_str().unwrap().to_owned())
            .collect::<BTreeSet<_>>(),
        expected
    );
    assert_eq!(agreement(&state)["reference_time"], json!(seconds(at(10))));
    assert_eq!(f.node.judgment_provider.calls.borrow().len(), 6);
    f.node.judgment_provider.calls.borrow_mut().clear();
    assert!(matches!(
        f.node.source_agreement_state_at(&f.claim, at(-1)),
        Err(Error::Conflict(_))
    ));
    assert!(f.node.judgment_provider.calls.borrow().is_empty());
}

#[test]
fn only_asserted_incoming_evidence_and_context_relations_are_selected() {
    let mut f = Fixture::new();
    let relations = [
        Relation::Supports,
        Relation::Contradicts,
        Relation::EvidenceFor,
        Relation::EvidenceAgainst,
        Relation::References,
        Relation::Cites,
    ];
    for (i, relation) in relations.into_iter().enumerate() {
        let source = f.object(&format!("source-{i}"), at(1));
        let origin = if i % 2 == 0 {
            EdgeOrigin::HumanAssertion
        } else {
            EdgeOrigin::ApplicationAssertion
        };
        let edge = f.edge(&source, relation, origin, at(10));
        f.import(vec![source], vec![edge]);
    }
    for (i, (relation, origin)) in [
        (Relation::Supports, EdgeOrigin::JudgmentDerived),
        (Relation::Supports, EdgeOrigin::ConsensusDerived),
        (Relation::ReplyTo, EdgeOrigin::HumanAssertion),
        (Relation::Quotes, EdgeOrigin::HumanAssertion),
    ]
    .into_iter()
    .enumerate()
    {
        let source = f.object(&format!("excluded-{i}"), at(1));
        let edge = f.edge(&source, relation, origin, at(10));
        f.import(vec![source], vec![edge]);
    }
    let self_edge = f.edge(
        &f.claim,
        Relation::Supports,
        EdgeOrigin::HumanAssertion,
        at(10),
    );
    let outgoing_target = f.object("outgoing is not evidence", at(1));
    let mut outgoing = f.edge(
        &outgoing_target,
        Relation::Supports,
        EdgeOrigin::HumanAssertion,
        at(10),
    );
    std::mem::swap(&mut outgoing.source, &mut outgoing.target);
    let outgoing = outgoing
        .with_metadata(BTreeMap::new())
        .unwrap()
        .sign(&f.author, &f.key)
        .unwrap();
    f.import(vec![outgoing_target], vec![self_edge, outgoing]);
    let state = f.state(at(10));
    assert_eq!(sources(&state).len(), 6);
    for source in sources(&state) {
        let text = source["text"].as_str().unwrap();
        assert!(text.starts_with("source-"));
        assert_eq!(
            source["is_context"],
            json!(text == "source-4" || text == "source-5")
        );
        assert!(source["vote"].is_null());
        assert!(source["user_id"].is_null());
    }
}

#[test]
fn repeated_edges_merge_roles_and_component_scores_come_from_local_provider() {
    let mut f = Fixture::new();
    let source = f.object(
        "According to the study dataset and reproduced methodology",
        at(1),
    );
    let edges = [
        Relation::References,
        Relation::Cites,
        Relation::Supports,
        Relation::Contradicts,
    ]
    .into_iter()
    .map(|r| f.edge(&source, r, EdgeOrigin::HumanAssertion, at(10)))
    .collect();
    f.import(vec![source.clone()], edges);
    let state = f.state(at(10));
    assert_eq!(sources(&state).len(), 1);
    let selected = &sources(&state)[0];
    assert_eq!(selected["is_context"], false);
    assert_eq!(selected["timestamp"], json!(seconds(at(1))));
    let calls = f.node.judgment_provider.calls.borrow();
    assert_eq!(calls.len(), 2);
    for (request, definition, output_key, source_key) in [
        (
            &calls[0],
            DefinitionId::moderation_v1(),
            "quality",
            "quality_score",
        ),
        (
            &calls[1],
            DefinitionId::evidence_quality_v1(),
            "score",
            "evidence_score",
        ),
    ] {
        assert_eq!(request.definition, definition);
        assert_eq!(request.state.context["text"], source.payload["text"]);
        let judgment = LocalProvider::default().judge(request).unwrap();
        assert_eq!(selected[source_key], judgment.output[output_key]);
        assert!(
            f.node
                .judgment_cache
                .entry(
                    &babble_judgment::cache_key(&f.node.judgment_provider.version(), request)
                        .unwrap()
                )
                .is_none()
        );
    }
    assert!(f.node.store.list_judgments().unwrap().is_empty());
}

#[test]
fn exact_copy_context_cannot_hide_an_evidence_relationship() {
    for context_first in [true, false] {
        let mut f = Fixture::new();
        let mut objects = vec![
            f.object("Identical copied source", at(1)),
            f.object("Identical copied source", at(2)),
        ];
        objects.sort_by(|a, b| a.id.cmp(&b.id));
        let edges = objects
            .iter()
            .enumerate()
            .map(|(i, o)| {
                let context = (i == 0) == context_first;
                f.edge(
                    o,
                    if context {
                        Relation::Cites
                    } else {
                        Relation::EvidenceFor
                    },
                    EdgeOrigin::HumanAssertion,
                    at(10),
                )
            })
            .collect();
        f.import(objects, edges);
        let state = f.state(at(10));
        assert_eq!(sources(&state).len(), 1);
        assert_eq!(
            sources(&state)[0]["is_context"],
            false,
            "context_first={context_first}"
        );
    }
}

#[test]
fn context_only_copies_remain_context_and_blank_sources_are_ignored() {
    let mut f = Fixture::new();
    for i in 1..=2 {
        let object = f.object("Copied context", at(i));
        let edge = f.edge(
            &object,
            Relation::References,
            EdgeOrigin::HumanAssertion,
            at(10),
        );
        f.import(vec![object], vec![edge]);
    }
    let blank = f.custom_object(json!({"text": " \n\t "}), at(1));
    let edge = f.edge(
        &blank,
        Relation::Supports,
        EdgeOrigin::HumanAssertion,
        at(10),
    );
    f.import(vec![blank], vec![edge]);
    let state = f.state(at(10));
    assert_eq!(sources(&state).len(), 1);
    assert_eq!(sources(&state)[0]["is_context"], true);
    assert_eq!(f.node.judgment_provider.calls.borrow().len(), 2);
}

#[test]
fn reuse_is_strictly_under_300_seconds_and_expiry_keeps_previous_score_after_restart() {
    let mut f = Fixture::new();
    f.add("Source evidence", at(1));
    let original = f.state(at(10));
    let judgment = f.remember(
        original.clone(),
        0.73,
        at(10),
        f.node.judgment_provider.version(),
    );
    f.node = LocalNode::open(&f.root, RecordingLocal::default()).unwrap();
    for time in [
        at(10),
        at(309),
        Timestamp(at(310).0 - Duration::milliseconds(1)),
    ] {
        assert_eq!(f.state(time), original);
        assert!(f.node.judgment_provider.calls.borrow().is_empty());
        assert!(f.node.judgment_provider.deadlines.borrow().is_empty());
    }
    for time in [at(310), at(311)] {
        let expired = f.state(time);
        assert_eq!(agreement(&expired)["reference_time"], json!(seconds(time)));
        assert_eq!(agreement(&expired)["previous_score"], json!(0.73));
        assert_eq!(sources(&expired), sources(&original));
    }
    assert_eq!(f.node.judgment_provider.calls.borrow().len(), 4);
    assert_eq!(
        f.node.store.get_judgment(&judgment.id).unwrap(),
        Some(judgment)
    );
    assert_eq!(
        f.node
            .store
            .object_judgment_inputs(&f.claim.id)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn changed_sources_and_changed_roles_invalidate_reuse_but_preserve_history() {
    let mut f = Fixture::new();
    let source = f.object("Context now evidence", at(1));
    let edge = f.edge(
        &source,
        Relation::References,
        EdgeOrigin::HumanAssertion,
        at(10),
    );
    f.import(vec![source.clone()], vec![edge]);
    let original = f.state(at(10));
    f.remember(original, 0.61, at(10), f.node.judgment_provider.version());
    let edge = f.edge(
        &source,
        Relation::Supports,
        EdgeOrigin::HumanAssertion,
        at(11),
    );
    f.import(vec![], vec![edge]);
    let changed = f.state(at(11));
    assert_eq!(sources(&changed)[0]["is_context"], false);
    assert_eq!(
        agreement(&changed)["reference_time"],
        json!(seconds(at(11)))
    );
    assert_eq!(agreement(&changed)["previous_score"], json!(0.61));
    f.add("Additional evidence", at(1));
    let changed = f.state(at(12));
    assert_eq!(sources(&changed).len(), 2);
    assert_eq!(
        agreement(&changed)["reference_time"],
        json!(seconds(at(12)))
    );
    assert_eq!(agreement(&changed)["previous_score"], json!(0.61));
}

#[test]
fn changed_persisted_source_metadata_invalidates_reuse() {
    for (field, value) in [
        ("kind", json!("research_paper")),
        ("timestamp", json!(seconds(at(0)))),
        ("user_id", json!("previous-attribution")),
        ("vote", json!(0.8)),
    ] {
        let mut f = Fixture::new();
        f.add("Evidence", at(1));
        let original = f.state(at(10));
        let mut altered = original.clone();
        let source = &mut altered.context.get_mut("source_agreement").unwrap()["sources"][0];
        source[field] = value;
        if field == "vote" {
            source["user_id"] = json!("previous-voter");
        }
        f.remember(altered, 0.61, at(10), f.node.judgment_provider.version());
        f.node.judgment_provider.calls.borrow_mut().clear();
        let refreshed = f.state(at(11));
        assert_eq!(sources(&refreshed), sources(&original), "{field}");
        assert_eq!(
            agreement(&refreshed)["previous_score"],
            json!(0.61),
            "{field}"
        );
        assert_eq!(
            agreement(&refreshed)["reference_time"],
            json!(seconds(at(11))),
            "{field}"
        );
        assert_eq!(f.node.judgment_provider.calls.borrow().len(), 2, "{field}");
    }
}

#[test]
fn history_uses_latest_eligible_provider_version_and_ignores_future_judgments() {
    let f = Fixture::new();
    let first = f.state(at(10));
    let version = f.node.judgment_provider.version();
    f.remember(first, 0.21, at(10), version.clone());
    let second = f.state(at(310));
    f.remember(second, 0.62, at(310), version.clone());
    let later = f.state(at(610));
    f.remember(later.clone(), 0.99, at(1000), version.clone());
    let mut other_version = version;
    other_version.version = "other-version".into();
    f.remember(later, 0.88, at(650), other_version);
    let state = f.state(at(700));
    assert_eq!(agreement(&state)["previous_score"], json!(0.62));
    assert_eq!(agreement(&state)["reference_time"], json!(seconds(at(700))));
    assert!(agreement(&f.state(at(5)))["previous_score"].is_null());
    assert_eq!(
        f.node
            .store
            .object_judgment_inputs(&f.claim.id)
            .unwrap()
            .len(),
        4
    );
}

#[test]
fn native_unsupported_fails_before_source_selection_scoring_or_persistence() {
    let mut f = Fixture::new();
    f.add(&"x".repeat(64 * 1024 + 1), at(1));
    let before = (
        f.node.store.list_objects().unwrap(),
        f.node.store.list_edges().unwrap(),
        f.node.store.list_events().unwrap(),
    );
    let error = f
        .node
        .judge_object(
            &f.claim.id,
            DefinitionId::source_agreement_v1(),
            BTreeMap::new(),
        )
        .unwrap_err();
    assert!(
        matches!(error, Error::ProviderUnavailable(ref message) if message.contains("source agreement provider is unavailable")),
        "{error}"
    );
    assert!(f.node.judgment_provider.calls.borrow().is_empty());
    assert!(f.node.store.list_judgments().unwrap().is_empty());
    assert!(
        f.node
            .store
            .object_judgment_inputs(&f.claim.id)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        (
            f.node.store.list_objects().unwrap(),
            f.node.store.list_edges().unwrap(),
            f.node.store.list_events().unwrap()
        ),
        before
    );
    // The same graph reaches the cap check when selection is invoked directly.
    f.assert_limit_without_scoring();
}

#[test]
fn no_source_claim_excludes_arbitrary_metadata_without_text_fallback() {
    let mut f = Fixture::new();
    for payload in [
        json!({"private": "SECRET", "metadata": {"text": "SECRET"}, "session_token": "SECRET"}),
        json!({"text": {"private": "SECRET"}, "title": 123, "description": null, "summary": ["SECRET"]}),
        json!({"title": "Public title", "description": "Public description", "summary": "Public summary",
            "private": "SECRET", "metadata": {"session_token": "SECRET"}}),
    ] {
        let claim = f.custom_object(payload.clone(), at(0));
        f.import(vec![claim.clone()], vec![]);
        let state = f.node.source_agreement_state_at(&claim, at(10)).unwrap();
        assert_eq!(state.context.len(), 2);
        assert_eq!(
            state.context["text"],
            if payload["title"].is_string() {
                json!("Public title\nPublic description\nPublic summary")
            } else {
                json!("")
            }
        );
        assert!(!serde_json::to_string(&state).unwrap().contains("SECRET"));
        assert!(sources(&state).is_empty());
        assert!(agreement(&state)["previous_score"].is_null());
    }
    assert!(f.node.judgment_provider.calls.borrow().is_empty());
    assert!(f.node.store.list_judgments().unwrap().is_empty());
}

#[test]
fn supporting_assessments_receive_only_allowlisted_public_text() {
    let mut f = Fixture::new();
    let source = f.custom_object(json!({
        "text": "Public text", "title": "Public title", "description": "Public description", "summary": "Public summary",
        "private": "SECRET", "session_token": "SECRET", "metadata": {"private": "SECRET"}
    }), at(1));
    let edge = f.edge(
        &source,
        Relation::Supports,
        EdgeOrigin::HumanAssertion,
        at(10),
    );
    f.import(vec![source.clone()], vec![edge]);
    let state = f.state(at(10));
    let public = json!("Public text\nPublic title\nPublic description\nPublic summary");
    assert_eq!(sources(&state)[0]["text"], public);
    assert!(!serde_json::to_string(&state).unwrap().contains("SECRET"));
    let calls = f.node.judgment_provider.calls.borrow();
    assert_eq!(calls.len(), 2);
    for request in calls.iter() {
        assert_eq!(request.state.subject, source.id.to_string());
        assert_eq!(
            request.state.context,
            BTreeMap::from([("text".into(), public.clone())])
        );
        assert!(request.parameters.is_empty());
    }
}

#[test]
fn claim_limit_includes_utf8_and_join_separators_without_truncation() {
    let f = Fixture::new();
    let text = "\u{00e9}".repeat(64 * 1024 - 1);
    let claim = f.custom_object(json!({"text": text, "title": "x"}), at(0));
    let state = f.node.source_agreement_state_at(&claim, at(10)).unwrap();
    assert_eq!(state.context["text"].as_str().unwrap().len(), 128 * 1024);
    assert!(state.context["text"].as_str().unwrap().ends_with("\nx"));
    let too_large = f.custom_object(json!({"text": text, "title": "xx"}), at(0));
    assert!(matches!(
        f.node.source_agreement_state_at(&too_large, at(10)),
        Err(Error::Conflict(message)) if message.contains("evaluation limits")
    ));
    assert!(f.node.judgment_provider.calls.borrow().is_empty());
    assert!(f.node.store.list_judgments().unwrap().is_empty());
}

#[test]
fn source_limit_counts_join_separators_and_deduplicates_across_field_layouts() {
    let mut f = Fixture::new();
    let text = "x".repeat(64 * 1024 - 2);
    let split = f.custom_object(json!({"text": text, "title": "z"}), at(1));
    let joined = f.object(&format!("{text}\nz"), at(2));
    let edges = [&split, &joined]
        .into_iter()
        .map(|source| {
            f.edge(
                source,
                Relation::Supports,
                EdgeOrigin::HumanAssertion,
                at(10),
            )
        })
        .collect();
    f.import(vec![split, joined], edges);
    let state = f.state(at(10));
    assert_eq!(sources(&state).len(), 1);
    assert_eq!(sources(&state)[0]["text"], format!("{text}\nz"));
    let too_large = f.custom_object(json!({"text": text, "title": "zz"}), at(1));
    let edge = f.edge(
        &too_large,
        Relation::Supports,
        EdgeOrigin::HumanAssertion,
        at(10),
    );
    f.import(vec![too_large], vec![edge]);
    f.assert_limit_without_scoring();
}

#[test]
fn expired_selection_budget_does_not_start_workers() {
    let mut f = Fixture::new();
    f.add("Evidence", at(1));
    assert!(matches!(
        f.node
            .source_agreement_state_before(&f.claim, at(10), Instant::now()),
        Err(Error::ProviderUnavailable(_))
    ));
    assert!(f.node.judgment_provider.calls.borrow().is_empty());
    assert!(f.node.store.list_judgments().unwrap().is_empty());
}

#[test]
fn supporting_assessments_and_aggregate_receive_one_deadline() {
    let mut f = Fixture::new();
    f.add("Evidence", at(1));
    assert_eq!(EVALUATION_BUDGET, std::time::Duration::from_secs(15));
    let deadline = Instant::now() + EVALUATION_BUDGET;
    let state = f
        .node
        .source_agreement_state_before(&f.claim, at(10), deadline)
        .unwrap();
    let result = f.node.prepare_judgment_request_before(
        &f.claim,
        JudgmentRequest {
            definition: DefinitionId::source_agreement_v1(),
            state,
            parameters: BTreeMap::new(),
        },
        Some(deadline),
    );
    // LocalProvider deliberately rejects aggregate scoring, after the deadline
    // has reached that call as well as both successful component assessments.
    let error = match result {
        Ok(_) => panic!("aggregate scoring must be unsupported by LocalProvider"),
        Err(error) => error,
    };
    assert!(matches!(error, Error::ProviderUnavailable(_)), "{error}");
    assert_eq!(
        *f.node.judgment_provider.deadlines.borrow(),
        vec![deadline; 3]
    );
    assert_eq!(
        f.node
            .judgment_provider
            .calls
            .borrow()
            .last()
            .unwrap()
            .definition,
        DefinitionId::source_agreement_v1()
    );
    assert!(f.node.store.list_judgments().unwrap().is_empty());
}

#[test]
fn late_supporting_result_stops_remaining_workers_and_leaves_no_cache_or_history() {
    let mut f = Fixture::new();
    f.add("Evidence", at(1));
    *f.node.judgment_provider.expire_on.borrow_mut() = Some(DefinitionId::moderation_v1());
    let deadline = Instant::now() + std::time::Duration::from_millis(250);
    assert!(matches!(
        f.node
            .source_agreement_state_before(&f.claim, at(10), deadline),
        Err(Error::ProviderUnavailable(_))
    ));
    let calls = f.node.judgment_provider.calls.borrow();
    assert_eq!(calls.len(), 1);
    let key = babble_judgment::cache_key(&f.node.judgment_provider.version(), &calls[0]).unwrap();
    assert!(f.node.judgment_cache.entry(&key).is_none());
    assert!(f.node.store.list_judgments().unwrap().is_empty());
    assert!(
        f.node
            .store
            .object_judgment_inputs(&f.claim.id)
            .unwrap()
            .is_empty()
    );
}
