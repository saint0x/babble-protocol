import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import { MessageChannel } from "node:worker_threads";
import ts from "typescript";

const context = { exports: {}, Error, console, AbortController, performance, setTimeout, clearTimeout, require(name) {
  assert.equal(name, "./protocol");
  return { mountSurface: (input) => defaultMount(input) };
} };
vm.runInNewContext(ts.transpileModule(readFileSync(new URL("../src/app/surfaces.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText, context);
const { Surfaces, createSurfaceClock } = context.exports;

test("lease clock counts OS sleep even when the browser monotonic clock pauses", () => {
  let monotonic = 100, wall = 100_000;
  const clock = createSurfaceClock(() => monotonic, () => wall);
  wall += 61_000;
  assert.equal(clock.now(), 61_000);
  monotonic += 10; wall += 10;
  assert.equal(clock.now(), 61_010);
});

test("wall-clock corrections cannot rewind or stall subsequent lease time", () => {
  let monotonic = 0, wall = 100_000;
  const clock = createSurfaceClock(() => monotonic, () => wall);
  monotonic += 100; wall -= 60_000;
  assert.equal(clock.now(), 100);
  monotonic += 100; wall += 100;
  assert.equal(clock.now(), 200);
  wall += 3_600_000;
  assert.equal(clock.now(), 3_600_200);
  wall -= 3_600_000; monotonic += 100;
  assert.equal(clock.now(), 3_600_300);
});
let defaultMount;
const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
const settle = () => new Promise((resolve) => setImmediate(resolve));
const plan = (objectId = "object-a", admission = "ready") => ({
  object_id: objectId, admission, blocked_reasons: admission === "ready" ? [] : ["Permission required"],
  lifecycle: "cold", surface: { role: "Feed", target: "Web", entry: "https://surface.test/app", min_version: 1 },
  budget: { memory_bytes: 1024, cpu_ms_per_minute: 100, network_bytes_per_minute: 1024,
    persistent_storage_bytes: 0, realtime_connections: 0, gpu_expected: false },
  capability_decisions: [], sandbox: { isolated_origin: true },
});
const session = (objectId = "object-a", id = `session-${objectId}`, finalPlan = plan(objectId), lifecycle = "prefetched") => ({
  id, plan: finalPlan, lifecycle, budget: finalPlan.budget, events: [],
  created_at: "2026-09-30T00:00:00Z", updated_at: "2026-09-30T00:00:00Z",
});
const transition = (current, lifecycle) => ({
  session: { ...current, lifecycle },
  event: { session_id: current.id, object_id: current.plan.object_id, lifecycle, kind: "lifecycle_transition",
    reason: "host", sequence: lifecycle === "warm" ? 1 : 2, at: current.updated_at, budget: current.budget },
});

const lease = (sessionId = "session-object-a", ttl = 60_000, renew = 15_000) => ({
  session_id: sessionId, expires_at: "2026-09-30T00:01:00Z", ttl_ms: ttl, renew_after_ms: renew,
});

function controlledClock() {
  let now = 0, sequence = 0;
  const timers = new Map();
  return {
    now: () => now,
    setTimeout(callback, delay) { const id = ++sequence; timers.set(id, { callback, at: now + delay }); return id; },
    clearTimeout(id) { timers.delete(id); },
    get pending() { return timers.size; },
    // jump() models a suspended page whose timer callbacks have not run yet.
    jump(milliseconds) { now += milliseconds; },
    async advance(milliseconds) {
      const target = now + milliseconds;
      for (;;) {
        const next = [...timers].filter(([, timer]) => timer.at <= target).sort((a, b) => a[1].at - b[1].at)[0];
        if (!next) break;
        now = Math.max(now, next[1].at);
        timers.delete(next[0]); next[1].callback();
        await settle();
      }
      now = target;
      await settle();
    },
  };
}

function harness(options = {}) {
  const states = [], errors = [], calls = [], mounts = [], remote = new Map();
  const clock = options.clock ?? controlledClock();
  const heartbeats = [];
  let allowed = true;
  const container = { childNodes: [], appendChild(child) { this.childNodes.push(child); return child; }, removeChild(child) {
    this.childNodes = this.childNodes.filter((value) => value !== child);
  } };
  const source = {
    async prepareSurface(objectId) {
      calls.push(["prepare", objectId]);
      return options.prepare ? options.prepare(objectId) : plan(objectId);
    },
    async startSurfaceSession(objectId) {
      calls.push(["start", objectId]);
      const value = options.start ? await options.start(objectId) : session(objectId);
      remote.set(value.id, value);
      return value;
    },
    async heartbeatSurfaceSession(id, signal) {
      calls.push(["heartbeat", id]);
      heartbeats.push({ id, signal, at: clock.now() });
      return options.heartbeat ? options.heartbeat(id, signal, heartbeats.length) : lease(id);
    },
    async registerSurfaceDocument(id, documentId, signal) {
      calls.push(["document", id, documentId]);
      await options.registerDocument?.(id, documentId, signal);
    },
    async transitionSurfaceSession(id, lifecycle, reason) {
      calls.push([lifecycle, id, reason]);
      const result = options.transition ? await options.transition(id, lifecycle, remote.get(id)) : transition(remote.get(id), lifecycle);
      remote.set(id, result.session);
      return result;
    },
  };
  const mount = (input) => {
    const frame = { remove() { input.container.removeChild(this); } };
    input.container.appendChild(frame);
    if (options.mountFailure) throw options.mountFailure;
    const listeners = new Set();
    const value = { input, frame, evictions: [], events: [], sessions: [], unmounts: 0,
      ready: typeof options.ready === "function" ? options.ready(input)
        : Object.hasOwn(options, "ready") ? options.ready : Promise.resolve(),
      emit(event) { for (const listener of listeners) listener(event); },
      lifecycle: {
        onChange(listener) { listeners.add(listener); return () => listeners.delete(listener); },
        applySession(value) { value && mounts.at(-1).sessions.push(value); },
        applyRuntimeEvent(event) { value.events.push(event); },
      },
      evict(reason) {
        value.evictions.push(reason);
        if (options.evictFailure) throw options.evictFailure;
        frame.remove();
      },
      unmount() { value.unmounts++; frame.remove(); },
    };
    mounts.push(value);
    options.afterMount?.();
    return value;
  };
  const dispatchCalls = [];
  const dispatch = async (...args) => {
    const [request] = args;
    dispatchCalls.push(request);
    return options.dispatch ? options.dispatch(...args) : { id: request.id, result: "allowed" };
  };
  const hooks = {
    clock,
    onState(state) { states.push(state); options.onState?.(state); },
    onCleanupError(error) { errors.push(error); options.onCleanupError?.(error); },
    ...(options.defaultMount ? {} : { mount: options.mount ?? mount }),
  };
  if (options.defaultMount) defaultMount = mount;
  const control = new Surfaces(hooks);
  const input = (objectId = "object-a") => ({ objectId, container, currentIdentityId: "account-a", source, dispatch, authorized: () => allowed });
  return { control, states, errors, calls, mounts, remote, container, dispatchCalls, input, clock, heartbeats,
    open: (objectId) => control.open(input(objectId)), revoke: () => { allowed = false; } };
}
const stages = ["prepare", "start", "ready", "warm", "active"];
function paused(stage, extra = {}) {
  const gate = deferred();
  const h = harness({ ...extra,
    ...(stage === "prepare" ? { prepare: () => gate.promise } : {}),
    ...(stage === "start" ? { start: () => gate.promise } : {}),
    ...(stage === "ready" ? { ready: (input) => input.plan.object_id === "object-a" ? gate.promise : Promise.resolve() } : {}),
    transition: (id, lifecycle, current) => lifecycle === stage ? gate.promise : transition(current, lifecycle),
  });
  const release = () => gate.resolve(stage === "prepare" ? plan() : stage === "start" ? session() : transition(session(), stage));
  return { ...h, gate, release };
}

test("document readiness gates warm and active while heartbeats keep renewing", async () => {
  const h = paused("ready");
  const opening = h.open(); await settle();
  assert.equal(h.control.phase, "mounting");
  assert.deepEqual(h.calls.map(([kind]) => kind), ["prepare", "start", "heartbeat"]);
  await h.clock.advance(45_000);
  assert.deepEqual(h.heartbeats.map(({ at }) => at), [0, 15_000, 30_000, 45_000]);
  assert.equal(h.control.phase, "mounting");
  assert.equal(h.mounts[0].events.length, 0);
  h.release(); await opening;
  assert.equal(h.control.phase, "active");
  assert.deepEqual(h.mounts[0].events.map(({ lifecycle }) => lifecycle), ["warm", "active"]);
  await h.control.close();
  assert.equal(h.clock.pending, 0);
});

for (const ready of [undefined, null, {}, true, { then: true }]) {
  test(`invalid readiness contract cleans up visibly: ${JSON.stringify(ready)}`, async () => {
    const h = harness({ ready });
    await h.open();
    assert.equal(h.control.phase, "error");
    assert.match(h.control.state.message, /missing or invalid ready promise/);
    assert.equal(h.container.childNodes.length, 0);
    assert.equal(h.clock.pending, 0);
    assert.deepEqual(h.calls.map(([kind]) => kind), ["prepare", "start", "heartbeat", "evicted"]);
  });
}

for (const lifecycleFailure of [false, true]) {
  test(`handshake timeout is visible with synchronous lifecycle disposal: ${lifecycleFailure}`, async () => {
    const h = paused("ready");
    const opening = h.open(); await settle();
    h.gate.reject(new Error("Surface bridge handshake timed out"));
    if (lifecycleFailure) h.mounts[0].emit({ current: "evicted", reason: "Surface bridge handshake timed out" });
    await opening;
    assert.equal(h.control.phase, "error");
    assert.match(h.control.state.message, /handshake timed out/);
    assert.match(h.control.state.message, /could not establish a secure bridge.*Reopen the Surface or update/);
    assert.equal(h.container.childNodes.length, 0);
    assert.equal(h.clock.pending, 0);
    assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 1);
    assert.equal(h.calls.some(([kind]) => kind === "warm" || kind === "active"), false);
  });
}

for (const action of ["close", "replace", "authority", "lease", "heartbeat"]) {
  for (const result of ["resolve", "reject"]) {
    test(`${action} drains pending readiness before late ${result}`, async () => {
      const h = paused("ready", { heartbeat: (id, signal, count) => {
        if (action === "heartbeat" && count > 1) throw new Error("Login expired");
        return lease(id);
      } });
      let drained = false;
      const opening = h.open().then(() => { drained = true; });
      await settle();
      let closing;
      if (action === "replace") await h.open("object-b");
      else if (action === "close") closing = h.control.close();
      else if (action === "heartbeat") await h.clock.advance(15_000);
      else {
        if (action === "authority") h.revoke();
        else h.clock.jump(60_000);
        assert.equal(h.control.checkLease(), false);
      }
      assert.equal(h.container.childNodes.length, action === "replace" ? 1 : 0);
      await settle();
      assert.equal(drained, true, "startup must settle before the ready promise");
      await Promise.all([opening, closing]);
      const state = h.control.state;
      assert.equal(h.control.phase, action === "replace" ? "active"
        : ["lease", "heartbeat"].includes(action) ? "error" : "idle");
      assert.equal(h.calls.filter(([kind, id]) => kind === "evicted" && id === "session-object-a").length, 1);
      assert.equal(h.calls.some(([kind, id]) => (kind === "warm" || kind === "active") && id === "session-object-a"), false);
      if (result === "resolve") h.release();
      else h.gate.reject(new Error("Old document handshake failed"));
      await settle();
      assert.equal(h.control.state, state);
      assert.equal(h.errors.length, 0);
      await h.control.close();
      assert.equal(h.clock.pending, 0);
    });
  }
}

test("lease expiry cancels a pending renewal and readiness independently", async () => {
  const heartbeat = deferred();
  const h = paused("ready", { heartbeat: (id, signal, count) => count === 1 ? lease(id, 10_000, 4_000) : heartbeat.promise });
  let drained = false;
  const opening = h.open().then(() => { drained = true; });
  await settle();
  await h.clock.advance(10_000);
  assert.equal(drained, true);
  assert.equal(h.control.phase, "error");
  assert.match(h.control.state.message, /lease expired/);
  assert.equal(h.heartbeats[1].signal.aborted, true);
  assert.equal(h.container.childNodes.length, 0);
  assert.equal(h.clock.pending, 0);
  await opening;
  heartbeat.resolve(lease()); h.gate.reject(new Error("Late handshake failure"));
  await settle();
  assert.equal(h.control.phase, "error");
  assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 1);
});

test("synchronous close during mount consumes rejected readiness without waiting", async () => {
  const gate = deferred();
  const h = harness({ ready: gate.promise, afterMount: () => {
    void h.control.close();
    gate.reject(new Error("Surface unmounted before bridge handshake"));
  } });
  await h.open(); await h.control.close();
  assert.equal(h.control.phase, "idle");
  assert.equal(h.mounts[0].evictions.length, 1);
  assert.equal(h.errors.length, 0);
  assert.equal(h.clock.pending, 0);
});

test("initial heartbeat gates mounting and transitions, then renews without remounting", async () => {
  const gate = deferred();
  const h = harness({ heartbeat: (id, signal, count) => count === 1 ? gate.promise : lease(id) });
  const opening = h.open();
  await settle();
  assert.deepEqual(h.calls.map(([kind]) => kind), ["prepare", "start", "heartbeat"]);
  assert.equal(h.mounts.length, 0);
  gate.resolve(lease()); await opening;
  await h.clock.advance(45_000);
  assert.deepEqual(h.heartbeats.map(({ at }) => at), [0, 15_000, 30_000, 45_000]);
  assert.equal(h.mounts.length, 1);
  assert.equal(h.mounts[0].events.length, 2);
  assert.equal(h.states.filter(({ phase }) => phase === "active").length, 1);
  await h.control.close();
  assert.equal(h.clock.pending, 0);
});

test("heartbeat deadlines use request start and do not extend the old lease while pending", async () => {
  const gate = deferred();
  const h = harness({ heartbeat: (id, signal, count) => count === 1 ? lease(id, 10_000, 4_000) : gate.promise });
  await h.open();
  await h.clock.advance(4_000);
  await h.clock.advance(5_999);
  assert.equal(h.control.phase, "active");
  assert.equal(h.heartbeats.length, 2);
  await h.clock.advance(1);
  assert.equal(h.control.phase, "error");
  assert.match(h.control.state.message, /lease expired.*retry/);
  assert.equal(h.container.childNodes.length, 0);
  assert.equal(h.heartbeats[1].signal.aborted, true);
  assert.equal(h.clock.pending, 0);
  assert.equal(h.remote.get("session-object-a").lifecycle, "evicted");
  gate.resolve(lease()); await settle();
  assert.equal(h.control.phase, "error");
  assert.equal(h.clock.pending, 0);
});

test("successful delayed heartbeat schedules renewal and expiry from request start", async () => {
  const gates = [deferred(), deferred()];
  const h = harness({ heartbeat: (id, signal, count) => gates[count - 1]?.promise ?? new Promise(() => {}) });
  const opening = h.open(); await settle();
  h.clock.jump(3_000); gates[0].resolve(lease(undefined, 10_000, 4_000)); await opening;
  await h.clock.advance(1_000);
  assert.equal(h.heartbeats[1].at, 4_000);
  h.clock.jump(2_000); gates[1].resolve(lease(undefined, 10_000, 4_000)); await settle();
  await h.clock.advance(2_000);
  assert.equal(h.heartbeats[2].at, 8_000);
  await h.clock.advance(6_000);
  assert.equal(h.control.phase, "error");
  assert.equal(h.clock.now(), 14_000);
  assert.equal(h.clock.pending, 0);
});

for (const phase of ["initial", "renewal"]) {
  test(`${phase} heartbeat timeout cancels even an uncooperative transport and permits retry`, async () => {
    const h = harness({ heartbeat: (id, signal, count) => phase === "renewal" && count === 1 ? lease(id) : new Promise(() => {}) });
    const opening = h.open(); await settle();
    if (phase === "renewal") { await opening; await h.clock.advance(15_000); }
    await h.clock.advance(30_000);
    assert.equal(h.control.phase, "error");
    assert.match(h.control.state.message, /heartbeat timed out.*retry/);
    assert.equal(h.heartbeats.at(-1).signal.aborted, true);
    assert.equal(h.clock.pending, 0);
    assert.equal(h.container.childNodes.length, 0);
    await opening;
    await h.control.close();
    const other = harness();
    await h.control.open(other.input("object-retry"));
    assert.equal(h.control.phase, "active");
    await h.control.close();
  });
}

for (const value of [null, {}, { session_id: "different" }, { session_id: 7 }, { expires_at: "tomorrow" },
  { expires_at: "2026-09-30" }, { expires_at: "2026-02-30T00:01:00Z" }, { expires_at: "2026-09-30T24:00:00Z" },
  { ttl_ms: 0 }, { ttl_ms: -1 }, { ttl_ms: 1.5 }, { ttl_ms: Infinity },
  { ttl_ms: 2 ** 40 }, { ttl_ms: 60_001 }, { renew_after_ms: 0 }, { renew_after_ms: -1 }, { renew_after_ms: 1.5 },
  { renew_after_ms: 60_000 }, { ttl_ms: "60000" }]) {
  test(`malformed heartbeat lease closes before mount: ${JSON.stringify(value)}`, async () => {
    const h = harness({ heartbeat: () => value === null ? null : Object.keys(value).length ? { ...lease(), ...value } : {} });
    await h.open();
    assert.equal(h.control.phase, "error");
    assert.match(h.control.state.message, /invalid or mismatched lease.*retry/);
    assert.equal(h.mounts.length, 0);
    assert.equal(h.clock.pending, 0);
    assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 1);
  });
}

test("an already expired initial response never authorizes a mount", async () => {
  const gate = deferred();
  const h = harness({ heartbeat: () => gate.promise });
  const opening = h.open(); await settle();
  h.clock.jump(5_000); gate.resolve(lease(undefined, 5_000, 1_000)); await opening;
  assert.equal(h.mounts.length, 0);
  assert.equal(h.control.phase, "error");
  assert.match(h.control.state.message, /expired before heartbeat arrived/);
  assert.equal(h.clock.pending, 0);
});

test("a late successful heartbeat cannot revive a lease after suspended timers", async () => {
  const gate = deferred();
  const h = harness({ heartbeat: (id, signal, count) => count === 1 ? lease(id, 10_000, 4_000) : gate.promise });
  await h.open(); await h.clock.advance(4_000);
  h.clock.jump(6_000); gate.resolve(lease()); await settle();
  assert.equal(h.control.phase, "error");
  assert.equal(h.container.childNodes.length, 0);
  assert.equal(h.clock.pending, 0);
});

for (const stage of ["warm", "active"]) {
  test(`renewal continues during pending ${stage}; expiry cleans independently of the transition`, async () => {
    const gate = deferred();
    const h = harness({ heartbeat: (id, signal, count) => count <= 2 ? lease(id, 10_000, 4_000) : new Promise(() => {}),
      transition: (id, lifecycle, current) => lifecycle === stage ? gate.promise : transition(current, lifecycle) });
    const opening = h.open(); await settle();
    await h.clock.advance(4_000);
    assert.equal(h.heartbeats.length, 2);
    assert.equal(h.mounts.length, 1);
    await h.clock.advance(10_000);
    assert.equal(h.control.phase, "error");
    assert.equal(h.container.childNodes.length, 0);
    assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 1);
    assert.equal(h.clock.pending, 0);
    gate.resolve(transition(session(), stage)); await opening;
    assert.equal(h.control.phase, "error");
    assert.equal(h.control.sessionId, null);
    assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 1);
    await h.control.close();
  });
}

