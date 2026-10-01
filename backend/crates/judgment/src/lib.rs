use babble_types::{Canonical, Error, Hash, JudgmentId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::time::Instant;

pub mod agreement;
mod orchestration;

pub use agreement::{
    AgreementSource, AgreementSourceKind, AgreementState, SourceAgreementInput,
    SourceAgreementOutput, validate_source_agreement_result,
};

pub use orchestration::{
    AppliedPrivacyPolicy, JudgmentBatchResult, JudgmentOrchestrator, JudgmentPrivacyPolicy,
    OrchestratedJudgment, ProviderDecision, ProviderRole, batch_by_definition,
};

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct DefinitionId(String);

impl DefinitionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn spam_v1() -> Self {
        Self::new("babble.judgment.spam.v1")
    }

    pub fn relevance_v1() -> Self {
        Self::new("babble.judgment.relevance.v1")
    }

    pub fn relationship_v1() -> Self {
        Self::new("babble.judgment.relationship.v1")
    }

    pub fn evidence_quality_v1() -> Self {
        Self::new("babble.judgment.evidence_quality.v1")
    }

    pub fn content_analysis_v1() -> Self {
        Self::new("babble.judgment.content_analysis.v1")
    }

    pub fn moderation_v1() -> Self {
        Self::new("babble.judgment.moderation.v1")
    }

    pub fn source_agreement_v1() -> Self {
        Self::new("babble.judgment.source_agreement.v1")
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgmentDefinition {
    pub id: DefinitionId,
    pub input_schema: String,
    pub output_schema: String,
    pub meaning: String,
    pub calibration: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgmentRegistry {
    pub definitions: Vec<JudgmentDefinition>,
}

impl JudgmentRegistry {
    pub fn babble_core() -> Self {
        Self {
            definitions: vec![
                JudgmentDefinition {
                    id: DefinitionId::spam_v1(),
                    input_schema: "babble.judgment.input.object_text.v1".to_string(),
                    output_schema: "babble.judgment.output.probability.v1".to_string(),
                    meaning: "Estimate whether the subject text is spam or manipulation.".to_string(),
                    calibration: "score is probability-like in [0, 1]; higher means more spam".to_string(),
                },
                JudgmentDefinition {
                    id: DefinitionId::relevance_v1(),
                    input_schema: "babble.judgment.input.object_text.v1".to_string(),
                    output_schema: "babble.judgment.output.bounded_score.v1".to_string(),
                    meaning: "Estimate relevance between subject text and a query.".to_string(),
                    calibration: "score is bounded in [0, 1]; higher means more relevant".to_string(),
                },
                JudgmentDefinition {
                    id: DefinitionId::relationship_v1(),
                    input_schema: "babble.judgment.input.object_text.v1".to_string(),
                    output_schema: "babble.judgment.output.relationship.v1".to_string(),
                    meaning: "Estimate whether the subject text supports, contradicts, or relates to context.".to_string(),
                    calibration: "score is bounded in [0, 1]; relation names the evaluated edge semantics".to_string(),
                },
                JudgmentDefinition {
                    id: DefinitionId::evidence_quality_v1(),
                    input_schema: "babble.judgment.input.object_text.v1".to_string(),
                    output_schema: "babble.judgment.output.bounded_score.v1".to_string(),
                    meaning: "Estimate the quality of evidence signals in subject text.".to_string(),
                    calibration: "score is bounded in [0, 1]; higher means stronger evidence quality".to_string(),
                },
                JudgmentDefinition {
                    id: DefinitionId::content_analysis_v1(),
                    input_schema: "babble.judgment.input.object_text.v1".to_string(),
                    output_schema: "babble.judgment.output.content_analysis.v1".to_string(),
                    meaning: "Extract reusable content features for discovery, provenance, and Lens inputs.".to_string(),
                    calibration: "topics, evidence markers, key terms, sentiment, and summary are descriptive features, not truth claims".to_string(),
                },
                JudgmentDefinition {
                    id: DefinitionId::moderation_v1(),
                    input_schema: "babble.judgment.input.object_text.v1".to_string(),
                    output_schema: "babble.judgment.output.moderation.v1".to_string(),
                    meaning: "Evaluate network-integrity and moderation policy signals without erasing protocol history.".to_string(),
                    calibration: "scores are bounded in [0, 1]; action is advisory policy output with reason flags".to_string(),
                },
                JudgmentDefinition {
                    id: DefinitionId::source_agreement_v1(),
                    input_schema: "babble.judgment.input.source_agreement.v1".to_string(),
                    output_schema: "babble.judgment.output.source_agreement.v1".to_string(),
                    meaning: "Describe lexical agreement among explicitly supplied sources; not truth or network consensus.".to_string(),
                    calibration: "bounded heuristic scores; confidence is zero and explicitly uncalibrated".to_string(),
                },
            ],
        }
    }

    pub fn definition(&self, id: &DefinitionId) -> Option<&JudgmentDefinition> {
        self.definitions
            .iter()
            .find(|definition| &definition.id == id)
    }

    pub fn validate_request(&self, request: &JudgmentRequest) -> Result<&JudgmentDefinition> {
        let definition = self.definition(&request.definition).ok_or_else(|| {
            Error::Conflict(format!(
                "unsupported Judgment definition: {}",
                request.definition.as_str()
            ))
        })?;
        request.validate_shape()?;
        match definition.id.as_str() {
            "babble.judgment.source_agreement.v1" => {
                SourceAgreementInput::from_request(request)?;
            }
            "babble.judgment.relevance.v1" => {
                optional_string_parameter(request, "query")?;
            }
            "babble.judgment.relationship.v1" => {
                if let Some(value) = request.parameters.get("relation") {
                    let relation = value.as_str().ok_or_else(|| {
                        Error::Conflict(
                            "relationship relation parameter must be a string".to_string(),
                        )
                    })?;
                    if !matches!(relation, "supports" | "contradicts" | "related") {
                        return Err(Error::Conflict(format!(
                            "unsupported relationship relation parameter: {relation}"
                        )));
                    }
                }
            }
            _ => {}
        }
        Ok(definition)
    }

    pub fn validate_output(&self, definition: &DefinitionId, output: &Value) -> Result<()> {
        self.definition(definition)
            .ok_or_else(|| {
                Error::Conflict(format!(
                    "unsupported Judgment definition: {}",
                    definition.as_str()
                ))
            })
            .and_then(|definition| validate_output_shape(definition, output))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ProviderVersion {
    pub provider: String,
    pub model: String,
    pub version: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgmentProviderDescriptor {
    pub provider: ProviderVersion,
    pub role: ProviderRole,
    pub supported_definitions: Vec<DefinitionId>,
    pub privacy_policy: JudgmentPrivacyPolicy,
    pub enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgmentState {
    pub subject: String,
    pub context: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JudgmentRequest {
    pub definition: DefinitionId,
    pub state: JudgmentState,
    pub parameters: BTreeMap<String, Value>,
}

impl JudgmentRequest {
    pub fn validate_shape(&self) -> Result<()> {
        validate_definition_id(self.definition.as_str())?;
        if self.state.subject.trim().is_empty() {
            return Err(Error::Conflict(
                "Judgment subject must not be empty".to_string(),
            ));
        }
        if let Some(text) = self.state.context.get("text") {
            let text = text.as_str().ok_or_else(|| {
                Error::Conflict("Judgment text context must be a string".to_string())
            })?;
            if text.trim().is_empty() {
                return Err(Error::Conflict(
                    "Judgment text context must not be empty".to_string(),
                ));
            }
        } else {
            return Err(Error::Conflict(
                "Judgment request requires text context".to_string(),
            ));
        }
        for key in self.parameters.keys() {
            validate_parameter_key(key)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Judgment {
    pub id: JudgmentId,
    pub definition: DefinitionId,
    pub provider: ProviderVersion,
    pub input_hash: Hash,
    pub output: Value,
    pub confidence: f64,
    pub created_at: Timestamp,
}

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct CacheKey {
    pub definition: DefinitionId,
    pub input_hash: Hash,
    pub provider: String,
    pub model: String,
    pub model_version: String,
    pub parameter_hash: Hash,
}

pub trait JudgmentProvider {
    fn version(&self) -> ProviderVersion;

    fn role(&self) -> ProviderRole {
        ProviderRole::Local
    }

    fn supported_definitions(&self) -> Vec<DefinitionId> {
        JudgmentRegistry::babble_core()
            .definitions
            .into_iter()
            .map(|definition| definition.id)
            .collect()
    }

    fn privacy_policy(&self) -> JudgmentPrivacyPolicy {
        match self.role() {
            ProviderRole::Local => JudgmentPrivacyPolicy::local_full(),
            ProviderRole::Remote | ProviderRole::Ensemble => {
                JudgmentPrivacyPolicy::remote_minimized()
            }
        }
    }

    fn descriptor(&self) -> JudgmentProviderDescriptor {
        JudgmentProviderDescriptor {
            provider: self.version(),
            role: self.role(),
            supported_definitions: self.supported_definitions(),
            privacy_policy: self.privacy_policy(),
            enabled: true,
        }
    }

    fn judge(&self, request: &JudgmentRequest) -> Result<Judgment>;

    /// Evaluate within a caller's shared work budget.
    ///
    /// The default checks before and after `judge`, including failed calls. It is
    /// cooperative: trusted synchronous providers cannot be interrupted while
    /// running. Providers with blocking transports must override this method to
    /// enforce the deadline throughout their exchange.
    fn judge_before(&self, request: &JudgmentRequest, deadline: Instant) -> Result<Judgment> {
        check_deadline(deadline)?;
        let result = self.judge(request);
        check_deadline(deadline)?;
        result
    }
}

fn check_deadline(deadline: Instant) -> Result<()> {
    if Instant::now() >= deadline {
        return Err(Error::ProviderUnavailable(
            "Judgment deadline exceeded".to_string(),
        ));
    }
    Ok(())
}

pub struct CascadeStep<'a> {
    provider: &'a dyn JudgmentProvider,
    accept_confidence: f64,
}

impl<'a> CascadeStep<'a> {
    pub fn new(provider: &'a dyn JudgmentProvider, accept_confidence: f64) -> Self {
        Self {
            provider,
            accept_confidence: accept_confidence.clamp(0.0, 1.0),
        }
    }
}

pub struct ProviderCascade<'a> {
    version: ProviderVersion,
    steps: Vec<CascadeStep<'a>>,
}

impl<'a> ProviderCascade<'a> {
    pub fn new(version: ProviderVersion) -> Self {
        Self {
            version,
            steps: Vec::new(),
        }
    }

    pub fn with_provider(
        mut self,
        provider: &'a dyn JudgmentProvider,
        accept_confidence: f64,
    ) -> Self {
        self.steps
            .push(CascadeStep::new(provider, accept_confidence));
        self
    }

    pub fn push_provider(&mut self, provider: &'a dyn JudgmentProvider, accept_confidence: f64) {
        self.steps
            .push(CascadeStep::new(provider, accept_confidence));
    }
}

impl JudgmentProvider for ProviderCascade<'_> {
    fn version(&self) -> ProviderVersion {
        self.version.clone()
    }

    fn judge(&self, request: &JudgmentRequest) -> Result<Judgment> {
        JudgmentRegistry::babble_core().validate_request(request)?;
        if self.steps.is_empty() {
            return Err(Error::ProviderUnavailable(
                "judgment cascade has no providers".to_string(),
            ));
        }

        let input_hash = request.state.canonical_hash()?;
        let mut evaluated = Vec::new();
        let mut selected = None;
        let mut last_error = None;

        for step in &self.steps {
            let provider_version = step.provider.version();
            match step.provider.judge(request) {
                Ok(judgment) => {
                    evaluated.push(CascadeTraceEntry {
                        provider: provider_version,
                        confidence: judgment.confidence,
                        accepted: judgment.confidence >= step.accept_confidence,
                        error: None,
                    });
                    let should_replace = selected
                        .as_ref()
                        .map(|current: &Judgment| judgment.confidence > current.confidence)
                        .unwrap_or(true);
                    if should_replace {
                        selected = Some(judgment.clone());
                    }
                    if judgment.confidence >= step.accept_confidence {
                        selected = Some(judgment);
                        break;
                    }
                }
                Err(err) => {
                    evaluated.push(CascadeTraceEntry {
                        provider: provider_version,
                        confidence: 0.0,
                        accepted: false,
                        error: Some(err.to_string()),
                    });
                    last_error = Some(err);
                }
            }
        }

        let Some(selected) = selected else {
            return Err(last_error.unwrap_or_else(|| {
                Error::ProviderUnavailable("judgment cascade produced no judgment".to_string())
            }));
        };

        let output = serde_json::json!({
            "kind": "cascade",
            "selected_provider": selected.provider,
            "selected_judgment_id": selected.id,
            "selected_output": selected.output,
            "evaluated": evaluated,
        });
        let commitment = (
            request.definition.clone(),
            self.version.clone(),
            input_hash.clone(),
            request.parameters.clone(),
            output.clone(),
        );
        let id = JudgmentId::from_hash(&commitment.canonical_hash()?);

        let judgment = Judgment {
            id,
            definition: request.definition.clone(),
            provider: self.version.clone(),
            input_hash,
            output,
            confidence: selected.confidence,
            created_at: Timestamp::now(),
        };
        JudgmentRegistry::babble_core().validate_output(&judgment.definition, &judgment.output)?;
        Ok(judgment)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
struct CascadeTraceEntry {
    provider: ProviderVersion,
    confidence: f64,
    accepted: bool,
    error: Option<String>,
}

#[derive(Default)]
pub struct JudgmentCache {
    values: BTreeMap<CacheKey, CachedJudgment>,
}

impl JudgmentCache {
    pub fn get(&self, key: &CacheKey) -> Option<&Judgment> {
        self.values.get(key).map(|entry| &entry.judgment)
    }

    pub fn entry(&self, key: &CacheKey) -> Option<&CachedJudgment> {
        self.values.get(key)
    }

    pub fn insert(&mut self, key: CacheKey, judgment: Judgment) {
        self.values.insert(
            key,
            CachedJudgment {
                judgment,
                created_at: Timestamp::now(),
                hits: 0,
            },
        );
    }

    pub fn record_hit(&mut self, key: &CacheKey) -> Option<Judgment> {
        let entry = self.values.get_mut(key)?;
        entry.hits = entry.hits.saturating_add(1);
        Some(entry.judgment.clone())
    }

    pub fn get_or_evaluate<P: JudgmentProvider>(
        &mut self,
        provider: &P,
        request: &JudgmentRequest,
    ) -> Result<Judgment> {
        JudgmentRegistry::babble_core().validate_request(request)?;
        let key = cache_key(&provider.version(), request)?;
        if let Some(cached) = self.record_hit(&key) {
            return Ok(cached);
        }
        let judgment = provider.judge(request)?;
        JudgmentRegistry::babble_core().validate_output(&judgment.definition, &judgment.output)?;
        self.insert(key, judgment.clone());
        Ok(judgment)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CachedJudgment {
    pub judgment: Judgment,
    pub created_at: Timestamp,
    pub hits: u64,
}

pub fn cache_key(provider: &ProviderVersion, request: &JudgmentRequest) -> Result<CacheKey> {
    JudgmentRegistry::babble_core().validate_request(request)?;
    Ok(CacheKey {
        definition: request.definition.clone(),
        input_hash: request.state.canonical_hash()?,
        provider: provider.provider.clone(),
        model: provider.model.clone(),
        model_version: provider.version.clone(),
        parameter_hash: request.parameters.canonical_hash()?,
    })
}

pub struct ConstantProvider {
    version: ProviderVersion,
    output: Value,
    confidence: f64,
}

impl ConstantProvider {
    pub fn new(version: ProviderVersion, output: Value, confidence: f64) -> Self {
        Self {
            version,
            output,
            confidence: confidence.clamp(0.0, 1.0),
        }
    }
}

impl Default for ConstantProvider {
    fn default() -> Self {
        Self {
            version: ProviderVersion {
                provider: "babble-constant".to_string(),
                model: "deterministic-baseline".to_string(),
                version: "1".to_string(),
            },
            output: serde_json::json!({
                "kind": "bounded_score",
                "score": 0.0,
                "reason": "constant baseline provider"
            }),
            confidence: 0.0,
        }
    }
}

impl JudgmentProvider for ConstantProvider {
    fn version(&self) -> ProviderVersion {
        self.version.clone()
    }

    fn judge(&self, request: &JudgmentRequest) -> Result<Judgment> {
        JudgmentRegistry::babble_core().validate_request(request)?;
        let input_hash = request.state.canonical_hash()?;
        let commitment = (
            request.definition.clone(),
            self.version.clone(),
            input_hash.clone(),
            request.parameters.clone(),
            self.output.clone(),
        );
        let id = JudgmentId::from_hash(&commitment.canonical_hash()?);
        let judgment = Judgment {
            id,
            definition: request.definition.clone(),
            provider: self.version.clone(),
            input_hash,
            output: self.output.clone(),
            confidence: self.confidence,
            created_at: Timestamp::now(),
        };
        JudgmentRegistry::babble_core().validate_output(&judgment.definition, &judgment.output)?;
        Ok(judgment)
    }
}

fn validate_output_shape(definition: &JudgmentDefinition, output: &Value) -> Result<()> {
    let object = output.as_object().ok_or_else(|| {
        Error::Conflict(format!(
            "Judgment output for {} must be a JSON object",
            definition.id.as_str()
        ))
    })?;
    if object.get("kind").and_then(Value::as_str) == Some("cascade") {
        validate_cascade_output(definition, object)?;
        return Ok(());
    }
    match definition.output_schema.as_str() {
        "babble.judgment.output.source_agreement.v1" => {
            let result: SourceAgreementOutput = serde_json::from_value(output.clone())
                .map_err(|_| Error::Conflict("invalid source agreement output fields".into()))?;
            result.validate()?;
        }
        "babble.judgment.output.probability.v1" => {
            require_kind(object, "probability")?;
            bounded_number(object, "score")?;
            bounded_number(object, "confidence")?;
            non_empty_string(object, "label")?;
        }
        "babble.judgment.output.bounded_score.v1" => {
            require_kind(object, "bounded_score")?;
            bounded_number(object, "score")?;
            if object.contains_key("confidence") {
                bounded_number(object, "confidence")?;
            }
        }
        "babble.judgment.output.relationship.v1" => {
            require_kind(object, "relationship")?;
            bounded_number(object, "score")?;
            bounded_number(object, "confidence")?;
            let relation = non_empty_string(object, "relation")?;
            if !matches!(relation, "supports" | "contradicts" | "related") {
                return Err(Error::Conflict(format!(
                    "unsupported relationship output relation: {relation}"
                )));
            }
        }
        "babble.judgment.output.content_analysis.v1" => {
            require_kind(object, "content_analysis")?;
            array_of_strings(object, "topics")?;
            array_of_strings(object, "evidence_markers")?;
            array_of_strings(object, "key_terms")?;
            non_empty_string(object, "summary")?;
            bounded_number(object, "sentiment")?;
            bounded_number(object, "confidence")?;
        }
        "babble.judgment.output.moderation.v1" => {
            require_kind(object, "moderation")?;
            let action = non_empty_string(object, "action")?;
            if !matches!(action, "allow" | "limit" | "flag" | "remove") {
                return Err(Error::Conflict(format!(
                    "unsupported moderation action: {action}"
                )));
            }
            array_of_strings(object, "flags")?;
            for field in [
                "spam",
                "quality",
                "safety",
                "coordination",
                "misinformation",
                "confidence",
            ] {
                bounded_number(object, field)?;
            }
        }
        schema => {
            return Err(Error::Conflict(format!(
                "unsupported Judgment output schema: {schema}"
            )));
        }
    }
    Ok(())
}

fn validate_cascade_output(
    definition: &JudgmentDefinition,
    object: &serde_json::Map<String, Value>,
) -> Result<()> {
    require_kind(object, "cascade")?;
    object
        .get("selected_provider")
        .ok_or_else(|| Error::Conflict("cascade output missing selected_provider".to_string()))?;
    object.get("selected_judgment_id").ok_or_else(|| {
        Error::Conflict("cascade output missing selected_judgment_id".to_string())
    })?;
    object
        .get("evaluated")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Conflict("cascade evaluated must be an array".to_string()))?;
    let selected = object
        .get("selected_output")
        .ok_or_else(|| Error::Conflict("cascade output missing selected_output".to_string()))?;
    validate_output_shape(definition, selected)
}

fn require_kind<'a>(object: &'a serde_json::Map<String, Value>, expected: &str) -> Result<&'a str> {
    let kind = non_empty_string(object, "kind")?;
    if kind == expected {
        Ok(kind)
    } else {
        Err(Error::Conflict(format!(
            "Judgment output kind must be {expected}, got {kind}"
        )))
    }
}

fn bounded_number(object: &serde_json::Map<String, Value>, field: &str) -> Result<f64> {
    let value = object
        .get(field)
        .and_then(Value::as_f64)
        .ok_or_else(|| Error::Conflict(format!("Judgment output {field} must be a number")))?;
    if (0.0..=1.0).contains(&value) {
        Ok(value)
    } else {
        Err(Error::Conflict(format!(
            "Judgment output {field} must be in [0, 1]"
        )))
    }
}

fn non_empty_string<'a>(
    object: &'a serde_json::Map<String, Value>,
    field: &str,
) -> Result<&'a str> {
    let value = object
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Conflict(format!("Judgment {field} must be a string")))?;
    if value.trim().is_empty() {
        Err(Error::Conflict(format!(
            "Judgment {field} must not be empty"
        )))
    } else {
        Ok(value)
    }
}

fn array_of_strings(object: &serde_json::Map<String, Value>, field: &str) -> Result<()> {
    let values = object
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| Error::Conflict(format!("Judgment {field} must be an array")))?;
    for value in values {
        let item = value
            .as_str()
            .ok_or_else(|| Error::Conflict(format!("Judgment {field} entries must be strings")))?;
        if item.trim().is_empty() {
            return Err(Error::Conflict(format!(
                "Judgment {field} entries must not be empty"
            )));
        }
    }
    Ok(())
}

fn optional_string_parameter(request: &JudgmentRequest, field: &str) -> Result<()> {
    if let Some(value) = request.parameters.get(field) {
        value.as_str().ok_or_else(|| {
            Error::Conflict(format!("Judgment parameter {field} must be a string"))
        })?;
    }
    Ok(())
}

fn validate_definition_id(value: &str) -> Result<()> {
    if namespaced(value) {
        Ok(())
    } else {
        Err(Error::Conflict(format!(
            "Judgment definition must be namespaced: {value}"
        )))
    }
}

fn validate_parameter_key(value: &str) -> Result<()> {
    let valid = !value.trim().is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    if valid {
        Ok(())
    } else {
        Err(Error::Conflict(format!(
            "invalid Judgment parameter key: {value}"
        )))
    }
}

fn namespaced(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && value.contains('.') && !value.contains(char::is_whitespace)
}
