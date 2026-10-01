import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import * as sdk from "@babble-protocol/sdk";
const { canonicalValueBytes } = sdk;

const context = { exports: {}, require: () => ({ canonicalValueBytes }), TextDecoder, structuredClone, Date, Error };
vm.runInNewContext(ts.transpileModule(readFileSync(new URL("../src/app/permissions.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText, context);
const { Permissions, samePermission, mayApprove, matchingGrants, permitsSurfaceStart, usesInvocationConsent } = context.exports;
const protocol = { exports: {}, require: id => id === "@babble-protocol/sdk" ? sdk : {},
  URL, Response, Headers, AbortSignal, TextEncoder, Date, Error, crypto, fetch };
vm.runInNewContext(ts.transpileModule(readFileSync(new URL("../src/app/protocol.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText, protocol);
const object = `obj_${"a".repeat(64)}`, other = `obj_${"b".repeat(64)}`;
const request = { id: "babble.storage.local", version: 1, scope: { namespace: "counter" } };
const grant = (id = "grant-one", patch = {}) => ({ id, object_id: object, capability: request.id,
  version: 1, scope: { ...request.scope }, decision: "approved", revoked_at: null, expires_at: null, ...patch });
function review(status = "requires_user", grants = []) {
  return { manifest: { object_id: object, requests: [structuredClone(request)] }, grants,
    decisions: [{ request: structuredClone(request), status, reason: "Host decision",
      definition: { permission: "ask_once" }, grant: grants[0] ?? null }] };
}
const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
const settle = () => new Promise(resolve => setImmediate(resolve));

test("permission transport uses host RPC binding and exact typed mutation payloads", async () => {
  const calls = [];
  let denied = false;
  const client = new protocol.exports.BabbleFrontendClient("https://babble.test", async (url, init) => {
    assert.equal(new URL(url).pathname, "/rpc");
    const envelope = JSON.parse(init.body); calls.push(envelope);
    return Response.json({ protocol: envelope.protocol, id: envelope.id, trace_id: null,
      result: denied ? null : envelope.method.includes("inspect") ? review() : { grants: [] },
      error: denied ? { code: "FORBIDDEN", message: "Not authorized", retryable: false, details: null } : null });
  });
  assert.equal((await client.inspectPermissions(object)).manifest.object_id, object);
  await client.approvePermission("viewer", object, request);
  await client.revokePermission("viewer", object, "grant-one");
  assert.deepEqual(calls.map(call => call.method), ["babble.capabilities.inspect.v1", "babble.capabilities.grant.v1", "babble.capabilities.revoke.v1"]);
  assert.deepEqual(calls[1].payload, { author_id: "viewer", object_id: object, capability: request, decision: "approved" });
  assert.deepEqual(calls[2].payload, { author_id: "viewer", object_id: object, grant_id: "grant-one" });
  for (const call of calls) {
    assert.deepEqual(call.binding, { object_id: null, surface_session_id: null, identity_id: null,
      runtime_id: "babble-web-runtime", origin: "browser://babble", capability_grants: [] });
    if (call.method.includes("inspect")) assert.equal(call.idempotency_key, null);
    else assert.match(call.idempotency_key, /^web-permission-/, "mutations require envelope keys even though server replay is unfinished");
  }
  denied = true;
  await assert.rejects(client.approvePermission("viewer", object, request), /Not authorized/);
});
function harness() {
  const reads = [], approvals = [], revocations = [], states = [];
  const record = (list, args) => { const pending = { ...deferred(), args }; list.push(pending); return pending.promise; };
  const source = { inspectPermissions: (...args) => record(reads, args),
    approvePermission: (...args) => record(approvals, args), revokePermission: (...args) => record(revocations, args) };
  const controller = new Permissions(state => states.push(state));
  const h = { reads, approvals, revocations, states, source, controller, authorized: true };
  h.open = () => controller.open(object, "viewer", source, () => h.authorized);
  h.ready = async (data = review()) => { const pending = h.open(); reads.at(-1).resolve(data); await pending; };
  return h;
}

test("scope equality uses canonical values and never widens identity or version", () => {
  assert.equal(samePermission(request, { ...request, scope: { namespace: "counter" } }), true);
  assert.equal(samePermission(request, { ...request, version: 2 }), false);
  assert.equal(samePermission(request, { ...request, id: "babble.storage.object" }), false);
  assert.equal(samePermission(request, { ...request, scope: { namespace: "other" } }), false);
  assert.equal(samePermission({ ...request, scope: { a: 1, b: 2 } }, { ...request, scope: { b: 2, a: 1 } }), true);
  assert.equal(samePermission({ ...request, scope: { size_bytes: 128 } }, { ...request, scope: { size_bytes: 129 } }), false);
});

test("review alone never approves; exact selected scope is sent once during a pending write", async () => {
  const h = harness(); await h.ready();
  assert.equal(h.approvals.length, 0);
  const pending = h.controller.change(0, "approve");
  await h.controller.change(0, "approve");
  assert.equal(h.approvals.length, 1);
  assert.deepEqual(h.approvals[0].args, ["viewer", object, request]);
  h.approvals[0].resolve(); await settle();
  assert.equal(h.controller.state.busy, true);
  h.reads.at(-1).resolve(review("granted", [grant()])); await pending;
  assert.equal(h.controller.state.review.decisions[0].status, "granted");
  assert.equal(h.controller.state.busy, false);
});

test("host-denied, unavailable, implicit and unsupported capabilities cannot be approved", async () => {
  for (const status of ["denied", "unavailable", "version_unsupported", "granted", "revoked"]) {
    const h = harness(); await h.ready(review(status));
    await h.controller.change(0, "approve"); assert.equal(h.approvals.length, 0);
  }
  for (const permission of ["unavailable", "denied_by_default", "implicit_safe"]) {
    assert.equal(mayApprove({ ...review().decisions[0], definition: { permission } }), false);
  }
  assert.equal(mayApprove({ ...review().decisions[0], definition: null }), false);
});

test("social and native browser invocation consent allow lazy admission but never create reusable approvals", async () => {
  for (const id of ["babble.social.follow", "babble.social.unfollow", "babble.social.reply", "babble.social.share", "babble.clipboard.write", "babble.fullscreen.enter"]) {
    const data = review();
    data.manifest.requests[0] = { id, version: 1, scope: { object_id: other } };
    const decision = data.decisions[0] = { ...data.decisions[0], request: structuredClone(data.manifest.requests[0]),
      definition: { permission: "ask_each_time" } };
    assert.equal(usesInvocationConsent(decision), true);
    assert.equal(permitsSurfaceStart(decision), true);
    assert.equal(mayApprove(decision), false);
    const h = harness(); await h.ready(data);
    await h.controller.change(0, "approve");
    assert.equal(h.approvals.length, 0);
    for (const status of ["denied", "unavailable", "version_unsupported", "revoked"]) {
      assert.equal(permitsSurfaceStart({ ...decision, status }), false);
    }
    assert.equal(permitsSurfaceStart({ ...decision, request: { ...decision.request, version: 2 } }), false);
  }
});

test("lazy invocation admission does not silently bypass other permission prerequisites", () => {
  const decision = review().decisions[0];
  assert.equal(permitsSurfaceStart(decision), false);
  assert.equal(permitsSurfaceStart({ ...decision, status: "granted" }), true);
  const external = { ...decision, request: { id: "babble.media.camera", version: 1, scope: {} },
    definition: { permission: "ask_each_time" } };
  assert.equal(usesInvocationConsent(external), false);
  assert.equal(permitsSurfaceStart(external), false);
});

test("revocation covers every live matching grant, never a different scope", async () => {
  const h = harness();
  await h.ready(review("granted", [grant("one"), grant("two"), grant("other", { scope: { namespace: "private" } }),
    grant("expired", { expires_at: "2000-01-01T00:00:00Z" }), grant("denied", { decision: "denied" }),
    grant("revoked", { revoked_at: "2026-01-01T00:00:00Z" })]));
  const pending = h.controller.change(0, "revoke");
  assert.deepEqual(h.revocations[0].args, ["viewer", object, "one"]);
  h.revocations[0].resolve(); await settle();
  assert.deepEqual(h.revocations[1].args, ["viewer", object, "two"]);
  h.revocations[1].resolve(); await settle();
  assert.equal(h.revocations.length, 2);
  h.reads.at(-1).resolve(review()); await pending;
  assert.equal(h.controller.state.review.decisions[0].status, "requires_user");
});

test("uncertain approval is reconciled without blindly repeating a grant", async () => {
  const h = harness(); await h.ready();
  const pending = h.controller.change(0, "approve");
  h.approvals[0].reject(new Error("connection lost")); await settle();
  h.reads.at(-1).resolve(review("granted", [grant()])); await pending;
  assert.match(h.controller.state.error, /not confirmed/);
  await h.controller.change(0, "approve"); assert.equal(h.approvals.length, 1);
});

test("failed reconciliation removes stale mutation controls until a successful refresh", async () => {
  const h = harness(); await h.ready();
  const pending = h.controller.change(0, "approve");
  h.approvals[0].resolve(); await settle();
  h.reads.at(-1).reject(new Error("offline")); await pending;
  assert.equal(h.controller.state.review, null);
  await h.controller.change(0, "approve"); assert.equal(h.approvals.length, 1);
  const reload = h.controller.refresh(); h.reads.at(-1).resolve(review()); await reload;
  assert.equal(h.controller.state.error, null);
});

test("closing and changing accounts prevent stale results and additional revocations", async () => {
  const h = harness(); await h.ready(review("granted", [grant("one"), grant("two")]));
  const pending = h.controller.change(0, "revoke");
  h.authorized = false; h.controller.close();
  h.revocations[0].resolve(); await pending;
  assert.equal(h.revocations.length, 1);
  assert.equal(h.controller.state.objectId, null);
  assert.equal(h.reads.length, 1);
});

test("late Object reads cannot replace the newly opened review", async () => {
  const h = harness(); const first = h.open();
  const second = h.controller.open(other, "viewer", h.source, () => true);
  const next = review(); next.manifest.object_id = other;
  h.reads[1].resolve(next); await second;
  h.reads[0].resolve(review()); await first;
  assert.equal(h.controller.state.objectId, other);
});

test("response mismatch and changed declarations fail closed", async () => {
  for (const mutate of [r => { r.manifest.object_id = other; }, r => { r.decisions[0].request.version = 2; },
    r => { r.grants = [grant("foreign", { object_id: other })]; }, r => { r.decisions = []; }]) {
    const h = harness(), data = review(); mutate(data); await h.ready(data);
    assert.equal(h.controller.state.review, null); assert.ok(h.controller.state.error);
    await h.controller.change(0, "approve"); assert.equal(h.approvals.length, 0);
  }
});

test("a concurrent grant surviving revocation is disclosed rather than reported as revoked", async () => {
  const h = harness(); await h.ready(review("granted", [grant("old")]));
  const pending = h.controller.change(0, "revoke");
  h.revocations[0].resolve(); await settle();
  h.reads.at(-1).resolve(review("granted", [grant("concurrent")])); await pending;
  assert.match(h.controller.state.error, /still active/);
  assert.equal(matchingGrants(h.controller.state.review, request).length, 1);
});

test("partial revocation failure reconciles remaining access and does not pretend success", async () => {
  const h = harness(); await h.ready(review("granted", [grant("one"), grant("two")]));
  const pending = h.controller.change(0, "revoke");
  h.revocations[0].resolve(); await settle();
  h.revocations[1].reject(new Error("network failure")); await settle();
  h.reads.at(-1).resolve(review("granted", [grant("two")])); await pending;
  assert.equal(matchingGrants(h.controller.state.review, request).length, 1);
  assert.match(h.controller.state.error, /network failure/);
});
