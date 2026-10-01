use babel_judgment::{
    ConstantProvider, DefinitionId, Judgment, JudgmentCache, JudgmentOrchestrator,
    JudgmentPrivacyPolicy, JudgmentProvider, JudgmentRequest, JudgmentState, ProviderCascade,
    ProviderRole, ProviderVersion,
};
use babel_types::{Canonical, Error as CoreError, JudgmentId, Timestamp};
use std::{cell::RefCell, collections::BTreeMap};

#[test]
fn cache_key_reuses_definition_provider_model_and_input() {
    let provider = ConstantProvider::default();
    let request = JudgmentRequest {
        definition: DefinitionId::evidence_quality_v1(),
        state: JudgmentState {
            subject: "obj_abc".to_string(),
            context: text_context("source-backed evidence"),
        },
        parameters: BTreeMap::new(),
    };

    let mut cache = JudgmentCache::default();
    let first = cache.get_or_evaluate(&provider, &request).unwrap();
    let second = cache.get_or_evaluate(&provider, &request).unwrap();

    assert_eq!(first.id, second.id);
    assert_eq!(first.provider, provider.version());
}

#[test]
fn cascade_returns_first_provider_that_meets_confidence_policy() {
    let low = ConstantProvider::new(
        ProviderVersion {
            provider: "babel-low".to_string(),
            model: "rules".to_string(),
            version: "1".to_string(),
        },
        serde_json::json!({"kind": "bounded_score", "score": 0.2, "confidence": 0.2}),
        0.2,
    );
    let high = ConstantProvider::new(
        ProviderVersion {
            provider: "babel-high".to_string(),
            model: "rules".to_string(),
            version: "1".to_string(),
        },
        serde_json::json!({"kind": "bounded_score", "score": 0.9, "confidence": 0.9}),
        0.9,
    );
    let cascade = ProviderCascade::new(ProviderVersion {
        provider: "babel-cascade".to_string(),
        model: "ordered-confidence".to_string(),
        version: "1".to_string(),
    })
    .with_provider(&low, 0.8)
    .with_provider(&high, 0.8);
    let request = JudgmentRequest {
        definition: DefinitionId::evidence_quality_v1(),
        state: JudgmentState {
            subject: "obj_abc".to_string(),
            context: text_context("source-backed evidence"),
        },
        parameters: BTreeMap::new(),
    };

    let judgment = cascade.judge(&request).unwrap();

    assert_eq!(judgment.provider, cascade.version());
    assert_eq!(judgment.confidence, 0.9);
    assert_eq!(
        judgment.output["selected_provider"]["provider"],
        serde_json::json!("babel-high")
    );
    assert_eq!(judgment.output["evaluated"].as_array().unwrap().len(), 2);
}

#[test]
fn orchestrator_minimizes_remote_input_and_records_cache_hits() {
    let provider = RecordingProvider::new(
        ProviderVersion {
            provider: "remote-semantic".to_string(),
            model: "batchable".to_string(),
            version: "1".to_string(),
        },
        0.82,
    );
    let orchestrator = JudgmentOrchestrator::new(ProviderVersion {
        provider: "babel-orchestrator".to_string(),
        model: "privacy-aware".to_string(),
        version: "1".to_string(),
    })
    .with_provider(
        &provider,
        ProviderRole::Remote,
        0.8,
        JudgmentPrivacyPolicy::remote_minimized(),
    );
    let request = JudgmentRequest {
        definition: DefinitionId::evidence_quality_v1(),
        state: JudgmentState {
            subject: "obj_sensitive".to_string(),
            context: BTreeMap::from([
                ("object".to_string(), serde_json::json!({"private": true})),
                (
                    "text".to_string(),
                    serde_json::json!("according to the dataset and methodology"),
                ),
            ]),
        },
        parameters: BTreeMap::new(),
    };
    let mut cache = JudgmentCache::default();

    let first = orchestrator.evaluate(&mut cache, &request).unwrap();
    let second = orchestrator.evaluate(&mut cache, &request).unwrap();

    assert!(!first.cache_hit);
    assert!(second.cache_hit);
    assert_eq!(provider.seen.borrow().len(), 1);
    let sent = provider.seen.borrow()[0].clone();
    assert_ne!(sent.state.subject, "obj_sensitive");
    assert!(sent.state.context.contains_key("text"));
    assert!(!sent.state.context.contains_key("object"));
    assert!(
        first.decisions[0]
            .privacy
            .redacted_context_keys
            .contains(&"object".to_string())
    );
    assert!(first.decisions[0].accepted);
    assert!(!second.decisions[0].accepted || second.decisions[0].cache_hit);
}