for (const disposition of ["resolve", "reject"]) {
  for (const phase of ["initial", "renewal"]) {
    test(`account replacement ignores late ${phase} heartbeat ${disposition}`, async () => {
      const gate = deferred();
      const h = harness({ heartbeat: (id, signal, count) => phase === "renewal" && count === 1 ? lease(id) : gate.promise });
      const first = h.open(); await settle();
      if (phase === "renewal") { await first; await h.clock.advance(15_000); }
      const other = harness();
      await h.control.open({ ...other.input("object-b"), currentIdentityId: "account-b", container: h.container });
      const replacementState = h.control.state;
      assert.equal(h.heartbeats.at(-1).signal.aborted, true);
      if (disposition === "resolve") gate.resolve(lease());
      else gate.reject(new Error("Old account transport failure"));
      await first; await settle();
      assert.equal(h.control.state, replacementState);
      assert.equal(h.control.sessionId, "session-object-b");
      assert.equal(h.container.childNodes.length, 1);
      assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 1);
      await h.control.close();
      assert.equal(h.clock.pending, 0);
    });
  }
}

test("final authority is checked after heartbeat success and failure", async () => {
  for (const disposition of ["resolve", "reject"]) {
    const gate = deferred();
    const h = harness({ heartbeat: () => gate.promise });
    const opening = h.open(); await settle(); h.revoke();
    if (disposition === "resolve") gate.resolve(lease()); else gate.reject(new Error("private account error"));
    await opening; await h.control.close();
    assert.equal(h.control.phase, "idle");
    assert.equal(h.states.some(({ phase }) => phase === "error"), false);
    assert.equal(h.mounts.length, 0);
    assert.equal(h.clock.pending, 0);
  }
});

