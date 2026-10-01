import assert from "node:assert/strict";
import test, { afterEach } from "node:test";
import {
  BrowserSurfaceHost,
  createSurfaceLifecycle,
  isRpcBridgeRequest,
  objectBinding,
  rpcBridgeResponse,
  surfaceBridgeControl,
} from "../dist/index.js";

const activeMounts = [];
afterEach(() => { for (const mounted of activeMounts.splice(0)) mounted.unmount(); });
class TestSurfaceHost extends BrowserSurfaceHost {
  mount(options) {
    const mounted = super.mount({ registerDocument: async () => {}, ...options });
    activeMounts.push(mounted);
    return mounted;
  }
}

test("BrowserSurfaceHost mounts a ready Web Surface with sandbox and bridge dispatch", async () => {
  const frame = new FakeFrame();
  const document = new FakeDocument(frame);
  const window = new FakeWindow("https://host.test");
  const container = new FakeContainer();
  const host = new TestSurfaceHost();
  const dispatches = [];

  const mounted = host.mount({
    container,
    document,
    window,
    hostOrigin: "https://host.test",
    surfaceSessionId: "surface_session_1",
    currentIdentityId: "id_alice",
    plan: readyPlan(),
    dispatch: (request) => {
      dispatches.push(request);
      return {
        protocol: "babel.rpc.v1",
        id: request.id,
        result: { results: [] },
        error: null,
        trace_id: request.trace_id ?? null,
      };
    },
    title: "Feed Surface",
    className: "surface-frame",
  });

  assert.equal(container.children[0], frame);
  assert.equal(frame.src, "https://object.test/surface.html");
  assert.equal(frame.title, "Feed Surface");
  assert.equal(frame.loading, "eager");
  assert.equal(frame.referrerPolicy, "no-referrer");
  assert.equal(frame.attributes.class, "surface-frame");
  assert.equal(
    frame.attributes.csp,
    "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'",
  );
  assert.match(frame.attributes.allow, /camera 'none'/);
  assert.match(frame.attributes.allow, /microphone 'none'/);
  assert.match(frame.attributes.allow, /usb 'none'/);
  assert.match(frame.attributes.allow, /webgpu 'none'/);
  assert.equal(frame.attributes.credentialless, "");
  assert.equal(frame.attributes["data-babel-object"], "obj_surface");
  assert.equal(frame.attributes["data-babel-surface-role"], "Feed");
  assert.equal(frame.attributes["data-babel-surface-target"], "Web");
  assert.equal(frame.attributes["data-babel-lifecycle"], "prefetched");
  assert.equal(frame.attributes["data-babel-memory-budget"], "33554432");
  assert.equal(frame.attributes["data-babel-network-budget"], "524288");
  assert.deepEqual([...frame.sandbox.tokens].sort(), ["allow-scripts"]);
  assert.equal(mounted.lifecycle.state, "prefetched");
  assert.deepEqual(mounted.lifecycle.budget, readyPlan().budget);
  assert.equal(mounted.surfaceOrigin, "https://object.test");

  window.dispatch({
    origin: "https://attacker.test",
    source: frame.contentWindow,
    data: requestMessage("ignored"),
  });
  window.dispatch({
    origin: "https://object.test",
    source: new FakeFrameWindow(),
    data: requestMessage("wrong-source"),
  });
  await connectFake(window, frame, "https://object.test");
  frame.port.postMessage(requestMessage("accepted-exact"));
  frame.port.postMessage(requestMessage("accepted"));
  await Promise.resolve();

  assert.equal(dispatches.length, 2);
  assert.deepEqual(dispatches.map((request) => request.id), ["accepted-exact", "accepted"]);
  assert.equal(frame.port.responses.length, 2);
  assert.equal(frame.port.responses[0].message.response.id, "accepted-exact");
  assert.equal(frame.port.responses[1].message.response.id, "accepted");
  assert.equal(isRpcBridgeRequest(requestMessage("shape-check")), true);

  mounted.activate();
  assert.equal(mounted.lifecycle.state, "active");
  assert.equal(frame.attributes["data-babel-lifecycle"], "active");
  mounted.suspend("offscreen");
  assert.equal(mounted.lifecycle.state, "suspended");
  assert.equal(frame.attributes["data-babel-lifecycle"], "suspended");
  assert.equal(frame.attributes["data-babel-suspended"], "true");
  mounted.evict("budget reclaimed");
  assert.equal(mounted.lifecycle.state, "evicted");
  assert.equal(frame.removed, true);
});

