import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import * as sdk from "@babble-protocol/sdk";
import { mediaResource } from "./media-modules.mjs";

function load(name, require = () => ({}), globals = {}) {
  const context = { exports: {}, require, URL, fetch, AbortController, AbortSignal, Response, TextEncoder, TextDecoder,
    structuredClone, console, crypto: globalThis.crypto, ...globals };
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, context);
  return context.exports;
}
const profiles = load("profile-response");
const invocations = load("invocations", () => sdk);
const { BabbleFrontendClient } = load("protocol", name => name === "./invocations" ? invocations
  : name === "@babble-protocol/sdk" ? sdk : name === "./media-resource" ? mediaResource : profiles);
const object = id => ({ id, author: "id_author", created_at: "2026-09-29T12:00:00Z",
  kind: "text", schema: "babble.text.v1", protocol: { name: "babble", version: 1 }, payload: { text: "Public content" },
  provenance: { parent: null, forked_from: null, remixed_from: [] }, relations: [], resources: [], surfaces: [], capabilities: [] });
const score = (id, survival = 0.456789) => ({ object_id: id, age_hours: 12.25, recency: 0.82, decay_rate: 0.1,
  time_sensitivity: 0.68, engagement_velocity: 0.12, survival_score: survival });
const evaluation = (provider = "babble-python") => ({ provider: { provider, model: "temporal-v1", version: "1" },
  reference_time: "2026-09-30T00:15:00.123456+00:00", scores: [score("b", 0.25), score("outside"), score("a")] });
const signals = { relevance: 0.6, novelty: 0.4, evidence_quality: 0.2, contradiction: 0.1, temporal: 0.456789,
  reputation: { evidence_quality: 0, domain_expertise: 0, epistemic_accuracy: 0, social_constructiveness: 0, creative_contribution: 0 } };
function discovery(temporal = evaluation()) {
  return { objects: [object("a"), object("b")], temporal,
    ranking_provider: { provider: "public-ranking", model: "ranking-v1", version: "1" },
    ranked: ["b", "a"].map(id => ({ candidate: { object_id: id, source: "Temporal", signals, sources: [{ source: "Temporal", weight: 1 }], created_at: object(id).created_at }, score: 0.7, reasons: [] })),
    trace: { candidates: [] }, diversity_trace: { candidates: [], filtered: [], policy: { source_floors: [], max_source_share: 1 } } };
}
function clientFor(response) {
  const client = new BabbleFrontendClient("https://babble.example");
  client.catalog = async () => ({ methods: [] });
  client.rpc = async method => {
    assert.equal(method, "babble.discovery.candidates.v1");
    return { discovery: response };
  };
  return client;
}

test("discovery matches temporal scores by ranked Object ID, retaining exact provenance and all subcomponents", async () => {
  for (const provider of ["babble-python", "babble-rust"]) {
    const response = discovery(evaluation(provider));
    response.objects.push(object("unranked"));
    response.temporal.scores.push(score("unranked"));
    const result = await clientFor(response).loadFeed("");
    assert.equal(result.cards.length, 3);
    for (const card of result.cards.slice(0, 2)) {
      assert.deepEqual(JSON.parse(JSON.stringify(card.temporal)), { provider: response.temporal.provider,
        reference_time: response.temporal.reference_time, score: response.temporal.scores.find(item => item.object_id === card.id) });
    }
    assert.equal(result.cards[2].temporal, undefined);
  }
});

test("local personalization keeps public temporal evaluation unchanged and does not send private preferences", async () => {
  const response = discovery();
  response.objects[1].author = "hidden-author";
  const client = clientFor(response), request = client.rpc;
  client.rpc = (method, input) => {
    assert.ok(!JSON.stringify(input).includes("hidden-author"));
    return request(method, input);
  };
  const result = await client.loadFeed("", "balanced", { hidden_authors: ["hidden-author"], model_revision: "private-revision" });
  assert.equal(result.cards.length, 1);
  assert.equal(result.cards[0].source, "local");
  assert.equal(result.cards[0].temporal.score, response.temporal.scores[2]);
  assert.equal(result.cards[0].temporal.provider, response.temporal.provider);
  assert.equal(result.cards[0].temporal.reference_time, response.temporal.reference_time);
});

