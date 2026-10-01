import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { once } from "node:events";
import test from "node:test";
import {
  createBabelSDK, hostBinding, HttpRpcTransport, personalizeCandidates, personalizeFeed, summarizeDiscoveryObject,
} from "../dist/index.js";
import { diversifyRanked } from "../dist/diversity.js";

const fixtures = JSON.parse(readFileSync(new URL("../../fixtures/protocol/v1/ranking.json", import.meta.url)));
const policy = {
  max_source_share: 0.55,
  source_floors: [{ source: "Exploration", minimum: 1 }, { source: "Contradiction", minimum: 1 }],
};
const noDiversity = { max_source_share: 1, source_floors: [] };
const ids = (trace) => trace.ranked.map((entry) => entry.ranked.candidate.object_id);
const id = (n) => `obj_${n.toString(16).padStart(64, "0")}`;

assert.equal(fixtures.cases.length, 19);
for (const fixture of fixtures.cases) {
  test(`canonical native diversity: ${fixture.name}`, () => {
    // Lens trace scores precede diversity; the public result.ranked scores do not.
    const ranked = fixture.result.trace.candidates.map((entry) => ({
      candidate: fixture.request.candidates.find((candidate) => candidate.object_id === entry.object_id),
      score: entry.score,
      reasons: [],
    }));
    const original = structuredClone(ranked);
    freeze(ranked);
    const trace = diversifyRanked(ranked, freeze(structuredClone(fixture.request.diversity)), fixture.request.limit);
    close(trace, fixture.result.diversity_trace);
    assert.deepEqual(trace.candidates.map((entry) => entry.object_id), fixture.result.ranked.map((entry) => entry.candidate.object_id));
    for (let index = 0; index < trace.candidates.length; index++) {
      close(trace.candidates[index].diversified_score, fixture.result.ranked[index].score);
      close(trace.candidates[index].reasons, fixture.result.ranked[index].reasons
        .filter((entry) => entry.signal.startsWith("diversity:"))
        .map((entry) => ({ ...entry, signal: entry.signal.slice("diversity:".length) })));
    }
    assert.deepEqual(ranked, original);
    assert.deepEqual(diversifyRanked([...ranked].reverse(), fixture.request.diversity, fixture.request.limit), trace);
  });
}

