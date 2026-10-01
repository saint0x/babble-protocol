import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("../src/app/local-preferences.ts", import.meta.url), "utf8");
const compiled = ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
}).outputText;
const { defaultPreferences, parsePreferences, readPreferences, writePreferences, clearLocalHistory } =
  await import(`data:text/javascript;base64,${Buffer.from(compiled).toString("base64")}`);

const author = (index) => `id_${index.toString(16).padStart(64, "0")}`;
const termFields = ["interests", "expertise", "mutedTerms", "hiddenTerms"];
const scalarFields = ["noveltyTolerance", "explorationPreference", "evidencePreference", "contradictionTolerance"];
const plain = (value) => JSON.parse(JSON.stringify(value));
function legacy() {
  return {
    interests: ["science"], expertise: ["biology"], mutedTerms: ["spoilers"], hiddenTerms: ["spam"],
    hiddenAuthors: [author(1)], creatorAffinity: { [author(2)]: 0.7 },
    noveltyTolerance: 0, explorationPreference: 1, evidencePreference: 0.4, contradictionTolerance: null,
  };
}
function storage(initial = []) {
  const entries = new Map(initial);
  const calls = [];
  return {
    entries, calls,
    getItem(key) { calls.push(["get", key]); return entries.get(key) ?? null; },
    setItem(key, value) { calls.push(["set", key]); entries.set(key, value); },
    removeItem(key) { calls.push(["remove", key]); entries.delete(key); },
  };
}

test("defaults have the exact legacy contract and independent mutable containers", () => {
  const first = defaultPreferences(), second = defaultPreferences();
  assert.deepEqual(Object.keys(first), Object.keys(legacy()));
  for (const field of [...termFields, "hiddenAuthors"]) {
    assert.deepEqual(first[field], []);
    first[field].push("mutated");
    assert.deepEqual(second[field], []);
  }
  for (const field of scalarFields) assert.equal(first[field], null);
  first.creatorAffinity[author(1)] = 0.2;
  assert.equal(second.creatorAffinity[author(1)], undefined);
  assert.equal(Object.getPrototypeOf(second.creatorAffinity), null);
});

test("legacy saved JSON reloads without migration, writes, or key changes", () => {
  const saved = JSON.stringify(legacy());
  const store = storage([["babel.preferences.legacy", saved]]);
  const result = readPreferences(store, "babel.preferences.legacy");
  assert.equal(result.warning, null);
  assert.deepEqual(plain(result.preferences), legacy());
  assert.equal(store.entries.get("babel.preferences.legacy"), saved);
  assert.deepEqual(store.calls, [["get", "babel.preferences.legacy"]]);
});

test("missing data is a normal default; unavailable reads report warnings", () => {
  const store = storage();
  assert.deepEqual(readPreferences(store, "missing"), { preferences: defaultPreferences(), warning: null });
  assert.deepEqual(store.calls, [["get", "missing"]]);
  for (const unavailable of [null, { getItem() { throw new Error("private detail"); } }]) {
    const result = readPreferences(unavailable, "key");
    assert.deepEqual(result.preferences, defaultPreferences());
    assert.match(result.warning, /unavailable/);
    assert.ok(!result.warning.includes("private detail"));
  }
});

test("malformed, empty, wrong-shaped and unknown saved fields remain untouched", () => {
  for (const saved of ["", "{", "null", "[]", "{}", JSON.stringify({ ...legacy(), extra: true }),
    JSON.stringify({ ...legacy(), interests: ["valid", 7] }),
    JSON.stringify({ ...legacy(), evidencePreference: 2 })]) {
    const store = storage([["prefs", saved]]);
    const result = readPreferences(store, "prefs");
    assert.deepEqual(result.preferences, defaultPreferences());
    assert.match(result.warning, /invalid/);
    assert.equal(store.entries.get("prefs"), saved);
    assert.deepEqual(store.calls, [["get", "prefs"]]);
  }
});

