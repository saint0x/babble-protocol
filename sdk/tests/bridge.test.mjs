import assert from "node:assert/strict";
import test from "node:test";
import {
  BabbleError,
  BrowserBridgeHost,
  BrowserBridgeTransport,
  createBabbleClient,
  createSurfaceSDK,
  hostBinding,
  isRpcBridgeRequest,
  isRpcBridgeCancel,
  rpcBridgeResponse,
} from "../dist/index.js";

test("BrowserBridgeTransport exchanges canonical RPC envelopes", async () => {
  const [objectEndpoint, hostEndpoint] = bridgePair("https://object.test", "https://host.test");
  hostEndpoint.addEventListener("message", (event) => {
    assert.equal(event.origin, "https://object.test");
    assert.equal(isRpcBridgeRequest(event.data), true);
    hostEndpoint.postMessage(
      rpcBridgeResponse({
        protocol: "babble.rpc.v1",
        id: event.data.envelope.id,
        result: { results: [] },
        error: null,
        trace_id: "trace-bridge",
      }),
      "https://object.test",
    );
  });

  const client = createBabbleClient({
    transport: new BrowserBridgeTransport(objectEndpoint, {
      targetOrigin: "https://host.test",
      allowedOrigins: ["https://host.test"],
    }),
    binding: hostBinding("runtime", "https://object.test"),
  });

  const result = await client.request(
    "babble.search.objects.v1",
    { q: "lenses", author: null, kind: null, limit: 3 },
    { id: "bridge-1" },
  );

  assert.deepEqual(result.results, []);
  assert.deepEqual(objectEndpoint.messages.map((message) => message.targetOrigin), ["https://host.test"]);
  client.close();
});

test("BrowserBridgeHost serves Surface SDK calls over the bridge", async () => {
  const [objectEndpoint, hostEndpoint] = bridgePair("https://object.test", "https://host.test");
  const host = new BrowserBridgeHost(
    hostEndpoint,
    (request) => {
      assert.equal(request.method, "babble.realtime.session.start.v1");
      assert.equal(request.binding.object_id, "obj_surface");
      assert.equal(request.binding.surface_session_id, "surface_1");
      assert.deepEqual(request.binding.capability_grants, ["grant_join"]);
      return {
        protocol: "babble.rpc.v1",
        id: request.id,
        result: {
          session: {
            id: "session_1",
            room_id: "room_1",
            member: "id_alice",
            joined_at: "2026-09-27T00:00:00Z",
            resume_token: "resume_1",
          },
        },
        error: null,
        trace_id: request.trace_id ?? null,
      };
    },
    {
      targetOrigin: "https://object.test",
      allowedOrigins: ["https://object.test"],
    },
  );
  const sdk = createSurfaceSDK({
    endpoint: objectEndpoint,
    plan: readySurfacePlan(),
    runtimeId: "runtime",
    surfaceSessionId: "surface_1",
    origin: "https://object.test",
    targetOrigin: "https://host.test",
    allowedOrigins: ["https://host.test"],
  });

  const response = await sdk.realtime.startSession(
    { author_id: "id_alice", room_id: "room_1" },
    { id: "session-request-1" },
  );

  assert.equal(response.session.id, "session_1");
  sdk.close();
  host.close();
});

test("BrowserBridgeHost translates dispatch failures into structured RPC errors", async () => {
  const [objectEndpoint, hostEndpoint] = bridgePair("https://object.test", "https://host.test");
  const host = new BrowserBridgeHost(hostEndpoint, () => {
    throw new Error("host dispatcher unavailable");
  });
  const client = createBabbleClient({
    transport: new BrowserBridgeTransport(objectEndpoint),
    binding: hostBinding("runtime", "https://object.test"),
  });

  await assert.rejects(
    client.request("babble.search.objects.v1", { q: "babble", author: null, kind: null, limit: 3 }, { id: "failure-1" }),
    (error) => {
      assert.ok(error instanceof BabbleError);
      assert.equal(error.code, "INTERNAL");
      assert.equal(error.message, "host dispatcher unavailable");
      return true;
    },
  );

  client.close();
  host.close();
});