test("missing evaluation or per-object score never synthesizes temporal provenance from ranking signals", async () => {
  for (const temporal of [null, { ...evaluation(), scores: [] }]) {
    const response = discovery(temporal);
    for (const model of [null, {}]) {
      const result = await clientFor(response).loadFeed("", "balanced", model);
      assert.ok(result.cards.every(card => card.temporal === undefined));
    }
  }
  const response = discovery();
  delete response.temporal;
  assert.ok((await clientFor(response).loadFeed("")).cards.every(card => card.temporal === undefined));
});

test("all Settings model fields stay outside every discovery RPC payload", async () => {
  const client = clientFor(discovery()), request = client.rpc, calls = [];
  client.rpc = (method, input) => { calls.push({ method, input }); return request(method, input); };
  await client.loadFeed("", "research", {
    model_revision: "private-model-marker", interests: ["private-interest-marker"], expertise: ["private-expertise-marker"],
    hidden_terms: ["private-hidden-marker"], muted_terms: ["private-muted-marker"], hidden_authors: ["private-author-marker"],
    creator_affinity: { "private-creator-marker": .7 }, seen_objects: { "private-seen-marker": 3 },
    novelty_tolerance: .2, exploration_preference: .4, evidence_preference: .6, contradiction_tolerance: .8,
  });
  assert.ok(calls.length > 0);
  assert.ok(!JSON.stringify(calls).includes("private-"));
  for (const { input } of calls) {
    for (const field of ["interests", "expertise", "hidden_terms", "muted_terms", "hidden_authors", "creator_affinity", "seen_objects",
      "model_revision", "novelty_tolerance", "exploration_preference", "evidence_preference", "contradiction_tolerance"])
      assert.ok(!Object.hasOwn(input, field), `Private field sent to discovery: ${field}`);
  }
});

test("direct, published, following, profile and search cards have no temporal evaluation, even with reused input", async () => {
  const client = clientFor(discovery());
  const evaluated = (await client.loadFeed("")).cards[0];
  for (const source of ["object", "published", "following", "profile", "search"]) {
    const card = await client.objectToCard({ object: evaluated.object, score: null, signals: null,
      reasons: [], source, temporal: evaluated.temporal });
    assert.equal(card.temporal, undefined);
  }
  const mismatched = await client.objectToCard({ object: object("different"), score: 0.7, signals,
    reasons: [], source: "discovery", temporal: evaluated.temporal });
  assert.equal(mismatched.temporal, undefined);
});

test("empty discovery stays empty without raw-search fallback and retains policy and privacy context", async () => {
  for (const query of ["", "  ", "absent-needle"]) {
    for (const lens of ["balanced", "research", "weird"]) {
      for (const localModel of [null, { hidden_authors: ["private-author"], model_revision: "private-only" }]) {
        const response = { ...discovery(), objects: [], ranked: [], temporal: { ...evaluation(), scores: [] },
          diversity_trace: { candidates: [], filtered: [], policy: {
            source_floors: [{ source: "Evidence", minimum: 1 }], max_source_share: 0.55 } } };
        const client = clientFor(response);
        const calls = [];
        client.rpc = async (method, input) => {
          calls.push({ method, input });
          assert.equal(method, "babble.discovery.candidates.v1", "empty admission must not bypass discovery");
          return { discovery: response };
        };
        const result = await client.loadFeed(query, lens, localModel);
        assert.equal(result.cards.length, 0);
        assert.equal(result.source, "discovery");
        assert.equal(result.diversity.maxSourceShare, 0.55);
        assert.deepEqual([...result.diversity.floors], ["Evidence 1"]);
        assert.equal(result.personalization.boundary, localModel ? "local_only" : "none");
        assert.equal(calls.length, 1);
        assert.equal(calls[0].input.search, query.trim() || null);
        assert.ok(!JSON.stringify(calls).includes("private-author"));
        assert.ok(!JSON.stringify(calls).includes("private-only"));
      }
    }
  }
});

