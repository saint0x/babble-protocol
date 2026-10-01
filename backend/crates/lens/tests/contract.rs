mod support;

use babble_lens::*;
use babble_types::ObjectId;
use support::{candidate, fixtures, request};

type Mutation<T> = Box<dyn Fn(&mut T)>;

#[test]
fn native_contract_preserves_existing_behavior_for_all_goldens() {
    let provider: &dyn RankingProvider = &NativeRanker;
    for (name, request) in fixtures() {
        let result = provider
            .rank(&request)
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        let (ranked, trace) = request.lens.rank_with_trace(&request.candidates);
        let (ranked, diversity_trace) =
            diversify_ranked(&ranked, &request.diversity, request.limit);
        assert_eq!(result.ranked, ranked, "{name}");
        assert_eq!(result.trace, trace, "{name}");
        assert_eq!(result.diversity_trace, diversity_trace, "{name}");
        let json = serde_json::to_vec(&result).unwrap();
        let decoded: RankingResult = serde_json::from_slice(&json).unwrap();
        decoded.validate_for(&request, &provider.version()).unwrap();
    }
}

#[test]
fn request_bounds_fail_closed() {
    let mutations: Vec<Mutation<RankingRequest>> = vec![
        Box::new(|r| r.limit = 0),
        Box::new(|r| r.limit = 201),
        Box::new(|r| r.candidates = (0..201).map(candidate).collect()),
        Box::new(|r| r.candidates.push(r.candidates[0].clone())),
        Box::new(|r| {
            r.candidates[0].object_id = ObjectId::new_unchecked(format!("obj_{}", "z".repeat(64)))
        }),
        Box::new(|r| {
            r.candidates[0].object_id = ObjectId::new_unchecked(format!("id_{}", "a".repeat(64)))
        }),
        Box::new(|r| r.lens.id.clear()),
        Box::new(|r| r.lens.id = "x".repeat(129)),
        Box::new(|r| r.lens.id = "space invalid".into()),
        Box::new(|r| r.lens.weights.push(r.lens.weights[0].clone())),
        Box::new(|r| r.lens.weights[0].weight = -1.0),
        Box::new(|r| r.lens.weights[0].weight = f64::INFINITY),
        Box::new(|r| r.candidates[0].sources.clear()),
        Box::new(|r| r.candidates[0].sources[0].source = CandidateSource::Evidence),
        Box::new(|r| {
            let duplicate = r.candidates[0].sources[0].clone();
            r.candidates[0].sources.push(duplicate);
        }),
        Box::new(|r| r.candidates[0].sources[0].weight = 1.1),
        Box::new(|r| r.candidates[0].signals.relevance = f64::NAN),
        Box::new(|r| r.candidates[0].signals.novelty = -0.1),
        Box::new(|r| r.candidates[0].signals.reputation.moderation = 1.1),
        Box::new(|r| r.candidates[0].signals.evidence.human_support = -0.1),
        Box::new(|r| r.candidates[0].signals.evidence.judgment_contradiction = f64::INFINITY),
        Box::new(|r| r.diversity.max_source_share = f64::NAN),
        Box::new(|r| r.diversity.max_source_share = 1.1),
        Box::new(|r| r.diversity.source_floors[0].minimum = 201),
        Box::new(|r| {
            r.diversity
                .source_floors
                .push(r.diversity.source_floors[0].clone())
        }),
    ];
    for (index, mutate) in mutations.into_iter().enumerate() {
        let mut r = request();
        mutate(&mut r);
        assert!(r.validate().is_err(), "mutation {index}");
        assert!(NativeRanker.rank(&r).is_err(), "mutation {index}");
    }
}