test("terms trim, lowercase and NFC-normalize with stable deduplication", () => {
  const value = legacy();
  for (const field of termFields) value[field] = ["  CAF\u00c9  ", "cafe\u0301", "\u65e5\u672c\u8a9e", "\u65e5\u672c\u8a9e", "two words"];
  value.hiddenAuthors = [author(3), author(1), author(3)];
  const result = parsePreferences(value);
  for (const field of termFields) assert.deepEqual(result[field], ["caf\u00e9", "\u65e5\u672c\u8a9e", "two words"]);
  assert.deepEqual(result.hiddenAuthors, [author(3), author(1)]);
  assert.equal(value.interests.length, 5);
});

test("term limits count Unicode code points and reject rather than truncate", () => {
  for (const field of termFields) {
    assert.equal(parsePreferences({ ...legacy(), [field]: ["\u{1f600}".repeat(128)] })[field][0], "\u{1f600}".repeat(128));
    for (const term of ["x".repeat(129), "\u{1f600}".repeat(129), " ", "", "\ud800", "\udfff", "x".repeat(65537)]) {
      assert.throws(() => parsePreferences({ ...legacy(), [field]: [term] }), new RegExp(field));
    }
  }
});

test("every term list accepts 128 entries and rejects excess even if duplicated", () => {
  for (const field of termFields) {
    const terms = Array.from({ length: 128 }, (_, i) => `term${i}`);
    assert.equal(parsePreferences({ ...legacy(), [field]: terms })[field].length, 128);
    for (const invalid of [[...terms, "extra"], Array(129).fill("duplicate"), null, "term", ["ok", 7], Array(2)]) {
      assert.throws(() => parsePreferences({ ...legacy(), [field]: invalid }), new RegExp(field));
    }
  }
});

test("phrase entries cannot silently exceed the SDK's 128 distinct word budget", () => {
  for (const field of termFields) {
    const entries = Array.from({ length: 64 }, (_, i) => `first${i} second${i}`);
    assert.equal(parsePreferences({ ...legacy(), [field]: entries })[field].length, 64);
    assert.throws(() => parsePreferences({ ...legacy(), [field]: [...entries, "extra"] }), /128 distinct words/);
    assert.equal(parsePreferences({ ...legacy(), [field]: [...entries, "first0 second0"] })[field].length, 64);
  }
});

test("hidden authors accept 256 canonical IDs and reject noncanonical IDs", () => {
  const ids = Array.from({ length: 256 }, (_, i) => author(i));
  assert.equal(parsePreferences({ ...legacy(), hiddenAuthors: ids }).hiddenAuthors.length, 256);
  assert.throws(() => parsePreferences({ ...legacy(), hiddenAuthors: [...ids, author(256)] }), /256/);
  for (const id of ["alice", author(1).toUpperCase(), ` ${author(1)}`, `${author(1)} `, `id_${"g".repeat(64)}`, author(1).slice(0, -1), 4]) {
    assert.throws(() => parsePreferences({ ...legacy(), hiddenAuthors: [id] }), /hiddenAuthors/);
    assert.throws(() => parsePreferences({ ...legacy(), creatorAffinity: { [id]: 0.5 } }), /creatorAffinity/);
  }
});

test("affinity map accepts 128 entries, rejects excess and clones to a null prototype", () => {
  const entries = Object.fromEntries(Array.from({ length: 128 }, (_, i) => [author(i), i / 128]));
  const result = parsePreferences({ ...legacy(), creatorAffinity: entries });
  assert.equal(Object.keys(result.creatorAffinity).length, 128);
  assert.equal(Object.getPrototypeOf(result.creatorAffinity), null);
  assert.throws(() => parsePreferences({ ...legacy(), creatorAffinity: { ...entries, [author(128)]: 1 } }), /128/);
  for (const invalid of [null, [], 1, "score"]) {
    assert.throws(() => parsePreferences({ ...legacy(), creatorAffinity: invalid }), /creatorAffinity/);
  }
});

