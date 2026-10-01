import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import { createPersonalizationFilter, summarizeDiscoveryObject } from "@babble-protocol/sdk";
import * as sdk from "@babble-protocol/sdk";

function module(name, require = () => { throw new Error("Unexpected import"); }, globals = {}) {
  const context = { exports: {}, require, AbortController, AbortSignal, URL, Response, Request, Headers,
    EventTarget, Event, TextEncoder, TextDecoder, structuredClone, console, Error, crypto: { randomUUID: () => "stable-key" }, ...globals };
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, context);
  return context.exports;
}
const response = module("profile-response");
const following = module("following", () => response);
const { FollowingClient, AuthorFollow, FollowingPages, FollowingError } = following;
const { Accounts } = module("accounts", undefined, { fetch });
const id = (letter) => `id_${letter.repeat(64)}`;
const owner = id("a"), target = id("b"), other = id("c");
const state = (following = false, revision = 0, author_id = owner, target_id = target) => ({ author_id, target_id, following, revision });
const identity = (author = target) => ({ id: author, handle: "Server authoritative handle", kind: "Person", created_at: "2026-09-29T12:00:00Z", public_key: {}, signature: {} });
const object = (key = "1", date = "2026-09-29T12:00:00Z") => ({ id: `obj_${key.repeat(64)}`, author: target,
  created_at: date, kind: "text", schema: "babble.text.v1", protocol: { name: "babble", version: 1 }, payload: { text: "Actual post" },
  provenance: { parent: null, forked_from: null, remixed_from: [] }, relations: [], resources: [], surfaces: [], capabilities: [] });
const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
const settle = () => new Promise((resolve) => setImmediate(resolve));

test("Following transport is authenticated REST GET/PUT with bounded queries, no cookies, author payload, history, discovery or ranking", async () => {
  const requests = [];
  const values = [state(), state(true, 1), { identities: [identity()], next_cursor: null }, { objects: [object()], next_cursor: "next" }];
  const client = new FollowingClient("https://babble.test", async (url, init) => {
    requests.push({ url, init }); return Response.json(values.shift());
  });
  const signal = new AbortController().signal;
  await client.state(owner, target, signal);
  const intent = { following: true, expected_revision: 0, idempotency_key: "intent-key" };
  await client.update(owner, target, intent, signal);
  await client.list("cursor|one", signal);
  const page = await client.feed(" exact search ", "cursor|two", signal);
  assert.equal(page.objects[0].id, object().id);
  assert.deepEqual(requests.map((r) => r.url.pathname), [`/social/following/${target}`, `/social/following/${target}`, "/social/following", "/feed/following"]);
  assert.equal(requests[1].init.method, "PUT");
  assert.deepEqual(JSON.parse(requests[1].init.body), intent);
  assert.equal(requests[2].url.searchParams.get("cursor"), "cursor|one");
  assert.equal(requests[3].url.searchParams.get("search"), "exact search");
  assert.equal(requests[3].url.searchParams.get("limit"), "20");
  for (const { init } of requests) {
    assert.equal(init.credentials, "omit"); assert.equal(init.redirect, "error"); assert.equal(init.cache, "no-store");
    assert.equal(init.signal.aborted, false);
  }
});

