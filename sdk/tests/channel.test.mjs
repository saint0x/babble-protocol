import assert from "node:assert/strict";
import test from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { getEventListeners } from "node:events";
import { connectSurfaceBridge, createSurfaceSDK, BrowserBridgeTransport } from "../dist/index.js";
import { control, response, request, rpc, plan, FakePort, harness, fakeChild, trackedTimers } from "./channel-fixtures.mjs";

test("native channel handshake transfers a single port, scopes RPC and closes both sides", async (t) => {
  const h = harness(t);
  const transport = await connectSurfaceBridge({ parentOrigin: "https://host.test", window: h.child });
  t.after(() => transport.close());
  await h.mounted.ready;
  const sdk = createSurfaceSDK({ transport, plan: plan(), origin: "https://object.test", runtimeId: "test", surfaceSessionId: "session" });
  const result = await sdk.search.objects({ q: "test" });
  assert.deepEqual(result, { ok: true });
  assert.deepEqual(h.calls[0].binding, { object_id: "object", surface_session_id: "session", runtime_id: "test", origin: "https://object.test", capability_grants: [], identity_id: "identity" });
  assert.deepEqual(h.offers, [{ data: control("connect"), origin: "https://host.test" }]);
  assert.equal(h.windowMessages.length, 0);
  sdk.close();
  await delay(10);
  assert.equal(h.mounted.lifecycle.state, "evicted");
  assert.equal(h.listeners.size, 0);
  assert.equal(h.pagehide.size, 0);
  await assert.rejects(transport.request(request()), /closed/);
});

test("same WindowProxy replacement before load cannot dispatch or receive the pending old-document response", async (t) => {
  let release;
  let started;
  const dispatched = new Promise(resolve => { started = resolve; });
  let calls = 0;
  const h = harness(t, { dispatch(envelope) {
    calls++;
    started();
    return new Promise(resolve => { release = () => resolve(response(envelope)); });
  } });
  const transport = await connectSurfaceBridge({ parentOrigin: "https://host.test", window: h.child });
  t.after(() => transport.close());
  await h.mounted.ready;
  const pending = transport.request(request("old-document"));
  await dispatched;
  const replacement = new MessageChannel();
  t.after(() => { replacement.port1.close(); replacement.port2.close(); });
  const replacementMessages = [];
  replacement.port1.addEventListener("message", event => replacementMessages.push(event.data));
  replacement.port1.start();
  const offered = structuredClone(replacement.port2, { transfer: [replacement.port2] });
  h.offer([offered]);
  h.window.emit({ source: h.frame.contentWindow, origin: "null", data: rpc(request("new-window")) });
  replacement.port1.postMessage(control("confirm"));
  replacement.port1.postMessage(rpc(request("new-port")));
  release();
  assert.equal((await pending).id, "old-document");
  await delay(10);
  assert.equal(calls, 1);
  assert.deepEqual(replacementMessages, []);
  assert.deepEqual(h.windowMessages, []);
  assert.equal(h.mounted.lifecycle.state, "prefetched");
});

test("first admission is irrevocable even when a second document offers before confirmation", async (t) => {
  const timers = trackedTimers(t);
  const h = harness(t, { handshakeTimeoutMs: 20 });
  const first = new FakePort();
  const second = new FakePort();
  h.offer([first]);
  h.offer([second]);
  assert.equal(second.closed, 1);
  assert.deepEqual(first.messages, [control("accept")]);
  first.emit(rpc(request()));
  assert.equal(h.calls.length, 0);
  const rejected = assert.rejects(h.mounted.ready, /timed out/);
  t.mock.timers.tick(20);
  await rejected;
  assert.equal(first.closed, 1);
  assert.equal(h.mounted.lifecycle.state, "evicted");
  assert.equal(h.listeners.size, 0);
  assert.equal(first.listenerCount, 0);
  assert.equal(timers.size, 0);
});