test("BrowserSurfaceHost keeps exact origin bridge policy for non-isolated Surfaces", async () => {
  const frame = new FakeFrame();
  const window = new FakeWindow("https://host.test");
  const host = new TestSurfaceHost();
  const dispatches = [];

  host.mount({
    container: new FakeContainer(),
    document: new FakeDocument(frame),
    window,
    hostOrigin: "https://host.test",
    surfaceSessionId: "surface_session_2",
    plan: {
      ...readyPlan(),
      sandbox: { ...readyPlan().sandbox, isolated_origin: false },
    },
    dispatch: (request) => {
      dispatches.push(request);
      return {
        protocol: "babel.rpc.v1",
        id: request.id,
        result: { results: [] },
        error: null,
        trace_id: request.trace_id ?? null,
      };
    },
  });

  assert.deepEqual([...frame.sandbox.tokens].sort(), ["allow-same-origin", "allow-scripts"]);
  assert.equal(
    frame.attributes.csp,
    "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'",
  );
  const rejected = await connectFake(window, frame, "null");
  assert.equal(rejected.peer.closed, true);
  await connectFake(window, frame, "https://object.test");
  frame.port.postMessage(requestMessage("exact-accepted", { surface_session_id: "surface_session_2" }));
  await Promise.resolve();

  assert.equal(dispatches.length, 1);
  assert.equal(dispatches[0].id, "exact-accepted");
  assert.equal(frame.contentWindow.messages.length, 0);
});

test("BrowserSurfaceHost synchronizes budgets but requires a fresh mount after suspension", () => {
  const frame = new FakeFrame();
  const mounted = new TestSurfaceHost().mount({
    container: new FakeContainer(),
    document: new FakeDocument(frame),
    window: new FakeWindow("https://host.test"),
    hostOrigin: "https://host.test",
    surfaceSessionId: "surface_session_3",
    plan: readyPlan(),
    dispatch: () => rpcBridgeResponse({
      protocol: "babel.rpc.v1",
      id: "unused",
      result: { results: [] },
      error: null,
      trace_id: null,
    }).response,
  });

  mounted.activate();
  mounted.suspend("viewport left active set");
  assert.equal(frame.attributes["data-babel-lifecycle"], "suspended");
  assert.equal(frame.attributes["data-babel-suspended"], "true");

  mounted.lifecycle.updateBudget(
    {
      ...readyPlan().budget,
      memory_bytes: 8388608,
      cpu_ms_per_minute: 250,
      network_bytes_per_minute: 65536,
      realtime_connections: 0,
    },
    "resource pressure",
  );
  assert.equal(frame.attributes["data-babel-memory-budget"], "8388608");
  assert.equal(frame.attributes["data-babel-cpu-budget"], "250");
  assert.equal(frame.attributes["data-babel-network-budget"], "65536");
  assert.equal(frame.attributes["data-babel-realtime-budget"], "0");
  assert.equal(frame.attributes["data-babel-storage-budget"], "1048576");
  assert.equal(frame.attributes["data-babel-gpu-expected"], "false");

  assert.equal(frame.removed, true);
  assert.throws(() => mounted.activate(), /fresh mount/);
  assert.equal(mounted.lifecycle.state, "suspended");
});

