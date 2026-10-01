from dataclasses import replace
from itertools import permutations
from math import fsum, inf, nan, nextafter
from random import Random
from typing import cast

import pytest

from babble_algorithms.consensus import (
    MAX_ID_BYTES,
    MAX_SOURCE_TEXT_BYTES,
    MAX_SOURCES,
    MAX_TIMESTAMP,
    MAX_TOTAL_TEXT_BYTES,
    MIN_TIMESTAMP,
    ConsensusAnalyzer,
    ConsensusResult,
    ConsensusSource,
    ConsensusState,
    SourceKind,
)


def source(identifier: str = "source", /, **changes: object) -> ConsensusSource:
    return replace(
        ConsensusSource(identifier, "official_docs", "The bridge is open.", 100.0),
        **changes,
    )


def evaluate(*sources: ConsensusSource, previous_score: float | None = None) -> ConsensusResult:
    return ConsensusAnalyzer().evaluate(
        "claim", sources, reference_time=100.0, previous_score=previous_score
    )


@pytest.mark.parametrize("field", ["source_id", "user_id", "content_id"])
@pytest.mark.parametrize("value", ["", " \t\n", 0, False, b"id", [], "x" * 513, "\ud800"])
def test_ids_reject_invalid_domain_values(field: str, value: object) -> None:
    with pytest.raises(ValueError, match=field):
        if field == "content_id":
            _ = ConsensusAnalyzer().evaluate(cast(str, value), (), reference_time=100.0)
        else:
            _ = source(**{field: value})


@pytest.mark.parametrize("value", ["plain-library-id", "x" * MAX_ID_BYTES, "\u00e9" * 256])
def test_ids_accept_opaque_strings_through_utf8_byte_limit(value: str) -> None:
    result = ConsensusAnalyzer().evaluate(
        value, (source(value, user_id=value, vote=1),), reference_time=100.0
    )
    assert result.content_id == value
    assert list(result.user_contributions) == [value]


@pytest.mark.parametrize("field", ["source_id", "user_id", "content_id"])
def test_id_limit_counts_utf8_bytes(field: str) -> None:
    with pytest.raises(ValueError, match=field):
        if field == "content_id":
            _ = ConsensusAnalyzer().evaluate("\u00e9" * 257, (), reference_time=100.0)
        else:
            _ = source(**{field: "\u00e9" * 257})


@pytest.mark.parametrize("field", ["source_id", "content_id"])
def test_required_ids_reject_null(field: str) -> None:
    with pytest.raises(ValueError, match=field):
        if field == "content_id":
            _ = ConsensusAnalyzer().evaluate(
                cast(str, cast(object, None)), (), reference_time=100.0
            )
        else:
            _ = source(**{field: None})


@pytest.mark.parametrize(
    "kind",
    [
        "official_docs",
        "research_paper",
        "technical_blog",
        "community_wiki",
        "forum_post",
        "social_media",
        "context",
    ],
)
def test_source_kind_literals_are_supported(kind: SourceKind) -> None:
    assert evaluate(source(kind=kind)).validation_count == 1


@pytest.mark.parametrize("kind", ["unknown", "Official_docs", "", None, 1, True, [], {}])
def test_source_kind_rejects_unknown_values_even_for_context(kind: object) -> None:
    with pytest.raises(ValueError, match="kind"):
        _ = source(kind=kind, is_context=True)


@pytest.mark.parametrize("value", [0, 1, "true", None])
def test_context_flag_is_boolean(value: object) -> None:
    with pytest.raises(ValueError, match="is_context"):
        _ = source(is_context=value)


@pytest.mark.parametrize("field", ["quality_score", "evidence_score", "vote", "previous_score"])
@pytest.mark.parametrize("value", [-0.01, 1.01, nan, inf, -inf, True, False, "0.5", 1j, 10**500])
def test_unit_scores_reject_invalid_values(field: str, value: object) -> None:
    with pytest.raises(ValueError, match=field):
        if field == "previous_score":
            _ = evaluate(previous_score=cast(float, value))
        else:
            _ = source(**{field: value})


@pytest.mark.parametrize("field", ["quality_score", "evidence_score"])
def test_required_scores_reject_null(field: str) -> None:
    with pytest.raises(ValueError, match=field):
        _ = source(**{field: None})


@pytest.mark.parametrize("value", [0, 0.0, 1, 1.0])
def test_unit_score_endpoints_are_inclusive(value: float) -> None:
    result = evaluate(
        source(quality_score=value, evidence_score=value, vote=value, user_id="u"),
        previous_score=value,
    )
    assert 0 <= result.consensus_score <= 1


