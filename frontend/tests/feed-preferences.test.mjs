import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";

function load(name, dependencies = {}) {
  const code = ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText;
  const exports = {};
  new Function("exports", "require", code)(exports, name => {
    assert.ok(name in dependencies, `Unexpected dependency ${name}`);
    return dependencies[name];
  });
  return exports;
}
const local = load("local-preferences");
const { FeedPreferences } = load("feed-preferences", { "./local-preferences": local });
const a = `id_${"a".repeat(64)}`, b = `id_${"b".repeat(64)}`;
const clone = value => JSON.parse(JSON.stringify(value));

function harness() {
  const data = new Map(), events = [], shows = [], messages = [], counts = [];
  const storage = {
    getItem: key => data.get(key) ?? null,
    setItem: (key, value) => data.set(key, value),
    removeItem: key => data.delete(key),
  };
  const accounts = { current: { identity: { id: a, handle: "reader" } },
    localDataKey(kind) { return `babel.local.v2:${JSON.stringify(["https://babel.test", this.current?.identity.id ?? null, kind])}`; } };
  const view = { show: value => shows.push(clone(value)), message: (...value) => messages.push(value), setSeenCount: count => counts.push(count) };
  const host = { changed: () => events.push("changed"), historyCleared: () => events.push("cleared"), seenCount: () => 3 };
  const controller = new FeedPreferences(accounts, () => storage, view, host);
  controller.account();
  return { controller, accounts, storage, data, events, shows, messages, counts,
    key: kind => accounts.localDataKey(kind), read: () => clone(controller.read()) };
}

test("saves the complete local model using the account partition without changing unrelated data", () => {
  const h = harness(); h.data.set("unrelated", "keep");
  const next = { ...local.defaultPreferences(), interests: ["Music"], hiddenAuthors: [b], creatorAffinity: { [b]: .8 }, noveltyTolerance: 0 };
  h.controller.save(next);
  assert.deepEqual(h.read().interests, ["music"]);
  assert.equal(h.read().noveltyTolerance, 0);
  assert.equal(h.read().explorationPreference, null);
  assert.deepEqual(h.events, ["changed"]);
  assert.equal(h.data.get("unrelated"), "keep");
  assert.equal(h.shows.at(-1).scope, "This device - @reader");
  assert.equal(h.messages.at(-1)[0], "Saved on this device.");
});

test("accounts and guests never inherit another partition; same-session metadata does not erase edits", () => {
  const h = harness(); h.controller.save({ ...local.defaultPreferences(), interests: ["private"] });
  const count = h.shows.length;
  h.accounts.current = { identity: { id: a, handle: "renamed" } }; h.controller.account();
  assert.equal(h.shows.length, count);
  h.accounts.current = { identity: { id: b, handle: "second" } }; h.controller.account();
  assert.deepEqual(h.shows.at(-1).preferences.interests, []);
  h.accounts.current = null; h.controller.account();
  assert.equal(h.shows.at(-1).scope, "This device - Guest");
  assert.deepEqual(h.read().interests, []);
  h.accounts.current = { identity: { id: a, handle: "reader" } }; h.controller.account();
  assert.deepEqual(h.shows.at(-1).preferences.interests, ["private"]);
});

test("stale account callbacks cannot save or clear history into a different partition", () => {
  const h = harness(); h.accounts.current = null;
  assert.throws(() => h.controller.save(local.defaultPreferences()), /account changed/);
  assert.throws(() => h.controller.clearHistory(), /account changed/);
  assert.deepEqual(h.events, []); assert.equal(h.data.size, 0);
});

test("external preference edits reject stale full-form saves until reopened", () => {
  const h = harness();
  local.writePreferences(h.storage, h.key("preferences"), { ...local.defaultPreferences(), interests: ["elsewhere"] });
  assert.throws(() => h.controller.save(local.defaultPreferences()), /another tab/);
  assert.deepEqual(h.read().interests, ["elsewhere"]);
  h.controller.show(); h.controller.save({ ...h.controller.read(), expertise: ["networks"] });
  assert.deepEqual(h.read().interests, ["elsewhere"]);
  assert.deepEqual(h.read().expertise, ["networks"]);
});

