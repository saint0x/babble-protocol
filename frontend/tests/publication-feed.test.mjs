import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const source = ts.createSourceFile("main.ts", readFileSync(new URL("../src/app/main.ts", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true);
const load = source.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === "loadFeed");
const code = ts.transpileModule(load.getText(source), { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None } }).outputText;
const noop = () => {};
function harness(ranked = []) {
  const node = () => ({ hidden: false, dataset: {}, textContent: "", querySelector: () => null, replaceChildren: noop });
  const context = { followingDirty: false, profiles: { close: noop }, objectVisits: { clear: noop }, quotes: { clear: noop, refresh: noop }, loadSequence: 0,
    updateEmptyFeed: noop,
    safetyControls: { ensure: async () => null },
    ensureFeedSafety: async () => {},
    followingFeed: { clear: noop, start: async () => {} }, activeLens: "weird", accounts: { current: null },
    followingToolbar: node(), followingMore: node(), followingSignIn: node(), followingStatus: node(),
    cards: [], currentIndex: 0, deck: node(), empty: node(), error: node(), feedSummary: node(),
    catalogLabel: node(), sourceLabel: node(), localLabel: node(), diversityLabel: node(), lensLabel: node(),
    localPersonalization: null, feedDiversity: null, closeSurface: noop, closeJudgments: noop,
    syncSearchUrl: noop, setStatus: noop, lensName: (name) => name, localModel: () => null,
    personalizationLabel: () => "local", diversityLabelText: () => "soft", syncSettingsStatus: noop,
    render: noop, showError: noop, feedErrorMessage: String, reactions: { select: noop },
    client: { loadFeed: async () => ({ cards: ranked, catalogMethods: 73, source: "discovery", personalization: {}, diversity: {} }) },
  };
  vm.runInNewContext(code, context);
  return context;
}

test("confirmed publication opens even when absent from the ranked page, without invented provenance", async () => {
  const ranked = { id: "ranked", score: 0.8 };
  const confirmed = { id: "published", score: null, rankingProvider: null, source: "published" };
  const h = harness([ranked]);
  await h.loadFeed("", confirmed.id, () => true, confirmed);
  assert.equal(h.cards.length, 2);
  assert.equal(h.cards[h.currentIndex], confirmed);
  assert.equal(h.cards[1], ranked);
  assert.equal(h.cards[0].score, null);
  assert.equal(h.cards[0].rankingProvider, null);
});

test("already-ranked publication keeps its real score and is not duplicated", async () => {
  const published = { id: "published", score: 0.8 };
  const h = harness([{ id: "other" }, published]);
  await h.loadFeed("", published.id, () => true, { id: published.id, score: null });
  assert.equal(h.cards.length, 2);
  assert.equal(h.cards[h.currentIndex], published);
});

test("stale publication completion and private Following never inject a public card", async () => {
  const h = harness([{ id: "ranked" }]);
  const original = h.cards;
  await h.loadFeed("", "published", () => false, { id: "published" });
  assert.equal(h.cards, original);
  h.activeLens = "following";
  await h.loadFeed("", "published", () => true, { id: "published" });
  assert.equal(h.cards.length, 0);
});