#[test]
fn single_orchestrator_uses_provider_declared_privacy_boundary() {
    let provider = RecordingProvider::new(
        ProviderVersion {
            provider: "remote-semantic".to_string(),
            model: "single".to_string(),
            version: "1".to_string(),
        },
        0.7,
    )
    .with_role(ProviderRole::Remote);
    let orchestrator = JudgmentOrchestrator::single(&provider);
    let request = JudgmentRequest {
        definition: DefinitionId::evidence_quality_v1(),
        state: JudgmentState {
            subject: "obj_sensitive".to_string(),
            context: BTreeMap::from([
                ("object".to_string(), serde_json::json!({"private": true})),
                (
                    "text".to_string(),
                    serde_json::json!("according to a dataset"),
                ),
            ]),
        },
        parameters: BTreeMap::new(),
    };

    let result = orchestrator
        .evaluate(&mut JudgmentCache::default(), &request)
        .unwrap();

    assert_eq!(result.decisions[0].role, ProviderRole::Remote);
    assert_ne!(provider.seen.borrow()[0].state.subject, "obj_sensitive");
    assert!(
        !provider.seen.borrow()[0]
            .state
            .context
            .contains_key("object")
    );
    assert!(
        result.decisions[0]
            .privacy
            .redacted_context_keys
            .contains(&"object".to_string())
    );
}

#[test]
fn orchestrator_degrades_across_provider_outage_and_low_confidence() {
    let unavailable = FailingProvider::new(ProviderVersion {
        provider: "local-down".to_string(),
        model: "rules".to_string(),
        version: "1".to_string(),
    });
    let low = RecordingProvider::new(
        ProviderVersion {
            provider: "local-low".to_string(),
            model: "rules".to_string(),
            version: "1".to_string(),
        },
        0.42,
    );
    let remote = RecordingProvider::new(
        ProviderVersion {
            provider: "jev-compatible".to_string(),
            model: "semantic".to_string(),
            version: "1".to_string(),
        },
        0.91,
    )
    .with_role(ProviderRole::Remote);
    let orchestrator = JudgmentOrchestrator::new(ProviderVersion {
        provider: "babel-provider-cascade".to_string(),
        model: "local-remote".to_string(),
        version: "1".to_string(),
    })
    .with_provider(
        &unavailable,
        ProviderRole::Local,
        0.8,
        JudgmentPrivacyPolicy::local_full(),
    )
    .with_provider(
        &low,
        ProviderRole::Local,
        0.8,
        JudgmentPrivacyPolicy::local_full(),
    )
    .with_provider(
        &remote,
        ProviderRole::Remote,
        0.8,
        JudgmentPrivacyPolicy::remote_minimized(),
    );
    let request = JudgmentRequest {
        definition: DefinitionId::evidence_quality_v1(),
        state: JudgmentState {
            subject: "obj_claim".to_string(),
            context: BTreeMap::from([
                (
                    "object".to_string(),
                    serde_json::json!({"author": "private"}),
                ),
                (
                    "text".to_string(),
                    serde_json::json!("according to the dataset and methodology"),
                ),
            ]),
        },
        parameters: BTreeMap::new(),
    };

    let result = orchestrator
        .evaluate(&mut JudgmentCache::default(), &request)
        .unwrap();

    assert_eq!(result.decisions.len(), 3);
    assert!(result.decisions[0].error.is_some());
    assert!(!result.decisions[1].accepted);
    assert!(result.decisions[2].accepted);
    assert_eq!(result.selected_provider.provider, "jev-compatible");
    assert_eq!(result.judgment.provider.provider, "jev-compatible");
    assert!(!remote.seen.borrow()[0].state.context.contains_key("object"));
    assert_eq!(low.seen.borrow().len(), 1);
}

