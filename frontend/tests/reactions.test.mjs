import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

function module(name, require = () => ({})) {
  const context = { exports: {}, require, Error, URL, Request, Response, Headers, AbortController, AbortSignal,
    Event, EventTarget, TextDecoder, setTimeout, clearTimeout, crypto };
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, context);
  return context.exports;
}
const { Accounts } = module("accounts");
const { Reactions, ReactionClient, ReactionError, parseReactionValue, parseReactionState, parseReactionSummary } = module("reactions", () => ({ Accounts }));
const actor = `id_${"a".repeat(64)}`, other = `id_${"b".repeat(64)}`;
const object = `obj_${"c".repeat(64)}`, next = `obj_${"d".repeat(64)}`;
const value = (patch = {}) => ({ appreciation: null, engagement: null, stance: null, certainty: null, ...patch });
const state = (patch = {}) => ({ author_id: actor, object_id: object, value: value(), revision: 0, ...patch });
const summary = (patch = {}) => ({ object_id: object, participants: 0, likes: 0, dislikes: 0, engaging: 0, not_engaging: 0,
  support: 0, oppose: 0, uncertain: 0, certainty_responses: 0, ...patch });
const session = (token = "a", owner = actor) => ({ identity: { id: owner, handle: "Reader" }, token: token.repeat(64), expires_at: "2099-01-01T00:00:00Z" });
const plain = (data) => JSON.parse(JSON.stringify(data));
const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
const settle = () => new Promise((resolve) => setImmediate(resolve));

test("canonical boundaries validate every enum, number, owner and object", () => {
  for (const appreciation of [null, "like", "dislike"]) for (const engagement of [null, "engaging", "not_engaging"])
    for (const stance of [null, "support", "oppose", "uncertain"]) {
      const input = value({ appreciation, engagement, stance, certainty: stance ? 0 : null });
      assert.deepEqual(plain(parseReactionValue(input)), input);
    }
  for (const invalid of [null, {}, [], value({ appreciation: "support" }), value({ engagement: false }), value({ stance: "neutral" }),
    value({ certainty: 50 }), value({ stance: "support", certainty: -1 }), value({ stance: "support", certainty: 101 }),
    value({ stance: "support", certainty: 2.5 }), value({ stance: "support", certainty: "50" }), value({ certainty: undefined })]) {
    assert.throws(() => parseReactionValue(invalid));
  }
  assert.equal(parseReactionValue(value({ stance: "uncertain", certainty: 100 })).certainty, 100);
  assert.deepEqual(plain(parseReactionState(state(), actor, object)), state());
  for (const patch of [{ author_id: other }, { object_id: next }, { revision: -1 }, { revision: 1.5 }, { revision: 2 ** 53 }, { value: {} }]) {
    assert.throws(() => parseReactionState(state(patch), actor, object));
  }
  for (const key of Object.keys(summary()).filter((key) => key !== "object_id")) {
    for (const bad of [undefined, null, -1, 0.2, "1", NaN, Infinity, 2 ** 53]) assert.throws(() => parseReactionSummary(summary({ [key]: bad }), object));
  }
  assert.throws(() => parseReactionSummary(summary({ object_id: next }), object));
  assert.equal(parseReactionSummary(summary({ participants: Number.MAX_SAFE_INTEGER, likes: Number.MAX_SAFE_INTEGER }), object).participants, Number.MAX_SAFE_INTEGER);
  for (const patch of [{ likes: 1 }, { participants: 2, likes: 1 }, { participants: 1, likes: 1, dislikes: 1 },
    { participants: 1, support: 1, oppose: 1 }, { participants: 1, engaging: 1, not_engaging: 1 },
    { participants: 1, likes: 1, certainty_responses: 1 }]) assert.throws(() => parseReactionSummary(summary(patch), object));
  assert.throws(() => parseReactionState(state({ value: value({ appreciation: "like" }) }), actor, object));
});

