import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import * as sdk from "@babel-protocol/sdk";
import { mediaResource } from "./media-modules.mjs";

function module(name, require, globals = {}) {
  const context = { exports: {}, require, AbortController, AbortSignal, URL, Response, TextEncoder, TextDecoder,
    structuredClone, console, crypto: globalThis.crypto, ...globals };
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, context);
  return context.exports;
}
const contract = module("profile-response");
const invocations = module("invocations", () => sdk);
const browserInvocations = module("browser-invocations", () => sdk);
const protocol = module("protocol", (name) => {
  if (name === "./invocations") return invocations;
  if (name === "./browser-invocations") return browserInvocations;
  if (name === "./media-resource") return mediaResource;
  if (name === "./profile-response") return contract;
  assert.equal(name, "@babel-protocol/sdk");
  return { HttpRpcTransport: class {}, hostBinding: () => ({}) };
});
const id = (letter) => `id_${letter.repeat(64)}`;
const identity = (author = id("a")) => ({ id: author, handle: "Authoritative handle", kind: "Person",
  created_at: "2026-09-29T12:00:00Z", public_key: { algorithm: "Ed25519", bytes: "a".repeat(64) },
  signature: { algorithm: "Ed25519", bytes: "b".repeat(128) } });
const card = (key, author = id("a")) => ({ id: key, author, title: `Title ${key}`, content: `Content ${key}`, createdAt: "2026-09-29T12:00:00Z" });
const page = (keys, cursor = null, author = id("a")) => ({ identity: identity(author), cards: keys.map((key) => card(key, author)), nextCursor: cursor });
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };
const settle = () => new Promise((resolve) => setImmediate(resolve));

class Element {
  constructor(tag, document) { Object.assign(this, { tag, document, children: [], dataset: {}, attrs: {}, events: {}, hidden: false, disabled: false, isConnected: true, open: false, scrollTop: 0, text: "" }); }
  append(...elements) { this.children.push(...elements); }
  replaceChildren(...elements) { this.children = elements; this.text = ""; }
  set textContent(text) { this.replaceChildren(); this.text = text; }
  get textContent() { return this.text + this.children.map((element) => element.textContent).join(""); }
  set innerHTML(_) { throw new Error("Unsafe HTML insertion"); }
  setAttribute(key, value) { this.attrs[key] = value; }
  removeAttribute(key) { delete this.attrs[key]; }
  closest() { return null; }
  querySelector(selector) { return this.refs?.[selector] ?? this.querySelectorAll(selector)[0] ?? null; }
  querySelectorAll(selector) { return this.children.filter((element) => selector === "[aria-current]" ? element.attrs["aria-current"] !== undefined : element.attrs["aria-current"] === "true"); }
  addEventListener(type, callback) { (this.events[type] ??= []).push(callback); }
  emit(type) { const event = { prevented: false, preventDefault() { this.prevented = true; } }; for (const callback of this.events[type] ?? []) callback(event); return event; }
  click() { if (!this.disabled) this.emit("click"); }
  focus() { this.document.activeElement = this; }
  showModal() { this.open = true; }
  close() { this.open = false; }
}

function harness() {
  const document = { activeElement: null };
  document.createElement = (tag) => new Element(tag, document);
  const dialog = document.createElement("dialog");
  const nodes = Object.fromEntries(["title", "identity", "status", "list", "more", "close"].map((key) => [key, document.createElement(key === "more" || key === "close" ? "button" : "div")]));
  dialog.refs = Object.fromEntries(Object.entries(nodes).map(([key, node]) => [`[data-public-profile-${key}]`, node]));
  const identities = [], pages = [], selections = [], closes = [];
  const source = {
    publicIdentity(author, signal) { const request = { ...deferred(), author, signal }; identities.push(request); return request.promise; },
    profileObjects(author, cursor, signal) { const request = { ...deferred(), author, cursor, signal }; pages.push(request); return request.promise; },
  };
  const { Profiles } = module("profiles", () => protocol, { document, HTMLElement: Element, Error });
  const profiles = new Profiles(dialog, source, (...args) => selections.push(args), () => closes.push(true));
  return { profiles, dialog, nodes, document, identities, pages, selections, closes, opener: document.createElement("button") };
}
async function open(h, author = id("a")) {
  h.profiles.open(author, h.opener);
  h.identities.at(-1).resolve(identity(author));
  await settle();
}

test("authoritative identity loads before empty state, and Escape returns focus", async () => {
  const h = harness();
  h.profiles.open(id("a"), h.opener);
  assert.equal(h.dialog.open, true);
  assert.equal(h.document.activeElement, h.nodes.close);
  assert.equal(h.nodes.status.textContent, "Loading profile...");
  h.identities[0].resolve(identity());
  await settle();
  assert.equal(h.nodes.title.textContent, "Authoritative handle");
  h.pages[0].resolve(page([]));
  await settle();
  assert.equal(h.nodes.status.textContent, "No public Objects yet.");
  assert.equal(h.nodes.more.hidden, true);
  assert.equal(h.dialog.emit("cancel").prevented, true);
  assert.equal(h.dialog.open, false);
  assert.equal(h.document.activeElement, h.opener);
  assert.equal(h.closes.length, 1);
});

