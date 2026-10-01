import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import { once } from "node:events";
import { safety, module, id, owner, target, other, state, identity, snapshot, plain, deferred, controllerHarness, ready, finish, settle } from "./safety-helpers.mjs";
const { SafetyClient, SafetyError, parseSafetyState, parseSafetySnapshot } = safety;

test("strict pair parser rejects cross-owner, self, invalid booleans, active revision zero and unsafe revisions", () => {
  for (const value of [null, [], {}, state(1), state(false, "false"), state(true), state(false, false, -1), state(false, false, 1.5),
    state(false, false, Number.MAX_SAFE_INTEGER + 1), state(false, false, 0, other), state(false, false, 0, owner, other)]) {
    assert.throws(() => parseSafetyState(value, owner, target), /invalid safety/);
  }
  assert.throws(() => parseSafetyState(state(false, false, 0, owner, owner), owner, owner));
  assert.equal(parseSafetyState(state(), owner, target).revision, 0);
});

test("snapshot parser enforces authoritative shape, active-only, sorted unique identities and 1000-entry bound", () => {
  const good = snapshot([state(true, true, 1)]);
  for (const value of [null, { ...good, author_id: other }, { ...good, revision: -1 }, { ...good, entries: Array(1001).fill(good.entries[0]) },
    { ...good, entries: [good.entries[0], good.entries[0]] }, snapshot([state(true, false, 2)], 1),
    { ...good, entries: [{ identity: identity(), state: state() }] },
    { ...good, entries: [{ identity: identity(other), state: state(true, false, 1) }] },
    { ...good, entries: [{ identity: { ...identity(), handle: "x".repeat(1025) }, state: state(true, false, 1) }] },
    { ...good, entries: [{ identity: { ...identity(), public_key: {} }, state: state(true, false, 1) }] }]) {
    assert.throws(() => parseSafetySnapshot(value, owner), /invalid safety/);
  }
  const states = Array.from({ length: 1000 }, (_, i) => state(false, true, 1, owner, `id_${i.toString(16).padStart(64, "0")}`));
  const large = snapshot(states, 1000); assert.equal(parseSafetySnapshot(large, owner).entries.length, 1000);
  assert.throws(() => parseSafetySnapshot({ ...large, entries: [...large.entries].reverse() }, owner));
  const parsed = parseSafetySnapshot(good, owner);
  assert.ok(Object.isFrozen(parsed.entries[0].identity.public_key));
  good.entries[0].identity.handle = "Mutated"; assert.equal(parsed.entries[0].identity.handle, "Author");
});

test("client uses authenticated host REST and bounds request payloads before transport", async () => {
  const calls = [], values = [state(), state(true, false, 1), snapshot([state(true, false, 1)])];
  const client = new SafetyClient("https://babble.test/base", async (url, init) => { calls.push({ url, init }); return Response.json(values.shift()); });
  const signal = new AbortController().signal;
  await client.state(owner, target, signal);
  const intent = { blocked: true, muted: false, expected_revision: 0, idempotency_key: "same-key" };
  await client.update(owner, target, intent, signal); await client.snapshot(owner, signal);
  assert.deepEqual(calls.map(c => c.url.pathname), [`/social/safety/${target}`, `/social/safety/${target}`, "/social/safety"]);
  assert.deepEqual(JSON.parse(calls[1].init.body), intent);
  for (const { init } of calls) { assert.equal(init.cache, "no-store"); assert.equal(init.redirect, "error"); assert.equal(init.credentials, "omit"); }
  for (const bad of [{ ...intent, expected_revision: -1 }, { ...intent, idempotency_key: "x".repeat(129) }, { ...intent, muted: 1 }]) {
    await assert.rejects(client.update(owner, target, bad, signal));
  }
  await assert.rejects(client.state(owner, "../auth", signal)); assert.equal(calls.length, 3);
});

