import assert from "node:assert/strict";
import { execFile, spawn } from "node:child_process";
import { createHash, randomBytes } from "node:crypto";
import { createServer, request as httpRequest } from "node:http";
import { createServer as netServer } from "node:net";
import { createReadStream } from "node:fs";
import { access, mkdir, mkdtemp, readFile, readdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { promisify } from "node:util";

// Production integration, never a replacement gateway or bridge. The parent
// must finish native binaries/generated SDK first; this script never builds them.
// --fixture-only generates and syntax-checks the portable fixture without API,
// native CLI execution, browser execution, or a claim of verification coverage.
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const exec = promisify(execFile);
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const sha256 = bytes => createHash("sha256").update(bytes).digest("hex");
async function fileHash(path) {
  const hash = createHash("sha256");
  for await (const bytes of createReadStream(path)) hash.update(bytes);
  return hash.digest("hex");
}
const fixtureOnly = process.argv.includes("--fixture-only");
assert.ok(process.argv.slice(2).every(arg => arg === "--fixture-only"), "unsupported argument");
const work = await mkdtemp(join(tmpdir(), "babble-verified-bundle-"));
const nonce = randomBytes(12).toString("hex");
const aegisAddr = process.env.BABBLE_VERIFIED_AEGIS_ADDR ?? "127.0.0.1:17894";
assert.match(aegisAddr, /^127\.0\.0\.1:\d+$/);
assert.ok(![7878, 7879, 17878, 17892].includes(Number(aegisAddr.split(":")[1])), "reserved Aegis port");
let apiBin = process.env.BABBLE_VERIFIED_API_BIN;
let cliBin = process.env.BABBLE_VERIFIED_CLI_BIN;
const sdkDir = resolve(process.env.BABBLE_VERIFIED_SDK_DIST ?? join(root, "sdk/dist"));
const env = { ...process.env, CARGO_INCREMENTAL: "0" };
for (const key of Object.keys(env)) if (/^(https?|all|no)_proxy$/i.test(key)) delete env[key];
for (const key of ["BABBLE_ALGORITHMS_DIR", "BABBLE_PYTHON_EXECUTABLE", "BABBLE_ALGORITHM_TIMEOUT_MS"]) delete env[key];
const evidence = { scope: "production inline bundle API + SDK", nonce, api: [], gateway: [], fixture: [], browser: null, stages: [] };
const services = [];
const leases = new Map();
const files = new Map();
const sdk = new Map();
let server, parentOrigin, apiOrigin, gatewayAddr, externalOrigin, token, identity, object, bundle, sessions, prepared;
let resolveBrowser;
const browserResult = new Promise(resolve => { resolveBrowser = resolve; });

async function bounded(promise, ms, label) {
  let timer;
  try { return await Promise.race([promise, new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(label)), ms);
  })]); } finally { clearTimeout(timer); }
}

async function reserve(port = 0) {
  const listener = netServer();
  await new Promise((resolve, reject) => { listener.once("error", reject); listener.listen(port, "127.0.0.1", resolve); });
  return listener;
}

function start(command, args, extraEnv = {}) {
  const child = spawn(command, args, { cwd: root, env: { ...env, ...extraEnv }, stdio: ["ignore", "pipe", "pipe"] });
  const service = { child, log: "", exited: null };
  service.exited = new Promise(resolve => {
    child.once("exit", (code, signal) => resolve({ code, signal }));
    child.once("error", error => resolve({ error: String(error) }));
  });
  const log = data => { service.log = (service.log + data).slice(-65536); };
  child.stdout.on("data", log); child.stderr.on("data", log);
  services.push(service);
  return service;
}

async function waitFor(check, label, ms = 20000) {
  const end = Date.now() + ms;
  let last;
  while (Date.now() < end) {
    try { const value = await check(); if (value) return value; } catch (error) { last = String(error); }
    for (const service of services) if (service.child.exitCode !== null || service.child.signalCode !== null) {
      throw new Error(`${label}: service exited\n${service.log}`);
    }
    await delay(75);
  }
  throw new Error(`${label} deadline: ${last ?? "predicate false"}`);
}

async function api(path, payload, expected = true, authenticated = true, method = payload === undefined ? "GET" : "POST") {
  const response = await fetch(apiOrigin + path, {
    method, credentials: "omit",
    headers: { "content-type": "application/json", ...(authenticated && token ? { authorization: `Bearer ${token}` } : {}) },
    ...(payload === undefined ? {} : { body: JSON.stringify(payload) }), signal: AbortSignal.timeout(15000),
  });
  const text = await response.text();
  let body;
  try { body = text ? JSON.parse(text) : null; } catch { body = text; }
  evidence.api.push({ path, method, status: response.status });
  if (expected) assert.ok(response.ok, `${path}: ${response.status} ${text}`);
  return { status: response.status, ok: response.ok, body };
}

async function maintainLease(sessionId) {
  const lease = { sessionId, active: true, timer: null, pending: null };
  leases.set(sessionId, lease);
  async function renew() {
    const value = (await api(`/runtime/surfaces/sessions/${sessionId}/heartbeat`, {})).body.lease;
    assert.equal(value.session_id, sessionId);
    assert.ok(Number.isSafeInteger(value.renew_after_ms) && value.renew_after_ms > 0 && value.renew_after_ms < value.ttl_ms);
    evidence.leases ??= [];
    evidence.leases.push({ sessionId, ...value });
    if (lease.active) lease.timer = setTimeout(() => {
      lease.pending = renew().catch(error => {
        evidence.heartbeatFailure = String(error);
        resolveBrowser({ ok: false, error: `real lease renewal failed: ${error}` });
      });
    }, value.renew_after_ms);
  }
  lease.pending = renew();
  await lease.pending;
}

async function stopLease(sessionId) {
  const lease = leases.get(sessionId);
  if (!lease) return;
  lease.active = false;
  clearTimeout(lease.timer);
  await lease.pending;
  leases.delete(sessionId);
}

