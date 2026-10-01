//! Canonical source-agreement boundary. Scores describe supplied sources, not truth.
use crate::JudgmentRequest;
use babel_types::{Error, Result};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize, de};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_AGREEMENT_SOURCES: usize = 200;
pub const MAX_AGREEMENT_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_AGREEMENT_TOTAL_TEXT_BYTES: usize = 512 * 1024;
pub const MAX_AGREEMENT_ID_BYTES: usize = 512;
pub const MIN_AGREEMENT_TIMESTAMP: f64 = -62_167_219_200.0;
pub const MAX_AGREEMENT_TIMESTAMP: f64 = 253_402_300_800.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgreementSourceKind {
    OfficialDocs,
    ResearchPaper,
    TechnicalBlog,
    CommunityWiki,
    ForumPost,
    SocialMedia,
    Context,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AgreementState {
    Emerging,
    Provisional,
    Established,
    Contested,
    Revoked,
    Insufficient,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AgreementSource {
    #[schemars(length(min = 1, max = 512))]
    pub source_id: String,
    pub kind: AgreementSourceKind,
    #[schemars(length(max = 65536))]
    pub text: String,
    #[schemars(schema_with = "timestamp_schema")]
    pub timestamp: f64,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub quality_score: f64,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub evidence_score: f64,
    #[serde(deserialize_with = "required_nullable")]
    #[schemars(required, schema_with = "nullable_id_schema")]
    pub user_id: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    #[schemars(required, schema_with = "nullable_unit_schema")]
    pub vote: Option<f64>,
    pub is_context: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceAgreementInput {
    #[schemars(schema_with = "timestamp_schema")]
    pub reference_time: f64,
    #[serde(deserialize_with = "required_nullable")]
    #[schemars(required, schema_with = "nullable_unit_schema")]
    pub previous_score: Option<f64>,
    #[schemars(length(max = 200))]
    pub sources: Vec<AgreementSource>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceAgreementOutput {
    #[schemars(schema_with = "kind_schema")]
    pub kind: String,
    #[schemars(range(min = 0.0, max = 0.0))]
    pub confidence: f64,
    #[schemars(schema_with = "confidence_status_schema")]
    pub confidence_status: String,
    #[schemars(schema_with = "timestamp_schema")]
    pub reference_time: f64,
    #[schemars(length(max = 200))]
    pub source_ids: Vec<String>,
    #[schemars(length(min = 1, max = 512))]
    pub content_id: String,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub consensus_score: f64,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub reliability_score: f64,
    #[schemars(range(max = 200))]
    pub validation_count: usize,
    pub state: AgreementState,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub temporal_weight: f64,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub term_agreement: f64,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub fact_agreement: f64,
    #[schemars(schema_with = "contributions_schema")]
    pub user_contributions: BTreeMap<String, f64>,
    #[schemars(length(min = 1, max = 16))]
    pub limitations: Vec<String>,
}

impl SourceAgreementInput {
    pub fn from_request(request: &JudgmentRequest) -> Result<Self> {
        id(&request.state.subject)?;
        if !request.parameters.is_empty() {
            return Err(invalid("parameters must be empty"));
        }
        let value = request
            .state
            .context
            .get("source_agreement")
            .ok_or_else(|| invalid("source_agreement context is required"))?;
        let input: Self = serde_json::from_value(value.clone())
            .map_err(|_| invalid("invalid source_agreement fields"))?;
        input.validate()?;
        Ok(input)
    }

    pub fn validate(&self) -> Result<()> {
        timestamp(self.reference_time)?;
        if let Some(previous) = self.previous_score {
            unit(previous)?;
        }
        if self.sources.len() > MAX_AGREEMENT_SOURCES {
            return Err(invalid("too many sources"));
        }
        let mut ids = BTreeSet::new();
        let mut total = 0;
        for source in &self.sources {
            id(&source.source_id)?;
            if !ids.insert(&source.source_id) {
                return Err(invalid("duplicate source ID"));
            }
            if source.text.len() > MAX_AGREEMENT_TEXT_BYTES {
                return Err(invalid("source text exceeds byte limit"));
            }
            total += source.text.len();
            if total > MAX_AGREEMENT_TOTAL_TEXT_BYTES {
                return Err(invalid("total source text exceeds byte limit"));
            }
            timestamp(source.timestamp)?;
            if source.timestamp > self.reference_time {
                return Err(invalid("future source"));
            }
            unit(source.quality_score)?;
            unit(source.evidence_score)?;
            if let Some(user) = &source.user_id {
                id(user)?;
            }
            if let Some(vote) = source.vote {
                unit(vote)?;
                if source.user_id.is_none() {
                    return Err(invalid("anonymous vote"));
                }
            }
        }
        Ok(())
    }
}

impl SourceAgreementOutput {
    pub fn validate(&self) -> Result<()> {
        if self.kind != "source_agreement"
            || self.confidence != 0.0
            || self.confidence_status != "uncalibrated"
        {
            return Err(invalid("output must be explicitly uncalibrated"));
        }
        timestamp(self.reference_time)?;
        id(&self.content_id)?;
        if self.source_ids.len() > MAX_AGREEMENT_SOURCES
            || self.validation_count != self.source_ids.len()
        {
            return Err(invalid("invalid validation count"));
        }
        let mut ids = BTreeSet::new();
        for source in &self.source_ids {
            id(source)?;
            if !ids.insert(source) {
                return Err(invalid("duplicate output source ID"));
            }
        }
        for score in [
            self.consensus_score,
            self.reliability_score,
            self.temporal_weight,
            self.term_agreement,
            self.fact_agreement,
        ] {
            unit(score)?;
        }
        if self.user_contributions.len() > self.validation_count {
            return Err(invalid("too many user contributions"));
        }
        for (user, score) in &self.user_contributions {
            id(user)?;
            unit(*score)?;
        }
        if self.limitations.is_empty()
            || self.limitations.len() > 16
            || self
                .limitations
                .iter()
                .any(|text| text.trim().is_empty() || text.len() > 1024)
        {
            return Err(invalid("invalid limitations"));
        }
        Ok(())
    }

    pub fn validate_for(&self, request: &JudgmentRequest) -> Result<()> {
        self.validate()?;
        let input = SourceAgreementInput::from_request(request)?;
        if self.content_id != request.state.subject
            || self.reference_time != input.reference_time
            || !self
                .source_ids
                .iter()
                .eq(input.sources.iter().map(|s| &s.source_id))
        {
            return Err(invalid("output does not match request"));
        }
        let voters: BTreeSet<_> = input
            .sources
            .iter()
            .filter(|s| s.vote.is_some())
            .filter_map(|s| s.user_id.as_ref())
            .collect();
        if voters != self.user_contributions.keys().collect() {
            return Err(invalid("output voters do not match request"));
        }
        Ok(())
    }
}

pub fn validate_source_agreement_result(
    request: &JudgmentRequest,
    output: &serde_json::Value,
) -> Result<()> {
    let output: SourceAgreementOutput =
        serde_json::from_value(output.clone()).map_err(|_| invalid("invalid output fields"))?;
    output.validate_for(request)
}

fn required_nullable<'de, D: de::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::deserialize(d)
}

fn id(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > MAX_AGREEMENT_ID_BYTES {
        return Err(invalid("ID must be nonblank and bounded"));
    }
    Ok(())
}

fn timestamp(value: f64) -> Result<()> {
    if !value.is_finite() || !(MIN_AGREEMENT_TIMESTAMP..MAX_AGREEMENT_TIMESTAMP).contains(&value) {
        return Err(invalid("timestamp outside supported range"));
    }
    Ok(())
}

fn unit(value: f64) -> Result<()> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(invalid("score outside finite unit interval"));
    }
    Ok(())
}

fn invalid(message: &str) -> Error {
    Error::Conflict(format!("Source agreement: {message}"))
}

fn timestamp_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::json!({"type": "number", "minimum": MIN_AGREEMENT_TIMESTAMP,
        "exclusiveMaximum": MAX_AGREEMENT_TIMESTAMP})
    .try_into()
    .unwrap()
}

fn nullable_unit_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::json!({"type": ["number", "null"], "minimum": 0, "maximum": 1})
        .try_into()
        .unwrap()
}

fn nullable_id_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::json!({"type": ["string", "null"], "minLength": 1, "maxLength": 512})
        .try_into()
        .unwrap()
}

fn kind_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::json!({"type": "string", "enum": ["source_agreement"]})
        .try_into()
        .unwrap()
}

fn confidence_status_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::json!({"type": "string", "enum": ["uncalibrated"]})
        .try_into()
        .unwrap()
}

fn contributions_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    serde_json::json!({"type": "object", "maxProperties": 200,
        "propertyNames": {"minLength": 1, "maxLength": 512},
        "additionalProperties": {"type": "number", "minimum": 0, "maximum": 1}})
    .try_into()
    .unwrap()
}