test("HTTP errors retain status without exposing server diagnostics; streamed size, invalid JSON and UTF-8 fail", async () => {
  const signal = new AbortController().signal;
  for (const status of [401, 403, 404, 409, 422, 429, 503]) for (const thrown of [true, false]) {
    const client = new SafetyClient("https://babble.test", async () => {
      if (thrown) throw Object.assign(new Error("secret diagnostic"), { status });
      return new Response("secret diagnostic", { status });
    });
    await assert.rejects(client.state(owner, target, signal), e => e.status === status && !e.message.includes("secret"));
  }
  for (const body of ["x".repeat(4097), "{broken", new Uint8Array([255])]) {
    const client = new SafetyClient("https://babble.test", async () => new Response(body));
    await assert.rejects(client.state(owner, target, signal), /Could not load/);
  }
  const client = new SafetyClient("https://babble.test", async () => new Response("x".repeat(4 * 1024 * 1024 + 1)));
  await assert.rejects(client.snapshot(owner, signal));
});

test("abort is checked after streamed JSON even when a fetch adapter ignores cancellation", async () => {
  const controller = new AbortController(); let stream;
  const client = new SafetyClient("https://babble.test", async () => new Response(new ReadableStream({ start(c) { stream = c; } })));
  const request = client.state(owner, target, controller.signal); await settle();
  controller.abort(); stream.enqueue(new TextEncoder().encode(JSON.stringify(state()))); stream.close();
  await assert.rejects(request, e => e.name === "AbortError");
});

test("identity labels use the authenticated identity route and reject another author's identity", async () => {
  const calls = []; let person = identity();
  const client = new SafetyClient("https://babble.test", async (url, init) => { calls.push({ url, init }); return Response.json({ identity: person }); });
  const signal = new AbortController().signal;
  assert.equal((await client.identity(target, signal)).handle, "Author");
  assert.equal(calls[0].url.pathname, `/identities/${target}`); assert.equal(calls[0].init.cache, "no-store");
  person = identity(other); await assert.rejects(client.identity(target, signal), /invalid safety/);
});

test("real Accounts transport supplies bearer credentials over host HTTP; private routes are never guest requests", async t => {
  const calls = [];
  const server = createServer((req, res) => {
    let body = ""; req.on("data", chunk => { body += chunk; });
    req.on("end", () => {
      calls.push({ path: req.url, auth: req.headers.authorization, method: req.method, body });
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify(req.url === "/auth/login" ? { identity: { id: owner, handle: "Owner" }, token: "host-test-token".repeat(3), expires_at: "2099-01-01T00:00:00Z" }
        : req.url === "/social/safety" ? snapshot() : req.method === "PUT" ? state(true, false, 1) : state()));
    });
  });
  server.listen(0, "127.0.0.1"); await once(server, "listening"); t.after(() => new Promise(resolve => { server.closeAllConnections(); server.close(resolve); }));
  const url = `http://127.0.0.1:${server.address().port}`;
  const { Accounts } = module("accounts", {}, { fetch });
  const accounts = new Accounts(url, null), client = new SafetyClient(url, accounts.authenticatedFetch);
  const signal = new AbortController().signal;
  await assert.rejects(client.snapshot(owner, signal), e => e.status === 401); assert.equal(calls.length, 0);
  await accounts.login(owner, "local-test-password");
  await client.state(owner, target, signal); await client.snapshot(owner, signal);
  await client.update(owner, target, { blocked: true, muted: false, expected_revision: 0, idempotency_key: "host-key" }, signal);
  assert.equal(calls.length, 4);
  assert.ok(calls.slice(1).every(call => call.auth === `Bearer ${"host-test-token".repeat(3)}`));
  assert.deepEqual(JSON.parse(calls.at(-1).body), { blocked: true, muted: false, expected_revision: 0, idempotency_key: "host-key" });
});

test("ensure deduplicates; loaded snapshots use sets and explicit refresh updates current revision only", async () => {
  const h = controllerHarness(); const first = h.controller.ensure(), second = h.controller.ensure(); assert.equal(first, second);
  h.snapshots[0].resolve(snapshot([state(false, true, 1)])); await first;
  assert.equal(h.controller.hidden(target), true); assert.equal(h.controller.blocked(target), false);
  await h.controller.ensure(); assert.equal(h.snapshots.length, 1);
  const refresh = h.controller.refresh(); h.snapshots[1].resolve(snapshot([state(true, true, 2)])); await refresh;
  assert.equal(h.controller.blocked(target), true); assert.equal(h.changes.length, 2);
});

