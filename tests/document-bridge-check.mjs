import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { verifyDocumentBridge } from "./document-bridge-browser.mjs";
import { verifyBridgeCancellation } from "./bridge-cancellation-browser.mjs";

const addr = process.env.AEGIS_SERVER_ADDR ?? "127.0.0.1:7879";
assert.equal(addr, "127.0.0.1:7879", "standalone document bridge regression owns only Aegis port 7879");
const frontend = process.env.BABEL_FRONTEND_URL ?? "http://127.0.0.1:4321/";
const api = process.env.BABEL_API_URL ?? "http://127.0.0.1:8787";

async function post(path, body) {
  const response = await fetch(`${api}${path}`, {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify(body), signal: AbortSignal.timeout(15_000),
  });
  assert.equal(response.ok, true, `${path}: ${await response.clone().text()}`);
  return response.json();
}

async function execute(commands) {
  const response = await fetch(`http://${addr}/execute`, {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ commands }), signal: AbortSignal.timeout(15_000),
  });
  assert.equal(response.ok, true);
  const result = await response.json();
  assert.equal(result.results.length, commands.length, "Aegis must return every command result");
  for (const entry of result.results) assert.equal(entry.ok, true, JSON.stringify(entry));
  return result;
}

async function waitFor(code, predicate) {
  const deadline = Date.now() + 30_000;
  let value;
  while (Date.now() < deadline) {
    value = (await execute([{ type: "eval", code }])).results[0].value;
    if (value !== undefined && predicate(value)) return value;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  assert.fail(`Aegis predicate timed out: ${code}; last value ${JSON.stringify(value)}`);
}

const discovery = await post("/discovery/candidates", {
  anchors: [], search: "Surface", followed_objects: [], limit: 10, exploration_slots: 1, lens: null,
});
const object = discovery.discovery.objects.find(object => object.surfaces.some(surface => surface.target === "Web"));
assert.ok(object, "requires a public Web Surface to supply a valid runtime plan");
const prepared = await post("/runtime/surfaces/prepare", { object_id: object.id, role: "Feed" });
assert.equal(prepared.plan.admission, "ready");
execFileSync("aegis", ["--server-addr", addr, "navigate", frontend], { timeout: 15_000 });
await waitFor(`({ origin: location.origin, ready: document.readyState })`,
  value => value.origin === new URL(frontend).origin && value.ready === "complete");
await verifyDocumentBridge(execute, waitFor, prepared.plan);
await verifyBridgeCancellation(execute, waitFor);
console.log("Standalone document bridge PASS: secure fixture and both security negative controls on Aegis 7879");