#[test]
fn orchestrator_batches_requests_through_shared_cache() {
    let provider = RecordingProvider::new(
        ProviderVersion {
            provider: "local-semantic".to_string(),
            model: "rules".to_string(),
            version: "1".to_string(),
        },
        0.7,
    );
    let orchestrator = JudgmentOrchestrator::single(&provider);
    let requests = vec![
        JudgmentRequest {
            definition: DefinitionId::evidence_quality_v1(),
            state: JudgmentState {
                subject: "obj_one".to_string(),
                context: text_context("source-backed evidence"),
            },
            parameters: BTreeMap::new(),
        },
        JudgmentRequest {
            definition: DefinitionId::evidence_quality_v1(),
            state: JudgmentState {
                subject: "obj_one".to_string(),
                context: text_context("source-backed evidence"),
            },
            parameters: BTreeMap::new(),
        },
    ];
    let mut cache = JudgmentCache::default();

    let result = orchestrator.evaluate_batch(&mut cache, &requests).unwrap();

    assert_eq!(result.judgments.len(), 2);
    assert!(!result.judgments[0].cache_hit);
    assert!(result.judgments[1].cache_hit);
    assert_eq!(provider.seen.borrow().len(), 1);
}

fn text_context(text: &str) -> BTreeMap<String, serde_json::Value> {
    BTreeMap::from([("text".to_string(), serde_json::json!(text))])
}

struct RecordingProvider {
    version: ProviderVersion,
    role: ProviderRole,
    confidence: f64,
    seen: RefCell<Vec<JudgmentRequest>>,
}

impl RecordingProvider {
    fn new(version: ProviderVersion, confidence: f64) -> Self {
        Self {
            version,
            role: ProviderRole::Local,
            confidence,
            seen: RefCell::new(Vec::new()),
        }
    }

    fn with_role(mut self, role: ProviderRole) -> Self {
        self.role = role;
        self
    }
}

impl JudgmentProvider for RecordingProvider {
    fn version(&self) -> ProviderVersion {
        self.version.clone()
    }

    fn role(&self) -> ProviderRole {
        self.role.clone()
    }

    fn judge(&self, request: &JudgmentRequest) -> babel_types::Result<Judgment> {
        self.seen.borrow_mut().push(request.clone());
        let input_hash = request.state.canonical_hash()?;
        let output = serde_json::json!({
            "kind": "bounded_score",
            "score": self.confidence,
            "confidence": self.confidence,
        });
        let commitment = (
            request.definition.clone(),
            self.version.clone(),
            input_hash.clone(),
            request.parameters.clone(),
            output.clone(),
        );
        Ok(Judgment {
            id: JudgmentId::from_hash(&commitment.canonical_hash()?),
            definition: request.definition.clone(),
            provider: self.version.clone(),
            input_hash,
            output,
            confidence: self.confidence,
            created_at: Timestamp::now(),
        })
    }
}

struct FailingProvider {
    version: ProviderVersion,
}

impl FailingProvider {
    fn new(version: ProviderVersion) -> Self {
        Self { version }
    }
}

impl JudgmentProvider for FailingProvider {
    fn version(&self) -> ProviderVersion {
        self.version.clone()
    }

    fn judge(&self, _request: &JudgmentRequest) -> babel_types::Result<Judgment> {
        Err(CoreError::ProviderUnavailable(format!(
            "{} unavailable",
            self.version.provider
        )))
    }
}