test("refresh failures invalidate loaded snapshots and reject; late superseded snapshots cannot commit", async () => {
  const h = controllerHarness(); await ready(h, state(true, false, 3));
  const stale = h.controller.refresh(), current = h.controller.refresh(); assert.equal(h.snapshots.at(-2).signal.aborted, true);
  h.snapshots.at(-1).resolve(snapshot([], 4)); await current;
  h.snapshots.at(-2).resolve(snapshot([state(true, false, 3)])); await stale; assert.equal(h.controller.snapshot.revision, 4);
  const failed = h.controller.refresh(); h.snapshots.at(-1).reject(new Error("offline")); await assert.rejects(failed);
  assert.equal(h.controller.snapshot, null); assert.equal(h.changes.at(-1), null);
});

test("account invalidation synchronously clears private snapshot and aborts all late reads", async () => {
  const h = controllerHarness(); await ready(h, state(true, true, 1));
  const loading = h.controller.refresh(); const reading = h.controller.read();
  h.controller.account(other);
  assert.equal(h.controller.snapshot, null); assert.equal(h.controller.hidden(target), false); assert.equal(h.controller.view.target, null);
  assert.equal(h.snapshots.at(-1).signal.aborted, true); assert.equal(h.reads.at(-1).signal.aborted, true);
  h.snapshots.at(-1).resolve(snapshot([state(true, true, 2)])); h.reads.at(-1).resolve(state(true, true, 2));
  await loading; await reading;
  assert.equal(h.controller.snapshot, null); assert.equal(h.controller.view.state, null);
});

test("guest and self never fetch pair state or submit changes", async () => {
  const h = controllerHarness(); h.controller.account(null); await h.controller.show(target); await h.controller.change("blocked", true);
  assert.equal(await h.controller.ensure(), null); h.controller.account(owner); await h.controller.show(owner); await h.controller.retry();
  assert.equal(h.reads.length + h.writes.length + h.snapshots.length, 0);
});

test("successful PUT is not optimistic; replay receipt never overrides current pair or snapshot", async () => {
  const h = controllerHarness(); await ready(h);
  const operation = h.controller.change("blocked", true);
  await h.controller.change("muted", true); assert.equal(h.writes.length, 1);
  assert.equal(h.controller.view.state.blocked, false); assert.equal(h.controller.hidden(target), false);
  await finish(h, operation, state(false, true, 3), { receipt: state(true, false, 1) });
  assert.equal(h.controller.view.state.blocked, false); assert.equal(h.controller.view.state.muted, true);
  assert.equal(h.controller.hidden(target), true); assert.equal(h.controller.blocked(target), false);
  assert.equal(h.controller.view.retry, null); assert.equal(h.changes.length, 2);
});

test("uncertain retries preserve exact key, both flags and CAS revision across readback and navigation", async () => {
  for (const error of [new Error("lost ack"), new SafetyError(503), new SafetyError(408), new SafetyError(429)]) {
    const h = controllerHarness(); await ready(h);
    const operation = h.controller.change("blocked", true); const request = plain(h.writes[0].intent);
    await finish(h, operation, state(true, false, 1), { error });
    await h.controller.show(null); const back = h.controller.show(target); h.reads.at(-1).resolve(state(false, true, 2)); await back;
    const retry = h.controller.retry(); assert.deepEqual(plain(h.writes.at(-1).intent), request);
    await finish(h, retry, state(false, true, 2), { receipt: state(true, false, 1) });
    assert.equal(h.controller.view.state.revision, 2); assert.equal(h.controller.view.retry, null);
  }
});

test("changed intent gets fresh key/revision and preserves independent block/mute state", async () => {
  const h = controllerHarness(); await ready(h);
  const block = h.controller.change("blocked", true); await finish(h, block, state(true, false, 1), { error: new Error("lost ack") });
  await h.controller.change("muted", true); assert.equal(h.writes.length, 1);
  const retry = h.controller.retry(); await finish(h, retry, state(true, false, 1));
  const mute = h.controller.change("muted", true);
  assert.deepEqual(plain(h.writes.at(-1).intent), { blocked: true, muted: true, expected_revision: 1, idempotency_key: "key-2" });
  await finish(h, mute, state(true, true, 2));
  const unblock = h.controller.change("blocked", false);
  assert.equal(h.writes.at(-1).intent.muted, true); await finish(h, unblock, state(false, true, 3));
  assert.equal(h.controller.hidden(target), true); assert.equal(h.controller.blocked(target), false);
});