test("boundary rejects unsafe revisions, cross-account states, oversized pages and malformed rows", () => {
  for (const value of [null, {}, state(false, -1), state(false, 1.5), state(false, Number.MAX_SAFE_INTEGER + 1), state(false, 0, other), state(false, 0, owner, other)]) {
    assert.throws(() => following.parseFollowState(value, owner, target), /invalid Following/);
  }
  for (const value of [null, { objects: Array(51).fill(object()), next_cursor: null },
    { objects: [{ ...object(), author: "untrusted" }], next_cursor: null },
    { objects: [{ ...object(), resources: [null] }], next_cursor: null },
    { objects: [object("1", "2026-09-28T00:00:00Z"), object("2")], next_cursor: null },
    { objects: [], next_cursor: "x".repeat(2049) }]) assert.throws(() => following.parseFollowingPage(value), /invalid Following/);
  assert.equal(following.parseFollowingPage({ objects: [], next_cursor: "x".repeat(2048) }).next_cursor.length, 2048);
  assert.throws(() => following.parseFollowList({ identities: [{ id: target, handle: "Fake" }], next_cursor: null }), /invalid Following/);
  assert.equal(following.parseFollowList({ identities: [identity()], next_cursor: null }).identities[0].handle, identity().handle);
  assert.deepEqual([...following.parseFollowingPage({ objects: [], next_cursor: null }).objects], []);
});

test("HTTP errors preserve 409/503 classification without displaying upstream diagnostics", async () => {
  for (const status of [401, 404, 409, 503]) {
    for (const mode of ["response", "throw"]) {
      const client = new FollowingClient("https://babble.test", async () => {
        if (mode === "throw") throw Object.assign(new Error("private diagnostic"), { status });
        return new Response("private diagnostic", { status });
      });
      await assert.rejects(client.state(owner, target, new AbortController().signal), (e) => e.status === status && !e.message.includes("private diagnostic"));
    }
  }
  const client = new FollowingClient("https://babble.test", async () => new Response("<html>bad response</html>"));
  await assert.rejects(client.feed("", null, new AbortController().signal), /Check your connection/);
});

function authorHarness() {
  const reads = [], writes = [], changes = [], mutations = [];
  const source = {
    state(owner, target, signal) { const request = { ...deferred(), owner, target, signal }; reads.push(request); return request.promise; },
    update(owner, target, intent, signal) { const request = { ...deferred(), owner, target, intent, signal }; writes.push(request); return request.promise; },
  };
  let key = 0;
  const control = new AuthorFollow(source, (value) => changes.push(value), () => mutations.push(true), () => `key-${++key}`);
  return { control, reads, writes, changes, mutations };
}
async function ready(h) { h.control.show(owner, target); h.reads[0].resolve(state()); await settle(); }

test("guests and self never request private state or mutate", async () => {
  const h = authorHarness();
  h.control.show(null, target); await h.control.toggle();
  h.control.show(owner, owner); await h.control.toggle();
  assert.equal(h.reads.length + h.writes.length, 0);
});

test("pending disables duplicate intent; fresh GET overrides stale durable mutation receipt", async () => {
  const h = authorHarness(); await ready(h);
  const operation = h.control.toggle();
  assert.equal(h.control.view.pending, true);
  await h.control.toggle(); assert.equal(h.writes.length, 1);
  h.writes[0].resolve(state(true, 1)); await settle();
  assert.equal(h.control.view.state.following, false);
  assert.equal(h.control.view.pending, true);
  h.reads[1].resolve(state(false, 2)); await operation;
  assert.equal(h.control.view.state.revision, 2);
  assert.equal(h.control.view.state.following, false);
  assert.equal(h.control.view.pending, false);
  assert.equal(h.control.view.retry, null);
});

test("network timeout and storage 503 retain exact intent key and CAS revision on retry", async () => {
  for (const failure of [new Error("Timed out"), new FollowingError(503)]) {
    const h = authorHarness(); await ready(h);
    const first = h.control.toggle();
    h.writes[0].reject(failure); await settle();
    h.reads[1].resolve(state(true, 1)); await first;
    assert.equal(h.control.view.retry, true);
    const retry = h.control.toggle();
    assert.deepEqual(h.writes[1].intent, h.writes[0].intent);
    h.writes[1].resolve(state(true, 1)); await settle();
    h.reads[2].resolve(state(false, 2)); await retry;
    assert.equal(h.control.view.state.revision, 2);
    assert.equal(h.control.view.retry, null);
  }
});

