import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import { canonicalValueBytes } from "@babble-protocol/sdk";

const sources = Object.fromEntries(["host-actions", "browser-invocations", "browser-action-prompt"].map(name => [name,
  ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText]));
const tick = async () => { for (let i = 0; i < 70; i++) await Promise.resolve(); };
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };
const plain = value => JSON.parse(JSON.stringify(value));
const actor = `id_${"a".repeat(64)}`, object = `obj_${"b".repeat(64)}`;
const documentId = "da9bab5c-3a90-4dc2-a9c9-ade1e1f8588c", dispatchId = "e".repeat(64);
function request(kind = "clipboard", payload) {
  return { protocol: "babble.rpc.v1", id: "request-1", trace_id: "trace-1", method: `babble.${kind}.${kind === "clipboard" ? "write" : "enter"}`,
    payload: payload ?? (kind === "clipboard" ? { text: "<script>literal text</script>" } : {}), idempotency_key: "one-browser-operation",
    binding: { capability_grants: [], identity_id: actor, object_id: object, surface_session_id: "surface-one" }, deadline: { timeout_ms: 30_000 } };
}
function pending(req, now) {
  return { invocation_id: "d".repeat(64), actor_id: actor, object_id: object, method: req.method, request_key: req.idempotency_key,
    origin: { kind: "surface", session_id: "surface-one", document_id: documentId },
    payload: req.method.includes("clipboard") ? structuredClone(req.payload)
      : { navigation_ui: req.payload.navigation_ui ?? "auto", target_hint: req.payload.target_hint ?? null },
    created_at: new Date(now).toISOString(), deadline: new Date(now + 30_000).toISOString(),
    state: { kind: "pending" }, revision: 0, result: null, execution_ticket: null };
}
const envelope = (req, invocation) => ({ protocol: req.protocol, id: req.id, trace_id: req.trace_id, result: null,
  error: { code: "PERMISSION_REQUIRED", message: "Consent required", retryable: false, retry_after_ms: null, details: { invocation } } });
const completed = (state, result) => ({ ...state, revision: 3, result, execution_ticket: null,
  state: result.kind === "failed" ? { kind: "failed", code: result.code }
    : { kind: "completed", outcome: { kind: "external", dispatch_id: dispatchId, result } } });

