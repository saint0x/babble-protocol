import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { existsSync } from "node:fs";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { test } from "node:test";
import { LiveStackLifecycle } from "./live-stack-lifecycle.mjs";

const fixture = new URL("./live-stack-lifecycle-fixture.mjs", import.meta.url).pathname;

function launch(role, storeRoot, mode) {
  const child = spawn(process.execPath, [fixture, role, storeRoot, mode], {
    stdio: ["ignore", "pipe", "pipe", "ipc"],
  });
  let log = "";
  child.stdout.on("data", chunk => { log += chunk; });
  child.stderr.on("data", chunk => { log += chunk; });
  const exited = new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => resolve({ code, signal, log }));
  });
  const message = once(child, "message", { signal: AbortSignal.timeout(5000) }).then(([value]) => value);
  return { child, exited, message };
}

async function stopFixture(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  const exited = once(child, "exit");
  child.kill("SIGKILL");
  await exited;
}

function running(pid) {
  try { process.kill(pid, 0); return true; }
  catch (error) { if (error.code === "ESRCH") return false; throw error; }
}

async function absent(pids) {
  const deadline = performance.now() + 2000;
  while (pids.some(running) && performance.now() < deadline) await delay(25);
  assert.deepEqual(pids.filter(running), [], "all owned children and descendants must exit");
}

