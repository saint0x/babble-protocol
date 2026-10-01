import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

function load(name, require = () => ({}), extra = {}) {
  const context = { exports: {}, require, EventTarget, Event, URL, Request, Response, Headers, AbortController, AbortSignal,
    TextDecoder, TextEncoder, setTimeout, clearTimeout, fetch, ...extra };
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, context);
  return context.exports;
}
const accountModule = load("accounts");
const { Accounts, AccountError, newPasswordError } = accountModule;
const securityModule = load("account-security", () => accountModule);
const { AccountSecurity, AccountSecurityClient, parseAccountSessions } = securityModule;
const origin = "https://babel.test";
const tokenKey = `babel.session.v1:${origin}`;
const login = (token = "a") => ({ identity: { id: `id_${token.repeat(64)}`, handle: "Reader" }, token: token.repeat(64), expires_at: "2099-01-01T00:00:00Z" });
const row = (id = "a", current = true) => ({ id: `account_${id.repeat(64)}`, current, created_at: "2026-09-30T00:00:00Z", expires_at: "2099-01-01T00:00:00Z" });
const json = (data, status = 200) => new Response(JSON.stringify(data), { status, headers: { "content-type": "application/json" } });
const noContent = () => new Response(null, { status: 204 });
const deferred = () => { let resolve, reject; const promise = new Promise((yes, no) => { resolve = yes; reject = no; }); return { promise, resolve, reject }; };
const tick = () => new Promise(resolve => setImmediate(resolve));
function harness(request, timeout = 1000, signedIn = true) {
  const values = new Map(signedIn ? [[tokenKey, JSON.stringify(login())]] : []);
  const storage = { getItem: key => values.get(key) ?? null, setItem: (key, value) => values.set(key, value), removeItem: key => values.delete(key) };
  const accounts = new Accounts(origin, storage, request, timeout);
  const client = new AccountSecurityClient(accounts, timeout);
  const controller = new AccountSecurity(accounts, client);
  return { accounts, client, controller, values };
}

test("new passwords count Unicode scalars and UTF-8 bytes without normalizing", () => {
  for (const valid of ["a".repeat(15), " ".repeat(15), "😀".repeat(15), "é".repeat(512), "a".repeat(1024)]) assert.equal(newPasswordError(valid), null);
  for (const invalid of ["a".repeat(14), "😀".repeat(14), "é".repeat(513), "😀".repeat(257), "x".repeat(15) + "\ud800"]) assert.ok(newPasswordError(invalid));
});

test("session boundaries reject duplicate IDs, secret fields, malformed dates and ambiguous current sessions", () => {
  assert.equal(parseAccountSessions({ sessions: [] }).length, 0);
  assert.equal(parseAccountSessions({ sessions: [{ ...row(), created_at: null }] })[0].created_at, null);
  for (const value of [null, {}, { sessions: Array(17).fill(row()) }, { sessions: [row(), row()] },
    { sessions: [row("a", false)] }, { sessions: [row(), row("b")] }, { sessions: [row()], token: "secret" },
    ...[{ id: "account_BAD" }, { created_at: undefined }, { created_at: "invalid" }, { expires_at: "2026-01-01" },
      { created_at: "2100-01-01T00:00:00Z" }, { current: "true" }, { token: "secret" }].map(patch => ({ sessions: [{ ...row(), ...patch }] }))]) {
    assert.throws(() => parseAccountSessions(value));
  }
});