test("checkLease synchronously removes stale resources before any timer or bridge output", async () => {
  const h = harness(); await h.open();
  assert.equal(h.control.checkLease(), true);
  h.clock.jump(60_000);
  assert.equal(h.control.checkLease(), false);
  assert.equal(h.container.childNodes.length, 0);
  assert.equal(h.control.phase, "error");
  assert.equal(h.clock.pending, 0);
  await assert.rejects(h.mounts[0].input.dispatch({ id: "too-late" }), /closed or its authority/);
  assert.equal(h.dispatchCalls.length, 0);
  await h.control.close();
  assert.equal(h.control.checkLease(), false);
});

test("authority revocation at renewal suppresses the request and disposes local resources", async () => {
  const h = harness(); await h.open(); h.revoke(); await h.clock.advance(15_000);
  assert.equal(h.heartbeats.length, 1);
  assert.equal(h.control.phase, "idle");
  assert.equal(h.container.childNodes.length, 0);
  assert.equal(h.clock.pending, 0);
});

test("renewal failure stays actionable despite local and remote cleanup failures", async () => {
  const h = harness({ heartbeat: (id, signal, count) => {
    if (count > 1) throw new Error("Login expired; sign in again");
    return lease(id);
  }, evictFailure: new Error("listener failed"), transition: (id, lifecycle, current) => {
    if (lifecycle === "evicted") throw new Error("Eviction unavailable");
    return transition(current, lifecycle);
  } });
  await h.open(); await h.clock.advance(15_000);
  assert.equal(h.control.phase, "error");
  assert.match(h.control.state.message, /Login expired; sign in again.*retry/);
  assert.equal(h.container.childNodes.length, 0);
  assert.equal(h.errors.length, 2);
  assert.equal(h.clock.pending, 0);
  await h.control.close();
});

