import assert from "node:assert/strict";
import { once } from "node:events";
import { createServer, request } from "node:http";
import test from "node:test";
import { startHttpObserver } from "./http-observer.mjs";

async function fixture(t, handler, options = {}) {
  const upstream = createServer(handler);
  upstream.listen(0, "127.0.0.1");
  await once(upstream, "listening");
  const observer = await startHttpObserver({ port: 0, upstreamPort: upstream.address().port,
    capture: path => path === "/rpc", ...options });
  t.after(async () => {
    await observer.close();
    upstream.closeAllConnections();
    await new Promise(resolve => upstream.close(resolve));
  });
  return { observer, upstream, origin: `http://127.0.0.1:${observer.port}` };
}

test("HTTP observation preserves actual body, auth, headers, status, and response without recording secrets", async t => {
  const bytes = '{ "text": "literal \\n ☃", "value": 0 }';
  const result = '{"real":true,"value":"☃"}';
  const { observer, origin } = await fixture(t, async (req, res) => {
    const chunks = []; for await (const chunk of req) chunks.push(chunk);
    assert.equal(Buffer.concat(chunks).toString(), bytes);
    assert.equal(req.headers.authorization, "Bearer disposable-secret");
    assert.equal(req.headers["x-test"], "unchanged");
    assert.equal(req.url, "/rpc?literal=1");
    res.writeHead(201, { "content-type": "application/json", "x-origin": "upstream" });
    res.write(result.slice(0, 9)); res.end(result.slice(9));
  });
  const response = await fetch(`${origin}/rpc?literal=1`, { method: "POST", body: bytes,
    headers: { authorization: "Bearer disposable-secret", "x-test": "unchanged" } });
  assert.equal(response.status, 201);
  assert.equal(response.headers.get("x-origin"), "upstream");
  assert.equal(await response.text(), result);
  const captured = observer.readRequests({ since: 0 });
  assert.deepEqual(captured, { cursor: 1, requests: [{ sequence: 1, url: `${origin}/rpc?literal=1`,
    method: "POST", requestBody: bytes, responseBody: result, status: 201 }] });
  assert.ok(!JSON.stringify(captured).includes("disposable-secret"));
  captured.requests[0].status = 500;
  assert.equal(observer.readRequests().requests[0].status, 201, "Snapshots must not mutate observations");
  assert.deepEqual(observer.readRequests({ since: 1 }), { cursor: 1, requests: [] });
});

test("uncaptured responses and redirects pass through; no auth body is retained", async t => {
  const { observer, origin } = await fixture(t, (_req, res) => {
    res.writeHead(302, { location: "/auth/session", "set-cookie": "fixture=1" });
    res.end("uncaptured credential response");
  });
  const response = await fetch(`${origin}/auth/session`, { redirect: "manual" });
  assert.equal(response.status, 302);
  assert.equal(response.headers.get("location"), "/auth/session");
  assert.equal(await response.text(), "uncaptured credential response");
  assert.deepEqual(observer.readRequests(), { cursor: 0, requests: [] });
});

test("request-arrival cursors can reread response completion and reject invalid cursors", async t => {
  let finish;
  const reached = new Promise(resolve => { finish = resolve; });
  let reply;
  const { observer, origin } = await fixture(t, (req, res) => {
    req.resume(); req.on("end", () => { reply = () => res.end("complete"); finish(); });
  });
  const pending = fetch(`${origin}/rpc`, { method: "POST", body: "{}" });
  await reached;
  assert.equal(observer.readRequests().cursor, 1);
  assert.equal(observer.readRequests().requests[0].responseBody, null);
  reply(); assert.equal(await (await pending).text(), "complete");
  assert.equal(observer.readRequests({ since: 0 }).requests[0].responseBody, "complete");
  for (const since of [-1, 0.1, 2, NaN]) assert.throws(() => observer.readRequests({ since }), /cursor/);
});

test("capture overflow fails evidence without replacing the real server result", async t => {
  const { observer, origin } = await fixture(t, (req, res) => { req.resume(); req.on("end", () => res.end("real long response")); }, { maxBytes: 4 });
  assert.equal(await (await fetch(`${origin}/rpc`, { method: "POST", body: "{}" })).text(), "real long response");
  assert.throws(() => observer.readRequests(), /byte budget/);
});

test("client cancellation and observer shutdown close actual upstream requests", async t => {
  let arrived, ended;
  const reached = new Promise(resolve => { arrived = resolve; });
  const upstreamClosed = new Promise(resolve => { ended = resolve; });
  const { observer, origin } = await fixture(t, (req, res) => {
    req.resume(); res.on("close", ended); arrived();
  });
  const controller = new AbortController();
  const pending = fetch(`${origin}/rpc`, { signal: controller.signal }).catch(error => error);
  await reached; controller.abort();
  assert.equal((await pending).name, "AbortError");
  await upstreamClosed;
  await Promise.all([observer.close(), observer.close()]);
  await assert.rejects(fetch(origin));
});

test("an upstream network failure remains a network failure", async t => {
  const { observer, origin } = await fixture(t, (req, _res) => req.socket.destroy());
  await assert.rejects(fetch(`${origin}/rpc`));
  assert.match(observer.readRequests().requests[0].error, /socket hang up/);
});

test("an interrupted request body does not crash or leave an upstream connection", async t => {
  let arrived;
  const reached = new Promise(resolve => { arrived = resolve; });
  let ended;
  const closed = new Promise(resolve => { ended = resolve; });
  const { observer, origin } = await fixture(t, (req, res) => { arrived(); req.on("error", () => {}); res.on("close", ended); });
  const client = request(`${origin}/rpc`, { method: "POST", headers: { "content-length": "100" } });
  client.on("error", () => {});
  client.write("partial");
  await reached; client.destroy(); await closed;
  assert.ok(observer.readRequests().requests[0].error);
});