test("BrowserBridgeHost enforces inbound size and in-flight request limits", async () => {
  const [objectEndpoint, hostEndpoint] = bridgePair("https://object.test", "https://host.test");
  let releaseFirst;
  let firstWireId;
  const firstDispatch = new Promise((resolve) => {
    releaseFirst = () =>
      resolve({
        protocol: "babble.rpc.v1",
        id: firstWireId,
        result: { results: [] },
        error: null,
        trace_id: null,
      });
  });
  const host = new BrowserBridgeHost(
    hostEndpoint,
    (request) => {
      if (request.payload.q === "first") {
        firstWireId = request.id;
        return firstDispatch;
      }
      return {
        protocol: "babble.rpc.v1",
        id: request.id,
        result: { results: [] },
        error: null,
        trace_id: null,
      };
    },
    {
      targetOrigin: "https://object.test",
      allowedOrigins: ["https://object.test"],
      maxInboundBytes: 900,
      maxInFlightRequests: 1,
    },
  );
  const client = createBabbleClient({
    transport: new BrowserBridgeTransport(objectEndpoint, {
      targetOrigin: "https://host.test",
      allowedOrigins: ["https://host.test"],
    }),
    binding: hostBinding("runtime", "https://object.test"),
  });

  const first = client.request(
    "babble.search.objects.v1",
    { q: "first", author: null, kind: null, limit: 3 },
    { id: "first" },
  );
  await assert.rejects(
    client.request("babble.search.objects.v1", { q: "second", author: null, kind: null, limit: 3 }, { id: "second" }),
    (error) => {
      assert.ok(error instanceof BabbleError);
      assert.equal(error.code, "RATE_LIMITED");
      assert.equal(error.retryable, true);
      return true;
    },
  );
  releaseFirst();
  assert.deepEqual(await first, { results: [] });

  await assert.rejects(
    client.request(
      "babble.search.objects.v1",
      { q: "x".repeat(1000), author: null, kind: null, limit: 3 },
      { id: "oversized" },
    ),
    (error) => {
      assert.ok(error instanceof BabbleError);
      assert.equal(error.code, "QUOTA_EXCEEDED");
      assert.match(error.message, /inbound byte limit/);
      return true;
    },
  );

  client.close();
  host.close();
});

test("BrowserBridgeHost times out hung dispatches without sending late duplicate responses", async () => {
  const [objectEndpoint, hostEndpoint] = bridgePair("https://object.test", "https://host.test");
  let releaseHung;
  const hungDispatch = new Promise((resolve) => {
    releaseHung = () =>
      resolve({
        protocol: "babble.rpc.v1",
        id: "hung",
        result: { results: [] },
        error: null,
        trace_id: null,
      });
  });
  const host = new BrowserBridgeHost(hostEndpoint, () => hungDispatch, {
    targetOrigin: "https://object.test",
    allowedOrigins: ["https://object.test"],
    maxDispatchMs: 10,
  });
  const client = createBabbleClient({
    transport: new BrowserBridgeTransport(objectEndpoint, {
      targetOrigin: "https://host.test",
      allowedOrigins: ["https://host.test"],
    }),
    binding: hostBinding("runtime", "https://object.test"),
  });

  await assert.rejects(
    client.request("babble.search.objects.v1", { q: "timeout", author: null, kind: null, limit: 3 }, { id: "hung" }),
    (error) => {
      assert.ok(error instanceof BabbleError);
      assert.equal(error.code, "TIMEOUT");
      assert.equal(error.retryable, true);
      return true;
    },
  );
  releaseHung();
  await new Promise((resolve) => setTimeout(resolve, 0));
  assert.equal(hostEndpoint.messages.filter((message) => message.data.response?.id === objectEndpoint.messages[0].data.envelope.id).length, 1);

  client.close();
  host.close();
});