test("malformed renewal stops the owned session without remounting or dispatching", async () => {
  const h = harness({ heartbeat: (id, signal, count) => lease(count === 1 ? id : "foreign-session") });
  await h.open(); await h.clock.advance(15_000);
  assert.equal(h.control.phase, "error");
  assert.match(h.control.state.message, /mismatched lease.*retry/);
  assert.equal(h.mounts.length, 1);
  assert.equal(h.container.childNodes.length, 0);
  assert.equal(h.clock.pending, 0);
  await assert.rejects(h.mounts[0].input.dispatch({ id: "stale" }), /closed or its authority/);
  assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 1);
});

test("normal flow mounts final session plan, applies remote events and uses default mount", async () => {
  const finalPlan = { ...plan(), surface: { ...plan().surface, entry: "https://surface.test/final" } };
  const h = harness({ defaultMount: true, start: () => session("object-a", "session-final", finalPlan) });
  await h.open();
  assert.equal(h.control.phase, "active");
  assert.equal(h.control.state.session.lifecycle, "active");
  assert.equal(h.control.objectId, "object-a");
  assert.equal(h.control.sessionId, "session-final");
  assert.equal(h.mounts[0].input.plan, finalPlan);
  assert.equal(h.mounts[0].input.currentIdentityId, "account-a");
  assert.deepEqual(h.mounts[0].events.map((event) => event.lifecycle), ["warm", "active"]);
  assert.deepEqual(h.states.map((state) => state.phase), ["preparing", "mounting", "active"]);
  const closing = h.control.close("User closed inline UI");
  assert.equal(h.container.childNodes.length, 0);
  assert.equal(h.control.phase, "idle");
  assert.equal(h.control.objectId, null);
  assert.equal(h.control.sessionId, null);
  await closing;
  await h.control.close();
  assert.deepEqual(h.calls.map((call) => call[0]), ["prepare", "start", "heartbeat", "warm", "active", "evicted"]);
  assert.deepEqual(h.mounts[0].evictions, ["User closed inline UI"]);
  assert.equal(h.errors.length, 0);
});

for (const admission of ["blocked", "needs_permission"]) {
  for (const at of ["prepare", "start", "warm", "active"]) {
    test(`${admission} at ${at} never activates and cleans any started session`, async () => {
      const blockedPlan = plan("object-a", admission);
      const h = harness({
        prepare: () => at === "prepare" ? blockedPlan : plan(),
        start: () => session("object-a", "session-a", at === "start" ? blockedPlan : plan()),
        transition: (id, lifecycle, current) => transition(lifecycle === at ? { ...current, plan: blockedPlan } : current, lifecycle),
      });
      await h.open();
      assert.equal(h.control.phase, "blocked");
      assert.equal(h.control.state.plan, blockedPlan);
      if (at === "prepare" || at === "start") assert.equal(h.heartbeats.length, 0);
      assert.match(h.control.state.message, /Permission required/);
      assert.equal(h.container.childNodes.length, 0);
      assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, at === "prepare" ? 0 : 1);
      assert.equal(h.calls.some(([kind]) => kind === "active"), at === "active");
      assert.equal(h.states.some(({ phase }) => phase === "active"), false);
      assert.equal(h.mounts.length, ["warm", "active"].includes(at) ? 1 : 0);
    });
  }
}

