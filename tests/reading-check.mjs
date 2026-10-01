import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";
import { verifyConversationReading } from "./conversation-browser.mjs";
import { verifyCardPresentation } from "./card-style-browser.mjs";

const addr = process.env.AEGIS_SERVER_ADDR ?? "127.0.0.1:7879";
const url = new URL(process.env.BABEL_READING_URL ?? "http://127.0.0.1:4321/");
execFileSync(process.execPath, ["--test", "frontend/tests/conversations.test.mjs", "frontend/tests/reading.test.mjs"], {
  stdio: "inherit", timeout: 30_000,
});
const navigation = await fetch(`http://${addr}/navigate`, {
  method: "POST", headers: { "content-type": "application/json" },
  body: JSON.stringify({ url: url.href }), signal: AbortSignal.timeout(15_000),
});
assert.equal(navigation.ok, true, "Aegis navigation must succeed");

async function execute(commands) {
  const response = await fetch(`http://${addr}/execute`, {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ commands }), signal: AbortSignal.timeout(10_000),
  });
  assert.equal(response.ok, true);
  return response.json();
}

async function waitFor(code, predicate) {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    const result = (await execute([{ type: "eval", code }])).results?.[0];
    if (result?.ok && result.value !== undefined && predicate(result.value)) return result.value;
    await delay(100);
  }
  throw new Error("Aegis reading check timed out");
}

await waitFor(`({ origin: location.origin, ready: document.readyState })`,
  (state) => state.origin === url.origin && state.ready === "complete");
await verifyConversationReading(execute, waitFor);
await verifyCardPresentation(execute, waitFor);
console.log("Production conversation controller: anchored refresh, nested parent, Back, and narrow-column layout PASS");
