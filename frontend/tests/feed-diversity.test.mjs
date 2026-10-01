import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import * as sdk from "@babble-protocol/sdk";
import { mediaResource } from "./media-modules.mjs";

function load(name, require = () => ({})) {
  const context = { exports: {}, require, URL, fetch, AbortController, AbortSignal, Response,
    TextEncoder, TextDecoder, structuredClone, crypto: globalThis.crypto };
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, context);
  return context.exports;
}
const profiles = load("profile-response"), invocations = load("invocations", () => sdk);
const { BabbleFrontendClient } = load("protocol", name => name === "./invocations" ? invocations
  : name === "@babble-protocol/sdk" ? sdk : name === "./media-resource" ? mediaResource : profiles);
const native = JSON.parse(readFileSync(new URL("../../fixtures/protocol/v1/ranking.json", import.meta.url), "utf8"));
const objectId = i => `obj_${i.toString(16).padStart(64, "0")}`;
const author = i => `id_${i.toString(16).padStart(64, "0")}`;
const policy = { max_source_share: .55, source_floors: [
  { source: "Exploration", minimum: 1 }, { source: "Contradiction", minimum: 1 },
] };

function response(count = 24) {
  const ranked = Array.from({ length: count }, (_, i) => {
    const candidate = structuredClone(native.cases[0].request.candidates[0]);
    candidate.object_id = objectId(i + 1);
    candidate.created_at = "2026-09-30T00:00:00Z";
    candidate.source = i === count - 2 ? "Contradiction" : i === count - 1 ? "Exploration" : "Temporal";
    candidate.sources = [{ source: candidate.source, weight: 1 }];
    Object.assign(candidate.signals, { novelty: 0, contradiction: 0, exploration: 0, evidence_quality: 0 });
    return { candidate, score: candidate.source === "Temporal" ? .9 : candidate.source === "Contradiction" ? .75 : .7,
      reasons: [{ signal: "diversity:stale_public_marker", contribution: .17 }] };
  });
  const objects = ranked.map(({ candidate }, i) => ({ id: candidate.object_id, author: author(i < 9 ? 1 : 2),
    created_at: candidate.created_at, kind: "text", schema: "babble.text.v1", protocol: { name: "babble", version: 1 },
    payload: { text: `Public note ${i}` }, provenance: { parent: null, forked_from: null, remixed_from: [] },
    relations: [], resources: [], surfaces: [], capabilities: [] }));
  return { ranked, objects, trace: { candidates: [] }, temporal: null,
    ranking_provider: { provider: "babble-python", model: "lenses-v1", version: "1" },
    diversity_trace: { policy, filtered: [], candidates: ranked.map((entry, i) => ({ rank: i + 1,
      object_id: entry.candidate.object_id, source: entry.candidate.source, lens_score: entry.score,
      diversified_score: entry.score, reasons: [{ signal: "stale_public_marker", contribution: .17 }] })) } };
}

function clientFor(data) {
  const client = new BabbleFrontendClient("https://babble.example"), requests = [], hydrated = [];
  client.catalog = async () => ({ methods: [] });
  client.rpc = async (method, input) => {
    requests.push({ method, input });
    assert.equal(method, "babble.discovery.candidates.v1");
    return { discovery: data };
  };
  const original = client.objectToCard.bind(client);
  client.objectToCard = input => { hydrated.push(input.object.id); return original(input); };
  return { client, requests, hydrated };
}

test("private filtering precedes the nine-card limit and final source diversity", async () => {
  const data = response(), before = structuredClone(data), { client, requests, hydrated } = clientFor(data);
  const result = await client.loadFeed("  public note  ", "research", {
    hidden_authors: [author(1)], interests: ["private-interest"], model_revision: "private-revision",
  });
  assert.equal(requests.length, 1);
  assert.equal(requests[0].input.limit, 200);
  assert.equal(requests[0].input.search, "public note");
  assert.ok(!JSON.stringify(requests).includes("private-"));
  assert.ok(!JSON.stringify(requests).includes(author(1)));
  assert.equal(result.cards.length, 9, "hidden initial results must not consume visible slots");
  assert.equal(hydrated.length, 9, "only the selected cards should be hydrated");
  assert.equal(result.personalization.filtered, 9);
  assert.equal(result.diversity.filtered, 6);
  assert.equal(result.diversity.active, true);
  assert.equal(result.diversity.maxSourceShare, .55);
  assert.deepEqual(Array.from(result.diversity.floors), ["Exploration 1", "Contradiction 1"]);
  assert.deepEqual(Array.from(result.cards.slice(0, 3), card => card.id), [objectId(10), objectId(23), objectId(24)]);
  assert.ok(result.cards.every(card => card.author !== author(1)));
  assert.ok(result.cards[1].reasons.includes("local.diversity.source_floor +18%"));
  assert.ok(result.cards[2].reasons.includes("local.diversity.source_floor +18%"));
  assert.ok(!JSON.stringify(result.cards.map(card => card.reasons)).includes("stale_public_marker"));
  assert.deepEqual(data, before, "public ranking and provenance must remain immutable");
  for (const card of result.cards) {
    assert.equal(card.source, "local");
    assert.deepEqual(card.rankingProvider, data.ranking_provider);
  }
});

test("disabled personalization retains the public nine-result request and its public reasons", async () => {
  const data = response(9), { client, requests } = clientFor(data);
  const result = await client.loadFeed("needle", "balanced", null);
  assert.equal(requests[0].input.limit, 9);
  assert.equal(result.personalization.boundary, "none");
  assert.equal(result.cards.length, 9);
  assert.ok(result.cards[0].reasons.includes("diversity.stale_public_marker +17%"));
});

test("all-filtered private pools stay empty and never restore candidates to meet source floors", async () => {
  const { client, requests, hydrated } = clientFor(response());
  const result = await client.loadFeed("missing", "weird", { hidden_authors: [author(1), author(2)] });
  assert.equal(requests.length, 1, "do not bypass discovery with raw search");
  assert.equal(result.cards.length, 0);
  assert.equal(hydrated.length, 0);
  assert.equal(result.personalization.filtered, 24);
  assert.equal(result.diversity.filtered, 0);
  assert.equal(result.diversity.active, false);
  assert.equal(result.diversity.maxSourceShare, .55);
});

test("private order is deterministic under input permutations and follows no alternate feed route", async () => {
  const data = response(), original = await clientFor(data).client.loadFeed("", "balanced", {});
  const reversed = { ...data, ranked: [...data.ranked].reverse(), objects: [...data.objects].reverse() };
  const result = await clientFor(reversed).client.loadFeed("", "balanced", {});
  assert.deepEqual(Array.from(result.cards, card => card.id), Array.from(original.cards, card => card.id));
  assert.equal(new Set(result.cards.map(card => card.id)).size, 9);
  const { client, requests } = clientFor(data);
  await assert.rejects(client.loadFeed("", "following", {}), /authenticated chronological feed/);
  assert.equal(requests.length, 0);
});
