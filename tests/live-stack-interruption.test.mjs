import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { createServer } from "node:net";
import test from "node:test";

test("terminating the evidence wrapper during the real production build cleans API, observer and store", { timeout: 120_000 }, async () => {
  const child = spawn(process.execPath, ["tests/live-stack-evidence.mjs", "moderation"], {
    env: { ...process.env, BABEL_MODERATION_SOURCE_FROZEN: "1", BABEL_LIVE_FRONTEND_MODE: "production" },
    stdio: ["ignore", "pipe", "pipe"],
  });
  let output = "", storeRoot, evidence, interrupted = false;
  const receive = data => {
    output += data;
    const path = output.match(/\/var\/folders\/[^\s]+?\/babel-live-stack-[^/\s]+/);
    const manifest = output.split("\n").find(line => line.startsWith('{"phase":"source-freeze-before"'));
    if (manifest) evidence = JSON.parse(manifest).path;
    if (path && !interrupted) {
      storeRoot = path[0];
      interrupted = true;
      child.kill("SIGTERM");
    }
  };
  child.stdout.on("data", receive); child.stderr.on("data", receive);
  const timer = setTimeout(() => child.kill("SIGTERM"), 90_000);
  let outcome;
  try {
    outcome = await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("close", (code, signal) => resolve({ code, signal }));
    });
  } finally { clearTimeout(timer); }
  assert.ok(interrupted && storeRoot && evidence, output.slice(-12_000));
  assert.deepEqual(outcome, { code: 143, signal: null }, output.slice(-12_000));
  assert.equal(existsSync(storeRoot), false, "Disposable store must be removed before interrupted exit");
  const check = JSON.parse(readFileSync(`${evidence}/source-check.json`, "utf8"));
  assert.deepEqual(check, { stable: true, mode: "moderation", frontendMode: "production", changed: [], exitCode: 143 });
  for (const port of [18787, 18788, 18789, 14329]) {
    await new Promise((resolve, reject) => {
      const server = createServer(); server.once("error", reject);
      server.listen(port, "127.0.0.1", () => server.close(error => error ? reject(error) : resolve()));
    });
  }
});