test("transport uses captured host credentials and exact routes without leaking passwords to storage", async () => {
  const calls = [];
  const h = harness(async (url, init) => { calls.push({ url, init }); return init.method === "GET" ? json({ sessions: [row()] }) : noContent(); });
  const session = h.accounts.current, signal = new AbortController().signal;
  await h.client.sessions(session, signal);
  await h.client.revoke(session, signal, row("b", false));
  await h.client.revoke(session, signal, "others");
  const next = "  a new passphrase  ";
  await h.client.password(session, signal, { current_password: " current password ", new_password: next });
  assert.deepEqual(calls.map(({ url, init }) => [url.pathname, init.method]), [["/auth/sessions", "GET"], [`/auth/sessions/${row("b").id}`, "DELETE"], ["/auth/sessions/revoke-others", "POST"], ["/auth/password", "POST"]]);
  for (const { init } of calls) {
    assert.equal(init.headers.get("authorization"), `Bearer ${session.token}`);
    assert.equal(init.cache, "no-store"); assert.equal(init.credentials, "omit"); assert.equal(init.redirect, "error");
  }
  assert.equal(calls[2].init.body, undefined);
  assert.deepEqual(JSON.parse(calls[3].init.body), { current_password: " current password ", new_password: next });
  assert.equal(h.accounts.current, null); assert.equal(h.values.size, 0);
});

test("password 403 preserves login; 401 invalidates it; 204 invalidates only the captured session", async () => {
  for (const status of [403, 401, 204]) {
    const h = harness(async () => status === 204 ? noContent() : json({ message: "not accepted" }, status));
    const pending = h.client.password(h.accounts.current, new AbortController().signal,
      { current_password: "old password", new_password: "new password valid" });
    if (status !== 204) await assert.rejects(pending, error => error.status === status); else await pending;
    assert.equal(Boolean(h.accounts.current), status === 403);
    h.controller.dispose();
  }
});

test("late successful mutation cannot clear a replacement login", async () => {
  const gate = deferred();
  const h = harness(async (url) => url.pathname === "/auth/password" ? gate.promise : json(login("b")));
  const original = h.accounts.current;
  const pending = h.client.password(original, new AbortController().signal, { current_password: "old", new_password: "new password valid" });
  h.accounts.forgetSession(original);
  await h.accounts.login(login("b").identity.id, "legacy-password");
  gate.resolve(noContent());
  await assert.rejects(pending);
  assert.equal(h.accounts.current.token, login("b").token);
  assert.equal(h.accounts.forgetSession(original), false);
});

test("uncooperative fetch and response bodies are bounded by the security deadline", async () => {
  for (const response of [new Promise(() => {}), Promise.resolve(new Response(new ReadableStream({ start() {} }))), Promise.resolve(json({ sessions: [], pad: "x".repeat(20000) }))]) {
    const h = harness(() => response, 15);
    await assert.rejects(h.client.sessions(h.accounts.current, new AbortController().signal));
    assert.ok(h.accounts.current); h.controller.dispose();
  }
});

test("closing aborts body reads and ignores late list responses", async () => {
  const gate = deferred();
  const h = harness(() => gate.promise);
  h.controller.open();
  assert.equal(h.controller.view.loading, true);
  h.controller.close();
  gate.resolve(json({ sessions: [row()] })); await tick();
  assert.equal(h.controller.view.sessions.length, 0);
  assert.equal(h.controller.view.loading, false);
});

test("revocation requires confirmation and cancellation is non-mutating", async () => {
  const calls = [];
  let sessions = [row(), row("b", false)];
  const h = harness(async (url, init) => {
    calls.push(init.method);
    if (init.method === "DELETE") { sessions = [row()]; return noContent(); }
    return json({ sessions });
  });
  h.controller.open(); await tick();
  h.controller.requestRevoke(row("b", false));
  assert.equal(h.controller.view.confirmation.id, row("b").id);
  h.controller.cancelRevoke(); await h.controller.confirmRevoke();
  assert.deepEqual(calls, ["GET"]);
  h.controller.requestRevoke(row("b", false)); await h.controller.confirmRevoke();
  assert.deepEqual(calls, ["GET", "DELETE", "GET"]);
  assert.equal(h.controller.view.sessions.length, 1);
  assert.equal(h.controller.view.sessionBusy, false);
});