test("private scoring concentration is corrected after filtering and before final nine, without network leakage", async (t) => {
  const candidates = Array.from({ length: 200 }, (_, index) => candidate(index));
  candidates[197] = candidate(197, "Exploration", 0.85);
  candidates[198] = candidate(198, "Contradiction", 0.85);
  candidates[199] = candidate(199, "Exploration", 1);
  const objects = candidates.map((entry, index) => ({
    id: entry.candidate.object_id,
    author: index === 199 ? "id_private_hidden_author" : "id_public_author",
    kind: "babel.text.v1",
    payload: { text: index < 197 ? "privateinterest" : "neutral" },
  }));
  const model = { interests: ["privateinterest"], hidden_authors: ["id_private_hidden_author"], muted_terms: ["privatemute"] };
  const response = { ranked: candidates, objects, diversity_trace: { policy, candidates: [], filtered: [] } };
  const requests = [];
  const server = createServer(async (req, res) => {
    let body = "";
    for await (const chunk of req) body += chunk;
    requests.push(JSON.parse(body));
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ protocol: "babel.rpc.v1", id: requests.at(-1).id, result: response, error: null, trace_id: null }));
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  t.after(() => { server.closeAllConnections(); server.close(); });
  const sdk = createBabelSDK({
    transport: new HttpRpcTransport(`http://127.0.0.1:${server.address().port}/rpc`),
    binding: hostBinding("feed-test", "http://feed.test"),
  });
  const publicRequest = { limit: 200 };
  const pool = await sdk.discovery.candidates(publicRequest, { id: "feed-pool" });
  const summaries = pool.objects.map(summarizeDiscoveryObject);
  const before = personalizeCandidates(model, pool.ranked, summaries);
  assert.deepEqual(new Set(before.ranked.slice(0, 9).map((entry) => entry.ranked.candidate.source)), new Set(["Following"]));
  const input = freeze({ model, candidates: pool.ranked, summaries, policy: pool.diversity_trace.policy });
  const snapshot = structuredClone(input);
  const final = personalizeFeed(input.model, input.candidates, input.summaries, input.policy, 9);
  assert.equal(final.ranked.length, 9);
  assert.deepEqual(new Set(final.ranked.map((entry) => entry.ranked.candidate.source)), new Set(["Following", "Exploration", "Contradiction"]));
  assert.deepEqual(final.filtered, [{ object_id: id(199), reasons: ["private.hidden_author"] }]);
  assert.equal(final.diversity_trace.filtered.length, 190);
  assert.ok(!ids(final).includes(id(199)));
  assert.ok(!JSON.stringify(final.diversity_trace).includes(id(199)));
  assert.equal(final.privacy_boundary, "local_only");
  for (const literal of ["privateinterest", "id_private_hidden_author", "privatemute"]) {
    assert.ok(!JSON.stringify(final).includes(literal));
    assert.ok(!JSON.stringify(requests).includes(literal));
  }
  assert.ok(!JSON.stringify(requests).includes(id(199)));
  assert.equal(requests.length, 1);
  assert.equal(requests[0].method, "babel.discovery.candidates.v1");
  assert.deepEqual(requests[0].payload, publicRequest);
  for (const entry of final.ranked) {
    const publicEntry = candidates.find((item) => item.candidate.object_id === entry.ranked.candidate.object_id);
    assert.deepEqual(entry.ranked, publicEntry);
    const privateEntry = before.ranked.find((item) => item.ranked.candidate.object_id === publicEntry.candidate.object_id);
    assert.equal(entry.public_score, privateEntry.public_score);
    const adjustment = final.diversity_trace.candidates.find((item) => item.object_id === publicEntry.candidate.object_id);
    assert.equal(adjustment.lens_score, privateEntry.personalized_score);
    assert.equal(entry.personalized_score, adjustment.diversified_score);
    assert.deepEqual(entry.reasons, [...privateEntry.reasons, ...adjustment.reasons.map((reason) => ({
      signal: `private.diversity.${reason.signal}`, contribution: reason.contribution,
    }))]);
    for (const reason of entry.reasons) assert.ok(Number.isFinite(reason.contribution));
  }
  assert.ok(final.ranked.some((entry) => entry.reasons.some((reason) => reason.signal === "private.diversity.source_floor")));
  assert.ok(final.ranked.some((entry) => entry.reasons.some((reason) => reason.signal === "private.diversity.source_concentration")));
  assert.deepEqual(personalizeFeed(input.model, input.candidates, input.summaries, input.policy, 9), final);
  assert.deepEqual(input, snapshot);
});

test("empty and entirely private-filtered pools remain empty with coherent traces", () => {
  const empty = personalizeFeed({}, [], [], policy, 9);
  assert.deepEqual(empty, { privacy_boundary: "local_only", ranked: [], filtered: [], diversity_trace: { policy, candidates: [], filtered: [] } });
  const pool = [candidate(1, "Exploration"), candidate(2, "Contradiction")];
  const summaries = pool.map((entry) => summary(entry, "secretfilter"));
  const filtered = personalizeFeed({ muted_terms: ["secretfilter"], model_revision: "rev-1" }, pool, summaries, policy, 9);
  assert.deepEqual(filtered.ranked, []);
  assert.deepEqual(filtered.diversity_trace, empty.diversity_trace);
  assert.equal(filtered.model_revision, "rev-1");
  assert.equal(filtered.filtered.length, 2);
  assert.ok(!JSON.stringify(filtered).includes("secretfilter"));
});

test("tiny and single-source pools cannot fabricate floors or treat soft limits as quotas", () => {
  const pool = [candidate(1), candidate(2), candidate(3)];
  const final = personalizeFeed({}, pool, [], { ...policy, max_source_share: 0 }, 200);
  assert.equal(final.ranked.length, 3);
  assert.deepEqual(new Set(ids(final)), new Set(pool.map((entry) => entry.candidate.object_id)));
  assert.ok(final.diversity_trace.candidates.every((entry) => entry.reasons.every((reason) => reason.signal !== "source_floor")));
  assert.deepEqual(final.diversity_trace.filtered, []);
  const one = personalizeFeed({}, [candidate(1)], [], policy, 1);
  assert.deepEqual(one.diversity_trace.candidates[0].reasons, []);
  const soft = diversifyRanked([candidate(1, "Following", 1), candidate(2, "Following", 0.95), candidate(3, "Exploration", 0)], policy, 2);
  assert.deepEqual(soft.candidates.map((entry) => entry.object_id), [id(1), id(2)]);
});

