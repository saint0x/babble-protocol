import type {
  ApiDiscoveryResponse_CandidateSource,
  ApiDiscoveryResponse_DiversityPolicy,
  ApiDiscoveryResponse_DiversityReason,
  ApiDiscoveryResponse_DiversityTrace,
  ApiDiscoveryResponse_RankedCandidate,
} from "./generated/protocol.js";

const SOURCES: readonly ApiDiscoveryResponse_CandidateSource[] = [
  "Following", "SocialGraph", "SemanticNeighborhood", "Temporal",
  "Emerging", "Evidence", "Contradiction", "Exploration",
];

export function validateDiversityInput(
  candidates: readonly ApiDiscoveryResponse_RankedCandidate[],
  policy: ApiDiscoveryResponse_DiversityPolicy,
  limit: number,
): void {
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > 200) {
    throw new Error("personalizeFeed limit must be an integer in 1..200");
  }
  if (!Array.isArray(candidates) || candidates.length > 200) {
    throw new Error("personalizeFeed accepts at most 200 candidates");
  }
  if (!policy || !Number.isFinite(policy.max_source_share) ||
      policy.max_source_share < 0 || policy.max_source_share > 1 || !Array.isArray(policy.source_floors)) {
    throw new Error("diversity policy requires max_source_share in 0..1 and source_floors");
  }
  const floors = new Set<string>();
  for (const floor of policy.source_floors) {
    if (!floor || !SOURCES.includes(floor.source) || !Number.isSafeInteger(floor.minimum) || floor.minimum < 0) {
      throw new Error("diversity policy requires valid sources and non-negative safe integer minimums");
    }
    if (floors.has(floor.source)) throw new Error("diversity policy contains duplicate source floors");
    floors.add(floor.source);
  }
  const ids = new Set<string>();
  for (const entry of candidates) {
    const candidate = entry.candidate;
    if (!candidate || typeof candidate.object_id !== "string" || candidate.object_id.length === 0) {
      throw new Error("personalizeFeed requires candidate object IDs");
    }
    if (ids.has(candidate.object_id)) throw new Error("personalizeFeed contains duplicate candidate IDs");
    ids.add(candidate.object_id);
    if (!SOURCES.includes(candidate.source) || !Array.isArray(candidate.sources) ||
        candidate.sources.some((source: ApiDiscoveryResponse_RankedCandidate["candidate"]["sources"][number]) =>
          !source || !SOURCES.includes(source.source) || !Number.isFinite(source.weight))) {
      throw new Error("personalizeFeed requires valid candidate sources and finite weights");
    }
    if (!Number.isFinite(entry.score)) throw new Error("personalizeFeed requires finite scores");
    assertFiniteNumbers(candidate.signals);
    for (const reason of entry.reasons) {
      if (!Number.isFinite(reason.contribution)) throw new Error("personalizeFeed requires finite contributions");
    }
    timestamp(candidate.created_at);
  }
}

export function assertFiniteNumbers(value: unknown): void {
  if (typeof value === "number" && !Number.isFinite(value)) {
    throw new Error("personalizeFeed requires finite numeric inputs");
  }
  if (value !== null && typeof value === "object") {
    for (const child of Object.values(value)) assertFiniteNumbers(child);
  }
}

