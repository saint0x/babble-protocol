import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { setImmediate } from "node:timers/promises";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import * as sdk from "@babble-protocol/sdk";

class Element {
  children = []; attributes = {}; events = {}; textContent = ""; open = false;
  constructor(tag) { this.tag = tag; }
  append(...children) { this.children.push(...children); }
  setAttribute(name, value) { this.attributes[name] = value; }
  addEventListener(type, callback) { this.events[type] = callback; }
  emit(type) { this.events[type]?.(); }
}
function load(name, globals = {}, require = () => ({})) {
  const context = { exports: {}, require, URL, AbortController, AbortSignal, TextEncoder, TextDecoder,
    structuredClone, console, crypto: globalThis.crypto, ...globals };
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, context);
  return context.exports;
}
const { agreementSummary, judgmentInputView } = load("judgment-view", {
  document: { createElement: tag => new Element(tag) },
});
const agreement = (overrides = {}) => ({ kind: "source_agreement", validation_count: 2,
  term_agreement: 0.425, fact_agreement: 0.12, reliability_score: 0.7, temporal_weight: 1,
  consensus_score: 0.99, ...overrides });

test("source comparison keeps four independent observations and their limitations, not a truth score", () => {
  const section = agreementSummary(agreement());
  assert.equal(section.attributes["aria-label"], "Uncalibrated source comparison");
  assert.match(section.children[0].textContent, /not a truth assessment/);
  assert.match(section.children[0].textContent, /unverified priors/);
  const rows = section.children[1].children;
  assert.equal(rows.length, 4);
  assert.deepEqual(rows.map(row => row.children.map(child => child.textContent)), [
    ["Term overlap", "43%"], ["Sentence overlap", "12%"], ["Source prior", "70%"], ["Age weight", "100%"],
  ]);
});

test("missing sources never masquerade as zero-percent factual certainty", () => {
  const section = agreementSummary(agreement({ validation_count: 0 }));
  assert.equal(section.children.length, 1);
  assert.equal(section.children[0].textContent, "No eligible public sources.");
  for (const input of [null, [], "text", {}, { kind: "spam" }]) assert.equal(agreementSummary(input), null);
  for (const invalid of [NaN, Infinity, -0.1, 1.1, "0.5", null]) {
    assert.equal(agreementSummary(agreement({ term_agreement: invalid })).children[1].children.length, 3);
  }
});

test("input disclosure is lazy, coalesces pending reads, preserves literal text, and caches successful reads", async () => {
  let reads = 0, resolve;
  const request = { state: { text: '<img src=x onerror="attack()">' } };
  const view = judgmentInputView({ judgmentInput(id) {
    assert.equal(id, "jud_test"); reads++;
    return new Promise(done => { resolve = done; });
  } }, "jud_test");
  assert.equal(reads, 0);
  view.open = true; view.emit("toggle"); view.emit("toggle");
  assert.equal(reads, 1);
  assert.equal(view.attributes["aria-busy"], "true");
  resolve({ request }); await setImmediate();
  assert.equal(view.attributes["aria-busy"], "false");
  assert.equal(view.children[2].textContent, JSON.stringify(request, null, 2));
  assert.equal(view.children[2].children.length, 0);
  view.open = false; view.emit("toggle"); view.open = true; view.emit("toggle");
  assert.equal(reads, 1);
});

test("failure exposes a working retry, while historical absence is an honest terminal state", async () => {
  let reads = 0;
  const view = judgmentInputView({ async judgmentInput() {
    if (++reads === 1) throw "transport failed";
    return null;
  } }, "jud_history");
  view.open = true; view.emit("toggle"); await setImmediate();
  const [summary, status, data, retry] = view.children;
  assert.equal(summary.textContent, "Evaluation inputs");
  assert.equal(status.attributes.role, "status");
  assert.equal(retry.hidden, false);
  assert.equal(view.attributes["aria-busy"], "false");
  retry.emit("click"); await setImmediate();
  assert.equal(reads, 2);
  assert.equal(retry.hidden, true);
  assert.equal(data.textContent, "");
  assert.match(status.textContent, /unavailable for this historical evaluation/);
  view.emit("toggle"); assert.equal(reads, 2);
});

