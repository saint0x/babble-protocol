//! Public source comparison. Signed relationships select evidence; they do not
//! certify source independence, authority, or the truth of a claim.
use crate::LocalNode;
use babble_graph::{EdgeOrigin, Relation};
use babble_judgment::{DefinitionId, JudgmentProvider, JudgmentRequest, JudgmentState};
use babble_object::Object;
use babble_types::{Error, ObjectId, Result, Timestamp};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const MAX_SOURCES: usize = 200;
const MAX_SOURCE_BYTES: usize = 64 * 1024;
const MAX_TOTAL_BYTES: usize = 512 * 1024;
const MAX_CLAIM_BYTES: usize = 128 * 1024;
const REUSE_SECONDS: f64 = 300.0;
pub(crate) const EVALUATION_BUDGET: Duration = Duration::from_secs(15);

#[cfg(test)]
#[path = "agreement_tests.rs"]
mod tests;

impl<P: JudgmentProvider> LocalNode<P> {
    #[cfg(test)]
    pub(crate) fn source_agreement_state_at(
        &self,
        object: &Object,
        reference: Timestamp,
    ) -> Result<JudgmentState> {
        self.source_agreement_state_before(object, reference, Instant::now() + EVALUATION_BUDGET)
    }

    pub(crate) fn source_agreement_state_before(
        &self,
        object: &Object,
        reference: Timestamp,
        deadline: Instant,
    ) -> Result<JudgmentState> {
        let check = || crate::judgments::check_deadline(Some(deadline));
        check()?;
        if object.created_at > reference {
            return Err(Error::Conflict("cannot evaluate a future claim".into()));
        }
        let claim_text = PublicText::new(object, MAX_CLAIM_BYTES)?;
        let mut selected: BTreeMap<ObjectId, bool> = BTreeMap::new();
        for edge in self.state.graph().incoming_iter(&object.id) {
            check()?;
            if edge.source == object.id
                || edge.created_at > reference
                || edge.created_at < object.created_at
                || !matches!(
                    edge.origin,
                    EdgeOrigin::HumanAssertion | EdgeOrigin::ApplicationAssertion
                )
                || !matches!(
                    edge.relation,
                    Relation::Supports
                        | Relation::Contradicts
                        | Relation::EvidenceFor
                        | Relation::EvidenceAgainst
                        | Relation::References
                        | Relation::Cites
                )
            {
                continue;
            }
            let Some(author) = &edge.author else { continue };
            if edge.signature.is_none() {
                continue;
            }
            edge.verify(&self.state.signing_identity_at(author, edge.created_at)?)?;
            let source = self.require_object(&edge.source)?;
            if source.created_at > reference || source.created_at > edge.created_at {
                continue;
            }
            PublicText::new(source, MAX_SOURCE_BYTES)?;
            check()?;
            source.verify(
                &self
                    .state
                    .signing_identity_at(&source.author, source.created_at)?,
            )?;
            check()?;
            let context = matches!(edge.relation, Relation::References | Relation::Cites);
            selected
                .entry(source.id.clone())
                .and_modify(|value| *value &= context)
                .or_insert(context);
        }

        // Collapse exact copied text before measuring agreement. Distinct authors
        // or Object IDs alone cannot demonstrate independent evidence.
        let mut seen_text: BTreeMap<[u8; 32], usize> = BTreeMap::new();
        let mut candidates: Vec<(&Object, PublicText<'_>, bool)> = Vec::new();
        let mut total_bytes = 0;
        for (id, context) in selected {
            check()?;
            let source = self.require_object(&id)?;
            let text = PublicText::new(source, MAX_SOURCE_BYTES)?;
            if text
                .parts
                .iter()
                .flatten()
                .all(|part| part.trim().is_empty())
            {
                continue;
            }
            let text_hash = text.hash();
            if let Some(&index) = seen_text.get(&text_hash) {
                // An evidence relationship on any copy takes precedence over context.
                candidates[index].2 &= context;
                continue;
            }
            total_bytes += text.len;
            if total_bytes > MAX_TOTAL_BYTES || candidates.len() >= MAX_SOURCES {
                return Err(evaluation_limit());
            }
            seen_text.insert(text_hash, candidates.len());
            candidates.push((source, text, context));
        }

        check()?;
        let provider = self.judgment_provider.version();
        let previous = self.store.latest_object_judgment_input(
            &object.id,
            &DefinitionId::source_agreement_v1(),
            &provider,
            reference,
        )?;
        check()?;
        let now = seconds(reference);
        let mut previous_score = None;
        if let Some((input, judgment)) = previous {
            let old = input
                .request
                .state
                .context
                .get("source_agreement")
                .ok_or_else(|| {
                    Error::Conflict("stored source agreement input is missing".into())
                })?;
            let old_time = old
                .get("reference_time")
                .and_then(Value::as_f64)
                .ok_or_else(|| Error::Conflict("stored source agreement time is invalid".into()))?;
            // Signed object IDs bind source content and timestamps. Compare roles
            // and public text too, before starting any supporting workers.
            let unchanged =
                old.get("sources")
                    .and_then(Value::as_array)
                    .is_some_and(|sources| {
                        sources.len() == candidates.len()
                            && sources.iter().zip(&candidates).all(
                                |(old, (source, text, context))| {
                                    old.get("source_id").and_then(Value::as_str)
                                        == Some(source.id.as_str())
                                        && old.get("kind").and_then(Value::as_str)
                                            == Some("social_media")
                                        && old.get("timestamp").and_then(Value::as_f64)
                                            == Some(seconds(source.created_at))
                                        && old.get("user_id").is_some_and(Value::is_null)
                                        && old.get("vote").is_some_and(Value::is_null)
                                        && old.get("is_context").and_then(Value::as_bool)
                                            == Some(*context)
                                        && old
                                            .get("text")
                                            .and_then(Value::as_str)
                                            .is_some_and(|old| text.matches(old))
                                },
                            )
                    })
                    && input
                        .request
                        .state
                        .context
                        .get("text")
                        .and_then(Value::as_str)
                        .is_some_and(|old| claim_text.matches(old));
            check()?;
            if unchanged && now >= old_time && now - old_time < REUSE_SECONDS {
                return Ok(input.request.state);
            }
            previous_score = Some(unit(&judgment.output, "consensus_score")?);
        }

        let mut sources = Vec::new();
        for (source, text, context) in candidates {
            check()?;
            let text = text.to_text();
            // Supporting assessments use the same allowlisted text as agreement,
            // never the imported Object's arbitrary payload or metadata.
            let assess = |definition| {
                check()?;
                self.prepare_judgment_request_before(
                    source,
                    JudgmentRequest {
                        definition,
                        state: JudgmentState {
                            subject: source.id.to_string(),
                            context: BTreeMap::from([("text".into(), Value::String(text.clone()))]),
                        },
                        parameters: BTreeMap::new(),
                    },
                    Some(deadline),
                )
            };
            let quality = assess(DefinitionId::moderation_v1())?;
            let evidence = assess(DefinitionId::evidence_quality_v1())?;
            sources.push(json!({
                "source_id": source.id,
                "kind": "social_media",
                "text": text,
                "timestamp": seconds(source.created_at),
                "quality_score": unit(&quality.orchestration.judgment.output, "quality")?,
                "evidence_score": unit(&evidence.orchestration.judgment.output, "score")?,
                "user_id": null,
                "vote": null,
                "is_context": context,
            }));
        }

        check()?;
        let state = JudgmentState {
            subject: object.id.to_string(),
            context: BTreeMap::from([
                ("text".into(), Value::String(claim_text.to_text())),
                (
                    "source_agreement".into(),
                    json!({
                        "reference_time": now,
                        "previous_score": previous_score,
                        "sources": sources,
                    }),
                ),
            ]),
        };
        check()?;
        Ok(state)
    }
}

fn seconds(time: Timestamp) -> f64 {
    time.0.unix_timestamp() as f64 + f64::from(time.0.nanosecond()) / 1_000_000_000.0
}

fn unit(value: &Value, field: &str) -> Result<f64> {
    value
        .get(field)
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
        .ok_or_else(|| {
            Error::ProviderUnavailable(format!(
                "source agreement requires a valid {field} assessment"
            ))
        })
}

fn evaluation_limit() -> Error {
    Error::Conflict("source agreement text exceeds evaluation limits".into())
}

// Borrow fields until all unique evidence passes the byte/count limits. Hashing
// includes join separators without allocating a joined copy of rejected text.
struct PublicText<'a> {
    parts: [Option<&'a str>; 4],
    len: usize,
}

impl<'a> PublicText<'a> {
    fn new(object: &'a Object, limit: usize) -> Result<Self> {
        let parts = ["text", "title", "description", "summary"]
            .map(|key| object.payload.get(key).and_then(Value::as_str));
        let mut len: usize = 0;
        for (index, part) in parts.iter().flatten().enumerate() {
            len = len
                .checked_add(usize::from(index != 0))
                .and_then(|len| len.checked_add(part.len()))
                .filter(|&len| len <= limit)
                .ok_or_else(evaluation_limit)?;
        }
        Ok(Self { parts, len })
    }

    fn hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        for (index, part) in self.parts.iter().flatten().enumerate() {
            if index != 0 {
                hasher.update(b"\n");
            }
            hasher.update(part.as_bytes());
        }
        *hasher.finalize().as_bytes()
    }

    fn matches(&self, mut text: &str) -> bool {
        if text.len() != self.len {
            return false;
        }
        for (index, part) in self.parts.iter().flatten().enumerate() {
            if index != 0 {
                let Some(rest) = text.strip_prefix('\n') else {
                    return false;
                };
                text = rest;
            }
            let Some(rest) = text.strip_prefix(part) else {
                return false;
            };
            text = rest;
        }
        text.is_empty()
    }

    fn to_text(&self) -> String {
        let mut text = String::with_capacity(self.len);
        for (index, part) in self.parts.iter().flatten().enumerate() {
            if index != 0 {
                text.push('\n');
            }
            text.push_str(part);
        }
        text
    }
}
