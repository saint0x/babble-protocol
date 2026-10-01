import assert from "node:assert/strict";
import { createHash, randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import { appendFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export async function runLiveStackEvidence({ focus = null, mode = focus ?? "full", artifactDirectory = "production-e2e", freezeVariable = "BABBLE_LIVE_SOURCE_FROZEN" } = {}) {
  assert.ok(focus === null || ["browser-invocations", "feed-diversity", "moderation"].includes(focus), "Unsupported production acceptance focus");
  const frontendMode = process.env.BABBLE_LIVE_FRONTEND_MODE ?? "production";
  assert.ok(["dev", "production"].includes(frontendMode), "expected dev or production frontend mode");
  assert.equal(process.env[freezeVariable], "1", `Parent source freeze required: ${freezeVariable}`);
  for (const [key, expected] of Object.entries({ BABBLE_LIVE_API_PORT: "18787", BABBLE_LIVE_GATEWAY_PORT: "18788",
    BABBLE_LIVE_FRONTEND_PORT: "14329", AEGIS_SERVER_ADDR: "127.0.0.1:17878" })) {
    assert.ok(process.env[key] === undefined || process.env[key] === expected, `reserved disposable endpoint required: ${key}`);
  }
  const excluded = new Set([".git", ".fozzy", "target", "artifacts", "node_modules", ".venv", "__pycache__"]);
  const extensions = /\.(rs|toml|lock|ts|tsx|js|mjs|cjs|json|py|css|astro|html|wasm)$/;
  function collect(directory) {
    return readdirSync(join(root, directory), { withFileTypes: true }).flatMap(entry => {
      if (excluded.has(entry.name)) return [];
      const path = join(directory, entry.name);
      return entry.isDirectory() ? collect(path) : extensions.test(path) ? [path] : [];
    });
  }
  const roots = ["backend/crates", "sdk/src", "sdk/scripts", "sdk/tests", "sdk/dist", "frontend/src", "frontend/tests", "fixtures", "tests", "algorithms"];
  const snapshot = () => Object.fromEntries([...new Set([
    ...roots.flatMap(collect), "backend/Cargo.toml", "backend/Cargo.lock", "sdk/package.json", "sdk/tsconfig.json",
    "frontend/package.json", "frontend/package-lock.json", "frontend/astro.config.mjs", "frontend/tsconfig.json",
  ])].sort().map(path => [path, createHash("sha256").update(readFileSync(join(root, path))).digest("hex")]));
  const run = join(root, "artifacts", artifactDirectory, `live-${mode}-${randomUUID()}`);
  mkdirSync(run, { recursive: true });
  const before = snapshot();
  writeFileSync(join(run, "sources-before.json"), JSON.stringify(before, null, 2) + "\n");
  console.log(JSON.stringify({ phase: "source-freeze-before", mode, frontendMode, files: Object.keys(before).length, path: relative(root, run) }));
  const env = { ...process.env, BABBLE_LIVE_SOURCE_FROZEN: "1", BABBLE_LIVE_FRONTEND_MODE: frontendMode, CARGO_INCREMENTAL: "0", CARGO_BUILD_JOBS: "2" };
  if (focus) {
    env.BABBLE_LIVE_FOCUS = focus;
    env[{ "browser-invocations": "BABBLE_BROWSER_INVOCATION_SOURCE_FROZEN", "feed-diversity": "BABBLE_FEED_DIVERSITY_SOURCE_FROZEN", moderation: "BABBLE_MODERATION_SOURCE_FROZEN" }[focus]] = "1";
  } else delete env.BABBLE_LIVE_FOCUS;
  let code = 1;
  try {
    const child = spawn(process.execPath, ["tests/live-stack.mjs"], { cwd: root, env, stdio: ["ignore", "pipe", "pipe", "ipc"] });
    for (const [stream, output] of [[child.stdout, process.stdout], [child.stderr, process.stderr]]) {
      stream.on("data", data => { appendFileSync(join(run, "run.log"), data); output.write(data); });
    }
    const stop = signal => child.kill(signal);
    const interrupt = () => stop("SIGINT"), terminate = () => stop("SIGTERM");
    process.on("SIGINT", interrupt); process.on("SIGTERM", terminate);
    try {
      code = await new Promise((resolve, reject) => {
        child.once("error", reject);
        child.once("close", (code, signal) => signal ? reject(new Error(`live-stack terminated: ${signal}`)) : resolve(code ?? 1));
      });
    } finally { process.off("SIGINT", interrupt); process.off("SIGTERM", terminate); }
  } finally {
    const after = snapshot();
    writeFileSync(join(run, "sources-after.json"), JSON.stringify(after, null, 2) + "\n");
    const changed = [...new Set([...Object.keys(before), ...Object.keys(after)])].filter(path => before[path] !== after[path]);
    writeFileSync(join(run, "source-check.json"), JSON.stringify({ stable: changed.length === 0, mode, frontendMode, changed, exitCode: code }, null, 2) + "\n");
    console.log(JSON.stringify({ phase: "source-freeze-after", mode, frontendMode, files: Object.keys(after).length, stable: changed.length === 0, changed, path: relative(root, run) }));
    assert.deepEqual(changed, [], "product or acceptance source changed during run");
  }
  return { code, run };
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const focus = process.argv[2] === "full" ? null : process.argv[2];
  const freezeVariable = ({ moderation: "BABBLE_MODERATION_SOURCE_FROZEN", "feed-diversity": "BABBLE_FEED_DIVERSITY_SOURCE_FROZEN", "browser-invocations": "BABBLE_BROWSER_INVOCATION_SOURCE_FROZEN" })[focus] ?? "BABBLE_LIVE_SOURCE_FROZEN";
  process.exitCode = (await runLiveStackEvidence({ focus, freezeVariable })).code;
}
