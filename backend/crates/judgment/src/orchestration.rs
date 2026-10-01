use crate::{
    CacheKey, CachedJudgment, DefinitionId, Judgment, JudgmentCache, JudgmentProvider,
    JudgmentRegistry, JudgmentRequest, JudgmentState, ProviderVersion, cache_key, check_deadline,
};
use babble_types::{Canonical, Error, Hash, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum ProviderRole {
    Local,
    Remote,
    Ensemble,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgmentPrivacyPolicy {
    pub include_subject: bool,
    pub allowed_context_keys: BTreeSet<String>,
    pub max_text_bytes: Option<usize>,
}

impl JudgmentPrivacyPolicy {
    pub fn local_full() -> Self {
        Self {
            include_subject: true,
            allowed_context_keys: BTreeSet::from(["object".to_string(), "text".to_string()]),
            max_text_bytes: None,
        }
    }

    pub fn remote_minimized() -> Self {
        Self {
            include_subject: false,
            allowed_context_keys: BTreeSet::from(["text".to_string()]),
            max_text_bytes: Some(4096),
        }
    }

    pub fn apply(
        &self,
        request: &JudgmentRequest,
    ) -> Result<(JudgmentRequest, AppliedPrivacyPolicy)> {
        JudgmentRegistry::babble_core().validate_request(request)?;
        let original_input_hash = request.state.canonical_hash()?;
        let mut context = BTreeMap::new();
        let mut redacted_context_keys = Vec::new();
        let mut truncated_text = false;

        for (key, value) in &request.state.context {
            if !self.allowed_context_keys.contains(key) {
                redacted_context_keys.push(key.clone());
                continue;
            }
            let value = if key == "text" {
                match value.as_str() {
                    Some(text) => {
                        let (text, truncated) = truncate_utf8(text, self.max_text_bytes);
                        truncated_text |= truncated;
                        Value::String(text)
                    }
                    None => value.clone(),
                }
            } else {
                value.clone()
            };
            context.insert(key.clone(), value);
        }

        if !context.contains_key("text") {
            let Some(text) = request.state.context.get("text").and_then(Value::as_str) else {
                return Err(Error::Conflict(
                    "privacy-minimized Judgment request requires text context".to_string(),
                ));
            };
            let (text, truncated) = truncate_utf8(text, self.max_text_bytes);
            truncated_text |= truncated;
            context.insert("text".to_string(), Value::String(text));
        }

        let subject = if self.include_subject {
            request.state.subject.clone()
        } else {
            format!("redacted:{}", original_input_hash.as_str())
        };
        let scoped = JudgmentRequest {
            definition: request.definition.clone(),
            state: JudgmentState { subject, context },
            parameters: request.parameters.clone(),
        };
        JudgmentRegistry::babble_core().validate_request(&scoped)?;
        let provider_input_hash = scoped.state.canonical_hash()?;
        let applied = AppliedPrivacyPolicy {
            include_subject: self.include_subject,
            allowed_context_keys: self.allowed_context_keys.iter().cloned().collect(),
            redacted_context_keys,
            truncated_text,
            original_input_hash,
            provider_input_hash,
        };
        Ok((scoped, applied))
    }
}

impl Default for JudgmentPrivacyPolicy {
    fn default() -> Self {
        Self::remote_minimized()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AppliedPrivacyPolicy {
    pub include_subject: bool,
    pub allowed_context_keys: Vec<String>,
    pub redacted_context_keys: Vec<String>,
    pub truncated_text: bool,
    pub original_input_hash: Hash,
    pub provider_input_hash: Hash,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderDecision {
    pub provider: ProviderVersion,
    pub role: ProviderRole,
    pub cache_hit: bool,
    pub accepted: bool,
    pub confidence: Option<f64>,
    pub accept_confidence: f64,
    pub judgment_id: Option<babble_types::JudgmentId>,
    pub privacy: AppliedPrivacyPolicy,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OrchestratedJudgment {
    pub judgment: Judgment,
    pub cache_hit: bool,
    pub selected_provider: ProviderVersion,
    pub decisions: Vec<ProviderDecision>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgmentBatchResult {
    pub judgments: Vec<OrchestratedJudgment>,
}

struct ProviderRoute<'a> {
    provider: &'a dyn JudgmentProvider,
    role: ProviderRole,
    accept_confidence: f64,
    privacy: JudgmentPrivacyPolicy,
}

pub struct JudgmentOrchestrator<'a> {
    version: ProviderVersion,
    routes: Vec<ProviderRoute<'a>>,
}

impl<'a> JudgmentOrchestrator<'a> {
    pub fn new(version: ProviderVersion) -> Self {
        Self {
            version,
            routes: Vec::new(),
        }
    }

    pub fn single(provider: &'a dyn JudgmentProvider) -> Self {
        let version = provider.version();
        Self::new(version).with_provider(provider, provider.role(), 0.0, provider.privacy_policy())
    }

    pub fn with_provider(
        mut self,
        provider: &'a dyn JudgmentProvider,
        role: ProviderRole,
        accept_confidence: f64,
        privacy: JudgmentPrivacyPolicy,
    ) -> Self {
        self.routes.push(ProviderRoute {
            provider,
            role,
            accept_confidence: accept_confidence.clamp(0.0, 1.0),
            privacy,
        });
        self
    }

    pub fn version(&self) -> ProviderVersion {
        self.version.clone()
    }

    pub fn evaluate(
        &self,
        cache: &mut JudgmentCache,
        request: &JudgmentRequest,
    ) -> Result<OrchestratedJudgment> {
        self.evaluate_impl(&mut EvaluationCache::new(cache, false), request, None)
    }

    /// Evaluate against a shared deadline, including privacy, hashing, cache
    /// access and output validation. An error leaves cache values and hit counts
    /// unchanged. Only entries touched by this evaluation are journaled, so cost
    /// does not grow with unrelated cached history.
    ///
    /// Synchronous local work is checked cooperatively; providers receive the
    /// same deadline through `JudgmentProvider::judge_before`.
    pub fn evaluate_before(
        &self,
        cache: &mut JudgmentCache,
        request: &JudgmentRequest,
        deadline: Instant,
    ) -> Result<OrchestratedJudgment> {
        check_deadline(deadline)?;
        let mut cache = EvaluationCache::new(cache, true);
        let result = self.evaluate_impl(&mut cache, request, Some(deadline));
        check_deadline(deadline)?;
        let result = result?;
        cache.commit();
        Ok(result)
    }

    fn evaluate_impl(
        &self,
        cache: &mut EvaluationCache<'_>,
        request: &JudgmentRequest,
        deadline: Option<Instant>,
    ) -> Result<OrchestratedJudgment> {
        let check = || deadline.map(check_deadline).unwrap_or(Ok(()));
        check()?;
        JudgmentRegistry::babble_core().validate_request(request)?;
        if self.routes.is_empty() {
            return Err(Error::ProviderUnavailable(
                "judgment orchestrator has no providers".to_string(),
            ));
        }

        let mut decisions = Vec::new();
        let mut selected = None;
        let mut selected_cache_hit = false;
        let mut last_error = None;

        for route in &self.routes {
            check()?;
            let provider_version = route.provider.version();
            let (provider_request, privacy) = route.privacy.apply(request)?;
            let key = cache_key(&provider_version, &provider_request)?;
            check()?;
            let evaluated = if let Some(judgment) = cache.record_hit(&key) {
                Ok((judgment, true))
            } else {
                let result = match deadline {
                    Some(deadline) => route.provider.judge_before(&provider_request, deadline),
                    None => route.provider.judge(&provider_request),
                };
                check()?;
                result
                    .inspect(|judgment| {
                        cache.insert(key.clone(), judgment.clone());
                    })
                    .map(|judgment| (judgment, false))
            };
            check()?;

            match evaluated {
                Ok((judgment, cache_hit)) => {
                    JudgmentRegistry::babble_core()
                        .validate_output(&judgment.definition, &judgment.output)?;
                    check()?;
                    let accepted = judgment.confidence >= route.accept_confidence;
                    decisions.push(ProviderDecision {
                        provider: provider_version.clone(),
                        role: route.role.clone(),
                        cache_hit,
                        accepted,
                        confidence: Some(judgment.confidence),
                        accept_confidence: route.accept_confidence,
                        judgment_id: Some(judgment.id.clone()),
                        privacy,
                        error: None,
                    });
                    let should_replace = selected
                        .as_ref()
                        .map(|current: &Judgment| judgment.confidence > current.confidence)
                        .unwrap_or(true);
                    if should_replace {
                        selected = Some(judgment.clone());
                        selected_cache_hit = cache_hit;
                    }
                    if accepted {
                        selected = Some(judgment);
                        selected_cache_hit = cache_hit;
                        break;
                    }
                }
                Err(err) => {
                    decisions.push(ProviderDecision {
                        provider: provider_version,
                        role: route.role.clone(),
                        cache_hit: false,
                        accepted: false,
                        confidence: None,
                        accept_confidence: route.accept_confidence,
                        judgment_id: None,
                        privacy,
                        error: Some(err.to_string()),
                    });
                    last_error = Some(err);
                }
            }
        }

        let Some(judgment) = selected else {
            return Err(last_error.unwrap_or_else(|| {
                Error::ProviderUnavailable("judgment orchestrator produced no judgment".to_string())
            }));
        };

        Ok(OrchestratedJudgment {
            selected_provider: judgment.provider.clone(),
            judgment,
            cache_hit: selected_cache_hit,
            decisions,
        })
    }

    pub fn evaluate_batch(
        &self,
        cache: &mut JudgmentCache,
        requests: &[JudgmentRequest],
    ) -> Result<JudgmentBatchResult> {
        let mut judgments = Vec::with_capacity(requests.len());
        for request in requests {
            judgments.push(self.evaluate(cache, request)?);
        }
        Ok(JudgmentBatchResult { judgments })
    }
}

// The exclusive cache borrow keeps provisional mutations invisible. On failure,
// restore only touched entries, including their original timestamps and hits.
struct EvaluationCache<'a> {
    cache: &'a mut JudgmentCache,
    originals: Option<BTreeMap<CacheKey, Option<CachedJudgment>>>,
}

impl<'a> EvaluationCache<'a> {
    fn new(cache: &'a mut JudgmentCache, transactional: bool) -> Self {
        Self {
            cache,
            originals: transactional.then(BTreeMap::new),
        }
    }

    fn remember(&mut self, key: &CacheKey) {
        if let Some(originals) = &mut self.originals {
            originals
                .entry(key.clone())
                .or_insert_with(|| self.cache.entry(key).cloned());
        }
    }

    fn record_hit(&mut self, key: &CacheKey) -> Option<Judgment> {
        if self.cache.entry(key).is_some() {
            self.remember(key);
        }
        self.cache.record_hit(key)
    }

    fn insert(&mut self, key: CacheKey, judgment: Judgment) {
        self.remember(&key);
        self.cache.insert(key, judgment);
    }

    fn commit(&mut self) {
        self.originals = None;
    }
}

impl Drop for EvaluationCache<'_> {
    fn drop(&mut self) {
        if let Some(originals) = self.originals.take() {
            for (key, original) in originals {
                match original {
                    Some(entry) => {
                        self.cache.values.insert(key, entry);
                    }
                    None => {
                        self.cache.values.remove(&key);
                    }
                }
            }
        }
    }
}

pub fn batch_by_definition(
    requests: &[JudgmentRequest],
) -> Result<BTreeMap<DefinitionId, Vec<JudgmentRequest>>> {
    let registry = JudgmentRegistry::babble_core();
    let mut batches: BTreeMap<DefinitionId, Vec<JudgmentRequest>> = BTreeMap::new();
    for request in requests {
        registry.validate_request(request)?;
        batches
            .entry(request.definition.clone())
            .or_default()
            .push(request.clone());
    }
    Ok(batches)
}

fn truncate_utf8(value: &str, max_bytes: Option<usize>) -> (String, bool) {
    let Some(max_bytes) = max_bytes else {
        return (value.to_string(), false);
    };
    if value.len() <= max_bytes {
        return (value.to_string(), false);
    }
    let mut end = max_bytes;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    (value[..end].to_string(), true)
}