async function gateway(origin, path, host = new URL(origin).host, headers = {}, method = "GET") {
  const result = await new Promise((resolve, reject) => {
    const req = httpRequest({ hostname: "127.0.0.1", port: Number(gatewayAddr.split(":")[1]),
      path, method, headers: { Host: host, ...headers }, agent: false }, response => {
      const chunks = [];
      response.on("data", chunk => chunks.push(chunk));
      response.on("end", () => resolve({ status: response.statusCode, headers: response.headers, bytes: Buffer.concat(chunks) }));
    });
    req.setTimeout(10000, () => req.destroy(new Error("gateway request deadline")));
    req.on("error", reject); req.end();
  });
  evidence.gateway.push({ host, path, method, headers, status: result.status, type: result.headers["content-type"],
    sha256: sha256(result.bytes), size: result.bytes.length });
  return result;
}

function assertNonExecutable(result, label) {
  assert.ok(result.status >= 400 && result.status < 500, `${label}: expected fail-closed 4xx, got ${result.status}`);
  assert.equal(result.headers.location, undefined, `${label}: redirects forbidden`);
  assert.match(result.headers["content-type"] ?? "", /^text\/plain(?:;|$)/, `${label}: error MIME`);
  assert.equal(result.headers["x-content-type-options"], "nosniff", label);
  assert.match(result.headers["content-security-policy"] ?? "", /default-src 'none'/, label);
}

async function captureSDK() {
  const names = (await readdir(sdkDir, { recursive: true })).filter(name => name.endsWith(".js")).sort();
  assert.ok(names.includes("index.js") && names.includes("channel.js") && names.includes("host.js"), "built SDK entrypoints required");
  for (const name of names) sdk.set(name, await readFile(join(sdkDir, name)));
  for (const [name, bytes] of sdk) assert.deepEqual(await readFile(join(sdkDir, name)), bytes, `SDK changed during capture: ${name}`);
  evidence.sdk = Object.fromEntries([...sdk].map(([name, bytes]) => [name, sha256(bytes)]));
}

function createBundle() {
  const add = (path, type, kind, body) => files.set(path, { type, kind, bytes: Buffer.isBuffer(body) ? body : Buffer.from(body) });
  const config = JSON.stringify({ parentOrigin, externalOrigin, nonce });
  add("app/index.html", "text/html", "document", '<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="./styles/main.css"><body><div id="marker">loading</div><script type="module" src="./main.js"></script>');
  add("app/main.js", "text/javascript", "script", `import { connectSurfaceBridge, createSurfaceSDK } from '../sdk/index.js';\nimport { cycle } from './cycle/a.js';\n(${childMain})(${config}, { connectSurfaceBridge, createSurfaceSDK, cycle }).catch(error => parent.postMessage({type:'verified.fixture.error',error:String(error)},${JSON.stringify(parentOrigin)}));`);
  add("app/cycle/a.js", "text/javascript", "script", "import { b } from './b.js'; export const a = () => 'a'; export const cycle = () => a() + ':' + b();");
  add("app/cycle/b.js", "text/javascript", "script", "import { a } from './a.js'; export const b = () => 'b:' + a();");
  add("app/nested/dynamic.js", "text/javascript", "script", "export { marker } from '../message.js';");
  add("app/message.js", "text/javascript", "script", "export const marker = 'verified-original-dynamic';\n");
  add("app/worker.js", "text/javascript", "script", "postMessage('forbidden-worker-executed');");
  add("app/unused.js", "text/javascript", "script", "export const inventoryOnly = 'must-verify-even-when-unused';\n");
  add("app/alternate.html", "text/html", "document", "<!doctype html><script>globalThis.alternateDocumentExecuted=true</script>");
  add("app/styles/main.css", "text/css", "stylesheet", '@import "./nested/color.css"; body { background-image: url("../../assets/pixel.png"); }');
  add("app/styles/nested/color.css", "text/css", "stylesheet", "body { color: rgb(17, 93, 121); }");
  add("assets/pixel.png", "image/png", "asset", Buffer.from("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=", "base64"));
  for (const [name, bytes] of sdk) add(`sdk/${name}`, "text/javascript", "script", bytes);
  return { kind: "babble.text", schema: "babble.schema.text.v1", payload: { text: `Verified bundle ${nonce}`, metadata: {} },
    surfaces: [{ role: "Feed", target: "Web", bundle: { entry_path: "app/index.html",
      files: [...files].map(([path, file]) => ({ path, file: `dist/${path}`, media_type: file.type, kind: file.kind })) } }] };
}