test("origin, exact source, control version/shape and exactly one port gate admission", async (t) => {
  const h = harness(t);
  for (const malformed of [{}, { ...control("connect"), version: 2 }, { ...control("connect"), protocol: "wrong" }, { ...control("connect"), grants: [] }, rpc(request())]) {
    const port = new FakePort();
    h.offer([port], malformed);
    assert.equal(port.closed, 1);
  }
  for (const ports of [[], [new FakePort(), new FakePort()]]) {
    h.offer(ports);
    for (const port of ports) assert.equal(port.closed, 1);
  }
  const badOrigin = new FakePort();
  h.offer([badOrigin], control("connect"), { origin: "https://evil.test" });
  assert.equal(badOrigin.closed, 1);
  const otherFrame = new FakePort();
  h.offer([otherFrame], control("connect"), { source: {} });
  assert.equal(otherFrame.messages.length, 0);
  // Other mounts own offers from other frames; this mount must not close them.
  assert.equal(otherFrame.closed, 0);
  const port = new FakePort();
  h.offer([port]);
  port.emit({ ...control("confirm"), version: 2 });
  port.emit(rpc(request()));
  assert.equal(h.calls.length, 0);
  port.emit(control("confirm"));
  await h.mounted.ready;
  port.emit(rpc(request()));
  await Promise.resolve();
  assert.equal(h.calls.length, 1);
  assert.deepEqual(h.windowMessages, []);
});

test("Static readiness resolves without listeners, timers or script authority", async (t) => {
  const timers = trackedTimers(t);
  const value = plan();
  value.surface.target = "Static";
  value.sandbox.capability_bridge = false;
  const h = harness(t, { plan: value });
  await h.mounted.ready;
  assert.equal(timers.size, 0);
  assert.equal(h.listeners.size, 0);
  h.offer([new FakePort()]);
  assert.equal(h.calls.length, 0);
});

for (const teardown of ["unmount", "suspend", "evict"]) {
  test(`${teardown} rejects connecting readiness and cancels all timers`, async (t) => {
    const timers = trackedTimers(t);
    const h = harness(t);
    const port = new FakePort();
    h.offer([port]);
    const rejected = assert.rejects(h.mounted.ready, /torn down/);
    h.mounted[teardown]();
    h.mounted[teardown]();
    await rejected;
    assert.equal(port.closed, 1);
    assert.equal(port.listenerCount, 0);
    assert.equal(timers.size, 0);
    assert.equal(h.frame.removed, 1);
    assert.equal(h.mounted.lifecycle.state, teardown === "suspend" ? "suspended" : "evicted");
  });
}

for (const stage of ["connecting", "ready"]) {
  for (const event of ["messageerror", "close"]) {
    test(`host ${event} during ${stage} is terminal and cancels in-flight work`, async (t) => {
      const timers = trackedTimers(t);
      let signal;
      let release;
      const h = harness(t, { dispatch(envelope, context) {
        signal = context.signal;
        return new Promise(resolve => { release = () => resolve(response(envelope)); });
      } });
      const port = new FakePort();
      h.offer([port]);
      const readiness = stage === "connecting" ? assert.rejects(h.mounted.ready, /messageerror|closed/) : h.mounted.ready;
      if (stage === "ready") {
        port.emit(control("confirm"));
        await readiness;
        port.emit(rpc(request()));
      }
      const queued = [...port.listeners.get("message")][0];
      port.emit(null, event);
      await readiness;
      if (signal) assert.equal(signal.aborted, true);
      queued({ data: rpc(request("queued")) });
      release?.();
      await Promise.resolve();
      assert.equal(port.messages.filter(value => value.type === "babel.rpc.response").length, 0);
      assert.equal(port.closed, 1);
      assert.equal(port.listenerCount, 0);
      assert.equal(h.listeners.size, 0);
      assert.equal(timers.size, 0);
    });
  }
}