@pytest.mark.parametrize("field", ["timestamp", "reference_time"])
@pytest.mark.parametrize(
    "value",
    [
        nan,
        inf,
        -inf,
        True,
        False,
        "100",
        None,
        1j,
        10**500,
        MIN_TIMESTAMP - 1,
        MAX_TIMESTAMP,
    ],
)
def test_timestamps_reject_invalid_values(field: str, value: object) -> None:
    with pytest.raises(ValueError, match=field):
        if field == "reference_time":
            _ = ConsensusAnalyzer().evaluate("claim", (), reference_time=cast(float, value))
        else:
            _ = source(timestamp=value)


@pytest.mark.parametrize("timestamp", [MIN_TIMESTAMP, -1, 0, nextafter(MAX_TIMESTAMP, -inf)])
def test_timestamp_bounds_and_negative_unix_times(timestamp: float) -> None:
    result = ConsensusAnalyzer().evaluate(
        "claim", (source(timestamp=timestamp),), reference_time=timestamp
    )
    assert result.temporal_weight == 1.0


@pytest.mark.parametrize("others", [(), (source("old", timestamp=0),)])
def test_future_source_is_rejected_even_if_another_source_is_old(
    others: tuple[ConsensusSource, ...],
) -> None:
    with pytest.raises(ValueError, match="after reference_time"):
        _ = evaluate(*others, source("future", timestamp=nextafter(100.0, inf)))


@pytest.mark.parametrize("value", [None, [], {}, "", (None,), ({},)])
def test_sources_reject_null_and_wrong_container_or_item_types(value: object) -> None:
    with pytest.raises(ValueError, match="sources"):
        _ = ConsensusAnalyzer().evaluate(
            "claim", cast(tuple[ConsensusSource, ...], value), reference_time=100.0
        )


def test_source_count_boundary_and_duplicate_ids() -> None:
    sources = tuple(source(str(index), text="") for index in range(MAX_SOURCES))
    assert evaluate(*sources).validation_count == MAX_SOURCES
    with pytest.raises(ValueError, match="at most 200"):
        _ = evaluate(*sources, source("overflow"))
    with pytest.raises(ValueError, match="unique"):
        _ = evaluate(source("duplicate"), source("duplicate", text="Different source text."))


@pytest.mark.parametrize("value", [None, 123, b"text", "\ud800"])
def test_text_rejects_non_strings_and_invalid_utf8(value: object) -> None:
    with pytest.raises(ValueError, match="text"):
        _ = source(text=value)


