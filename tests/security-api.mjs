import assert from "node:assert/strict";

export async function prepareSecurityRestart(api) {
  const password = "Independent security verification passphrase 930!";
  const changedPassword = "Changed security verification passphrase 930!";
  const request = async (method, path, token, body) => {
    const response = await fetch(api + path, {
      method, redirect: "error", signal: AbortSignal.timeout(15_000),
      headers: { ...(token ? { authorization: `Bearer ${token}` } : {}),
        ...(body ? { "content-type": "application/json" } : {}) },
      ...(body ? { body: JSON.stringify(body) } : {}),
    });
    assert.equal(response.headers.get("cache-control"), "no-store");
    return { status: response.status, body: response.status === 204 ? null : await response.json() };
  };
  const account = await request("POST", "/auth/register", null,
    { handle: `security-${process.pid}`, kind: "Person", password });
  assert.equal(account.status, 200);
  const actor = account.body.identity.id;
  const first = account.body.token;
  const login = async (secret = password) => request("POST", "/auth/login", null,
    { identity_id: actor, password: secret });
  const second = await login();
  assert.equal(second.status, 200);
  const initial = await request("GET", "/auth/sessions", first);
  assert.equal(initial.status, 200);
  assert.equal(initial.body.sessions.length, 2);
  const current = initial.body.sessions.find(session => session.current);
  const other = initial.body.sessions.find(session => !session.current);
  assert.match(current.id, /^account_[a-f0-9]{64}$/);
  assert.match(other.id, /^account_[a-f0-9]{64}$/);
  for (const session of initial.body.sessions) {
    assert.deepEqual(Object.keys(session).sort(), ["created_at", "current", "expires_at", "id"]);
    assert.ok(Number.isFinite(Date.parse(session.created_at)));
    assert.ok(Date.parse(session.expires_at) > Date.parse(session.created_at));
    assert.notEqual(session.id, first);
  }
  assert.equal((await request("GET", "/auth/session", current.id)).status, 401);
  assert.equal((await request("DELETE", `/auth/sessions/${other.id}`, first)).status, 204);
  assert.equal((await request("DELETE", `/auth/sessions/${other.id}`, first)).status, 204);
  assert.equal((await request("GET", "/auth/session", second.body.token)).status, 401);
  assert.equal((await request("GET", "/auth/session", first)).status, 200);
  const third = await login();
  assert.equal(third.status, 200);
  assert.equal((await request("POST", "/auth/sessions/revoke-others", first)).status, 204);
  assert.equal((await request("GET", "/auth/session", third.body.token)).status, 401);
  assert.equal((await request("GET", "/auth/session", first)).status, 200);
  assert.equal((await request("POST", "/auth/password", first,
    { current_password: "Incorrect current password 930!", new_password: changedPassword })).status, 403);
  assert.equal((await request("GET", "/auth/session", first)).status, 200);
  const fourth = await login();
  assert.equal(fourth.status, 200);
  assert.equal((await request("POST", "/auth/password", first,
    { current_password: password, new_password: changedPassword })).status, 204);
  for (const token of [first, fourth.body.token]) {
    assert.equal((await request("GET", "/auth/session", token)).status, 401);
  }
  assert.equal((await login()).status, 401);
  const replacement = await login(changedPassword);
  assert.equal(replacement.status, 200);
  const beforeRestart = await request("GET", "/auth/sessions", replacement.body.token);
  assert.equal(beforeRestart.body.sessions.length, 1);
  return async () => {
    assert.equal((await request("GET", "/auth/session", first)).status, 401);
    assert.equal((await login()).status, 401);
    const sessions = await request("GET", "/auth/sessions", replacement.body.token);
    assert.equal(sessions.status, 200);
    assert.deepEqual(sessions.body, beforeRestart.body);
    assert.equal((await login(changedPassword)).status, 200);
    console.log("Account security restart PASS", JSON.stringify({ stableSessionIds: true,
      individualRevocation: true, revokeOthers: true, passwordChange: true, oldCredentialsRejected: true }));
  };
}