test("BrowserSurfaceHost applies pressure signals as budget reductions and lifecycle actions", () => {
  const frame = new FakeFrame();
  const mounted = new TestSurfaceHost().mount({
    container: new FakeContainer(),
    document: new FakeDocument(frame),
    window: new FakeWindow("https://host.test"),
    hostOrigin: "https://host.test",
    surfaceSessionId: "surface_session_4",
    plan: readyPlan(),
    dispatch: () => rpcBridgeResponse({
      protocol: "babel.rpc.v1",
      id: "unused",
      result: { results: [] },
      error: null,
      trace_id: null,
    }).response,
  });

  mounted.activate();
  mounted.applyPressure({
    level: "moderate",
    reason: "renderer memory pressure",
    budget: {
      memory_bytes: 4194304,
      cpu_ms_per_minute: 125,
      network_bytes_per_minute: 32768,
      persistent_storage_bytes: 262144,
      realtime_connections: 0,
    },
  });

  assert.equal(mounted.lifecycle.state, "suspended");
  assert.equal(frame.attributes["data-babel-lifecycle"], "suspended");
  assert.equal(frame.attributes["data-babel-memory-budget"], "4194304");
  assert.equal(frame.attributes["data-babel-cpu-budget"], "125");
  assert.equal(frame.attributes["data-babel-network-budget"], "32768");
  assert.equal(frame.attributes["data-babel-storage-budget"], "262144");
  assert.equal(frame.attributes["data-babel-realtime-budget"], "0");

  assert.throws(
    () =>
      mounted.applyPressure({
        level: "normal",
        budget: { memory_bytes: 8388608 },
      }),
    /cannot increase memory_bytes/,
  );

  mounted.applyPressure({ level: "critical", reason: "tab discarded by host" });
  assert.equal(mounted.lifecycle.state, "evicted");
  assert.equal(frame.removed, true);
});

test("BrowserSurfaceHost gates WebGPU through iframe Permissions Policy", () => {
  const webFrame = new FakeFrame();
  new TestSurfaceHost().mount({
    container: new FakeContainer(),
    document: new FakeDocument(webFrame),
    window: new FakeWindow("https://host.test"),
    hostOrigin: "https://host.test",
    surfaceSessionId: "surface_session_5",
    plan: readyPlan(),
    dispatch: () => rpcBridgeResponse({
      protocol: "babel.rpc.v1",
      id: "unused",
      result: { results: [] },
      error: null,
      trace_id: null,
    }).response,
  });
  assert.match(webFrame.attributes.allow, /webgpu 'none'/);

  const gpuFrame = new FakeFrame();
  new TestSurfaceHost().mount({
    container: new FakeContainer(),
    document: new FakeDocument(gpuFrame),
    window: new FakeWindow("https://host.test"),
    hostOrigin: "https://host.test",
    surfaceSessionId: "surface_session_6",
    plan: {
      ...readyPlan(),
      surface: { ...readyPlan().surface, target: "WebGpu" },
      budget: { ...readyPlan().budget, gpu_expected: true },
    },
    dispatch: () => rpcBridgeResponse({
      protocol: "babel.rpc.v1",
      id: "unused",
      result: { results: [] },
      error: null,
      trace_id: null,
    }).response,
  });
  assert.match(gpuFrame.attributes.allow, /webgpu \*/);
  assert.doesNotMatch(gpuFrame.attributes.allow, /webgpu 'none'/);
});

test("BrowserSurfaceHost normalizes spoofed Surface bridge bindings", async () => {
  const frame = new FakeFrame();
  const window = new FakeWindow("https://host.test");
  const dispatches = [];

  new TestSurfaceHost().mount({
    container: new FakeContainer(),
    document: new FakeDocument(frame),
    window,
    hostOrigin: "https://host.test",
    surfaceSessionId: "surface_session_1",
    currentIdentityId: "id_alice",
    plan: readyPlan(),
    dispatch: (request) => {
      dispatches.push(request);
      return {
        protocol: "babel.rpc.v1",
        id: request.id,
        result: { results: [] },
        error: null,
        trace_id: request.trace_id ?? null,
      };
    },
  });

  await connectFake(window, frame);
  frame.port.postMessage(requestMessage("wrong-object", { object_id: "obj_other" }));
  frame.port.postMessage(requestMessage("wrong-session", { surface_session_id: "surface_session_2" }));
  frame.port.postMessage(requestMessage("wrong-origin", { origin: "https://evil.test" }));
  frame.port.postMessage(requestMessage("wrong-grants", { capability_grants: ["grant_admin"] }));
  frame.port.postMessage(requestMessage("wrong-identity", { identity_id: "id_eve" }));
  await Promise.resolve();

  assert.equal(dispatches.length, 5);
  assert.deepEqual(
    dispatches.map((request) => request.binding),
    [
      surfaceBinding("id_alice"),
      surfaceBinding("id_alice"),
      surfaceBinding("id_alice"),
      surfaceBinding("id_alice"),
      surfaceBinding("id_alice"),
    ],
  );
  assert.equal(frame.port.responses.length, 5);
  assert.deepEqual(
    frame.port.responses.map((message) => message.message.response.error),
    [null, null, null, null, null],
  );
  assert.deepEqual(
    frame.port.responses.map((message) => message.message.response.id),
    ["wrong-object", "wrong-session", "wrong-origin", "wrong-grants", "wrong-identity"],
  );
});