@pytest.mark.parametrize("character", ["x", "\u00e9", "\U0001f600"])
def test_per_source_text_limit_is_utf8_bytes(character: str) -> None:
    text = character * (MAX_SOURCE_TEXT_BYTES // len(character.encode("utf-8")))
    assert evaluate(source(text=text)).validation_count == 1
    with pytest.raises(ValueError, match="text"):
        _ = source(text=text + "x")


def test_total_text_limit_includes_each_source_and_counts_utf8() -> None:
    text = "\u00e9" * (MAX_SOURCE_TEXT_BYTES // 2)
    sources = tuple(source(str(index), text=text) for index in range(8))
    assert sum(len(item.text.encode("utf-8")) for item in sources) == MAX_TOTAL_TEXT_BYTES
    assert evaluate(*sources, source("empty", text="")).validation_count == 9
    with pytest.raises(ValueError, match="total source text"):
        _ = evaluate(*sources, source("extra", text="x"))


@pytest.mark.parametrize("text", ["", " \t\n", "...!!!", "the and of", "It is.", "Has is are."])
def test_empty_lexical_evidence_does_not_manufacture_agreement(text: str) -> None:
    result = evaluate(source("a", text=text), source("b", text=text))
    assert result.term_agreement == result.fact_agreement == 0
    assert result.state == ConsensusState.INSUFFICIENT


def test_empty_sources_have_zero_signals_and_real_absence_of_contributions() -> None:
    result = evaluate()
    assert result == ConsensusResult("claim", 0, 0, 0, ConsensusState.INSUFFICIENT, 0, 0, 0, {})
    assert evaluate(previous_score=0.8).state == ConsensusState.REVOKED


def test_single_source_cannot_agree_with_itself() -> None:
    result = evaluate(source(quality_score=1, evidence_score=1, user_id="u", vote=1))
    assert result.term_agreement == result.fact_agreement == 0
    assert result.state == ConsensusState.INSUFFICIENT


def test_repeated_user_votes_have_one_users_weight_and_average_contributions() -> None:
    first = source("a", user_id="repeat", vote=0, timestamp=0)
    second = source("b", user_id="repeat", vote=0.6, timestamp=50)
    other = source("c", user_id="other", vote=1)
    result = evaluate(first, second, other)
    expected_vote = ((0 + 0.6) / 2 + 1) / 2
    expected_score = (0.32 + 0.42 + 0.16 * result.reliability_score + 0.1 * expected_vote) * (
        0.72 + 0.28 * result.temporal_weight
    )
    assert result.consensus_score == pytest.approx(expected_score)
    expected_contribution = (
        fsum(
            [
                2 ** (-100 / 604800) * (1 - abs(result.consensus_score)),
                2 ** (-50 / 604800) * (1 - abs(result.consensus_score - 0.6)),
            ]
        )
        / 2
    )
    assert result.user_contributions["repeat"] == pytest.approx(expected_contribution)
    assert list(result.user_contributions) == ["other", "repeat"]


def test_copying_one_users_identical_vote_does_not_change_vote_weight() -> None:
    first = source("a", user_id="one", vote=0)
    second = source("b", user_id="two", vote=1)
    baseline = evaluate(first, second)
    repeated = evaluate(first, second, replace(first, source_id="copy"))
    assert repeated.consensus_score == baseline.consensus_score
    assert repeated.user_contributions == baseline.user_contributions
    assert repeated.validation_count == 3


@pytest.mark.parametrize("vote", [0, 1, None])
def test_anonymous_votes_are_excluded_and_null_votes_do_not_create_voters(
    vote: float | None,
) -> None:
    anonymous = source("anonymous", vote=vote)
    named_without_vote = source("named", user_id="user", vote=None)
    result = evaluate(anonymous, named_without_vote)
    assert result == evaluate(replace(anonymous, vote=None), named_without_vote)
    assert result.user_contributions == {}
    authenticated = source("voter", user_id="authenticated", vote=0.2)
    assert evaluate(anonymous, authenticated) == evaluate(
        replace(anonymous, vote=None), authenticated
    )


def test_sources_without_votes_do_not_dilute_a_users_contribution() -> None:
    sources = (source("a", user_id="u", vote=0.4), source("b", user_id="u", vote=None))
    result = evaluate(*sources)
    assert result.user_contributions == {"u": 1 - abs(result.consensus_score - 0.4)}


def test_permutations_preserve_every_result_field_and_contribution_order() -> None:
    sources = (
        source("z", user_id="z", vote=0.11, timestamp=1, quality_score=0.9),
        source("a", user_id="a", vote=0.72, text="The bridge is closed.", timestamp=2),
        source("c", user_id="z", vote=0.92, text="The bridge is open today.", timestamp=3),
        source("b", user_id=None, vote=1, text="It is.", timestamp=4, evidence_score=0.07),
    )
    baseline = evaluate(*sources)
    for permutation in permutations(sources):
        result = evaluate(*permutation)
        assert result == baseline
        assert list(result.user_contributions.items()) == list(baseline.user_contributions.items())


@pytest.mark.parametrize(
    ("score", "previous", "expected"),
    [
        (0, None, ConsensusState.INSUFFICIENT),
        (nextafter(0.4, 0), None, ConsensusState.INSUFFICIENT),
        (0.4, None, ConsensusState.EMERGING),
        (nextafter(0.6, 0), None, ConsensusState.EMERGING),
        (0.6, None, ConsensusState.PROVISIONAL),
        (nextafter(0.8, 0), None, ConsensusState.PROVISIONAL),
        (0.8, None, ConsensusState.ESTABLISHED),
        (1, None, ConsensusState.ESTABLISHED),
        (0.2, 0.8, ConsensusState.REVOKED),
        (nextafter(0.4, 0), 0.8, ConsensusState.REVOKED),
        (0.4, 0.8, ConsensusState.CONTESTED),
        (nextafter(0.6, 0), 0.8, ConsensusState.CONTESTED),
        (0.6, 0.8, ConsensusState.ESTABLISHED),
        (0.2, nextafter(0.8, 0), ConsensusState.INSUFFICIENT),
    ],
)
def test_all_six_state_transitions_at_exact_boundaries(
    score: float,
    previous: float | None,
    expected: ConsensusState,
) -> None:
    assert ConsensusAnalyzer()._state(score, previous) == expected  # pyright: ignore[reportPrivateUsage]


def test_meaningful_agreement_and_contradiction_are_lexical_not_truth_claims() -> None:
    agreeing = evaluate(source("a"), source("b", text="The bridge is open today."))
    contradicting = evaluate(source("a"), source("b", text="The bridge is closed."))
    negated = evaluate(source("a"), source("b", text="The bridge is not open."))
    assert agreeing.fact_agreement == pytest.approx(2 / 3)
    assert contradicting.fact_agreement == pytest.approx(1 / 3)
    # A negated claim shares vocabulary: no invented semantic contradiction detector.
    assert negated.fact_agreement == pytest.approx(2 / 3)
    assert agreeing.consensus_score > contradicting.consensus_score
    identical = evaluate(source("a"), source("b"))
    assert identical.fact_agreement == identical.term_agreement == 1


def test_fact_vocabulary_preserves_numbers_and_ignores_sentence_repetition() -> None:
    left = source("a", text="The bridge has 2 lanes.")
    right = source("b", text="The bridge has 4 lanes.")
    result = evaluate(left, right)
    assert result.fact_agreement == 0.5
    assert evaluate(left, replace(right, text=right.text * 3)).fact_agreement == 0.5
    assert (
        evaluate(source("a", text="Bridge open"), source("b", text="Bridge open")).fact_agreement
        == 0
    )


def test_fact_vocabulary_normalizes_unicode_and_retains_empty_pairs_in_denominator() -> None:
    left = source("a", text="The CAF\u00c9 is open.")
    right = source("b", text="The cafe\u0301 is open.")
    assert evaluate(left, right).fact_agreement == 1
    result = evaluate(left, right, source("c", text=""))
    assert result.fact_agreement == result.term_agreement == pytest.approx(1 / 3)


def test_context_kind_is_an_unverified_prior_and_decay_uses_oldest_source() -> None:
    explicit = source("a", kind="context", quality_score=0.2, evidence_score=0.8)
    overridden = replace(explicit, kind="official_docs", is_context=True)
    assert evaluate(explicit) == evaluate(overridden)
    result = evaluate(explicit)
    assert result.reliability_score == pytest.approx(0.42 * 0.62 + 0.3 * 0.2 + 0.28 * 0.8)
    old = replace(explicit, timestamp=100 - 604800)
    assert evaluate(old, source("new")).temporal_weight == 0.5


def test_extreme_valid_age_underflows_to_zero_without_invalid_scores() -> None:
    result = ConsensusAnalyzer().evaluate(
        "claim",
        (source(timestamp=MIN_TIMESTAMP, user_id="u", vote=1),),
        reference_time=nextafter(MAX_TIMESTAMP, -inf),
    )
    assert result.temporal_weight == 0
    assert result.user_contributions == {"u": 0}
    assert 0 <= result.consensus_score <= 1


@pytest.mark.parametrize(
    ("texts", "previous", "expected"),
    [
        (("Bridge open", "Other words"), None, ConsensusState.INSUFFICIENT),
        (("Bridge open", "Bridge open"), None, ConsensusState.EMERGING),
        (("Bridge is open", "Bridge is open today"), None, ConsensusState.PROVISIONAL),
        (("Bridge is open", "Bridge is open"), None, ConsensusState.ESTABLISHED),
        (("Bridge open", "Bridge open"), 0.8, ConsensusState.CONTESTED),
        (("Bridge open", "Other words"), 0.8, ConsensusState.REVOKED),
    ],
)
def test_all_states_are_reachable_through_public_evaluation(
    texts: tuple[str, str],
    previous: float | None,
    expected: ConsensusState,
) -> None:
    assert (
        evaluate(
            source("a", text=texts[0]), source("b", text=texts[1]), previous_score=previous
        ).state
        == expected
    )


@pytest.mark.parametrize("seed", range(25))
def test_seeded_valid_inputs_remain_bounded_and_permutation_invariant(seed: int) -> None:
    rng = Random(seed)
    sources = [
        source(
            str(index),
            text=rng.choice(["", "It is.", "Bridge is open", "Bridge is not open", "Other words"]),
            timestamp=rng.uniform(-1e8, 100),
            quality_score=rng.random(),
            evidence_score=rng.random(),
            user_id=rng.choice([None, "a", "b", "c"]),
            vote=rng.choice([None, rng.random()]),
        )
        for index in range(rng.randrange(1, 40))
    ]
    baseline = evaluate(*sources)
    for _ in range(4):
        rng.shuffle(sources)
        assert evaluate(*sources) == baseline
    values = [
        baseline.consensus_score,
        baseline.reliability_score,
        baseline.temporal_weight,
        baseline.term_agreement,
        baseline.fact_agreement,
        *baseline.user_contributions.values(),
    ]
    assert all(0 <= value <= 1 for value in values)