test("host send failure during handshake closes admitted port and rejects readiness", async (t) => {
  const timers = trackedTimers(t);
  const h = harness(t);
  const port = new FakePort();
  port.postMessage = () => { throw new Error("send failed"); };
  const rejected = assert.rejects(h.mounted.ready, /send failed/);
  h.offer([port]);
  await rejected;
  assert.equal(port.closed, 1);
  assert.equal(port.listenerCount, 0);
  assert.equal(h.frame.removed, 1);
  assert.equal(timers.size, 0);
});

test("native pagehide closes the child and aborts the host dispatch", async (t) => {
  let signal;
  let started;
  const dispatched = new Promise(resolve => { started = resolve; });
  const h = harness(t, { dispatch(envelope, context) {
    signal = context.signal;
    started();
    return new Promise(resolve => context.signal.addEventListener("abort", () => resolve(response(envelope)), { once: true }));
  } });
  const transport = await connectSurfaceBridge({ parentOrigin: "https://host.test", window: h.child });
  const pending = assert.rejects(transport.request(request()), /closed/);
  await dispatched;
  for (const listener of h.pagehide) listener();
  await pending;
  await delay(10);
  assert.equal(signal.aborted, true);
  assert.equal(h.mounted.lifecycle.state, "evicted");
  assert.equal(h.pagehide.size, 0);
});

for (const cause of ["abort", "timeout", "transfer", "messageerror", "pagehide", "peer-close", "start"]) {
  test(`child connecting ${cause} closes both ports and leaves no handlers/timers`, async (t) => {
    const timers = trackedTimers(t);
    const channel = { port1: new FakePort(), port2: new FakePort() };
    const controller = new AbortController();
    const opts = fakeChild(channel, () => { if (cause === "transfer") throw new Error("transfer failed"); });
    if (cause === "start") channel.port1.start = () => { throw new Error("start failed"); };
    const connecting = connectSurfaceBridge({ ...opts, signal: controller.signal, timeoutMs: 50 });
    const rejected = assert.rejects(connecting, /aborted|timed out|transfer failed|messageerror|pagehide|closed|start failed/);
    if (cause === "abort") controller.abort();
    if (cause === "timeout") t.mock.timers.tick(50);
    if (cause === "messageerror") channel.port1.emit(null, "messageerror");
    if (cause === "pagehide") for (const listener of opts.pagehide) listener();
    if (cause === "peer-close") channel.port1.emit(control("close"));
    await rejected;
    assert.equal(timers.size, 0);
    assert.equal(channel.port1.closed, 1);
    assert.equal(channel.port2.closed, 1);
    assert.equal(channel.port1.listenerCount, 0);
    assert.equal(opts.pagehide.size, 0);
    assert.equal(getEventListeners(controller.signal, "abort").length, 0);
  });
}

test("connector validates origin/timeout and pre-abort before allocating any channel", async () => {
  let created = 0;
  const options = { parentOrigin: "https://host.test", createChannel() { created++; throw new Error("unreachable"); } };
  for (const timeoutMs of [0, -1, Infinity, 120001, 0.5]) await assert.rejects(connectSurfaceBridge({ ...options, timeoutMs }), /timeout/);
  for (const parentOrigin of ["*", "null", "https://host.test/path", "https://host.test/", "data:text/plain,a"]) await assert.rejects(connectSurfaceBridge({ ...options, parentOrigin }));
  await assert.rejects(connectSurfaceBridge({ ...options, signal: AbortSignal.abort(new Error("pre-abort")) }), /pre-abort/);
  assert.equal(created, 0);
});