#[test]
fn malicious_results_are_rejected() {
    let r = request();
    let good = NativeRanker.rank(&r).unwrap();
    let mutations: Vec<Mutation<RankingResult>> = vec![
        Box::new(|r| r.provider.provider = "impostor".into()),
        Box::new(|r| r.ranked.pop().map(drop).unwrap()),
        Box::new(|r| r.ranked[0].candidate.signals.temporal = 0.345),
        Box::new(|r| r.ranked[0].score = f64::NAN),
        Box::new(|r| r.ranked[0].reasons[0].contribution = f64::INFINITY),
        Box::new(|r| r.ranked[0].reasons[0].signal = "untrusted".into()),
        Box::new(|r| r.ranked[1] = r.ranked[0].clone()),
        Box::new(|r| r.ranked.swap(0, 1)),
        Box::new(|r| r.trace.stack_id = "other".into()),
        Box::new(|r| r.trace.candidates[0].rank = 0),
        Box::new(|r| r.trace.candidates[0].score = 2.0),
        Box::new(|r| r.trace.candidates[0].sources.clear()),
        Box::new(|r| r.trace.candidates[0].lens_contributions[0].weight = 0.5),
        Box::new(|r| r.trace.candidates[0].lens_contributions[0].reasons.clear()),
        Box::new(|r| r.trace.candidates[0].lens_contributions[0].reasons[0].contribution = -0.1),
        Box::new(|r| r.trace.candidates[1] = r.trace.candidates[0].clone()),
        Box::new(|r| {
            r.trace.candidates.swap(0, 1);
            r.trace.candidates[0].rank = 1;
            r.trace.candidates[1].rank = 2;
        }),
        Box::new(|r| r.diversity_trace.candidates[0].lens_score = 0.234),
        Box::new(|r| r.diversity_trace.candidates[0].diversified_score = 1.1),
        Box::new(|r| {
            r.diversity_trace.candidates[0]
                .reasons
                .push(DiversityReason {
                    signal: "source_floor".into(),
                    contribution: 0.18,
                })
        }),
        Box::new(|r| r.diversity_trace.policy.max_source_share = 0.1),
        Box::new(|r| r.diversity_trace.filtered[0] = r.ranked[0].candidate.object_id.clone()),
        Box::new(|r| r.diversity_trace.filtered.reverse()),
    ];
    for (index, mutate) in mutations.into_iter().enumerate() {
        let mut bad = good.clone();
        mutate(&mut bad);
        assert!(
            bad.validate_for(&r, &NativeRanker.version()).is_err(),
            "mutation {index}"
        );
    }
}

#[test]
fn normalization_handles_overflow_and_preserves_following_fallback() {
    let mut r = request();
    let following = NativeRanker.rank(&r).unwrap();
    r.lens.weights.clear();
    assert_eq!(NativeRanker.rank(&r).unwrap(), following);
    r.lens.weights = vec![LensWeight {
        lens: BuiltInLens::Research,
        weight: 0.0,
    }];
    assert_eq!(NativeRanker.rank(&r).unwrap(), following);
    r.lens.weights = vec![
        LensWeight {
            lens: BuiltInLens::Research,
            weight: f64::MAX,
        },
        LensWeight {
            lens: BuiltInLens::Weird,
            weight: f64::MAX,
        },
    ];
    let large = NativeRanker.rank(&r).unwrap();
    for w in &mut r.lens.weights {
        w.weight = 1.0;
    }
    assert_eq!(NativeRanker.rank(&r).unwrap(), large);
}

#[test]
fn nested_unknown_fields_and_invalid_timestamps_are_rejected() {
    let base = serde_json::to_value(request()).unwrap();
    for pointer in [
        "",
        "/lens",
        "/lens/weights/0",
        "/diversity",
        "/diversity/source_floors/0",
        "/candidates/0",
        "/candidates/0/sources/0",
        "/candidates/0/signals",
        "/candidates/0/signals/evidence",
        "/candidates/0/signals/reputation",
    ] {
        let mut value = base.clone();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), true.into());
        assert!(
            serde_json::from_value::<RankingRequest>(value).is_err(),
            "{pointer}"
        );
    }
    let mut value = base;
    value["candidates"][0]["created_at"] = "not-a-time".into();
    assert!(serde_json::from_value::<RankingRequest>(value).is_err());
    let base = serde_json::to_value(NativeRanker.rank(&request()).unwrap()).unwrap();
    for pointer in [
        "",
        "/provider",
        "/trace",
        "/trace/candidates/0",
        "/trace/candidates/0/lens_contributions/0",
        "/trace/candidates/0/lens_contributions/0/reasons/0",
        "/ranked/0",
        "/ranked/0/reasons/0",
        "/diversity_trace",
        "/diversity_trace/candidates/0",
    ] {
        let mut value = base.clone();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), true.into());
        assert!(
            serde_json::from_value::<RankingResult>(value).is_err(),
            "{pointer}"
        );
    }
}

#[test]
fn validation_does_not_recompute_native_scores() {
    let mut r = request();
    r.candidates.truncate(1);
    let mut result = NativeRanker.rank(&r).unwrap();
    let python = RankingProviderVersion {
        provider: "babble-python".into(),
        model: "lenses-v1".into(),
        version: "1".into(),
    };
    result.provider = python.clone();
    result.trace.candidates[0].score = 0.0;
    let part = &mut result.trace.candidates[0].lens_contributions[0];
    part.score = 0.0;
    for reason in &mut part.reasons {
        reason.contribution = 0.0;
    }
    result.ranked[0].score = 0.0;
    for reason in &mut result.ranked[0].reasons {
        reason.contribution = 0.0;
    }
    result.diversity_trace.candidates[0].lens_score = 0.0;
    result.diversity_trace.candidates[0].diversified_score = 0.0;
    result.validate_for(&r, &python).unwrap();
}
