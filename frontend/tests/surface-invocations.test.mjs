import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import { canonicalValueBytes } from "@babel-protocol/sdk";

const code = name => ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;
const module = { exports: {}, require: () => ({ canonicalValueBytes }), URL, Error, TextDecoder, structuredClone, Date };
vm.runInNewContext(code("invocations"), module);
const deferred = () => { let resolve; const promise = new Promise(yes => { resolve = yes; }); return { promise, resolve }; };
const tick = async () => { for (let i = 0; i < 20; i++) await Promise.resolve(); };
const actor = `id_${"a".repeat(64)}`, object = `obj_${"b".repeat(64)}`, target = `obj_${"c".repeat(64)}`;
const docId = "da9bab5c-3a90-4dc2-a9c9-ade1e1f8588c";

function request(patch = {}) {
  return { protocol: "babel.rpc.v1", id: "bridge-request", method: "babel.social.reply.v2", trace_id: "trace", idempotency_key: "stable-intent",
    deadline: { timeout_ms: 30_000 }, payload: { author_id: actor, target_object_id: target, text: "Literal <script>text</script>" },
    binding: { identity_id: actor, object_id: object, surface_session_id: "surface-one", capability_grants: [] }, ...patch };
}
function challenge(req = request()) {
  return { invocation_id: "d".repeat(64), actor_id: actor, object_id: object, method: req.method, request_key: req.idempotency_key,
    origin: { kind: "surface", session_id: "surface-one", document_id: docId },
    payload: { target_object_id: target, text: req.payload.text, media: null }, created_at: new Date().toISOString(),
    deadline: new Date(Date.now() + 30_000).toISOString(), state: { kind: "pending" }, revision: 0, result: null };
}
const required = (req, value) => ({ protocol: req.protocol, id: req.id, trace_id: req.trace_id, result: null,
  error: { code: "PERMISSION_REQUIRED", message: "Consent required", details: { invocation: value }, retryable: false, retry_after_ms: null } });
function harness(acquireConsent) {
  const prompts = [], actions = [], warnings = [], timers = new Set(), intervals = new Set();
  let allowed = true, response, dispatchImpl, advanceImpl;
  const targetElement = { isConnected: true, ownerDocument: { visibilityState: "visible" } };
  const options = { target: targetElement, title: "Actual Object", identity: { id: actor, handle: "Alice" }, authorized: () => allowed, acquireConsent,
    api: { async advance(action, current, expected, signal) {
      actions.push({ action, current, expected, signal });
      if (advanceImpl) return advanceImpl(action, current, expected, signal);
      if (action === "status") return current;
      if (action === "execute") return { ...current, revision: 2, state: { kind: "completed" }, result: { object: { id: "published" }, edge: {}, receipt: {} } };
      return { ...current, revision: current.revision + 1, state: { kind: action === "allow_once" ? "approved" : action === "deny" ? "denied" : "cancelled" } };
    } } };
  class Prompt {
    prompt(summary, context) {
      const wait = deferred();
      const cancel = () => wait.resolve("cancel");
      context.signal.addEventListener("abort", cancel, { once: true });
      prompts.push({ summary, context, resolve: wait.resolve });
      return wait.promise.finally(() => context.signal.removeEventListener("abort", cancel));
    }
    dispose() { prompts.at(-1)?.resolve("cancel"); }
  }
  const context = { exports: {}, require: name => name === "./invocations" ? module.exports : { InvocationPrompt: Prompt },
    AbortController, AbortSignal, Error, structuredClone, console: { warn: (...args) => warnings.push(args) },
    setTimeout: fn => { timers.add(fn); return fn; }, clearTimeout: fn => timers.delete(fn),
    setInterval: fn => { intervals.add(fn); return fn; }, clearInterval: fn => intervals.delete(fn) };
  vm.runInNewContext(code("surface-invocations"), context);
  const controller = new context.exports.SurfaceInvocations(options);
  const dispatches = [];
  const dispatch = controller.wrap(async (req, ctx) => {
    dispatches.push({ req, ctx });
    return dispatchImpl ? dispatchImpl(req, ctx) : response ?? required(req, challenge(req));
  });
  const abort = new AbortController();
  return { controller, dispatch, prompts, actions, warnings, timers, intervals, dispatches, targetElement,
    context: { signal: abort.signal, surfaceDocumentId: docId }, abort,
    set allowed(value) { allowed = value; }, set response(value) { response = value; },
    set dispatchImpl(value) { dispatchImpl = value; }, set advanceImpl(value) { advanceImpl = value; },
    poll: () => { for (const fn of intervals) fn(); } };
}

