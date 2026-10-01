//! Versioned wire DTOs and schemas. Python returns output, never Judgment records.
use crate::{invalid, unavailable};
use babble_discovery::{TemporalProviderVersion, TemporalRequest, TemporalResult};
use babble_judgment::{DefinitionId, JudgmentRegistry, JudgmentRequest, ProviderVersion};
use babble_lens::{RankingProviderVersion, RankingRequest, RankingResult};
use babble_types::Result;
use schemars::JsonSchema;
use serde::{
    Deserialize, Serialize,
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use serde_json::{Map, Value};
use std::{
    cell::Cell,
    fmt,
    io::{self, Write},
};

pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_JUDGMENT_LINE_BYTES: usize = 1024 * 1024;
pub const MAX_ID: u64 = 9_007_199_254_740_991;
pub const MAX_DEPTH: usize = 16;
pub const MAX_NODES: usize = 200_000;
pub const MAX_JUDGMENT_NODES: usize = 4096;
pub const MAX_ARRAY_ITEMS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Protocol {
    #[serde(rename = "babble.algorithms.v1")]
    V1,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Request {
    pub protocol: Protocol,
    #[schemars(range(min = 1, max = 9_007_199_254_740_991_u64))]
    pub id: u64,
    #[serde(flatten)]
    pub method: Method,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", rename_all = "snake_case", deny_unknown_fields)]
pub enum Method {
    Health,
    Judge {
        #[schemars(schema_with = "judgment_request_schema")]
        request: JudgmentRequest,
    },
    Rank {
        request: RankingRequest,
    },
    Temporal {
        request: TemporalRequest,
    },
}

impl Request {
    pub fn health(id: u64) -> Self {
        Self {
            protocol: Protocol::V1,
            id,
            method: Method::Health,
        }
    }

    pub fn judge(id: u64, request: JudgmentRequest) -> Self {
        Self {
            protocol: Protocol::V1,
            id,
            method: Method::Judge { request },
        }
    }

    pub fn rank(id: u64, request: RankingRequest) -> Self {
        Self {
            protocol: Protocol::V1,
            id,
            method: Method::Rank { request },
        }
    }

    pub fn temporal(id: u64, request: TemporalRequest) -> Self {
        Self {
            protocol: Protocol::V1,
            id,
            method: Method::Temporal { request },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub protocol: Protocol,
    #[serde(deserialize_with = "required_nullable")]
    #[schemars(required, range(min = 1, max = 9_007_199_254_740_991_u64))]
    pub id: Option<u64>,
    #[serde(deserialize_with = "required_nullable")]
    #[schemars(required)]
    pub result: Option<WorkerResult>,
    #[serde(deserialize_with = "required_nullable")]
    #[schemars(required)]
    pub error: Option<WorkerError>,
}

fn required_nullable<'de, D: de::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> std::result::Result<Option<T>, D::Error> {
    Option::deserialize(d)
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum WorkerResult {
    Health(HealthResult),
    Judge(JudgeResult),
    Rank(RankingResult),
    Temporal(TemporalResult),
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HealthResult {
    pub provider: ProviderVersion,
    pub supported_definitions: Vec<DefinitionId>,
    pub ranking_provider: RankingProviderVersion,
    pub temporal_provider: TemporalProviderVersion,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JudgeResult {
    pub provider: ProviderVersion,
    pub output: Value,
    #[schemars(range(min = 0.0, max = 1.0))]
    pub confidence: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WorkerError {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidRequest,
    UnsupportedDefinition,
    AlgorithmFailure,
}

pub fn provider() -> ProviderVersion {
    ProviderVersion {
        provider: "babble-python".into(),
        model: "lexical-v1".into(),
        version: "1".into(),
    }
}

pub fn ranking_provider() -> RankingProviderVersion {
    RankingProviderVersion {
        provider: "babble-python".into(),
        model: "lenses-v1".into(),
        version: "1".into(),
    }
}

pub fn definitions() -> Vec<DefinitionId> {
    JudgmentRegistry::babble_core()
        .definitions
        .into_iter()
        .map(|definition| definition.id)
        .collect()
}

pub fn temporal_provider() -> TemporalProviderVersion {
    TemporalProviderVersion {
        provider: "babble-python".into(),
        model: "temporal-v1".into(),
        version: "1".into(),
    }
}

/// A portable bundle generated directly from the Rust versioned contract.
pub fn schemas() -> Value {
    let mut request = serde_json::to_value(schemars::schema_for!(Request)).unwrap();
    request["unevaluatedProperties"] = false.into();
    for name in ["JudgmentRequest", "JudgmentState"] {
        request["$defs"][name]["additionalProperties"] = false.into();
    }
    // Optional parameters may be absent, but explicit null is invalid on the wire.
    for name in [
        "RelevanceParameters",
        "RelationshipParameters",
        "ModerationParameters",
        "ModerationContext",
        "ModerationPolicy",
    ] {
        if let Some(properties) = request["$defs"][name]["properties"].as_object_mut() {
            for property in properties.values_mut() {
                if let Some(types) = property.get_mut("type").and_then(Value::as_array_mut) {
                    types.retain(|kind| kind != "null");
                }
                if let Some(choices) = property.get_mut("anyOf").and_then(Value::as_array_mut) {
                    choices
                        .retain(|choice| choice.get("type") != Some(&Value::String("null".into())));
                }
            }
        }
    }
    let mut response = serde_json::to_value(schemars::schema_for!(Response)).unwrap();
    // Required fields with nullable values differ from optional fields in this protocol.
    for field in ["id", "result", "error"] {
        let schema = response["properties"][field].take();
        response["properties"][field] = serde_json::json!({"anyOf": [schema, {"type": "null"}]});
    }
    response["oneOf"] = serde_json::json!([
        {"properties": {"result": {"type": "object"}, "error": {"type": "null"}}},
        {"properties": {"result": {"type": "null"}, "error": {"type": "object"}}}
    ]);
    response["$defs"]["ProviderVersion"]["additionalProperties"] = false.into();
    for (key, value) in [
        ("provider", "babble-python"),
        ("model", "lexical-v1"),
        ("version", "1"),
    ] {
        response["$defs"]["ProviderVersion"]["properties"][key]["const"] = value.into();
    }
    for (key, value) in [
        ("provider", "babble-python"),
        ("model", "temporal-v1"),
        ("version", "1"),
    ] {
        response["$defs"]["TemporalProviderVersion"]["properties"][key]["const"] = value.into();
    }
    serde_json::json!({
        "protocol": Protocol::V1,
        "request": request,
        "response": response,
        "limits": {"line_bytes": MAX_LINE_BYTES, "text_bytes": 131072, "subject_bytes": 4096,
            "map_entries": 64, "json_depth": MAX_DEPTH, "max_id": MAX_ID,
            "array_items": MAX_ARRAY_ITEMS, "json_nodes": MAX_NODES,
            "judgment_line_bytes": MAX_JUDGMENT_LINE_BYTES, "judgment_json_nodes": MAX_JUDGMENT_NODES},
        "provider": provider(),
        "ranking_provider": ranking_provider(),
        "temporal_provider": temporal_provider(),
        "supported_definitions": definitions(),
    })
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RelevanceParameters {
    query: Option<String>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct RelationshipParameters {
    relation: Option<Relation>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Relation {
    Supports,
    Contradicts,
    Related,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ModerationParameters {
    context: Option<ModerationContext>,
    policy: Option<ModerationPolicy>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ModerationContext {
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    repeated_messages: Option<u64>,
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    account_age_days: Option<f64>,
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    reports: Option<u64>,
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    similar_recent_posts: Option<u64>,
    #[schemars(range(min = 0, max = 9_007_199_254_740_991_u64))]
    external_links: Option<u64>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ModerationPolicy {
    #[schemars(range(min = 0.0, max = 1.0))]
    spam_limit: Option<f64>,
    #[schemars(range(min = 0.0, max = 1.0))]
    quality_warn: Option<f64>,
    #[schemars(range(min = 0.0, max = 1.0))]
    safety_remove: Option<f64>,
    #[schemars(range(min = 0.0, max = 1.0))]
    coordination_limit: Option<f64>,
}

fn judgment_request_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
    let base = generator.subschema_for::<JudgmentRequest>();
    let agreement = generator.subschema_for::<babble_judgment::SourceAgreementInput>();
    let empty = serde_json::json!({"type": "object", "additionalProperties": false});
    let cases: Vec<_> = definitions().into_iter().map(|definition| {
        let parameters = match definition.as_str() {
            "babble.judgment.relevance.v1" => serde_json::to_value(generator.subschema_for::<RelevanceParameters>()).unwrap(),
            "babble.judgment.relationship.v1" => serde_json::to_value(generator.subschema_for::<RelationshipParameters>()).unwrap(),
            "babble.judgment.moderation.v1" => serde_json::to_value(generator.subschema_for::<ModerationParameters>()).unwrap(),
            _ => empty.clone(),
        };
        let mut case = serde_json::json!({"properties": {"definition": {"const": definition}, "parameters": parameters}});
        if definition == DefinitionId::source_agreement_v1() {
            case["properties"]["state"] = serde_json::json!({"properties": {
                "subject": {"maxLength": 512},
                "context": {"required": ["source_agreement"], "properties": {"source_agreement": agreement}}
            }});
        }
        case
    }).collect();
    serde_json::json!({"allOf": [base, {"oneOf": cases}]})
        .try_into()
        .unwrap()
}

pub(crate) fn validate_request(request: &JudgmentRequest) -> Result<()> {
    // Bound recursive serialization/hashing before either sees caller-owned data.
    let mut values: Vec<_> = request
        .state
        .context
        .values()
        .map(|value| (value, 5))
        .chain(request.parameters.values().map(|value| (value, 4)))
        .collect();
    if request.state.subject.len() > 4096
        || request.state.context.len() > 64
        || request.state.context.keys().any(|key| key.len() > 4096)
        || request.parameters.len() > 64
        || request
            .state
            .context
            .get("text")
            .and_then(Value::as_str)
            .is_none_or(|text| text.len() > 131072)
    {
        return Err(invalid("request exceeds worker limits"));
    }
    // Depth-first traversal uses at most the already bounded wire size in memory.
    let mut budget = MAX_JUDGMENT_NODES - 10;
    while let Some((value, depth)) = values.pop() {
        if (depth > MAX_DEPTH && (value.is_array() || value.is_object())) || budget == 0 {
            return Err(invalid("request exceeds worker limits"));
        }
        budget -= 1;
        match value {
            Value::Array(items) => {
                if items.len() > MAX_ARRAY_ITEMS || items.len() > budget {
                    return Err(invalid("request exceeds worker limits"));
                }
                values.extend(items.iter().map(|value| (value, depth + 1)));
            }
            Value::Object(items) => {
                if items.len() > 64 || items.keys().any(|key| key.len() > 4096) {
                    return Err(invalid("request exceeds worker limits"));
                }
                values.extend(items.values().map(|value| (value, depth + 1)));
            }
            Value::String(text) if text.len() > 131072 => {
                return Err(invalid("request exceeds worker limits"));
            }
            _ => {}
        }
    }
    JudgmentRegistry::babble_core()
        .validate_request(request)
        .map_err(|_| invalid("invalid Judgment request"))?;
    validate_parameters(request)?;
    // Serialization overhead and escaped strings also count against the line cap.
    #[derive(Serialize)]
    struct BorrowedRequest<'a> {
        protocol: Protocol,
        id: u64,
        method: &'static str,
        request: &'a JudgmentRequest,
    }
    encode_value(
        &BorrowedRequest {
            protocol: Protocol::V1,
            id: MAX_ID,
            method: "judge",
            request,
        },
        MAX_JUDGMENT_LINE_BYTES,
    )?;
    Ok(())
}

fn validate_parameters(request: &JudgmentRequest) -> Result<()> {
    let allowed: &[&str] = match request.definition.as_str() {
        "babble.judgment.relevance.v1" => &["query"],
        "babble.judgment.relationship.v1" => &["relation"],
        "babble.judgment.moderation.v1" => &["context", "policy"],
        _ => &[],
    };
    if request
        .parameters
        .keys()
        .any(|key| !allowed.contains(&key.as_str()))
    {
        return Err(invalid("unsupported parameter"));
    }
    for (name, value) in &request.parameters {
        if name == "context" || name == "policy" {
            let values = value
                .as_object()
                .ok_or_else(|| invalid("parameter must be an object"))?;
            for (key, value) in values {
                let valid = if name == "policy" {
                    [
                        "spam_limit",
                        "quality_warn",
                        "safety_remove",
                        "coordination_limit",
                    ]
                    .contains(&key.as_str())
                        && value
                            .as_f64()
                            .is_some_and(|number| (0.0..=1.0).contains(&number))
                } else if key == "account_age_days" {
                    value
                        .as_f64()
                        .is_some_and(|number| (0.0..=MAX_ID as f64).contains(&number))
                } else {
                    [
                        "repeated_messages",
                        "reports",
                        "similar_recent_posts",
                        "external_links",
                    ]
                    .contains(&key.as_str())
                        && value.as_u64().is_some_and(|number| number <= MAX_ID)
                };
                if !valid {
                    return Err(invalid("invalid moderation parameter"));
                }
            }
        }
    }
    Ok(())
}

struct LimitedLine(Vec<u8>, usize);
impl Write for LimitedLine {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > self.1 - 1 - self.0.len() {
            return Err(io::Error::other("line limit"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub fn encode(request: &Request) -> Result<Vec<u8>> {
    if let Method::Judge { request } = &request.method {
        validate_request(request)?;
    }
    if let Method::Temporal { request } = &request.method {
        request.validate()?;
    }
    let limit = if matches!(request.method, Method::Judge { .. }) {
        MAX_JUDGMENT_LINE_BYTES
    } else {
        MAX_LINE_BYTES
    };
    encode_value(request, limit)
}

fn encode_value(request: &impl Serialize, limit: usize) -> Result<Vec<u8>> {
    let mut line = LimitedLine(Vec::new(), limit);
    serde_json::to_writer(&mut line, request).map_err(|_| invalid("worker request too large"))?;
    line.0.push(b'\n');
    Ok(line.0)
}

pub fn decode(bytes: &[u8]) -> Result<Response> {
    decode_bounded(bytes, MAX_LINE_BYTES, MAX_NODES)
}

pub(crate) fn decode_for(bytes: &[u8], method: &Method) -> Result<Response> {
    if matches!(method, Method::Rank { .. } | Method::Temporal { .. }) {
        decode(bytes)
    } else {
        decode_bounded(bytes, MAX_JUDGMENT_LINE_BYTES, MAX_JUDGMENT_NODES)
    }
}

fn decode_bounded(bytes: &[u8], byte_limit: usize, node_limit: usize) -> Result<Response> {
    if bytes.len() >= byte_limit {
        return Err(unavailable("worker output too large"));
    }
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let budget = Cell::new(node_limit);
    let value = StrictValue {
        depth: 1,
        budget: &budget,
        agreement_path: 0,
    }
    .deserialize(&mut deserializer)
    .map_err(|_| unavailable("invalid worker JSON"))?;
    deserializer
        .end()
        .map_err(|_| unavailable("invalid worker JSON"))?;
    if let Some(provider) = value.pointer("/result/provider")
        && provider.as_object().is_none_or(|object| object.len() != 3)
    {
        return Err(unavailable("invalid worker provider"));
    }
    let response: Response =
        serde_json::from_value(value).map_err(|_| unavailable("invalid worker envelope"))?;
    if matches!(&response.result, Some(WorkerResult::Judge(_)))
        && (bytes.len() >= MAX_JUDGMENT_LINE_BYTES
            || node_limit - budget.get() > MAX_JUDGMENT_NODES)
    {
        return Err(unavailable("Judgment output too large"));
    }
    if response.id.is_some_and(|id| id == 0 || id > MAX_ID)
        || response.result.is_some() == response.error.is_some()
    {
        return Err(unavailable("invalid worker envelope"));
    }
    Ok(response)
}

// serde_json::Value normally overwrites duplicate keys; reject them at every depth.
struct StrictValue<'a> {
    depth: usize,
    budget: &'a Cell<usize>,
    // Only /result/output/user_contributions may contain up to 200 voter entries.
    agreement_path: u8,
}
impl<'de> DeserializeSeed<'de> for StrictValue<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> std::result::Result<Value, D::Error> {
        let remaining = self
            .budget
            .get()
            .checked_sub(1)
            .ok_or_else(|| de::Error::custom("node limit"))?;
        self.budget.set(remaining);
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for StrictValue<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("bounded JSON without duplicate keys")
    }
    fn visit_bool<E: de::Error>(self, value: bool) -> std::result::Result<Value, E> {
        Ok(value.into())
    }
    fn visit_i64<E: de::Error>(self, value: i64) -> std::result::Result<Value, E> {
        Ok(value.into())
    }
    fn visit_u64<E: de::Error>(self, value: u64) -> std::result::Result<Value, E> {
        Ok(value.into())
    }
    fn visit_f64<E: de::Error>(self, value: f64) -> std::result::Result<Value, E> {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .ok_or_else(|| de::Error::custom("nonfinite number"))
    }
    fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Value, E> {
        Ok(value.into())
    }
    fn visit_string<E: de::Error>(self, value: String) -> std::result::Result<Value, E> {
        Ok(value.into())
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> std::result::Result<Value, A::Error> {
        if self.depth > MAX_DEPTH {
            return Err(de::Error::custom("depth limit"));
        }
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(StrictValue {
            depth: self.depth + 1,
            budget: self.budget,
            agreement_path: 4,
        })? {
            if values.len() >= MAX_ARRAY_ITEMS {
                return Err(de::Error::custom("array limit"));
            }
            values.push(value);
        }
        Ok(Value::Array(values))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Value, A::Error> {
        if self.depth > MAX_DEPTH {
            return Err(de::Error::custom("depth limit"));
        }
        let mut values = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let map_limit = if self.agreement_path == 3 { 200 } else { 64 };
            if values.len() >= map_limit || key.len() > 4096 {
                return Err(de::Error::custom("map limit"));
            }
            if values.contains_key(&key) {
                return Err(de::Error::custom("duplicate key"));
            }
            let value = map.next_value_seed(StrictValue {
                depth: self.depth + 1,
                budget: self.budget,
                agreement_path: match (self.agreement_path, key.as_str()) {
                    (0, "result") => 1,
                    (1, "output") => 2,
                    (2, "user_contributions") => 3,
                    _ => 4,
                },
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}
