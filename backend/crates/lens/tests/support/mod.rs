use babel_lens::*;
use babel_types::{Hash, ObjectId, Timestamp};
use time::OffsetDateTime;

pub fn candidate(index: usize) -> Candidate {
    let sources = [
        CandidateSource::Following,
        CandidateSource::SocialGraph,
        CandidateSource::SemanticNeighborhood,
        CandidateSource::Temporal,
        CandidateSource::Emerging,
        CandidateSource::Evidence,
        CandidateSource::Contradiction,
        CandidateSource::Exploration,
    ];
    let source = sources[index % sources.len()].clone();
    let x = (index % 11) as f64 / 10.0;
    Candidate {
        object_id: ObjectId::from_hash(&Hash::from_bytes(
            format!("ranking-fixture-{index}").as_bytes(),
        )),
        source: source.clone(),
        sources: vec![CandidateSourceContribution {
            source,
            weight: 1.0,
        }],
        created_at: Timestamp(
            OffsetDateTime::from_unix_timestamp(1_700_000_000 + index as i64).unwrap(),
        ),
        signals: Signals {
            social_distance: x,
            followed_author: index.is_multiple_of(2),
            relevance: 1.0 - x,
            novelty: x,
            evidence_quality: x,
            contradiction: 1.0 - x,
            temporal: x,
            exploration: 1.0 - x,
            evidence: EvidenceSignals {
                human_support: index as f64,
                judgment_support: 0.75,
                human_contradiction: x * 2.0,
                judgment_contradiction: 1.0 - x,
            },
            reputation: ReputationSignals {
                epistemic_accuracy: x,
                evidence_quality: 1.0 - x,
                social_constructiveness: x,
                creative_contribution: 1.0 - x,
                moderation: 0.25,
                domain_expertise: 0.75,
            },
        },
    }
}

pub fn request() -> RankingRequest {
    RankingRequest {
        candidates: (0..8).map(candidate).collect(),
        lens: LensStack::new(
            "ranking-fixture",
            vec![LensWeight {
                lens: BuiltInLens::Following,
                weight: 1.0,
            }],
        ),
        diversity: DiversityPolicy::default(),
        limit: 5,
    }
}

pub fn fixtures() -> Vec<(String, RankingRequest)> {
    let mut cases = Vec::new();
    for lens in BuiltInLens::all() {
        let mut r = request();
        r.lens.weights[0].lens = lens.clone();
        cases.push((lens.id().to_string(), r));
    }
    let mut blend = request();
    blend.lens.weights = BuiltInLens::all()
        .into_iter()
        .enumerate()
        .map(|(i, lens)| LensWeight {
            lens,
            weight: (i + 1) as f64,
        })
        .collect();
    cases.push(("all-lens-blend".into(), blend.clone()));
    for weight in &mut blend.lens.weights {
        weight.weight = f64::MAX;
    }
    cases.push(("overflow-safe-large-weights".into(), blend));
    let mut ties = request();
    let baseline = ties.candidates[0].clone();
    for c in &mut ties.candidates {
        c.signals = baseline.signals.clone();
        c.created_at = baseline.created_at;
    }
    ties.candidates[1].created_at = Timestamp(baseline.created_at.0 + time::Duration::seconds(1));
    ties.diversity = DiversityPolicy {
        max_source_share: 1.0,
        source_floors: vec![],
    };
    cases.push(("timestamp-and-id-ties".into(), ties));
    let mut overlap = request();
    overlap.lens.weights[0].lens = BuiltInLens::Weird;
    overlap.candidates[0].sources.extend([
        CandidateSourceContribution {
            source: CandidateSource::Emerging,
            weight: 0.0,
        },
        CandidateSourceContribution {
            source: CandidateSource::Contradiction,
            weight: 0.3,
        },
    ]);
    overlap.candidates[0].signals.novelty = 1.0;
    overlap.candidates[0].signals.exploration = 1.0;
    overlap.candidates[0].signals.relevance = 0.0;
    overlap.candidates[0]
        .signals
        .reputation
        .creative_contribution = 1.0;
    overlap.candidates[0].signals.contradiction = 1.0;
    cases.push(("source-overlap-weird-above-one".into(), overlap));
    let mut evidence = request();
    evidence.lens.weights[0].lens = BuiltInLens::Contradictions;
    evidence.candidates[0].signals.evidence = EvidenceSignals {
        human_support: f64::MAX,
        judgment_support: f64::MAX,
        human_contradiction: f64::MAX,
        judgment_contradiction: f64::MAX,
    };
    cases.push(("saturating-evidence-counts".into(), evidence));
    let mut floors = request();
    floors.diversity = DiversityPolicy {
        max_source_share: 0.0,
        source_floors: vec![
            SourceFloor {
                source: CandidateSource::Exploration,
                minimum: 200,
            },
            SourceFloor {
                source: CandidateSource::Evidence,
                minimum: 0,
            },
        ],
    };
    floors.limit = 8;
    cases.push(("soft-floors-zero-share-zero-minimum".into(), floors));
    let mut empty = request();
    empty.candidates.clear();
    cases.push(("empty-candidates".into(), empty));
    let mut fallback = request();
    fallback.lens.weights.clear();
    cases.push(("empty-stack-following-fallback".into(), fallback.clone()));
    fallback.lens.weights = BuiltInLens::all()
        .into_iter()
        .map(|lens| LensWeight { lens, weight: 0.0 })
        .collect();
    cases.push(("zero-stack-following-fallback".into(), fallback));
    let mut subnormal = request();
    subnormal.lens.weights[0].weight = f64::from_bits(1);
    cases.push(("subnormal-weight".into(), subnormal));
    let mut maximum = request();
    maximum.candidates = (0..200).map(candidate).collect();
    maximum.limit = 200;
    cases.push(("maximum-request".into(), maximum));
    cases
}