test("409 refreshes current state and explains conflict without overwriting; next choice gets fresh revision/key", async () => {
  const h = authorHarness(); await ready(h);
  const first = h.control.toggle();
  h.writes[0].reject(new FollowingError(409)); await settle();
  h.reads[1].resolve(state(true, 3)); await first;
  assert.match(h.control.view.message, /changed elsewhere/);
  assert.equal(h.control.view.retry, null);
  assert.equal(h.writes.length, 1);
  const next = h.control.toggle();
  assert.equal(h.writes[1].intent.expected_revision, 3);
  assert.equal(h.writes[1].intent.following, false);
  assert.notEqual(h.writes[0].intent.idempotency_key, h.writes[1].intent.idempotency_key);
  h.writes[1].resolve(state(false, 4)); await settle(); h.reads[2].resolve(state(false, 4)); await next;
});

test("refresh failure after accepted mutation cannot display an obsolete receipt or issue an unsafe new mutation", async () => {
  const h = authorHarness(); await ready(h);
  const operation = h.control.toggle(); h.writes[0].resolve(state(true, 1)); await settle();
  h.reads[1].reject(new Error("Offline")); await operation;
  assert.equal(h.control.view.state, null);
  assert.match(h.control.view.message, /Change saved/);
  const refresh = h.control.toggle(); assert.equal(h.writes.length, 1);
  h.reads[2].resolve(state(false, 2)); await refresh;
});

test("profile/account changes abort and suppress stale reads and writes, clearing private intent on logout", async () => {
  const h = authorHarness(); await ready(h);
  const operation = h.control.toggle();
  h.control.show(other, target);
  assert.equal(h.writes[0].signal.aborted, true);
  h.writes[0].resolve(state(true, 1)); await operation;
  assert.equal(h.mutations.length, 0);
  assert.equal(h.reads.length, 2); // no stale old-account follow-up GET
  h.reads[1].resolve(state(false, 0, other)); await settle();
  assert.equal(h.control.view.owner, other);
  h.control.show(other, owner);
  const stale = h.reads[2]; h.control.show(null, null); stale.resolve(state(true, 1, other, owner)); await settle();
  assert.equal(stale.signal.aborted, true);
  assert.equal(h.control.view.state, null);
  assert.equal(h.control.view.retry, null);
});