test("pagination deduplicates, retries same cursor, preserves rows and moves focus off disappearing more", async () => {
  const h = harness();
  await open(h);
  h.pages[0].resolve(page(["a", "b"], "next"));
  await settle();
  const first = h.nodes.list.children[0];
  h.nodes.more.click();
  h.nodes.more.click();
  assert.equal(h.pages.length, 2);
  h.pages[1].reject(new Error("Connection lost"));
  await settle();
  assert.equal(h.nodes.more.textContent, "Retry");
  assert.equal(h.nodes.list.children[0], first);
  h.nodes.more.focus();
  h.nodes.more.click();
  assert.equal(h.pages[2].cursor, "next");
  h.pages[2].resolve(page(["b", "c"]));
  await settle();
  assert.equal(h.nodes.list.children.length, 3);
  assert.equal(h.document.activeElement, h.nodes.list.children[2]);
  assert.equal(h.nodes.more.hidden, true);
});

test("select Object suspends modal, back restores row focus and scroll, close restores feed callback", async () => {
  const h = harness();
  await open(h);
  h.pages[0].resolve(page(["a", "b"]));
  await settle();
  h.dialog.scrollTop = 320;
  h.nodes.list.children[1].click();
  assert.equal(h.dialog.open, false);
  assert.equal(h.selections[0][0].id, "b");
  assert.equal(h.selections[0][1].length, 2);
  assert.equal(h.closes.length, 0);
  h.dialog.scrollTop = 0;
  h.profiles.resume();
  h.dialog.emit("close"); // delayed close from suspension must not close a reopened dialog
  assert.equal(h.dialog.open, true);
  assert.equal(h.dialog.scrollTop, 320);
  assert.equal(h.document.activeElement, h.nodes.list.children[1]);
  h.nodes.close.click();
  assert.equal(h.closes.length, 1);
});

test("author changes, close and account invalidation discard stale successful and failed responses", async () => {
  const h = harness();
  await open(h);
  const stale = h.pages[0];
  await open(h, id("b"));
  assert.equal(stale.signal.aborted, true);
  stale.resolve(page(["stale"]));
  h.pages[1].resolve(page(["current"], null, id("b")));
  await settle();
  assert.equal(h.nodes.list.children.length, 1);
  assert.match(h.nodes.list.textContent, /current/);
  h.profiles.open(id("a"));
  const pending = h.identities.at(-1);
  h.profiles.close();
  pending.reject(new Error("stale error"));
  await settle();
  assert.equal(pending.signal.aborted, true);
  assert.notEqual(h.nodes.status.textContent, "stale error");
  assert.equal(h.dialog.open, false);
});

test("identity failure retries identity, and snapshot conflict refreshes page one", async () => {
  const h = harness();
  h.profiles.open(id("a"));
  h.identities[0].reject(new Error("Unavailable"));
  await settle();
  assert.equal(h.nodes.more.textContent, "Retry");
  h.nodes.more.click();
  assert.equal(h.identities.length, 2);
  h.identities[1].resolve(identity());
  await settle();
  h.pages[0].resolve(page(["a"], "next"));
  await settle();
  h.nodes.more.click();
  h.pages[1].reject(new protocol.ProfileRequestError(409));
  await settle();
  assert.equal(h.nodes.more.textContent, "Refresh profile");
  h.nodes.more.click();
  assert.equal(h.nodes.list.children.length, 0);
  h.identities[2].resolve(identity());
  await settle();
  assert.equal(h.pages[2].cursor, null);
});

test("repeated cursors and cross-author pages are recoverable errors without partial appends", async () => {
  for (const invalid of [page(["bad"], "next"), page(["bad"], null, id("b"))]) {
    const h = harness();
    await open(h);
    h.pages[0].resolve(page(["a"], "next"));
    await settle();
    h.nodes.more.click();
    h.pages[1].resolve(invalid);
    await settle();
    assert.equal(h.nodes.more.textContent, "Refresh profile");
    assert.equal(h.nodes.list.children.length, 1);
  }
});

const object = () => ({ id: `obj_${"c".repeat(64)}`, author: id("a"), created_at: "2026-09-29T12:00:00Z",
  kind: "text", schema: "babel.text.v1", protocol: { name: "babel", version: 1 }, payload: { text: "Real content" },
  provenance: { parent: null, forked_from: null, remixed_from: [] }, relations: [], resources: [], surfaces: [], capabilities: [] });

test("explicit Object opening fetches the requested record without inventing rank provenance", async () => {
  const client = new protocol.BabelFrontendClient("https://babel.example", async () => { throw new Error("unexpected HTTP request"); });
  const record = object();
  client.rpc = async (method, input) => {
    assert.equal(method, "babel.object.get.v1");
    assert.equal(input.object_id, record.id);
    return { object: record };
  };
  const card = await client.publicObject(record.id);
  assert.equal(card.id, record.id);
  assert.equal(card.source, "object");
  assert.equal(card.rankingProvider, null);
  assert.equal(card.score, null);
  client.rpc = async () => ({ object: { ...record, id: `obj_${"d".repeat(64)}` } });
  await assert.rejects(client.publicObject(record.id), /different Object/);
});