for (const stage of stages) {
  for (const disposition of ["resolve", "reject"]) {
    test(`close during ${stage}, late ${disposition}: no resurrection or stale UI`, async () => {
      const h = paused(stage);
      const opening = h.open();
      await settle();
      const closing = h.control.close();
      const stateCount = h.states.length;
      assert.equal(h.container.childNodes.length, 0);
      assert.equal(h.calls.some(([kind]) => kind === "evicted"), false);
      if (disposition === "resolve") h.release();
      else h.gate.reject(new Error("Late server failure"));
      await Promise.all([opening, closing]);
      assert.equal(h.states.length, stateCount);
      assert.equal(h.control.phase, "idle");
      assert.equal(h.errors.length, 0);
      const expected = stage === "prepare" || (stage === "start" && disposition === "reject") ? 0 : 1;
      assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, expected);
      if (expected) assert.equal(h.remote.get("session-object-a").lifecycle, "evicted");
      if (h.mounts.length) assert.equal(h.mounts[0].evictions.length, 1);
    });
  }

  test(`authority loss during ${stage} suppresses stale completion and cleans up`, async () => {
    const h = paused(stage);
    const opening = h.open();
    await settle();
    h.revoke(); h.release();
    await opening;
    await h.control.close();
    assert.equal(h.control.phase, "idle");
    assert.equal(h.container.childNodes.length, 0);
    assert.equal(h.states.some((state) => state.phase === "active"), false);
    assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, stage === "prepare" ? 0 : 1);
  });

  test(`replacement during ${stage} keeps mount and session ownership separate`, async () => {
    const h = paused(stage);
    const first = h.open();
    await settle();
    const other = harness();
    const second = h.control.open({ ...other.input("object-b"), container: h.container });
    await second;
    const lastState = h.control.state;
    h.release(); await first;
    assert.equal(h.control.state, lastState);
    assert.equal(h.control.objectId, "object-b");
    assert.equal(h.control.phase, "active");
    assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, stage === "prepare" ? 0 : 1);
    assert.equal(other.calls.some(([kind]) => kind === "evicted"), false);
    assert.equal(h.container.childNodes.length, 1);
    await h.control.close();
    assert.equal(other.remote.get("session-object-b").lifecycle, "evicted");
    assert.equal(h.container.childNodes.length, 0);
  });
}

for (const stage of ["prepare", "start", "mount", "warm", "active"]) {
  test(`${stage} failure is visible and evicts all owned resources`, async () => {
    const failure = new Error(`${stage} unavailable`);
    const h = harness({
      prepare: () => { if (stage === "prepare") throw failure; return plan(); },
      start: () => { if (stage === "start") throw failure; return session(); },
      mountFailure: stage === "mount" ? failure : undefined,
      transition: (id, lifecycle, current) => { if (stage === lifecycle) throw failure; return transition(current, lifecycle); },
    });
    await h.open();
    assert.equal(h.control.phase, "error");
    assert.equal(h.control.state.message, failure.message);
    assert.equal(h.container.childNodes.length, 0);
    assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, ["prepare", "start"].includes(stage) ? 0 : 1);
    assert.equal(h.errors.length, 0);
  });
}

test("cleanup failure exposes session ID and original cause, without retry or rejected close", async () => {
  const failure = new Error("Captured account token expired; authenticate to reconcile session");
  const h = harness({ transition: (id, lifecycle, current) => {
    if (lifecycle === "evicted") throw failure;
    return transition(current, lifecycle);
  } });
  await h.open();
  await Promise.all([h.control.close(), h.control.close()]);
  assert.equal(h.errors.length, 1);
  assert.equal(h.errors[0].cause, failure);
  assert.match(h.errors[0].message, /evict remote Surface session.*session-object-a.*Captured account token expired/);
  assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 1);
  assert.equal(h.container.childNodes.length, 0);
});

test("local eviction failure still removes frame and attempts remote eviction", async () => {
  const failure = new Error("Lifecycle listener failed");
  const h = harness({ evictFailure: failure });
  await h.open(); await h.control.close();
  assert.equal(h.errors[0].cause, failure);
  assert.equal(h.mounts[0].unmounts, 1);
  assert.equal(h.container.childNodes.length, 0);
  assert.equal(h.remote.get("session-object-a").lifecycle, "evicted");
});

for (const action of ["close", "replace", "revoke"]) {
  for (const result of ["resolve", "reject"]) {
    test(`bridge ${result} after ${action} cannot return stale output or dispatch again`, async () => {
      const gate = deferred();
      const h = harness({ dispatch: () => gate.promise });
      await h.open();
      const dispatch = h.mounts[0].input.dispatch;
      const pending = dispatch({ id: "rpc-a" });
      const rejected = assert.rejects(pending, /closed or its authority/);
      if (action === "revoke") h.revoke();
      else if (action === "close") await h.control.close();
      else await h.open("object-b");
      if (result === "resolve") gate.resolve({ result: "private old-account output" });
      else gate.reject(new Error("private old-account failure"));
      await rejected;
      await assert.rejects(dispatch({ id: "rpc-b" }), /closed or its authority/);
      assert.equal(h.dispatchCalls.length, 1);
      await h.control.close();
    });
  }
}

test("reentrant state hooks can close during prepare or mounting without issuing the next stage", async () => {
  for (const phase of ["preparing", "mounting"]) {
    const h = harness({ onState: (state) => { if (state.phase === phase) void h.control.close(); } });
    await h.open(); await h.control.close();
    assert.equal(h.control.phase, "idle");
    assert.equal(h.mounts.length, 0);
    assert.equal(h.calls.filter(([kind]) => kind === "start").length, phase === "preparing" ? 0 : 1);
  }
});

test("close reentered during synchronous mount evicts the returned handle exactly once", async () => {
  const h = harness({ afterMount: () => { void h.control.close(); } });
  await h.open(); await h.control.close();
  assert.equal(h.mounts[0].evictions.length, 1);
  assert.equal(h.container.childNodes.length, 0);
  assert.deepEqual(h.calls.map(([kind]) => kind), ["prepare", "start", "heartbeat", "evicted"]);
});

test("mismatched transition responses cannot apply events to the owned frame", async () => {
  const h = harness({ transition: (id, lifecycle, current) => lifecycle === "warm"
    ? transition(session("object-b"), lifecycle) : transition(current, lifecycle) });
  await h.open();
  assert.equal(h.control.phase, "error");
  assert.match(h.control.state.message, /mismatched session/);
  assert.equal(h.mounts[0].events.length, 0);
  assert.equal(h.remote.get("session-object-a").lifecycle, "evicted");
});

test("wrong object plans are rejected at prepare and start", async () => {
  for (const stage of ["prepare", "start"]) {
    const h = harness({ prepare: () => plan(stage === "prepare" ? "object-b" : "object-a"),
      start: () => session("object-a", "session-object-a", plan("object-b")) });
    await h.open();
    assert.equal(h.control.phase, "error");
    assert.equal(h.mounts.length, 0);
    assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, stage === "start" ? 1 : 0);
  }
});