function harness(loggedIn = true) {
  const accounts = new EventTarget(); accounts.current = loggedIn ? session() : null;
  const summaries = [], reads = [], writes = [], changed = [];
  const request = (list, data) => { const item = { ...deferred(), ...data }; list.push(item); return item.promise; };
  const client = { capture(valid) {
    const owner = accounts.current?.identity.id ?? null;
    return { actor: owner,
      summary: (object, signal) => request(summaries, { object, signal, valid }),
      state: (object, signal) => request(reads, { object, signal, owner, valid }),
      update: (object, intent, signal) => request(writes, { object, intent, signal, owner, valid }),
    };
  } };
  let keys = 0;
  const control = new Reactions(accounts, (view) => changed.push(view), client, () => `intent-${++keys}`);
  return { control, accounts, summaries, reads, writes, changed,
    account(next) { accounts.current = next; accounts.dispatchEvent(new Event("change")); } };
}
async function read(h, data = state(), aggregate = summary()) {
  h.summaries.at(-1).resolve(aggregate); await settle();
  if (h.accounts.current) { h.reads.at(-1).resolve(data); await settle(); }
}
async function ready(h) { const start = h.control.select(object); await read(h); await start; }
async function publish(h, patch = { appreciation: "like" }) {
  await h.control.publish(value(patch));
  assert.ok(h.control.view.confirmation);
  return { operation: h.control.confirm() };
}

test("signed-out reads only public totals; selection does not manufacture a private state", async () => {
  const h = harness(false); await ready(h);
  assert.equal(h.reads.length, 0); assert.equal(h.control.view.summary.likes, 0);
  await h.control.publish(value({ appreciation: "like" })); await h.control.confirm(); await h.control.retry();
  assert.equal(h.writes.length, 0); assert.equal(h.control.view.state, null);
  h.control.dispose();
});

test("first publication needs consent; cancel does not write; subsequent choices do not reprompt", async () => {
  const h = harness(); await ready(h);
  await h.control.publish(value({ stance: "uncertain" })); assert.equal(h.writes.length, 0);
  h.control.cancelConfirmation(); assert.equal(h.control.view.confirmation, null);
  const { operation } = await publish(h);
  await h.control.confirm(); await h.control.publish(value({ appreciation: "dislike" })); await h.control.retry();
  assert.equal(h.writes.length, 1);
  assert.equal(h.control.view.state.value.appreciation, null); assert.equal(h.control.view.summary.likes, 0);
  h.writes[0].resolve(state({ value: value({ appreciation: "like" }), revision: 1 })); await settle();
  assert.equal(h.control.view.state, null);
  await read(h, state({ revision: 4, value: value({ appreciation: "dislike" }) }), summary({ dislikes: 1, participants: 1 })); await operation;
  assert.equal(h.control.view.state.value.appreciation, "dislike"); assert.equal(h.control.view.state.revision, 4);
  const withdrawal = h.control.publish(value());
  assert.equal(h.writes.length, 2); assert.equal(h.writes[1].intent.expected_revision, 4);
  assert.deepEqual(plain(h.writes[1].intent.value), value());
  h.writes[1].resolve(state({ revision: 5 })); await settle(); await read(h, state({ revision: 5 })); await withdrawal;
  assert.equal(h.control.view.confirmation, null); h.control.dispose();
});

for (const failure of [new Error("lost ack"), new ReactionError(503)]) test(`uncertain ${failure.message} retries exact payload even if fresh GET already shows the write`, async () => {
  const h = harness(); await ready(h);
  const { operation } = await publish(h, { stance: "uncertain", certainty: 0 });
  h.writes[0].reject(failure); await settle(); await read(h, state({ revision: 1, value: value({ stance: "uncertain", certainty: 0 }) })); await operation;
  assert.equal(h.control.view.retry, true);
  await h.control.publish(value({ appreciation: "dislike" })); assert.equal(h.writes.length, 1);
  const retry = h.control.retry(); assert.equal(h.writes.length, 2);
  assert.deepEqual(h.writes[0].intent, h.writes[1].intent);
  h.writes[1].resolve(state({ revision: 1 })); await settle(); await read(h, state({ revision: 8 })); await retry;
  assert.equal(h.control.view.state.revision, 8); assert.equal(h.control.view.retry, false); h.control.dispose();
});