test("lost revoke acknowledgements trigger authoritative refresh, not assumed success", async () => {
  let mutated = false;
  const h = harness(async (_url, init) => {
    if (init.method !== "GET") { mutated = true; throw new Error("network"); }
    return json({ sessions: mutated ? [row()] : [row(), row("b", false)] });
  });
  h.controller.open(); await tick();
  h.controller.requestRevoke("others"); await h.controller.confirmRevoke();
  assert.equal(h.controller.view.sessions.length, 1);
  assert.equal(h.controller.view.statusState, "error");
  assert.match(h.controller.view.status, /not confirmed/);
});

test("password uncertainty blocks automatic retry and permits explicit sign-in recovery", async () => {
  let writes = 0;
  const h = harness(async (_url, init) => {
    if (init.method === "GET") return json({ sessions: [row()] });
    writes++; throw new Error("acknowledgement lost");
  });
  h.controller.open(); await tick();
  await h.controller.changePassword("old password", "new valid password", "new valid password");
  assert.equal(h.controller.view.uncertainPassword, true);
  assert.match(h.controller.view.passwordStatus, /may have succeeded/);
  await h.controller.changePassword("old password", "new valid password", "new valid password");
  assert.equal(writes, 1); assert.ok(h.accounts.current);
  h.controller.recoverSignIn(); assert.equal(h.accounts.current, null);
});

test("password 403 preserves a retryable form; success and 401 publish outcomes after synchronous invalidation", async () => {
  for (const status of [403, 401, 204]) {
    const h = harness(async (_url, init) => init.method === "GET" ? json({ sessions: [row()] }) : status === 204 ? noContent() : json({}, status));
    const notices = []; h.controller.addEventListener("notice", event => notices.push(event.message));
    h.controller.open(); await tick();
    await h.controller.changePassword("old password", "new valid password", "new valid password");
    if (status === 403) {
      assert.ok(h.accounts.current); assert.equal(h.controller.view.passwordState, "error");
      assert.equal(h.controller.view.uncertainPassword, false); assert.equal(notices.length, 0);
    } else {
      assert.equal(h.accounts.current, null); assert.equal(notices.length, 1);
      assert.match(notices[0], status === 204 ? /Password changed.*Every session/ : /may have succeeded/);
    }
  }
});

test("password validation performs no network mutation and preserves exact secrets", async () => {
  let writes = 0;
  const h = harness(async () => { writes++; return json({ sessions: [row()] }); });
  h.controller.open(); await tick();
  for (const [current, next, confirmation] of [["", "long valid password", "long valid password"], ["old", "short", "short"], ["old", "long valid password", "mismatch"]]) {
    await h.controller.changePassword(current, next, confirmation);
    assert.equal(h.controller.view.passwordState, "error");
  }
  assert.equal(writes, 1);
  await h.controller.changePassword("long valid password", "long valid password", "long valid password");
  assert.match(h.controller.view.passwordStatus, /different from your current password/);
  assert.equal(writes, 1);
});

test("late password results and notices never replace a new account or closed view", async () => {
  for (const switchAccount of [false, true]) {
    const gate = deferred(), notices = [];
    const h = harness(async (url, init) => url.pathname === "/auth/login" ? json(login("b"))
      : init.method === "GET" ? json({ sessions: [row()] }) : gate.promise);
    h.controller.addEventListener("notice", event => notices.push(event.message));
    h.controller.open(); await tick();
    const pending = h.controller.changePassword("old password", "new valid password", "new valid password");
    if (switchAccount) {
      h.accounts.forgetSession(h.accounts.current); await h.accounts.login(login("b").identity.id, "password");
      notices.length = 0;
    } else h.controller.close();
    gate.resolve(noContent()); await pending;
    assert.equal(notices.length, 0);
    if (switchAccount) assert.equal(h.accounts.current.token, login("b").token);
  }
});

