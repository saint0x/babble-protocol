import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import { createPersonalizationFilter } from "@babel-protocol/sdk";

const source = (name) => readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8");
const main = ts.createSourceFile("main.ts", source("main"), ts.ScriptTarget.Latest, true);
const transpile = (text) => ts.transpileModule(text, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;
function module(name, globals = {}, require = () => { throw new Error(`Unexpected import in ${name}`); }) {
  const context = { exports: {}, AbortController, Error, require, ...globals };
  vm.runInNewContext(transpile(source(name)), context);
  return context.exports;
}
const { ObjectVisits } = module("object-visits");
const reading = module("reading");
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};
const settle = () => new Promise((resolve) => setImmediate(resolve));
const card = (id, author = "public-author") => ({ id, author, title: `Post ${id}`, content: id,
  createdAt: "2026-09-30T12:00:00Z", object: { payload: { text: id } }, lineage: [], relations: [], surfaces: [] });
const readyPage = (items, next = null) => ({ items, phase: "ready", next, message: "", restart: false });
const ids = (cards) => Array.from(cards, (entry) => entry.id);

class Element {
  constructor(document) {
    Object.assign(this, { document, children: [], refs: {}, dataset: {}, attrs: {}, events: {},
      hidden: false, disabled: false, connected: true, parent: null, open: false,
      scrollTop: 0, clientHeight: 400, offsetTop: 0, offsetHeight: 100, offsetParent: null, text: "" });
  }
  get isConnected() { return this.connected && (!this.parent || this.parent.isConnected); }
  append(...children) { for (const child of children) { child.parent = this; this.children.push(child); } }
  replaceChildren(...children) {
    for (const child of this.children) { child.parent = null; child.connected = false; }
    this.children = []; this.text = ""; this.append(...children);
  }
  set textContent(value) { this.replaceChildren(); this.text = value ?? ""; }
  get textContent() { return this.text + this.children.map((child) => child.textContent).join(""); }
  setAttribute(name, value) { this.attrs[name] = value; }
  removeAttribute(name) { delete this.attrs[name]; }
  querySelector(selector) { return this.refs[selector] ?? this.querySelectorAll(selector)[0] ?? null; }
  querySelectorAll(selector) {
    const matches = (element) => selector === "[data-reading-anchor]" ? element.dataset.readingAnchor !== undefined
      : selector === "[aria-current]" ? element.attrs["aria-current"] !== undefined
        : selector === "[aria-current='true']" && element.attrs["aria-current"] === "true";
    return this.children.flatMap((child) => [...(matches(child) ? [child] : []), ...child.querySelectorAll(selector)]);
  }
  closest(selector) {
    assert.equal(selector, "[inert], [hidden]");
    return this.hidden || this.attrs.inert !== undefined ? this : this.parent?.closest(selector) ?? null;
  }
  addEventListener(type, callback) { (this.events[type] ??= []).push(callback); }
  click() { if (!this.disabled) for (const callback of this.events.click ?? []) callback({ currentTarget: this }); }
  focus(options) { this.document.activeElement = this; this.focusOptions = options; }
  showModal() { this.open = true; }
  close() { this.open = false; }
}

// Execute main's initializers, callbacks and helpers, not a copy of their logic.
const variables = ["profileReturn", "objectVisits", "profiles"];
const functions = ["openPublicProfile", "loadFeed", "renderFollowingFeed", "filterFollowingCards", "syncAccount"];
const controls = ["objectBack", "profileBack", "refreshButton"];
const statements = main.statements.filter((node) => {
  if (ts.isVariableStatement(node)) return node.declarationList.declarations.some((entry) => variables.includes(entry.name.getText(main)));
  if (ts.isFunctionDeclaration(node)) return functions.includes(node.name?.text);
  if (ts.isForOfStatement(node)) return ts.isIdentifier(node.expression) && node.expression.text === "lensButtons";
  if (!ts.isExpressionStatement(node) || !ts.isCallExpression(node.expression)) return false;
  const callee = node.expression.expression;
  return ts.isPropertyAccessExpression(callee) && callee.name.text === "addEventListener"
    && ts.isIdentifier(callee.expression) && controls.includes(callee.expression.text);
});
assert.equal(statements.length, variables.length + functions.length + controls.length + 1,
  "Navigation extraction must include every production initializer, helper and listener");

