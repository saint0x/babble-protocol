use babble_judgment::{
    DefinitionId, Judgment, JudgmentProvider, JudgmentRegistry, JudgmentRequest, ProviderVersion,
};
use babble_types::{Canonical, JudgmentId, Result, Timestamp};
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct LocalProvider {
    version: ProviderVersion,
}

impl Default for LocalProvider {
    fn default() -> Self {
        Self {
            version: ProviderVersion {
                provider: "babble-local".to_string(),
                model: "rules-v1".to_string(),
                version: "1".to_string(),
            },
        }
    }
}

impl JudgmentProvider for LocalProvider {
    fn supported_definitions(&self) -> Vec<DefinitionId> {
        JudgmentRegistry::babble_core()
            .definitions
            .into_iter()
            .map(|definition| definition.id)
            .filter(|id| *id != DefinitionId::source_agreement_v1())
            .collect()
    }

    fn version(&self) -> ProviderVersion {
        self.version.clone()
    }

    fn judge(&self, request: &JudgmentRequest) -> Result<Judgment> {
        if request.definition == DefinitionId::source_agreement_v1() {
            return Err(babble_types::Error::ProviderUnavailable(
                "Rust local provider does not support source agreement".into(),
            ));
        }
        JudgmentRegistry::babble_core().validate_request(request)?;
        let input_hash = request.state.canonical_hash()?;
        let output = evaluate(request);
        let confidence = output
            .get("confidence")
            .and_then(Value::as_f64)
            .unwrap_or(0.5);
        let commitment = (
            request.definition.clone(),
            self.version.clone(),
            input_hash.clone(),
            request.parameters.clone(),
            output.clone(),
        );
        let judgment = Judgment {
            id: JudgmentId::from_hash(&commitment.canonical_hash()?),
            definition: request.definition.clone(),
            provider: self.version.clone(),
            input_hash,
            output,
            confidence,
            created_at: Timestamp::now(),
        };
        JudgmentRegistry::babble_core().validate_output(&judgment.definition, &judgment.output)?;
        Ok(judgment)
    }
}

fn evaluate(request: &JudgmentRequest) -> Value {
    if request.definition == DefinitionId::spam_v1() {
        return spam(request);
    }
    if request.definition == DefinitionId::evidence_quality_v1() {
        return evidence_quality(request);
    }
    if request.definition == DefinitionId::relevance_v1() {
        return relevance(request);
    }
    if request.definition == DefinitionId::relationship_v1() {
        return relationship(request);
    }
    if request.definition == DefinitionId::content_analysis_v1() {
        return content_analysis(request);
    }
    if request.definition == DefinitionId::moderation_v1() {
        return moderation(request);
    }
    serde_json::json!({
        "kind": "unknown_definition",
        "score": 0.0,
        "confidence": 0.0,
        "reason": "local provider has no rule for this definition"
    })
}

fn spam(request: &JudgmentRequest) -> Value {
    let text = text(request).to_lowercase();
    let indicators = [
        "buy now",
        "limited time",
        "act now",
        "click here",
        "free money",
        "guaranteed",
        "!!!",
        "http://",
        "https://",
    ];
    let hits = indicators
        .iter()
        .filter(|indicator| text.contains(**indicator))
        .count() as f64;
    let score = (hits / 4.0).min(1.0);
    serde_json::json!({
        "kind": "probability",
        "label": "spam",
        "score": score,
        "confidence": if hits > 0.0 { 0.72 } else { 0.55 },
        "matched_indicators": hits as u64
    })
}

fn evidence_quality(request: &JudgmentRequest) -> Value {
    let text = text(request).to_lowercase();
    let markers = [
        "according to",
        "dataset",
        "doi",
        "study",
        "citation",
        "source",
        "reproduced",
        "methodology",
    ];
    let marker_hits = markers
        .iter()
        .filter(|marker| text.contains(**marker))
        .count() as f64;
    let length_bonus = (text.split_whitespace().count() as f64 / 120.0).min(0.35);
    let score = ((marker_hits / 5.0) + length_bonus).min(1.0);
    serde_json::json!({
        "kind": "bounded_score",
        "label": "evidence_quality",
        "score": score,
        "confidence": 0.65,
        "marker_hits": marker_hits as u64
    })
}

fn relevance(request: &JudgmentRequest) -> Value {
    let text = text(request).to_lowercase();
    let query = request
        .parameters
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    let query_terms: Vec<&str> = query.split_whitespace().collect();
    let hits = query_terms
        .iter()
        .filter(|term| !term.is_empty() && text.contains(**term))
        .count();
    let score = if query_terms.is_empty() {
        0.0
    } else {
        hits as f64 / query_terms.len() as f64
    };
    serde_json::json!({
        "kind": "bounded_score",
        "label": "relevance",
        "score": score,
        "confidence": if query_terms.is_empty() { 0.0 } else { 0.60 }
    })
}

fn relationship(request: &JudgmentRequest) -> Value {
    let relation = request
        .parameters
        .get("relation")
        .and_then(Value::as_str)
        .unwrap_or("related");
    let text = text(request).to_lowercase();
    let score = match relation {
        "supports" => contains_any(&text, &["supports", "evidence", "confirms", "because"]),
        "contradicts" => contains_any(&text, &["contradicts", "however", "but", "fails"]),
        _ => contains_any(&text, &["related", "references", "context"]),
    };
    serde_json::json!({
        "kind": "relationship",
        "relation": relation,
        "score": score,
        "confidence": 0.55
    })
}