async function childMain(c, sdk) {
  const report = value => parent.postMessage({ type: "verified.fixture.report", value }, c.parentOrigin);
  const violations = [];
  document.addEventListener("securitypolicyviolation", event => violations.push({ directive: event.effectiveDirective, blocked: event.blockedURI }));
  let resolveBinding;
  const binding = new Promise(resolve => { resolveBinding = resolve; });
  let client;
  window.addEventListener("message", async event => {
    if (event.source !== parent || event.origin !== c.parentOrigin) return;
    if (event.data?.type === "verified.fixture.bind") resolveBinding(event.data.binding);
    if (event.data?.type === "verified.fixture.after-revoke") {
      try {
        await client.search.objects({ q: c.nonce, author: null, kind: null, limit: 5 }, { id: "after-revoke", traceId: "after-revoke", timeoutMs: 4000 });
        report({ stage: "revoked-rpc", rejected: false });
      } catch (error) { report({ stage: "revoked-rpc", rejected: true, error: String(error) }); }
    }
  });
  report({ stage: "entry-evaluated", origin: location.origin });
  const transport = await sdk.connectSurfaceBridge({ parentOrigin: c.parentOrigin, timeoutMs: 10000 });
  const bound = await binding;
  client = sdk.createSurfaceSDK({ transport, plan: bound.plan, runtimeId: "verified-browser-child",
    surfaceSessionId: bound.sessionId, origin: location.origin, currentIdentityId: bound.identityId });
  const dynamic = await import("./nested/dynamic.js");
  const search = await client.search.objects({ q: c.nonce, author: null, kind: null, limit: 5 }, { id: "permitted-search", traceId: "permitted-search", timeoutMs: 5000 });
  let domDenied = false;
  try { void parent.document.body; } catch (error) { domDenied = error.name === "SecurityError"; }
  const inline = document.createElement("script");
  inline.textContent = "globalThis.forbiddenInline = true";
  document.head.append(inline);
  let evalBlocked = false;
  try { (0, eval)("globalThis.forbiddenEval = true"); } catch (error) { evalBlocked = error.name === "EvalError"; }
  let externalBlocked = false;
  try { await import(c.externalOrigin + "/forbidden.js"); } catch { externalBlocked = true; }
  let undeclaredBlocked = false;
  try { await import("./undeclared.js"); } catch { undeclaredBlocked = true; }
  const blobURL = URL.createObjectURL(new Blob(["globalThis.forbiddenBlob = true; export default 1"], { type: "text/javascript" }));
  let blobBlocked = false;
  try { await import(blobURL); } catch { blobBlocked = true; }
  URL.revokeObjectURL(blobURL);
  async function worker(url) {
    return new Promise(resolve => {
      let instance, timer;
      const finish = result => { clearTimeout(timer); instance?.terminate(); resolve(result); };
      try {
        instance = new Worker(url);
        instance.onmessage = event => finish({ blocked: false, message: event.data });
        instance.onerror = event => { event.preventDefault(); finish({ blocked: true, mechanism: "error" }); };
        timer = setTimeout(() => finish({ blocked: false, mechanism: "unobserved-timeout" }), 3000);
      } catch (error) { finish({ blocked: error.name === "SecurityError", mechanism: error.name }); }
    });
  }
  const normalWorker = await worker(new URL("./worker.js", import.meta.url));
  const workerURL = URL.createObjectURL(new Blob(["postMessage('forbidden-blob-worker')"], { type: "text/javascript" }));
  const blobWorker = await worker(workerURL);
  URL.revokeObjectURL(workerURL);
  await new Promise(resolve => setTimeout(resolve, 40));
  document.querySelector("#marker").textContent = dynamic.marker;
  report({ stage: "complete", origin: location.origin, secure: isSecureContext, cycle: sdk.cycle(), dynamic: dynamic.marker,
    objectIds: search.results.map(result => result.object.id), domDenied,
    inlineBlocked: !globalThis.forbiddenInline, evalBlocked, externalBlocked, undeclaredBlocked, blobBlocked,
    blobExecuted: Boolean(globalThis.forbiddenBlob), normalWorker, blobWorker, violations,
    color: getComputedStyle(document.body).color, background: getComputedStyle(document.body).backgroundImage,
    resources: performance.getEntriesByType("resource").map(entry => ({ name: entry.name, initiator: entry.initiatorType })) });
}

