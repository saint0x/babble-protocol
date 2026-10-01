//! Public signal preparation and versioned temporal evaluation.
use super::*;
use babble_discovery::{
    ObjectSignals, TemporalClass, TemporalEngagement, TemporalItem, TemporalRequest, TemporalScore,
};

struct PreparedSignals {
    item: TemporalItem,
    followed_author: bool,
    relevance: f64,
    novelty: f64,
    evidence: EvidenceSignals,
    reputation: ReputationSignals,
    relation_diversity: f64,
}

impl PreparedSignals {
    fn finish(self, score: &TemporalScore, total_objects: usize) -> ObjectSignals {
        ObjectSignals {
            object_id: self.item.object_id,
            created_at: self.item.published_at,
            followed_author: self.followed_author,
            relevance: self.relevance,
            novelty: self.novelty,
            evidence_quality: self.item.quality_score,
            contradiction: self.evidence.contradiction_score(),
            evidence: self.evidence,
            exploration: exploration_signal(
                self.relevance,
                self.novelty,
                score.survival_score,
                self.relation_diversity,
                self.reputation.creative_score(),
                total_objects,
            ),
            reputation: self.reputation,
            temporal: score.survival_score,
        }
    }
}

impl<P: JudgmentProvider> LocalNode<P> {
    pub(super) fn discovery_signals(
        &mut self,
        objects: &[Object],
        query: &DiscoveryQuery,
        search_relevance: &BTreeMap<ObjectId, f64>,
        reference_time: Timestamp,
        population: &retrieval::Population,
    ) -> Result<(
        BTreeMap<ObjectId, ObjectSignals>,
        BTreeMap<ObjectId, TemporalScore>,
    )> {
        let mut summaries = BTreeMap::new();
        let mut scores = BTreeMap::new();
        // Retrieval has already admitted at most 200 public Objects.
        for chunk in objects.chunks(200) {
            let mut prepared = Vec::with_capacity(chunk.len());
            for object in chunk {
                let incoming = self.incoming_edges(&object.id);
                let outgoing = self.outgoing_edges(&object.id);
                let evidence = Self::evidence_signals(&incoming);
                let references = incoming
                    .iter()
                    .chain(&outgoing)
                    .filter(|edge| {
                        matches!(
                            edge.relation,
                            Relation::References
                                | Relation::Cites
                                | Relation::Quotes
                                | Relation::Extends
                                | Relation::DerivesFrom
                                | Relation::Supersedes
                                | Relation::Forks
                                | Relation::Remixes
                        )
                    })
                    .count() as f64;
                let relevance = search_relevance
                    .get(&object.id)
                    .copied()
                    .unwrap_or_default()
                    .max(if query.anchors.contains(&object.id) {
                        1.0
                    } else {
                        0.0
                    })
                    .max((references / 4.0).min(1.0));
                let quality =
                    self.discovery_judgment_score(&object.id, DefinitionId::evidence_quality_v1())?;
                let spam = self.discovery_judgment_score(&object.id, DefinitionId::spam_v1())?;
                let reputation =
                    Self::reputation_signals(&incoming, &outgoing, &evidence, quality, 1.0 - spam);
                let relation_diversity = relation_diversity(&incoming, &outgoing);
                let novelty = novelty_signal(
                    population.author_count(&object.author),
                    population.max_author_objects,
                    relation_diversity,
                    evidence.support_score(),
                    evidence.contradiction_score(),
                );
                let (content_class, tags) = classify_time(object);
                prepared.push(PreparedSignals {
                    item: TemporalItem {
                        object_id: object.id.clone(),
                        published_at: object.created_at,
                        content_class,
                        tags,
                        quality_score: quality.max(evidence.support_score()),
                        engagement: public_activity(object, &incoming, reference_time),
                    },
                    followed_author: query.followed_objects.contains(&object.id),
                    relevance,
                    novelty,
                    evidence,
                    reputation,
                    relation_diversity,
                });
            }
            let request = TemporalRequest {
                reference_time,
                items: prepared.iter().map(|p| p.item.clone()).collect(),
            };
            request.validate()?;
            let result = self.temporal_provider.score(&request)?;
            result
                .validate_for(&request, &self.temporal_provider.version())
                .map_err(|_| {
                    babble_types::Error::ProviderUnavailable(
                        "temporal provider returned invalid output".into(),
                    )
                })?;
            for (input, score) in prepared.into_iter().zip(result.scores) {
                summaries.insert(
                    score.object_id.clone(),
                    input.finish(&score, population.total),
                );
                scores.insert(score.object_id.clone(), score);
            }
        }
        Ok((summaries, scores))
    }

    fn discovery_judgment_score(
        &mut self,
        object: &ObjectId,
        definition: DefinitionId,
    ) -> Result<f64> {
        self.judge_object(object, definition, BTreeMap::new())?
            .output
            .get("score")
            .and_then(Value::as_f64)
            .filter(|score| score.is_finite() && (0.0..=1.0).contains(score))
            .ok_or_else(|| {
                babble_types::Error::ProviderUnavailable(
                    "discovery Judgment has no valid score".into(),
                )
            })
    }
}