test("BrowserSurfaceHost never delegates host signing, consent, or administration to a Surface", async () => {
  const frame = new FakeFrame();
  const window = new FakeWindow("https://host.test");
  const dispatches = [];
  new TestSurfaceHost().mount({
    container: new FakeContainer(), document: new FakeDocument(frame), window,
    hostOrigin: "https://host.test", surfaceSessionId: "surface_session_1",
    currentIdentityId: "id_alice", plan: readyPlan(),
    dispatch: (request) => { dispatches.push(request); throw new Error("Host-only request escaped"); },
  });
  const methods = [
    "babel.identity.create.v1", "babel.object.publish_text.v1", "babel.object.publish.v1",
    "babel.object.publish_media.v1", "babel.object.fork.v1", "babel.object.remix.v1",
    "babel.capabilities.grant.v1", "babel.capabilities.revoke.v1",
    "babel.graph.edge.publish.v1", "babel.media.blob.put.v1",
    "babel.events.import.v1", "babel.consensus.checkpoint.publish.v1",
    "babel.observability.snapshot.v1", "babel.personalization.sync.list.v1",
    "babel.runtime.surface.session.start.v1", "babel.runtime.surface.session.transition.v1",
    "babel.runtime.surface.session.heartbeat.v1",
    "babel.social.reactions.mine.v1", "babel.social.reactions.set.v1",
    "babel.judgment.object.evaluate.v1", "babel.realtime.room.define.v1", "unknown.method.v1",
  ];
  await connectFake(window, frame);
  for (const method of methods) {
    const message = requestMessage(method);
    message.envelope.method = method;
    message.envelope.payload = { author_id: "id_alice" };
    frame.port.postMessage(message);
  }
  await Promise.resolve();
  assert.equal(dispatches.length, 0);
  // Unknown methods are discarded by the envelope parser before scoped dispatch.
  assert.equal(frame.port.responses.length, methods.length - 1);
  assert.deepEqual(frame.port.responses.map(({ message }) => message.response.id), methods.slice(0, -1));
  for (const { message } of frame.port.responses) {
    assert.equal(message.response.error.code, "CAPABILITY_DENIED");
  }
});

test("BrowserSurfaceHost can read public reaction summaries and signed records", async () => {
  const frame = new FakeFrame();
  const window = new FakeWindow("https://host.test");
  const dispatches = [];
  const mounted = new TestSurfaceHost().mount({
    container: new FakeContainer(), document: new FakeDocument(frame), window,
    hostOrigin: "https://host.test", surfaceSessionId: "surface_session_1",
    currentIdentityId: "id_alice", plan: readyPlan(),
    dispatch: async (request) => {
      dispatches.push(request);
      return { protocol: "babel.rpc.v1", id: request.id, result: {}, error: null, trace_id: null };
    },
  });
  await connectFake(window, frame);
  for (const kind of ["summary", "record"]) {
    const message = requestMessage(`reaction-${kind}`);
    message.envelope.method = `babel.social.reactions.${kind}.v1`;
    message.envelope.payload = { object_id: "obj_public", ...(kind === "record" ? { actor_id: "id_bob" } : {}) };
    frame.port.postMessage(message);
  }
  await Promise.resolve();
  assert.equal(dispatches.length, 2);
  assert.equal(dispatches[1].payload.actor_id, "id_bob");
  assert.equal(dispatches[1].binding.identity_id, "id_alice");
  assert.equal(dispatches[1].binding.object_id, "obj_surface");
  mounted.unmount();
});