test("closing again waits for previous remote cleanup, while replacement can proceed", async () => {
  const gate = deferred();
  const h = harness({ transition: (id, lifecycle, current) => id === "session-object-a" && lifecycle === "evicted"
    ? gate.promise : transition(current, lifecycle) });
  await h.open();
  const firstClose = h.control.close();
  await h.open("object-b");
  assert.equal(h.control.phase, "active");
  let drained = false;
  const secondClose = h.control.close().then(() => { drained = true; });
  await settle();
  assert.equal(drained, false);
  gate.resolve(transition(session(), "evicted"));
  await Promise.all([firstClose, secondClose]);
  assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 2);
});

test("bridge forwards the SDK cancellation context unchanged", async () => {
  const contexts = [];
  const h = harness({ dispatch: (request, context) => { contexts.push(context); return { id: request.id }; } });
  await h.open();
  const context = { signal: new AbortController().signal };
  await h.mounts[0].input.dispatch({ id: "rpc-cancel" }, context);
  assert.equal(contexts[0], context);
  await h.control.close();
});

test("cleanup error hooks may open a replacement without being overwritten by older state", async () => {
  for (const action of ["close", "replace"]) {
    let recovered = false, recovery;
    const h = harness({ evictFailure: new Error("Local teardown failed"), onCleanupError: () => {
      if (!recovered) { recovered = true; recovery = h.open("object-recovered"); }
    } });
    await h.open();
    if (action === "close") await h.control.close();
    else await h.open("object-superseded");
    await recovery;
    assert.equal(h.control.objectId, "object-recovered");
    assert.equal(h.control.state.objectId, "object-recovered");
    assert.equal(h.control.phase, "active");
    assert.equal(h.calls.some(([kind, object]) => kind === "start" && object === "object-superseded"), false);
    await h.control.close();
  }
});

// Exercise current SDK source without building or modifying the SDK worker's files.
const sdkModules = new Map();
function sdkModule(url) {
  if (sdkModules.has(url.href)) return sdkModules.get(url.href);
  const exports = {};
  sdkModules.set(url.href, exports);
  vm.runInNewContext(ts.transpileModule(readFileSync(url, "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, { exports, require: (name) => sdkModule(new URL(name.replace(/\.js$/, ".ts"), url)),
    URL, Error, AbortController, crypto: globalThis.crypto, performance, setTimeout, clearTimeout, TextEncoder });
  return exports;
}

function sdkHostHarness(options = {}) {
  const { BrowserSurfaceHost } = sdkModule(new URL("../../sdk/src/host.ts", import.meta.url));
  const listeners = new Set(), frames = [], mounted = [], messages = [], channels = [];
  const hostWindow = {
    location: { origin: "https://host.test" },
    addEventListener: (type, handler) => listeners.add(handler),
    removeEventListener: (type, handler) => listeners.delete(handler),
  };
  const realPlan = options.plan ?? { ...plan(), surface: { ...plan().surface, target: options.target ?? "Web" },
    sandbox: { isolated_origin: true, capability_bridge: true,
      csp: "default-src 'none'; script-src 'self'", host_cookies: false, top_navigation: false } };
  const h = harness({ ...options, prepare: () => options.prepared ?? realPlan,
    start: () => session("object-a", "session-object-a", realPlan),
    mount: (input) => {
      const surface = new BrowserSurfaceHost().mount({ ...input, window: hostWindow,
        handshakeTimeoutMs: options.handshakeTimeoutMs,
        document: { createElement: () => {
          const frame = {
            sandbox: { add() {} }, attributes: {}, contentWindow: { postMessage: (value) => messages.push(value) },
            events: new Map(),
            addEventListener(name, handler) { this.events.set(name, handler); },
            removeEventListener(name) { this.events.delete(name); },
            setAttribute(name, value) { this.attributes[name] = value; },
            remove() { input.container.removeChild(this); this.contentWindow = null; },
          };
          frames.push(frame);
          return frame;
        } },
      });
      mounted.push(surface);
      return surface;
    },
  });
  const windowMessage = (data, ports = []) => {
    const transferred = structuredClone(ports, { transfer: ports });
    for (const handler of listeners) handler({ source: frames.at(-1).contentWindow, origin: options.origin ?? "null", data, ports: transferred });
  };
  const connect = async (confirm = true) => {
    const channel = new MessageChannel(); channels.push(channel);
    const received = [], waiters = new Map();
    const next = (type) => new Promise((resolve) => waiters.set(type, resolve));
    channel.port1.on("message", (message) => {
      received.push(message);
      waiters.get(message.type)?.(message);
      waiters.delete(message.type);
    });
    const accepted = next("babel.surface.accept");
    windowMessage({ type: "babel.surface.connect", protocol: "babel.rpc.v1", version: 1 }, [channel.port2]);
    await accepted;
    const child = { port: channel.port1, received, next, async confirm() {
      const ready = next("babel.surface.ready");
      channel.port1.postMessage({ type: "babel.surface.confirm", protocol: "babel.rpc.v1", version: 1 });
      await ready;
    } };
    if (confirm) await child.confirm();
    return child;
  };
  return { ...h, realPlan, listeners, frames, mounted, messages, windowMessage, connect,
    async dispose() {
      const closing = h.control.close();
      for (const channel of channels) { channel.port1.close(); channel.port2.close(); }
      await closing;
    },
  };
}

test("server document acknowledgement gates the real SDK bridge and frontend activation", { timeout: 5000 }, async (t) => {
  const gate = deferred(), entered = deferred();
  let observed;
  const h = sdkHostHarness({ registerDocument: (id, documentId, signal) => {
    observed = { id, documentId, signal }; entered.resolve(); return gate.promise;
  } });
  t.after(() => h.dispose());
  const opening = h.open(); await settle();
  const child = await h.connect(false);
  child.port.postMessage({ type: "babel.surface.confirm", protocol: "babel.rpc.v1", version: 1 });
  await entered.promise;
  assert.equal(h.control.phase, "mounting");
  assert.equal(observed.id, "session-object-a");
  assert.match(observed.documentId, /^[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}$/);
  assert.equal(observed.signal.aborted, false);
  assert.equal(child.received.some(message => message.type === "babel.surface.ready"), false);
  assert.equal(h.calls.some(([kind]) => kind === "warm" || kind === "active"), false);
  gate.resolve(); await opening;
  assert.equal(h.control.phase, "active", h.control.state.message);
  assert.equal(h.calls.filter(([kind]) => kind === "document").length, 1);
  assert.deepEqual(h.calls.filter(([kind]) => ["document", "warm", "active"].includes(kind)).map(([kind]) => kind),
    ["document", "warm", "active"]);
});

for (const ending of ["close", "authority", "lease", "reject"]) {
  test(`pending registration is cancelled or fails visibly on ${ending}, without late activation`, { timeout: 5000 }, async (t) => {
    const gate = deferred(), entered = deferred();
    let signal;
    const h = sdkHostHarness({ registerDocument: (_id, _documentId, registrationSignal) => {
      signal = registrationSignal; entered.resolve(); return gate.promise;
    } });
    t.after(() => h.dispose());
    const opening = h.open(); await settle();
    const child = await h.connect(false);
    child.port.postMessage({ type: "babel.surface.confirm", protocol: "babel.rpc.v1", version: 1 });
    await entered.promise;
    if (ending === "close") await h.control.close();
    if (ending === "authority") { h.revoke(); h.control.checkLease(); }
    if (ending === "lease") { h.clock.jump(60_001); h.control.checkLease(); }
    if (ending === "reject") gate.reject(new Error("Surface document registration failed (409)"));
    else gate.resolve();
    await opening; await settle();
    assert.equal(signal.aborted, true);
    assert.equal(h.container.childNodes.length, 0);
    assert.equal(h.calls.some(([kind]) => kind === "warm" || kind === "active"), false);
    assert.equal(child.received.some(message => message.type === "babel.surface.ready"), false);
    if (ending === "reject") {
      assert.equal(h.control.phase, "error");
      assert.match(h.control.state.message, /registration failed/);
    }
    assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 1);
  });
}

