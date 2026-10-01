import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import { canonicalValueBytes } from "@babble-protocol/sdk";

const context = { exports: {}, URL, Error, Date, TextEncoder, AbortSignal, structuredClone,
  require: name => { assert.equal(name, "@babble-protocol/sdk"); return { canonicalValueBytes }; } };
vm.runInNewContext(ts.transpileModule(readFileSync(new URL("../src/app/browser-invocations.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText, context);
const { BrowserInvocationApi, parseBrowserInvocation: parse, normalizedBrowserPayload: normalize, isBrowserInvocationMethod } = context.exports;
const actor = `id_${"a".repeat(64)}`, object = `obj_${"b".repeat(64)}`;
const documentId = "da9bab5c-3a90-4dc2-a9c9-ade1e1f8588c", dispatchId = "e".repeat(64);
const expected = (method = "babble.clipboard.write") => ({ actorId: actor, objectId: object, method,
  requestKey: "one-browser-operation", origin: { kind: "surface", session_id: "surface-one", document_id: documentId },
  payload: method.includes("clipboard") ? { text: "  <script>literal</script>  " } : {} });
const pending = (exp = expected()) => ({ invocation_id: "d".repeat(64), actor_id: exp.actorId, object_id: exp.objectId,
  method: exp.method, request_key: exp.requestKey, origin: structuredClone(exp.origin), payload: plain(normalize(exp.method, exp.payload)),
  created_at: "2026-09-30T00:00:00Z", deadline: "2026-09-30T00:00:30Z", state: { kind: "pending" }, revision: 0,
  result: null, execution_ticket: null });
const approved = (exp = expected()) => ({ ...pending(exp), state: { kind: "approved" }, revision: 1 });
const running = (exp = expected(), ticket = false) => ({ ...pending(exp), state: { kind: "running", dispatch_id: dispatchId }, revision: 2,
  execution_ticket: ticket ? { dispatch_id: dispatchId, executor: "babble.browser.v1" } : null });
const success = (exp = expected()) => exp.method.includes("clipboard") ? { kind: "clipboard_write", written: true } : { kind: "fullscreen_enter", entered: true };
const completed = (exp = expected(), result = success(exp)) => ({ ...pending(exp), revision: 3, result,
  state: result.kind === "failed" ? { kind: "failed", code: result.code }
    : { kind: "completed", outcome: { kind: "external", dispatch_id: dispatchId, result: structuredClone(result) } } });
const plain = value => JSON.parse(JSON.stringify(value));
const signal = () => new AbortController().signal;
const api = fetcher => new BrowserInvocationApi(new URL("https://node.test/base"), fetcher);
const deferred = () => { let resolve; const promise = new Promise(yes => { resolve = yes; }); return { promise, resolve }; };

test("only durable methods are recognized; payload normalization preserves literal clipboard bytes", () => {
  for (const action of ["clipboard.write", "fullscreen.enter"]) {
    assert.equal(isBrowserInvocationMethod(`babble.${action}`), true);
    for (const version of ["v1", "v3"]) assert.equal(isBrowserInvocationMethod(`babble.${action}.${version}`), false);
  }
  assert.equal(isBrowserInvocationMethod("babble.social.reply"), false);
  const exp = expected();
  assert.deepEqual(plain(normalize(exp.method, exp.payload)), exp.payload);
  assert.deepEqual(plain(normalize("babble.fullscreen.enter", {})), { navigation_ui: "auto", target_hint: null });
  assert.deepEqual(plain(normalize("babble.fullscreen.enter", { navigation_ui: "hide", target_hint: "#untrusted" })),
    { navigation_ui: "hide", target_hint: "#untrusted" });
  for (const payload of [null, [], {}, { text: 1 }, { text: "x".repeat(65_537) }, { text: "\u00e9".repeat(32_769) }, { text: "hi", grant: "x" }]) {
    assert.throws(() => normalize(exp.method, payload));
  }
  for (const payload of [null, [], { navigation_ui: "bad" }, { target_hint: 1 }, { extra: true }]) {
    assert.throws(() => normalize("babble.fullscreen.enter", payload));
  }
});

test("clipboard payloads fit both canonical and serialized 65,536-byte journal budgets", () => {
  const method = "babble.clipboard.write";
  // The canonical object/string framing consumes 50 bytes; JSON escaping has
  // its own budget and can dominate even when the literal UTF-8 text is small.
  const cases = [
    { text: "x".repeat(65_485), encoding: "canonical" },
    { text: "\u00e9".repeat(32_742) + "x", encoding: "canonical" },
    { text: '"'.repeat(32_762) + "x", encoding: "json" },
    { text: "\\".repeat(32_762) + "x", encoding: "json" },
    { text: "\n".repeat(32_762) + "x", encoding: "json" },
    { text: "\u0000".repeat(10_920) + "xxxxx", encoding: "json" },
  ];
  for (const { text, encoding } of cases) {
    const payload = { text };
    const bytes = encoding === "canonical" ? canonicalValueBytes(payload)
      : new TextEncoder().encode(JSON.stringify(payload));
    assert.equal(bytes.byteLength, 65_536, `${encoding} boundary fixture`);
    assert.equal(normalize(method, payload).text, text, "Allowed text stays literal");
    assert.throws(() => normalize(method, { text: text + "x" }), `${encoding} overflow`);
  }
  assert.throws(() => normalize(method, { text: "x".repeat(65_536) }), "Text alone cannot consume the entire payload budget");
});

test("parser binds actor, object, method, request key, exact document/session source and normalized payload", () => {
  const exp = expected();
  const mutations = [v => v.actor_id = "other", v => v.object_id = "other", v => v.method = "babble.clipboard.write.v1",
    v => v.request_key = "other", v => v.origin.document_id = "df9bab5c-3a90-4dc2-a9c9-ade1e1f8588c",
    v => v.origin.session_id = "other", v => v.origin.kind = "host_action", v => v.origin.extra = true,
    v => v.payload.text = "substituted", v => v.payload.text = v.payload.text.trim(), v => v.payload.extra = true];
  for (const mutate of mutations) { const value = pending(exp); mutate(value); assert.throws(() => parse(value, exp), String(mutate)); }
  assert.deepEqual(parse(pending(exp), exp), pending(exp));
  const full = expected("babble.fullscreen.enter");
  assert.deepEqual(parse(pending(full), full), pending(full));
  const missingDefaults = pending(full); missingDefaults.payload = {};
  assert.throws(() => parse(missingDefaults, full));
});

test("fullscreen target hint is optional/null or a nonblank string bounded to 256 UTF-8 bytes", () => {
  const method = "babble.fullscreen.enter";
  for (const payload of [{}, { target_hint: null }]) assert.equal(normalize(method, payload).target_hint, null);
  for (const target_hint of ["#content", "  #content  ", "x".repeat(256), "\u00e9".repeat(128), "\ud83d\ude00".repeat(64)]) {
    assert.equal(normalize(method, { target_hint }).target_hint, target_hint, "Valid hints retain exact supplied text");
  }
  for (const target_hint of ["", " ", "\t\r\n", "\u00a0", "x".repeat(257), "\u00e9".repeat(129), "\ud83d\ude00".repeat(65), 0, false, [], {}]) {
    assert.throws(() => normalize(method, { target_hint }), `Invalid target hint ${JSON.stringify(target_hint).slice(0,80)}`);
  }
});

test("parser rejects malformed hashes, times, revisions, absent null fields and result/state mismatches", () => {
  const mutations = [v => v.invocation_id = "d".repeat(63), v => v.invocation_id = "D".repeat(64),
    v => v.created_at = "garbage", v => v.deadline = "garbage", v => v.deadline = v.created_at,
    v => v.deadline = "2026-09-29T23:59:59Z", v => v.deadline = "2026-09-30T00:05:01Z",
    v => v.revision = -1, v => v.revision = 1.5, v => v.revision = 5, v => v.revision = "0",
    v => v.state = null, v => v.state.kind = "invented", v => v.result = success(), v => delete v.result,
    v => delete v.execution_ticket, v => v.state = { kind: "running", dispatch_id: "short" }];
  for (const mutate of mutations) { const value = pending(); mutate(value); assert.throws(() => parse(value, expected()), String(mutate)); }
});

test("parsed snapshots do not alias mutable response, expectation, payload, origin, result or ticket", () => {
  const exp = expected(), value = running(exp, true), before = approved(exp), snapshot = parse(value, exp, before, true);
  value.payload.text = "changed"; value.origin.document_id = "changed"; value.execution_ticket.dispatch_id = "changed";
  exp.payload.text = "changed again"; exp.origin.session_id = "changed";
  assert.deepEqual(snapshot, running(expected(), true));
});

test("tickets authorize only the first approved-to-running dispatch response", () => {
  const exp = expected(), value = running(exp, true), before = approved(exp);
  assert.equal(parse(value, exp, before, true).execution_ticket.dispatch_id, dispatchId);
  assert.throws(() => parse(value, exp, before));
  assert.throws(() => parse(value, exp, undefined, true));
  for (const prior of [pending(), running(), { ...running(), state: { kind: "unknown", dispatch_id: dispatchId } }, completed()]) {
    assert.throws(() => parse(value, exp, prior, true));
  }
  for (const mutate of [v => v.execution_ticket.executor = "other", v => v.execution_ticket.dispatch_id = "f".repeat(64),
    v => v.execution_ticket.extra = true, v => v.state.kind = "unknown"]) {
    const invalid = structuredClone(value); mutate(invalid); assert.throws(() => parse(invalid, exp, before, true));
  }
  assert.equal(parse(running(), exp, running(), true).execution_ticket, null);
});

test("revision observations retain invocation identity and immutable creation/deadline", () => {
  for (const mutate of [v => v.invocation_id = "f".repeat(64), v => v.created_at = "2026-09-30T00:00:01Z",
    v => v.deadline = "2026-09-30T00:00:31Z", v => v.revision = 0,
    v => { v.revision = 1; v.state = { kind: "denied" }; }]) {
    const value = approved(); mutate(value); assert.throws(() => parse(value, expected(), approved()), String(mutate));
  }
  assert.deepEqual(parse(approved(), expected(), approved()), approved());
});

test("native typed results require matching method, true confirmation and exact external outcome", () => {
  for (const method of ["babble.clipboard.write", "babble.fullscreen.enter"]) {
    const exp = expected(method);
    assert.deepEqual(parse(completed(exp), exp, running(exp)), completed(exp));
    for (const mutate of [v => v.result = { kind: "invented" }, v => v.result.extra = true,
      v => v.result[method.includes("clipboard") ? "written" : "entered"] = false,
      v => v.state.outcome.kind = "publication", v => v.state.outcome.dispatch_id = "short",
      v => v.state.outcome.result = { kind: "failed", code: "native_error" }]) {
      const value = completed(exp); mutate(value); assert.throws(() => parse(value, exp), String(mutate));
    }
    for (const code of ["not_allowed", "unavailable", "context_lost", "native_error"]) {
      const value = completed(exp, { kind: "failed", code });
      assert.deepEqual(parse(value, exp), value);
      value.state.code = "different"; assert.throws(() => parse(value, exp));
    }
    for (const result of [{ kind: "failed", code: "unknown" }, { kind: "failed", code: "native_error", extra: true },
      success(expected(method.includes("clipboard") ? "babble.fullscreen.enter" : "babble.clipboard.write"))]) {
      assert.throws(() => parse(completed(exp, result), exp));
    }
  }
});

test("advance uses exact endpoint, HTTP verb, source document header and abort signal", async () => {
  for (const kind of ["surface", "host_action"]) {
    const exp = expected(); if (kind === "host_action") exp.origin = { kind, document_id: documentId };
    for (const action of ["status", "allow_once", "deny", "dispatch", "cancel"]) {
      const abort = signal(), before = action === "dispatch" ? approved(exp) : pending(exp);
      const value = action === "dispatch" ? running(exp, true) : action === "allow_once" ? approved(exp)
        : action === "status" ? pending(exp) : { ...pending(exp), revision: 1, state: { kind: action === "deny" ? "denied" : "cancelled" } };
      let calls = 0;
      const client = api(async (url, init) => {
        calls++; assert.equal(url.origin, "https://node.test");
        assert.equal(url.pathname, `/invocations/v1/browser/${before.invocation_id}/${["allow_once", "deny"].includes(action) ? "decision" : action}`);
        assert.equal(init.method, action === "status" ? "GET" : "POST"); assert.equal(init.signal, abort);
        assert.equal(init.headers[kind === "surface" ? "x-babble-surface-document" : "x-babble-host-document"], documentId);
        assert.equal(init.headers[kind === "surface" ? "x-babble-host-document" : "x-babble-surface-document"], undefined);
        assert.equal(init.headers.accept, "application/json");
        if (action === "status") { assert.equal(init.body, undefined); assert.equal(init.headers["content-type"], undefined); }
        else { assert.equal(init.headers["content-type"], "application/json"); assert.deepEqual(JSON.parse(init.body),
          ["allow_once", "deny"].includes(action) ? { decision: action } : {}); }
        return Response.json(value);
      });
      assert.deepEqual(await client.advance(action, before, exp, abort), value); assert.equal(calls, 1);
    }
  }
});

test("dispatch is never automatically retried; running and unknown observations have no ticket", async () => {
  let calls = 0;
  await assert.rejects(api(async () => { calls++; throw Error("connection lost after dispatch"); }).advance("dispatch", approved(), expected(), signal()));
  assert.equal(calls, 1);
  for (const kind of ["running", "unknown"]) {
    const value = { ...running(), state: { kind, dispatch_id: dispatchId } };
    const client = api(async () => Response.json(value));
    assert.equal((await client.advance("dispatch", value, expected(), signal())).execution_ticket, null);
    await assert.rejects(api(async () => Response.json(running(expected(), true))).advance("status", value, expected(), signal()));
  }
});

test("ack retries transient loss with the identical dispatch/result, never dispatching again", async () => {
  for (const first of [() => { throw Error("lost ack response"); }, () => new Response(null, { status: 503 })]) {
    const calls = [], exp = expected(), abort = signal();
    const client = api(async (url, init) => { calls.push({ path: url.pathname, body: init.body, signal: init.signal });
      return calls.length === 1 ? first() : Response.json(completed(exp)); });
    assert.deepEqual(await client.acknowledge(running(exp), exp, dispatchId, success(exp), abort), completed(exp));
    assert.equal(calls.length, 2); assert.deepEqual(calls[0], calls[1]);
    assert.equal(calls[0].path, `/invocations/v1/browser/${pending().invocation_id}/ack`);
    assert.deepEqual(JSON.parse(calls[0].body), { dispatch_id: dispatchId, result: success(exp) });
  }
});

test("ack accepts typed failures and completion from running or unknown but rejects wrong context/result/dispatch", async () => {
  for (const kind of ["running", "unknown"]) {
    for (const result of [success(), { kind: "failed", code: "native_error" }]) {
      const current = { ...running(), state: { kind, dispatch_id: dispatchId } };
      assert.deepEqual(await api(async () => Response.json(completed(expected(), result)))
        .acknowledge(current, expected(), dispatchId, result, signal()), completed(expected(), result));
    }
  }
  for (const mutate of [v => v.origin.document_id = "df9bab5c-3a90-4dc2-a9c9-ade1e1f8588c", v => v.actor_id = "other",
    v => v.state.outcome.dispatch_id = "f".repeat(64), v => v.result = { kind: "failed", code: "context_lost" },
    v => v.execution_ticket = running(expected(), true).execution_ticket]) {
    const value = completed(); mutate(value);
    await assert.rejects(api(async () => Response.json(value)).acknowledge(running(), expected(), dispatchId, success(), signal()));
  }
  let count = 0; const client = api(async () => { count++; return Response.json(completed()); });
  for (const [current, id, result] of [[pending(), dispatchId, success()], [running(), "f".repeat(64), success()],
    [running(), "short", success()], [running(), dispatchId, { kind: "clipboard_write", written: false }]]) {
    await assert.rejects(client.acknowledge(current, expected(), id, result, signal()));
  }
  assert.equal(count, 0);
});

test("HTTP status errors are sanitized and ack never retries nontransient errors or aborts", async () => {
  for (const status of [401, 403, 409, 429, 500]) {
    let calls = 0; const client = api(async () => { calls++; return new Response("PRIVATE STACK", { status }); });
    await assert.rejects(client.advance("status", pending(), expected(), signal()), e => e.status === status && !e.message.includes("PRIVATE"));
    assert.equal(calls, 1); calls = 0;
    await assert.rejects(client.acknowledge(running(), expected(), dispatchId, success(), signal()), e => e.status === status);
    assert.equal(calls, status < 500 ? 1 : 2);
  }
  const abort = new AbortController(); let calls = 0;
  const client = api(async () => { calls++; abort.abort(); return Response.json(completed()); });
  await assert.rejects(client.acknowledge(running(), expected(), dispatchId, success(), abort.signal)); assert.equal(calls, 1);
  await assert.rejects(client.advance("status", pending(), expected(), abort.signal)); assert.equal(calls, 1);
  const exp = expected(); exp.origin.document_id = "not-a-document";
  await assert.rejects(client.advance("status", pending(exp), exp, signal())); assert.equal(calls, 1);
});

test("enum validation rejects JSON arrays instead of coercing them to strings", () => {
  assert.throws(() => normalize("babble.fullscreen.enter", { navigation_ui: ["auto"] }));
  assert.throws(() => parse({ ...pending(), state: { kind: ["pending"] } }, expected()));
  const value = completed(expected(), { kind: "failed", code: ["native_error"] });
  assert.throws(() => parse(value, expected()));
});

test("later revisions cannot replace dispatch identity or resurrect completed invocation", () => {
  const before = running();
  assert.throws(() => parse({ ...before, revision: 3, state: { kind: "unknown", dispatch_id: "f".repeat(64) } }, expected(), before));
  const changed = completed(); changed.state.outcome.dispatch_id = "f".repeat(64);
  assert.throws(() => parse(changed, expected(), before));
  assert.throws(() => parse({ ...pending(), revision: 4 }, expected(), completed()));
});

test("ack snapshots caller result and expectation before awaiting network so retry retains exact intent", async () => {
  const exp = expected(), original = structuredClone(exp), result = success(), lost = deferred(), calls = [];
  const client = api(async (url, init) => {
    calls.push({ path: url.pathname, body: init.body, document: init.headers["x-babble-surface-document"] });
    return calls.length === 1 ? lost.promise : Response.json(completed(original));
  });
  const work = client.acknowledge(running(exp), exp, dispatchId, result, signal());
  result.written = false; exp.origin.document_id = "df9bab5c-3a90-4dc2-a9c9-ade1e1f8588c"; exp.payload.text = "mutated";
  lost.resolve(new Response(null, { status: 503 }));
  await assert.doesNotReject(work); assert.equal(calls.length, 2); assert.deepEqual(calls[0], calls[1]);
});

test("advance snapshots expectation and current revision before an asynchronous status response", async () => {
  const exp = expected(), current = pending(exp), original = pending(exp), waiting = deferred();
  const work = api(() => waiting.promise).advance("status", current, exp, signal());
  exp.payload.text = "mutated"; exp.origin.document_id = "df9bab5c-3a90-4dc2-a9c9-ade1e1f8588c"; current.revision = 4;
  waiting.resolve(Response.json(original)); assert.deepEqual(await work, original);
});