test("CAS conflict restores actual state and requires new explicit intent; errors never leak diagnostics", async () => {
  const h = controllerHarness(); await ready(h);
  const operation = h.controller.change("muted", true); await finish(h, operation, state(true, false, 4), { error: new SafetyError(409) });
  assert.match(h.controller.view.message, /changed elsewhere/); assert.equal(h.controller.view.retry, null);
  assert.equal(h.writes.length, 1);
  const next = h.controller.change("muted", true); assert.equal(h.writes.at(-1).intent.expected_revision, 4);
  assert.notEqual(h.writes.at(-1).intent.idempotency_key, h.writes[0].intent.idempotency_key);
  await finish(h, next, state(true, true, 5));
});

test("write and readback failures never present success; snapshot refresh still runs", async () => {
  const h = controllerHarness(); await ready(h);
  const operation = h.controller.change("blocked", true); h.writes[0].resolve(state(true, false, 1)); await settle();
  h.reads.at(-1).reject(new Error("offline")); await settle(); h.snapshots.at(-1).reject(new Error("offline")); await operation;
  assert.equal(h.controller.view.state, null); assert.equal(h.controller.snapshot, null);
  assert.match(h.controller.view.message, /Could not confirm/); assert.equal(h.controller.view.pending, false);
});

test("account switch aborts pending writes and suppresses readback, callbacks, retry receipts", async () => {
  const h = controllerHarness(); await ready(h);
  const operation = h.controller.change("blocked", true); h.controller.account(other); const callbacks = h.changes.length;
  assert.equal(h.writes[0].signal.aborted, true); h.writes[0].resolve(state(true, false, 1)); await operation;
  assert.equal(h.reads.length, 1); assert.equal(h.changes.length, callbacks); assert.equal(h.controller.view.retry, null);
});

test("closing during an explicit write suppresses old dialog state but still refreshes account filters", async () => {
  const h = controllerHarness(); await ready(h);
  const operation = h.controller.change("blocked", true); await h.controller.show(null);
  await finish(h, operation, state(true, false, 1));
  assert.equal(h.controller.view.target, null); assert.equal(h.controller.view.state, null); assert.equal(h.controller.blocked(target), true);
});

test("reopening during an acknowledged write clears the old retry receipt and reads the selected author", async () => {
  const h = controllerHarness(); await ready(h);
  const operation = h.controller.change("blocked", true); await h.controller.show(null); await h.controller.show(target);
  h.writes.at(-1).resolve(state(true, false, 1)); await settle(); h.reads.at(-1).resolve(state(true, false, 1)); await settle();
  h.snapshots.at(-1).resolve(snapshot([state(true, false, 1)])); await settle();
  assert.equal(h.controller.view.retry, null); h.reads.at(-1).resolve(state(true, false, 1)); await operation;
  assert.equal(h.controller.view.state.blocked, true); assert.equal(h.controller.view.pending, false);
});

test("late identity lookup cannot leak into a different selected author or account", async () => {
  const h = controllerHarness(); const person = deferred(); h.source.identity = () => person.promise;
  const show = h.controller.show(target); h.reads.at(-1).resolve(state()); h.controller.account(other);
  person.resolve({ ...identity(), handle: "Previous account context" }); await show;
  assert.equal(h.controller.view.identity, null); assert.equal(h.controller.view.state, null);
});

test("deterministic independent toggle sequences use the current CAS revision and never share receipt keys", async () => {
  const h = controllerHarness(); await ready(h);
  let expected = state(), random = 91930; const keys = new Set();
  for (let i = 0; i < 48; i++) {
    random = (Math.imul(random, 1664525) + 1013904223) >>> 0;
    const field = random & 16 ? "blocked" : "muted";
    const operation = h.controller.change(field, !expected[field]);
    const intent = h.writes.at(-1).intent;
    assert.equal(intent.expected_revision, expected.revision); assert.equal(keys.has(intent.idempotency_key), false); keys.add(intent.idempotency_key);
    expected = { ...expected, [field]: !expected[field], revision: expected.revision + 1 };
    await finish(h, operation, expected);
    assert.equal(h.controller.hidden(target), expected.blocked || expected.muted); assert.equal(h.controller.blocked(target), expected.blocked);
  }
});
