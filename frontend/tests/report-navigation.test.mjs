import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const source = name => readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8");
const main = ts.createSourceFile("main.ts", source("main"), ts.ScriptTarget.Latest, true);
const declaration = main.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === "openReportedObject");
assert.ok(declaration, "Exercise the production report navigation handler");
const compile = text => ts.transpileModule(text, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;

function harness() {
  const pending = [], visits = [], messages = [];
  const context = {
    accounts: { current: { identity: { id: "author" }, token: "session-one" } }, loadSequence: 10,
    client: { publicObject(id) {
      return new Promise((resolve, reject) => pending.push({ id, resolve, reject }));
    } },
    objectVisits: { open: (...args) => visits.push(args) },
    setAuthorStatus: (...args) => messages.push(args), errorMessage: cause => cause.message,
  };
  vm.runInNewContext(compile(declaration.getText(main)), context);
  return { context, pending, visits, messages, open: (...args) => context.openReportedObject(...args) };
}

test("report navigation reads the actual public Object and labels an explicit visit", async () => {
  const h = harness(), opener = {}, card = { id: "reported" };
  const task = h.open("reported", opener);
  assert.equal(h.pending[0].id, "reported");
  assert.equal(h.visits.length, 0);
  h.pending[0].resolve(card); await task;
  assert.deepEqual(h.visits, [[card, opener, "Object"]]);
  assert.equal(h.messages.at(-1)[1], "ready");
});

for (const stale of ["feed navigation", "account replacement", "same identity new login"]) {
  test(`late report navigation cannot replace ${stale}`, async () => {
    const h = harness();
    const task = h.open("reported", null);
    if (stale === "feed navigation") h.context.loadSequence++;
    else h.context.accounts.current = { identity: { id: stale === "account replacement" ? "other" : "author" }, token: "session-two" };
    h.pending[0].resolve({ id: "reported" }); await task;
    assert.equal(h.visits.length, 0);
    assert.equal(h.messages.length, 1, "a stale result cannot overwrite the newer status");
  });
}

test("concurrent report visits show only the most recent selection", async () => {
  const h = harness();
  const first = h.open("first", null), second = h.open("second", null);
  h.pending[1].resolve({ id: "second" }); await second;
  h.pending[0].resolve({ id: "first" }); await first;
  assert.deepEqual(h.visits, [[{ id: "second" }, null, "Object"]]);
});

test("failed public reads keep the current feed and expose a retryable error", async () => {
  const h = harness();
  const task = h.open("missing", null);
  h.pending[0].reject(new Error("Object not found")); await task;
  assert.equal(h.visits.length, 0);
  assert.deepEqual(h.messages.at(-1), ["Could not open reported post: Object not found", "error"]);
});

test("explicit report visits retain feed reading history and do not relabel ordinary shares", () => {
  const context = { exports: {} };
  vm.runInNewContext(compile(source("object-visits")), context);
  const feed = { cards: [{ id: "feed" }], index: 0, source: "discovery", reading: { top: 320 } };
  let current = feed;
  const visits = new context.exports.ObjectVisits({
    capture: () => current, beforeVisit() {}, show: value => { current = value; }, focus() {}, availability() {},
  });
  visits.open({ id: "reported" }, null, "Object");
  assert.equal(current.source, "Object");
  visits.back(); assert.equal(current, feed);
  visits.open({ id: "shared" }, null);
  assert.equal(current.source, "Shared post");
});

test("report intake does not disrupt reading; decisions invalidate recommendation and navigation caches", () => {
  const fn = main.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === "refreshModeratedFeed");
  assert.ok(fn);
  const calls = [];
  const context = {
    loadSequence: 8, cards: [{ id: "reported" }], currentIndex: 0,
    profileReturn: {}, profileBack: { hidden: false }, searchInput: { value: "current query" },
    closeSurface: () => calls.push("surface"), closeJudgments: () => calls.push("judgments"),
    profiles: { close: () => calls.push("profile") }, conversations: { clear: () => calls.push("replies") },
    quotes: { clear: () => calls.push("quotes") }, objectVisits: { clear: () => calls.push("visits") },
    followingFeed: { clear: () => calls.push("following") }, render: () => calls.push("render"),
    loadFeed: (...args) => calls.push(args),
  };
  vm.runInNewContext(compile(fn.getText(main)), context);
  context.refreshModeratedFeed({ object_id: "reported", decisions: [] });
  assert.equal(calls.length, 0); assert.equal(context.cards.length, 1);
  context.refreshModeratedFeed({ object_id: "reported", decisions: [{ outcome: "restrict" }] });
  assert.equal(context.loadSequence, 9); assert.equal(context.cards.length, 0);
  assert.equal(context.profileReturn, null); assert.equal(context.profileBack.hidden, true);
  assert.deepEqual(calls, ["surface", "judgments", "profile", "replies", "quotes", "visits", "following", "render", ["current query", "reported"]]);
  calls.length = 0;
  context.refreshModeratedFeed({ object_id: "reported", decisions: [{ outcome: "restrict" }, { outcome: "no_action" }] });
  assert.deepEqual(calls.at(-1), ["current query", "reported"], "a reversal reloads authoritative composition rather than restoring stale cards");
});

test("production deck and account listeners route reports through the host with the nearest Object target", () => {
  const hosts = new Set(["deck", "profileDropdown"]);
  const handlers = main.statements.filter(node => ts.isExpressionStatement(node) && ts.isCallExpression(node.expression)
    && ts.isPropertyAccessExpression(node.expression.expression) && node.expression.expression.name.text === "addEventListener"
    && hosts.has(node.expression.expression.expression.getText(main)) && node.expression.arguments[0]?.text === "click");
  assert.equal(handlers.length, 2);
  const listeners = {}, reports = [], inbox = [], menus = [], accountButton = {};
  class Element {
    constructor(action, objectId) { this.action = action; this.objectId = objectId; this.button = { dataset: { action, menuAction: action } }; }
    closest(selector) {
      if (selector === "[data-profile-author]") return null;
      if (selector === "[data-object-id]") return { dataset: { objectId: this.objectId } };
      if (selector === "button[data-action]" || selector === "button[data-menu-action]") return this.button;
      throw new Error(`Unexpected delegation selector: ${selector}`);
    }
  }
  const context = {
    Element, HTMLElement: Element,
    deck: { addEventListener: (type, fn) => { listeners.deck = fn; } },
    profileDropdown: { addEventListener: (type, fn) => { listeners.account = fn; } },
    toggleProfileButton: accountButton, toggleProfile: open => menus.push(open),
    moderationControls: { openReport: (...args) => reports.push(args), openInbox: (...args) => inbox.push(args) },
  };
  vm.runInNewContext(compile(handlers.map(node => node.getText(main)).join("\n")), context);
  const primary = new Element("report", "primary"), reply = new Element("report", "reply-not-root");
  listeners.deck({ target: primary }); listeners.deck({ target: reply });
  assert.deepEqual(reports, [["primary", primary.button], ["reply-not-root", reply.button]]);
  listeners.account({ target: new Element("reports") });
  assert.deepEqual(inbox, [[accountButton]]); assert.deepEqual(menus, [false]);
});