function harness({ lens = "balanced", guest = false } = {}) {
  const document = { activeElement: null, createElement: () => new Element(document) };
  const node = () => document.createElement();
  const dialog = node();
  const profileNodes = Object.fromEntries(["title", "identity", "status", "list", "more", "close"].map((key) => [key, node()]));
  dialog.refs = Object.fromEntries(Object.entries(profileNodes).map(([key, value]) => [`[data-public-profile-${key}]`, value]));
  document.querySelector = (selector) => {
    assert.equal(selector, "[data-public-profile]");
    return dialog;
  };
  const { Profiles } = module("profiles", { document, HTMLElement: Element }, () => ({ ProfileRequestError: class extends Error {} }));
  const initial = [card("first"), card("reading"), card("last")];
  const calls = [], requests = [], feedRequests = [], elements = new Map();
  let current = null;
  const h = {
    feedPreferences: { account() {} }, updateEmptyFeed() {}, createPersonalizationFilter,
    safetyControls: { account() {}, ensure: async () => null }, safetyHiddenAuthors: () => [], moderationControls: { account() {} },
    safetyUnavailable: false, ensureFeedSafety: async () => {},
    conversations: { clear: () => calls.push(["conversations-clear"]) },
    profileTargetChanged: id => calls.push(["profile-target", id]),
    refreshAccountFeed: () => h.loadFeed(h.searchInput.value, h.cards[h.currentIndex]?.id),
    ObjectVisits, Profiles, document, HTMLElement: Element, required: (value) => value, ...reading,
    cards: initial, currentIndex: 1, loadSequence: 4, activeLens: lens, followingDirty: false,
    author: guest ? null : { identityId: "owner", handle: "Owner" },
    accounts: { current: guest ? null : { identity: { id: "owner", handle: "Owner" } }, origin: new URL("https://babel.test"),
      fetch() { assert.fail("Object navigation must not issue authenticated transport requests"); } },
    client: {
      publicIdentity(id, signal) { calls.push(["identity", id]); return Promise.resolve({ id, handle: id }); },
      profileObjects(id, cursor, signal) { const request = { ...deferred(), id, cursor, signal }; requests.push(request); return request.promise; },
      loadFeed(query, lens) { const request = { ...deferred(), query, lens }; feedRequests.push(request); return request.promise; },
    },
    sourceLabel: Object.assign(node(), { textContent: lens === "following" ? "following" : "discovery" }),
    objectBack: Object.assign(node(), { hidden: true }), profileBack: Object.assign(node(), { hidden: true }),
    followingToolbar: Object.assign(node(), { hidden: lens !== "following" }),
    followingMore: node(), followingSignIn: node(), followingStatus: node(), error: node(), empty: node(),
    feedSummary: node(), catalogLabel: node(), localLabel: node(), diversityLabel: node(), lensLabel: node(),
    refreshButton: node(), searchInput: { value: "" }, authorHandleInput: node(),
    profileDropdown: null, toggleProfileButton: null, surfacePlacement: null,
    seenThisSession: new Set(["reading"]), accountFeedRefresh: null,
    localPersonalization: {}, feedDiversity: {},
    drafts: { setOwner: () => false, current: null }, composerView: { render() {} },
    accountPanel: { open: () => assert.fail("Public Object visits cannot request sign in") },
    reactions: { select: (id) => calls.push(["reaction", id]) },
    followingControls: { profile: (id) => calls.push(["profile-target", id]), account: (id) => calls.push(["account", id]) },
    followingFeed: {
      view: readyPage(initial),
      clear() { this.view = { ...readyPage([]), phase: "idle" }; calls.push(["following-clear"]); },
      async start(owner, query) { calls.push(["following-start", owner, query]); },
    },
    quotes: { clear: () => calls.push(["quotes-clear"]), refresh: (id) => calls.push(["quotes-refresh", id]) },
    deck: {
      querySelector(selector) {
        assert.match(selector, /\.post-card\[data-offset="0"\]:not\(\[data-exiting\]\)/);
        return current;
      },
      replaceChildren() { if (current) current.connected = false; current = null; calls.push(["deck-clear"]); },
    },
    render() {
      if (current) current.connected = false;
      const id = h.cards[h.currentIndex]?.id;
      current = id ? elements.get(id) ?? node() : null;
      if (current) { elements.set(id, current); current.connected = true; current.scrollTop = 0; }
      calls.push(["render", ids(h.cards), h.currentIndex]);
    },
    closeSurface: () => calls.push(["close-surface"]), closeJudgments: () => calls.push(["close-judgments"]),
    toggleComposer: (open) => calls.push(["composer", open]), toggleProfile: (open) => calls.push(["profile", open]),
    toggleSettings: (open) => calls.push(["settings", open]), toggleHelp: (open) => calls.push(["help", open]),
    setStatus: (...args) => calls.push(["status", ...args]), setAuthorStatus() {},
    syncSearchUrl() {}, syncSettingsStatus() {}, syncLensButtons() {},
    loadLocalPreferences: () => ({ hiddenAuthors: [], hiddenTerms: [], mutedTerms: [] }),
    summarizeDiscoveryObject: (object) => ({ kind: "text", text: object.payload.text, topics: [] }),
    localModel: () => null, lensName: (lens) => lens, lensMode: (value) => value,
    personalizationLabel: () => "Personalization", diversityLabelText: () => "Diversity",
    showError: (message) => assert.fail(message), feedErrorMessage: (cause) => cause.message,
    lensButtons: ["balanced", "following"].map((lens) => Object.assign(node(), { dataset: { lens } })),
  };
  const context = vm.createContext(h);
  vm.runInContext(transpile(statements.map((node) => node.getText(main)).join("\n")), context);
  const integration = vm.runInContext("({ objectVisits, profiles, get profileReturn() { return profileReturn; } })", context);
  h.render(); current.scrollTop = 240; calls.length = 0;
  const opener = node(); current.append(opener);
  return { h, integration, calls, requests, feedRequests, initial, opener, node, dialog, profileNodes,
    current: () => current, visit: (post = card("original"), control = opener) => integration.objectVisits.open(post, control) };
}