function pageHarness() {
  const requests = [], changes = [];
  const pages = new FollowingPages((cursor, query, signal) => {
    const request = { ...deferred(), cursor, query, signal }; requests.push(request); return request.promise;
  }, (view) => changes.push(view));
  return { pages, requests, changes };
}
test("empty following is authoritative, pagination preserves order and retries same cursor without duplicates", async () => {
  const h = pageHarness(); const start = h.pages.start(owner, "search");
  h.requests[0].resolve({ items: [], next: null }); await start;
  await h.pages.more(); assert.equal(h.requests.length, 1);
  const restart = h.pages.start(owner); h.requests[1].resolve({ items: [{ id: "b" }, { id: "a" }], next: "next" }); await restart;
  const more = h.pages.more(); await h.pages.more(); assert.equal(h.requests.length, 3);
  h.requests[2].reject(new Error("Offline")); await more;
  const retry = h.pages.more(); assert.equal(h.requests[3].cursor, "next");
  h.requests[3].resolve({ items: [{ id: "a" }, { id: "c" }], next: null }); await retry;
  assert.deepEqual(Array.from(h.pages.view.items, (item) => item.id), ["b", "a", "c"]);
});
test("snapshot 409 and repeated cursors require explicit Refresh from page one", async () => {
  for (const failure of ["conflict", "cycle"]) {
    const h = pageHarness(); const start = h.pages.start(owner);
    h.requests[0].resolve({ items: [{ id: "a" }], next: "next" }); await start;
    const more = h.pages.more();
    if (failure === "conflict") h.requests[1].reject(new FollowingError(409));
    else h.requests[1].resolve({ items: [{ id: "bad" }], next: "next" });
    await more;
    assert.equal(h.pages.view.restart, true);
    assert.equal(h.pages.view.items.length, 1);
    const refresh = h.pages.more(); assert.equal(h.requests[2].cursor, null); assert.equal(h.pages.view.items.length, 0);
    h.requests[2].resolve({ items: [], next: null }); await refresh;
  }
});
test("short and empty filtered pages keep Load more available until cursor is null", async () => {
  const h = pageHarness(); const start = h.pages.start(owner);
  h.requests[0].resolve({ items: [], next: "scan-next" }); await start;
  assert.equal(h.pages.view.next, "scan-next");
  const more = h.pages.more(); h.requests[1].resolve({ items: [{ id: "found" }], next: "scan-again" }); await more;
  assert.equal(h.pages.view.next, "scan-again");
  const last = h.pages.more(); h.requests[2].resolve({ items: [], next: null }); await last;
  assert.equal(h.pages.view.next, null); assert.equal(h.pages.view.items.length, 1);
});
test("account, search, feed-mode and logout generations immediately clear private pages and discard late results", async () => {
  const h = pageHarness(); const first = h.pages.start(owner, "old");
  const second = h.pages.start(other, "new"); assert.equal(h.requests[0].signal.aborted, true);
  h.requests[0].resolve({ items: [{ id: "private-old" }], next: "old-cursor" }); await first;
  h.requests[1].resolve({ items: [{ id: "current" }], next: "next" }); await second;
  assert.equal(h.pages.view.items[0].id, "current");
  const more = h.pages.more(); await h.pages.start(null);
  assert.equal(h.pages.view.phase, "guest"); assert.equal(h.pages.view.items.length, 0);
  h.requests[2].resolve({ items: [{ id: "stale" }], next: null }); await more;
  assert.equal(h.pages.view.items.length, 0);
  const changedSearch = h.pages.start(owner, "new search"); h.pages.clear();
  h.requests[3].reject(new Error("Old search failure")); await changedSearch;
  assert.equal(h.pages.view.phase, "idle");
});

test("production Accounts bearer transport rejects guests and responses from previous sessions", async () => {
  const session = { token: "t".repeat(64), identity: { id: owner, handle: "Reader" }, expires_at: "2099-01-01T00:00:00Z" };
  const store = { getItem: () => JSON.stringify(session), setItem() {}, removeItem() {} };
  const pending = deferred(); let received;
  const accounts = new Accounts("https://babble.test", store, async (url, init) => {
    if (url.pathname === "/auth/session") return new Response(null, { status: 204 });
    received = init; return pending.promise;
  });
  const request = accounts.authenticatedFetch(new URL("https://babble.test/social/following"));
  assert.equal(received.headers.get("authorization"), `Bearer ${session.token}`);
  assert.equal(received.credentials, "omit");
  await accounts.logout(); pending.resolve(Response.json({ identities: [identity()], next_cursor: null }));
  await assert.rejects(request, /account changed/);
  await assert.rejects(accounts.authenticatedFetch(new URL("https://babble.test/social/following")), /Sign in/);
});