test("BrowserBridgeHost drops malformed protocol-shaped bridge requests before dispatch", async () => {
  const [objectEndpoint, hostEndpoint] = bridgePair("https://object.test", "https://host.test");
  const dispatches = [];
  const host = new BrowserBridgeHost(
    hostEndpoint,
    (request) => {
      dispatches.push(request);
      return {
        protocol: "babble.rpc.v1",
        id: request.id,
        result: { results: [] },
        error: null,
        trace_id: null,
      };
    },
    {
      targetOrigin: "https://object.test",
      allowedOrigins: ["https://object.test"],
    },
  );
  const valid = bridgeRequest("valid-shape");
  const malformedMessages = [
    null,
    { type: "babble.rpc.request", protocol: "babble.rpc.v1", envelope: {} },
    bridgeRequest("missing-payload", { payload: undefined }),
    bridgeRequest("unknown-method", { method: "babble.unknown.method.v1" }),
    bridgeRequest("bad-binding", { binding: { ...requestEnvelope("x").binding, capability_grants: ["grant", 7] } }),
    bridgeRequest("bad-deadline", { deadline: { timeout_ms: 0, client_started_at: null } }),
    bridgeRequest("bad-id", { id: "" }),
  ];

  for (const message of malformedMessages) {
    objectEndpoint.postMessage(message, "https://host.test");
  }
  objectEndpoint.postMessage(valid, "https://host.test");
  await new Promise((resolve) => setTimeout(resolve, 0));

  assert.equal(dispatches.length, 1);
  assert.equal(dispatches[0].id, "valid-shape");
  assert.equal(hostEndpoint.messages.length, 1);
  assert.equal(hostEndpoint.messages[0].data.response.id, "valid-shape");
  assert.equal(isRpcBridgeRequest(valid), true);
  for (const message of malformedMessages) {
    assert.equal(isRpcBridgeRequest(message), false);
  }

  host.close();
});

test("createSurfaceSDK refuses non-ready admission plans", () => {
  assert.throws(
    () =>
      createSurfaceSDK({
        endpoint: new MemoryEndpoint("https://object.test"),
        plan: { ...readySurfacePlan(), admission: "needs_permission" },
        runtimeId: "runtime",
        surfaceSessionId: "surface_1",
        origin: "https://object.test",
      }),
    /non-ready Surface admission/,
  );
});

test("BrowserBridgeTransport ignores responses from untrusted origins", async () => {
  const endpoint = new MemoryEndpoint("https://object.test");
  const transport = new BrowserBridgeTransport(endpoint, { allowedOrigins: ["https://host.test"] });
  let settled = false;
  const response = transport.request(requestEnvelope("origin-check")).then(value => {
    settled = true;
    return value;
  });
  const wireId = endpoint.messages[0].data.envelope.id;
  const reply = marker => rpcBridgeResponse({
    protocol: "babble.rpc.v1", id: wireId, result: { marker }, error: null, trace_id: null,
  });
  endpoint.dispatch({ origin: "https://attacker.test", data: reply("forged") });
  await Promise.resolve();
  assert.equal(settled, false);
  endpoint.dispatch({ origin: "https://host.test", data: reply("trusted") });
  assert.deepEqual((await response).result, { marker: "trusted" });
  assert.equal((await response).id, "origin-check");
  transport.close();
});

test("BrowserBridgeTransport aborts and closes pending requests", async () => {
  const endpoint = new MemoryEndpoint("https://object.test");
  const aborting = new BrowserBridgeTransport(endpoint);
  const controller = new AbortController();
  const aborted = aborting.request(requestEnvelope("abort-1"), { signal: controller.signal });
  controller.abort(new Error("surface suspended"));
  await assert.rejects(aborted, /surface suspended/);
  aborting.close();

  const closing = new BrowserBridgeTransport(endpoint);
  const pending = closing.request(requestEnvelope("close-1"));
  closing.close();
  await assert.rejects(pending, /transport closed/);
});