test("409 refreshes without automatic overwrite; choose-again uses current revision and fresh key", async () => {
  const h = harness(); await ready(h); const { operation } = await publish(h);
  h.writes[0].reject(new ReactionError(409)); await settle(); await read(h, state({ revision: 7 })); await operation;
  assert.match(h.control.view.message, /choose again/); assert.equal(h.control.view.retry, false); assert.equal(h.writes.length, 1);
  const fresh = h.control.publish(value({ appreciation: "dislike" }));
  assert.equal(h.writes[1].intent.expected_revision, 7); assert.notEqual(h.writes[1].intent.idempotency_key, h.writes[0].intent.idempotency_key);
  h.writes[1].resolve(state({ revision: 8 })); await settle(); await read(h, state({ revision: 8 })); await fresh; h.control.dispose();
});

test("successful PUT with failed readback exposes Refresh, never historic receipt or unsafe new write", async () => {
  const h = harness(); await ready(h); const { operation } = await publish(h);
  h.writes[0].resolve(state({ revision: 1, value: value({ appreciation: "like" }) })); await settle();
  h.summaries.at(-1).reject(new Error("offline")); await settle(); h.reads.at(-1).reject(new Error("offline")); await operation;
  assert.equal(h.control.view.state, null); assert.equal(h.control.view.summary, null); assert.equal(h.control.view.needsRefresh, true);
  assert.equal(h.control.view.retry, false); assert.match(h.control.view.message, /saved/);
  await h.control.publish(value()); assert.equal(h.writes.length, 1);
  const refresh = h.control.refresh(); await read(h, state({ revision: 6 })); await refresh;
  assert.equal(h.control.view.state.revision, 6); assert.equal(h.control.view.needsRefresh, false); h.control.dispose();
});

test("swiping aborts reads and writes; reentry retains exact uncertain intent and still reads current state", async () => {
  const h = harness(); await ready(h); const { operation } = await publish(h);
  const second = h.control.select(next);
  assert.equal(h.writes[0].signal.aborted, true);
  h.writes[0].resolve(state({ revision: 1 })); await operation;
  assert.equal(h.summaries.length, 2);
  await read(h, state({ object_id: next }), summary({ object_id: next })); await second;
  assert.equal(h.control.view.retry, false);
  const reenter = h.control.select(object); await read(h, state({ revision: 1 })); await reenter;
  assert.equal(h.control.view.retry, true);
  const retry = h.control.retry(); assert.deepEqual(h.writes[1].intent, h.writes[0].intent);
  h.writes[1].resolve(state({ revision: 1 })); await settle(); await read(h, state({ revision: 1 })); await retry; h.control.dispose();
});

test("old private and public completions cannot repopulate a new card or detached panel", async () => {
  const h = harness(); const old = h.control.select(object); const request = h.summaries[0];
  await h.control.select(null); request.resolve(summary()); await old;
  assert.equal(h.control.view.object, null); assert.equal(h.control.view.summary, null); assert.equal(h.reads.length, 0);
  const start = h.control.select(object); h.summaries.at(-1).resolve(summary()); await settle();
  const mine = h.reads.at(-1); await h.control.select(null); mine.resolve(state()); await start;
  assert.equal(h.control.view.state, null); assert.equal(mine.signal.aborted, true); h.control.dispose();
});

test("same-identity new-token relogin discards consent and pending intent, invalidates transport generation", async () => {
  const h = harness(); await ready(h); const { operation } = await publish(h);
  const old = h.writes[0]; h.account(session("b"));
  assert.equal(old.valid(), false); assert.equal(old.signal.aborted, true);
  old.reject(new Error("old request")); await operation; await read(h);
  assert.equal(h.control.view.retry, false);
  await h.control.publish(value({ appreciation: "dislike" }));
  assert.ok(h.control.view.confirmation); assert.equal(h.writes.length, 1);
  h.account(null); await read(h);
  assert.equal(h.control.view.actor, null); assert.equal(h.control.view.confirmation, null); h.control.dispose();
});

