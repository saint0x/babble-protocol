import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import { canonicalValueBytes } from "@babble-protocol/sdk";

const context = { exports: {}, URL, Error, Date, TextDecoder, AbortSignal, structuredClone,
  console: { warn() {} }, require: () => ({ canonicalValueBytes }) };
vm.runInNewContext(ts.transpileModule(readFileSync(new URL("../src/app/invocations.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText, context);
const { InvocationApi, parseInvocation, normalizedInvocationPayload, isInvocationMethod } = context.exports;
const actor = `id_${"a".repeat(64)}`, object = `obj_${"b".repeat(64)}`, target = `obj_${"c".repeat(64)}`;
const documentId = "da9bab5c-3a90-4dc2-a9c9-ade1e1f8588c";
const expected = () => ({ actorId: actor, objectId: object, method: "babble.social.reply", requestKey: "one-operation",
  origin: { kind: "host_action", document_id: documentId }, payload: { author_id: actor, target_object_id: target, text: "  Literal <script>text</script>  " } });
const pending = (exp = expected()) => ({ invocation_id: "d".repeat(64), actor_id: exp.actorId, object_id: exp.objectId,
  method: exp.method, request_key: exp.requestKey, origin: structuredClone(exp.origin),
  payload: { target_object_id: target, text: "Literal <script>text</script>", media: null },
  created_at: "2026-09-30T00:00:00Z", deadline: "2026-09-30T00:00:30Z", state: { kind: "pending" }, revision: 0, result: null });
const completed = (base = pending()) => ({ ...base, state: { kind: "completed", outcome: { kind: "publication", receipt: "e".repeat(64) } }, revision: 2,
  result: { object: { id: `obj_${"f".repeat(64)}`, author: actor }, edge: { id: `edge_${"e".repeat(64)}`, author: actor },
    receipt: { request: { author: actor, id: "e".repeat(64), fingerprint: "f".repeat(64) },
      outcome: { object: `obj_${"f".repeat(64)}`, edges: [`edge_${"e".repeat(64)}`], event: `evt_${"a".repeat(64)}` } } } });
const plain = value => JSON.parse(JSON.stringify(value));

test("invocation methods use  without changing other social contracts", () => {
  for (const name of ["follow", "unfollow", "reply", "share"]) {
    assert.equal(isInvocationMethod(`babble.social.${name}`), true);
    assert.equal(isInvocationMethod(`babble.social.${name}.v1`), false);
  }
  assert.equal(isInvocationMethod("babble.social.replies.list.v1"), false);
});

test("normalization freezes server defaults without inventing actor or widening scope", () => {
  assert.deepEqual(plain(normalizedInvocationPayload("babble.social.reply", { text: " hello " }, object)), {
    target_object_id: object, text: "hello", media: null,
  });
  assert.deepEqual(plain(normalizedInvocationPayload("babble.social.follow", { target_object_id: target }, object)), {
    target_object_id: target, text: null, media: null,
  });
  assert.throws(() => normalizedInvocationPayload("babble.social.reply", { text: 5 }, object));
});

test("authoritative intent readback is a detached snapshot", () => {
  const input = pending(), result = parseInvocation(input, expected());
  input.payload.text = "changed";
  assert.equal(result.payload.text, "Literal <script>text</script>");
});

test("binary canonical comparison rejects adjacent media sizes with invalid UTF-8 bytes", () => {
  const exp = expected();
  exp.payload.media = { title: "Image", resources: [{ uri: "media:test", media_type: "image/png", size_bytes: 128, integrity: "a".repeat(64) }] };
  const value = pending(exp);
  value.payload = plain(normalizedInvocationPayload(exp.method, exp.payload, exp.objectId));
  assert.equal(parseInvocation(value, exp).payload.media.resources[0].size_bytes, 128);
  value.payload.media.resources[0].size_bytes = 129;
  assert.throws(() => parseInvocation(value, exp));
});

for (const [name, change] of Object.entries({
  actor: v => { v.actor_id = "another"; }, object: v => { v.object_id = target; },
  method: v => { v.method = "babble.social.share"; }, key: v => { v.request_key = "other"; },
  document: v => { v.origin.document_id = crypto.randomUUID(); }, origin: v => { v.origin.kind = "surface"; },
  text: v => { v.payload.text += " changed"; }, target: v => { v.payload.target_object_id = object; },
  expiry: v => { v.deadline = "invalid"; }, budget: v => { v.deadline = "2026-10-01T00:00:00Z"; },
  backwards: v => { v.deadline = v.created_at; }, challenge: v => { v.invocation_id = "../decision"; },
  revision: v => { v.revision = 99; }, state: v => { v.state = { kind: "running" }; },
  result: v => { v.result = {}; },
})) {
  test(`reject changed or invalid ${name} before approval`, () => {
    const value = pending(); change(value);
    assert.throws(() => parseInvocation(value, expected()));
  });
}

test("subsequent responses cannot change the immutable challenge or deadline or move backwards", () => {
  const original = { ...pending(), revision: 1, state: { kind: "approved" } };
  for (const patch of [{ invocation_id: "e".repeat(64) }, { deadline: "2026-09-30T00:00:40Z" }, { revision: 0 }]) {
    assert.throws(() => parseInvocation({ ...original, ...patch }, expected(), original));
  }
  assert.equal(parseInvocation(completed(), expected(), original).state.kind, "completed");
});

test("completed results must belong to the approved actor and contain their publication", () => {
  for (const change of [v => { v.result.object.author = "other"; }, v => { v.result.edge.author = "other"; },
    v => { delete v.result.object; }, v => { delete v.result.receipt; }]) {
    const value = completed(); change(value); assert.throws(() => parseInvocation(value, expected()));
  }
});

test("host transport uses distinct header, exact body and confirmation flow with no grant call", async () => {
  const calls = [], exp = expected();
  const api = new InvocationApi(new URL("https://node.test"), async (url, init) => {
    const body = init.body && JSON.parse(init.body);
    calls.push({ path: url.pathname, method: init.method, headers: init.headers, body });
    if (url.pathname.endsWith("recover")) {
      assert.equal(init.headers["x-babble-host-document"], undefined);
      assert.equal(init.headers["x-babble-surface-document"], undefined);
      assert.deepEqual(body, { object_id: object, method: exp.method, request_key: exp.requestKey, payload: exp.payload });
      return new Response(null, { status: 204 });
    }
    if (init.method === "PUT") return Response.json({ document_id: documentId, object_id: object,
      expires_at: new Date(Date.now() + 60_000).toISOString(), renew_after_ms: 20_000 });
    if (init.method === "DELETE") return new Response(null, { status: 204 });
    assert.equal(init.headers["x-babble-host-document"], documentId);
    assert.equal(init.headers["x-babble-surface-document"], undefined);
    if (url.pathname.endsWith("prepare")) {
      assert.equal(body.request_key, exp.requestKey);
      assert.deepEqual(body.payload, exp.payload);
      return Response.json(pending());
    }
    if (url.pathname.endsWith("decision")) {
      assert.deepEqual(body, { decision: "allow_once" });
      return Response.json({ ...pending(), revision: 1, state: { kind: "approved" } });
    }
    assert.ok(url.pathname.endsWith("execute"));
    return Response.json(completed());
  });
  assert.equal((await api.performHost(exp, new AbortController().signal)).object.author, actor);
  assert.deepEqual(calls.map(c => c.method), ["POST", "PUT", "POST", "POST", "POST", "DELETE"]);
  assert.equal(calls[1].headers["x-babble-host-document"], undefined);
  assert.deepEqual(calls[1].body, { object_id: object });
});

test("completed prepare retry returns stored result without another decision or execution", async () => {
  const calls = [], api = new InvocationApi(new URL("https://node.test"), async (url, init) => {
    calls.push(url.pathname);
    if (url.pathname.endsWith("recover")) return new Response(null, { status: 204 });
    if (init.method === "PUT") return Response.json({ document_id: documentId, object_id: object,
      expires_at: new Date(Date.now() + 60_000).toISOString(), renew_after_ms: 20_000 });
    if (init.method === "DELETE") return new Response(null, { status: 204 });
    return Response.json(completed());
  });
  await api.performHost(expected(), new AbortController().signal);
  assert.equal(calls.some(p => p.endsWith("decision") || p.endsWith("execute")), false);
});

test("Surface operations use registered document header, never host authority", async () => {
  const exp = { ...expected(), origin: { kind: "surface", session_id: "surface-one", document_id: documentId } };
  const api = new InvocationApi(new URL("https://node.test"), async (url, init) => {
    assert.equal(init.headers["x-babble-surface-document"], documentId);
    assert.equal(init.headers["x-babble-host-document"], undefined);
    assert.equal(init.method, "GET"); assert.equal(init.body, undefined);
    assert.ok(url.pathname.endsWith("/status"));
    return Response.json(pending(exp));
  });
  await api.advance("status", pending(exp), exp, new AbortController().signal);
  await assert.rejects(api.performHost(exp, new AbortController().signal));
});

test("registration mismatch or stale account cannot progress to prepare", async () => {
  for (const response of [{ document_id: "other", object_id: object }, { document_id: documentId, object_id: target }]) {
    let calls = 0;
    const api = new InvocationApi(new URL("https://node.test"), async (url) => {
      if (url.pathname.endsWith("recover")) return new Response(null, { status: 204 });
      calls++; return Response.json({ ...response, expires_at: new Date(Date.now() + 60_000).toISOString(), renew_after_ms: 20_000 });
    });
    await assert.rejects(api.performHost(expected(), new AbortController().signal));
    assert.equal(calls, 1);
  }
  const controller = new AbortController(); let calls = 0;
  const api = new InvocationApi(new URL("https://node.test"), async () => { calls++; controller.abort(); return Response.json(pending()); });
  await assert.rejects(api.prepare(expected(), 30_000, controller.signal));
  await assert.rejects(api.prepare(expected(), 30_000, controller.signal));
  assert.equal(calls, 1);
});

test("completed host history survives a replacement document without registering or executing", async () => {
  const exp = expected(); exp.origin.document_id = crypto.randomUUID();
  const calls = [], api = new InvocationApi(new URL("https://node.test"), async (url, init) => {
    calls.push(url.pathname);
    assert.equal(init.headers["x-babble-host-document"], undefined);
    return Response.json(completed());
  });
  assert.equal((await api.performHost(exp, new AbortController().signal)).object.author, actor);
  assert.deepEqual(calls, ["/invocations/v1/recover"]);
});

test("history recovery rejects pending, Surface, wrong-actor or altered-intent responses", async () => {
  for (const value of [pending(), { ...completed(), origin: { kind: "surface", session_id: "surface", document_id: documentId } },
    { ...completed(), actor_id: "other" }, { ...completed(), payload: { target_object_id: target, text: "different", media: null } }]) {
    let calls = 0;
    const api = new InvocationApi(new URL("https://node.test"), async () => { calls++; return Response.json(value); });
    await assert.rejects(api.performHost(expected(), new AbortController().signal));
    assert.equal(calls, 1);
  }
});

test("HTTP failures retain status without exposing upstream error bodies or retrying", async () => {
  for (const status of [401, 403, 409, 429, 500]) {
    let count = 0;
    const api = new InvocationApi(new URL("https://node.test"), async () => { count++; return new Response("PRIVATE STACK", { status }); });
    await assert.rejects(api.prepare(expected(), 30_000, new AbortController().signal), error => error.status === status && !error.message.includes("PRIVATE"));
    assert.equal(count, 1);
  }
});