for (const outcome of ["resolve", "reject", "timeout"]) {
  test(`closed hosts clear timers and suppress late ${outcome} and queued input`, async (t) => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const clear = t.mock.method(globalThis, "clearTimeout");
    const endpoint = new MemoryEndpoint("https://host.test");
    let settle;
    let signal;
    let calls = 0;
    const host = new BrowserBridgeHost(endpoint, (request, context) => {
      calls += 1;
      signal = context.signal;
      return new Promise((resolve, reject) => {
        settle = () => outcome === "reject" ? reject(new Error("late failure")) : resolve({ protocol: "babble.rpc.v1", id: request.id, result: null, error: null });
      });
    }, { maxDispatchMs: 10 });
    const queued = [...endpoint.listeners][0];
    endpoint.dispatch({ data: bridgeRequest("pending"), origin: "https://object.test" });
    host.close();
    host.close();
    assert.equal(clear.mock.callCount(), 1);
    assert.equal(endpoint.listeners.size, 0);
    assert.equal(signal.aborted, true);
    queued({ data: bridgeRequest("queued"), origin: "https://object.test" });
    if (outcome !== "timeout") settle();
    t.mock.timers.tick(100);
    await Promise.resolve();
    assert.equal(calls, 1);
    assert.equal(endpoint.messages.length, 0);
    if (outcome === "timeout") { settle(); await Promise.resolve(); }
  });
}

test("duplicate request IDs cannot dispatch twice or release occupied capacity", async () => {
  const endpoint = new MemoryEndpoint("https://host.test");
  const releases = new Map();
  const calls = [];
  const host = new BrowserBridgeHost(endpoint, (request) => {
    calls.push(request.id);
    return new Promise((resolve) => releases.set(request.id, () => resolve({ protocol: "babble.rpc.v1", id: request.id, result: null, error: null })));
  }, { maxInFlightRequests: 2 });
  const send = (id) => endpoint.dispatch({ data: bridgeRequest(id) });
  send("a");
  send("a");
  send("b");
  send("c");
  assert.deepEqual(calls, ["a", "b"]);
  assert.deepEqual(endpoint.messages.map(({ data }) => data.response.error.code), ["INVALID_INPUT", "RATE_LIMITED"]);
  releases.get("a")();
  await Promise.resolve();
  send("c");
  assert.deepEqual(calls, ["a", "b", "c"]);
  host.close();
  releases.get("b")();
  releases.get("c")();
  await Promise.resolve();
  assert.equal(endpoint.messages.length, 3);
});

test("timeouts signal cancellation without freeing capacity before dispatch settles", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const endpoint = new MemoryEndpoint("https://host.test");
  let release;
  let signal;
  let calls = 0;
  const host = new BrowserBridgeHost(endpoint, (request, context) => {
    calls += 1;
    signal = context.signal;
    return new Promise((resolve) => { release = () => resolve({ protocol: "babble.rpc.v1", id: request.id, result: null, error: null }); });
  }, { maxInFlightRequests: 1, maxDispatchMs: 10 });
  const send = (id) => endpoint.dispatch({ data: bridgeRequest(id) });
  send("a");
  t.mock.timers.tick(10);
  assert.equal(signal.aborted, true);
  send("a");
  send("b");
  assert.equal(calls, 1);
  assert.deepEqual(endpoint.messages.map(({ data }) => data.response.error.code), ["TIMEOUT", "INVALID_INPUT", "RATE_LIMITED"]);
  release();
  await Promise.resolve();
  assert.equal(endpoint.messages.length, 3);
  send("b");
  assert.equal(calls, 2);
  host.close();
  release();
  await Promise.resolve();
});

test("reentrant close from dispatch suppresses even synchronous responses", async () => {
  const endpoint = new MemoryEndpoint("https://host.test");
  const host = new BrowserBridgeHost(endpoint, (request, context) => {
    host.close();
    assert.equal(context.signal.aborted, true);
    return { protocol: "babble.rpc.v1", id: request.id, result: null, error: null };
  });
  endpoint.dispatch({ data: bridgeRequest("close-during-dispatch") });
  await Promise.resolve();
  assert.equal(endpoint.messages.length, 0);
});