test("discovery failure remains an error rather than falling back to raw search", async () => {
  const client = clientFor(discovery());
  client.rpc = async method => {
    assert.equal(method, "babble.discovery.candidates.v1");
    throw new Error("provider unavailable");
  };
  await assert.rejects(client.loadFeed("needle"), /provider unavailable/);
});

// DOM port for production card rendering; browser/layout verification belongs to the live Aegis suite.
class Element {
  constructor(tag, document) {
    Object.assign(this, { tag, document, children: [], dataset: {}, attrs: {}, events: {}, hidden: false,
      className: "", text: "", scrollTop: 0, style: { setProperty() {} } });
  }
  get childNodes() { return this.children; }
  append(...nodes) { for (const node of nodes) { node.remove(); node.parentElement = this; this.children.push(node); } }
  prepend(node) { node.remove(); node.parentElement = this; this.children.unshift(node); }
  insertBefore(node, reference) { node.remove(); node.parentElement = this; const index = this.children.indexOf(reference); this.children.splice(index < 0 ? this.children.length : index, 0, node); }
  remove() { if (this.parentElement) this.parentElement.children = this.parentElement.children.filter(node => node !== this); this.parentElement = null; }
  replaceChildren(...nodes) { for (const child of [...this.children]) child.remove(); this.text = ""; this.append(...nodes); }
  set textContent(value) { this.replaceChildren(); this.text = String(value); }
  get textContent() { return this.text + this.children.map(node => node.textContent).join(""); }
  set innerHTML(_) { throw new Error("Unsafe HTML insertion"); }
  get ariaExpanded() { return this.getAttribute("aria-expanded"); }
  set ariaExpanded(value) { this.setAttribute("aria-expanded", value); }
  setAttribute(key, value) { this.attrs[key] = String(value); }
  getAttribute(key) { return this.attrs[key] ?? null; }
  matches(selector) {
    if (selector === '[data-quotes-root]') return this.dataset.quotesRoot !== undefined;
    const match = selector.match(/^(\.[\w-]+|[\w-]+)(?:\[data-kind="([\w-]+)"\])?$/);
    assert.ok(match, `Unsupported selector: ${selector}`);
    return (match[1][0] === "." ? this.className.split(" ").includes(match[1].slice(1)) : this.tag === match[1])
      && (!match[2] || this.dataset.kind === match[2]);
  }
  querySelectorAll(selector) { return this.children.flatMap(node => [...(node.matches(selector) ? [node] : []), ...node.querySelectorAll(selector)]); }
  querySelector(selector) { return this.querySelectorAll(selector)[0] ?? null; }
  closest(selector) { return this.matches(selector) ? this : this.parentElement?.closest(selector) ?? null; }
  contains(node) { return this === node || this.children.some(child => child.contains(node)); }
  addEventListener(type, callback) { (this.events[type] ??= []).push(callback); }
  emit(type, values = {}) { for (const callback of this.events[type] ?? []) callback({ target: this, ...values }); }
  focus() { this.document.activeElement = this; }
}
function deckHarness() {
  const document = new Element("document");
  document.createElement = tag => new Element(tag, document);
  const deck = document.createElement("main");
  document.append(deck);
  const timers = [];
  let views = 0;
  const { renderDeck } = load("cards", name => name === "lucide" ? { createElement: () => document.createElement("svg") }
    : name === "./media-player" ? { pauseMedia: () => {} }
    : { captureReading: node => ({ scrollTop: node.scrollTop }), restoreReading: (node, state) => { node.scrollTop = state.scrollTop; } },
  { document, Node: Element, requestAnimationFrame: callback => callback(), window: { setTimeout: callback => timers.push(callback) } });
  const quotes = new Map();
  return { document, deck, render: cards => renderDeck(deck, cards, 0, { view: () => { views++; return document.createElement("aside"); } },
    { view: id => { if (!quotes.has(id)) { const panel = document.createElement("section"); panel.dataset.quotesRoot = id; quotes.set(id, panel); } return quotes.get(id); } }),
    evictQuote: id => quotes.delete(id),
    get views() { return views; }, flush: () => timers.splice(0).forEach(callback => callback()) };
}
function metricValues(section) {
  return Object.fromEntries(section.querySelectorAll("div").filter(node => node.children[0]?.tag === "span")
    .map(node => node.children.map(child => child.textContent)));
}