async function parentMain(c, sdk) {
  const state = { cases: [], rpc: [], http: [], stages: [], errors: [] };
  const mounts = [];
  const cleanup = [];
  const check = (value, message) => { if (!value) throw new Error(message); };
  const wait = async (predicate, message, ms = 15000) => {
    const end = Date.now() + ms;
    while (Date.now() < end) {
      if (state.errors.length) throw new Error(JSON.stringify(state.errors));
      const result = predicate();
      if (result) return result;
      await new Promise(resolve => setTimeout(resolve, 25));
    }
    throw new Error("deadline: " + message);
  };
  const control = async operation => {
    const response = await fetch("/control", { method: "POST", headers: { "content-type": "application/json" },
      credentials: "omit", body: JSON.stringify({ operation }) });
    const value = await response.json();
    check(response.ok && value.ok, JSON.stringify(value));
    return value;
  };
  const transport = new sdk.HttpRpcTransport(c.apiOrigin + "/rpc", async (input, init) => {
    const response = await fetch(input, { ...init, credentials: "omit", headers: { ...init.headers, authorization: "Bearer " + c.token } });
    const request = JSON.parse(init.body);
    state.http.push({ status: response.status, id: request.id, traceId: request.trace_id,
      sessionId: request.binding.surface_session_id });
    return response;
  });
  const host = new sdk.BrowserSurfaceHost();
  const admissionRejected = s => !s.ready && s.calls.length === 0;
  async function mount(name, session, replacementURL, mode = "secure") {
    const s = { name, sessionId: session.id, origin: session.plan.verified_mount.origin, offers: [], reports: [], calls: [], ready: false };
    state.cases.push(s);
    let mounted;
    const observe = event => {
      if (event.source === null || !mounted?.frame.contentWindow || event.source !== mounted.frame.contentWindow) return;
      if (event.data?.type === "babble.surface.connect") {
        const offer = { origin: event.origin, sourceMatch: true, closed: false, messages: [] };
        s.offers.push(offer);
        for (const port of event.ports) {
          const close = port.close;
          const send = port.postMessage;
          cleanup.push(() => { port.close = close; port.postMessage = send; });
          port.close = function() { close.call(this); offer.closed = true; };
          port.postMessage = function(value, ...args) { send.call(this, value, ...args); offer.messages.push(value?.type); };
        }
      }
      if (event.data?.type === "verified.fixture.report") s.reports.push({ eventOrigin: event.origin, ...event.data.value });
      if (event.data?.type === "verified.fixture.error") state.errors.push(event.data.error);
    };
    window.addEventListener("message", observe);
    cleanup.push(() => window.removeEventListener("message", observe));
    const element = document.createElement("section"); document.body.append(element);
    const container = replacementURL ? { appendChild(frame) { frame.src = replacementURL; return element.appendChild(frame); } } : element;
    // Deliberately corrupt only the test's event view. The real SDK host and
    // real transferred MessagePort remain unchanged, with no synthetic offers.
    const handlers = new Map();
    const hostWindow = mode === "secure" ? window : {
      location: window.location,
      addEventListener(type, handler) {
        const wrapped = event => handler({ data: event.data, ports: event.ports,
          origin: mode === "rewrite-origin" && event.source === mounted?.frame.contentWindow ? s.origin : event.origin,
          source: mode === "rewrite-source" && event.origin === s.origin ? mounted?.frame.contentWindow : event.source });
        handlers.set(handler, wrapped); window.addEventListener(type, wrapped);
      },
      removeEventListener(type, handler) {
        window.removeEventListener(type, handlers.get(handler)); handlers.delete(handler);
      },
    };
    cleanup.push(() => { for (const handler of handlers.values()) window.removeEventListener("message", handler); });
    mounted = host.mount({ container, plan: session.plan, surfaceSessionId: session.id,
      window: hostWindow,
      currentIdentityId: c.identityId, handshakeTimeoutMs: replacementURL ? 3000 : 10000,
      registerDocument: async (documentId, signal) => {
        const response = await fetch(c.apiOrigin + "/runtime/surfaces/sessions/" + encodeURIComponent(session.id) + "/document", {
          method: "PUT", signal, credentials: "omit",
          headers: { "content-type": "application/json", authorization: "Bearer " + c.token },
          body: JSON.stringify({ document_id: documentId }),
        });
        check(response.ok, "document registration status " + response.status);
        const binding = await response.json();
        check(binding.session_id === session.id && binding.document_id === documentId, "document binding mismatch");
      },
      dispatch: async (request, context) => {
        s.calls.push(request.id);
        const reply = await transport.request(request, context);
        state.rpc.push({ name, id: request.id, traceId: request.trace_id, method: request.method, binding: request.binding,
          objectIds: reply.result?.results?.map(result => result.object.id), error: reply.error });
        return reply;
      } });
    mounts.push(mounted);
    mounted.ready.then(() => { s.ready = true; }, error => { s.readyError = String(error); });
    if (replacementURL) return { mounted, s };
    await mounted.ready;
    check(mounted.frame.src === session.plan.verified_mount.entry_url, "SDK must use verified entry_url");
    check(mounted.frame.sandbox.contains("allow-scripts") && mounted.frame.sandbox.contains("allow-same-origin"), "gateway sandbox tokens");
    mounted.frame.contentWindow.postMessage({ type: "verified.fixture.bind", binding: {
      plan: session.plan, sessionId: session.id, identityId: c.identityId,
    } }, session.plan.verified_mount.origin);
    const result = await wait(() => s.reports.find(r => r.stage === "complete"), name + " child execution");
    check(result.eventOrigin === s.origin && result.origin === s.origin, "exact execution origin");
    check(result.secure && result.domDenied, "secure context and parent DOM isolation");
    check(result.cycle === "a:b:a" && result.dynamic === "verified-original-dynamic", "immutable module execution markers");
    check(result.color === "rgb(17, 93, 121)" && result.background.includes("/assets/pixel.png"), "relative CSS");
    check(result.resources.some(r => r.name === s.origin + "/assets/pixel.png"), "CSS asset actually requested");
    check(result.objectIds.includes(c.objectId), "permitted production RPC returned published Object");
    for (const name of ["inlineBlocked", "evalBlocked", "externalBlocked", "undeclaredBlocked", "blobBlocked"]) check(result[name], name);
    check(!result.blobExecuted && result.normalWorker.blocked && result.blobWorker.blocked, "workers/blob denied with observed failures");
    for (const blocked of ["inline", "eval"]) check(result.violations.some(v => v.blocked === blocked), "CSP event: " + blocked);
    check(result.violations.some(v => v.directive === "worker-src"), "worker CSP enforcement observed");
    const rpc = state.rpc.find(r => r.name === s.name && r.traceId === "permitted-search");
    check(rpc && /^bridge-[0-9a-f-]{36}$/.test(rpc.id), "bridge assigns a fresh transport attempt ID");
    check(rpc && !rpc.error && rpc.binding.identity_id === c.identityId && rpc.binding.object_id === c.objectId
      && rpc.binding.surface_session_id === session.id && rpc.binding.origin === s.origin, "SDK dispatch scoped to actual account session");
    check(s.offers.length === 1 && s.offers[0].origin === s.origin && s.ready, "production child handshake admitted once");
    return { mounted, s };
  }
  async function wrongSource(mode) {
    const session = c.sessions[3];
    const target = await mount("wrong-source-" + mode, session, c.externalOrigin + "/idle.html", mode);
    const sibling = document.createElement("iframe");
    sibling.sandbox = "allow-scripts allow-same-origin";
    sibling.setAttribute("credentialless", "");
    const observations = target.s.sibling = { offers: [], reports: [] };
    const observe = event => {
      if (event.source === null || !sibling.contentWindow || event.source !== sibling.contentWindow) return;
      if (event.data?.type === "babble.surface.connect") observations.offers.push({ origin: event.origin,
        matchesAssignedFrame: event.source === target.mounted.frame.contentWindow });
      if (event.data?.type === "verified.fixture.report") observations.reports.push({ eventOrigin: event.origin, ...event.data.value });
    };
    window.addEventListener("message", observe);
    cleanup.push(() => { window.removeEventListener("message", observe); sibling.remove(); });
    sibling.src = session.plan.verified_mount.entry_url;
    document.body.append(sibling);
    await wait(() => observations.offers.length && observations.reports.some(r => r.stage === "entry-evaluated"), "real sibling connector offered");
    check(observations.offers[0].origin === session.plan.verified_mount.origin && !observations.offers[0].matchesAssignedFrame, "same-origin different-frame offer");
    if (mode === "secure") {
      await wait(() => target.s.readyError, "real SDK source rejection deadline", 5000);
      check(admissionRejected(target.s), "actual sibling cannot establish assigned frame channel");
    } else {
      await wait(() => target.s.ready, "source negative control accepted actual sibling");
      sibling.contentWindow.postMessage({ type: "verified.fixture.bind", binding: {
        plan: session.plan, sessionId: session.id, identityId: c.identityId,
      } }, session.plan.verified_mount.origin);
      await wait(() => observations.reports.some(r => r.stage === "complete"), "source negative control actual RPC");
      check(!admissionRejected(target.s) && state.rpc.some(r => r.name === target.s.name && r.traceId === "permitted-search" && !r.error), "source rejection predicate catches weakened SDK event wrapper");
    }
    sibling.remove();
    target.mounted.evict("source control finished");
  }
  try {
    let prematureFrames = 0, prematureRejected = false;
    try {
      host.mount({ container: { appendChild(frame) { prematureFrames++; return frame; } }, plan: c.prepared,
        surfaceSessionId: c.sessions[0].id, currentIdentityId: c.identityId,
        registerDocument: async () => { throw new Error("premature document registration"); },
        dispatch: () => { throw new Error("premature dispatch"); } });
    } catch (error) { prematureRejected = /verified bundle execution gateway/.test(String(error)); }
    check(prematureRejected && prematureFrames === 0, "prepare receipt alone must not mount an iframe");
    state.stages.push({ operation: "prepare-without-mount-descriptor-rejected", ok: true });
    const a = await mount("mount-a", c.sessions[0]);
    const b = await mount("mount-b", c.sessions[1]);
    check(a.s.origin !== b.s.origin, "same immutable bundle gets two different origins");
    const wrong = await mount("wrong-first-origin", c.sessions[2], c.externalOrigin + "/external.html");
    await wait(() => wrong.s.reports.some(r => r.stage === "external-attempt") && wrong.s.offers.length, "real external connector attempted");
    await wait(() => wrong.s.offers[0].closed, "production SDK closes wrong-origin offered port");
    check(wrong.s.offers[0].origin === c.externalOrigin && wrong.s.offers[0].sourceMatch, "wrong origin with correct frame source");
    check(admissionRejected(wrong.s) && wrong.s.offers[0].messages.length === 0, "wrong first origin never admitted or dispatched");
    wrong.mounted.evict("wrong origin experiment complete");
    const weakened = await mount("wrong-first-origin-negative-control", c.sessions[2], c.externalOrigin + "/external.html", "rewrite-origin");
    await wait(() => weakened.s.reports.some(r => r.stage === "external-admitted"), "external production client completes negative-control RPC");
    check(weakened.s.offers[0].origin === c.externalOrigin && weakened.s.offers[0].sourceMatch, "negative control actual origin remains external");
    check(!admissionRejected(weakened.s) && state.rpc.some(r => r.name === weakened.s.name && r.traceId === "external-rpc" && !r.error), "origin rejection predicate catches weakened SDK event wrapper");
    weakened.mounted.evict("origin negative control finished");
    await wrongSource("secure");
    await wrongSource("rewrite-source");
    const unknown = await mount("unknown-first-document", c.sessions[2], c.sessions[2].plan.verified_mount.origin + "/missing.html");
    await wait(() => unknown.s.readyError, "unknown document readiness rejection", 5000);
    check(!unknown.s.ready && !unknown.s.offers.length && !unknown.s.calls.length, "unknown document never admitted");
    unknown.mounted.evict("unknown document experiment complete");
    state.stages.push(await control("evict-a"));
    a.mounted.evict("native session evicted");
    check(!a.mounted.frame.isConnected, "SDK eviction removes frame");
    state.stages.push(await control("revoke-account-session"));
    b.mounted.frame.contentWindow.postMessage({ type: "verified.fixture.after-revoke" }, b.s.origin);
    const revoked = await wait(() => b.s.reports.find(r => r.stage === "revoked-rpc"), "actual revoked RPC rejection");
    check(revoked.rejected && state.http.some(r => r.traceId === "after-revoke" && r.sessionId === b.s.sessionId && [401, 403].includes(r.status)), "revocation rejects real authenticated RPC");
    b.mounted.evict("account session revoked");
    state.ok = true;
  } catch (error) { state.ok = false; state.error = String(error); }
  finally {
    for (const mounted of mounts) mounted.unmount("integration complete");
    for (const restore of cleanup.reverse()) restore();
    transport.close();
    document.querySelector("#result").textContent = (state.ok ? "VERIFIED_BUNDLE_PASS " : "VERIFIED_BUNDLE_FAIL ") + JSON.stringify(state);
    await fetch("/result", { method: "POST", credentials: "omit", body: JSON.stringify(state) });
  }
}