test("dispose invalidates everything, unsubscribes account changes and never reloads", async () => {
  const h = harness(); const start = h.control.select(object); h.control.dispose();
  h.summaries[0].resolve(summary()); await start; h.account(session("b"));
  await h.control.select(next); assert.equal(h.summaries.length, 1); assert.equal(h.reads.length, 0);
});

function transportHarness(request, publicFetch, timeout = 100) {
  const saved = session();
  const storage = { getItem: () => JSON.stringify(saved), setItem() {}, removeItem() {} };
  const accounts = new Accounts("https://babble.test", storage, request);
  const client = new ReactionClient(accounts, publicFetch ?? request, timeout);
  return { accounts, client, source: client.capture(() => true) };
}

test("real Accounts transport adds bearer only to mine; no cookies, redirects or cache on any endpoint", async () => {
  const calls = [];
  const request = async (url, init) => { calls.push({ url, init }); return Response.json(url.pathname.endsWith("mine") ? state() : summary()); };
  const { source } = transportHarness(request);
  const signal = new AbortController().signal;
  await source.summary(object, signal); await source.state(object, signal);
  await source.update(object, { value: value({ stance: "support", certainty: 100 }), expected_revision: 0, idempotency_key: "k" }, signal);
  for (const { init } of calls) { assert.equal(init.credentials, "omit"); assert.equal(init.redirect, "error"); assert.equal(init.cache, "no-store"); }
  assert.equal(new Headers(calls[0].init.headers).get("authorization"), null);
  assert.equal(new Headers(calls[1].init.headers).get("authorization"), `Bearer ${session().token}`);
  assert.equal(calls[2].init.method, "PUT"); assert.equal(JSON.parse(calls[2].init.body).author_id, undefined);
});

test("captured session rejects after relogin before sending and during body decoding", async () => {
  let sends = 0, body;
  const h = transportHarness(async (url) => {
    if (url.pathname === "/auth/session") return new Response(null, { status: 204 });
    if (url.pathname === "/auth/login") return Response.json(session("b"));
    sends++; return new Response(new ReadableStream({ start(controller) { body = controller; } }));
  });
  const signal = new AbortController().signal;
  const pending = h.source.state(object, signal);
  await settle(); await h.accounts.logout(); await h.accounts.login(actor, "password");
  body.enqueue(new TextEncoder().encode(JSON.stringify(state()))); body.close();
  await assert.rejects(pending, /Sign in again/);
  await assert.rejects(h.source.state(object, signal), /Sign in again/); assert.equal(sends, 1);
});

test("transport has bounded body bytes, bounded stalled bodies/fetches, external cancellation and malformed-response rejection", async () => {
  for (const body of [JSON.stringify(state({ author_id: other })), "{}", "not json", " ".repeat(16_385)]) {
    const h = transportHarness(async () => new Response(body));
    await assert.rejects(h.source.state(object, new AbortController().signal));
  }
  for (const request of [() => new Promise(() => {}), async () => new Response(new ReadableStream({ start() {} }))]) {
    const h = transportHarness(request, request, 15);
    await assert.rejects(h.source.state(object, new AbortController().signal), /timed out/);
  }
  const h = transportHarness(() => new Promise(() => {}));
  const abort = new AbortController(); const pending = h.source.summary(object, abort.signal); abort.abort(new Error("swiped"));
  await assert.rejects(pending, /swiped/);
});

test("explicit stale session selection fails closed and never activates an object", async () => {
  const h = harness();
  await assert.rejects(h.control.select(object, session("stale")), /Sign in again/);
  assert.equal(h.summaries.length, 0); assert.equal(h.control.view.object, null); h.control.dispose();
});