test("BrowserSurfaceHost rejects unsafe or unsupported plans", () => {
  const harness = mountHarness();
  const host = new TestSurfaceHost();

  assert.throws(
    () =>
      host.mount({
        ...harness,
        plan: { ...readyPlan(), admission: "blocked" },
      }),
    /admission status/,
  );

  assert.throws(
    () =>
      host.mount({
        ...mountHarness(),
        plan: {
          ...readyPlan(),
          surface: { ...readyPlan().surface, target: "Wasm" },
        },
      }),
    /cannot mount Surface target/,
  );

  assert.throws(
    () =>
      host.mount({
        ...mountHarness(),
        plan: {
          ...readyPlan(),
          surface: { ...readyPlan().surface, entry: "babel://object/surface" },
        },
      }),
    /HTTP\(S\) Surface entry/,
  );

  assert.throws(
    () =>
      host.mount({
        ...mountHarness(),
        plan: {
          ...readyPlan(),
          sandbox: { ...readyPlan().sandbox, capability_bridge: false },
        },
      }),
    /capability bridge/,
  );
});

for (const state of ["suspended", "evicted"]) {
  test(`runtime ${state} events dispose the iframe and suppress pending bridge output`, async (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const harness = mountHarness();
    const frame = harness.document.frame;
    let resolve;
    let signal;
    let calls = 0;
    const mounted = new TestSurfaceHost().mount({
      ...harness, plan: readyPlan(),
      dispatch: (request, context) => {
        calls += 1;
        signal = context.signal;
        return new Promise((done) => { resolve = () => done({ protocol: "babel.rpc.v1", id: request.id, result: null, error: null }); });
      },
    });
    await connectFake(harness.window, frame);
    const event = { data: requestMessage("pending") };
    frame.port.postMessage(event.data);
    const queued = [...frame.port.peer.listeners.get("message")][0];
    mounted.lifecycle.applyRuntimeEvent({ kind: "lifecycle_transition", lifecycle: state, reason: "runtime pressure" });
    assert.equal(frame.removed, true);
    assert.equal(frame.removalCount, 1);
    assert.equal(harness.window.listeners.size, 0);
    assert.equal(signal.aborted, true);
    queued(event);
    resolve();
    await Promise.resolve();
    t.mock.timers.tick(60000);
    assert.equal(calls, 1);
    assert.equal(frame.contentWindow.messages.length, 0);
    assert.equal(frame.port.responses.length, 0);
    assert.throws(() => mounted.activate(), /fresh mount/);
    mounted.unmount();
    mounted.evict();
    assert.equal(frame.removalCount, 1);
    assert.equal(mounted.lifecycle.state, "evicted");
  });
}

test("runtime session snapshots dispose and recursive eviction hooks unmount only once", () => {
  const harness = mountHarness();
  const mounted = new TestSurfaceHost().mount({ ...harness, plan: readyPlan() });
  mounted.lifecycle.onChange(() => mounted.unmount("recursive hook"));
  mounted.lifecycle.signal.addEventListener("abort", () => mounted.unmount("abort hook"));
  mounted.lifecycle.applySession({ lifecycle: "evicted", budget: readyPlan().budget, updated_at: "2026-09-30T00:00:00Z" });
  assert.equal(harness.document.frame.removalCount, 1);
  assert.equal(harness.window.listeners.size, 0);
  assert.equal(harness.document.frame.attributes["data-babel-lifecycle"], "evicted");
  mounted.unmount();
  assert.equal(harness.document.frame.removalCount, 1);
});

test("cold and prefetched lifecycle states allow backend suspension", () => {
  for (const initial of ["cold", "prefetched"]) {
    const lifecycle = createSurfaceLifecycle(initial);
    lifecycle.transition("suspended");
    lifecycle.transition("active");
    lifecycle.transition("evicted");
    assert.throws(() => lifecycle.transition("active"), /invalid.*transition/);
  }
  const harness = mountHarness();
  const mounted = new TestSurfaceHost().mount({ ...harness, plan: readyPlan() });
  mounted.suspend();
  assert.equal(mounted.lifecycle.state, "suspended");
  assert.equal(harness.document.frame.removed, true);
  mounted.unmount();
});

