import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import { createPersonalizationFilter, summarizeDiscoveryObject } from "@babel-protocol/sdk";

const source = ts.createSourceFile("main.ts", readFileSync(new URL("../src/app/main.ts", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true);
function functions(names, context) {
  const selected = source.statements.filter(node => ts.isFunctionDeclaration(node) && names.includes(node.name?.text));
  assert.equal(selected.length, names.length, "Exercise each production integration function");
  vm.runInNewContext(ts.transpileModule(selected.map(node => node.getText(source)).join("\n"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022 },
  }).outputText, context);
  return context;
}
const owner = `id_${"a".repeat(64)}`, muted = `id_${"b".repeat(64)}`, blocked = `id_${"c".repeat(64)}`;
const card = (id, author) => ({ id, author, object: { id, author, kind: "text", payload: { text: "A post" } } });
const snapshot = (revision = 1) => ({ author_id: owner, revision, entries: [
  { state: { target_id: muted, blocked: false, muted: true } },
  { state: { target_id: blocked, blocked: true, muted: false } },
] });

test("account safety stays in the local model without changing device preferences", () => {
  const preferences = { interests: [], expertise: [], mutedTerms: [], hiddenTerms: [], hiddenAuthors: [owner], creatorAffinity: {} };
  const h = functions(["localModel", "safetyHiddenAuthors"], {
    safetyControls: { snapshot: snapshot() }, accounts: { current: { identity: { id: owner } } },
    loadLocalPreferences: () => preferences, queryInterests: () => [], loadSeenObjects: () => [],
    lensDefaults: () => ({ noveltyTolerance: 0.5, explorationPreference: 0.2, evidencePreference: 0.5, contradictionTolerance: 0.5 }),
  });
  assert.deepEqual(Array.from(h.localModel("", "balanced").hidden_authors), [owner, muted, blocked]);
  assert.deepEqual(preferences.hiddenAuthors, [owner]);
  h.accounts.current = null;
  assert.deepEqual(Array.from(h.localModel("", "balanced").hidden_authors), [owner], "old account snapshot cannot affect a guest");
});

test("cached feed restoration removes blocked and muted authors and preserves the selected survivor", () => {
  const h = functions(["applyFeedSafety"], {
    safetyControls: { hidden: author => [muted, blocked].includes(author) },
    accounts: { current: null },
    cards: [card("muted", muted), card("keep", owner), card("blocked", blocked)], currentIndex: 1,
    sourceLabel: { textContent: "discovery" },
  });
  h.applyFeedSafety();
  assert.deepEqual(Array.from(h.cards, item => item.id), ["keep"]);
  assert.equal(h.currentIndex, 0);
});

for (const label of ["public profile", "Shared post", "Object"]) {
  test(`explicit ${label} is not silently treated as a recommendation`, () => {
    const items = [card("requested", muted)];
    const h = functions(["applyFeedSafety"], {
      safetyControls: { hidden: () => true }, cards: items, currentIndex: 0, sourceLabel: { textContent: label },
    });
    h.applyFeedSafety(); assert.equal(h.cards, items);
  });
}

test("Following applies account safety alongside Unicode device filters", () => {
  const h = functions(["filterFollowingCards"], { createPersonalizationFilter, summarizeDiscoveryObject });
  const result = h.filterFollowingCards([card("mute", muted), card("block", blocked), card("keep", owner)],
    { hiddenAuthors: [muted], hiddenTerms: [], mutedTerms: [] }, [blocked]);
  assert.deepEqual(Array.from(result, item => item.id), ["keep"]);
});

test("snapshot initialization does not reload recursively; subsequent changes invalidate suspended feeds", () => {
  let refreshed = 0, profiles = 0, conversations = 0, quotes = 0, invalidated = 0;
  const h = functions(["safetyChanged"], {
    safetyRevision: null, safetyUnavailable: false, accounts: { current: { identity: { id: owner } } },
    syncProfileSafety: () => { profiles++; }, refreshLocalFeed: () => { refreshed++; },
    conversations: { clear: () => conversations++ }, quotes: { clear: () => quotes++ },
    invalidateUnsafeFeed: () => invalidated++,
  });
  h.safetyChanged(snapshot()); assert.equal(refreshed, 0);
  h.safetyChanged(snapshot()); assert.equal(refreshed, 0);
  h.safetyChanged(snapshot(2)); assert.equal(refreshed, 1);
  h.safetyChanged(null); assert.equal(refreshed, 1);
  h.safetyChanged(snapshot(2)); assert.equal(refreshed, 2, "same-revision recovery must reload the cleared timeline");
  h.safetyChanged({ ...snapshot(3), author_id: muted }); assert.equal(refreshed, 2);
  assert.ok(profiles >= 3);
  assert.equal(conversations, 2); assert.equal(quotes, 2); assert.equal(invalidated, 1);
});

test("missing current-account safety never restores cached recommendations", () => {
  const h = functions(["applyFeedSafety"], {
    safetyControls: { snapshot: null, hidden: () => false }, accounts: { current: { identity: { id: owner } } },
    cards: [card("blocked", blocked)], currentIndex: 3, sourceLabel: { textContent: "following" },
  });
  h.applyFeedSafety(); assert.equal(h.cards.length, 0); assert.equal(h.currentIndex, 0);
});

test("superseded safety reads await the replacement before admitting feed work", async () => {
  let resolve, calls = 0, admitted = false;
  const pending = new Promise(done => { resolve = done; });
  const h = functions(["ensureFeedSafety"], {
    DOMException, safetyUnavailable: false, accounts: { current: { identity: { id: owner } } },
    safetyControls: { ensure: () => ++calls === 1 ? Promise.resolve(null) : pending },
  });
  const task = h.ensureFeedSafety().then(() => { admitted = true; });
  await new Promise(done => setImmediate(done));
  assert.equal(calls, 2); assert.equal(admitted, false);
  resolve(snapshot()); await task; assert.equal(admitted, true);
});

test("safety admission rejects account replacement and records current-account failures for recovery", async () => {
  let resolve;
  const pending = new Promise(done => { resolve = done; });
  const h = functions(["ensureFeedSafety"], {
    DOMException, safetyUnavailable: false, accounts: { current: { identity: { id: owner } } },
    safetyControls: { ensure: () => pending },
  });
  const task = h.ensureFeedSafety(); h.accounts.current = null; resolve(snapshot());
  await assert.rejects(task, { name: "AbortError" }); assert.equal(h.safetyUnavailable, false);
  h.accounts.current = { identity: { id: owner } };
  h.safetyControls.ensure = async () => { throw new Error("offline"); };
  await assert.rejects(h.ensureFeedSafety(), /offline/); assert.equal(h.safetyUnavailable, true);
});