fn content_analysis(request: &JudgmentRequest) -> Value {
    let original = text(request);
    let normalized = original.to_lowercase();
    let topics = extract_topics(&normalized);
    let evidence_markers = evidence_markers(&normalized);
    let key_terms = key_terms(&normalized);
    let sentiment = sentiment(&normalized);
    let summary = original
        .split_whitespace()
        .take(28)
        .collect::<Vec<_>>()
        .join(" ");
    serde_json::json!({
        "kind": "content_analysis",
        "topics": topics,
        "evidence_markers": evidence_markers,
        "key_terms": key_terms,
        "summary": if summary.is_empty() { request.state.subject.clone() } else { summary },
        "sentiment": sentiment,
        "confidence": 0.64
    })
}

fn moderation(request: &JudgmentRequest) -> Value {
    let normalized = text(request).to_lowercase();
    let spam = score_hits(
        &normalized,
        &[
            "buy now",
            "limited time",
            "act now",
            "click here",
            "free money",
            "guaranteed",
            "!!!",
            "http://",
            "https://",
        ],
        4.0,
    );
    let misinformation = score_hits(
        &normalized,
        &[
            "secret cure",
            "guaranteed truth",
            "they don't want you to know",
            "fake study",
        ],
        3.0,
    );
    let safety = 1.0
        - score_hits(
            &normalized,
            &[
                "malware",
                "exploit",
                "steal credentials",
                "phishing",
                "weapon",
            ],
            2.0,
        );
    let coordination = score_hits(
        &normalized,
        &["brigade", "mass report", "botnet", "spam wave"],
        2.0,
    );
    let quality = quality_score(&normalized);
    let mut flags = Vec::new();
    if spam >= 0.45 {
        flags.push("spam");
    }
    if misinformation >= 0.45 {
        flags.push("misinformation");
    }
    if safety <= 0.45 {
        flags.push("safety");
    }
    if coordination >= 0.45 {
        flags.push("coordination");
    }
    if quality <= 0.30 {
        flags.push("low_quality");
    }
    let action = if safety <= 0.25 || spam >= 0.88 {
        "remove"
    } else if misinformation >= 0.65 || coordination >= 0.65 || spam >= 0.62 {
        "flag"
    } else if spam >= 0.35 || quality <= 0.35 {
        "limit"
    } else {
        "allow"
    };
    serde_json::json!({
        "kind": "moderation",
        "action": action,
        "flags": flags,
        "spam": spam,
        "quality": quality,
        "safety": safety,
        "coordination": coordination,
        "misinformation": misinformation,
        "confidence": if flags.is_empty() { 0.58 } else { 0.72 }
    })
}

fn contains_any(text: &str, needles: &[&str]) -> f64 {
    if needles.iter().any(|needle| text.contains(needle)) {
        0.75
    } else {
        0.25
    }
}

fn extract_topics(text: &str) -> Vec<String> {
    let topic_rules = [
        (
            "protocol",
            ["protocol", "rpc", "schema", "canonical", "gossip"],
        ),
        (
            "evidence",
            ["evidence", "dataset", "study", "source", "methodology"],
        ),
        (
            "runtime",
            ["surface", "runtime", "capability", "sandbox", "wasm"],
        ),
        (
            "social-graph",
            ["graph", "relationship", "reply", "follow", "trust"],
        ),
        ("media", ["image", "video", "audio", "document", "blob"]),
        (
            "moderation",
            ["spam", "abuse", "policy", "safety", "misinformation"],
        ),
    ];
    let mut topics = topic_rules
        .iter()
        .filter(|(_, needles)| needles.iter().any(|needle| text.contains(needle)))
        .map(|(topic, _)| (*topic).to_string())
        .collect::<Vec<_>>();
    if topics.is_empty() {
        topics.push("general".to_string());
    }
    topics
}

fn evidence_markers(text: &str) -> Vec<String> {
    [
        "according to",
        "dataset",
        "doi",
        "study",
        "citation",
        "source",
        "reproduced",
        "methodology",
    ]
    .iter()
    .filter(|marker| text.contains(**marker))
    .map(|marker| (*marker).to_string())
    .collect()
}

fn key_terms(text: &str) -> Vec<String> {
    let stop = [
        "about", "after", "again", "also", "and", "are", "because", "but", "for", "from", "has",
        "have", "into", "not", "object", "that", "the", "this", "with", "you",
    ];
    let mut terms = text
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| term.len() >= 4 && !stop.contains(term))
        .map(str::to_string)
        .collect::<Vec<_>>();
    terms.sort();
    terms.dedup();
    terms.truncate(8);
    if terms.is_empty() {
        terms.push("general".to_string());
    }
    terms
}

fn sentiment(text: &str) -> f64 {
    let positive = score_hits(
        text,
        &[
            "good",
            "useful",
            "constructive",
            "support",
            "works",
            "reproduced",
        ],
        4.0,
    );
    let negative = score_hits(
        text,
        &["bad", "harm", "fails", "contradicts", "spam", "abuse"],
        4.0,
    );
    (0.5 + 0.35 * positive - 0.35 * negative).clamp(0.0, 1.0)
}

fn quality_score(text: &str) -> f64 {
    let words = text.split_whitespace().count() as f64;
    let length = (words / 80.0).min(0.45);
    let evidence = (evidence_markers(text).len() as f64 / 4.0).min(0.30);
    let punctuation_penalty = if text.matches("!!!").count() > 0 {
        0.18
    } else {
        0.0
    };
    (0.32 + length + evidence - punctuation_penalty).clamp(0.0, 1.0)
}

fn score_hits(text: &str, needles: &[&str], denominator: f64) -> f64 {
    let hits = needles
        .iter()
        .filter(|needle| text.contains(**needle))
        .count() as f64;
    (hits / denominator).min(1.0)
}

fn text(request: &JudgmentRequest) -> String {
    request
        .state
        .context
        .get("text")
        .and_then(Value::as_str)
        .unwrap_or(&request.state.subject)
        .to_string()
}