test("mount preserves runnable lifecycle states and rejects stopped plans before attaching", () => {
  for (const state of ["prefetched", "warm", "active"]) {
    const mounted = new TestSurfaceHost().mount({ ...mountHarness(), plan: { ...readyPlan(), lifecycle: state } });
    assert.equal(mounted.lifecycle.state, state);
    mounted.unmount();
  }
  for (const state of ["suspended", "evicted"]) {
    const harness = mountHarness();
    assert.throws(() => new TestSurfaceHost().mount({ ...harness, plan: { ...readyPlan(), lifecycle: state } }), /stopped lifecycle/);
    assert.equal(harness.container.children.length, 0);
    assert.equal(harness.window.listeners.size, 0);
  }
});

test("reload revokes the old document bridge and suppresses late responses", async () => {
  const harness = mountHarness();
  const frame = harness.document.frame;
  let resolve;
  const mounted = new TestSurfaceHost().mount({
    ...harness, plan: readyPlan(),
    dispatch: () => new Promise((done) => { resolve = done; }),
  });
  frame.dispatchEvent(new Event("load"));
  await connectFake(harness.window, frame);
  frame.port.postMessage(requestMessage("old-document"));
  frame.dispatchEvent(new Event("load"));
  resolve({ protocol: "babel.rpc.v1", id: "old-document", result: null, error: null });
  await Promise.resolve();
  assert.equal(mounted.lifecycle.state, "evicted");
  assert.equal(frame.removalCount, 1);
  assert.equal(frame.contentWindow.messages.length, 0);
  assert.equal(harness.window.listeners.size, 0);
  assert.equal(frame.port.responses.length, 0);
});

test("replacement frame windows cannot receive responses or inherit an old bridge", async () => {
  const harness = mountHarness();
  const frame = harness.document.frame;
  let resolve;
  let calls = 0;
  const mounted = new TestSurfaceHost().mount({
    ...harness, plan: readyPlan(),
    dispatch: () => { calls += 1; return new Promise((done) => { resolve = done; }); },
  });
  const oldWindow = frame.contentWindow;
  await connectFake(harness.window, frame);
  const originalPort = frame.port;
  frame.port.postMessage(requestMessage("old"));
  frame.contentWindow = new FakeFrameWindow();
  harness.window.dispatch({ origin: "null", source: frame.contentWindow, data: requestMessage("new") });
  resolve({ protocol: "babel.rpc.v1", id: "old", result: null, error: null });
  await Promise.resolve();
  assert.equal(calls, 1);
  assert.equal(oldWindow.messages.length, 0);
  assert.equal(frame.contentWindow.messages.length, 0);
  assert.equal(originalPort.responses[0].message.response.id, "old");
  mounted.unmount();
});

test("failed attachment cleans up the bridge", () => {
  const harness = mountHarness();
  harness.container.appendChild = () => { throw new Error("attachment failed"); };
  assert.throws(() => new TestSurfaceHost().mount({ ...harness, plan: readyPlan() }), /attachment failed/);
  assert.equal(harness.window.listeners.size, 0);
  assert.equal(harness.document.frame.removalCount, 1);
});

test("bundle declarations cannot enter the unverified URL host even with a ready plan", () => {
  const harness = mountHarness();
  const plan = readyPlan();
  plan.surface.bundle = { version: 1, entry_path: "index.html", files: [] };
  assert.throws(() => new TestSurfaceHost().mount({ ...harness, plan }), /verified bundle execution gateway/);
});

function mountHarness() {
  return {
    container: new FakeContainer(),
    document: new FakeDocument(new FakeFrame()),
    window: new FakeWindow("https://host.test"),
    hostOrigin: "https://host.test",
    surfaceSessionId: "surface_session_1",
    dispatch: () => rpcBridgeResponse({
      protocol: "babel.rpc.v1",
      id: "unused",
      result: null,
      error: {
        code: "INTERNAL",
        message: "unused",
        retryable: false,
        retry_after_ms: null,
        details: null,
      },
      trace_id: null,
    }).response,
  };
}

