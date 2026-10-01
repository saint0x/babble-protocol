use babble_judgment::{
    ConstantProvider, DefinitionId, Judgment, JudgmentCache, JudgmentOrchestrator,
    JudgmentPrivacyPolicy, JudgmentProvider, JudgmentRequest, JudgmentState, ProviderRole,
    ProviderVersion, cache_key,
};
use babble_types::{Error, Result};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    thread,
    time::{Duration, Instant},
};

fn request() -> JudgmentRequest {
    JudgmentRequest {
        definition: DefinitionId::evidence_quality_v1(),
        state: JudgmentState {
            subject: "source-budget".into(),
            context: BTreeMap::from([("text".into(), json!("Published dataset evidence."))]),
        },
        parameters: BTreeMap::new(),
    }
}

fn constant(name: &str, confidence: f64) -> ConstantProvider {
    ConstantProvider::new(
        ProviderVersion {
            provider: name.into(),
            model: "budget-test".into(),
            version: "1".into(),
        },
        json!({"kind": "bounded_score", "score": 0.5}),
        confidence,
    )
}

fn deadline_error<T: std::fmt::Debug>(result: Result<T>) {
    let error = result.unwrap_err();
    assert!(matches!(error, Error::ProviderUnavailable(_)), "{error}");
    assert!(error.to_string().contains("deadline"), "{error}");
}

struct Cooperative {
    calls: Cell<usize>,
    delay: Duration,
    fail: bool,
}

impl JudgmentProvider for Cooperative {
    fn version(&self) -> ProviderVersion {
        constant("cooperative", 0.5).version()
    }

    fn judge(&self, request: &JudgmentRequest) -> Result<Judgment> {
        self.calls.set(self.calls.get() + 1);
        thread::sleep(self.delay);
        if self.fail {
            Err(Error::Conflict("provider failed".into()))
        } else {
            constant("cooperative", 0.5).judge(request)
        }
    }
}

struct BudgetProvider {
    inner: ConstantProvider,
    seen: RefCell<Vec<Instant>>,
    exhaust: bool,
}

impl BudgetProvider {
    fn new(name: &str, exhaust: bool) -> Self {
        Self {
            inner: constant(name, 0.4),
            seen: RefCell::new(vec![]),
            exhaust,
        }
    }
}

impl JudgmentProvider for BudgetProvider {
    fn version(&self) -> ProviderVersion {
        self.inner.version()
    }

    fn judge(&self, _: &JudgmentRequest) -> Result<Judgment> {
        panic!("bounded evaluation must forward judge_before")
    }

    fn judge_before(&self, request: &JudgmentRequest, deadline: Instant) -> Result<Judgment> {
        self.seen.borrow_mut().push(deadline);
        if self.exhaust {
            thread::sleep(
                deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(5),
            );
        }
        // Deliberately return late: orchestration must enforce its own budget.
        self.inner.judge(request)
    }
}

fn route<'a>(
    orchestrator: JudgmentOrchestrator<'a>,
    provider: &'a dyn JudgmentProvider,
) -> JudgmentOrchestrator<'a> {
    orchestrator.with_provider(
        provider,
        ProviderRole::Local,
        0.9,
        JudgmentPrivacyPolicy::local_full(),
    )
}

#[test]
fn default_deadline_checks_before_and_after_success_or_failure() {
    for fail in [false, true] {
        let provider = Cooperative {
            calls: Cell::new(0),
            delay: Duration::from_millis(40),
            fail,
        };
        deadline_error(provider.judge_before(&request(), Instant::now()));
        assert_eq!(provider.calls.get(), 0);
        deadline_error(
            provider.judge_before(&request(), Instant::now() + Duration::from_millis(15)),
        );
        assert_eq!(provider.calls.get(), 1);
        let result = provider.judge_before(&request(), Instant::now() + Duration::from_secs(1));
        assert_eq!(result.is_ok(), !fail);
        if fail {
            assert!(matches!(result, Err(Error::Conflict(_))));
        }
    }
}

#[test]
fn expired_cache_hit_leaves_entry_and_hit_count_unchanged() {
    let provider = constant("cached", 0.9);
    let req = request();
    let mut cache = JudgmentCache::default();
    let orchestrator = JudgmentOrchestrator::single(&provider);
    orchestrator.evaluate(&mut cache, &req).unwrap();
    let key = cache_key(&provider.version(), &req).unwrap();
    let before = cache.entry(&key).unwrap().clone();
    deadline_error(orchestrator.evaluate_before(&mut cache, &req, Instant::now()));
    assert_eq!(cache.entry(&key), Some(&before));
    let result = orchestrator
        .evaluate_before(&mut cache, &req, Instant::now() + Duration::from_secs(1))
        .unwrap();
    assert!(result.cache_hit);
    assert_eq!(cache.entry(&key).unwrap().hits, before.hits + 1);
}