test("unrelated reads preserve envelope and context", async () => {
  const h = harness(), req = request({ method: "babel.object.get.v1" });
  const expected = h.response = { id: req.id, result: {} };
  assert.equal(await h.dispatch(req, h.context), expected);
  assert.equal(h.dispatches[0].ctx, h.context);
  assert.equal(h.actions.length, 0); h.controller.dispose();
});

test("social request waits for host decision then executes stored intent once", async () => {
  const h = harness(), req = request(), work = h.dispatch(req, h.context);
  await tick();
  assert.deepEqual(h.actions.map(a => a.action), ["status"]);
  assert.equal(h.prompts[0].summary.text, req.payload.text);
  assert.equal(h.prompts[0].summary.actor.id, actor);
  assert.equal(h.prompts[0].summary.requester.id, object);
  assert.equal(h.prompts[0].summary.recipient.id, target);
  assert.equal(h.dispatches[0].ctx.surfaceDocumentId, docId);
  h.prompts[0].resolve("allow");
  const result = await work;
  assert.deepEqual(h.actions.map(a => a.action), ["status", "allow_once", "execute"]);
  assert.equal(result.id, req.id); assert.equal(result.trace_id, req.trace_id);
  assert.equal(result.error, null); assert.equal(result.result.object.id, "published");
  assert.equal(h.timers.size + h.intervals.size, 0); h.controller.dispose();
});

for (const decision of ["deny", "cancel"]) {
  test(`${decision} records no execution and never grants reusable capability`, async () => {
    const h = harness(), work = h.dispatch(request(), h.context); await tick();
    h.prompts[0].resolve(decision);
    const result = await work;
    assert.equal(result.error.code, decision === "deny" ? "CAPABILITY_DENIED" : "CANCELLED");
    assert.deepEqual(h.actions.map(a => a.action), ["status", decision]); h.controller.dispose();
  });
}

test("busy social prompt rejects social flooding before dispatch", async () => {
  const h = harness(), work = h.dispatch(request(), h.context); await tick();
  for (const method of ["babel.social.reply.v2", "babel.social.follow.v2", "babel.social.share.v2"]) {
    assert.equal((await h.dispatch(request({ id: "other", method }), h.context)).error.code, "RATE_LIMITED");
  }
  assert.equal(h.dispatches.length, 1); assert.equal(h.prompts.length, 1);
  h.prompts[0].resolve("deny"); await work; h.controller.dispose();
});

test("shared gate rejects a browser owner before RPC and releases a social owner exactly once", async () => {
  let busy = true, releases = 0;
  const acquire = () => {
    if (busy) return null;
    busy = true; let released = false;
    return () => { assert.equal(released, false); released = true; busy = false; releases++; };
  };
  const h = harness(acquire);
  assert.equal((await h.dispatch(request(), h.context)).error.code, "RATE_LIMITED");
  assert.equal(h.dispatches.length, 0); assert.equal(h.prompts.length, 0); assert.equal(releases, 0);
  busy = false;
  const work = h.dispatch(request(), h.context); await tick(); assert.equal(busy, true);
  assert.equal(acquire(), null, "A competing browser controller cannot acquire consent");
  h.prompts[0].resolve("deny"); await work;
  assert.equal(busy, false); assert.equal(releases, 1); h.controller.dispose(); assert.equal(releases, 1);
});

test("social cancellation holds the shared gate through late approval and cancellation reconciliation", async () => {
  let busy = false, releases = 0;
  const h = harness(() => { if (busy) return null; busy = true; return () => { busy = false; releases++; }; });
  const approval = deferred(), reconciliation = deferred();
  h.advanceImpl = async (action, current) => action === "allow_once" ? approval.promise : action === "cancel" ? reconciliation.promise : current;
  const work = h.dispatch(request(), h.context); await tick(); h.prompts[0].resolve("allow"); await tick(); h.abort.abort();
  assert.equal(busy, true); assert.equal(releases, 0);
  approval.resolve({ ...h.actions[0].current, state: { kind: "approved" }, revision: 1 }); await tick();
  assert.equal(busy, true); assert.equal(releases, 0); assert.equal(h.actions.at(-1).action, "cancel");
  reconciliation.resolve({ ...h.actions[0].current, state: { kind: "cancelled" }, revision: 2 });
  assert.equal((await work).error.code, "CANCELLED"); assert.equal(busy, false); assert.equal(releases, 1);
  assert.equal(h.actions.some(action => action.action === "execute"), false); h.controller.dispose();
});

test("social gate releases after RPC error, invalid challenge and disposal during initial RPC", async () => {
  for (const mode of ["rpc-error", "invalid-challenge", "dispose"]) {
    let releases = 0; const h = harness(() => () => { releases++; }), reply = deferred(), req = request();
    if (mode === "rpc-error") h.dispatchImpl = async () => { throw Error("offline"); };
    if (mode === "invalid-challenge") h.response = required(req, { ...challenge(req), actor_id: "other" });
    if (mode === "dispose") h.dispatchImpl = () => reply.promise;
    const work = h.dispatch(req, h.context);
    if (mode === "dispose") { h.controller.dispose(); assert.equal(releases, 0); reply.resolve(required(req, challenge(req))); }
    assert.ok((await work).error); assert.equal(releases, 1); assert.equal(h.prompts.length, 0); h.controller.dispose();
  }
});