for (const mismatched of [false, true]) {
  test(`frontend controller uses the session's verified bundle mount and rejects mismatched assignment: ${mismatched}`, { timeout: 5000 }, async (t) => {
    const origin = "http://m-0123456789abcdef.localhost:8788", hash = "b".repeat(64);
    const prepared = { ...plan(), surface: { role: "Feed", target: "Web", entry: `babel://blobs/${hash}`, integrity: hash,
      bundle: { version: 1, entry_path: "app/index.html", files: [{ path: "app/index.html", source_uri: `babel://blobs/${hash}`,
        integrity: hash, media_type: "text/html", kind: "document", size_bytes: 24 }] } },
      bundle_verification: { policy_version: 1, manifest_hash: "a".repeat(64) },
      sandbox: { isolated_origin: true, capability_bridge: true, host_cookies: false, top_navigation: false,
        iframe_sandbox: "allow-scripts allow-same-origin", csp: "default-src 'none'" },
    };
    const admitted = { ...prepared, verified_mount: { version: 1, object_id: "object-a", role: "Feed",
      session_id: mismatched ? "wrong-session" : "session-object-a", manifest_hash: "a".repeat(64), origin,
      entry_url: `${origin}/app/index.html` } };
    const h = sdkHostHarness({ plan: admitted, prepared, origin });
    t.after(() => h.dispose());
    const opening = h.open(); await settle();
    if (mismatched) {
      await opening;
      assert.equal(h.control.phase, "error");
      assert.match(h.control.state.message, /session binding mismatch/);
      assert.equal(h.frames.length, 0);
    } else {
      assert.equal(h.control.phase, "mounting");
      assert.equal(h.frames[0].src, admitted.verified_mount.entry_url);
      await h.connect(); await opening;
      assert.equal(h.control.phase, "active", h.control.state.message);
      assert.equal(h.mounted[0].surfaceOrigin, origin);
      assert.equal(h.heartbeats.length, 1);
      await h.control.close();
    }
    assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 1);
    assert.equal(h.container.childNodes.length, 0);
  });
}

test("real SDK legacy-window Surface times out with actionable error and never activates", { timeout: 5000 }, async (t) => {
  const h = sdkHostHarness({ handshakeTimeoutMs: 20 });
  t.after(() => h.dispose());
  const opening = h.open(); await settle();
  assert.equal(h.control.phase, "mounting");
  h.windowMessage({ type: "babel.rpc.request", protocol: "babel.rpc.v1", envelope: { id: "legacy-window" } });
  await opening;
  assert.equal(h.control.phase, "error");
  assert.match(h.control.state.message, /could not establish a secure bridge.*handshake timed out.*Reopen the Surface or update/);
  assert.equal(h.states.some(({ phase }) => phase === "active"), false);
  assert.equal(h.dispatchCalls.length, 0);
  assert.equal(h.container.childNodes.length, 0);
  assert.equal(h.listeners.size, 0);
  assert.equal(h.clock.pending, 0);
  assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 1);
});

test("real SDK Static Surface becomes ready without a channel", async (t) => {
  const h = sdkHostHarness({ target: "Static" });
  t.after(() => h.dispose());
  await h.open();
  assert.equal(h.control.phase, "active", h.control.state.message);
  assert.equal(h.listeners.size, 0);
  assert.equal(h.messages.length, 0);
  assert.deepEqual(h.calls.map(([kind]) => kind), ["prepare", "start", "heartbeat", "warm", "active"]);
});

for (const action of ["close", "account", "lease", "replace"]) {
  test(`real SDK ${action} before channel confirmation releases readiness and ignores late confirm`, { timeout: 5000 }, async (t) => {
    const h = sdkHostHarness();
    t.after(() => h.dispose());
    const opening = h.open(); await settle();
    const child = await h.connect(false);
    assert.equal(h.control.phase, "mounting");
    const rejected = assert.rejects(h.mounted[0].ready, /torn down/);
    if (action === "close") await h.control.close();
    else if (action === "replace") {
      const nextPlan = { ...h.realPlan, object_id: "object-b" };
      const other = harness({ prepare: () => nextPlan, start: () => session("object-b", "session-object-b", nextPlan) });
      const replacement = h.control.open({ ...other.input("object-b"), container: h.container });
      await settle();
      assert.equal(h.control.phase, "mounting", h.control.state.message);
      await h.connect(); await replacement;
    } else {
      if (action === "account") h.revoke();
      else h.clock.jump(60_000);
      assert.equal(h.control.checkLease(), false);
    }
    await opening; await rejected;
    const state = h.control.state;
    child.port.postMessage({ type: "babel.surface.confirm", protocol: "babel.rpc.v1", version: 1 });
    await settle();
    assert.equal(h.control.state, state);
    assert.equal(h.control.phase, action === "replace" ? "active" : action === "lease" ? "error" : "idle");
    assert.equal(h.container.childNodes.length, action === "replace" ? 1 : 0);
    assert.equal(h.calls.some(([kind]) => kind === "warm" || kind === "active"), false);
    assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 1);
  });
}