// Internal score-stage shared by the private feed and native fixture conformance tests.
export function diversifyRanked(
  ranked: readonly ApiDiscoveryResponse_RankedCandidate[],
  inputPolicy: ApiDiscoveryResponse_DiversityPolicy,
  limit: number,
): ApiDiscoveryResponse_DiversityTrace {
  validateDiversityInput(ranked, inputPolicy, limit);
  const policy = {
    max_source_share: inputPolicy.max_source_share,
    source_floors: inputPolicy.source_floors.filter((floor) => floor.minimum > 0).map((floor) => ({ ...floor })),
  };
  const remaining = ranked.map((entry) => ({ entry, time: timestamp(entry.candidate.created_at) }));
  // Also canonicalize omitted IDs, independently of caller iteration order.
  remaining.sort((left, right) => compareBase(left, right));
  const selected: ApiDiscoveryResponse_DiversityTrace["candidates"][number][] = [];
  const counts = new Map<ApiDiscoveryResponse_CandidateSource, number>();
  while (remaining.length > 0 && selected.length < limit) {
    let bestIndex = 0;
    let bestScore = -Infinity;
    let bestReasons: ApiDiscoveryResponse_DiversityReason[] = [];
    for (let index = 0; index < remaining.length; index++) {
      const current = remaining[index]!;
      const reasons = diversityReasons(current.entry.candidate.source, counts, selected.length, policy);
      const score = Math.min(1, Math.max(0,
        current.entry.score + reasons.reduce((total, reason) => total + reason.contribution, 0)));
      if (score > bestScore || (score === bestScore && compareBase(current, remaining[bestIndex]!) < 0)) {
        bestIndex = index;
        bestScore = score;
        bestReasons = reasons;
      }
    }
    const { entry } = remaining.splice(bestIndex, 1)[0]!;
    const source = entry.candidate.source;
    selected.push({
      rank: selected.length + 1,
      object_id: entry.candidate.object_id,
      source,
      lens_score: entry.score,
      diversified_score: bestScore,
      reasons: bestReasons,
    });
    counts.set(source, (counts.get(source) ?? 0) + 1);
  }
  return { policy, candidates: selected, filtered: remaining.map(({ entry }) => entry.candidate.object_id) };
}

function diversityReasons(
  source: ApiDiscoveryResponse_CandidateSource,
  counts: ReadonlyMap<ApiDiscoveryResponse_CandidateSource, number>,
  selected: number,
  policy: ApiDiscoveryResponse_DiversityPolicy,
): ApiDiscoveryResponse_DiversityReason[] {
  if (selected === 0) return [];
  const count = counts.get(source) ?? 0;
  const reasons: ApiDiscoveryResponse_DiversityReason[] = [];
  if (policy.source_floors.some((floor) => floor.source === source && count < floor.minimum)) {
    reasons.push({ signal: "source_floor", contribution: 0.18 / (count + 1) });
  }
  const nextShare = (count + 1) / (selected + 1);
  if (nextShare > policy.max_source_share) {
    const penalty = Math.min(0.25, Math.max(0,
      (nextShare - policy.max_source_share) / Math.max(1 - policy.max_source_share, 0.01) * 0.25));
    if (penalty !== 0) reasons.push({ signal: "source_concentration", contribution: -penalty });
  }
  return reasons;
}

type TimedCandidate = { entry: ApiDiscoveryResponse_RankedCandidate; time: ReturnType<typeof timestamp> };

function compareBase(left: TimedCandidate, right: TimedCandidate): number {
  return right.entry.score - left.entry.score || right.time.seconds - left.time.seconds ||
    right.time.nanoseconds - left.time.nanoseconds ||
    compareIds(left.entry.candidate.object_id, right.entry.candidate.object_id);
}

export function compareIds(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

function timestamp(value: string): { seconds: number; nanoseconds: number } {
  const match = /^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(?:\.(\d{1,9}))?([Zz]|[+-]\d{2}:\d{2})$/u.exec(value);
  if (!match) throw new Error("personalizeFeed requires RFC3339 candidate timestamps with up to nanosecond precision");
  const [, year, month, day, hour, minute, second, fraction, zone] = match;
  const seconds = Number(second);
  const date = new Date(0);
  date.setUTCFullYear(Number(year), Number(month) - 1, Number(day));
  date.setUTCHours(Number(hour), Number(minute), Math.min(seconds, 59), 0);
  if (date.getUTCFullYear() !== Number(year) || date.getUTCMonth() !== Number(month) - 1 ||
      date.getUTCDate() !== Number(day) || Number(hour) > 23 || Number(minute) > 59 || seconds > 60) {
    throw new Error("personalizeFeed requires valid RFC3339 calendar timestamps");
  }
  let offset = 0;
  if (zone !== "Z" && zone !== "z") {
    const hours = Number(zone!.slice(1, 3));
    const minutes = Number(zone!.slice(4, 6));
    if (hours > 23 || minutes > 59) throw new Error("personalizeFeed requires valid RFC3339 offsets");
    offset = (hours * 60 + minutes) * 60 * (zone![0] === "+" ? 1 : -1);
  }
  // Keep subsecond precision separately, including Chrono's leap-second representation.
  return {
    seconds: date.getTime() / 1000 - offset,
    nanoseconds: Number((fraction ?? "").padEnd(9, "0")) + (seconds === 60 ? 1_000_000_000 : 0),
  };
}