// A focused DOM port executes the real AccountPanel against the real Accounts event order.
class Control extends EventTarget {
  value = ""; disabled = false; hidden = false; textContent = ""; dataset = {}; open = false; validity = "";
  setCustomValidity(value) { this.validity = value; }
  reportValidity() { return !this.validity; }
  setAttribute() {}
  closest() { return null; }
  showModal() { this.open = true; }
  close() { this.open = false; this.dispatchEvent(new Event("close")); }
}
function panelHarness(request) {
  const h = harness(request, 1000, false), controls = new Map();
  const selectors = ["dialog", "security", "close", "password", "form", "logout", "details", "handle", "id", "expiry", "login-label", "handle-label", "login", "register", "submit", "status"];
  for (const selector of selectors) controls.set(`[data-account-${selector}]`, new Control());
  const modes = ["login", "register"].map(mode => { const control = new Control(); control.dataset.accountMode = mode; return control; });
  const dialog = controls.get("[data-account-dialog]");
  dialog.querySelector = selector => controls.get(selector);
  dialog.querySelectorAll = selector => selector === "[data-account-mode]" ? modes : [...modes, controls.get("[data-account-submit]"), controls.get("[data-account-logout]")];
  class SecurityPort {
    controller = { passwordSignoutPending: false };
    open() {} close() {} clearSecrets() {} requestCurrentLogout() { this.confirmation = true; }
  }
  const { AccountPanel } = load("account-panel", path => path === "./accounts" ? accountModule
    : path === "./account-security" ? securityModule : { AccountSecurityView: SecurityPort },
  { document: { querySelector: () => dialog } });
  const panel = new AccountPanel(h.accounts, () => {});
  return { ...h, panel, modes, controls, dialog, get: name => controls.get(`[data-account-${name}]`) };
}

test("actual AccountPanel preserves successful registration and login statuses across synchronous account changes", async () => {
  for (const action of ["register", "login"]) {
    const h = panelHarness(async () => json(login()));
    h.panel.open();
    h.modes[action === "register" ? 1 : 0].dispatchEvent(new Event("click"));
    h.get("register").value = "Reader"; h.get("login").value = login().identity.id;
    h.get("password").value = "valid registration passphrase";
    h.get("form").dispatchEvent(new Event("submit", { cancelable: true })); await tick();
    assert.equal(h.get("status").textContent, action === "register" ? "Account created" : "Signed in");
    assert.equal(h.get("submit").disabled, false);
    assert.equal(h.get("password").value, "");
    assert.equal(h.get("form").hidden, true);
    assert.equal(h.get("details").hidden, false);
  }
});

test("actual AccountPanel clears secrets on close and old failures cannot clear a newly typed password", async () => {
  const gate = deferred();
  const h = panelHarness(() => gate.promise);
  h.panel.open(); h.get("login").value = login().identity.id; h.get("password").value = "old secret";
  h.get("form").dispatchEvent(new Event("submit", { cancelable: true }));
  h.dialog.close(); assert.equal(h.get("password").value, "");
  h.panel.open(); h.get("password").value = "new input";
  gate.resolve(json({}, 403)); await tick();
  assert.equal(h.get("password").value, "new input");
  assert.notEqual(h.get("status").textContent, "Could not sign in");
});

test("same-handle replacement registration cannot be reported as the panel's own success", async () => {
  const first = deferred(); let requests = 0;
  const h = panelHarness(async () => ++requests === 1 ? first.promise : json(login("b")));
  h.panel.open(); h.modes[1].dispatchEvent(new Event("click"));
  h.get("register").value = "Reader"; h.get("password").value = "a valid initial passphrase";
  h.get("form").dispatchEvent(new Event("submit", { cancelable: true }));
  const replacement = await h.accounts.register("Reader", "a valid replacement passphrase");
  assert.equal(replacement.token, login("b").token);
  assert.equal(h.get("status").textContent, "");
  first.resolve(json(login("a"))); await tick();
  assert.equal(h.accounts.current.token, login("b").token);
  assert.equal(h.get("status").textContent, "");
  assert.equal(h.get("submit").disabled, false);
});