async function selectProfile(t, cards = [card("profile-a"), card("profile-b")], index = 1) {
  t.h.openPublicProfile("public-author", t.current());
  await settle();
  const request = t.requests.at(-1);
  request.resolve({ identity: { id: request.id, handle: request.id }, cards, nextCursor: null });
  await settle();
  t.profileNodes.list.children[index].click();
}
function resolveFeed(t, cards = [card("fresh")]) {
  t.feedRequests.at(-1).resolve({ cards, personalization: {}, diversity: {}, catalogMethods: 9, source: "refreshed" });
}

test("main Object visit closes competing panels, preserves the feed snapshot, and back restores anchor reading and opener", () => {
  const t = harness();
  const anchor = t.node();
  Object.assign(anchor, { dataset: { readingAnchor: "comments" }, offsetParent: t.current(), offsetTop: 300 });
  t.current().append(anchor);
  t.visit();
  assert.deepEqual(t.calls.slice(0, 4), [["close-surface"], ["close-judgments"], ["composer", false], ["profile", false]]);
  assert.equal(t.h.loadSequence, 5);
  assert.deepEqual(ids(t.h.cards), ["original"]);
  assert.equal(t.h.currentIndex, 0);
  assert.equal(t.h.sourceLabel.textContent, "Shared post");
  assert.equal(t.h.objectBack.hidden, false);
  assert.equal(t.current().scrollTop, 0);
  assert.equal(t.h.document.activeElement, t.current());
  anchor.offsetTop = 360; // Reading follows the anchor after a changed layout, not just a raw scroll value.
  t.h.objectBack.click();
  assert.equal(t.h.cards, t.initial);
  assert.equal(t.h.currentIndex, 1);
  assert.equal(t.h.sourceLabel.textContent, "discovery");
  assert.equal(t.current().scrollTop, 300);
  assert.equal(t.h.document.activeElement, t.opener);
  assert.equal(t.opener.focusOptions.preventScroll, true);
  assert.equal(t.h.objectBack.hidden, true);
  assert.equal(t.h.loadSequence, 6);
});

for (const unavailable of ["detached", "hidden", "inert"]) {
  test(`main back focuses the restored card when the quote opener is ${unavailable}`, () => {
    const t = harness(); t.visit();
    if (unavailable === "detached") t.opener.connected = false;
    if (unavailable === "hidden") t.opener.hidden = true;
    if (unavailable === "inert") t.opener.setAttribute("inert", "");
    t.h.objectBack.click();
    assert.equal(t.h.document.activeElement, t.current());
    assert.equal(t.current().focusOptions.preventScroll, true);
    assert.equal(t.current().scrollTop, 240);
  });
}

test("Following pages arriving during a visit cannot replace the original; back adopts the latest page and keeps the reading Object", () => {
  const t = harness({ lens: "following" }); t.visit();
  const paged = readyPage([card("newest"), ...t.initial, card("older")], "next");
  t.h.followingFeed.view = paged;
  const renders = t.calls.filter(([kind]) => kind === "render").length;
  t.h.renderFollowingFeed(paged);
  assert.deepEqual(ids(t.h.cards), ["original"]);
  assert.equal(t.calls.filter(([kind]) => kind === "render").length, renders);
  assert.equal(t.h.followingToolbar.hidden, true);
  t.h.objectBack.click();
  assert.deepEqual(ids(t.h.cards), ids(paged.items));
  assert.equal(t.h.cards[t.h.currentIndex].id, "reading");
  assert.equal(t.h.currentIndex, 2);
  assert.equal(t.current().scrollTop, 240);
  assert.equal(t.h.followingToolbar.hidden, false);
  assert.equal(t.h.followingMore.hidden, false);
  assert.equal(t.h.sourceLabel.textContent, "following");
  assert.equal(t.h.document.activeElement, t.opener);
});

