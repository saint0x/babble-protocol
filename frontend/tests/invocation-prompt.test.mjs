import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const code = ts.transpileModule(readFileSync(new URL("../src/app/invocation-prompt.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;

function summary(method = "reply") {
  return { method: `babel.social.${method}.v2`, actor: { id: "actor-1", label: "Alice" },
    requester: { id: "requester-1", title: "Notebook" }, recipient: { id: "recipient-1", title: "A conversation" },
    text: "Exact text\n  with whitespace", media: [{ id: "media-1", title: "A photo", mimeType: "image/png", sizeBytes: 4096, digest: "sha256:abc" }],
    deadlineEpochMs: 31_000 };
}

// Matches host-actions' VM/DOM harness, with explicit clocks, ancestry and observer delivery.
function harness() {
  const elements = [], intervals = new Set(), timeouts = new Map(), observers = new Set();
  let doc, now = 1000, monotonic = 0, allowed = true, authorizationError = false, modalFailure = false;
  class Events {
    listeners = new Map();
    addEventListener(type, listener) {
      const listeners = this.listeners.get(type) ?? new Set(); listeners.add(listener); this.listeners.set(type, listeners);
    }
    removeEventListener(type, listener) { this.listeners.get(type)?.delete(listener); }
    emit(type, values = {}) {
      const event = values.target ? values : { type, target: this, defaultPrevented: false, stopped: false,
        preventDefault() { this.defaultPrevented = true; }, stopPropagation() { this.stopped = true; }, ...values };
      for (const listener of [...this.listeners.get(type) ?? []]) listener(event);
      if (!event.stopped && this.parentElement) this.parentElement.emit(type, event);
      return event;
    }
    get listenerCount() { return [...this.listeners.values()].reduce((total, listeners) => total + listeners.size, 0); }
  }
  class Element extends Events {
    children = []; attributes = {}; style = {}; parentElement = null; disabled = false; focusCalls = 0; localText = "";
    constructor(tag) { super(); this.tag = tag; elements.push(this); }
    get ownerDocument() { return doc; }
    get isConnected() { return this === doc.documentElement || Boolean(this.parentElement?.isConnected); }
    get open() { return Object.hasOwn(this.attributes, "open"); }
    set open(value) { if (value) this.attributes.open = ""; else delete this.attributes.open; }
    get textContent() { return this.localText + this.children.map(child => child.textContent).join(""); }
    set textContent(value) { this.localText = value; this.children = []; }
    set innerHTML(_value) { throw Error("Untrusted HTML must never be parsed"); }
    setAttribute(key, value) { this.attributes[key] = value; }
    removeAttribute(key) { delete this.attributes[key]; }
    append(...children) { for (const child of children) { child.remove(); this.children.push(child); child.parentElement = this; } }
    remove() { if (this.parentElement) this.parentElement.children = this.parentElement.children.filter(child => child !== this); this.parentElement = null; }
    closest(selector) {
      for (let el = this; el; el = el.parentElement) {
        if (selector.includes("[inert]") && Object.hasOwn(el.attributes, "inert") ||
          selector.includes("[hidden]") && Object.hasOwn(el.attributes, "hidden") ||
          selector.includes('[aria-hidden="true"]') && el.attributes["aria-hidden"] === "true") return el;
      }
      return null;
    }
    matches(selector) { return selector === ":disabled" && this.disabled; }
    contains(other) { return this === other || this.children.some(child => child.contains(other)); }
    focus() { doc.activeElement = this; this.focusCalls++; }
    showModal() { if (modalFailure) throw Error("Unavailable"); this.open = true; this.focus(); }
    close() { this.open = false; this.emit("close"); }
  }
  class Observer {
    constructor(callback) { this.callback = callback; }
    observe() { observers.add(this); }
    disconnect() { observers.delete(this); }
  }
  const win = new Events();
  win.performance = { now: () => monotonic };
  win.getComputedStyle = element => ({ display: "block", visibility: "visible", opacity: "1", ...element.style });
  doc = new Events();
  Object.assign(doc, { defaultView: win, visibilityState: "visible", createElement: tag => new Element(tag),
    querySelectorAll: () => elements.filter(el => el.tag === "dialog" && el.open && el.isConnected),
    querySelector: () => doc.querySelectorAll()[0] ?? null });
  doc.documentElement = new Element("html");
  const target = new Element("section"), trigger = new Element("button");
  doc.documentElement.append(target, trigger); trigger.focus();
  const context = { exports: {}, HTMLElement: Element, MutationObserver: Observer, Date: { now: () => now },
    setInterval: callback => { intervals.add(callback); return callback; }, clearInterval: callback => intervals.delete(callback),
    setTimeout: (callback, delay) => { const handle = {}; timeouts.set(handle, { callback, at: monotonic + delay }); return handle; },
    clearTimeout: handle => timeouts.delete(handle),
    require: name => { assert.equal(name, "lucide"); return { X: {}, createElement: () => new Element("svg") }; } };
  vm.runInNewContext(code, context);
  const options = { target, authorized: () => { if (authorizationError) throw Error("Stale context"); return allowed; } };
  const host = new context.exports.InvocationPrompt(options);
  const data = key => elements.findLast(el => Object.hasOwn(el.attributes, key));
  const controller = new AbortController();
  const poll = () => { for (const callback of [...intervals]) callback(); };
  return { host, options, doc, win, target, trigger, elements, intervals, timeouts, observers, controller, data,
    prompt: input => host.prompt(input ?? summary(), { signal: controller.signal }),
    another: () => new context.exports.InvocationPrompt(options),
    dialog: () => data("data-invocation-prompt"), allow: () => data("data-invocation-allow").emit("click"),
    deny: () => data("data-invocation-deny").emit("click"),
    close: () => elements.findLast(el => el.attributes["aria-label"] === "Cancel request").emit("click"),
    poll, observe: () => { for (const observer of [...observers]) observer.callback(); },
    advance: (ms, wall = ms) => { monotonic += ms; now += wall; for (const [handle, timer] of [...timeouts]) {
      if (timer.at <= monotonic) { timeouts.delete(handle); timer.callback(); }
    } },
    set allowed(value) { allowed = value; }, set authorizationError(value) { authorizationError = value; },
    set modalFailure(value) { modalFailure = value; },
    assertClean() {
      assert.equal(doc.querySelectorAll().length, 0);
      assert.equal(intervals.size + timeouts.size + observers.size, 0);
      assert.equal(doc.listenerCount + win.listenerCount + elements.reduce((n, el) => n + el.listenerCount, 0), 0);
    } };
}

test("four supported methods show their exact context and resolve allow only after explicit confirmation", async () => {
  for (const method of ["follow", "unfollow", "share", "reply"]) {
    const h = harness(), input = summary(method), result = h.prompt(input);
    assert.equal(h.dialog().parentElement, h.target);
    assert.equal(h.doc.activeElement, h.data("data-invocation-deny"));
    assert.equal(h.data("data-invocation-allow").textContent.toLowerCase(), `${method} once`);
    for (const value of [input.actor.id, input.actor.label, input.requester.id, input.requester.title,
      input.recipient.id, input.recipient.title, input.text, "A photo", "image/png", "4096 bytes", "sha256:abc"]) {
      assert.ok(h.dialog().textContent.includes(value), value);
    }
    let settled = false; result.then(() => { settled = true; }); await Promise.resolve(); assert.equal(settled, false);
    h.allow(); h.deny(); assert.equal(await result, "allow");
    assert.equal(h.doc.activeElement, h.trigger); h.assertClean();
  }
});

test("Deny is distinct from close, Escape and external dialog close", async () => {
  for (const [dismiss, expected] of [[h => h.deny(), "deny"], [h => h.close(), "cancel"],
    [h => h.dialog().emit("cancel"), "cancel"], [h => h.dialog().close(), "cancel"]]) {
    const h = harness(), result = h.prompt(); dismiss(h); assert.equal(await result, expected); h.assertClean();
  }
});

test("literal HTML, whitespace, long IDs and attachment metadata stay intact without parsing or fetching", async () => {
  const h = harness(), literal = '<img src=x onerror="alert(1)"><script>bad()</script>\n  & more', id = "a".repeat(300);
  const input = summary(); input.actor = { id, label: literal }; input.requester.title = literal;
  input.recipient = { id, title: literal }; input.text = literal; input.media = [{ id, title: literal, mimeType: literal, sizeBytes: 0, digest: literal }];
  const result = h.prompt(input);
  assert.equal(h.elements.find(el => el.tag === "pre").textContent, literal);
  assert.equal(h.elements.filter(el => el.tag === "code" && el.textContent === id).length, 2);
  assert.equal(h.elements.filter(el => el.tag === "summary" && el.textContent === "Full ID").length, 2);
  assert.ok(h.dialog().textContent.includes("0 bytes"));
  assert.equal(h.elements.some(el => ["img", "script", "iframe", "a"].includes(el.tag)), false);
  h.deny(); await result; h.assertClean();
});

test("missing optional fields and empty exact text have distinct honest presentations", async () => {
  for (const exactText of [undefined, ""]) {
    const h = harness(), input = summary(); input.actor = { id: "actor" }; input.recipient = { id: "recipient" };
    delete input.text; delete input.media; if (exactText !== undefined) input.text = exactText;
    const result = h.prompt(input), preview = h.elements.find(el => el.tag === "pre");
    assert.equal(preview?.textContent, exactText); assert.equal(h.dialog().textContent.includes("undefined"), false);
    h.deny(); await result; h.assertClean();
  }
});

test("caller mutation cannot change displayed content or extend the original server deadline", async () => {
  const h = harness(), input = summary(), result = h.prompt(input);
  input.actor.id = "replacement"; input.text = "replacement"; input.media[0].title = "replacement"; input.deadlineEpochMs = 1e12;
  h.options.authorized = () => true;
  assert.equal(h.dialog().textContent.includes("replacement"), false);
  h.advance(30_000); assert.equal(await result, "cancel"); h.assertClean();
});

test("immutable deadline survives wall-clock rollback and cancels before late allow", async () => {
  const h = harness(), result = h.prompt(); h.advance(30_000, -5000); h.allow();
  assert.equal(await result, "cancel"); assert.equal(h.trigger.focusCalls, 1); h.assertClean();
});

test("expired or malformed summaries fail closed before mounting", async () => {
  for (const change of [s => { s.deadlineEpochMs = 1000; }, s => { s.deadlineEpochMs = NaN; }, s => { s.deadlineEpochMs = Infinity; },
    s => { s.method = "babel.clipboard.write.v1"; }, s => { s.method = "babel.social.follow.v1"; },
    s => { s.method = "babel.social.unfollow.v1"; }, s => { s.method = "babel.social.share.v1"; },
    s => { s.method = "babel.social.reply.v1"; }, s => { s.method = "toString"; }, s => { s.actor.id = ""; },
    s => { s.text = 7; }, s => { s.requester.title = null; }, s => { s.media[0].sizeBytes = -1; }, s => { s.media[0].digest = {}; }]) {
    const h = harness(), input = summary(); change(input);
    assert.equal(await h.prompt(input), "cancel"); assert.equal(h.dialog(), undefined); h.assertClean();
  }
});

test("pre-aborted, hidden, inert, disconnected, unauthorized and disposed contexts never mount", async () => {
  for (const stop of [h => h.controller.abort(), h => { h.doc.visibilityState = "hidden"; }, h => h.target.setAttribute("inert", ""),
    h => h.target.remove(), h => { h.allowed = false; }, h => { h.authorizationError = true; }, h => h.host.dispose(),
    h => h.target.setAttribute("hidden", ""), h => { h.target.style.display = "none"; }]) {
    const h = harness(); stop(h); assert.equal(await h.prompt(), "cancel"); assert.equal(h.dialog(), undefined); h.assertClean();
  }
});

test("lifecycle loss cancels, releases capacity and never restores obsolete focus", async () => {
  for (const stop of [h => h.controller.abort(), h => h.host.dispose(),
    h => { h.doc.visibilityState = "hidden"; h.doc.emit("visibilitychange"); },
    h => h.win.emit("pagehide"), h => h.win.emit("accountchange"), h => h.doc.emit("accountchange"),
    h => { h.allowed = false; h.poll(); }, h => { h.authorizationError = true; h.poll(); },
    h => { h.target.remove(); h.observe(); }, h => { h.dialog().remove(); h.observe(); },
    h => { h.target.setAttribute("inert", ""); h.observe(); },
    h => { h.target.setAttribute("hidden", ""); h.observe(); },
    h => { h.target.style.visibility = "hidden"; h.observe(); },
    h => { h.dialog().style.display = "none"; h.observe(); },
    h => { h.dialog().removeAttribute("open"); h.observe(); },
    h => { h.doc.documentElement.setAttribute("aria-hidden", "true"); h.observe(); },
    h => { h.doc.documentElement.append(h.dialog()); h.observe(); }]) {
    const h = harness(), result = h.prompt(); stop(h); h.allow(); assert.equal(await result, "cancel");
    assert.equal(h.trigger.focusCalls, 1); h.assertClean(); h.host.dispose();
  }
});

test("allow and deny check current context before the lifecycle watcher runs", async () => {
  for (const decide of [h => h.allow(), h => h.deny()]) {
    for (const stop of [h => { h.allowed = false; }, h => h.target.setAttribute("inert", ""),
      h => { h.target.style.opacity = "0"; }, h => { h.doc.visibilityState = "hidden"; },
      h => h.target.remove(), h => { h.authorizationError = true; }]) {
      const h = harness(), result = h.prompt(); stop(h); decide(h); assert.equal(await result, "cancel"); h.assertClean();
    }
  }
});

test("one prompt per document across instances, no queued follow-up and capacity is reusable", async () => {
  const h = harness(), other = h.another(), first = h.prompt();
  for (let i = 0; i < 25; i++) assert.equal(await other.prompt(summary(), { signal: h.controller.signal }), "cancel");
  assert.equal(await h.prompt(), "cancel"); assert.equal(h.doc.querySelectorAll().length, 1);
  h.deny(); assert.equal(await first, "deny");
  const next = other.prompt(summary(), { signal: h.controller.signal }); h.allow(); assert.equal(await next, "allow"); h.assertClean();
});

test("independent documents can each prompt without blocking each other", async () => {
  const a = harness(), b = harness(), first = a.prompt(), second = b.prompt();
  a.allow(); b.deny(); assert.equal(await first, "allow"); assert.equal(await second, "deny"); a.assertClean(); b.assertClean();
});

test("existing dialogs prevent stacking and a new competing dialog cancels consent", async () => {
  for (const alreadyOpen of [true, false]) {
    const h = harness(), other = h.doc.createElement("dialog");
    if (alreadyOpen) { h.target.append(other); other.showModal(); }
    const result = h.prompt();
    if (!alreadyOpen) { h.target.append(other); other.showModal(); h.allow(); }
    assert.equal(await result, "cancel"); assert.equal(other.open, true); other.remove(); h.assertClean();
  }
});

test("focus restoration respects removed, hidden, inert, disabled and newly focused elements", async () => {
  for (const change of [h => h.trigger.remove(), h => h.trigger.setAttribute("hidden", ""), h => h.trigger.setAttribute("inert", ""),
    h => { h.trigger.disabled = true; }, h => { h.trigger.style.display = "none"; },
    h => { const other = h.doc.createElement("button"); h.target.append(other); other.focus(); }]) {
    const h = harness(), result = h.prompt(); change(h); h.deny(); assert.equal(await result, "deny");
    assert.equal(h.trigger.focusCalls, 1); h.assertClean();
  }
});

test("keyboard, pointer and touch interactions cannot bubble to swipe or shortcut handlers", async () => {
  const h = harness(), result = h.prompt();
  for (const type of ["keydown", "keyup", "keypress", "pointerdown", "pointermove", "pointerup", "pointercancel", "touchstart", "touchmove", "touchend", "wheel"]) {
    const event = h.data("data-invocation-deny").emit(type, { key: "ArrowRight" });
    assert.equal(event.stopped, true, type); assert.equal(event.defaultPrevented, false, type);
  }
  h.deny(); await result; h.assertClean();
});

test("unsupported modal cancels cleanly, with no poisoned capacity", async () => {
  const h = harness(); h.modalFailure = true;
  assert.equal(await h.prompt(), "cancel"); h.assertClean();
  h.modalFailure = false; const result = h.prompt(); h.allow(); assert.equal(await result, "allow"); h.assertClean();
});

test("a stale button cannot resolve or dismiss a subsequent prompt", async () => {
  const h = harness(), first = h.prompt(), oldAllow = h.data("data-invocation-allow");
  h.deny(); await first;
  const next = h.prompt(); oldAllow.emit("click"); assert.equal(h.dialog().isConnected, true);
  h.deny(); assert.equal(await next, "deny"); h.assertClean();
});