test("secondary provenance neither earns nor consumes primary source floor credit", () => {
  const overlap = candidate(1, "Following", 0.8);
  overlap.candidate.sources.push({ source: "Exploration", weight: 0.9 });
  const next = candidate(2, "Exploration", 0.7);
  const trace = diversifyRanked([overlap, next], policy, 2);
  assert.deepEqual(trace.candidates[0].reasons, []);
  assert.deepEqual(trace.candidates[1].reasons, [{ signal: "source_floor", contribution: 0.18 }]);
  const alone = diversifyRanked([candidate(3, "Following", 1), overlap], policy, 2);
  assert.ok(alone.candidates[1].reasons.every((reason) => reason.signal !== "source_floor"));
});

test("timestamp ties use nanoseconds and equivalent UTC offsets, independent of locale or input order", () => {
  const pool = [
    candidate(1, "Following", 0.5, "2026-09-30T01:00:00.000000001+01:00"),
    candidate(2, "Following", 0.5, "2026-09-29T17:00:00.000000002-07:00"),
    candidate(3, "Following", 0.5, "2026-09-30T00:00:00.000000002Z"),
    candidate(4, "Following", 0.5, "2026-09-30T00:00:00.000000003Z"),
    candidate(5, "Following", 0.5, "2026-09-30T01:00:00.000000003+02:00"),
  ];
  const expected = [id(4), id(2), id(3), id(1), id(5)];
  for (let seed = 1; seed <= 40; seed++) {
    const trace = personalizeFeed({}, shuffle(pool, seed), [], noDiversity, 5);
    assert.deepEqual(ids(trace), expected);
  }
  const leap = diversifyRanked([
    candidate(1, "Following", 0.5, "2016-12-31T23:59:60.999999999Z"),
    candidate(2, "Following", 0.5, "2017-01-01T00:00:00Z"),
  ], noDiversity, 2);
  assert.deepEqual(leap.candidates.map((entry) => entry.object_id), [id(2), id(1)]);
});

test("clamped adjusted score ties prefer the unadjusted score before timestamp", () => {
  const trace = diversifyRanked([
    candidate(1, "Following", 1),
    candidate(2, "Exploration", 0.9, "2026-09-29T00:00:00Z"),
    candidate(3, "Contradiction", 0.85, "2026-09-30T00:00:00Z"),
  ], { ...policy, max_source_share: 1 }, 3);
  assert.deepEqual(trace.candidates.map((entry) => entry.object_id), [id(1), id(2), id(3)]);
  assert.equal(trace.candidates[1].diversified_score, 1);
  assert.equal(trace.candidates[2].diversified_score, 1);
});

test("200-candidate private feeds are deterministic under permutations and all legal limits", () => {
  const sources = ["Following", "SocialGraph", "SemanticNeighborhood", "Temporal", "Emerging", "Evidence", "Contradiction", "Exploration"];
  const pool = Array.from({ length: 200 }, (_, index) => candidate(index, sources[index % sources.length], (index % 10) / 10));
  const summaries = pool.map((entry, index) => summary(entry, index % 9 === 0 ? "muted" : index % 2 ? "interest" : "neutral"));
  const model = { interests: ["interest"], muted_terms: ["muted"] };
  const baseline = personalizeFeed(model, pool, summaries, policy, 200);
  for (let limit = 1; limit <= 200; limit++) {
    const trace = personalizeFeed(model, shuffle(pool, limit), shuffle(summaries, limit + 1), policy, limit);
    assert.deepEqual(trace.ranked, baseline.ranked.slice(0, limit));
    assert.deepEqual(trace.filtered, baseline.filtered);
    assert.equal(trace.diversity_trace.filtered.length, baseline.ranked.length - trace.ranked.length);
    assert.equal(new Set([...ids(trace), ...trace.filtered.map((entry) => entry.object_id), ...trace.diversity_trace.filtered]).size, 200);
    assert.deepEqual(trace, personalizeFeed(model, pool, summaries, policy, limit));
  }
});

test("invalid limits, oversized pools, duplicate IDs and summaries are rejected explicitly", () => {
  for (const limit of [0, -1, 201, 1.5, NaN, Infinity, null, "9", undefined]) {
    assert.throws(() => personalizeFeed({}, [], [], policy, limit), /limit/);
  }
  assert.throws(() => personalizeFeed({}, Array.from({ length: 201 }, (_, index) => candidate(index)), [], policy, 9), /200/);
  assert.throws(() => personalizeFeed({}, [candidate(1), candidate(1)], [], policy, 9), /duplicate candidate/);
  assert.throws(() => personalizeFeed({}, [], [summary(candidate(1)), summary(candidate(1))], policy, 9), /duplicate summary/);
  assert.throws(() => personalizeFeed({}, [], Array.from({ length: 201 }, (_, index) => summary(candidate(index))), policy, 9), /200/);
});

