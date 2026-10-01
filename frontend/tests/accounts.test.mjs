import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const code = ts.transpileModule(readFileSync(new URL("../src/app/accounts.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;
const exports = {};
vm.runInNewContext(code, { exports, EventTarget, Event, URL, Request, Headers, Date, fetch, AbortSignal, TextDecoder, TextEncoder });
const { Accounts, AccountError } = exports;
const origin = "https://babble.test";
const key = `babble.session.v1:${origin}`;
const identity = { id: "id_test", handle: "reader" };
const session = (token = "a".repeat(64)) => ({ identity, token, expires_at: new Date(Date.now() + 3600000).toISOString() });
const json = (body, status = 200) => new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
function storage() {
  const values = new Map();
  return { getItem: (key) => values.get(key) ?? null, setItem: (key, value) => values.set(key, value), removeItem: (key) => values.delete(key), values };
}

test("registration creates a real session and never persists the password", async () => {
  const store = storage();
  let request;
  const accounts = new Accounts(origin, store, async (url, init) => { request = { url: url.href, ...init }; return json(session()); });
  let changes = 0;
  accounts.addEventListener("change", () => changes++);
  await accounts.register("reader", "test-only-password");
  assert.equal(request.url, `${origin}/auth/register`);
  assert.deepEqual(JSON.parse(request.body), { handle: "reader", kind: "Person", password: "test-only-password" });
  assert.equal(request.redirect, "error");
  assert.equal(request.credentials, "omit");
  assert.equal(accounts.current.identity.id, identity.id);
  assert.equal(changes, 1);
  assert.ok(!store.getItem(key).includes("test-only-password"));
  await assert.rejects(accounts.login("another", "password"), /Sign out before switching/);
});

test("restoring metadata for the same session does not invalidate an in-flight private read", async () => {
  const store = storage(); store.setItem(key, JSON.stringify(session()));
  let finish;
  const pending = new Promise(resolve => { finish = resolve; });
  const refreshed = { identity: { ...identity, handle: "updated-reader" }, expires_at: session().expires_at };
  const accounts = new Accounts(origin, store, async url => url.pathname === "/auth/session" ? json(refreshed) : pending);
  let changes = 0; accounts.addEventListener("change", () => changes++);
  const read = accounts.authenticatedFetch(new URL("/auth/sessions", origin));
  await accounts.restore();
  finish(json({ sessions: ["current"] }));
  assert.deepEqual(await (await read).json(), { sessions: ["current"] });
  assert.equal(accounts.current.identity.handle, "updated-reader");
  assert.equal(changes, 1);
});

test("logout and login still invalidate old private reads even if a server reuses the token", async () => {
  const store = storage(); store.setItem(key, JSON.stringify(session()));
  let finish;
  const pending = new Promise(resolve => { finish = resolve; });
  const accounts = new Accounts(origin, store, async url => url.pathname === "/auth/session"
    ? new Response(null, { status: 204 }) : url.pathname === "/auth/login" ? json(session()) : pending);
  const read = accounts.authenticatedFetch(new URL("/private", origin));
  await accounts.logout(); await accounts.login(identity.id, "test-only-password");
  finish(json({ private: true }));
  await assert.rejects(read, /account changed/);
});

test("local preferences and history are isolated by origin and account, not session token", async () => {
  const store = storage();
  const accounts = new Accounts(origin, store, async (url) => url.pathname === "/auth/session"
    ? new Response(null, { status: 204 }) : json(session()));
  const guest = accounts.localDataKey("seen");
  await accounts.login(identity.id, "test-only-password");
  const owner = accounts.localDataKey("seen");
  assert.notEqual(guest, owner);
  assert.notEqual(accounts.localDataKey("preferences"), owner);
  await accounts.logout();
  assert.equal(accounts.localDataKey("seen"), guest);
  await accounts.login(identity.id, "test-only-password");
  assert.equal(accounts.localDataKey("seen"), owner);
  const otherStore = storage();
  otherStore.setItem(key, JSON.stringify({ ...session(), identity: { id: "id_other", handle: "reader" } }));
  assert.notEqual(new Accounts(origin, otherStore).localDataKey("seen"), owner);
  const otherOrigin = "https://other.test";
  otherStore.setItem(`babble.session.v1:${otherOrigin}`, JSON.stringify(session()));
  assert.notEqual(new Accounts(otherOrigin, otherStore).localDataKey("seen"), owner);
});

test("login uses the canonical identity ID, not an unverified stored author", async () => {
  const store = storage();
  store.setItem("babble.frontend.author.v1", JSON.stringify({ identityId: "forged", handle: "reader" }));
  const accounts = new Accounts(origin, store, async (url, init) => {
    assert.equal(url.pathname, "/auth/login");
    assert.equal(JSON.parse(init.body).identity_id, identity.id);
    return json(session());
  });
  assert.equal(accounts.current, null);
  await accounts.login(identity.id, "password");
  assert.equal(accounts.current.identity.id, identity.id);
});

test("restoration validates server ownership and keeps origin storage separate", async () => {
  const store = storage();
  store.setItem(key, JSON.stringify(session()));
  const accounts = new Accounts(origin, store, async (_url, init) => {
    assert.equal(init.headers.get("authorization"), `Bearer ${"a".repeat(64)}`);
    return json({ identity, expires_at: session().expires_at });
  });
  await accounts.restore();
  assert.equal(accounts.current.identity.id, identity.id);
  assert.equal(new Accounts("https://other.test", store).current, null);
});

test("expired, malformed, or disabled storage cannot authorize requests", async () => {
  for (const stored of ["{", JSON.stringify({ ...session(), expires_at: "invalid" }), JSON.stringify({ ...session(), expires_at: "2000-01-01" })]) {
    const store = storage(); store.setItem(key, stored);
    assert.equal(new Accounts(origin, store).current, null);
  }
  const store = { getItem() { throw new Error("denied"); }, setItem() { throw new Error("denied"); }, removeItem() { throw new Error("denied"); } };
  const accounts = new Accounts(origin, store, async () => json(session()));
  await accounts.register("reader", "test-only-password");
  assert.equal(accounts.current.identity.id, identity.id);
});

test("host transport rejects foreign origins, overrides injected auth and refuses redirects", async () => {
  const store = storage(); store.setItem(key, JSON.stringify(session()));
  let calls = 0;
  const accounts = new Accounts(origin, store, async (_url, init) => {
    calls++;
    assert.equal(init.headers.get("authorization"), `Bearer ${"a".repeat(64)}`);
    assert.equal(init.headers.get("content-type"), "application/json");
    assert.equal(init.redirect, "error");
    assert.equal(init.credentials, "omit");
    return json({});
  });
  await accounts.fetch(`${origin}/rpc`, { headers: { authorization: "Bearer malicious", "content-type": "application/json" } });
  await assert.rejects(accounts.fetch("https://attacker.test/rpc"), /another origin/);
  await assert.rejects(accounts.fetch("https://user:secret@babble.test/rpc"), /another origin/);
  assert.equal(calls, 1);
});

test("a revoked session is cleared on 401, but network/503 failures preserve it", async () => {
  const store = storage(); store.setItem(key, JSON.stringify(session()));
  let status = 503;
  const accounts = new Accounts(origin, store, async () => json({ message: "server error" }, status));
  await assert.rejects(accounts.fetch(`${origin}/rpc`), (error) => error instanceof AccountError && error.status === 503);
  assert.ok(accounts.current);
  status = 401;
  await assert.rejects(accounts.fetch(`${origin}/rpc`), (error) => error.status === 401);
  assert.equal(accounts.current, null);
  assert.equal(store.getItem(key), null);
});

test("logout revokes remotely before clearing state; failure remains actionable", async () => {
  const store = storage(); store.setItem(key, JSON.stringify(session()));
  let failed = true;
  const accounts = new Accounts(origin, store, async (url, init) => {
    assert.equal(url.pathname, "/auth/session"); assert.equal(init.method, "DELETE");
    return failed ? json({}, 503) : new Response(null, { status: 204 });
  });
  await assert.rejects(accounts.logout());
  assert.ok(accounts.current);
  failed = false;
  await accounts.logout();
  assert.equal(accounts.current, null);
  assert.equal(store.getItem(key), null);
});

test("an already-revoked session can be signed out", async () => {
  const store = storage(); store.setItem(key, JSON.stringify(session()));
  const accounts = new Accounts(origin, store, async () => json({}, 401));
  await accounts.logout();
  assert.equal(accounts.current, null);
});

test("restoration cannot replace an account with a mismatched server identity", async () => {
  const store = storage(); store.setItem(key, JSON.stringify(session()));
  const accounts = new Accounts(origin, store, async () => json({ identity: { ...identity, id: "another" }, expires_at: session().expires_at }));
  await assert.rejects(accounts.restore(), /Invalid account/);
  assert.equal(accounts.current.identity.id, identity.id);
});

test("a late 401 from an old session does not clear the replacement session", async () => {
  const store = storage(); store.setItem(key, JSON.stringify(session()));
  let finish;
  const pending = new Promise((resolve) => { finish = resolve; });
  const accounts = new Accounts(origin, store, async (url, init) => {
    if (new URL(url).pathname === "/rpc") return pending;
    if (init.method === "DELETE") return new Response(null, { status: 204 });
    return json(session("b".repeat(64)));
  });
  const request = accounts.fetch(`${origin}/rpc`);
  await accounts.logout();
  await accounts.login(identity.id, "password");
  finish(json({}, 401));
  await assert.rejects(request);
  assert.equal(accounts.current.token, "b".repeat(64));
});

test("hung authentication requests time out without inventing a session", async () => {
  const keepAlive = setTimeout(() => {}, 1000);
  try {
    const accounts = new Accounts(origin, null, (_url, init) => new Promise((_resolve, reject) => {
      init.signal.addEventListener("abort", () => reject(init.signal.reason), { once: true });
    }), 5);
    await assert.rejects(accounts.login(identity.id, "password"), (error) => error.name === "TimeoutError");
    assert.equal(accounts.current, null);
  } finally { clearTimeout(keepAlive); }
});

test("proxy error pages produce actionable account errors", async () => {
  const accounts = new Accounts(origin, null, async () => new Response("<html>Unavailable</html>", { status: 503 }));
  await assert.rejects(accounts.login(identity.id, "password"), (error) => error.status === 503 && /invalid response/.test(error.message));
  assert.equal(accounts.current, null);
});