test("child rejects spoofed and out-of-order handshakes then handles abort after readiness", async (t) => {
  const timers = trackedTimers(t);
  const channel = { port1: new FakePort(), port2: new FakePort() };
  const opts = fakeChild(channel);
  const controller = new AbortController();
  const connecting = connectSurfaceBridge({ ...opts, signal: controller.signal });
  channel.port1.emit(control("ready"));
  channel.port1.emit({ ...control("accept"), version: 0 });
  assert.equal(channel.port1.messages.length, 0);
  channel.port1.emit(control("accept"));
  assert.deepEqual(channel.port1.messages, [control("confirm")]);
  channel.port1.emit(control("ready"));
  const transport = await connecting;
  assert.equal(timers.size, 0);
  const pending = assert.rejects(transport.request(request()), /closed/);
  controller.abort();
  await pending;
  transport.close();
  assert.equal(channel.port1.closed, 1);
  assert.equal(channel.port2.closed, 1);
  assert.equal(channel.port1.listenerCount, 0);
  assert.equal(opts.pagehide.size, 0);
  assert.equal(getEventListeners(controller.signal, "abort").length, 0);
  assert.equal(timers.size, 0);
});

test("generic transport postMessage clone failures clean pending request and allow id reuse", async (t) => {
  const timers = trackedTimers(t);
  const port = new FakePort();
  port.postMessage = () => { throw new Error("clone failed"); };
  const transport = new BrowserBridgeTransport(port);
  t.after(() => transport.close());
  await assert.rejects(transport.request(request()), /clone failed/);
  assert.equal(timers.size, 0);
  await assert.rejects(transport.request(request()), /clone failed/);
  assert.equal(timers.size, 0);
});

test("host response transfer failure after readiness revokes port and concurrent dispatches", async (t) => {
  const timers = trackedTimers(t);
  const signals = [];
  const releases = [];
  const h = harness(t, { dispatch(envelope, context) {
    signals.push(context.signal);
    return new Promise(resolve => releases.push(() => resolve(response(envelope))));
  } });
  const port = new FakePort();
  h.offer([port]);
  port.emit(control("confirm"));
  await h.mounted.ready;
  port.emit(rpc(request("one")));
  port.emit(rpc(request("two")));
  port.postMessage = () => { throw new Error("response clone failed"); };
  releases[0]();
  await Promise.resolve();
  assert.equal(signals.every(signal => signal.aborted), true);
  releases[1]();
  await Promise.resolve();
  assert.equal(h.mounted.lifecycle.state, "evicted");
  assert.equal(port.closed, 1);
  assert.equal(port.listenerCount, 0);
  assert.equal(timers.size, 0);
});

test("a stalled ready acknowledgment keeps the child bounded after accept/confirm", async (t) => {
  const timers = trackedTimers(t);
  const channel = { port1: new FakePort(), port2: new FakePort() };
  const opts = fakeChild(channel);
  const rejected = assert.rejects(connectSurfaceBridge({ ...opts, timeoutMs: 20 }), /timed out/);
  channel.port1.emit(control("accept"));
  assert.deepEqual(channel.port1.messages, [control("confirm")]);
  t.mock.timers.tick(20);
  await rejected;
  assert.deepEqual(channel.port1.messages, [control("confirm"), control("close")]);
  assert.equal(channel.port1.closed, 1);
  assert.equal(channel.port2.closed, 1);
  assert.equal(channel.port1.listenerCount, 0);
  assert.equal(opts.pagehide.size, 0);
  assert.equal(timers.size, 0);
});

test("messageerror after child readiness rejects every pending RPC and removes listeners", async (t) => {
  const timers = trackedTimers(t);
  const channel = { port1: new FakePort(), port2: new FakePort() };
  const opts = fakeChild(channel);
  const controller = new AbortController();
  const connecting = connectSurfaceBridge({ ...opts, signal: controller.signal });
  channel.port1.emit(control("accept"));
  channel.port1.emit(control("ready"));
  const transport = await connecting;
  const first = assert.rejects(transport.request(request("one"), { signal: controller.signal }), /closed/);
  const second = assert.rejects(transport.request(request("two"), { signal: controller.signal }), /closed/);
  channel.port1.emit(null, "messageerror");
  await Promise.all([first, second]);
  assert.equal(channel.port1.closed, 1);
  assert.equal(channel.port1.listenerCount, 0);
  assert.equal(getEventListeners(controller.signal, "abort").length, 0);
  assert.equal(opts.pagehide.size, 0);
  assert.equal(timers.size, 0);
});
