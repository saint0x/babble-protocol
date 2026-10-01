import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, readdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = new URL("../../", import.meta.url);
const tests = ["browser-invocations", "host-actions", "surface-invocations", "browser-action-prompt", "permissions"]
  .map(name => `frontend/tests/${name}.test.mjs`);
const files = [...tests,
  ...["browser-invocations", "host-actions", "surface-invocations", "browser-action-prompt", "permissions", "invocations", "protocol"]
    .map(name => `frontend/src/app/${name}.ts`),
  "frontend/tests/browser-invocations-evidence.mjs", "frontend/tests/browser-invocations-host.fozzy.json",
  "frontend/tests/browser-invocations.fozzy.json", "backend/crates/api/browser-invocation-contract.md",
  "frontend/node_modules/typescript/package.json", "frontend/node_modules/typescript/lib/typescript.js",
  "sdk/package.json", "sdk/src/generated/protocol.ts",
  ...readdirSync(new URL("sdk/dist/", root), { recursive: true }).filter(path => path.endsWith(".js")).map(path => `sdk/dist/${path}`),
].sort();
const hashes = () => Object.fromEntries(files.map(path => [path, createHash("sha256").update(readFileSync(new URL(path, root))).digest("hex")]));
const before = hashes();
console.log(JSON.stringify({ phase: "source-freeze-before", node: process.version, hashes: before }));
const result = spawnSync(process.execPath, ["--test", ...tests], { cwd: fileURLToPath(root), stdio: "inherit", timeout: 120_000 });
const after = hashes();
console.log(JSON.stringify({ phase: "source-freeze-after", hashes: after }));
assert.deepEqual(after, before, "Validation sources changed during the client suite; rerun after coordination");
if (result.error) throw result.error;
assert.equal(result.signal, null, "Client tests were interrupted");
process.exitCode = result.status ?? 1;