// DOM models events and ownership only; controller, prompt, parsing and HTTP retries are actual modules.
function harness(options = {}) {
  const elements = [], intervals = new Set(), timeouts = new Set(), clipboard = [], fullscreen = [], warnings = [], calls = [], rpcCalls = [];
  let now = Date.parse("2026-09-30T12:00:00Z"), allowed = true, gateBusy = false, releases = 0, state = null;
  class Clock extends Date { static now() { return now; } }
  class Events {
    listeners = new Map();
    addEventListener(type, callback) { const set = this.listeners.get(type) ?? new Set(); set.add(callback); this.listeners.set(type, set); }
    removeEventListener(type, callback) { this.listeners.get(type)?.delete(callback); }
    emit(type) { const event = { preventDefault() {}, stopPropagation() {} }; for (const cb of [...this.listeners.get(type) ?? []]) cb(event); }
    get listenerCount() { return [...this.listeners.values()].reduce((count, set) => count + set.size, 0); }
  }
  let doc;
  class Element extends Events {
    children = []; attributes = {}; disabled = false; isConnected = true; open = false; inert = false;
    constructor(tag) { super(); this.tag = tag; elements.push(this); }
    get ownerDocument() { return doc; }
    setAttribute(key, value) { this.attributes[key] = value; }
    append(...children) { this.children.push(...children); for (const child of children) child.parent = this; }
    remove() { this.isConnected = false; if (this.parent) this.parent.children = this.parent.children.filter(child => child !== this); }
    focus() { doc.activeElement = this; }
    closest() { return this.inert ? this : this.parent?.closest() ?? null; }
    showModal() { if (options.modalFailure) throw Error("unavailable"); this.open = true; }
    close() { this.open = false; this.emit("close"); }
  }
  const window = new Events();
  window.navigator = { clipboard: { writeText: text => { clipboard.push(text); return Promise.resolve(); } } };
  doc = new Events();
  Object.assign(doc, { defaultView: window, visibilityState: "visible", fullscreenElement: null, fullscreenEnabled: true,
    createElement: tag => new Element(tag), activeElement: new Element("button"),
    exitFullscreen: async () => { doc.exits++; doc.fullscreenElement = null; doc.emit("fullscreenchange"); }, exits: 0 });
  const trigger = doc.activeElement, target = new Element("section");
  target.requestFullscreen = async nativeOptions => { fullscreen.push(plain(nativeOptions)); doc.fullscreenElement = target; doc.emit("fullscreenchange"); };
  const modules = {}, globals = { HTMLElement: Element, AbortController, AbortSignal, TextEncoder, Error, Promise, Date: Clock,
    URL, structuredClone, console: { warn: (...args) => warnings.push(args) },
    setInterval: cb => { intervals.add(cb); return cb; }, clearInterval: cb => intervals.delete(cb),
    setTimeout: cb => { timeouts.add(cb); return cb; }, clearTimeout: cb => timeouts.delete(cb) };
  for (const name of ["browser-invocations", "browser-action-prompt", "host-actions"]) {
    const context = { ...globals, exports: {}, require: dependency => {
      if (dependency === "lucide") return { createElement: () => new Element("svg") };
      if (dependency === "@babble-protocol/sdk") return { canonicalValueBytes };
      const loaded = modules[dependency.replace(/^\.\//, "")]; assert.ok(loaded, `Unexpected dependency ${dependency}`); return loaded;
    } };
    vm.runInNewContext(sources[name], context); modules[name] = context.exports;
  }
  const fetcher = async (url, init) => {
    const action = url.pathname.split("/").at(-1), body = init.body ? JSON.parse(init.body) : undefined;
    const call = { action: action === "decision" ? body.decision : action, body, signal: init.signal, path: url.pathname }; calls.push(call);
    assert.equal(init.headers["x-babble-surface-document"], documentId); assert.equal(init.headers["x-babble-host-document"], undefined);
    assert.equal(init.method, action === "status" ? "GET" : "POST");
    assert.equal(url.pathname, `/invocations/v1/browser/${state.invocation_id}/${action}`);
    if (options.fetch) { const response = await options.fetch(call, h); if (response !== undefined) return response; }
    if (call.action === "allow_once") { assert.equal(state.state.kind, "pending"); state = { ...state, revision: 1, state: { kind: "approved" } }; }
    else if (action === "dispatch" && state.state.kind === "approved") {
      state = { ...state, revision: 2, state: { kind: "running", dispatch_id: dispatchId } };
      return Response.json({ ...state, execution_ticket: { dispatch_id: dispatchId, executor: "babble.browser.v1" } });
    } else if (action === "ack") {
      assert.equal(body.dispatch_id, dispatchId); assert.ok(["running", "unknown", "completed", "failed"].includes(state.state.kind));
      if (state.result) assert.deepEqual(body.result, state.result, "Ack retries must retain the native result");
      state = completed(state, body.result);
    } else if (call.action === "deny" || action === "cancel") {
      state = { ...state, revision: state.revision + 1, state: { kind: call.action === "deny" ? "denied" : "cancelled" } };
    }
    return Response.json(state);
  };
  const api = new modules["browser-invocations"].BrowserInvocationApi(new URL("https://node.test"), fetcher);
  const host = new modules["host-actions"].HostActions({ target, label: "Trusted Object title", identity: { id: actor, handle: "test-actor" },
    api, authorized: () => allowed, acquireConsent: () => {
      if (gateBusy) return null; gateBusy = true; let released = false;
      return () => { assert.equal(released, false, "Consent ownership released twice"); released = true; releases++; gateBusy = false; };
    } });
  const data = key => elements.findLast(element => Object.hasOwn(element.attributes, key));
  const rpc = async (req, context) => {
    rpcCalls.push({ request: req, context }); state ??= pending(req, now);
    if (options.rpc) return options.rpc(req, context, h);
    return envelope(req, structuredClone(state));
  };
  const h = { host, api, rpc, doc, window, elements, target, trigger, clipboard, fullscreen, warnings, intervals, timeouts, calls, rpcCalls,
    set allowed(value) { allowed = value; }, get now() { return now; }, elapse: ms => { now += ms; },
    get state() { return state; }, set state(value) { state = value; }, get releases() { return releases; },
    get gateBusy() { return gateBusy; }, set gateBusy(value) { gateBusy = value; },
    poll: () => { for (const callback of intervals) callback(); }, expire: () => { for (const callback of [...timeouts]) callback(); },
    dialog: () => data("data-host-action-dialog"), allow: () => data("data-host-action-allow").emit("click"),
    cancel: () => data("data-host-action-cancel").emit("click"), data,
    start: (req = request(), context = {}) => host.wrap(rpc)(req, { signal: new AbortController().signal, surfaceDocumentId: documentId, ...context }) };
  return h;
}
async function ready(h) {
  await tick(); assert.ok(h.dialog()?.isConnected, "Expected actual consent prompt");
  h.allow(); await tick(); assert.equal(h.dialog().attributes["data-host-action-stage"], "ready");
}

test("unrelated requests preserve dispatch/context; v1 explicitly rejects without RPC or native", async () => {
  const h = harness(), req = { ...request(), method: "babble.object.get.v1" }, context = { signal: new AbortController().signal }, result = {};
  assert.equal(h.host.wrap((r, c) => { assert.equal(r, req); assert.equal(c, context); return result; })(req, context), result);
  for (const kind of ["clipboard", "fullscreen"]) {
    const old = request(kind); old.method = `${old.method}.v1`; assert.equal((await h.start(old)).error.code, "UNSUPPORTED_VERSION");
  }
  assert.equal(h.rpcCalls.length, 0); assert.equal(h.clipboard.length + h.fullscreen.length, 0); h.host.dispose();
});

test("missing actor/object/session/document/idempotency and invalid payloads fail before RPC", async () => {
  for (const mutate of [r => delete r.binding.identity_id, r => r.binding.identity_id = "other", r => delete r.binding.object_id,
    r => delete r.binding.surface_session_id, r => delete r.idempotency_key, r => r.payload.text = 123,
    r => r.payload.text = "x".repeat(65_537), r => r.payload.text = "\u00e9".repeat(32_769), r => r.payload.extra = true]) {
    const h = harness(), req = request(); mutate(req); assert.ok((await h.start(req)).error, String(mutate));
    assert.equal(h.rpcCalls.length, 0); assert.equal(h.dialog(), undefined); h.host.dispose();
  }
  for (const surfaceDocumentId of [undefined, ""]) {
    const h = harness(); assert.ok((await h.start(request(), { surfaceDocumentId })).error); assert.equal(h.rpcCalls.length, 0); h.host.dispose();
  }
});

test("status then Allow once yields first ticket; a second fresh click runs native and ack alone confirms success", async () => {
  const req = request(), native = deferred(), ack = deferred(), h = harness({ fetch: call => call.action === "ack" ? ack.promise : undefined });
  h.window.navigator.clipboard.writeText = text => { h.clipboard.push(text); return native.promise; };
  let settled = false; const work = h.start(req).then(value => { settled = true; return value; }); await tick();
  assert.deepEqual(h.calls.map(c => c.action), ["status"]); assert.equal(h.doc.activeElement, h.data("data-host-action-cancel"));
  assert.equal(h.elements.find(e => e.tag === "pre").textContent, req.payload.text); assert.equal(h.dialog().parent, h.target);
  h.allow(); h.allow(); await tick(); assert.deepEqual(h.calls.map(c => c.action), ["status", "allow_once", "dispatch"]);
  assert.equal(h.clipboard.length, 0); assert.equal(settled, false);
  h.allow(); assert.deepEqual(h.clipboard, [req.payload.text], "Native starts synchronously inside the second click");
  h.allow(); await tick(); assert.equal(settled, false); assert.equal(h.calls.some(c => c.action === "ack"), false);
  native.resolve(); await tick(); assert.equal(settled, false);
  const result = { kind: "clipboard_write", written: true }; assert.deepEqual(h.calls.at(-1).body, { dispatch_id: dispatchId, result });
  ack.resolve(Response.json(completed(h.state, result)));
  const response = await work; assert.equal(response.error, null); assert.deepEqual(plain(response.result), result);
  assert.equal(response.id, req.id); assert.equal(response.protocol, req.protocol); assert.equal(h.clipboard.length, 1);
  assert.equal(h.dialog().isConnected, false); assert.equal(h.doc.activeElement, h.trigger); assert.equal(h.dialog().listenerCount, 0);
  assert.equal(h.releases, 1); h.host.dispose(); assert.equal(h.doc.listenerCount + h.window.listenerCount + h.intervals.size + h.timeouts.size, 0);
});

test("RPC preserves document and replaces cancellation signal; non-consent failures pass through", async () => {
  const controller = new AbortController(), req = request(), response = { ...envelope(req, null), error: { code: "CAPABILITY_DENIED" } };
  const h = harness({ rpc: async () => response }); assert.equal(await h.start(req, { signal: controller.signal }), response);
  assert.equal(h.rpcCalls[0].context.surfaceDocumentId, documentId); assert.notEqual(h.rpcCalls[0].context.signal, controller.signal);
  assert.equal(h.calls.length, 0); assert.equal(h.dialog(), undefined); h.host.dispose();
});

test("mismatched RPC envelope/challenge and legacy grants cannot reach consent or native", async () => {
  for (const mutate of [r => r.id = "other", r => r.protocol = "other", r => r.error.details.invocation.actor_id = "other",
    r => r.error.details.invocation.payload.text = "substituted", r => r.error.details.invocation.origin.document_id = "other",
    r => r.error.details.invocation.request_key = "other", r => delete r.error.details.invocation,
    r => { r.error = null; r.result = { action: { kind: "clipboard.write" }, receipt: { grant_id: "legacy" } }; }]) {
    const h = harness({ rpc: async (req, _context, fixture) => { const response = envelope(req, structuredClone(fixture.state)); mutate(response); return response; } });
    assert.ok((await h.start()).error, String(mutate)); assert.equal(h.dialog(), undefined); assert.equal(h.clipboard.length, 0); h.host.dispose();
  }
});

test("cancel, Escape and close durably deny consent without native execution", async () => {
  for (const dismiss of [h => h.cancel(), h => h.dialog().emit("cancel"), h => h.dialog().close()]) {
    const h = harness(), work = h.start(); await tick(); dismiss(h); assert.equal((await work).error.code, "CAPABILITY_DENIED");
    assert.deepEqual(h.calls.map(c => c.action), ["status", "deny"]); assert.equal(h.clipboard.length, 0);
    assert.equal(h.dialog().isConnected, false); assert.equal(h.releases, 1); h.host.dispose();
  }
});

test("running/unknown status and completed history never redispatch or rerun native", async () => {
  for (const kind of ["running", "unknown", "completed"]) {
    const h = harness(), result = { kind: "clipboard_write", written: true };
    h.state = kind === "completed" ? completed(pending(request(), h.now), result)
      : { ...pending(request(), h.now), revision: 2, state: { kind, dispatch_id: dispatchId } };
    const response = await h.start(); if (kind === "completed") assert.deepEqual(plain(response.result), result); else assert.ok(response.error);
    assert.equal(h.calls.some(c => c.action === "dispatch"), false); assert.equal(h.dialog(), undefined); assert.equal(h.clipboard.length, 0); h.host.dispose();
  }
});

test("absent/wrong dispatch ticket and lost dispatch response never invoke native or redispatch", async () => {
  for (const mode of ["missing-ticket", "lost-response", "wrong-ticket"]) {
    const h = harness({ fetch: (call, fixture) => {
      if (call.action !== "dispatch") return;
      if (mode === "lost-response") throw Error("connection lost");
      return Response.json({ ...fixture.state, revision: 2, state: { kind: "running", dispatch_id: dispatchId },
        execution_ticket: mode === "missing-ticket" ? null : { executor: "babble.browser.v1", dispatch_id: "f".repeat(64) } });
    } });
    const work = h.start(); await tick(); h.allow(); await tick(); assert.ok((await work).error); h.allow();
    assert.equal(h.clipboard.length, 0); assert.equal(h.calls.filter(c => c.action === "dispatch").length, 1); h.host.dispose();
  }
});

test("ack retry repeats only native result; persistent ack failure cannot manufacture success", async () => {
  for (const failPermanently of [false, true]) {
    let acks = 0; const h = harness({ fetch: call => {
      if (call.action === "ack" && (++acks === 1 || failPermanently)) return new Response(null, { status: 503 });
    } });
    const work = h.start(); await ready(h); h.allow(); const response = await work;
    assert.equal(h.clipboard.length, 1); assert.equal(acks, 2); assert.equal(h.calls.filter(c => c.action === "dispatch").length, 1);
    const calls = h.calls.filter(c => c.action === "ack"); assert.deepEqual(calls[0].body, calls[1].body);
    if (failPermanently) assert.ok(response.error); else assert.equal(response.error, null); h.host.dispose();
  }
});

test("native synchronous/asynchronous denial and failure acknowledge exact typed failure", async () => {
  for (const kind of ["clipboard", "fullscreen"]) for (const name of ["NotAllowedError", "SecurityError", "Error"]) for (const sync of [false, true]) {
    const h = harness(), error = Error("native refused"); error.name = name;
    const fail = () => { if (sync) throw error; return Promise.reject(error); };
    if (kind === "clipboard") h.window.navigator.clipboard.writeText = fail; else h.target.requestFullscreen = fail;
    const work = h.start(request(kind)); await ready(h); h.allow(); assert.ok((await work).error);
    assert.deepEqual(h.calls.find(c => c.action === "ack").body.result, { kind: "failed", code: name === "Error" ? "native_error" : "not_allowed" }); h.host.dispose();
  }
});

test("fullscreen uses trusted target; false native success is acknowledged as failure", async () => {
  const h = harness(), work = h.start(request("fullscreen", { target_hint: "#untrusted", navigation_ui: "hide" }));
  await ready(h); h.allow(); assert.equal((await work).error, null); assert.deepEqual(h.fullscreen, [{ navigationUI: "hide" }]);
  assert.equal(h.doc.fullscreenElement, h.target); assert.deepEqual(h.calls.at(-1).body.result, { kind: "fullscreen_enter", entered: true });
  h.host.dispose(); await tick(); assert.equal(h.doc.exits, 1);
  const other = harness(); other.target.requestFullscreen = async () => {};
  const failed = other.start(request("fullscreen")); await ready(other); other.allow();
  assert.ok((await failed).error); assert.deepEqual(other.calls.at(-1).body.result, { kind: "failed", code: "native_error" }); other.host.dispose();
});

test("unavailable APIs return before RPC and show no unusable prompt", async () => {
  for (const kind of ["clipboard", "fullscreen"]) {
    const h = harness(); if (kind === "clipboard") delete h.window.navigator.clipboard; else h.doc.fullscreenEnabled = false;
    assert.equal((await h.start(request(kind))).error.code, "CAPABILITY_UNAVAILABLE");
    assert.equal(h.rpcCalls.length, 0); assert.equal(h.dialog(), undefined); h.host.dispose();
  }
});

test("shared consent gate rejects another owner and releases exactly once", async () => {
  const h = harness(); h.gateBusy = true;
  assert.equal((await h.start()).error.code, "RATE_LIMITED"); assert.equal(h.rpcCalls.length, 0); assert.equal(h.releases, 0);
  h.gateBusy = false; const work = h.start(); await tick(); assert.equal(h.gateBusy, true);
  assert.equal((await h.start({ ...request(), id: "second", idempotency_key: "second" })).error.code, "RATE_LIMITED");
  h.cancel(); await work; assert.equal(h.gateBusy, false); assert.equal(h.releases, 1); h.host.dispose();
});

test("abort suppresses late RPC authorization and retains capacity until admitted work settles", async () => {
  const auth = deferred(), controller = new AbortController(), h = harness({ rpc: () => auth.promise });
  const work = h.start(request(), { signal: controller.signal }); controller.abort(); assert.equal((await work).error.code, "CANCELLED");
  assert.equal(h.rpcCalls[0].context.signal.aborted, true); assert.equal((await h.start()).error.code, "RATE_LIMITED");
  auth.resolve(envelope(request(), h.state)); await tick(); assert.equal(h.dialog(), undefined); assert.equal(h.clipboard.length, 0); h.host.dispose();
});

test("abort, visibility loss, detached/inert target, lost authority, pagehide and dispose remove pending UI", async () => {
  for (const stop of [h => h.host.dispose(), h => { h.allowed = false; h.poll(); }, h => { h.target.isConnected = false; h.poll(); },
    h => { h.target.inert = true; h.poll(); }, h => { h.doc.visibilityState = "hidden"; h.doc.emit("visibilitychange"); },
    h => h.window.emit("pagehide"), (_h, controller) => controller.abort()]) {
    const h = harness(), controller = new AbortController(), work = h.start(request(), { signal: controller.signal }); await tick(); stop(h, controller);
    assert.equal((await work).error.code, "CANCELLED"); assert.equal(h.dialog().isConnected, false);
    h.allow(); assert.equal(h.clipboard.length, 0); await tick(); h.host.dispose();
  }
});

test("fresh native click rechecks authority and deadline without waiting for poll", async () => {
  for (const invalidate of [h => h.allowed = false, h => h.elapse(30_001)]) {
    const h = harness(), work = h.start(); await ready(h); invalidate(h); h.allow(); assert.ok((await work).error);
    assert.equal(h.clipboard.length, 0); assert.deepEqual(h.calls.find(c => c.action === "ack").body.result, { kind: "failed", code: "context_lost" }); h.host.dispose();
  }
});

test("late native completion stays owned after cancellation; ack reports observed effect with gate still held", async () => {
  const native = deferred(), controller = new AbortController(), h = harness();
  h.window.navigator.clipboard.writeText = text => { h.clipboard.push(text); return native.promise; };
  const work = h.start(request(), { signal: controller.signal }); await ready(h); h.allow(); controller.abort();
  assert.equal((await work).error.code, "CANCELLED"); assert.equal(h.dialog().isConnected, false);
  assert.equal((await h.start()).error.code, "RATE_LIMITED"); assert.equal(h.gateBusy, true);
  native.resolve(); await tick(); assert.equal(h.clipboard.length, 1);
  assert.deepEqual(h.calls.find(c => c.action === "ack").body.result, { kind: "clipboard_write", written: true });
  assert.equal(h.calls.find(c => c.action === "ack").signal.aborted, false); assert.equal(h.releases, 1); h.host.dispose();
});

test("late fullscreen is cleaned up; preexisting, user-exited, rejected and unrelated fullscreen are not claimed", async () => {
  const native = deferred(), controller = new AbortController(), h = harness(); h.target.requestFullscreen = () => native.promise;
  const work = h.start(request("fullscreen"), { signal: controller.signal }); await ready(h); h.allow(); controller.abort();
  assert.equal((await work).error.code, "CANCELLED"); h.doc.fullscreenElement = h.target; native.resolve(); await tick(); assert.equal(h.doc.exits, 1); h.host.dispose();
  for (const mode of ["preexisting", "user-exited", "rejected", "other-target"]) {
    const next = harness(); if (mode === "other-target") { next.doc.fullscreenElement = next.trigger; next.host.dispose(); assert.equal(next.doc.exits, 0); continue; }
    if (mode === "preexisting") next.doc.fullscreenElement = next.target;
    if (mode === "rejected") next.target.requestFullscreen = () => Promise.reject(Error("refused"));
    const attempt = next.start(request("fullscreen")); await ready(next); next.allow(); await attempt;
    if (mode === "user-exited") { next.doc.fullscreenElement = null; next.doc.emit("fullscreenchange"); }
    next.doc.fullscreenElement = next.target; next.host.dispose(); assert.equal(next.doc.exits, 0, mode);
  }
});

test("deadline timeout removes prompt and stale focus while retaining native capacity until settlement", async () => {
  const h = harness(), native = deferred(); h.window.navigator.clipboard.writeText = () => native.promise;
  const work = h.start(); await ready(h); h.allow(); h.trigger.isConnected = false; h.expire();
  assert.equal((await work).error.code, "CANCELLED"); assert.equal(h.dialog().isConnected, false); assert.notEqual(h.doc.activeElement, h.trigger);
  assert.equal((await h.start()).error.code, "RATE_LIMITED"); native.resolve(); await tick(); assert.equal(h.releases, 1); h.host.dispose();
});

test("caller mutations cannot substitute request binding, payload, method or RPC correlation during authorization", async () => {
  const auth = deferred(), req = request(), original = structuredClone(req), h = harness({ rpc: () => auth.promise });
  const work = h.start(req); req.payload.text = "substituted"; req.binding.identity_id = "other"; req.binding.object_id = "other";
  req.method = "babble.fullscreen.enter"; req.id = "other"; req.idempotency_key = "other";
  assert.deepEqual(h.rpcCalls[0].request, original); auth.resolve(envelope(original, structuredClone(h.state)));
  await ready(h); h.allow(); const result = await work; assert.equal(result.error, null); assert.equal(result.id, original.id);
  assert.deepEqual(h.clipboard, [original.payload.text]); h.host.dispose();
});

test("challenge mutation after status begins cannot alter displayed intent or native bytes", async () => {
  const status = deferred(); let challenge;
  const h = harness({ rpc: async (req, _context, fixture) => { challenge = structuredClone(fixture.state); return envelope(req, challenge); },
    fetch: (call, fixture) => call.action === "status" ? status.promise.then(() => Response.json(fixture.state)) : undefined });
  const work = h.start(); await tick(); challenge.payload.text = "substituted"; challenge.origin.document_id = "other"; challenge.deadline = "2000-01-01";
  status.resolve(); await ready(h); h.allow(); assert.equal((await work).error, null);
  assert.deepEqual(h.clipboard, [request().payload.text]); h.host.dispose();
});

test("cancelled in-flight approval never proceeds to dispatch or native after its late response", async () => {
  const approval = deferred(), h = harness({ fetch: call => call.action === "allow_once" ? approval.promise : undefined });
  const work = h.start(); await tick(); h.allow(); await tick(); h.cancel();
  assert.equal((await work).error.code, "CANCELLED"); assert.equal(h.dialog().isConnected, false);
  assert.equal((await h.start()).error.code, "RATE_LIMITED"); assert.equal(h.releases, 0);
  approval.resolve(Response.json({ ...h.state, revision: 1, state: { kind: "approved" } })); await tick();
  assert.equal(h.calls.some(c => c.action === "dispatch"), false); assert.equal(h.clipboard.length, 0);
  assert.equal(h.releases, 1); h.host.dispose();
});

test("cancelling after ticket admission acknowledges context_lost without invoking native", async () => {
  const h = harness(), work = h.start(); await ready(h); h.cancel(); assert.equal((await work).error.code, "CANCELLED");
  assert.deepEqual(h.calls.map(c => c.action), ["status", "allow_once", "dispatch", "ack"]);
  assert.deepEqual(h.calls.at(-1).body, { dispatch_id: dispatchId, result: { kind: "failed", code: "context_lost" } });
  assert.equal(h.clipboard.length, 0); assert.equal(h.releases, 1); h.host.dispose();
});

test("cancelled native failure still reports actual observed typed failure before releasing ownership", async () => {
  const native = deferred(), h = harness(); h.window.navigator.clipboard.writeText = () => native.promise;
  const work = h.start(); await ready(h); h.allow(); h.cancel(); assert.equal((await work).error.code, "CANCELLED");
  assert.equal(h.gateBusy, true); const error = Error("denied late"); error.name = "NotAllowedError"; native.reject(error); await tick();
  assert.deepEqual(h.calls.find(c => c.action === "ack").body.result, { kind: "failed", code: "not_allowed" });
  assert.equal(h.releases, 1); h.host.dispose();
});

test("native completion retains gate during outstanding acknowledgement and suppresses result after authority loss", async () => {
  const ack = deferred(), h = harness({ fetch: call => call.action === "ack" ? ack.promise : undefined });
  const work = h.start(); await ready(h); h.allow(); await tick();
  assert.equal(h.clipboard.length, 1); assert.equal(h.gateBusy, true); assert.equal(h.releases, 0);
  assert.equal((await h.start()).error.code, "RATE_LIMITED");
  h.allowed = false; h.poll(); assert.equal((await work).error.code, "CANCELLED"); assert.equal(h.releases, 0);
  ack.resolve(Response.json(completed(h.state, { kind: "clipboard_write", written: true }))); await tick();
  assert.equal(h.releases, 1); assert.equal(h.clipboard.length, 1); h.host.dispose();
});

test("wrong-context or wrong-dispatch ack cannot confirm success and cannot repeat native", async () => {
  for (const mutate of [v => v.origin.document_id = "df9bab5c-3a90-4dc2-a9c9-ade1e1f8588c",
    v => v.state.outcome.dispatch_id = "f".repeat(64)]) {
    const h = harness({ fetch: (call, fixture) => {
      if (call.action !== "ack") return;
      const value = completed(structuredClone(fixture.state), call.body.result); mutate(value); return Response.json(value);
    } });
    const work = h.start(); await ready(h); h.allow(); assert.ok((await work).error);
    assert.equal(h.clipboard.length, 1); assert.equal(h.calls.filter(c => c.action === "dispatch").length, 1); h.host.dispose();
  }
});
