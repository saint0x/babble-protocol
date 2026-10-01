import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import { createPersonalizationFilter, summarizeDiscoveryObject } from "@babble-protocol/sdk";

const source = ts.createSourceFile("main.ts", readFileSync(new URL("../src/app/main.ts", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true);
function functions(names, context) {
  const selected = source.statements.filter(node => ts.isFunctionDeclaration(node) && names.includes(node.name?.text));
  assert.equal(selected.length, names.length);
  vm.runInNewContext(ts.transpileModule(selected.map(node => node.getText(source)).join("\n"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022 },
  }).outputText, context);
  return context;
}
const author = `id_${"a".repeat(64)}`, other = `id_${"b".repeat(64)}`;
const card = (id, payload, owner = author) => ({ id, author: owner, object: { id, author: owner, kind: "media", payload } });
const preferences = overrides => ({ hiddenAuthors: [], hiddenTerms: [], mutedTerms: [], ...overrides });

test("main uses the same explicit Unicode and full-caption filter for ranked refresh and chronological Following", () => {
  const h = functions(["filterFollowingCards"], { createPersonalizationFilter, summarizeDiscoveryObject });
  const posts = [card("a", { title: "Holiday", description: "Privateword in the full caption" }),
    card("b", { text: "CAF\u00c9", alt: "A drawing" }), card("c", { text: "I agree" }),
    card("d", { text: "Wholeword not partword" }), card("e", { text: "Allowed" }, other)];
  const result = h.filterFollowingCards(posts, preferences({ mutedTerms: ["privateword", "cafe\u0301", "i", "part"], hiddenAuthors: [other] }));
  assert.deepEqual(Array.from(result, post => post.id), ["d"]);
});

test("saving filters removes hidden loaded cards immediately and retires suspended profile, Object and Surface navigation", () => {
  const calls = [];
  const h = functions(["refreshLocalFeed", "filterFollowingCards"], {
    createPersonalizationFilter, summarizeDiscoveryObject,
    closeSurface: () => calls.push("surface"), closeJudgments: () => calls.push("judgments"),
    profiles: { close: () => calls.push("profile") }, objectVisits: { clear: () => calls.push("visits") },
    profileReturn: { cards: [card("old-profile", {}, author)] }, profileBack: { hidden: false },
    cards: [card("hidden", {}), card("visible", {}, other)], currentIndex: 0,
    loadLocalPreferences: () => preferences({ hiddenAuthors: [author] }), searchInput: { value: "query" },
    render: () => calls.push("render"), loadFeed: (...args) => { calls.push(args); return new Promise(() => {}); },
  });
  h.refreshLocalFeed();
  assert.deepEqual(Array.from(h.cards, post => post.id), ["visible"]);
  assert.equal(h.currentIndex, 0); assert.equal(h.profileReturn, null); assert.equal(h.profileBack.hidden, true);
  assert.deepEqual(calls, ["surface", "judgments", "profile", "visits", "render", ["query", "visible"]]);
});

test("filter refresh preserves the currently visible allowed Object even when preceding cards are removed", () => {
  const h = functions(["refreshLocalFeed", "filterFollowingCards"], {
    createPersonalizationFilter, summarizeDiscoveryObject, closeSurface() {}, closeJudgments() {},
    profiles: { close() {} }, objectVisits: { clear() {} }, profileReturn: null, profileBack: { hidden: true },
    cards: [card("hidden", {}), card("keep", {}, other), card("last", {}, other)], currentIndex: 1,
    loadLocalPreferences: () => preferences({ hiddenAuthors: [author] }), searchInput: { value: "" },
    render() {}, loadFeed(_query, id) { assert.equal(id, "keep"); return Promise.resolve(); },
  });
  h.refreshLocalFeed(); assert.equal(h.cards[h.currentIndex].id, "keep");
});

test("failed hide reports storage error without claiming success or offering Undo", () => {
  const messages = [];
  const h = functions(["hideFeedAuthor"], { cards: [card("post", {})],
    feedPreferences: { hideAuthor() { throw Error("Storage full"); } },
    setAuthorStatus: (...args) => messages.push(args), errorMessage: error => error.message,
    document: { createElement() { assert.fail("No Undo after failed save"); } },
  });
  h.hideFeedAuthor("post"); assert.deepEqual(messages, [["Storage full", "error"]]);
});

test("successful hide wires a real one-shot Undo and catches account or storage failures", () => {
  for (const fail of [false, true]) {
    const callbacks = [], messages = [], children = []; let restored = 0, focused = false;
    const button = { dataset: {}, append() {}, addEventListener: (_name, handler) => callbacks.push(handler), focus: () => { focused = true; } };
    const h = functions(["hideFeedAuthor"], { cards: [card("post", {})],
      feedPreferences: { hideAuthor: () => () => { restored++; if (fail) throw Error("Account changed"); } },
      document: { createElement: () => button, querySelector: () => ({ append: node => children.push(node) }) },
      createElement: () => ({}), Undo2: {}, errorMessage: error => error.message,
      setAuthorStatus: (...args) => messages.push(args),
    });
    h.hideFeedAuthor("post"); assert.equal(focused, true); assert.equal(children[0], button);
    callbacks[0](); assert.equal(restored, 1);
    assert.deepEqual(messages[1], fail ? ["Account changed", "error"] : ["Author restored to your feed.", "ready"]);
  }
});

test("filtered empty state offers Feed controls and unfiltered search does not claim filtering", () => {
  const nodes = new Map();
  const h = functions(["updateEmptyFeed"], { activeLens: "following", searchInput: { value: "" },
    empty: { querySelector(selector) { if (!nodes.has(selector)) nodes.set(selector, {}); return nodes.get(selector); } },
  });
  h.updateEmptyFeed(true);
  assert.equal(nodes.get('[data-open-feed-preferences]').hidden, false);
  assert.equal(nodes.get('.empty-title').textContent, "No posts match your feed preferences");
  h.updateEmptyFeed(false);
  assert.equal(nodes.get('[data-open-feed-preferences]').hidden, true);
  assert.equal(nodes.get('.empty-title').textContent, "No Following posts");
  h.activeLens = "balanced"; h.updateEmptyFeed(false);
  assert.equal(nodes.get('.empty-title').textContent, "No posts found");
});