/// Inbound, signed human/application content relationships are public activity,
/// not views, unique readers, or a calibrated measure of engagement quality.
fn public_activity(
    object: &Object,
    incoming: &[Edge],
    reference_time: Timestamp,
) -> TemporalEngagement {
    let mut seen = BTreeSet::new();
    let mut total = 0;
    let mut recent = 0;
    for edge in incoming {
        if edge.source == object.id
            || edge.target != object.id
            || edge.signature.is_none()
            || edge.author.is_none()
            || !matches!(
                edge.origin,
                EdgeOrigin::HumanAssertion | EdgeOrigin::ApplicationAssertion
            )
            || !matches!(
                edge.relation,
                Relation::ReplyTo
                    | Relation::References
                    | Relation::Quotes
                    | Relation::Cites
                    | Relation::Supports
                    | Relation::Contradicts
                    | Relation::EvidenceFor
                    | Relation::EvidenceAgainst
                    | Relation::Extends
                    | Relation::DerivesFrom
                    | Relation::Supersedes
                    | Relation::Forks
                    | Relation::Remixes
            )
            || edge.created_at < object.created_at
            || edge.created_at > reference_time
            || !seen.insert(&edge.id)
        {
            continue;
        }
        total += 1;
        if (reference_time.0 - edge.created_at.0).whole_nanoseconds()
            <= 7 * 24 * 3600 * 1_000_000_000_i128
        {
            recent += 1;
        }
    }
    TemporalEngagement {
        total_views: 0,
        recent_views: 0,
        total_interactions: total,
        recent_interactions: recent,
    }
}

fn classify_time(object: &Object) -> (TemporalClass, Vec<String>) {
    let text = ["text", "title", "description", "name", "summary"]
        .iter()
        .filter_map(|key| object.payload.get(key).and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    let words: Vec<_> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let has = |terms: &[&str]| words.iter().any(|word| terms.contains(word));
    let phrase = |first: &str, second: &str| words.windows(2).any(|pair| pair == [first, second]);
    let class = if has(&["breaking", "today", "urgent", "now", "live"]) {
        TemporalClass::News
    } else if has(&["guide", "tutorial", "walkthrough"]) || phrase("how", "to") {
        TemporalClass::Tutorial
    } else if has(&["reference", "spec", "schema", "documentation", "api"]) {
        TemporalClass::Reference
    } else if has(&["analysis", "evidence", "methodology", "dataset"]) {
        TemporalClass::Analysis
    } else {
        TemporalClass::Discussion
    };
    let mut tags = Vec::new();
    for term in ["breaking", "evergreen", "reference"] {
        if has(&[term]) {
            tags.push(term.into());
        }
    }
    if phrase("time", "sensitive") {
        tags.push("time-sensitive".into());
    }
    (class, tags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use babble_identity::IdentityKind;
    use time::{Duration, OffsetDateTime};

    #[test]
    fn temporal_classification_uses_words_not_incidental_substrings() {
        let key = Keypair::generate();
        let author = Identity::create(IdentityKind::Person, "reader", &key).unwrap();
        let object = Object::text(&author, "Deliver this speculative overview").unwrap();
        assert_eq!(classify_time(&object).0, TemporalClass::Discussion);
        let object = Object::text(&author, "Evergreen API reference").unwrap();
        assert_eq!(
            classify_time(&object),
            (
                TemporalClass::Reference,
                vec!["evergreen".into(), "reference".into()]
            )
        );
    }

    #[test]
    fn public_activity_counts_deduplicated_inbound_signed_content_edges_only() {
        let key = Keypair::generate();
        let author = Identity::create(IdentityKind::Person, "reader", &key).unwrap();
        let now = Timestamp(OffsetDateTime::from_unix_timestamp(1_700_000_000).unwrap());
        let mut object = Object::text(&author, "Discussion").unwrap();
        object.created_at = Timestamp(now.0 - Duration::days(30));
        let source = Object::text(&author, "Response").unwrap();
        let edge = |relation, origin, age| {
            let mut e = Edge::new(
                source.id.clone(),
                object.id.clone(),
                relation,
                origin,
                Some(author.id.clone()),
            )
            .unwrap();
            e.created_at = Timestamp(now.0 - Duration::days(age));
            e.with_metadata(BTreeMap::new())
                .unwrap()
                .sign(&author, &key)
                .unwrap()
        };
        let recent = edge(Relation::ReplyTo, EdgeOrigin::HumanAssertion, 1);
        let mut unsigned = recent.clone();
        unsigned.signature = None;
        let mut self_edge = recent.clone();
        self_edge.source = object.id.clone();
        let edges = vec![
            recent.clone(),
            recent,
            unsigned,
            self_edge,
            edge(Relation::Cites, EdgeOrigin::HumanAssertion, 10),
            edge(Relation::ReplyTo, EdgeOrigin::ApplicationAssertion, 7),
            edge(Relation::Follows, EdgeOrigin::HumanAssertion, 1),
            edge(Relation::Supports, EdgeOrigin::JudgmentDerived, 1),
            edge(Relation::Quotes, EdgeOrigin::HumanAssertion, -1),
            edge(Relation::Quotes, EdgeOrigin::HumanAssertion, 31),
        ];
        let result = public_activity(&object, &edges, now);
        assert_eq!(result.total_views, 0);
        assert_eq!(result.recent_views, 0);
        assert_eq!(result.total_interactions, 3);
        assert_eq!(result.recent_interactions, 2);
    }
}