test("same object/session selections preserve pending read, draft confirmation, generation and request count", async () => {
  const h = harness(); const start = h.control.select(object); const generation = h.control.view.generation;
  await h.control.select(object); assert.equal(h.summaries.length, 1); assert.equal(h.summaries[0].signal.aborted, false);
  await read(h); await start;
  await h.control.publish(value({ stance: "uncertain", certainty: 0 }));
  await h.control.select(object); assert.ok(h.control.view.confirmation);
  assert.equal(h.control.view.generation, generation); assert.equal(h.summaries.length, 1); h.control.dispose();
});

test("partial read failure preserves the other authoritative result and refuses mutation without private state", async () => {
  const h = harness(); const start = h.control.select(object);
  h.summaries[0].resolve(summary({ participants: 1, likes: 1 })); await settle();
  h.reads[0].reject(new Error("mine unavailable")); await start;
  assert.equal(h.control.view.summary.likes, 1); assert.equal(h.control.view.state, null);
  await h.control.publish(value({ appreciation: "like" })); assert.equal(h.writes.length, 0);
  const refresh = h.control.refresh(); h.summaries.at(-1).reject(new Error("summary unavailable")); await settle();
  h.reads.at(-1).resolve(state({ revision: 3, value: value({ stance: "oppose", certainty: 0 }) })); await refresh;
  assert.equal(h.control.view.state.value.certainty, 0); assert.equal(h.control.view.summary, null);
  assert.equal(h.control.view.needsRefresh, true); h.control.dispose();
});

test("conflict plus failed refresh requires reading before a new intent, never automatic replay", async () => {
  const h = harness(); await ready(h); const { operation } = await publish(h);
  h.writes[0].reject(new ReactionError(409)); await settle(); h.summaries.at(-1).reject(new Error("offline")); await settle();
  h.reads.at(-1).reject(new Error("offline")); await operation;
  assert.equal(h.control.view.retry, false); assert.equal(h.control.view.state, null);
  assert.match(h.control.view.message, /changed elsewhere/);
  await h.control.retry(); await h.control.publish(value()); assert.equal(h.writes.length, 1); h.control.dispose();
});

test("logout during real authenticated PUT cannot send a follow-up GET under the new account", async () => {
  const pending = deferred(); const paths = [];
  const io = async (url, init) => {
    paths.push([url.pathname, init.method]);
    if (url.pathname === "/auth/session") return new Response(null, { status: 204 });
    if (url.pathname === "/auth/login") return Response.json(session("b", other));
    if (init.method === "PUT") return pending.promise;
    return Response.json(url.pathname.endsWith("mine") ? state() : summary());
  };
  const h = transportHarness(io);
  const control = new Reactions(h.accounts, () => {}, h.client);
  await control.select(object); await control.publish(value({ appreciation: "like" })); const write = control.confirm();
  await h.accounts.logout(); await h.accounts.login(other, "password"); await settle();
  const count = paths.length;
  pending.resolve(Response.json(state({ revision: 1, value: value({ appreciation: "like" }) }))); await write;
  assert.equal(paths.length, count); assert.equal(control.view.actor, other); assert.equal(control.view.retry, false); control.dispose();
});

test("pre-aborted operation never invokes transport and non-JSON HTTP errors retain status", async () => {
  let calls = 0;
  const h = transportHarness(async () => { calls++; return new Response("conflict", { status: 409 }); });
  const abort = new AbortController(); abort.abort(new Error("cancelled"));
  await assert.rejects(h.source.summary(object, abort.signal), /cancelled/); assert.equal(calls, 0);
  await assert.rejects(h.source.state(object, new AbortController().signal), (error) => error instanceof ReactionError && error.status === 409);
});

test("first action withdrawal also explains public signed history before committing", async () => {
  const h = harness(); const start = h.control.select(object);
  await read(h, state({ revision: 4, value: value({ appreciation: "like" }) })); await start;
  await h.control.publish(value());
  assert.ok(h.control.view.confirmation); assert.equal(h.writes.length, 0);
  const operation = h.control.confirm(); h.writes[0].resolve(state({ revision: 5 })); await settle();
  await read(h, state({ revision: 5 })); await operation; h.control.dispose();
});