test("close clears every deadline before invoking reentrant cancellation observers", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const endpoint = new MemoryEndpoint("https://host.test");
  const signals = [];
  const releases = [];
  const host = new BrowserBridgeHost(endpoint, (request, context) => {
    signals.push(context.signal);
    context.signal.addEventListener("abort", () => {
      t.mock.timers.tick(100);
      endpoint.dispatch({ data: bridgeRequest("reentrant") });
    });
    return new Promise((resolve) => releases.push(() => resolve({ protocol: "babble.rpc.v1", id: request.id, result: null, error: null })));
  }, { maxDispatchMs: 10 });
  endpoint.dispatch({ data: bridgeRequest("a") });
  endpoint.dispatch({ data: bridgeRequest("b") });
  host.close();
  assert.equal(signals.length, 2);
  assert.equal(signals.every((signal) => signal.aborted), true);
  releases.forEach((release) => release());
  await Promise.resolve();
  assert.equal(endpoint.messages.length, 0);
});

test("close, dispatch settlement and deadline permutations never emit after close", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const orders = [
    ["close", "settle", "timeout"], ["close", "timeout", "settle"],
    ["settle", "close", "timeout"], ["settle", "timeout", "close"],
    ["timeout", "close", "settle"], ["timeout", "settle", "close"],
  ];
  for (const outcome of ["resolve", "reject"]) {
    for (const order of orders) {
      const endpoint = new MemoryEndpoint("https://host.test");
      let settle;
      const host = new BrowserBridgeHost(endpoint, (request) => new Promise((resolve, reject) => {
        settle = () => outcome === "resolve"
          ? resolve({ protocol: "babble.rpc.v1", id: request.id, result: null, error: null })
          : reject(new Error("dispatch rejected"));
      }), { maxDispatchMs: 10 });
      endpoint.dispatch({ data: bridgeRequest("permutation") });
      let messagesAtClose;
      for (const action of order) {
        if (action === "close") { host.close(); messagesAtClose = endpoint.messages.length; }
        if (action === "settle") settle();
        if (action === "timeout") t.mock.timers.tick(10);
        await Promise.resolve();
        if (messagesAtClose !== undefined) assert.equal(endpoint.messages.length, messagesAtClose, `${outcome}: ${order}`);
      }
      assert.ok(endpoint.messages.length <= 1);
      assert.equal(endpoint.listeners.size, 0);
    }
  }
});

test("caller abort cancels only its host dispatch and keeps capacity until settlement", async () => {
  const [clientEndpoint, hostEndpoint] = bridgePair("https://object.test", "https://host.test");
  const signals = new Map();
  const releases = new Map();
  const host = new BrowserBridgeHost(hostEndpoint, (request, { signal }) => {
    signals.set(request.payload.q, signal);
    return new Promise(resolve => releases.set(request.payload.q, () => resolve({
      protocol: "babble.rpc.v1", id: request.id, result: { results: [] }, error: null, trace_id: null,
    })));
  }, { maxInFlightRequests: 2 });
  const transport = new BrowserBridgeTransport(clientEndpoint);
  const controller = new AbortController();
  const input = { ...requestEnvelope("same-id"), payload: { q: "first" }, idempotency_key: "stable-operation" };
  const first = assert.rejects(transport.request(input, { signal: controller.signal }), /dismissed/);
  const second = transport.request({ ...requestEnvelope("other"), payload: { q: "other" } });
  await Promise.resolve();
  controller.abort(new Error("dismissed"));
  await first;
  await Promise.resolve();
  assert.equal(signals.get("first").aborted, true);
  assert.equal(signals.get("other").aborted, false);
  const limited = await transport.request(requestEnvelope("blocked"));
  assert.equal(limited.error.code, "RATE_LIMITED");
  assert.equal(signals.size, 2);
  assert.equal(input.id, "same-id", "the caller's envelope must not be mutated");
  assert.equal(clientEndpoint.messages[0].data.envelope.idempotency_key, "stable-operation");
  const cancellation = clientEndpoint.messages.find(({ data }) => isRpcBridgeCancel(data)).data;
  assert.equal(cancellation.id, clientEndpoint.messages[0].data.envelope.id);
  assert.notEqual(cancellation.id, input.id);
  releases.get("first")();
  releases.get("other")();
  assert.equal((await second).id, "other");
  await Promise.resolve();
  assert.equal(hostEndpoint.messages.some(({ data }) => data.response?.id === cancellation.id), false);
  transport.close(); host.close();
});