test("profile selection suspends nested quote history; returning restores it without overwriting its feed or reading positions", async () => {
  const t = harness(); t.visit(card("original")); t.current().scrollTop = 88;
  t.visit(card("nested"), null); t.current().scrollTop = 144;
  await selectProfile(t);
  assert.deepEqual(ids(t.h.cards), ["profile-a", "profile-b"]);
  assert.equal(t.h.currentIndex, 1);
  assert.equal(t.integration.objectVisits.active, false);
  assert.equal(t.h.objectBack.hidden, true);
  assert.equal(t.h.profileBack.hidden, false);
  assert.equal(t.integration.profileReturn.visits.length, 2);
  t.h.profileBack.click();
  assert.equal(t.dialog.open, true);
  assert.equal(t.h.document.activeElement, t.profileNodes.list.children[1]);
  t.profileNodes.list.children[0].click();
  assert.equal(t.h.currentIndex, 0);
  t.h.profileBack.click(); t.profileNodes.close.click();
  assert.deepEqual(ids(t.h.cards), ["nested"]);
  assert.equal(t.current().scrollTop, 144);
  assert.equal(t.integration.profileReturn, null);
  assert.equal(t.h.objectBack.hidden, false);
  t.h.objectBack.click();
  assert.deepEqual(ids(t.h.cards), ["original"]);
  assert.equal(t.current().scrollTop, 88);
  t.h.objectBack.click();
  assert.equal(t.h.cards, t.initial);
  assert.equal(t.current().scrollTop, 240);
});

test("returning to a failed Following page preserves its error status instead of reporting online", () => {
  const t = harness({ lens: "following" }); t.visit();
  t.h.followingFeed.view = { ...readyPage(t.initial), phase: "error", message: "Connection failed" };
  t.h.objectBack.click();
  assert.equal(t.h.cards[t.h.currentIndex].id, "reading");
  assert.deepEqual(t.calls.filter(([kind]) => kind === "status").at(-1), ["status", "error", "Following unavailable"]);
});

test("returning before Following has a live page still renders the saved snapshot", () => {
  const t = harness({ lens: "following" }); t.visit();
  t.h.followingFeed.view = { ...readyPage([]), phase: "idle" };
  t.h.objectBack.click();
  assert.equal(t.h.cards, t.initial);
  assert.equal(t.current().scrollTop, 240);
  assert.equal(t.h.document.activeElement, t.opener);
});

test("Following profile round-trip restores the quote before resuming newer private pages on final back", async () => {
  const t = harness({ lens: "following" }); t.visit(); t.current().scrollTop = 96;
  await selectProfile(t);
  const paged = readyPage([...t.initial, card("older")]);
  t.h.followingFeed.view = paged;
  t.h.renderFollowingFeed(paged);
  assert.deepEqual(ids(t.h.cards), ["profile-a", "profile-b"]);
  t.h.profileBack.click(); t.profileNodes.close.click();
  assert.deepEqual(ids(t.h.cards), ["original"]);
  assert.equal(t.current().scrollTop, 96);
  assert.equal(t.h.followingToolbar.hidden, true, "Following controls must remain hidden while the quote visit is restored");
  t.h.objectBack.click();
  assert.deepEqual(ids(t.h.cards), ids(paged.items));
  assert.equal(t.h.cards[t.h.currentIndex].id, "reading");
  assert.equal(t.h.followingToolbar.hidden, false);
});

test("syncAccount discards suspended quote history and cannot navigate back into the prior account's Following feed", async () => {
  const t = harness({ lens: "following" }); t.visit(); await selectProfile(t);
  t.h.accounts.current = { identity: { id: "other", handle: "Other" } };
  t.h.syncAccount(); await t.h.accountFeedRefresh;
  assert.equal(t.integration.profileReturn, null);
  assert.equal(t.integration.objectVisits.active, false);
  assert.equal(t.h.objectBack.hidden, true);
  assert.equal(t.h.profileBack.hidden, true);
  assert.deepEqual(ids(t.h.cards), []);
  assert.equal(t.current(), null);
  assert.equal(t.h.seenThisSession.size, 0);
  assert.ok(t.calls.some((entry) => entry[0] === "following-start" && entry[1] === "other"));
  t.h.objectBack.click(); t.h.profileBack.click(); t.profileNodes.close.click();
  assert.deepEqual(ids(t.h.cards), []);
  assert.equal(t.dialog.open, false);
});