async function externalMain(parentOrigin, sdk) {
  parent.postMessage({ type: "verified.fixture.report", value: { stage: "external-attempt" } }, parentOrigin);
  try {
    const transport = await sdk.connectSurfaceBridge({ parentOrigin, timeoutMs: 2000 });
    const client = sdk.createBabbleSDK({ transport, binding: sdk.hostBinding("external-control", location.origin) });
    const result = await client.search.objects({ q: null, author: null, kind: null, limit: 5 }, { id: "external-rpc", traceId: "external-rpc", timeoutMs: 4000 });
    parent.postMessage({ type: "verified.fixture.report", value: { stage: "external-admitted", objectIds: result.results.map(r => r.object.id) } }, parentOrigin);
  } catch (error) {
    parent.postMessage({ type: "verified.fixture.report", value: { stage: "external-rejected", error: String(error) } }, parentOrigin);
  }
}

async function fixtureRoute(req, res) {
  const origin = `http://${req.headers.host}`;
  evidence.fixture.push({ host: req.headers.host, path: req.url, method: req.method });
  const respond = (status, type, body) => {
    res.writeHead(status, { "content-type": type, "cache-control": "no-store", "x-content-type-options": "nosniff",
      "referrer-policy": "no-referrer", "Allow-CSP-From": "*" });
    res.end(body);
  };
  if (![parentOrigin, externalOrigin].includes(origin)) return respond(421, "text/plain", "unknown fixture host");
  if (req.method === "POST" && origin === parentOrigin && ["/result", "/control"].includes(req.url)) {
    assert.equal(req.headers.origin, parentOrigin, "parent-only fixture control");
    let body = "";
    for await (const chunk of req) { body += chunk; assert.ok(body.length < 1024 * 1024, "bounded result"); }
    const value = JSON.parse(body);
    if (req.url === "/result") { resolveBrowser(value); return respond(200, "application/json", '{"ok":true}'); }
    if (value.operation === "evict-a") {
      await stopLease(sessions[0].id);
      const transition = await api(`/runtime/surfaces/sessions/${sessions[0].id}/lifecycle`, { lifecycle: "evicted", reason: "verified browser test" });
      assert.equal(transition.body.session.lifecycle, "evicted");
      for (const path of files.keys()) assertNonExecutable(await gateway(sessions[0].plan.verified_mount.origin, "/" + path), "evicted session delivery");
    } else if (value.operation === "revoke-account-session") {
      for (const sessionId of [...leases.keys()]) await stopLease(sessionId);
      await api("/auth/session", undefined, true, true, "DELETE");
      for (const path of files.keys()) assertNonExecutable(await gateway(sessions[1].plan.verified_mount.origin, "/" + path), "revoked session delivery");
    } else throw new Error("unsupported fixture control");
    return respond(200, "application/json", JSON.stringify({ ok: true, operation: value.operation }));
  }
  if (req.method !== "GET") return respond(405, "text/plain", "method denied");
  if (req.url.startsWith("/sdk/") && sdk.has(req.url.slice(5))) return respond(200, "text/javascript", sdk.get(req.url.slice(5)));
  if (origin === parentOrigin && req.url === "/") return respond(200, "text/html", '<!doctype html><meta charset="utf-8"><pre id="result">RUNNING</pre><script type="module" src="/parent.js"></script>');
  if (origin === parentOrigin && req.url === "/parent.js") return respond(200, "text/javascript",
    `import * as sdk from '/sdk/index.js';\nconst config = await (await fetch('/config')).json();\n(${parentMain})(config, sdk);`);
  if (origin === parentOrigin && req.url === "/config") return respond(200, "application/json", JSON.stringify({
    apiOrigin, parentOrigin, externalOrigin, token, identityId: identity.id, objectId: object.id, nonce, sessions, prepared,
  }));
  if (origin === externalOrigin && req.url === "/external.html") return respond(200, "text/html", '<!doctype html><script type="module" src="/external.js"></script>');
  if (origin === externalOrigin && req.url === "/external.js") return respond(200, "text/javascript",
    `import * as sdk from '/sdk/index.js';\n(${externalMain})(${JSON.stringify(parentOrigin)},sdk);`);
  if (origin === externalOrigin && req.url === "/idle.html") return respond(200, "text/html", "<!doctype html><title>Inactive designated frame</title>");
  if (req.url === "/forbidden.js") return respond(200, "text/javascript", "globalThis.forbiddenExternal = true; export default 1;");
  return respond(404, "text/plain", "unknown fixture path");
}