test("all scalars and affinity scores require finite numbers in the closed unit interval", () => {
  for (const field of scalarFields) {
    for (const value of [null, 0, 0.5, 1]) assert.equal(parsePreferences({ ...legacy(), [field]: value })[field], value);
    for (const invalid of [undefined, -0.01, 1.01, NaN, Infinity, -Infinity, "0.5", false, {}, []]) {
      assert.throws(() => parsePreferences({ ...legacy(), [field]: invalid }), new RegExp(field));
      assert.throws(() => parsePreferences({ ...legacy(), creatorAffinity: { [author(1)]: invalid } }), /creatorAffinity/);
    }
  }
  assert.throws(() => parsePreferences({ ...legacy(), creatorAffinity: { [author(1)]: null } }), /creatorAffinity/);
  for (const score of [0, 1]) assert.equal(parsePreferences({ ...legacy(), creatorAffinity: { [author(1)]: score } }).creatorAffinity[author(1)], score);
});

test("all fields are required; unknown or nonplain objects cannot silently pass", () => {
  for (const invalid of [null, undefined, [], 42, "{}", new Date(), Object.create(legacy())]) {
    assert.throws(() => parsePreferences(invalid), /plain object/);
  }
  for (const field of Object.keys(legacy())) {
    const value = legacy(); delete value[field];
    assert.throws(() => parsePreferences(value), new RegExp(`missing ${field}`));
  }
  assert.throws(() => parsePreferences({ ...legacy(), unexpected: true }), /unknown field/);
  const value = Object.assign(Object.create(null), legacy());
  assert.deepEqual(plain(parsePreferences(value)), legacy());
});

test("prototype fields, symbols, nonenumerable properties and getters are rejected", () => {
  for (const key of ["__proto__", "constructor", "prototype"]) {
    const value = JSON.parse(JSON.stringify(legacy()));
    Object.defineProperty(value, key, { enumerable: true, value: {} });
    assert.throws(() => parsePreferences(value), /unsupported property/);
    assert.throws(() => parsePreferences({ ...legacy(), creatorAffinity: JSON.parse(`{"${key}":0.5}`) }), /unsupported property/);
  }
  for (const field of ["interests", "creatorAffinity"]) {
    const value = legacy();
    Object.defineProperty(value, field, { enumerable: true, get() { assert.fail("getter must not execute"); } });
    assert.throws(() => parsePreferences(value), /unsupported property/);
  }
  for (const key of [Symbol("hidden"), "nonenumerable"]) {
    const value = legacy(); Object.defineProperty(value, key, { value: true });
    assert.throws(() => parsePreferences(value), /unsupported property/);
  }
  const inherited = Object.create({ [author(1)]: 1 });
  assert.throws(() => parsePreferences({ ...legacy(), creatorAffinity: inherited }), /inherited/);
  const getterMap = {};
  Object.defineProperty(getterMap, author(1), { enumerable: true, get() { assert.fail("getter must not execute"); } });
  assert.throws(() => parsePreferences({ ...legacy(), creatorAffinity: getterMap }), /unsupported property/);
  const list = ["valid"];
  Object.defineProperty(list, "0", { get() { assert.fail("getter must not execute"); } });
  assert.throws(() => parsePreferences({ ...legacy(), interests: list }), /must be a string/);
  const extraList = ["valid"]; extraList.extra = true;
  assert.throws(() => parsePreferences({ ...legacy(), interests: extraList }), /only string entries/);
  assert.equal({}.polluted, undefined);
});

test("reads enforce a 64 KiB UTF-8 bound including otherwise harmless whitespace", () => {
  const base = JSON.stringify(legacy());
  const exact = base + " ".repeat(65536 - Buffer.byteLength(base));
  assert.equal(readPreferences(storage([["prefs", exact]]), "prefs").warning, null);
  for (const saved of [exact + " ", JSON.stringify({ ...legacy(), interests: ["\u754c".repeat(23000)] })]) {
    const store = storage([["prefs", saved]]);
    assert.match(readPreferences(store, "prefs").warning, /64 KiB/);
    assert.equal(store.entries.get("prefs"), saved);
    assert.deepEqual(store.calls, [["get", "prefs"]]);
  }
});