#[test]
fn budget_failure_rolls_back_hits_and_inserts_and_never_retries() {
    let cached = BudgetProvider::new("cached", false);
    let inserted = BudgetProvider::new("inserted", false);
    let late = BudgetProvider::new("late", true);
    let uncalled = BudgetProvider::new("uncalled", false);
    let req = request();
    let mut cache = JudgmentCache::default();
    let key = cache_key(&cached.version(), &req).unwrap();
    cache.insert(key.clone(), cached.inner.judge(&req).unwrap());
    cache.record_hit(&key);
    let before = cache.entry(&key).unwrap().clone();
    let orchestrator = route(
        route(
            route(
                route(JudgmentOrchestrator::new(cached.version()), &cached),
                &inserted,
            ),
            &late,
        ),
        &uncalled,
    );
    let deadline = Instant::now() + Duration::from_millis(60);
    deadline_error(orchestrator.evaluate_before(&mut cache, &req, deadline));
    assert_eq!(cache.entry(&key), Some(&before));
    for provider in [&inserted, &late, &uncalled] {
        assert!(
            cache
                .entry(&cache_key(&provider.version(), &req).unwrap())
                .is_none()
        );
    }
    assert!(cached.seen.borrow().is_empty());
    assert_eq!(*inserted.seen.borrow(), vec![deadline]);
    assert_eq!(*late.seen.borrow(), vec![deadline]);
    assert!(uncalled.seen.borrow().is_empty());
}

#[test]
fn repeated_route_keys_restore_original_hit_count_once() {
    let cached = BudgetProvider::new("cached", false);
    let late = BudgetProvider::new("late", true);
    let req = request();
    let mut cache = JudgmentCache::default();
    let key = cache_key(&cached.version(), &req).unwrap();
    cache.insert(key.clone(), cached.inner.judge(&req).unwrap());
    let before = cache.entry(&key).unwrap().clone();
    let orchestrator = route(
        route(
            route(JudgmentOrchestrator::new(cached.version()), &cached),
            &cached,
        ),
        &late,
    );
    deadline_error(orchestrator.evaluate_before(
        &mut cache,
        &req,
        Instant::now() + Duration::from_millis(40),
    ));
    assert_eq!(cache.entry(&key), Some(&before));
}

#[test]
fn successful_budget_evaluation_commits_and_matches_unbounded_selection() {
    let low = constant("low", 0.4);
    let high = constant("high", 0.95);
    let orchestrator = route(route(JudgmentOrchestrator::new(low.version()), &low), &high);
    let req = request();
    let mut bounded = JudgmentCache::default();
    let mut unbounded = JudgmentCache::default();
    let expected = orchestrator.evaluate(&mut unbounded, &req).unwrap();
    let actual = orchestrator
        .evaluate_before(&mut bounded, &req, Instant::now() + Duration::from_secs(1))
        .unwrap();
    assert_eq!(actual.judgment.id, expected.judgment.id);
    assert_eq!(actual.decisions, expected.decisions);
    for provider in [&low, &high] {
        assert!(
            bounded
                .entry(&cache_key(&provider.version(), &req).unwrap())
                .is_some()
        );
    }
}

#[test]
fn expired_budget_wins_over_invalid_input_without_invoking_provider() {
    let provider = BudgetProvider::new("unused", false);
    let mut req = request();
    req.state.context.clear();
    deadline_error(JudgmentOrchestrator::single(&provider).evaluate_before(
        &mut JudgmentCache::default(),
        &req,
        Instant::now(),
    ));
    assert!(provider.seen.borrow().is_empty());
}

#[test]
fn ordinary_provider_failure_can_fall_back_within_budget() {
    let failing = Cooperative {
        calls: Cell::new(0),
        delay: Duration::ZERO,
        fail: true,
    };
    let good = BudgetProvider::new("fallback", false);
    let orchestrator = route(
        route(JudgmentOrchestrator::new(good.version()), &failing),
        &good,
    );
    let deadline = Instant::now() + Duration::from_secs(1);
    let result = orchestrator
        .evaluate_before(&mut JudgmentCache::default(), &request(), deadline)
        .unwrap();
    assert_eq!(result.selected_provider, good.version());
    assert!(result.decisions[0].error.is_some());
    assert_eq!(*good.seen.borrow(), vec![deadline]);
}
