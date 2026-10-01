import assert from "node:assert/strict";
import test from "node:test";
import { connectSurfaceBridge, HttpRpcTransport } from "../dist/index.js";
import { control, response, request, rpc, plan, FakePort, harness, trackedTimers } from "./channel-fixtures.mjs";

function deferred() {
  let resolve, reject;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}

test("tracked timers advance handshake deadlines on the same deterministic clock", (t) => {
  const timers = trackedTimers(t);
  const deadline = performance.now() + 40;
  let fired = false;
  setTimeout(() => { fired = true; }, 40);
  t.mock.timers.tick(30);
  assert.equal(performance.now(), 30);
  assert.ok(performance.now() < deadline);
  assert.equal(fired, false);
  t.mock.timers.tick(10);
  assert.equal(performance.now(), deadline);
  assert.equal(fired, true);
  assert.equal(timers.size, 0);
});

test("confirmation registers once, gates readiness/RPC and injects only the host document into HTTP", async (t) => {
  const registration = deferred();
  const registrations = [];
  const contexts = [];
  const fetched = [];
  const http = new HttpRpcTransport("https://host.test/rpc", async (_url, init) => {
    fetched.push(init);
    return { ok: true, json: async () => response(JSON.parse(init.body)) };
  });
  const h = harness(t, {
    registerDocument(documentId, signal) { registrations.push({ documentId, signal }); return registration.promise; },
    dispatch(envelope, context) { contexts.push(context); return http.request(envelope, context); },
  });
  let ready = false;
  void h.mounted.ready.then(() => { ready = true; });
  const port = new FakePort();
  h.offer([port]);
  port.emit(rpc(request("pre-confirm")));
  assert.equal(registrations.length, 0);
  port.emit(control("confirm"));
  assert.equal(registrations.length, 1);
  const { documentId, signal } = registrations[0];
  assert.match(documentId, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  assert.equal(signal.aborted, false);
  port.emit(control("confirm"));
  port.emit(rpc(request("during-registration")));
  const replacement = new FakePort();
  h.offer([replacement]);
  replacement.emit(control("confirm"));
  await Promise.resolve();
  assert.equal(ready, false);
  assert.equal(contexts.length, 0);
  assert.equal(registrations.length, 1);
  assert.equal(replacement.closed, 1);
  assert.deepEqual(port.messages, [control("accept")]);
  registration.resolve();
  await h.mounted.ready;
  assert.equal(ready, true);
  assert.deepEqual(port.messages, [control("accept"), control("ready")]);
  port.emit(control("confirm"));
  const forged = rpc(request("forged"));
  forged.context = { surfaceDocumentId: "forged", signal: { aborted: false } };
  forged.surfaceDocumentId = "forged";
  forged.envelope.binding.surfaceDocumentId = "forged";
  forged.envelope.binding.document_id = "forged";
  port.emit(forged);
  assert.equal(contexts.length, 1);
  assert.equal(contexts[0].surfaceDocumentId, documentId);
  assert.ok(contexts[0].signal instanceof AbortSignal);
  assert.notEqual(contexts[0].signal, signal);
  assert.equal(new Headers(fetched[0].headers).get("x-babel-surface-document"), documentId);
  const binding = JSON.parse(fetched[0].body).binding;
  assert.equal(binding.object_id, "object");
  assert.equal(binding.surface_session_id, "session");
  assert.equal(Object.hasOwn(binding, "surfaceDocumentId"), false);
  assert.equal(Object.hasOwn(binding, "document_id"), false);
  assert.equal(registrations.length, 1);
  assert.equal(h.windowMessages.length, 0);
});

for (const cause of ["timeout", "unmount", "suspend", "evict", "navigation", "close", "messageerror", "peer-close"]) {
  for (const completion of ["resolve", "reject"]) {
    test(`registration ${cause} aborts and suppresses late ${completion}`, async (t) => {
      const timers = trackedTimers(t);
      const registration = deferred();
      let signal;
      let calls = 0;
      const h = harness(t, { handshakeTimeoutMs: 40,
        registerDocument(_id, value) { calls++; signal = value; return registration.promise; },
      });
      h.frame.dispatchEvent(new Event("load"));
      const port = new FakePort();
      h.offer([port]);
      t.mock.timers.tick(30);
      port.emit(control("confirm"));
      const queued = [...port.listeners.get("message")][0];
      const rejected = assert.rejects(h.mounted.ready, /timed out|torn down|closed|messageerror/);
      if (cause === "timeout") t.mock.timers.tick(10);
      else if (cause === "navigation") h.frame.dispatchEvent(new Event("load"));
      else if (cause === "peer-close") port.emit(control("close"));
      else if (cause === "close" || cause === "messageerror") port.emit(null, cause);
      else h.mounted[cause]();
      await rejected;
      assert.equal(signal.aborted, true);
      assert.equal(port.closed, 1);
      assert.equal(port.listenerCount, 0);
      assert.equal(timers.size, 0);
      assert.equal(h.listeners.size, 0);
      assert.equal(h.frame.removed, 1);
      registration[completion](new Error("late registration failure"));
      await Promise.resolve();
      queued({ data: control("confirm") });
      queued({ data: rpc(request("late")) });
      await Promise.resolve();
      assert.equal(calls, 1);
      assert.equal(h.calls.length, 0);
      assert.equal(port.messages.some(message => message.type === "babel.surface.ready"), false);
    });
  }
}

for (const synchronous of [true, false]) {
  test(`${synchronous ? "throwing" : "rejecting"} registration tears down without unhandled rejection`, async (t) => {
    const timers = trackedTimers(t);
    let signal;
    const failure = new Error("registration denied");
    const h = harness(t, { registerDocument(_id, value) {
      signal = value;
      if (synchronous) throw failure;
      return Promise.reject(failure);
    } });
    const port = new FakePort();
    h.offer([port]);
    const rejected = assert.rejects(h.mounted.ready, error => error === failure);
    port.emit(control("confirm"));
    await rejected;
    assert.equal(signal.aborted, true);
    assert.equal(h.mounted.lifecycle.state, "evicted");
    assert.equal(port.closed, 1);
    assert.equal(port.listenerCount, 0);
    assert.equal(timers.size, 0);
    assert.equal(h.calls.length, 0);
  });
}

test("invalid origin/source and malformed confirmations never register", async (t) => {
  let calls = 0;
  const h = harness(t, { registerDocument: async () => { calls++; } });
  for (const extra of [{ origin: "https://evil.test" }, { source: {} }, { source: null }]) {
    const port = new FakePort();
    h.offer([port], control("connect"), extra);
    port.emit(control("confirm"));
  }
  assert.equal(calls, 0);
  const port = new FakePort();
  h.offer([port]);
  port.emit({ ...control("confirm"), surfaceDocumentId: "forged" });
  port.emit({ ...control("confirm"), version: 2 });
  assert.equal(calls, 0);
  port.emit(control("confirm"));
  await h.mounted.ready;
  assert.equal(calls, 1);
});

test("executable mounts require registration while Static never invokes it", async (t) => {
  for (const target of ["Web", "WebGpu"]) {
    const value = plan();
    value.surface.target = target;
    for (const registerDocument of [undefined, null, false]) {
      assert.throws(() => harness(t, { plan: value, registerDocument }), /document registration callback/);
    }
  }
  t.mock.method(globalThis.crypto, "randomUUID", () => { assert.fail("Static required crypto.randomUUID"); });
  const value = plan();
  value.surface.target = "Static";
  const h = harness(t, { plan: value, registerDocument() { assert.fail("Static registered a document"); } });
  await h.mounted.ready;
  assert.equal(h.listeners.size, 0);
});

test("JavaScript registration hooks must return a promise or thenable", async (t) => {
  for (const result of [undefined, null, false, 1, {}, { then: true }]) {
    const h = harness(t, { registerDocument: () => result });
    const port = new FakePort();
    h.offer([port]);
    const rejected = assert.rejects(h.mounted.ready, /must return a promise/);
    port.emit(control("confirm"));
    await rejected;
    assert.equal(h.frame.removed, 1);
    assert.equal(port.closed, 1);
    assert.equal(h.calls.length, 0);
    assert.equal(port.messages.some(message => message.type === "babel.surface.ready"), false);
  }
  const h = harness(t, { registerDocument: () => ({ then(resolve) { resolve(); } }) });
  const port = new FakePort();
  h.offer([port]);
  port.emit(control("confirm"));
  await h.mounted.ready;
  assert.deepEqual(port.messages, [control("accept"), control("ready")]);
});

for (const stage of ["confirm", "registration"]) {
  test(`overdue ${stage} cannot bypass the shared deadline when timer execution is delayed`, async (t) => {
    const timers = trackedTimers(t);
    let now = 0;
    t.mock.method(globalThis.performance, "now", () => now);
    const registration = deferred();
    let calls = 0;
    let signal;
    const h = harness(t, { handshakeTimeoutMs: 40, registerDocument(_id, value) {
      calls++;
      signal = value;
      return registration.promise;
    } });
    const port = new FakePort();
    h.offer([port]);
    const rejected = assert.rejects(h.mounted.ready, /timed out/);
    if (stage === "confirm") now = 40;
    port.emit(control("confirm"));
    now = 40;
    registration.resolve();
    await rejected;
    assert.equal(calls, stage === "confirm" ? 0 : 1);
    if (signal) assert.equal(signal.aborted, true);
    assert.equal(port.closed, 1);
    assert.equal(timers.size, 0);
    assert.equal(port.messages.some(message => message.type === "babel.surface.ready"), false);
  });
}

test("native child pagehide aborts pending host registration", async (t) => {
  const started = deferred();
  const aborted = deferred();
  const registration = deferred();
  const h = harness(t, { registerDocument(_id, signal) {
    signal.addEventListener("abort", () => aborted.resolve(), { once: true });
    started.resolve();
    return registration.promise;
  } });
  const connecting = assert.rejects(connectSurfaceBridge({ parentOrigin: "https://host.test", window: h.child }), /pagehide/);
  const readiness = assert.rejects(h.mounted.ready, /closed/);
  await started.promise;
  for (const listener of h.pagehide) listener();
  await connecting;
  await aborted.promise;
  await readiness;
  registration.resolve();
  await Promise.resolve();
  assert.equal(h.calls.length, 0);
  assert.equal(h.frame.removed, 1);
});