test("invalid policies, nonfinite inputs, and invalid sources are rejected even before private filtering", () => {
  for (const bad of [
    null, {}, { ...policy, max_source_share: null }, { ...policy, max_source_share: NaN },
    { ...policy, max_source_share: Infinity }, { ...policy, max_source_share: -0.1 },
    { ...policy, max_source_share: 1.1 }, { ...policy, source_floors: null },
    ...[-1, 0.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1].map((minimum) => ({ ...policy, source_floors: [{ source: "Exploration", minimum }] })),
    { ...policy, source_floors: [{ source: "Unknown", minimum: 1 }] },
    { ...policy, source_floors: [policy.source_floors[0], policy.source_floors[0]] },
  ]) assert.throws(() => personalizeFeed({}, [], [], bad, 9), /policy/);
  for (const mutate of [
    (entry) => { entry.score = NaN; }, (entry) => { entry.score = Infinity; },
    (entry) => { entry.candidate.signals.novelty = NaN; },
    (entry) => { entry.candidate.signals.evidence.human_support = Infinity; },
    (entry) => { entry.candidate.source = "unknown"; }, (entry) => { delete entry.candidate.source; },
    (entry) => { entry.candidate.sources[0].source = "unknown"; },
    (entry) => { entry.candidate.sources[0].weight = -Infinity; },
    (entry) => { entry.reasons[0].contribution = NaN; },
  ]) {
    const entry = candidate(1);
    mutate(entry);
    assert.throws(() => personalizeFeed({ muted_terms: ["hidden"] }, [entry], [summary(entry, "hidden")], policy, 9), /finite|sources/);
  }
  for (const model of [{ novelty_tolerance: NaN }, { creator_affinity: { secret: Infinity } }]) {
    assert.throws(() => personalizeFeed(model, [], [], policy, 9), /finite/);
  }
});

test("invalid RFC3339 dates and offsets are rejected instead of losing deterministic ordering", () => {
  for (const time of ["not-a-date", "2026-02-29T00:00:00Z", "2026-09-31T00:00:00Z", "2026-09-30T24:00:00Z",
    "2026-09-30T00:00:00+24:00", "2026-09-30T00:00:00+01:60", "2026-09-30T00:00:61Z", "2026-09-30T00:00:00.0000000001Z"] ) {
    assert.throws(() => personalizeFeed({}, [candidate(1, "Following", 0.5, time)], [], policy, 9), /RFC3339/);
  }
});

function candidate(index, source = "Following", score = 0.9, created_at = "2026-09-30T00:00:00Z") {
  return {
    candidate: {
      object_id: id(index), source, sources: [{ source, weight: 1 }], created_at,
      signals: { ...structuredClone(fixtures.cases[0].request.candidates[0].signals), novelty: 0, contradiction: 0 },
    },
    score,
    reasons: [{ signal: "public.baseline", contribution: score }],
  };
}

function summary(entry, text = "neutral") {
  return { object_id: entry.candidate.object_id, author: "id_public", kind: "babel.text.v1", text, topics: [] };
}

function freeze(value) {
  if (value && typeof value === "object") {
    for (const child of Object.values(value)) freeze(child);
    Object.freeze(value);
  }
  return value;
}

function shuffle(items, seed) {
  const output = [...items];
  let state = seed;
  for (let index = output.length - 1; index > 0; index--) {
    state = (Math.imul(state, 1664525) + 1013904223) >>> 0;
    const swap = state % (index + 1);
    [output[index], output[swap]] = [output[swap], output[index]];
  }
  return output;
}

function close(actual, expected) {
  if (typeof expected === "number") {
    assert.ok(Number.isFinite(actual) && Math.abs(actual - expected) <= 1e-12, `${actual} != ${expected}`);
  } else if (expected !== null && typeof expected === "object") {
    assert.deepEqual(Object.keys(actual), Object.keys(expected));
    for (const key of Object.keys(expected)) close(actual[key], expected[key]);
  } else {
    assert.deepEqual(actual, expected);
  }
}