test("input fetch binds returned IDs, handles legacy absence, and refuses failed requests", async () => {
  let response = { judgment: { id: "jud_test" }, input: { judgment_id: "jud_test" } };
  let status = 200, requested;
  const profiles = load("profile-response");
  const invocations = load("invocations", {}, () => sdk);
  const { BabbleFrontendClient } = load("protocol", { fetch: async (url, options) => {
    requested = { url, options };
    return Response.json(response, { status });
  } }, name => name === "./invocations" ? invocations : name === "@babble-protocol/sdk" ? sdk : profiles);
  const client = new BabbleFrontendClient("https://babble.example");
  assert.equal((await client.judgmentInput("jud_test")).judgment_id, "jud_test");
  assert.equal(requested.url.href, "https://babble.example/judgments/jud_test");
  assert.ok(requested.options.signal instanceof AbortSignal);
  for (const bad of [
    { judgment: { id: "jud_other" }, input: null },
    { judgment: { id: "jud_test" }, input: { judgment_id: "jud_other" } },
  ]) {
    response = bad;
    await assert.rejects(client.judgmentInput("jud_test"), /does not match/);
  }
  response = { judgment: { id: "jud_test" } };
  assert.equal(await client.judgmentInput("jud_test"), null);
  status = 503;
  await assert.rejects(client.judgmentInput("jud_test"), /503/);
});

function panelHarness() {
  const source = ts.createSourceFile("main.ts", readFileSync(new URL("../src/app/main.ts", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true);
  const names = ["openJudgments", "closeJudgments", "evaluateActiveJudgment"];
  const functions = source.statements.filter(node => ts.isFunctionDeclaration(node) && names.includes(node.name?.text));
  assert.equal(functions.length, 3);
  const code = ts.transpileModule(functions.map(node => node.getText(source)).join("\n"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022 },
  }).outputText;
  const pending = [], evaluations = [], rendered = [], statuses = [];
  const context = { activeJudgmentObjectId: null, judgmentRequestSequence: 0, cards: [],
    judgmentPanel: {}, judgmentTitle: {}, judgmentSubmit: { disabled: false },
    judgmentList: { replaceChildren() {} }, accounts: { current: {} },
    setAnimatedVisibility: (panel, visible) => { panel.visible = visible; },
    setJudgmentStatus: (...status) => statuses.push(status), renderJudgments: value => rendered.push(value),
    judgmentMessage: String, compactId: String, errorMessage: String, definitionLabel: String,
    selectedJudgmentDefinition: () => "babble.judgment.source_agreement.v1", judgmentParameters: () => ({}),
    syncJudgmentParameterInputs() {},
    client: {
      objectJudgments: id => new Promise((resolve, reject) => pending.push({ id, resolve, reject })),
      evaluateObject: id => new Promise((resolve, reject) => evaluations.push({ id, resolve, reject })),
    },
  };
  vm.runInNewContext(code, context);
  return { context, pending, evaluations, rendered, statuses };
}

test("same-object refresh ignores out-of-order lists and errors", async () => {
  for (const reject of [false, true]) {
    const h = panelHarness();
    const old = h.context.openJudgments("object");
    const current = h.context.openJudgments("object");
    h.pending[1].resolve({ judgments: ["new"] }); await current;
    if (reject) h.pending[0].reject("old error"); else h.pending[0].resolve({ judgments: ["old"] });
    await old;
    assert.deepEqual(h.rendered, [["new"]]);
    assert.equal(h.statuses.at(-1)[1], "ready");
  }
});

test("closing or switching Objects during evaluation cannot reopen a stale inspector", async () => {
  for (const mode of ["close", "switch", "error"]) {
    const h = panelHarness();
    h.context.activeJudgmentObjectId = "old";
    const evaluation = h.context.evaluateActiveJudgment();
    assert.equal(h.context.judgmentSubmit.disabled, true);
    if (mode === "switch") {
      const next = h.context.openJudgments("new");
      h.pending[0].resolve({ judgments: ["new"] }); await next;
    } else h.context.closeJudgments();
    if (mode === "error") h.evaluations[0].reject("stale failure"); else h.evaluations[0].resolve({});
    await evaluation;
    assert.equal(h.context.activeJudgmentObjectId, mode === "switch" ? "new" : null);
    assert.equal(h.pending.length, mode === "switch" ? 1 : 0);
    assert.equal(h.context.judgmentSubmit.disabled, false);
    assert.equal(h.statuses.at(-1)[1], "ready");
  }
});

test("current evaluation reloads persisted history and restores the submit control", async () => {
  const h = panelHarness();
  h.context.activeJudgmentObjectId = "current";
  const evaluation = h.context.evaluateActiveJudgment();
  h.evaluations[0].resolve({}); await setImmediate();
  assert.equal(h.pending[0].id, "current");
  h.pending[0].resolve({ judgments: ["persisted"] }); await evaluation;
  assert.deepEqual(h.rendered, [["persisted"]]);
  assert.equal(h.context.judgmentSubmit.disabled, false);
});
