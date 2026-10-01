//! Canonical temporal scoring contract. Providers are selected explicitly.
use babel_types::{Error, ObjectId, Result, Timestamp};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

const MAX_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
// 10,000 Gregorian years plus the maximum opposing RFC3339 offsets.
const MAX_AGE_HOURS: f64 = 87_658_248.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemporalRequest {
    #[serde(deserialize_with = "deserialize_timestamp")]
    #[schemars(schema_with = "timestamp_schema")]
    pub reference_time: Timestamp,
    #[schemars(length(max = 200))]
    pub items: Vec<TemporalItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemporalItem {
    #[schemars(regex(pattern = "^obj_[0-9a-f]{64}$"))]
    pub object_id: ObjectId,
    #[schemars(schema_with = "timestamp_schema")]
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub published_at: Timestamp,
    pub content_class: TemporalClass,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub quality_score: f64,
    /// At most 32 tags, each at most 64 UTF-8 bytes (validated at runtime).
    #[schemars(length(max = 32), inner(length(max = 64)))]
    pub tags: Vec<String>,
    pub engagement: TemporalEngagement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TemporalClass {
    News,
    Discussion,
    Analysis,
    Tutorial,
    Reference,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemporalEngagement {
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub total_views: u64,
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub recent_views: u64,
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub total_interactions: u64,
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    pub recent_interactions: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemporalProviderVersion {
    #[schemars(length(min = 1, max = 128), regex(pattern = "^[!-~]+$"))]
    pub provider: String,
    #[schemars(length(min = 1, max = 128), regex(pattern = "^[!-~]+$"))]
    pub model: String,
    #[schemars(length(min = 1, max = 128), regex(pattern = "^[!-~]+$"))]
    pub version: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemporalResult {
    pub provider: TemporalProviderVersion,
    #[schemars(schema_with = "timestamp_schema")]
    #[serde(deserialize_with = "deserialize_timestamp")]
    pub reference_time: Timestamp,
    #[schemars(length(max = 200))]
    pub scores: Vec<TemporalScore>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemporalScore {
    #[schemars(regex(pattern = "^obj_[0-9a-f]{64}$"))]
    pub object_id: ObjectId,
    #[schemars(range(min = 0.0, max = 87_658_248.0))]
    pub age_hours: f64,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub recency: f64,
    #[schemars(range(min = 0.01, max = 0.5))]
    pub decay_rate: f64,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub time_sensitivity: f64,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub engagement_velocity: f64,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub survival_score: f64,
}

pub trait TemporalProvider: Send + Sync {
    fn version(&self) -> TemporalProviderVersion;
    fn score(&self, request: &TemporalRequest) -> Result<TemporalResult>;
}

impl TemporalRequest {
    pub fn validate(&self) -> Result<()> {
        timestamp(self.reference_time)?;
        require(self.items.len() <= 200, "at most 200 items")?;
        let mut ids = BTreeSet::new();
        for item in &self.items {
            let id = item.object_id.as_str();
            require(
                id.strip_prefix("obj_").is_some_and(|hash| {
                    hash.len() == 64
                        && hash
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                }),
                "object ID must be canonical",
            )?;
            require(ids.insert(&item.object_id), "duplicate object ID")?;
            timestamp(item.published_at)?;
            number(item.quality_score, 0.0, 1.0, "quality score")?;
            require(
                item.tags.len() <= 32 && item.tags.iter().all(|tag| tag.len() <= 64),
                "tags exceed byte or count limit",
            )?;
            let e = item.engagement;
            require(
                e.total_views <= MAX_SAFE_INTEGER
                    && e.total_interactions <= MAX_SAFE_INTEGER
                    && e.recent_views <= e.total_views
                    && e.recent_interactions <= e.total_interactions,
                "invalid engagement counts",
            )?;
        }
        Ok(())
    }
}

impl TemporalResult {
    /// Validate identity, scope and bounds without substituting native scoring
    /// for the provider's output. Parity is established separately in tests.
    pub fn validate_for(
        &self,
        request: &TemporalRequest,
        expected_provider: &TemporalProviderVersion,
    ) -> Result<()> {
        request.validate()?;
        require(&self.provider == expected_provider, "unexpected provider")?;
        for value in [
            &self.provider.provider,
            &self.provider.model,
            &self.provider.version,
        ] {
            require(
                (1..=128).contains(&value.len()) && value.bytes().all(|b| b.is_ascii_graphic()),
                "invalid provider identity",
            )?;
        }
        timestamp(self.reference_time)?;
        require(
            self.reference_time == request.reference_time
                && self.reference_time.0.offset() == request.reference_time.0.offset(),
            "reference time changed",
        )?;
        require(
            self.scores.len() == request.items.len(),
            "score count mismatch",
        )?;
        for (score, item) in self.scores.iter().zip(&request.items) {
            require(
                score.object_id == item.object_id,
                "score ID or order mismatch",
            )?;
            number(score.age_hours, 0.0, MAX_AGE_HOURS, "age hours")?;
            let expected_age = age_hours(request.reference_time, item.published_at);
            require(
                (score.age_hours - expected_age).abs()
                    <= (4.0 * f64::EPSILON * expected_age.abs()).max(1e-12),
                "age does not match timestamps",
            )?;
            number(score.decay_rate, 0.01, 0.5, "decay rate")?;
            for value in [
                score.recency,
                score.time_sensitivity,
                score.engagement_velocity,
                score.survival_score,
            ] {
                number(value, 0.0, 1.0, "temporal score")?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeTemporalScorer;

impl TemporalProvider for NativeTemporalScorer {
    fn version(&self) -> TemporalProviderVersion {
        TemporalProviderVersion {
            provider: "babel-rust".into(),
            model: "temporal-v1".into(),
            version: "1".into(),
        }
    }

    fn score(&self, request: &TemporalRequest) -> Result<TemporalResult> {
        request.validate()?;
        let scores = request
            .items
            .iter()
            .map(|item| {
                let age_hours = age_hours(request.reference_time, item.published_at);
                let (weight, sensitivity) = match item.content_class {
                    TemporalClass::News => (1.2, 0.92),
                    TemporalClass::Discussion => (1.0, 0.68),
                    TemporalClass::Analysis => (0.82, 0.5),
                    TemporalClass::Tutorial => (0.66, 0.28),
                    TemporalClass::Reference => (0.45, 0.12),
                };
                let base = if age_hours <= 2.0 {
                    1.0
                } else if age_hours <= 24.0 {
                    0.82
                } else if age_hours <= 72.0 {
                    0.62
                } else if age_hours <= 168.0 {
                    0.42
                } else if age_hours <= 720.0 {
                    0.23
                } else {
                    0.1
                };
                let recency = unit(base * weight);
                let e = item.engagement;
                let engagement_velocity = if age_hours <= 0.0 {
                    0.0
                } else {
                    unit(
                        (0.62 * (e.recent_views as f64 / e.total_views.max(1) as f64)
                            + 0.38
                                * (e.recent_interactions as f64
                                    / e.total_interactions.max(1) as f64))
                            * (-age_hours / 168.0).exp(),
                    )
                };
                let mut time_sensitivity = sensitivity;
                if item
                    .tags
                    .iter()
                    .any(|tag| tag_matches(tag, "time-sensitive") || tag_matches(tag, "breaking"))
                {
                    time_sensitivity += 0.18;
                }
                if item
                    .tags
                    .iter()
                    .any(|tag| tag_matches(tag, "evergreen") || tag_matches(tag, "reference"))
                {
                    time_sensitivity -= 0.18;
                }
                let time_sensitivity = unit(time_sensitivity);
                let decay_rate = (0.1 + 0.16 * time_sensitivity
                    - 0.12 * item.quality_score
                    - 0.1 * engagement_velocity)
                    .clamp(0.01, 0.5);
                let survival_score = unit(
                    recency * (-decay_rate * age_hours / 24.0).exp() + 0.22 * engagement_velocity,
                );
                TemporalScore {
                    object_id: item.object_id.clone(),
                    age_hours,
                    recency,
                    decay_rate,
                    time_sensitivity,
                    engagement_velocity,
                    survival_score,
                }
            })
            .collect();
        let result = TemporalResult {
            provider: self.version(),
            reference_time: request.reference_time,
            scores,
        };
        result.validate_for(request, &self.version())?;
        Ok(result)
    }
}

fn age_hours(reference: Timestamp, published: Timestamp) -> f64 {
    // Subtract before float conversion to retain nanosecond age boundaries.
    (reference.0.unix_timestamp_nanos() - published.0.unix_timestamp_nanos()).max(0) as f64
        / 3_600_000_000_000.0
}

fn tag_matches(tag: &str, keyword: &str) -> bool {
    // Only long s and Kelvin sign casefold from non-ASCII to single ASCII
    // letters. Multi-character folds cannot occur in these four keywords.
    tag.chars()
        .map(|c| match c {
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            c => c.to_ascii_lowercase(),
        })
        .eq(keyword.chars())
}

fn timestamp(value: Timestamp) -> Result<()> {
    require(
        (0..=9999).contains(&value.0.year()) && value.0.offset().seconds_past_minute() == 0,
        "timestamp must be RFC3339 serializable",
    )
}

fn deserialize_timestamp<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Timestamp, D::Error> {
    let text = String::deserialize(deserializer)?;
    if !matches!(text.as_bytes().get(10), Some(b'T' | b't')) {
        return Err(serde::de::Error::custom(
            "invalid temporal timestamp separator",
        ));
    }
    Timestamp::deserialize(serde::de::value::StrDeserializer::<D::Error>::new(&text))
}

fn timestamp_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({
        "type": "string",
        "format": "date-time",
        "description": "RFC3339 timestamp with year 0000..9999 and whole-minute UTC offset.",
        "pattern": "^[0-9]{4}-[0-9]{2}-[0-9]{2}[Tt][0-9]{2}:[0-9]{2}:[0-9]{2}(\\.[0-9]+)?([Zz]|[+-][0-9]{2}:[0-9]{2})$"
    })
}

fn unit(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}
fn require(condition: bool, message: &str) -> Result<()> {
    if condition {
        Ok(())
    } else {
        Err(Error::Canonical(format!(
            "invalid temporal contract: {message}"
        )))
    }
}
fn number(value: f64, min: f64, max: f64, message: &str) -> Result<()> {
    require(value.is_finite() && (min..=max).contains(&value), message)
}