test("aggregate serialized byte size is bounded for parsing and writing", () => {
  const value = legacy();
  for (const field of termFields) value[field] = Array.from({ length: 128 }, (_, i) => `${i}${"\u754c".repeat(124)}`);
  assert.throws(() => parsePreferences(value), /64 KiB/);
  const store = storage([["prefs", "original"]]);
  assert.throws(() => writePreferences(store, "prefs", value), /64 KiB/);
  assert.equal(store.entries.get("prefs"), "original");
  assert.deepEqual(store.calls, []);
});

test("validation failures never touch storage or partially replace saved values", () => {
  const store = storage([["prefs", JSON.stringify(legacy())]]);
  for (const value of [{ ...legacy(), interests: ["valid", ""] }, { ...legacy(), evidencePreference: Infinity },
    { ...legacy(), creatorAffinity: { [author(1)]: 2 } }, { ...legacy(), extra: false }]) {
    assert.throws(() => writePreferences(store, "prefs", value));
    assert.deepEqual(store.calls, []);
    assert.equal(store.entries.get("prefs"), JSON.stringify(legacy()));
  }
});

test("write and history removal failures are explicit and preserve their cause", () => {
  const failure = new Error("quota or permissions");
  assert.throws(() => writePreferences(null, "prefs", legacy()), /unavailable/);
  assert.throws(() => writePreferences({ setItem() { throw failure; } }, "prefs", legacy()),
    (error) => /could not be saved/.test(error.message) && error.cause === failure);
  assert.throws(() => clearLocalHistory(null, "seen"), /unavailable/);
  assert.throws(() => clearLocalHistory({ removeItem() { throw failure; } }, "seen"),
    (error) => /could not be cleared/.test(error.message) && error.cause === failure);
});

test("successful writes and reads defensively clone every list and affinity map", () => {
  const value = legacy(), store = storage();
  const written = writePreferences({ setItem: store.setItem.bind(store) }, "prefs", value);
  const snapshot = JSON.stringify(legacy());
  for (const field of [...termFields, "hiddenAuthors"]) {
    assert.notEqual(written[field], value[field]);
    value[field].push("source mutation");
    written[field].push("result mutation");
  }
  value.creatorAffinity[author(2)] = 0;
  written.creatorAffinity[author(2)] = 1;
  assert.equal(store.entries.get("prefs"), snapshot);
  const first = readPreferences({ getItem: store.getItem.bind(store) }, "prefs");
  first.preferences.interests.push("read mutation");
  first.preferences.creatorAffinity[author(2)] = 0;
  assert.deepEqual(plain(readPreferences(store, "prefs").preferences), legacy());
});

test("caller keys isolate accounts, origins and guest preferences without ambient reads", () => {
  const store = storage();
  const keys = ["origin-a:alice:preferences", "origin-a:bob:preferences", "origin-b:alice:preferences", "origin-a:guest:preferences"];
  for (const [index, key] of keys.entries()) writePreferences(store, key, { ...legacy(), interests: [`interest ${index}`] });
  for (const [index, key] of keys.entries()) assert.deepEqual(readPreferences(store, key).preferences.interests, [`interest ${index}`]);
  assert.deepEqual([...store.entries.keys()], keys);
  assert.equal(store.calls.length, 8);
});

test("history clearing removes only the supplied key and is idempotent", () => {
  const store = storage([["alice:seen", "history"], ["alice:preferences", "preferences"], ["bob:seen", "other history"], ["session", "secret"]]);
  clearLocalHistory({ removeItem: store.removeItem.bind(store) }, "alice:seen");
  clearLocalHistory({ removeItem: store.removeItem.bind(store) }, "alice:seen");
  assert.deepEqual([...store.entries], [["alice:preferences", "preferences"], ["bob:seen", "other history"], ["session", "secret"]]);
  assert.deepEqual(store.calls, [["remove", "alice:seen"], ["remove", "alice:seen"]]);
});