async function prepareAndPublish() {
  async function binary(explicit, name) {
    const target = process.env.CARGO_BUILD_TARGET ?? (process.platform === "darwin" && process.arch === "arm64" ? "aarch64-apple-darwin" : null);
    const paths = explicit ? [resolve(explicit)] : [join(root, "backend/target/debug", name), ...(target ? [join(root, "backend/target", target, "debug", name)] : [])];
    for (const path of paths) { try { await access(path); return path; } catch (error) { if (error.code !== "ENOENT") throw error; } }
    throw new Error(`parent-built ${name} binary missing: ${paths.join(", ")}`);
  }
  apiBin = await binary(apiBin, "babble-api"); cliBin = await binary(cliBin, "babble");
  evidence.binaries = { apiBin, cliBin, apiSHA256: await fileHash(apiBin), cliSHA256: await fileHash(cliBin) };
  const apiReservation = await reserve();
  const gatewayReservation = await reserve();
  apiOrigin = `http://127.0.0.1:${apiReservation.address().port}`;
  gatewayAddr = `127.0.0.1:${gatewayReservation.address().port}`;
  const store = join(work, "store");
  await new Promise(resolve => apiReservation.close(resolve));
  await new Promise(resolve => gatewayReservation.close(resolve));
  start(apiBin, [], { BABBLE_API_ADDR: new URL(apiOrigin).host, BABBLE_PUBLIC_ORIGIN: apiOrigin,
    BABBLE_STORE_ROOT: store, BABBLE_SEED_PROFILE: "", BABBLE_JUDGMENT_PROVIDER: "rust-local",
    BABBLE_CORS_ORIGINS: parentOrigin, BABBLE_BUNDLE_GATEWAY_ADDR: gatewayAddr });
  await waitFor(async () => (await fetch(apiOrigin + "/health")).ok, "API ready");
  const password = `Verified bundle ${nonce}!`;
  const registered = (await api("/auth/register", { kind: "Person", handle: `verified-${nonce}`, password }, true, false)).body;
  identity = registered.identity;
  const login = (await api("/auth/login", { identity_id: identity.id, password }, true, false)).body;
  token = login.token;
  assert.equal(login.identity.id, identity.id);
  const build = JSON.parse((await exec(cliBin, ["build", join(work, "manifest.json")], { env, timeout: 30000, maxBuffer: 8 * 1024 * 1024 })).stdout);
  bundle = build.draft.surfaces[0].bundle;
  assert.equal(bundle.files.length, files.size);
  for (const file of bundle.files) {
    const captured = files.get(file.path);
    assert.ok(captured, file.path);
    assert.equal(file.size_bytes, captured.bytes.length);
    const blob = (await api("/media/blobs", { media_type: file.media_type, bytes_hex: captured.bytes.toString("hex") })).body.blob;
    assert.equal(blob.integrity, file.integrity, "API hashes exactly the CLI-captured bytes");
  }
  object = (await api("/objects", { author_id: identity.id, draft: build.draft })).body.object;
  assert.equal(object.author, identity.id);
  assert.deepEqual(object.surfaces[0].bundle, bundle, "signed publication retains exact inventory");
  evidence.objectId = object.id;
  evidence.bundle = bundle;
  const request = { object_id: object.id, role: "Feed" };
  const unused = bundle.files.find(file => file.path === "app/unused.js");
  const unusedPath = join(store, "blobs", unused.integrity);
  await writeFile(unusedPath, Buffer.alloc(unused.size_bytes, 120));
  const failedPrepare = await api("/runtime/surfaces/prepare", request, false);
  assert.ok(!failedPrepare.ok || failedPrepare.body.plan?.admission !== "ready", "corrupt unused dependency prevents preparation");
  assert.ok(!failedPrepare.body?.plan?.bundle_verification, "corruption cannot issue verification receipt");
  const failedStart = await api("/runtime/surfaces/sessions", { ...request, session_id: null }, false);
  assert.ok(!failedStart.ok && !failedStart.body?.session?.plan?.verified_mount, "corruption prevents session and mount issuance");
  evidence.stages.push({ stage: "corrupt-unused-before-prepare-and-start", prepare: failedPrepare, start: failedStart });
  await writeFile(unusedPath, files.get(unused.path).bytes);
  prepared = (await api("/runtime/surfaces/prepare", request)).body.plan;
  assert.equal(prepared.admission, "ready");
  assert.equal(prepared.bundle_verification.policy_version, 1);
  assert.ok(prepared.bundle_verification.manifest_hash);
  assert.ok(!prepared.verified_mount, "prepare does not allocate an executable mount");
  sessions = [];
  for (let i = 0; i < 4; i++) {
    const session = (await api("/runtime/surfaces/sessions", { ...request, session_id: null })).body.session;
    const mount = session.plan.verified_mount;
    assert.ok(mount, "session-start must issue production verified mount descriptor");
    assert.equal(mount.version, 1);
    assert.equal(mount.session_id, session.id); assert.equal(mount.object_id, object.id); assert.equal(mount.role, "Feed");
    assert.equal(mount.manifest_hash, prepared.bundle_verification.manifest_hash);
    assert.deepEqual(session.plan.bundle_verification, prepared.bundle_verification);
    const origin = new URL(mount.origin);
    assert.match(origin.hostname, /^m-[0-9a-f]{48}\.localhost$/);
    assert.equal(origin.port, gatewayAddr.split(":")[1]);
    assert.notEqual(mount.origin, parentOrigin); assert.notEqual(mount.origin, apiOrigin);
    assert.equal(new URL(mount.entry_url).origin, mount.origin);
    assert.equal(new URL(mount.entry_url).pathname, "/" + bundle.entry_path);
    sessions.push(session);
    await maintainLease(session.id);
  }
  assert.equal(new Set(sessions.map(s => s.plan.verified_mount.origin)).size, 4);
  evidence.sessions = sessions;
  // Corrupt the entry's dynamic dependency after all sessions hold snapshots and
  // before the browser has requested a single byte from any mount.
  const dependency = bundle.files.find(file => file.path === "app/message.js");
  const replacement = Buffer.from("export const marker = 'corrupted-on-disk';\n".padEnd(dependency.size_bytes, " "));
  assert.equal(replacement.length, dependency.size_bytes);
  await writeFile(join(store, "blobs", dependency.integrity), replacement);
  const freshPrepare = await api("/runtime/surfaces/prepare", request, false);
  assert.ok(!freshPrepare.ok || freshPrepare.body.plan?.admission !== "ready", "new prepare fails after disk corruption");
  const freshStart = await api("/runtime/surfaces/sessions", { ...request, session_id: null }, false);
  assert.ok(!freshStart.ok, "new start fails after disk corruption");
  const resumed = (await api("/runtime/surfaces/sessions", { ...request, session_id: sessions[0].id })).body.session;
  assert.deepEqual(resumed.plan.verified_mount, sessions[0].plan.verified_mount, "idempotent start retains the captured mount after disk corruption");
  evidence.stages.push({ stage: "fresh-reverification-fails-existing-session-retained", prepare: freshPrepare.status, start: freshStart.status });
  for (const session of sessions) {
    for (const [path, file] of files) {
      const served = await gateway(session.plan.verified_mount.origin, "/" + path);
      if (file.kind === "document" && path !== bundle.entry_path) {
        assertNonExecutable(served, "alternate declared document denied");
        continue;
      }
      assert.equal(served.status, 200, path);
      assert.deepEqual(served.bytes, file.bytes, `snapshot serves captured bytes after disk mutation: ${path}`);
      assert.equal(served.headers["content-type"]?.split(";")[0], file.type, path);
      assert.equal(served.headers["x-content-type-options"], "nosniff", path);
      assert.equal(served.headers["referrer-policy"], "no-referrer", path);
      assert.ok(served.headers["permissions-policy"], "production Permissions-Policy");
      assert.equal(served.headers["cross-origin-resource-policy"], "same-origin");
      assert.equal(served.headers["cache-control"], "no-store");
      assert.match(served.headers["content-security-policy"] ?? "", /default-src 'none'/);
    }
  }
  evidence.stages.push({ stage: "snapshot-survives-disk-corruption", dependency: dependency.path });
  const origin = sessions[0].plan.verified_mount.origin;
  assertNonExecutable(await gateway(origin, "/app/index.html", undefined, { "Sec-Fetch-Dest": "document" }), "top-level navigation denied");
  assert.equal((await gateway(origin, "/app/index.html", undefined, { "Sec-Fetch-Dest": "iframe" })).status, 200, "entry iframe allowed");
  assertNonExecutable(await gateway(origin, "/app/main.js", undefined, { "Sec-Fetch-Dest": "iframe" }), "script navigation denied");
  assertNonExecutable(await gateway(origin, "/app/alternate.html", undefined, { "Sec-Fetch-Dest": "iframe" }), "alternate document navigation denied");
  assertNonExecutable(await gateway(origin, "/app/main.js", undefined, {}, "POST"), "mutation method denied");
  const head = await gateway(origin, "/app/main.js", undefined, {}, "HEAD");
  assert.equal(head.status, 200); assert.equal(head.bytes.length, 0);
  for (const path of ["/missing.html", "/app/index.html?mime=text/javascript", "/%2e%2e/api", "/app/../index.html", "/app/%2fmain.js", "//app/index.html", "/rpc"]) {
    assertNonExecutable(await gateway(origin, path), path);
  }
  for (const host of [`unknown-${nonce}.localhost:${new URL(origin).port}`, new URL(origin).hostname,
    `${new URL(origin).hostname}.evil:${new URL(origin).port}`, new URL(apiOrigin).host]) {
    assertNonExecutable(await gateway(origin, "/app/index.html", host), host);
  }
}

