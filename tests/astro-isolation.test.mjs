import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { once } from "node:events";
import { mkdtemp, readFile, readdir, rm } from "node:fs/promises";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const script = fileURLToPath(new URL("./astro-server.mjs", import.meta.url));
const frontend = fileURLToPath(new URL("../frontend/", import.meta.url));
const activeChildren = new Set();

function launch(port, cache, mode) {
  const child = spawn(process.execPath, [script, String(port), cache, mode], {
    env: { ...process.env, ASTRO_TELEMETRY_DISABLED: "1", PUBLIC_BABEL_API_URL: "http://127.0.0.1:18787" },
    stdio: ["ignore", "pipe", "pipe", "ipc"],
  });
  activeChildren.add(child);
  let log = "";
  const capture = chunk => { log = (log + chunk).slice(-16_384); };
  child.stdout.on("data", capture);
  child.stderr.on("data", capture);
  const exited = new Promise((resolve, reject) => {
    child.once("error", reject);
    child.once("exit", (code, signal) => {
      activeChildren.delete(child);
      resolve({ code, signal, log });
    });
  });
  const timer = setTimeout(() => child.kill("SIGKILL"), 90_000);
  exited.finally(() => clearTimeout(timer)).catch(() => {});
  return { child, exited };
}

async function ready(process, mode) {
  const result = await Promise.race([
    once(process.child, "message").then(([message]) => message),
    process.exited.then(outcome => { throw new Error(`Astro exited before readiness: ${JSON.stringify(outcome)}`); }),
  ]);
  assert.equal(result.ready, true);
  assert.equal(result.mode, mode);
  return result.port;
}

async function availablePort() {
  const reservation = createServer();
  reservation.listen(0, "127.0.0.1");
  await once(reservation, "listening");
  const port = reservation.address().port;
  await new Promise(resolve => reservation.close(resolve));
  return port;
}

async function sharedFiles() {
  const files = {};
  async function collect(directory) {
    let entries;
    try { entries = await readdir(join(frontend, directory), { withFileTypes: true }); }
    catch (error) { if (error.code === "ENOENT") return; throw error; }
    for (const entry of entries) {
      const path = join(directory, entry.name);
      if (entry.isDirectory()) await collect(path);
      else files[path] = createHash("sha256").update(await readFile(join(frontend, path))).digest("hex");
    }
  }
  await collect("dist");
  try { files[".astro/dev.json"] = await readFile(join(frontend, ".astro/dev.json"), "utf8"); }
  catch (error) { if (error.code !== "ENOENT") throw error; }
  return files;
}

for (const mode of ["dev", "production"]) test(`isolated Astro ${mode} owns its port, output, cache, and lifetime`, async () => {
  const cache = await mkdtemp(join(tmpdir(), "babel-astro-isolation-"));
  const sharedBefore = await sharedFiles();
  const existing = createServer((_request, response) => response.end("existing preview"));
  existing.listen(0, "127.0.0.1");
  await once(existing, "listening");
  const existingPort = existing.address().port;
  const retained = async () => assert.equal(await (await fetch(`http://127.0.0.1:${existingPort}`)).text(), "existing preview");
  try {
    const collision = launch(existingPort, join(cache, "collision"), mode);
    const rejected = await collision.exited;
    assert.equal(rejected.code, 1, rejected.log);
    assert.match(rejected.log, /already in use/);
    await retained();

    const ownedPort = await availablePort();
    const owned = launch(ownedPort, join(cache, "owned"), mode);
    assert.equal(await ready(owned, mode), ownedPort);
    const origin = `http://127.0.0.1:${ownedPort}`;
    const response = await fetch(origin);
    const page = await response.text();
    assert.equal(response.status, 200, page.slice(0, 2_000));
    assert.match(response.headers.get("content-type"), /text\/html/);
    assert.match(page, /Babel Protocol/);
    assert.match(page, /data-feed-root/);
    assert.match(page, /data-babel-api="http:\/\/127\.0\.0\.1:18787"/);
    assert.doesNotMatch(page, /FailedToLoadModuleSSR|<astro-error-overlay/);
    if (mode === "production") {
      assert.equal(await readFile(join(cache, "owned/dist/index.html"), "utf8"), page);
      assert.doesNotMatch(page, /@vite\/client|astro\/runtime\/client\/dev|\/src\//);
      const assets = [...page.matchAll(/(?:src|href)="(\/assets\/[^"?#]+\.(?:js|css))"/g)].map(match => match[1]);
      assert.ok(assets.some(asset => asset.endsWith(".js")), "compiled application script must be referenced");
      assert.ok(assets.some(asset => asset.endsWith(".css")), "compiled stylesheet must be referenced");
      let styles = "";
      for (const asset of assets) {
        const response = await fetch(`${origin}${asset}`);
        assert.equal(response.status, 200, `compiled asset: ${asset}`);
        const content = await response.text();
        assert.ok(content.length > 0, `nonempty compiled asset: ${asset}`);
        if (asset.endsWith(".css")) styles += content;
      }
      assert.match(styles, /\.host-action-dialog/);
      assert.equal((await fetch(`${origin}/@vite/client`)).status, 404);
      assert.equal((await fetch(`${origin}/src/styles/host-actions.css`)).status, 404);
    } else {
      const stylesheet = await fetch(`${origin}/src/styles/host-actions.css`);
      assert.equal(stylesheet.status, 200);
      assert.match(await stylesheet.text(), /\.host-action-dialog/);
    }
    await retained();
    owned.child.disconnect();
    const closed = await owned.exited;
    assert.equal(closed.code, 0, closed.log);
    await assert.rejects(fetch(`http://127.0.0.1:${ownedPort}`));
    await retained();
    assert.deepEqual(await sharedFiles(), sharedBefore, "shared build output and user preview lock must be unchanged");
  } finally {
    for (const child of activeChildren) child.kill("SIGKILL");
    await Promise.all([...activeChildren].map(child => once(child, "exit")));
    await new Promise(resolve => existing.close(resolve));
    await rm(cache, { recursive: true, force: true });
  }
});