test("real SDK connector completes document handshake and closes controller on pagehide", { timeout: 5000 }, async (t) => {
  const { connectSurfaceBridge } = sdkModule(new URL("../../sdk/src/channel.ts", import.meta.url));
  const h = sdkHostHarness();
  t.after(() => h.dispose());
  const opening = h.open(); await settle();
  const pagehide = new Set();
  const transport = await connectSurfaceBridge({ parentOrigin: "https://host.test", createChannel: () => new MessageChannel(),
    window: {
      parent: { postMessage(data, origin, ports) {
        assert.equal(origin, "https://host.test");
        h.windowMessage(data, ports);
      } },
      addEventListener(type, listener) { assert.equal(type, "pagehide"); pagehide.add(listener); },
      removeEventListener(type, listener) { pagehide.delete(listener); },
    },
  });
  t.after(() => transport.close());
  await opening;
  assert.equal(h.control.phase, "active", h.control.state.message);
  for (const listener of pagehide) listener();
  // The close control travels asynchronously over the native channel.
  await new Promise((resolve) => h.mounted[0].lifecycle.onChange(({ current }) => {
    if (current === "evicted") resolve();
  }));
  await h.control.close();
  assert.equal(h.control.phase, "idle");
  assert.equal(h.container.childNodes.length, 0);
  assert.equal(pagehide.size, 0);
  assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 1);
});

test("production heartbeat client uses real SDK transport, host binding, empty payload and cancellation", async () => {
  const { mediaResource } = await import("./media-modules.mjs");
  const sdk = await import("@babel-protocol/sdk");
  const invocationContext = { exports: {}, URL, AbortController, AbortSignal, TextEncoder, TextDecoder, structuredClone, console,
    crypto: globalThis.crypto, require: name => { assert.equal(name, "@babel-protocol/sdk"); return sdk; } };
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL("../src/app/invocations.ts", import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, invocationContext);
  const browserContext = { ...invocationContext, exports: {} };
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL("../src/app/browser-invocations.ts", import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, browserContext);
  const protocolContext = { exports: {}, URL, AbortController, AbortSignal, crypto: globalThis.crypto, Error, require: (name) => {
    if (name === "./invocations") return invocationContext.exports;
    if (name === "./browser-invocations") return browserContext.exports;
    if (name === "./media-resource") return mediaResource;
    if (name === "@babel-protocol/sdk") return sdkModule(new URL("../../sdk/src/transport.ts", import.meta.url));
    assert.equal(name, "./profile-response");
    return sdkModule(new URL("../src/app/profile-response.ts", import.meta.url));
  } };
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL("../src/app/protocol.ts", import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, protocolContext);
  const calls = [];
  let pending = false;
  const client = new protocolContext.exports.BabelFrontendClient("https://babel.test", async (url, init) => {
    calls.push({ url, init, envelope: JSON.parse(init.body) });
    if (pending) return new Promise((resolve, reject) => init.signal.addEventListener("abort", () => reject(init.signal.reason), { once: true }));
    return Response.json({ protocol: "babel.rpc.v1", id: calls.at(-1).envelope.id, result: { lease: lease() }, error: null });
  });
  const controller = new AbortController();
  assert.deepEqual(await client.heartbeatSurfaceSession("session-object-a", controller.signal), lease());
  const call = calls[0];
  assert.equal(call.url.href, "https://babel.test/rpc");
  assert.equal(call.init.signal, controller.signal);
  assert.equal(call.envelope.method, "babel.runtime.surface.session.heartbeat.v1");
  assert.deepEqual(call.envelope.payload, {});
  assert.deepEqual(call.envelope.binding, { object_id: null, surface_session_id: "session-object-a",
    identity_id: null, runtime_id: "babel-web-runtime", origin: "browser://babel", capability_grants: [] });
  pending = true;
  const request = client.heartbeatSurfaceSession("session-object-a", controller.signal);
  const rejected = assert.rejects(request, /cancelled by host/);
  controller.abort(new Error("cancelled by host"));
  await rejected;
});

for (const outcome of ["resolve", "reject", "navigation", "suspended", "lease expired", "heartbeat rejected"]) {
  test(`real SDK host disposes and suppresses late bridge output: ${outcome}`, { timeout: 5000 }, async (t) => {
    const gate = deferred(), dispatched = deferred(), contexts = [];
    const h = sdkHostHarness({
      heartbeat: (id, signal, count) => {
        if (outcome === "heartbeat rejected" && count > 1) throw new Error("Host login expired");
        return lease(id);
      },
      dispatch: (request, context) => { contexts.push(context); dispatched.resolve(); return gate.promise; },
    });
    t.after(() => h.dispose());
    const opening = h.open(); await settle();
    assert.equal(h.control.phase, "mounting");
    const child = await h.connect(false);
    assert.equal(h.control.phase, "mounting");
    assert.equal(h.calls.some(([kind]) => kind === "warm" || kind === "active"), false);
    await child.confirm(); await opening;
    assert.equal(h.control.phase, "active", h.control.state.message);
    assert.equal(h.frames.length, 1);
    const envelope = {
      protocol: "babel.rpc.v1", id: "bridge-pending", method: "babel.search.objects.v1",
      binding: { object_id: "object-a", surface_session_id: "session-object-a", identity_id: "account-a",
        runtime_id: "runtime", origin: "https://surface.test", capability_grants: [] },
      payload: { q: "surface", author: null, kind: null, limit: 3 }, idempotency_key: null, trace_id: null,
      deadline: { timeout_ms: 30000, client_started_at: "2026-09-30T00:00:00Z" },
    };
    h.windowMessage({ type: "babel.rpc.request", protocol: "babel.rpc.v1", envelope });
    await settle();
    assert.equal(h.dispatchCalls.length, 0, "raw window RPC must remain unsupported");
    child.port.postMessage({ type: "babel.rpc.request", protocol: "babel.rpc.v1", envelope });
    await dispatched.promise;
    assert.equal(h.dispatchCalls.length, 1);
    if (outcome === "navigation") {
      h.frames[0].events.get("load")();
      h.frames[0].events.get("load")();
    } else if (outcome === "suspended") h.mounted[0].suspend("Memory pressure");
    else if (outcome === "lease expired") { h.clock.jump(60_000); assert.equal(h.control.checkLease(), false); }
    else if (outcome === "heartbeat rejected") await h.clock.advance(15_000);
    else void h.control.close();
    assert.equal(h.control.phase, ["lease expired", "heartbeat rejected"].includes(outcome) ? "error" : "idle");
    await h.control.close();
    assert.equal(h.container.childNodes.length, 0);
    assert.equal(h.listeners.size, 0);
    if (outcome !== "reject") gate.resolve({ protocol: "babel.rpc.v1", id: envelope.id, result: { secret: "old account" }, error: null });
    else gate.reject(new Error("Old account transport failed"));
    await settle();
    assert.equal(h.messages.length, 0);
    assert.equal(child.received.some(({ type }) => type === "babel.rpc.response"), false);
    assert.equal(contexts[0]?.signal.aborted, true);
    assert.equal(h.calls.filter(([kind]) => kind === "evicted").length, 1);
  });
}