test("production analytics and inspector render heuristic values, exact time, provider and model without probability claims", async () => {
  const card = (await clientFor(discovery()).loadFeed("")).cards[0];
  const h = deckHarness();
  h.render([card]);
  const article = h.deck.querySelector("article");
  const panel = article.querySelector('.popover-panel[data-kind="analytics"]');
  const trigger = panel.parentElement.querySelector("button");
  assert.equal(panel.hidden, true);
  assert.equal(trigger.ariaLabel, "Show analytics");
  assert.equal(trigger.getAttribute("aria-controls"), panel.id);
  trigger.emit("click");
  assert.equal(trigger.ariaExpanded, "true");
  assert.equal(panel.hidden, false);
  const temporal = panel.querySelector(".temporal-metrics");
  assert.equal(temporal.getAttribute("aria-label"), "Temporal heuristics from public graph activity");
  assert.deepEqual(metricValues(temporal), { "Temporal heuristic": "0.457", "Public activity": "0.120" });
  assert.equal(temporal.querySelector("time"), null);
  assert.doesNotMatch(temporal.textContent, /%|probability|quality|views|dwell|scroll/i);
  const inspector = article.querySelector(".post-inspection");
  assert.deepEqual(metricValues(inspector.querySelector(".temporal-metrics")), {
    "Temporal heuristic": "0.457", "Public activity": "0.120", Recency: "0.820", "Age (hours)": "12.25",
    "Decay rate": "0.100", "Time sensitivity": "0.680", "Temporal provider": "babble-python",
    "Temporal model": "temporal-v1", "Model version": "1", Evaluated: card.temporal.reference_time,
  });
  assert.equal(inspector.querySelector("time").dateTime, card.temporal.reference_time);
  assert.deepEqual(JSON.parse(inspector.querySelector("pre").textContent).temporal, JSON.parse(JSON.stringify(card.temporal)));
  panel.focus();
  h.document.emit("keydown", { key: "Escape" });
  h.flush();
  assert.equal(panel.hidden, true);
  assert.equal(h.document.activeElement, trigger);
});

test("reused cards replace stale scores, time and provider while preserving open inspector, focus, conversation and reading", async () => {
  const card = (await clientFor(discovery()).loadFeed("")).cards[0];
  const h = deckHarness();
  h.render([card]);
  const article = h.deck.querySelector("article"), details = article.querySelector("details");
  const panel = article.querySelector('.popover-panel[data-kind="analytics"]');
  const trigger = panel.parentElement.querySelector("button");
  article.scrollTop = 200;
  details.open = true;
  trigger.emit("click");
  trigger.focus();
  const temporal = { provider: { provider: "babble-rust", model: "temporal-v1", version: "2" },
    reference_time: "2026-10-01T00:00:00Z", score: { ...score(card.id, 0), recency: 0, age_hours: 0, engagement_velocity: 0 } };
  h.render([{ ...card, temporal }]);
  assert.equal(h.deck.querySelector("article"), article);
  assert.equal(article.scrollTop, 200);
  assert.equal(details.open, true);
  assert.equal(panel.hidden, false);
  assert.equal(h.document.activeElement, trigger);
  assert.equal(h.views, 1);
  assert.equal(metricValues(panel)["Temporal heuristic"], "0.000");
  assert.equal(metricValues(panel)["Public activity"], "0.000");
  assert.equal(metricValues(details)["Temporal provider"], "babble-rust");
  assert.equal(metricValues(details).Evaluated, temporal.reference_time);
  assert.deepEqual(JSON.parse(article.querySelector("pre").textContent).temporal, temporal);
  const plain = await clientFor(discovery()).objectToCard({ object: card.object, score: null, source: "profile", signals: null, reasons: [] });
  h.render([plain]);
  assert.equal(article.querySelector(".temporal-metrics"), null);
  assert.equal(JSON.parse(article.querySelector("pre").textContent).temporal, undefined);
  assert.doesNotMatch(panel.textContent, /temporal|babble-rust|2026-10-01/i);
  h.render([card]);
  assert.equal(metricValues(details)["Temporal provider"], "babble-python");
  assert.equal(h.views, 1);
});