for (const signal of ["SIGINT", "SIGTERM"]) test(`${signal} stops descendants after their group leader exits and preserves an unrelated service`,
  { timeout: 10_000 }, async () => {
    const directory = await mkdtemp(join(tmpdir(), "babble-lifecycle-tests-"));
    const storeRoot = await mkdtemp(join(directory, "store-"));
    const userFile = join(directory, "preview-user-data");
    await writeFile(userFile, "keep this data");
    const unrelated = launch("service", directory, "unrelated");
    const owner = launch("owner", storeRoot, "normal");
    let owned;
    try {
      const original = await unrelated.message;
      owned = await owner.message;
      assert.equal(owned.stage, "ready");
      for (const port of owned.ports) assert.equal((await fetch(`http://127.0.0.1:${port}`)).status, 200);
      const started = performance.now();
      owner.child.kill(signal);
      await delay(25);
      owner.child.kill(signal); // A repeated interrupt must not bypass graceful cleanup.
      const result = await owner.exited;
      assert.equal(result.code, signal === "SIGINT" ? 130 : 143, result.log);
      assert.ok(performance.now() - started < 3000, "shutdown must be bounded");
      await absent(owned.pids);
      assert.equal(existsSync(storeRoot), false);
      for (const port of owned.ports) await assert.rejects(fetch(`http://127.0.0.1:${port}`));
      assert.equal(await (await fetch(`http://127.0.0.1:${original.port}`)).text(), "live fixture");
      assert.equal(await readFile(userFile, "utf8"), "keep this data");
    } finally {
      await stopFixture(owner.child);
      await stopFixture(unrelated.child);
      for (const pid of owned?.pids ?? []) { try { process.kill(pid, "SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; } }
      await rm(directory, { recursive: true, force: true });
    }
  });

test("interrupt before readiness removes the owned child and disposable store", { timeout: 10_000 }, async () => {
  const storeRoot = await mkdtemp(join(tmpdir(), "babble-lifecycle-startup-"));
  const owner = launch("owner", storeRoot, "before-ready");
  let owned;
  try {
    owned = await owner.message;
    assert.equal(owned.stage, "spawned");
    owner.child.kill("SIGTERM");
    const result = await owner.exited;
    assert.equal(result.code, 143, result.log);
    await absent(owned.pids);
    assert.equal(existsSync(storeRoot), false);
  } finally {
    await stopFixture(owner.child);
    await rm(storeRoot, { recursive: true, force: true });
  }
});

test("parent IPC disconnect cleans running descendants even when no signal was forwarded", { timeout: 10_000 }, async () => {
  const storeRoot = await mkdtemp(join(tmpdir(), "babble-lifecycle-disconnect-"));
  const owner = launch("owner", storeRoot, "normal");
  let owned;
  try {
    owned = await owner.message;
    assert.equal(owned.stage, "ready");
    owner.child.disconnect();
    const result = await owner.exited;
    assert.equal(result.code, 143, result.log);
    await absent(owned.pids);
    assert.equal(existsSync(storeRoot), false);
    for (const port of owned.ports) await assert.rejects(fetch(`http://127.0.0.1:${port}`));
  } finally {
    await stopFixture(owner.child);
    for (const pid of owned?.pids ?? []) { try { process.kill(pid, "SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; } }
    await rm(storeRoot, { recursive: true, force: true });
  }
});

test("killed evidence wrapper closes IPC and log pipes without interrupting descendant cleanup", { timeout: 10_000 }, async () => {
  const storeRoot = await mkdtemp(join(tmpdir(), "babble-lifecycle-wrapper-"));
  const wrapper = launch("wrapper", storeRoot, "normal");
  let owned;
  try {
    owned = await wrapper.message;
    wrapper.child.kill("SIGKILL");
    assert.equal((await wrapper.exited).signal, "SIGKILL");
    await absent(owned.pids);
    assert.equal(existsSync(storeRoot), false);
    for (const port of owned.ports) await assert.rejects(fetch(`http://127.0.0.1:${port}`));
  } finally {
    await stopFixture(wrapper.child);
    for (const pid of owned?.pids ?? []) { try { process.kill(pid, "SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; } }
    await rm(storeRoot, { recursive: true, force: true });
  }
});

test("failure before readiness still cleans the other owned process and store", { timeout: 10_000 }, async () => {
  const storeRoot = await mkdtemp(join(tmpdir(), "babble-lifecycle-failure-"));
  const owner = launch("owner", storeRoot, "startup-failure");
  try {
    const owned = await owner.message;
    const result = await owner.exited;
    assert.equal(result.code, 1, result.log);
    assert.match(result.log, /fixture failed before readiness/);
    await absent(owned.pids);
    assert.equal(existsSync(storeRoot), false);
  } finally {
    await stopFixture(owner.child);
    await rm(storeRoot, { recursive: true, force: true });
  }
});

test("concurrent shutdown is idempotent, rejects restart admission, and removes storage only after child exit", { timeout: 10_000 }, async () => {
  const storeRoot = await mkdtemp(join(tmpdir(), "babble-lifecycle-restart-"));
  const lifecycle = new LiveStackLifecycle(storeRoot, { graceMs: 150, killMs: 1500 });
  const child = lifecycle.spawn(process.execPath, [fixture, "service", storeRoot, "stubborn"], {
    stdio: ["ignore", "ignore", "inherit", "ipc"],
  });
  try {
    await once(child, "message");
    let storeExistedAtExit = false;
    child.once("exit", () => { storeExistedAtExit = existsSync(storeRoot); });
    const stopped = lifecycle.stop(child);
    const first = lifecycle.cleanup();
    const second = lifecycle.cleanup();
    assert.equal(first, second);
    assert.equal(lifecycle.stopping, true);
    assert.throws(() => lifecycle.spawn(process.execPath, ["-e", "process.exit(0)"]), /cannot start/);
    await Promise.all([stopped, first, second]);
    assert.equal(storeExistedAtExit, true, "store deletion must follow child exit");
    assert.equal(existsSync(storeRoot), false);
    await lifecycle.stop(child);
    await absent([child.pid]);
  } finally { await lifecycle.cleanup(); }
});

test("spawn failure is cleanable and an unowned process is never stopped", { timeout: 10_000 }, async () => {
  const storeRoot = await mkdtemp(join(tmpdir(), "babble-lifecycle-spawn-"));
  const lifecycle = new LiveStackLifecycle(storeRoot, { graceMs: 150, killMs: 1500 });
  const unrelated = launch("service", storeRoot, "unrelated");
  try {
    const ready = await unrelated.message;
    await assert.rejects(lifecycle.stop(unrelated.child), /not owned/);
    const missing = lifecycle.spawn(join(storeRoot, "missing-executable"), [], { stdio: "ignore" });
    const [error] = await once(missing, "error");
    assert.equal(error.code, "ENOENT");
    await lifecycle.cleanup();
    assert.equal(existsSync(storeRoot), false);
    assert.equal(await (await fetch(`http://127.0.0.1:${ready.port}`)).text(), "live fixture");
  } finally {
    await lifecycle.cleanup();
    await stopFixture(unrelated.child);
  }
});

for (const failure of ["throw", "timeout"]) test(`owned service cleanup ${failure} still stops children and preserves the store`,
  { timeout: 10_000 }, async () => {
    const storeRoot = await mkdtemp(join(tmpdir(), "babble-lifecycle-hook-"));
    let calls = 0;
    const lifecycle = new LiveStackLifecycle(storeRoot, { graceMs: 100, killMs: 1500, beforeStop: async () => {
      calls++;
      assert.equal(running(child.pid), true, "service cleanup precedes child shutdown");
      if (failure === "throw") throw new Error("observer close failed");
      await new Promise(() => {});
    } });
    const child = lifecycle.spawn(process.execPath, [fixture, "service", storeRoot, "stubborn"], {
      stdio: ["ignore", "ignore", "inherit", "ipc"],
    });
    try {
      await once(child, "message");
      const first = lifecycle.cleanup();
      assert.equal(first, lifecycle.cleanup());
      await assert.rejects(first, error => error instanceof AggregateError && error.errors.some(cause =>
        failure === "throw" ? cause.message === "observer close failed" : /timed out/.test(cause.message)));
      assert.equal(calls, 1);
      await absent([child.pid]);
      assert.equal(existsSync(storeRoot), true, "failed cleanup retains evidence instead of deleting the store");
    } finally {
      try { process.kill(-child.pid, "SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; }
      await rm(storeRoot, { recursive: true, force: true });
    }
  });

test("owned in-process HTTP observer closes once before child shutdown and store removal", { timeout: 10_000 }, async () => {
  const storeRoot = await mkdtemp(join(tmpdir(), "babble-lifecycle-observer-"));
  const observer = createServer((_request, response) => response.end("observed"));
  observer.listen(0, "127.0.0.1");
  await once(observer, "listening");
  const url = `http://127.0.0.1:${observer.address().port}`;
  let calls = 0;
  const lifecycle = new LiveStackLifecycle(storeRoot, { graceMs: 150, killMs: 1500, beforeStop: async () => {
    calls++;
    assert.equal(running(child.pid), true);
    assert.equal(existsSync(storeRoot), true);
    observer.closeAllConnections();
    await new Promise((resolve, reject) => observer.close(error => error ? reject(error) : resolve()));
  } });
  const child = lifecycle.spawn(process.execPath, [fixture, "service", storeRoot, "unrelated"], {
    stdio: ["ignore", "ignore", "inherit", "ipc"],
  });
  try {
    await once(child, "message");
    assert.equal(await (await fetch(url)).text(), "observed");
    await Promise.all([lifecycle.cleanup(), lifecycle.cleanup()]);
    assert.equal(calls, 1);
    await assert.rejects(fetch(url));
    await absent([child.pid]);
    assert.equal(existsSync(storeRoot), false);
  } finally {
    await lifecycle.cleanup();
    observer.closeAllConnections();
    observer.close();
  }
});
