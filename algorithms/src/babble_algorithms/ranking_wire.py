"""Strict public ranking boundary; every numeric and nested field is validated."""

import math
import re

from babble_algorithms.boundary import InvalidRequest, Json, object_value, string
from babble_algorithms.ranking_time import parse_timestamp
from babble_algorithms.ranking_types import (
    BuiltInLens,
    Candidate,
    DiversityPolicy,
    EvidenceSignals,
    LensStack,
    LensWeight,
    RankingRequest,
    Signals,
    SourceFloor,
)
from babble_algorithms.types import CandidateSource, CandidateSourceContribution, ReputationSignals


def record(value: Json, fields: str) -> dict[str, Json]:
    result = object_value(value)
    if set(result) != set(fields.split()):
        raise InvalidRequest("missing or unknown ranking fields")
    return result


def array(value: Json, maximum: int) -> list[Json]:
    if not isinstance(value, list) or len(value) > maximum:
        raise InvalidRequest("expected a bounded ranking array")
    return value


def number(value: Json, maximum: float = 1.0) -> float:
    if isinstance(value, bool) or not isinstance(value, (float, int)):
        raise InvalidRequest("expected a ranking number")
    try:
        result = float(value)
    except OverflowError:
        raise InvalidRequest("ranking numbers must be finite") from None
    if not math.isfinite(result) or not 0.0 <= result <= maximum:
        raise InvalidRequest("ranking number outside its finite range")
    return result


def count(value: Json, minimum: int = 0) -> int:
    if type(value) is not int or not minimum <= value <= 200:
        raise InvalidRequest("ranking count outside its integer range")
    return value


def source(value: Json) -> CandidateSource:
    if value not in (
        "Following",
        "SocialGraph",
        "SemanticNeighborhood",
        "Temporal",
        "Emerging",
        "Evidence",
        "Contradiction",
        "Exploration",
    ):
        raise InvalidRequest("unsupported candidate source")
    return value


def parse_candidate(value: Json) -> Candidate:
    obj = record(value, "object_id source sources created_at signals")
    identity = string(obj["object_id"], 68)
    if re.fullmatch(r"obj_[0-9a-f]{64}", identity) is None:
        raise InvalidRequest("invalid ranking object ID")
    primary = source(obj["source"])
    sources: list[CandidateSourceContribution] = []
    for raw in array(obj["sources"], 8):
        item = record(raw, "source weight")
        sources.append(CandidateSourceContribution(source(item["source"]), number(item["weight"])))
    names = {item.source for item in sources}
    if len(names) != len(sources) or primary not in names:
        raise InvalidRequest("candidate sources must be unique and include the primary source")
    created_at = string(obj["created_at"])
    try:
        created_at, _ = parse_timestamp(created_at)
    except ValueError:
        raise InvalidRequest("invalid ranking timestamp") from None
    sig = record(
        obj["signals"],
        "social_distance followed_author relevance novelty "
        + "evidence_quality contradiction evidence reputation temporal exploration",
    )
    followed = sig["followed_author"]
    if not isinstance(followed, bool):
        raise InvalidRequest("followed_author must be a boolean")
    evidence = record(
        sig["evidence"], "human_support judgment_support human_contradiction judgment_contradiction"
    )
    reputation = record(
        sig["reputation"],
        "epistemic_accuracy evidence_quality "
        + "social_constructiveness creative_contribution moderation domain_expertise",
    )
    return Candidate(
        identity,
        primary,
        tuple(sources),
        created_at,
        Signals(
            social_distance=number(sig["social_distance"]),
            followed_author=followed,
            relevance=number(sig["relevance"]),
            novelty=number(sig["novelty"]),
            evidence_quality=number(sig["evidence_quality"]),
            contradiction=number(sig["contradiction"]),
            evidence=EvidenceSignals(
                **{key: number(val, math.inf) for key, val in evidence.items()}
            ),
            reputation=ReputationSignals(**{key: number(val) for key, val in reputation.items()}),
            temporal=number(sig["temporal"]),
            exploration=number(sig["exploration"]),
        ),
    )


def parse_ranking_request(value: Json) -> RankingRequest:
    obj = record(value, "candidates lens diversity limit")
    candidates = tuple(parse_candidate(item) for item in array(obj["candidates"], 200))
    if len({candidate.object_id for candidate in candidates}) != len(candidates):
        raise InvalidRequest("ranking candidates must have unique IDs")
    lens = record(obj["lens"], "id weights")
    identity = string(lens["id"], 128)
    if not identity or any(not 0x21 <= ord(char) <= 0x7E for char in identity):
        raise InvalidRequest("Lens stack ID must be visible ASCII")
    weights: list[LensWeight] = []
    for raw in array(lens["weights"], 8):
        item = record(raw, "lens weight")
        try:
            builtin = BuiltInLens(string(item["lens"]))
        except ValueError:
            raise InvalidRequest("unsupported built-in Lens") from None
        weights.append(LensWeight(builtin, number(item["weight"], math.inf)))
    if len({weight.lens for weight in weights}) != len(weights):
        raise InvalidRequest("Lens weights must be unique")
    diversity = record(obj["diversity"], "max_source_share source_floors")
    floors: list[SourceFloor] = []
    for raw in array(diversity["source_floors"], 8):
        item = record(raw, "source minimum")
        floors.append(SourceFloor(source(item["source"]), count(item["minimum"])))
    if len({floor.source for floor in floors}) != len(floors):
        raise InvalidRequest("source floors must be unique")
    return RankingRequest(
        candidates,
        LensStack(identity, tuple(weights)),
        DiversityPolicy(number(diversity["max_source_share"]), tuple(floors)),
        count(obj["limit"], 1),
    )