function requestMessage(id, binding = {}) {
  return {
    type: "babel.rpc.request",
    protocol: "babel.rpc.v1",
    envelope: {
      protocol: "babel.rpc.v1",
      id,
      method: "babel.search.objects.v1",
      binding: {
        ...objectBinding({
          objectId: "obj_surface",
          surfaceSessionId: "surface_session_1",
          runtimeId: "runtime",
          origin: "https://object.test",
          capabilityGrants: [],
        }),
        ...binding,
      },
      payload: { q: "babel", author: null, kind: null, limit: 3 },
      idempotency_key: null,
      deadline: {
        timeout_ms: 30000,
        client_started_at: "2026-09-27T00:00:00Z",
      },
      trace_id: null,
    },
  };
}

function surfaceBinding(identityId = null) {
  return objectBinding({
    objectId: "obj_surface",
    surfaceSessionId: "surface_session_1",
    runtimeId: "runtime",
    origin: "https://object.test",
    capabilityGrants: [],
    identityId,
  });
}

function readyPlan() {
  return {
    object_id: "obj_surface",
    surface: {
      role: "Feed",
      target: "Web",
      entry: "https://object.test/surface.html",
      integrity: "hash_surface",
    },
    lifecycle: "cold",
    admission: "ready",
    budget: {
      memory_bytes: 33554432,
      cpu_ms_per_minute: 1500,
      gpu_expected: false,
      network_bytes_per_minute: 524288,
      persistent_storage_bytes: 1048576,
      realtime_connections: 1,
      background_eligible: false,
    },
    sandbox: {
      isolated_origin: true,
      csp: "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'none'; worker-src 'none'; object-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
      host_cookies: false,
      top_navigation: false,
      wasi_filesystem: false,
      wasi_network: false,
      capability_bridge: true,
    },
    capability_decisions: [],
    blocked_reasons: [],
  };
}

class FakeWindow {
  listeners = new Set();

  constructor(origin) {
    this.location = { origin };
  }

  addEventListener(type, handler) {
    assert.equal(type, "message");
    this.listeners.add(handler);
  }

  removeEventListener(type, handler) {
    assert.equal(type, "message");
    this.listeners.delete(handler);
  }

  dispatch(event) {
    for (const handler of this.listeners) {
      handler(event);
    }
  }
}

class FakeDocument {
  constructor(frame) {
    this.frame = frame;
  }

  createElement(tagName) {
    assert.equal(tagName, "iframe");
    return this.frame;
  }
}

class FakeContainer {
  children = [];

  appendChild(frame) {
    this.children.push(frame);
    return frame;
  }
}

class FakeFrame extends EventTarget {
  src = "";
  title = "";
  loading = "lazy";
  referrerPolicy = "";
  attributes = {};
  removed = false;
  removalCount = 0;
  contentWindow = new FakeFrameWindow();
  sandbox = {
    tokens: new Set(),
    add: (token) => {
      this.sandbox.tokens.add(token);
    },
  };

  setAttribute(name, value) {
    this.attributes[name] = value;
  }

  remove() {
    this.removalCount += 1;
    this.removed = true;
  }
}

class FakeFrameWindow {
  messages = [];

  postMessage(message, targetOrigin) {
    this.messages.push({ message, targetOrigin });
  }
}

async function connectFake(window, frame, origin = "null") {
  const child = new FakePort();
  const host = new FakePort();
  child.peer = host;
  host.peer = child;
  frame.port = child;
  window.dispatch({ origin, source: frame.contentWindow, data: surfaceBridgeControl("connect"), ports: [host] });
  if (!host.closed) child.postMessage(surfaceBridgeControl("confirm"));
  return child;
}

class FakePort {
  listeners = new Map([['message', new Set()], ['messageerror', new Set()], ['close', new Set()]]);
  received = [];
  closed = false;
  get responses() { return this.received.filter(({ message }) => message.type === "babel.rpc.response"); }
  postMessage(message) {
    if (this.closed || this.peer.closed) return;
    this.peer.received.push({ message });
    for (const handler of this.peer.listeners.get("message")) handler({ data: message });
  }
  addEventListener(type, handler) { this.listeners.get(type).add(handler); }
  removeEventListener(type, handler) { this.listeners.get(type).delete(handler); }
  start() {}
  close() { this.closed = true; }
}