for (const cause of ["timeout", "close"]) {
  test(`transport ${cause} sends cancellation and suppresses the host's late response`, async t => {
    t.mock.timers.enable({ apis: ["setTimeout"] });
    const [clientEndpoint, hostEndpoint] = bridgePair("https://object.test", "https://host.test");
    let signal, release;
    const host = new BrowserBridgeHost(hostEndpoint, (request, context) => {
      signal = context.signal;
      return new Promise(resolve => { release = () => resolve({ protocol: "babble.rpc.v1", id: request.id, result: null, error: null }); });
    });
    const transport = new BrowserBridgeTransport(clientEndpoint);
    const rejected = assert.rejects(transport.request(requestEnvelope("pending"), { timeoutMs: 10 }), /timed out|closed/);
    await Promise.resolve();
    if (cause === "close") transport.close();
    else t.mock.timers.tick(10);
    await rejected;
    await Promise.resolve();
    assert.equal(signal.aborted, true);
    assert.equal(clientEndpoint.messages.filter(({ data }) => isRpcBridgeCancel(data)).length, 1);
    release();
    await Promise.resolve();
    t.mock.timers.tick(30_000);
    assert.equal(hostEndpoint.messages.length, 0);
    transport.close(); host.close();
  });
}

test("late legacy-host replies cannot resolve a reused caller ID", async () => {
  const endpoint = new MemoryEndpoint("https://object.test");
  const transport = new BrowserBridgeTransport(endpoint, { allowedOrigins: ["https://host.test"] });
  const controller = new AbortController();
  const first = assert.rejects(transport.request(requestEnvelope("reused"), { signal: controller.signal }), /old attempt/);
  const firstWireId = endpoint.messages[0].data.envelope.id;
  controller.abort(new Error("old attempt"));
  await first;
  let settled = false;
  const next = transport.request(requestEnvelope("reused")).then(value => { settled = true; return value; });
  const secondWireId = endpoint.messages.at(-1).data.envelope.id;
  assert.notEqual(firstWireId, secondWireId);
  const reply = (id, result) => endpoint.dispatch({ origin: "https://host.test", data: rpcBridgeResponse({
    protocol: "babble.rpc.v1", id, result, error: null, trace_id: null,
  }) });
  reply(firstWireId, { obsolete: true });
  await Promise.resolve();
  assert.equal(settled, false);
  reply(secondWireId, { current: true });
  assert.deepEqual((await next).result, { current: true });
  assert.equal((await next).id, "reused");
  transport.close();
});

test("cancellation requires valid shape and the dispatch origin, and cannot pre-cancel a request", async t => {
  t.mock.timers.enable({ apis: ["setTimeout"] });
  const endpoint = new MemoryEndpoint("https://host.test");
  let signal, release, aborts = 0;
  const host = new BrowserBridgeHost(endpoint, (request, context) => {
    signal = context.signal;
    signal.addEventListener("abort", () => { aborts++; t.mock.timers.tick(30_000); });
    return new Promise(resolve => { release = () => resolve({ protocol: "babble.rpc.v1", id: request.id, result: null, error: null }); });
  }, { allowedOrigins: ["https://object.test", "https://other.test"] });
  const cancel = { type: "babble.rpc.cancel", protocol: "babble.rpc.v1", id: "same" };
  const send = (data, origin = "https://object.test") => endpoint.dispatch({ data, origin });
  send(cancel);
  send(bridgeRequest("same"));
  for (const invalid of [
    { ...cancel, protocol: "babble.rpc" }, { ...cancel, id: "" },
    { ...cancel, id: "x".repeat(129) }, { ...cancel, reason: "extra data" },
    { ...cancel, id: "missing" },
  ]) send(invalid);
  send(cancel, "https://other.test");
  send(cancel, "https://attacker.test");
  assert.equal(signal.aborted, false);
  send(cancel);
  send(cancel);
  assert.equal(signal.aborted, true);
  assert.equal(aborts, 1);
  release();
  await Promise.resolve();
  assert.equal(endpoint.messages.length, 0, "reentrant cancellation cannot trigger the cleared deadline");
  host.close();
});