const main = ts.createSourceFile("main.ts", readFileSync(new URL("../src/app/main.ts", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true);
function mainFunctions(names, context) {
  const statements = main.statements.filter((node) => ts.isFunctionDeclaration(node) && names.includes(node.name?.text));
  assert.equal(statements.length, names.length);
  vm.runInNewContext(ts.transpileModule(statements.map((node) => node.getText(main)).join("\n"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022 },
  }).outputText, context);
  return context;
}
test("production main Following branch never invokes discovery or local model, including an empty feed and guest", async () => {
  for (const account of [{ identity: { id: owner } }, null]) {
    let started, cleared = 0;
    const element = () => ({ hidden: false, textContent: "", replaceChildren() {} });
    const h = mainFunctions(["loadFeed"], {
      objectVisits: { clear() {} }, quotes: { refresh() {} },
      profiles: { close() {} }, followingDirty: true, loadSequence: 0, followingToolbar: element(), followingMore: element(),
      reactions: { select: (object) => assert.equal(object, null) },
      followingSignIn: element(), followingStatus: element(), followingFeed: { clear() { cleared++; }, async start(...args) { started = args; } },
      activeLens: "following", cards: [{ id: "old" }], currentIndex: 2, deck: element(), closeSurface() {}, closeJudgments() {}, syncSearchUrl() {},
      accounts: { current: account }, client: { loadFeed() { assert.fail("discovery called"); } }, localModel() { assert.fail("local ranking called"); },
    });
    await h.loadFeed("  exact search  ");
    assert.equal(started[0], account?.identity.id ?? null); assert.equal(started[1], "exact search");
    assert.equal(h.cards.length, 0); assert.equal(h.currentIndex, 0); assert.equal(cleared, 1);
  }
});
test("production main local filters preserve order and only use explicit author/term exclusions", () => {
  const h = mainFunctions(["filterFollowingCards"], { createPersonalizationFilter, summarizeDiscoveryObject });
  const card = (id, text, author = target) => ({ id, author, object: { ...object(), id, author, payload: { text } } });
  const input = [card("new", "Fresh news"), card("middle", "muted token"), card("hidden", "news", other), card("old", "Older news")];
  const result = h.filterFollowingCards(input, { hiddenAuthors: [other], hiddenTerms: [], mutedTerms: ["MUTED"], creatorAffinity: { [other]: 1000 } });
  assert.deepEqual(Array.from(result, (item) => item.id), ["new", "old"]);
});

test("production discovery client explicitly rejects Following before any fallback or catalog request", async () => {
  const invocations = module("invocations", () => sdk);
  const protocol = module("protocol", (name) => name === "./invocations" ? invocations
    : name === "./profile-response" ? response : { HttpRpcTransport: class {}, hostBinding: () => ({}) });
  const client = new protocol.BabbleFrontendClient("https://babble.test", async () => assert.fail("unexpected fetch"));
  await assert.rejects(client.loadFeed("", "following"), /authenticated chronological feed/);
});

class Element {
  constructor(document) {
    Object.assign(this, { document, children: [], attrs: {}, dataset: {}, hidden: false, disabled: false, open: false, events: {}, text: "" });
    this.children.item = (index) => this.children[index] ?? null;
  }
  append(...items) { this.children.push(...items); }
  replaceChildren(...items) { this.children.splice(0, this.children.length, ...items); this.text = ""; }
  set textContent(value) { this.replaceChildren(); this.text = value; }
  get textContent() { return this.text + this.children.map((child) => child.textContent).join(""); }
  set innerHTML(_) { assert.fail("Unsafe HTML insertion"); }
  setAttribute(name, value) { this.attrs[name] = value; }
  addEventListener(type, callback) { (this.events[type] ??= []).push(callback); }
  click() { if (!this.disabled) for (const callback of this.events.click ?? []) callback(); }
  focus() { this.document.activeElement = this; }
  showModal() { this.open = true; }
  close() { this.open = false; }
}
function controlsHarness() {
  const document = { activeElement: null };
  document.createElement = () => new Element(document);
  const nodes = new Map(); document.querySelector = (selector) => { if (!nodes.has(selector)) nodes.set(selector, new Element(document)); return nodes.get(selector); };
  const reads = [], lists = [], opened = [], signIns = [];
  const source = {
    state(owner, target, signal) { const request = { ...deferred(), owner, target, signal }; reads.push(request); return request.promise; },
    update() { assert.fail("Guest navigation must not fabricate a follow"); },
    list(cursor, signal) { const request = { ...deferred(), cursor, signal }; lists.push(request); return request.promise; },
  };
  const { FollowingControls } = module("following-view", () => following, { document, HTMLElement: Element });
  const controls = new FollowingControls(source, () => { signIns.push(true); controls.profile(null); }, (...args) => opened.push(args), () => {});
  return { controls, document, node: (selector) => document.querySelector(selector), reads, lists, opened, signIns };
}
test("production Follow control sends guests to sign-in, then reopens and reads the correct target for the new account", async () => {
  const h = controlsHarness();
  h.controls.profile(target);
  const button = h.node("[data-author-follow]");
  assert.equal(button.textContent, "Follow"); assert.equal(button.hidden, false);
  button.click(); assert.equal(h.signIns.length, 1); assert.equal(h.reads.length, 0);
  h.controls.account(owner); h.controls.resumeAfterSignIn();
  assert.equal(h.opened[0][0], target);
  h.controls.profile(target); assert.equal(h.reads[0].owner, owner); assert.equal(button.disabled, true);
  h.reads[0].resolve(state()); await settle(); assert.equal(button.textContent, "Follow");
  h.controls.profile(owner); assert.equal(button.hidden, true); assert.equal(h.reads.length, 1);
});
test("production private list renders authoritative handles as text and clears/aborts on account switch", async () => {
  const h = controlsHarness(); h.controls.account(owner);
  const opener = h.document.createElement("button"); h.controls.openList(opener);
  h.lists[0].resolve({ identities: [identity()], next_cursor: "next" }); await settle();
  const list = h.node("[data-following-list]");
  assert.equal(list.children[0].children[0].textContent, identity().handle);
  assert.equal(list.children[0].dataset.followingAuthor, target);
  h.node("[data-following-list-more]").click();
  h.controls.account(other);
  assert.equal(h.lists[1].signal.aborted, true); assert.equal(list.children.length, 0);
  assert.equal(h.node("[data-following-dialog]").open, false);
  h.lists[1].resolve({ identities: [identity(other)], next_cursor: null }); await settle();
  assert.equal(list.children.length, 0);
  h.controls.openList(opener); h.lists[2].resolve({ identities: [identity(target)], next_cursor: null }); await settle();
  list.children[0].click(); assert.equal(h.opened[0][0], target); assert.equal(h.node("[data-following-dialog]").open, false);
});

test("blocked profiles cannot be followed, but existing follows can still be removed", async () => {
  for (const following of [false, true]) {
    const h = controlsHarness();
    h.controls.account(owner); h.controls.profile(target); h.controls.restrict(true);
    h.reads[0].resolve(state(following, 1)); await settle();
    const button = h.node("[data-author-follow]");
    assert.equal(button.disabled, !following);
    assert.equal(button.textContent, following ? "Following" : "Blocked");
    h.controls.restrict(false);
    assert.equal(button.disabled, false);
    assert.equal(button.textContent, following ? "Following" : "Follow");
    h.controls.restrict(true); h.controls.account(other);
    h.controls.profile(target); h.reads[1].resolve(state(false, 0, other)); await settle();
    assert.equal(button.disabled, false, "the previous account's block does not survive account replacement");
  }
});

test("a blocked profile still permits retrying failed follow-state reads", async () => {
  const h = controlsHarness(); h.controls.account(owner); h.controls.profile(target); h.controls.restrict(true);
  h.reads[0].reject(new Error("offline")); await settle();
  h.controls.author.view = { ...h.controls.author.view, retry: true };
  h.controls.restrict(true);
  h.controls.author.toggle = () => assert.fail("Unknown blocked state must not retry an ambiguous follow write");
  const button = h.node("[data-author-follow]");
  assert.equal(button.disabled, false); assert.equal(button.textContent, "Refresh follow state");
  button.click(); assert.equal(h.reads.length, 2);
  h.reads[1].resolve(state(true, 1)); await settle();
  assert.equal(button.disabled, false); assert.equal(h.controls.author.view.state.following, true);
  assert.equal(button.textContent, "Retry follow", "unresolved receipt remains explicit after the fresh read");
});

test("production account integration immediately clears private deck and invalidates pending feed generations", () => {
  const calls = [];
  const h = mainFunctions(["syncAccount"], {
    feedPreferences: { account() {} },
    safetyControls: { account: owner => calls.push(["safety", owner]) }, conversations: { clear: () => calls.push(["conversations-clear"]) },
    moderationControls: { account: owner => calls.push(["moderation", owner]) },
    safetyUnavailable: false,
    accounts: { current: { identity: { id: other, handle: "Other" } }, origin: new URL("https://babble.test") },
    drafts: { setOwner: () => false }, author: { identityId: owner }, followingDirty: true,
    composerView: { render() {} }, objectVisits: { clear() {} }, quotes: { clear() {}, refresh() {} },
    reactions: { select: (object) => calls.push(["reactions", object]) },
    followingControls: { account: (owner) => calls.push(["account", owner]) }, profiles: { close() {} },
    authorHandleInput: {}, setAuthorStatus() {}, closeSurface() {}, profileDropdown: null, toggleProfileButton: null,
    loadSequence: 5, followingFeed: { clear: () => calls.push(["clear"]) }, profileReturn: { cards: [{ id: "private" }] },
    cards: [{ id: "private" }], currentIndex: 4, deck: { replaceChildren: () => calls.push(["deck"]) }, seenThisSession: new Set(["private"]),
    searchInput: { value: "search" }, accountFeedRefresh: null, loadFeed: () => Promise.resolve(),
  });
  h.syncAccount();
  assert.equal(h.loadSequence, 6); assert.equal(h.cards.length, 0); assert.equal(h.profileReturn, null); assert.equal(h.seenThisSession.size, 0);
  assert.deepEqual(calls, [["safety", other], ["moderation", other], ["account", other], ["conversations-clear"], ["reactions", null], ["clear"], ["deck"]]);
});

test("production profile return incorporates pages that arrived while reading a profile Object", () => {
  const statements = main.statements.filter((node) => ts.isVariableStatement(node)
    && node.declarationList.declarations.some((declaration) => ["profileReturn", "profiles"].includes(declaration.name.getText(main))));
  let callbacks;
  const original = [{ id: "first" }, { id: "reading" }];
  const paged = [...original, { id: "older" }];
  const current = { scrollTop: 123, focus() {} };
  const h = {
    Profiles: class { constructor(_dialog, _source, select, closed) { callbacks = { select, closed }; } },
    profileTargetChanged() {},
    objectVisits: { active: false, checkpoint: () => [], clear() {}, restore() {} },
    required: (value) => value, document: { querySelector: () => ({}) }, client: {},
    cards: original, currentIndex: 1, loadSequence: 3, profileBack: { hidden: true }, followingDirty: false,
    followingToolbar: { hidden: false }, activeLens: "following", followingFeed: { view: { items: paged } },
    renderFollowingFeed(view) { h.cards = view.items; },
    sourceLabel: { textContent: "following" }, error: { hidden: true }, deck: { querySelector: () => current },
    captureReading: () => ({ top: current.scrollTop }), restoreReading: (_, position) => { current.scrollTop = position.top; },
    closeSurface() {}, closeJudgments() {}, render() { current.scrollTop = 0; }, setStatus() {},
  };
  vm.runInNewContext(ts.transpileModule(statements.map((node) => node.getText(main)).join("\n"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022 },
  }).outputText, h);
  callbacks.select({ id: "profile" }, [{ id: "profile" }]);
  assert.equal(h.followingToolbar.hidden, true);
  callbacks.closed();
  assert.equal(h.cards, paged); assert.equal(h.currentIndex, 1); assert.equal(current.scrollTop, 123);
  assert.equal(h.followingToolbar.hidden, false);
});