test("provider labels render as text and absence renders no temporal section", async () => {
  const h = deckHarness(), client = clientFor(discovery());
  const card = (await client.loadFeed("")).cards[0];
  const provider = { provider: '<img src=x onerror="bad()">', model: "<script>bad()</script>", version: "1" };
  h.render([{ ...card, temporal: { ...card.temporal, provider } }]);
  assert.equal(h.deck.querySelector("img"), null);
  assert.equal(h.deck.querySelector("script"), null);
  assert.match(h.deck.textContent, /<script>bad\(\)<\/script>/);
  h.render([{ ...card, temporal: undefined }]);
  assert.equal(h.deck.querySelector(".temporal-metrics"), null);
});

test("a reused deck card reconnects an evicted quote panel without duplicating context or replacing its conversation", async () => {
  const card = (await clientFor(discovery()).loadFeed("")).cards[0];
  const h = deckHarness(); h.render([card]);
  const article = h.deck.querySelector("article");
  const previous = article.querySelector("[data-quotes-root]");
  const conversation = article.querySelector("aside");
  h.render([card]);
  assert.equal(article.querySelector("[data-quotes-root]"), previous);
  h.evictQuote(card.id); h.render([card]);
  const replacement = article.querySelector("[data-quotes-root]");
  assert.notEqual(replacement, previous);
  assert.equal(article.children[1], replacement);
  assert.equal(article.querySelector("aside"), conversation);
  assert.equal(article.querySelectorAll("[data-quotes-root]").length, 1);
  assert.equal(previous.parentElement, null);
  assert.equal(h.views, 1);
});

test("production feed generation discards late temporal results after another search or a Following switch", async () => {
  const source = ts.createSourceFile("main.ts", readFileSync(new URL("../src/app/main.ts", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true);
  const loadFeed = source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === "loadFeed");
  const code = ts.transpileModule(loadFeed.getText(source), { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText;
  const card = (await clientFor(discovery()).loadFeed("")).cards[0];
  for (const mode of ["search", "following"]) {
    const noop = () => {}, pending = [];
    const node = () => ({ hidden: false, dataset: {}, textContent: "", querySelector: () => null, replaceChildren: noop });
    const context = { followingDirty: false, profiles: { close: noop }, objectVisits: { clear: noop }, quotes: { clear: noop, refresh: noop }, loadSequence: 0,
      updateEmptyFeed: noop,
      safetyControls: { ensure: async () => null },
      ensureFeedSafety: async () => {},
      followingFeed: { clear: noop, start: async () => {} }, activeLens: "balanced", accounts: { current: null },
      followingToolbar: node(), followingMore: node(), followingSignIn: node(), followingStatus: node(),
      cards: [], currentIndex: 0, deck: node(), empty: node(), error: node(), feedSummary: node(),
      catalogLabel: node(), sourceLabel: node(), localLabel: node(), diversityLabel: node(), lensLabel: node(),
      localPersonalization: null, feedDiversity: null, closeSurface: noop, closeJudgments: noop,
      syncSearchUrl: noop, setStatus: noop, lensName: String, localModel: () => null,
      personalizationLabel: String, diversityLabelText: String, syncSettingsStatus: noop,
      render: noop, showError: error => assert.fail(error), feedErrorMessage: String, reactions: { select: noop },
      client: { loadFeed: () => new Promise(resolve => pending.push(resolve)) } };
    vm.runInNewContext(code, context);
    const stale = context.loadFeed("old");
    await new Promise(resolve => setImmediate(resolve));
    const result = cards => ({ cards, catalogMethods: 1, source: "discovery", personalization: {}, diversity: {} });
    if (mode === "following") {
      context.activeLens = "following";
      await context.loadFeed("");
    } else {
      const current = context.loadFeed("new");
      await new Promise(resolve => setImmediate(resolve));
      pending[1](result([{ ...card, temporal: undefined }]));
      await current;
    }
    pending[0](result([card]));
    await stale;
    assert.equal(context.cards[0]?.temporal, undefined);
    assert.equal(context.cards.length, mode === "following" ? 0 : 1);
  }
});