test("pre-aborted calls send nothing and completed calls do not send cancellation", async () => {
  const [clientEndpoint, hostEndpoint] = bridgePair("https://object.test", "https://host.test");
  const host = new BrowserBridgeHost(hostEndpoint, request => ({ protocol: "babble.rpc.v1", id: request.id, result: {}, error: null }));
  const transport = new BrowserBridgeTransport(clientEndpoint);
  await assert.rejects(transport.request(requestEnvelope("never-sent"), { signal: AbortSignal.abort(new Error("cancelled first")) }), /cancelled first/);
  assert.equal(clientEndpoint.messages.length, 0);
  const controller = new AbortController();
  assert.equal((await transport.request(requestEnvelope("finished"), { signal: controller.signal })).id, "finished");
  controller.abort();
  transport.close();
  assert.equal(clientEndpoint.messages.length, 1);
  host.close();
});

function requestEnvelope(id) {
  return {
    protocol: "babble.rpc.v1",
    id,
    method: "babble.search.objects.v1",
    binding: hostBinding("runtime", "https://object.test"),
    payload: { q: "babble", author: null, kind: null, limit: 3 },
    idempotency_key: null,
    deadline: {
      timeout_ms: 30000,
      client_started_at: "2026-09-27T00:00:00Z",
    },
    trace_id: null,
  };
}

function bridgeRequest(id, envelopeOverrides = {}) {
  return {
    type: "babble.rpc.request",
    protocol: "babble.rpc.v1",
    envelope: {
      ...requestEnvelope(id),
      ...envelopeOverrides,
    },
  };
}

function readySurfacePlan() {
  return {
    object_id: "obj_surface",
    surface: {
      role: "Feed",
      target: "Web",
      entry: "https://object.test/surface.js",
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
      csp: "default-src 'none'",
      host_cookies: false,
      top_navigation: false,
      wasi_filesystem: false,
      wasi_network: false,
      capability_bridge: true,
    },
    capability_decisions: [
      {
        request: { id: "babble.realtime.join", version: 1, scope: { room: "room_1" } },
        status: "granted",
        reason: "grant is active",
        definition: {
          id: "babble.realtime.join",
          version: 1,
          permission: "ask_once",
          quota: {
            calls_per_minute: 60,
            bytes_per_minute: 1048576,
            persistent_bytes: 0,
            max_call_ms: 30000,
            realtime_connections: 1,
            background_allowed: false,
          },
          request_schema: {},
          response_schema: {},
        },
        grant: {
          id: "grant_join",
          object_id: "obj_surface",
          capability: "babble.realtime.join",
          version: 1,
          scope: { room: "room_1" },
          decision: "approved",
          quota: {
            calls_per_minute: 60,
            bytes_per_minute: 1048576,
            persistent_bytes: 0,
            max_call_ms: 30000,
            realtime_connections: 1,
            background_allowed: false,
          },
          created_at: "2026-09-27T00:00:00Z",
          expires_at: null,
          revoked_at: null,
        },
      },
      {
        request: { id: "babble.realtime.send", version: 1, scope: { room: "room_1" } },
        status: "requires_user",
        reason: "user approval required",
        definition: null,
        grant: null,
      },
    ],
    blocked_reasons: [],
  };
}

function bridgePair(leftOrigin, rightOrigin) {
  const left = new MemoryEndpoint(leftOrigin);
  const right = new MemoryEndpoint(rightOrigin);
  left.peer = right;
  right.peer = left;
  return [left, right];
}

class MemoryEndpoint {
  messages = [];
  listeners = new Set();
  peer = null;

  constructor(origin) {
    this.origin = origin;
  }

  postMessage(data, targetOrigin) {
    this.messages.push({ data, targetOrigin });
    const peer = this.peer;
    if (!peer) {
      return;
    }
    queueMicrotask(() => {
      peer.dispatch({ data, origin: this.origin });
    });
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