test("missing trusted context fails before any dispatch or prompt", async () => {
  for (const change of [r => { r.binding.identity_id = "other"; }, r => { r.binding.object_id = null; },
    r => { r.binding.surface_session_id = null; }, r => { r.idempotency_key = null; }]) {
    const h = harness(), req = request(); change(req);
    assert.equal((await h.dispatch(req, h.context)).error.code, "CAPABILITY_DENIED");
    assert.equal(h.dispatches.length, 0); h.controller.dispose();
  }
});

for (const change of [v => { v.payload.text = "substitution"; }, v => { v.origin.document_id = crypto.randomUUID(); },
  v => { v.actor_id = "other"; }, v => { v.request_key = "other"; }]) {
  test("mismatched challenge cannot reach prompt or approval", async () => {
    const h = harness(), req = request(), value = challenge(req); change(value);
    h.response = required(req, value);
    assert.equal((await h.dispatch(req, h.context)).error.code, "INTERNAL");
    assert.equal(h.prompts.length, 0); assert.equal(h.actions.length, 0); h.controller.dispose();
  });
}

for (const kind of ["signal", "dispose", "account", "hidden", "timeout"]) {
  test(`${kind} cancels pending intent and suppresses late allow`, async () => {
    const h = harness(), work = h.dispatch(request(), h.context); await tick();
    if (kind === "signal") h.abort.abort();
    if (kind === "dispose") h.controller.dispose();
    if (kind === "account") { h.allowed = false; h.poll(); }
    if (kind === "hidden") { h.targetElement.ownerDocument.visibilityState = "hidden"; h.poll(); }
    if (kind === "timeout") for (const fn of h.timers) fn();
    h.prompts[0].resolve("allow");
    assert.equal((await work).error.code, "CANCELLED");
    assert.deepEqual(h.actions.map(a => a.action), ["status", "cancel"]);
    assert.equal(h.timers.size + h.intervals.size, 0); h.controller.dispose();
  });
}

test("closing while approval is in flight cannot execute after acknowledgement", async () => {
  const h = harness(), approved = deferred();
  h.advanceImpl = async (action, current) => action === "allow_once" ? approved.promise : current;
  const work = h.dispatch(request(), h.context); await tick(); h.prompts[0].resolve("allow"); await tick();
  h.controller.dispose();
  approved.resolve({ ...h.actions[0].current, state: { kind: "approved" }, revision: 1 });
  assert.equal((await work).error.code, "CANCELLED");
  assert.deepEqual(h.actions.map(a => a.action), ["status", "allow_once", "cancel"]);
});

test("real API rejection or completed retry does not create a second prompt", async () => {
  for (const response of [{ error: { code: "CAPABILITY_DENIED" }, result: null }, { error: null, result: { object: {} } }]) {
    const h = harness(), req = request(); h.response = { ...response, id: req.id, protocol: req.protocol };
    const result = await h.dispatch(req, h.context);
    assert.equal(result.error, response.error); assert.equal(h.prompts.length, 0); assert.equal(h.actions.length, 0);
    h.controller.dispose();
  }
});

for (const loss of ["none", "abort", "account", "dispose"]) {
  test(`lost execution acknowledgement reconciles completed result with ${loss} context loss`, async () => {
    const h = harness(), receipt = { object: { id: "committed" }, edge: {}, receipt: {} };
    h.advanceImpl = async (action, current) => {
      if (action === "allow_once") return { ...current, state: { kind: "approved" }, revision: 1 };
      if (action === "execute") throw new Error("Connection reset after commit");
      if (action === "cancel") {
        if (loss === "abort") h.abort.abort();
        if (loss === "account") h.allowed = false;
        if (loss === "dispose") h.controller.dispose();
        return { ...current, state: { kind: "completed" }, revision: 2, result: receipt };
      }
      return current;
    };
    const work = h.dispatch(request(), h.context); await tick(); h.prompts[0].resolve("allow");
    const result = await work;
    assert.deepEqual(h.actions.map(a => a.action), ["status", "allow_once", "execute", "cancel"]);
    if (loss === "none") { assert.equal(result.error, null); assert.equal(result.result, receipt); }
    else { assert.equal(result.error.code, "CANCELLED"); assert.equal(result.result, null); }
    assert.equal(h.timers.size + h.intervals.size, 0); h.controller.dispose();
  });
}
