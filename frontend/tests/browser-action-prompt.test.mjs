import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const code = ts.transpileModule(readFileSync(new URL("../src/app/browser-action-prompt.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;
const tick = async () => { for (let i = 0; i < 12; i++) await Promise.resolve(); };
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };

function harness() {
  const elements = [];
  let doc;
  class Element {
    attributes = {}; children = []; listeners = new Map(); isConnected = true; disabled = false; open = false;
    constructor(tag) { this.tag = tag; elements.push(this); }
    get ownerDocument() { return doc; }
    append(...children) { this.children.push(...children); for (const child of children) child.parent = this; }
    setAttribute(key, value) { this.attributes[key] = value; }
    addEventListener(type, cb) { const set = this.listeners.get(type) ?? new Set(); set.add(cb); this.listeners.set(type, set); }
    removeEventListener(type, cb) { this.listeners.get(type)?.delete(cb); }
    emit(type) { const event = { stopped: false, preventDefault() {}, stopPropagation() { this.stopped = true; } }; for (const cb of [...this.listeners.get(type) ?? []]) cb(event); return event; }
    focus() { doc.activeElement = this; }
    closest() { return null; }
    showModal() { this.open = true; }
    close() { this.open = false; this.emit("close"); }
    remove() { this.isConnected = false; }
  }
  doc = { createElement: tag => new Element(tag), visibilityState: "visible", activeElement: new Element("button") };
  const target = new Element("section"), trigger = doc.activeElement, abort = new AbortController();
  const context = { exports: {}, HTMLElement: Element, Promise, require: name => name === "lucide" ? { createElement: () => new Element("svg") } : {} };
  vm.runInNewContext(code, context);
  let live = true, authCount = 0, nativeCount = 0;
  const data = key => elements.findLast(e => Object.hasOwn(e.attributes, key));
  const start = (overrides = {}) => context.exports.promptBrowserAction({ target, label: "An Object", actor: "Alice", live: () => live },
    { kind: "clipboard", text: "<b>Literal text</b>" }, {
      signal: abort.signal, cancel: () => abort.abort(),
      authorize: async () => { authCount++; }, execute: async () => { nativeCount++; }, ...overrides,
    });
  return { start, abort, target, trigger, doc, elements, data, set live(value) { live = value; },
    get authCount() { return authCount; }, get nativeCount() { return nativeCount; },
    allow: () => data("data-host-action-allow").emit("click"), cancel: () => data("data-host-action-cancel").emit("click"),
    dialog: () => data("data-host-action-dialog") };
}

test("two separate clicks authorize once then synchronously invoke native; default focus is cancel", async () => {
  const h = harness(), work = h.start();
  assert.equal(h.doc.activeElement, h.data("data-host-action-cancel"));
  assert.equal(h.elements.find(e => e.tag === "pre").textContent, "<b>Literal text</b>");
  h.allow(); h.allow(); await tick();
  assert.equal(h.authCount, 1); assert.equal(h.nativeCount, 0);
  assert.equal(h.dialog().attributes["data-host-action-stage"], "ready");
  h.allow();
  assert.equal(h.nativeCount, 1, "Native starts inside the fresh click, not a deferred task");
  h.allow(); assert.equal((await work).kind, "completed");
  assert.equal(h.nativeCount, 1); assert.equal(h.doc.activeElement, h.trigger);
  assert.equal(h.dialog().isConnected, false);
  assert.equal([...h.dialog().listeners.values()].reduce((n, s) => n + s.size, 0), 0);
});

test("delayed authorization never executes native and cancellation prevents its late continuation", async () => {
  const h = harness(), auth = deferred(), work = h.start({ authorize: () => auth.promise });
  h.allow(); await tick();
  assert.equal(h.dialog().attributes["data-host-action-stage"], "authorizing");
  h.cancel(); assert.equal(h.abort.signal.aborted, true); assert.equal(h.dialog().isConnected, false);
  auth.resolve(); assert.equal((await work).kind, "cancelled");
  h.allow(); assert.equal(h.nativeCount, 0);
});

test("native settlement remains owned after abort and reports actual observed completion", async () => {
  const h = harness(), native = deferred();
  let settled = false;
  const work = h.start({ execute: () => native.promise }).then(result => { settled = true; return result; });
  h.allow(); await tick(); h.allow(); h.abort.abort(); await tick();
  assert.equal(settled, false); assert.equal(h.dialog().isConnected, false);
  native.resolve(); assert.equal((await work).kind, "completed");
});

test("denial, ready-stage cancellation and lost authority do not execute", async () => {
  for (const stage of ["consent", "ready", "lost-authority"]) {
    const h = harness(), work = h.start();
    if (stage !== "consent") { h.allow(); await tick(); }
    if (stage === "lost-authority") { h.live = false; h.allow(); } else h.cancel();
    assert.equal((await work).kind, stage === "consent" ? "denied" : "cancelled");
    assert.equal(h.nativeCount, 0);
  }
});

test("authorization failures and synchronous/asynchronous native failures stay distinct", async () => {
  for (const stage of ["authorize", "native-sync", "native-async"]) {
    const h = harness(), error = Error(stage);
    const work = h.start(stage === "authorize" ? { authorize: async () => { throw error; } }
      : { execute: () => { if (stage === "native-sync") throw error; return Promise.reject(error); } });
    h.allow(); await tick(); if (stage !== "authorize") h.allow();
    const outcome = await work;
    assert.equal(outcome.kind, "failed"); assert.equal(outcome.error, error);
    assert.equal(outcome.nativeStarted, stage !== "authorize");
  }
});

test("prompt isolates navigation, swipe, scroll and keyboard events", async () => {
  const h = harness(), work = h.start();
  for (const type of ["keydown", "keyup", "keypress", "pointerdown", "pointermove", "pointerup", "pointercancel", "touchstart", "touchmove", "touchend", "click", "wheel"]) {
    assert.equal(h.dialog().emit(type).stopped, true, type);
  }
  h.cancel(); await work;
});