try {
  if (!fixtureOnly) assert.equal(process.env.BABBLE_VERIFIED_BUNDLE_READY, "1", "await parent's binaries/types-ready signal, then set BABBLE_VERIFIED_BUNDLE_READY=1");
  await captureSDK();
  server = createServer((req, res) => fixtureRoute(req, res).catch(error => {
    evidence.controlError = String(error);
    if (!res.headersSent) res.writeHead(500, { "content-type": "application/json" });
    res.end(JSON.stringify({ ok: false, error: String(error) }));
  }));
  await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
  parentOrigin = `http://127.0.0.1:${server.address().port}`;
  externalOrigin = `http://external-${nonce}.localhost:${server.address().port}`;
  const manifest = createBundle();
  for (const [path, file] of files) {
    const target = join(work, "dist", path);
    await mkdir(dirname(target), { recursive: true }); await writeFile(target, file.bytes);
  }
  await writeFile(join(work, "manifest.json"), JSON.stringify(manifest, null, 2));
  await writeFile(join(work, "package.json"), '{"type":"module"}');
  // File syntax checking uses Node's parser only, never a substitute browser.
  for (const name of ["app/main.js", "app/cycle/a.js", "app/cycle/b.js", "app/nested/dynamic.js"] ) {
    await exec(process.execPath, ["--check", join(work, "dist", name)], { env, timeout: 10000 });
  }
  await writeFile(join(work, "parent.mjs"), `(${parentMain});\n(${externalMain});`);
  await exec(process.execPath, ["--check", join(work, "parent.mjs")], { env, timeout: 10000 });
  if (fixtureOnly) {
    console.log(`VERIFIED_BUNDLE_FIXTURE_PASS: ${files.size} portable files and browser source syntax; no native/browser execution. ${work}`);
  } else {
    await exec("sh", ["-c", "command -v aegis"], { env });
    await exec("aegis", ["--help"], { env }); await exec("aegis", ["serve", "--help"], { env });
    const probe = await reserve(Number(aegisAddr.split(":")[1])); await new Promise(resolve => probe.close(resolve));
    await prepareAndPublish();
    const aegis = (...args) => exec("aegis", ["--server-addr", aegisAddr, ...args], { env, timeout: 25000, maxBuffer: 8 * 1024 * 1024 });
    start("aegis", ["--mode", "headless", "serve", "--addr", aegisAddr]);
    await waitFor(async () => { await aegis("page", "inspect"); return true; }, "Aegis ready");
    await aegis("navigate", parentOrigin + "/");
    evidence.browser = await bounded(browserResult, 90000, "production browser result deadline");
    evidence.page = (await aegis("page", "text", "--scope", "full")).stdout;
    assert.equal(evidence.browser.ok, true, evidence.browser.error ?? "browser assertions failed");
    assert.match(evidence.page, /VERIFIED_BUNDLE_PASS/);
    assert.ok(!evidence.fixture.some(r => r.path === "/forbidden.js"), "CSP external import never reaches hostile server");
    assert.ok(!evidence.controlError, evidence.controlError);
    assert.ok(!evidence.heartbeatFailure, evidence.heartbeatFailure);
    console.log("VERIFIED_BUNDLE_PASS: production snapshot gateway and SDK, two origins, real account RPC, corruption, CSP, route denial, eviction and revocation.");
  }
} catch (error) {
  evidence.failure = String(error);
  throw error;
} finally {
  for (const sessionId of [...leases.keys()]) await stopLease(sessionId);
  for (const service of services.reverse()) {
    if (service.child.exitCode === null && service.child.signalCode === null) {
      service.child.kill("SIGTERM");
      try { await bounded(service.exited, 5000, "service shutdown"); }
      catch { service.child.kill("SIGKILL"); await service.exited; }
    }
  }
  if (server?.listening) { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); }
  evidence.serviceLogs = services.map(service => service.log);
  const fullEvidence = JSON.stringify(evidence, null, 2);
  await writeFile(join(work, "evidence.json"), fullEvidence);
  console.log(`Evidence: ${work}/evidence.json; sha256=${sha256(fullEvidence)}; owned native/browser/listener processes stopped.`);
  // Keep complete evidence on disk and reduce trace output by omitting repeated
  // plan inventories and duplicated page text.
  console.log(JSON.stringify({ ...evidence, page: undefined,
    sessions: evidence.sessions?.map(s => ({ id: s.id, verification: s.plan.bundle_verification, mount: s.plan.verified_mount })) }));
}