test("reset preferences preserves reading history and never clears account/session storage", () => {
  const h = harness(); h.controller.save({ ...local.defaultPreferences(), mutedTerms: ["spoiler"] });
  h.data.set(h.key("seen"), '{"obj":2}'); h.data.set("babel.session", "keep");
  h.controller.reset();
  assert.deepEqual(h.read(), clone(local.defaultPreferences()));
  assert.equal(h.data.get(h.key("seen")), '{"obj":2}');
  assert.equal(h.data.get("babel.session"), "keep");
});

test("clearing history preserves preferences and unsaved form fields", () => {
  const h = harness(); h.controller.save({ ...local.defaultPreferences(), expertise: ["science"] });
  const saved = h.data.get(h.key("preferences")), shown = h.shows.length;
  h.data.set(h.key("seen"), '{"obj":2}'); h.data.set("other-history", "keep");
  h.controller.clearHistory();
  assert.equal(h.data.has(h.key("seen")), false);
  assert.equal(h.data.get(h.key("preferences")), saved);
  assert.equal(h.data.get("other-history"), "keep");
  assert.equal(h.shows.length, shown);
  assert.deepEqual(h.counts, [0]);
  assert.deepEqual(h.events.slice(-2), ["cleared", "changed"]);
});

test("failed persistence never announces success, reranks the feed or erases form inputs", () => {
  const h = harness(); const shown = h.shows.length;
  h.storage.setItem = () => { throw Error("quota"); };
  assert.throws(() => h.controller.save(local.defaultPreferences()), /could not be saved/);
  assert.throws(() => h.controller.hideAuthor(b), /could not be saved/);
  assert.deepEqual(h.events, []); assert.deepEqual(h.messages, []);
  assert.equal(h.shows.length, shown);
  h.storage.removeItem = () => { throw Error("denied"); };
  assert.throws(() => h.controller.clearHistory(), /could not be cleared/);
  assert.deepEqual(h.events, []);
});

test("author hide and one-use Undo preserve concurrent unrelated settings", () => {
  const h = harness(); const undo = h.controller.hideAuthor(b);
  assert.deepEqual(h.read().hiddenAuthors, [b]);
  assert.equal(h.controller.hideAuthor(b), null);
  local.writePreferences(h.storage, h.key("preferences"), { ...h.controller.read(), interests: ["later"], hiddenAuthors: [a, b] });
  undo();
  assert.deepEqual(h.read().hiddenAuthors, [a]);
  assert.deepEqual(h.read().interests, ["later"]);
  const calls = h.events.length; undo(); assert.equal(h.events.length, calls);
});

test("an Undo retained from another account cannot change either owner's data", () => {
  const h = harness(); const undo = h.controller.hideAuthor(b), firstKey = h.key("preferences");
  h.accounts.current = null; h.controller.account();
  assert.throws(undo, /account changed/);
  assert.deepEqual(JSON.parse(h.data.get(firstKey)).hiddenAuthors, [b]);
  assert.equal(h.data.has(h.key("preferences")), false);
});

test("unreadable saved data is preserved and an author shortcut cannot replace it with defaults", () => {
  const h = harness(); h.data.set(h.key("preferences"), "broken");
  h.controller.show();
  assert.match(h.shows.at(-1).warning, /invalid JSON/);
  assert.throws(() => h.controller.hideAuthor(b), /warning/);
  assert.equal(h.data.get(h.key("preferences")), "broken");
  h.controller.reset(); assert.deepEqual(h.read(), clone(local.defaultPreferences()));
});

test("invalid form values fail before any storage or feed change", () => {
  const h = harness();
  assert.throws(() => h.controller.save({ ...local.defaultPreferences(), evidencePreference: NaN }), /finite/);
  assert.throws(() => h.controller.hideAuthor("not-an-author"), /canonical author/);
  assert.equal(h.data.size, 0); assert.deepEqual(h.events, []);
});

test("storage events ignore other accounts and history events do not create a cross-tab reranking loop", () => {
  const h = harness(); h.controller.externalChange("other-account");
  assert.deepEqual(h.events, []); assert.deepEqual(h.counts, []);
  h.controller.externalChange(h.key("seen"));
  assert.deepEqual(h.events, []); assert.deepEqual(h.counts, [3]);
  h.controller.externalChange(h.key("preferences"));
  assert.deepEqual(h.events, ["changed"]);
  assert.match(h.messages.at(-1)[0], /another tab/);
  h.controller.externalChange(null); assert.equal(h.events.length, 2);
});