test("production profile HTTP client uses injected transport, bounded page request and existing Object conversion", async () => {
  const requests = [];
  const client = new protocol.BabelFrontendClient("https://babel.example", async (url, options) => {
    requests.push({ url, options });
    return Response.json(url.pathname.endsWith("/objects") ? { identity: identity(), objects: [object()], next_cursor: null } : { identity: identity() });
  });
  const signal = new AbortController().signal;
  assert.equal((await client.publicIdentity(id("a"), signal)).handle, "Authoritative handle");
  const result = await client.profileObjects(id("a"), "opaque|cursor", signal);
  assert.equal(requests[1].url.searchParams.get("limit"), "20");
  assert.equal(requests[1].url.searchParams.get("cursor"), "opaque|cursor");
  assert.equal(result.cards[0].content, "Real content");
  assert.equal(result.cards[0].author, id("a"));
});

test("HTTP malformed JSON, missing shapes, oversized pages and wrong author produce readable bounded errors", async () => {
  for (const value of [null, {}, { identity: identity(), objects: null },
    { identity: identity(), objects: Array(51).fill(object()), next_cursor: null },
    { identity: identity(), objects: [{ ...object(), resources: [null] }], next_cursor: null },
    { identity: identity(id("b")), objects: [], next_cursor: null }]) {
    const client = new protocol.BabelFrontendClient("https://babel.example", async () => Response.json(value));
    await assert.rejects(client.profileObjects(id("a"), null, new AbortController().signal), /invalid profile response/);
  }
  const malformed = new protocol.BabelFrontendClient("https://babel.example", async () => new Response("<html>bad upstream</html>"));
  await assert.rejects(malformed.publicIdentity(id("a"), new AbortController().signal), /invalid profile response/);
  const conflict = new protocol.BabelFrontendClient("https://babel.example", async () => new Response("private diagnostic", { status: 409 }));
  await assert.rejects(conflict.profileObjects(id("a"), "bookmark", new AbortController().signal), /profile has changed/);
});

test("JSON response byte limit stops oversized payloads", async () => {
  await assert.rejects(contract.profileJson(new Response("x".repeat(32 * 1024 * 1024 + 1))), /too large/);
});

test("production deck integration opens the selected Object and restores original feed index and reading position", () => {
  const main = ts.createSourceFile("main.ts", readFileSync(new URL("../src/app/main.ts", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true);
  const names = new Set(["profileReturn", "profiles"]);
  const statements = main.statements.filter((statement) => ts.isVariableStatement(statement)
    && statement.declarationList.declarations.some((declaration) => names.has(declaration.name.getText(main))));
  assert.equal(statements.length, 2);
  let callbacks;
  const cardElement = { scrollTop: 240, focus() { this.focused = true; } };
  const original = [card("feed-a"), card("feed-b")];
  const h = {
    Profiles: class { constructor(_dialog, _source, select, closed) { callbacks = { select, closed }; } },
    profileTargetChanged() {},
    required: (value) => value, document: { querySelector: () => ({}) }, client: {},
    cards: original, currentIndex: 1, loadSequence: 3, profileBack: { hidden: true },
    followingDirty: false, followingToolbar: { hidden: true }, activeLens: "balanced",
    sourceLabel: { textContent: "discovery" }, error: { hidden: false },
    deck: { querySelector(selector) { assert.match(selector, /:not\(\[data-exiting\]\)/); return cardElement; } },
    captureReading: (element) => ({ top: element.scrollTop }),
    restoreReading: (element, position) => { element.scrollTop = position.top; },
    closeSurface() {}, closeJudgments() {}, setStatus() {}, render() { cardElement.scrollTop = 0; },
    objectVisits: { active: false, checkpoint: () => [], clear() {}, restore() {} },
  };
  vm.runInNewContext(ts.transpileModule(statements.map((statement) => statement.getText(main)).join("\n"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None },
  }).outputText, h);
  const authored = [card("profile-a"), card("profile-b")];
  callbacks.select(authored[1], authored);
  assert.equal(h.cards, authored);
  assert.equal(h.currentIndex, 1);
  assert.equal(h.loadSequence, 4);
  assert.equal(h.profileBack.hidden, false);
  assert.equal(h.sourceLabel.textContent, "public profile");
  assert.equal(cardElement.focused, true);
  callbacks.select(authored[0], authored); // another selection must not replace original feed history
  callbacks.closed();
  assert.equal(h.cards, original);
  assert.equal(h.currentIndex, 1);
  assert.equal(h.profileBack.hidden, true);
  assert.equal(h.sourceLabel.textContent, "discovery");
  assert.equal(cardElement.scrollTop, 240);
});
