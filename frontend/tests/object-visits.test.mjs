import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const exports = {};
vm.runInNewContext(ts.transpileModule(readFileSync(new URL("../src/app/object-visits.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText, { exports });
const { ObjectVisits } = exports;
const card = id => ({ id });
function harness() {
  const feed = { cards: [card("a"), card("b")], index: 1, source: "following",
    reading: { anchor: "reply:kept", offset: -12, top: 320 } };
  const h = { current: feed, feed, closes: 0, shows: [], focuses: [], available: false };
  h.visits = new ObjectVisits({ capture: () => h.current, beforeVisit() { h.closes++; },
    show(snapshot) { h.current = snapshot; h.shows.push(snapshot); },
    focus(opener) { h.focuses.push(opener); }, availability(back) { h.available = back; } });
  return h;
}

test("quote visits preserve the exact feed, source, index, reading anchor and opener", () => {
  const h = harness(), opener = {};
  h.visits.open(card("original"), opener);
  assert.equal(h.current.cards.length, 1);
  assert.equal(h.current.cards[0].id, "original");
  assert.equal(h.current.source, "Shared post");
  assert.equal(h.current.reading, null); assert.equal(h.available, true);
  assert.equal(h.focuses[0], null);
  h.visits.back();
  assert.equal(h.current, h.feed); assert.equal(h.focuses.at(-1), opener);
  assert.equal(h.closes, 2); assert.equal(h.available, false);
});

test("nested quotes unwind one level at a time and each remembers its reading position", () => {
  const h = harness(); h.visits.open(card("one"), {});
  h.current = { ...h.current, reading: { anchor: "quote:two", offset: 4, top: 100 } };
  const first = h.current;
  h.visits.open(card("two"), {}); h.visits.back();
  assert.equal(h.current, first); assert.equal(h.available, true);
  h.visits.back(); assert.equal(h.current, h.feed);
});

test("opening the active Object does not create a loop or interrupt its controls", () => {
  const h = harness(); h.visits.open(card("b"), {});
  assert.equal(h.closes, 0); assert.equal(h.visits.active, false);
  h.visits.back(); assert.equal(h.shows.length, 0);
});

test("long quote chains stay bounded without losing the original feed", () => {
  const h = harness();
  for (let i = 0; i < 80; i++) h.visits.open(card(`quote-${i}`), null);
  assert.equal(h.visits.checkpoint().length, 32);
  let backs = 0;
  while (h.visits.active) { h.visits.back(); backs++; }
  assert.equal(backs, 32); assert.equal(h.current, h.feed);
});

test("profile detours can suspend and restore quote history without sharing its array", () => {
  const h = harness(); h.visits.open(card("one"), {});
  const checkpoint = h.visits.checkpoint();
  h.visits.clear(); assert.equal(checkpoint.length, 1); assert.equal(h.available, false);
  h.visits.restore(checkpoint); assert.equal(h.available, true);
  checkpoint.length = 0;
  h.visits.back(); assert.equal(h.current, h.feed);
});

test("account or feed reset cannot later restore a previous feed through back", () => {
  const h = harness(); h.visits.open(card("one"), {}); h.visits.clear();
  const after = h.current, count = h.shows.length;
  h.visits.back(); assert.equal(h.shows.length, count); assert.equal(h.current, after);
});