test("account logout aborts an open profile request and suppresses late results after quote history is invalidated", async () => {
  const t = harness({ lens: "following" }); t.visit();
  t.h.openPublicProfile("public-author", t.current()); await settle();
  const pending = t.requests[0];
  t.h.accounts.current = null; t.h.syncAccount(); await t.h.accountFeedRefresh;
  assert.equal(pending.signal.aborted, true);
  pending.resolve({ identity: { id: "public-author", handle: "Public" }, cards: [card("stale")], nextCursor: null });
  await settle();
  assert.equal(t.profileNodes.list.children.length, 0);
  assert.equal(t.dialog.open, false);
  assert.equal(t.integration.objectVisits.active, false);
  assert.deepEqual(ids(t.h.cards), []);
  t.h.objectBack.click(); assert.deepEqual(ids(t.h.cards), []);
});

test("a changed Following relationship refreshes on profile close instead of reviving the suspended private navigation", async () => {
  const t = harness({ lens: "following" }); t.visit(); await selectProfile(t);
  t.h.followingDirty = true;
  t.h.profileBack.click(); t.profileNodes.close.click();
  await settle();
  assert.equal(t.h.followingDirty, false);
  assert.equal(t.integration.profileReturn, null);
  assert.equal(t.integration.objectVisits.active, false);
  assert.equal(t.h.objectBack.hidden, true);
  assert.deepEqual(ids(t.h.cards), []);
  assert.ok(t.calls.some((entry) => entry[0] === "following-start" && entry[1] === "owner"));
  const refreshed = readyPage([card("new-following")]);
  t.h.followingFeed.view = refreshed; t.h.renderFollowingFeed(refreshed);
  t.h.objectBack.click();
  assert.deepEqual(ids(t.h.cards), ["new-following"]);
});

test("refresh clears visit history and a subsequent quote visit invalidates the pending ranked response", async () => {
  const t = harness(); t.visit();
  t.h.refreshButton.click();
  assert.equal(t.integration.objectVisits.active, false);
  assert.equal(t.h.objectBack.hidden, true);
  await settle();
  assert.equal(t.feedRequests.length, 1);
  t.visit(card("another-original"));
  resolveFeed(t); await settle();
  assert.deepEqual(ids(t.h.cards), ["another-original"]);
  assert.equal(t.h.sourceLabel.textContent, "Shared post");
});

test("a stale ranked request failure cannot clear or mark a currently visited original offline", async () => {
  const t = harness();
  const loading = t.h.loadFeed("");
  await settle();
  t.visit();
  const before = t.calls.length;
  t.feedRequests[0].reject(new Error("Old feed disconnected"));
  await loading;
  assert.deepEqual(ids(t.h.cards), ["original"]);
  assert.equal(t.h.error.hidden, true);
  assert.equal(t.calls.length, before);
  assert.equal(t.h.objectBack.hidden, false);
});

test("lens controls clear quote history and show the new lens result without reviving an old feed on back", async () => {
  const t = harness({ lens: "following" }); t.visit();
  t.h.lensButtons[0].click();
  assert.equal(t.h.activeLens, "balanced");
  assert.equal(t.integration.objectVisits.active, false);
  await settle();
  assert.equal(t.feedRequests[0].lens, "balanced");
  resolveFeed(t); await settle();
  assert.deepEqual(ids(t.h.cards), ["fresh"]);
  assert.equal(t.h.sourceLabel.textContent, "refreshed");
  t.h.objectBack.click(); assert.deepEqual(ids(t.h.cards), ["fresh"]);
});

test("guests open and return from public quoted originals without authentication or Surface execution", () => {
  const t = harness({ guest: true });
  const interactive = { ...card("interactive-original"), surfaces: [{ id: "browser-app" }] };
  t.h.openSurface = () => assert.fail("Opening an original must not execute its Surface");
  t.h.client.prepareSurface = () => assert.fail("A public visit does not prepare capabilities");
  t.visit(interactive);
  assert.equal(t.h.cards[0], interactive);
  assert.equal(t.h.accounts.current, null);
  assert.equal(t.requests.length + t.feedRequests.length, 0);
  assert.equal(t.current().scrollTop, 0);
  t.h.objectBack.click();
  assert.equal(t.h.cards, t.initial);
  assert.equal(t.h.document.activeElement, t.opener);
});
