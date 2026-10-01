import assert from "node:assert/strict";
import { execFile, spawn } from "node:child_process";
import { randomBytes } from "node:crypto";
import { createServer, request as httpRequest } from "node:http";
import { createServer as createNetServer } from "node:net";
import { mkdtemp, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";

// Browser-platform experiment only: no production SDK, verifier, API or bundles.
// All browser control goes through the installed Aegis CLI. The page runs its
// own assertions and reports observations to its temporary loopback fixture.
const exec = promisify(execFile);
const addr = process.env.BABEL_ORIGIN_AEGIS_ADDR ?? "127.0.0.1:17892";
assert.match(addr, /^127\.0\.0\.1:\d+$/);
assert.ok(![7878, 7879, 17878].includes(Number(addr.split(":")[1])), "dedicated Aegis port required");
const nonce = randomBytes(12).toString("hex");
const requests = [];
const output = await mkdtemp(join(tmpdir(), "babel-bundle-origins-"));
const env = { ...process.env };
for (const key of Object.keys(env)) if (/^(https?|all|no)_proxy$/i.test(key)) delete env[key];
const cli = (...args) => exec("aegis", args, { env, timeout: 25_000, maxBuffer: 4 * 1024 * 1024 });
let runtime, runtimeExit, fixture, config, resolveResult, observations;
const resultPromise = new Promise(resolve => { resolveResult = resolve; });
const files = new Map();

function reply(response, status, type, body, csp) {
  response.writeHead(status, {
    "Content-Type": type, "X-Content-Type-Options": "nosniff",
    "Cache-Control": "no-store", "Referrer-Policy": "no-referrer",
    "Permissions-Policy": "camera=(), microphone=(), geolocation=()",
    "Content-Security-Policy": csp ?? "default-src 'none'; frame-ancestors 'none'; sandbox",
  });
  response.end(body);
}

function gatewayPolicy() {
  return `default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; base-uri 'none'; object-src 'none'; frame-src 'none'; worker-src 'none'; form-action 'none'; frame-ancestors ${config.parent}`;
}

async function route(request, response) {
  const host = request.headers.host;
  const entry = { host, path: request.url, method: request.method,
    cookie: request.headers.cookie ?? "", authorization: request.headers.authorization ?? "", status: 0 };
  requests.push(entry);
  response.on("finish", () => { entry.status = response.statusCode; });
  // Raw exact Host and URL matching: no Host suffix acceptance, query overrides,
  // redirects, aliases, normalized traversal, directory fallback or proxying.
  const origin = `http://${host}`;
  if (origin === config.parent && request.method === "POST" && request.url === "/result") {
    let body = "";
    for await (const chunk of request) {
      body += chunk;
      if (body.length > 128 * 1024) throw new Error("oversized result");
    }
    reply(response, 200, "text/plain", "recorded");
    resolveResult(JSON.parse(body));
    return;
  }
  if (request.method !== "GET") return reply(response, 405, "text/plain", "method denied");
  if (origin === config.parent && request.url === "/parent.html") {
    return reply(response, 200, "text/html", '<!doctype html><meta charset="utf-8"><title>Bundle origin experiment</title><pre id="result">RUNNING</pre><script src="/parent.js"></script>',
      `default-src 'none'; script-src 'self'; connect-src 'self'; frame-src ${config.a} ${config.b} ${config.external}`);
  }
  if (origin === config.parent && request.url === "/parent.js") {
    return reply(response, 200, "text/javascript", `(${parentMain})(${JSON.stringify(config)});`, "default-src 'none'");
  }
  if (origin === config.external && request.url === "/entry.html") {
    return reply(response, 200, "text/html", '<!doctype html><script src="/external.js"></script>', gatewayPolicy());
  }
  if (origin === config.external && request.url === "/external.js") {
    return reply(response, 200, "text/javascript", `(${externalMain})(${JSON.stringify(config)});`, gatewayPolicy());
  }
  const file = [config.a, config.b].includes(origin) && files.get(request.url);
  if (file) return reply(response, 200, file.type, file.body, gatewayPolicy());
  return reply(response, [config.parent, config.a, config.b, config.external].includes(origin) ? 404 : 421,
    "text/plain", "not an executable document");
}

function parentMain(c) {
  const evidence = { parent: { origin: location.origin, secure: isSecureContext }, cases: [], storage: {}, markers: [] };
  const frames = [], ports = [];
  const check = (value, label) => { if (!value) throw new Error(label); };
  const equal = (a, b, label) => check(JSON.stringify(a) === JSON.stringify(b), label + ': ' + JSON.stringify(a));
  const wait = async (predicate, label) => {
    const end = Date.now() + 12_000;
    while (Date.now() < end) {
      const value = predicate();
      if (value) return value;
      await new Promise(resolve => setTimeout(resolve, 20));
    }
    throw new Error('timeout: ' + label);
  };
  const control = type => ({ type: 'babel.surface.' + type, protocol: 'babel.rpc.v1', version: 1 });
  const isControl = (data, type) => data?.type === 'babel.surface.' + type
    && data.protocol === 'babel.rpc.v1' && data.version === 1 && Object.keys(data).length === 3;
  function frame() {
    const f = document.createElement('iframe');
    f.sandbox = 'allow-scripts allow-same-origin';
    document.body.append(f);
    frames.push(f);
    return f;
  }
  function mount(name, expectedOrigin, mode = 'secure') {
    check(expectedOrigin !== location.origin, 'allow-same-origin requires separate origin');
    const f = frame();
    const s = { name, expectedOrigin, mode, offers: [], reports: [], replies: {}, loads: 0,
      dispatches: [], accepted: false, confirmed: false, closed: false, port: null };
    evidence.cases.push(s);
    f.addEventListener('load', () => {
      s.loads++;
      if (s.loads > 1 && s.accepted) { s.closed = true; s.port?.close(); }
    });
    const listener = event => {
      if (event.data?.type === 'fixture.report' && event.source === f.contentWindow) {
        s.reports.push({ origin: event.origin, ...event.data.value });
      }
      if (!isControl(event.data, 'connect') || event.ports.length !== 1) return;
      // Keep unrelated mounted-frame traffic out of this case's instrumentation.
      if (event.source !== f.contentWindow && (!name.startsWith('source-') || event.origin !== expectedOrigin)) return;
      const sourceMatch = event.source === f.contentWindow;
      const originMatch = event.origin === expectedOrigin;
      const reason = !sourceMatch && mode !== 'omit-source' ? 'source'
        : !originMatch && mode !== 'omit-origin' ? 'origin'
          : s.accepted ? 'already-admitted' : s.closed ? 'closed' : 'accepted';
      s.offers.push({ origin: event.origin, sourceMatch, originMatch, reason });
      const port = event.ports[0];
      if (reason !== 'accepted') { port.close(); return; }
      s.accepted = true;
      s.port = port;
      ports.push(port);
      port.onmessage = e => {
        if (!s.confirmed && isControl(e.data, 'confirm')) {
          s.confirmed = true;
          port.postMessage(control('ready'));
        } else if (s.confirmed && e.data?.type === 'fixture.rpc') {
          s.dispatches.push(e.data.id);
          port.postMessage({ type: 'fixture.response', id: e.data.id });
        } else if (s.confirmed && e.data?.type === 'fixture.reply') {
          s.replies[e.data.id] = e.data.value;
        }
      };
      port.start();
      port.postMessage(control('accept'));
    };
    window.addEventListener('message', listener);
    return { f, s, close() { s.port?.close(); window.removeEventListener('message', listener); f.remove(); } };
  }
  let sequence = 0;
  async function command(m, action, payload = {}) {
    const id = String(++sequence);
    m.s.port.postMessage({ type: 'fixture.command', id, action, ...payload });
    const value = await wait(() => m.s.replies[id], action);
    check(!value.error, JSON.stringify(value));
    return value;
  }
  const loaded = async m => {
    await wait(() => m.s.dispatches.includes('ready-rpc') && m.s.loads > 0, m.s.name + ' confirmed RPC');
    equal(m.s.dispatches, ['ready-rpc'], 'pre-confirmation RPC must not dispatch');
  };
  async function main() {
    check(isSecureContext, 'parent secure context');
    document.cookie = 'parent_only=' + c.nonce + '; Path=/; SameSite=Lax';
    const a = mount('mount-a', c.a);
    a.f.src = c.a + '/entry.html';
    await loaded(a);
    const factsA = await command(a, 'facts');
    evidence.markers.push(factsA);
    equal(factsA.origin, c.a, 'mount A origin');
    check(factsA.secure && factsA.domDenied && factsA.frameElementHidden, 'A secure context and parent DOM isolation');
    equal(factsA.cycle, 'a:b:a', 'cyclic relative modules');
    equal(factsA.dynamic, 'dynamic-evaluated', 'nested dynamic import');
    equal(factsA.css, 'rgb(17, 93, 121)', 'relative CSS import applied');
    check(factsA.background.includes('/assets/pixel.png'), 'relative CSS asset URL');
    check(!factsA.inline && factsA.evalBlocked && factsA.externalBlocked && factsA.unknownBlocked,
      'CSP inline/eval/external and inventory controls');
    evidence.storage.aWrite = await command(a, 'storage', { write: 'A' });
    equal(evidence.storage.aWrite.localBefore, null, 'fresh A localStorage');
    equal(evidence.storage.aWrite.idbBefore, null, 'fresh A indexedDB');
    equal(evidence.storage.aWrite.localAfter, 'A', 'A localStorage write');
    equal(evidence.storage.aWrite.idbAfter, 'A', 'A indexedDB write');
    // Embedded SameSite=Lax writes can be blocked in this cross-site parent.
    // Separate top-level probes below require working cookie positive controls.
    check(!evidence.storage.aWrite.cookieAfter.includes('shared_domain='), 'Domain=.localhost rejected on A');
    check(!evidence.storage.aWrite.cookieAfter.includes('parent_only='), 'parent cookie absent from A');
    const b = mount('mount-b', c.b);
    b.f.src = c.b + '/entry.html';
    await loaded(b);
    const factsB = await command(b, 'facts');
    evidence.markers.push(factsB);
    equal(factsB.origin, c.b, 'mount B origin');
    check(factsB.secure && factsB.domDenied, 'B secure context and DOM isolation');
    evidence.storage.bWrite = await command(b, 'storage', { write: 'B' });
    equal(evidence.storage.bWrite.localBefore, null, 'B cannot see A localStorage');
    equal(evidence.storage.bWrite.idbBefore, null, 'B cannot see A indexedDB');
    check(!evidence.storage.bWrite.cookieBefore.includes('mount_only='), 'B cannot see A cookie');
    check(!evidence.storage.bWrite.cookieBefore.includes('shared_domain='), 'B cannot see Domain=.localhost cookie');
    evidence.storage.aRead = await command(a, 'storage');
    equal(evidence.storage.aRead.localBefore, 'A', 'A retains localStorage');
    equal(evidence.storage.aRead.idbBefore, 'A', 'A retains indexedDB');
    check(!document.cookie.includes('mount_only=') && !document.cookie.includes('shared_domain='), 'parent cookie isolation');

    // A same-origin sibling is executable, but cannot offer the designated frame's port.
    for (const mode of ['secure', 'omit-source']) {
      const m = mount('source-' + mode, c.a, mode);
      m.f.src = c.b + '/entry.html';
      await wait(() => m.s.loads > 0 && m.s.reports.length && m.s.offers.length, 'other-origin designated frame loaded');
      equal(m.s.offers[0].reason, 'origin', 'designated frame wrong origin rejected');
      const sibling = frame();
      sibling.src = c.a + '/entry.html';
      const offer = await wait(() => m.s.offers.find(o => !o.sourceMatch), 'sibling offer');
      if (mode === 'secure') {
        equal(offer.reason, 'source', 'source guard rejects actual sibling');
        equal(m.s.dispatches, [], 'sibling dispatch denied');
      } else {
        await wait(() => m.s.dispatches.includes('ready-rpc'), 'source negative control dispatch');
        check(!offer.sourceMatch, 'source negative control must be wrong source');
      }
      sibling.remove(); m.close();
    }

    for (const mode of ['secure', 'omit-origin']) {
      const m = mount('first-external-' + mode, c.a, mode);
      m.f.src = c.external + '/entry.html';
      await wait(() => m.s.reports.some(r => r.marker === 'external-executed') && m.s.offers.length, 'external first document executed and offered');
      if (mode === 'secure') {
        equal(m.s.offers[0].reason, 'origin', 'external first document rejected before any admission');
        check(!m.s.accepted, 'external first document not admitted');
        equal(m.s.dispatches, [], 'external dispatch denied');
      } else {
        await wait(() => m.s.dispatches.includes('external-rpc'), 'origin negative control dispatch');
        check(!m.s.offers[0].originMatch, 'origin negative control must be wrong origin');
      }
      m.close();
    }
    const unknown = mount('first-unknown', c.a);
    unknown.f.src = c.a + '/unknown.html';
    await wait(() => unknown.s.loads > 0, 'unknown document load completed');
    equal(unknown.s.offers, [], 'unknown document cannot offer');
    check(!unknown.s.accepted, 'unknown document not admitted');
    unknown.close();

    // Replacement on the approved origin tests the single-admission latch, then
    // a real external replacement tests the exact-origin guard with the same WindowProxy.
    const before = a.s.offers.length;
    a.f.src = c.a + '/entry.html';
    await wait(() => a.s.offers.length > before && a.s.loads > 1, 'same-origin replacement');
    equal(a.s.offers.at(-1).reason, 'already-admitted', 'replacement entry cannot establish second port');
    equal(a.s.dispatches, ['ready-rpc'], 'replacement cannot dispatch');
    check(a.s.closed, 'replacement load closes old port');
    a.f.src = c.external + '/entry.html';
    await wait(() => a.s.reports.some(r => r.marker === 'external-executed') && a.s.offers.at(-1).origin === c.external,
      'external replacement execution');
    equal(a.s.offers.at(-1).reason, 'origin', 'external replacement rejected');
    check(a.s.offers.at(-1).sourceMatch, 'external replacement preserves frame WindowProxy');
    equal(a.s.dispatches, ['ready-rpc'], 'external replacement cannot dispatch');
    a.close(); b.close();
    evidence.ok = true;
  }
  main().catch(error => { evidence.ok = false; evidence.error = String(error); }).finally(async () => {
    for (const port of ports) port.close();
    for (const f of frames) f.remove();
    for (const s of evidence.cases) delete s.port;
    const text = JSON.stringify(evidence);
    document.querySelector('#result').textContent = evidence.ok ? 'BUNDLE_ORIGIN_PASS\n' + text : 'BUNDLE_ORIGIN_FAIL\n' + text;
    await fetch('/result', { method: 'POST', credentials: 'omit', body: text });
  });
}

function childMain(c, cycle, dynamic) {
  if (parent === window) {
    const action = location.hash.slice(1);
    const cookie = { action, origin: location.origin, secure: isSecureContext, before: document.cookie };
    if (action === 'write-A' || action === 'write-B') {
      const value = action.slice(-1);
      document.cookie = 'mount_only=' + value + '; Path=/; SameSite=Lax';
      document.cookie = 'shared_domain=' + value + '; Domain=.localhost; Path=/; SameSite=Lax';
    }
    cookie.after = document.cookie;
    document.body.textContent = 'BUNDLE_COOKIE_RESULT ' + JSON.stringify(cookie);
    return;
  }
  const violations = [];
  document.addEventListener('securitypolicyviolation', e => violations.push({ directive: e.effectiveDirective, blocked: e.blockedURI }));
  const control = type => ({ type: 'babel.surface.' + type, protocol: 'babel.rpc.v1', version: 1 });
  const channel = new MessageChannel();
  const request = req => new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
  async function storage(write) {
    const opening = indexedDB.open('bundle-origin', 1);
    opening.onupgradeneeded = () => opening.result.createObjectStore('values');
    const db = await request(opening);
    const value = {
      localBefore: localStorage.getItem('mount'),
      idbBefore: (await request(db.transaction('values').objectStore('values').get('mount'))) ?? null,
      cookieBefore: document.cookie,
    };
    if (write) {
      localStorage.setItem('mount', write);
      const tx = db.transaction('values', 'readwrite');
      tx.objectStore('values').put(write, 'mount');
      await new Promise((resolve, reject) => { tx.oncomplete = resolve; tx.onerror = () => reject(tx.error); });
      document.cookie = 'mount_only=' + write + '; Path=/; SameSite=Lax';
      document.cookie = 'shared_domain=' + write + '; Domain=.localhost; Path=/; SameSite=Lax';
    }
    value.localAfter = localStorage.getItem('mount');
    value.idbAfter = (await request(db.transaction('values').objectStore('values').get('mount'))) ?? null;
    value.cookieAfter = document.cookie;
    db.close();
    return value;
  }
  async function facts() {
    let domDenied = false, evalBlocked = false, externalBlocked = false, unknownBlocked = false;
    try { void parent.document.body; } catch (e) { domDenied = e.name === 'SecurityError'; }
    try { (0, eval)('globalThis.fixtureEval = true'); } catch (e) { evalBlocked = e.name === 'EvalError'; }
    try { await import(c.external + '/external.js'); } catch { externalBlocked = true; }
    try { await import('./undeclared.js'); } catch { unknownBlocked = true; }
    await new Promise(resolve => setTimeout(resolve, 30));
    return { origin: location.origin, secure: isSecureContext, cycle, dynamic,
      domDenied, frameElementHidden: frameElement === null, inline: Boolean(globalThis.fixtureInline),
      evalBlocked, externalBlocked, unknownBlocked, violations,
      css: getComputedStyle(document.body).color, background: getComputedStyle(document.body).backgroundImage };
  }
  channel.port1.onmessage = async event => {
    const data = event.data;
    if (data?.type === 'babel.surface.accept') channel.port1.postMessage(control('confirm'));
    if (data?.type === 'babel.surface.ready') channel.port1.postMessage({ type: 'fixture.rpc', id: 'ready-rpc' });
    if (data?.type === 'fixture.command') {
      let value;
      try { value = data.action === 'facts' ? await facts() : await storage(data.write); }
      catch (error) { value = { error: String(error) }; }
      channel.port1.postMessage({ type: 'fixture.reply', id: data.id, value });
    }
  };
  parent.postMessage(control('connect'), c.parent, [channel.port2]);
  channel.port1.postMessage({ type: 'fixture.rpc', id: 'too-early' });
  parent.postMessage({ type: 'fixture.report', value: { marker: 'modules-evaluated', cycle, dynamic, origin: location.origin } }, c.parent);
}

function externalMain(c) {
  const channel = new MessageChannel();
  const control = type => ({ type: 'babel.surface.' + type, protocol: 'babel.rpc.v1', version: 1 });
  channel.port1.onmessage = e => {
    if (e.data?.type === 'babel.surface.accept') channel.port1.postMessage(control('confirm'));
    if (e.data?.type === 'babel.surface.ready') channel.port1.postMessage({ type: 'fixture.rpc', id: 'external-rpc' });
  };
  parent.postMessage(control('connect'), c.parent, [channel.port2]);
  parent.postMessage({ type: 'fixture.report', value: { marker: 'external-executed' } }, c.parent);
}

async function rawGet(host, path) {
  return new Promise((resolve, reject) => {
    const request = httpRequest({ hostname: '127.0.0.1', port: fixture.address().port, path,
      headers: { Host: host }, agent: false }, response => {
      let body = '';
      response.on('data', data => { body += data; });
      response.on('end', () => resolve({ status: response.statusCode, headers: response.headers, body }));
    });
    request.on('error', reject);
    request.end();
  });
}

async function deadline(promise, ms, label) {
  let timer;
  try { return await Promise.race([promise, new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(label)), ms);
  })]); } finally { clearTimeout(timer); }
}

try {
  await exec('sh', ['-c', 'command -v aegis'], { env });
  await cli('--help');
  await cli('serve', '--help');
  const version = (await cli('version')).stdout.trim();
  // Refuse to attach to or terminate another agent's runtime.
  const probe = createNetServer();
  await new Promise((resolve, reject) => { probe.once('error', reject); probe.listen(Number(addr.split(':')[1]), '127.0.0.1', resolve); });
  await new Promise(resolve => probe.close(resolve));
  fixture = createServer((req, res) => { route(req, res).catch(error => { res.destroy(error); resolveResult({ ok: false, error: String(error) }); }); });
  await new Promise(resolve => fixture.listen(0, '127.0.0.1', resolve));
  const port = fixture.address().port;
  config = { nonce, parent: `http://127.0.0.1:${port}`, a: `http://a-${nonce}.localhost:${port}`,
    b: `http://b-${nonce}.localhost:${port}`, external: `http://external-${nonce}.localhost:${port}` };
  const add = (path, type, body) => files.set(path, { type, body });
  add('/entry.html', 'text/html', '<!doctype html><meta charset="utf-8"><link rel="stylesheet" href="./styles/main.css"><body>Bundle fixture<script>globalThis.fixtureInline=true</script><script type="module" src="./modules/main.js"></script>');
  add('/modules/main.js', 'text/javascript', `import { cycle } from './cycle/a.js';\nconst { dynamic } = await import('./nested/dynamic.js');\n(${childMain})(${JSON.stringify(config)}, cycle(), dynamic);`);
  add('/modules/cycle/a.js', 'text/javascript', "import { b } from './b.js'; export const a = () => 'a'; export const cycle = () => a() + ':' + b();");
  add('/modules/cycle/b.js', 'text/javascript', "import { a } from './a.js'; export const b = () => 'b:' + a();");
  add('/modules/nested/dynamic.js', 'text/javascript', "export { dynamic } from '../shared.js';");
  add('/modules/shared.js', 'text/javascript', "export const dynamic = 'dynamic-evaluated';");
  add('/styles/main.css', 'text/css', '@import "./nested/theme.css"; body { background-image: url("../assets/pixel.png"); }');
  add('/styles/nested/theme.css', 'text/css', 'body { color: rgb(17, 93, 121); }');
  add('/assets/pixel.png', 'image/png', Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=', 'base64'));
  const invalid = [
    [`unknown-${nonce}.localhost:${port}`, '/entry.html', 421],
    [`a-${nonce}.localhost.evil:${port}`, '/entry.html', 421],
    [`a-${nonce}.localhost`, '/entry.html', 421],
    [`a-${nonce}.localhost:${port}`, '/unknown.html', 404],
    [`a-${nonce}.localhost:${port}`, '/entry.html?mime=text/javascript', 404],
    [`a-${nonce}.localhost:${port}`, '/%2e%2e/parent.js', 404],
    [`a-${nonce}.localhost:${port}`, '/modules/../entry.html', 404],
    [`a-${nonce}.localhost:${port}`, '//entry.html', 404],
    [`a-${nonce}.localhost:${port}`, '/api', 404],
  ];
  for (const [host, path, status] of invalid) {
    const r = await rawGet(host, path);
    assert.equal(r.status, status);
    assert.equal(r.headers['content-type'], 'text/plain');
    assert.equal(r.headers['x-content-type-options'], 'nosniff');
    assert.match(r.headers['content-security-policy'], /default-src 'none'/);
    assert.equal(r.headers.location, undefined);
    assert.equal(r.body, 'not an executable document');
  }
  let log = '';
  runtime = spawn('aegis', ['--mode', 'headless', 'serve', '--addr', addr], { env, stdio: ['ignore', 'pipe', 'pipe'] });
  runtimeExit = new Promise(resolve => { runtime.once('exit', (code, signal) => resolve({ code, signal })); });
  runtime.stdout.on('data', data => { log += data; });
  runtime.stderr.on('data', data => { log += data; });
  runtime.on('error', error => resolveResult({ ok: false, error: String(error) }));
  const readyUntil = Date.now() + 25_000;
  while (true) {
    try { await cli('--server-addr', addr, 'page', 'inspect'); break; }
    catch (error) {
      if (runtime.exitCode !== null || Date.now() > readyUntil) throw new Error(`Aegis startup failed: ${error}\n${log}`);
      await new Promise(resolve => setTimeout(resolve, 150));
    }
  }
  await cli('--server-addr', addr, 'navigate', config.parent + '/parent.html');
  const result = await deadline(resultPromise, 90_000, 'browser result deadline');
  const page = (await cli('--server-addr', addr, 'page', 'text', '--scope', 'full')).stdout;
  observations = { version, addr, fixture: config, result, requests, page, cookies: [] };
  async function cookies(origin, action) {
    await cli('--server-addr', addr, 'navigate', origin + '/entry.html#' + action);
    const end = Date.now() + 12_000;
    while (Date.now() < end) {
      const text = (await cli('--server-addr', addr, 'page', 'text', '--scope', 'full')).stdout.trim();
      if (text.startsWith('BUNDLE_COOKIE_RESULT ')) {
        const value = JSON.parse(text.slice('BUNDLE_COOKIE_RESULT '.length));
        if (value.origin === origin && value.action === action) {
          observations.cookies.push(value);
          return value;
        }
      }
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw new Error('cookie observation deadline');
  }
  if (result.ok) {
    const a = await cookies(config.a, 'write-A');
    assert.ok(a.after.includes('mount_only=A'), 'top-level A cookie positive control');
    assert.ok(!a.after.includes('shared_domain='), 'A rejects Domain=.localhost');
    const b = await cookies(config.b, 'write-B');
    assert.ok(!b.before.includes('mount_only=') && !b.before.includes('shared_domain='), 'B cannot read A cookies');
    assert.ok(b.after.includes('mount_only=B'), 'top-level B cookie positive control');
    assert.ok(!b.after.includes('shared_domain='), 'B rejects Domain=.localhost');
    const reread = await cookies(config.a, 'read-A');
    assert.ok(reread.before.includes('mount_only=A') && !reread.before.includes('mount_only=B'), 'A cookie persists after B writes');
    const rereadB = await cookies(config.b, 'read-B');
    assert.equal(rereadB.before, 'mount_only=B', 'B cookie persists after A revisit');
  }
  console.log(JSON.stringify(observations, null, 2));
  await writeFile(join(output, 'observations.json'), JSON.stringify(observations, null, 2));
  assert.equal(result.ok, true, JSON.stringify(result));
  assert.match(page, /BUNDLE_ORIGIN_PASS/);
  for (const origin of [config.a, config.b]) {
    const host = new URL(origin).host;
    for (const path of files.keys()) assert.ok(requests.some(r => r.host === host && r.path === path && r.status === 200), `${host}${path} actually requested`);
    assert.ok(requests.some(r => r.host === host && r.path === '/modules/undeclared.js' && r.status === 404));
  }
  assert.ok(requests.every(r => !r.authorization), 'no account credentials');
  assert.ok(requests.every(r => !r.cookie.includes('shared_domain=')), 'Domain=.localhost cookie never sent across mounts');
  for (const origin of [config.a, config.b]) {
    const expected = origin === config.a ? 'A' : 'B';
    const own = requests.filter(r => r.host === new URL(origin).host);
    assert.ok(own.some(r => r.path === '/entry.html' && r.cookie === `mount_only=${expected}`), 'own cookie actually sent on top-level revisit');
    assert.ok(own.every(r => !r.cookie.includes('parent_only=')));
    assert.ok(own.every(r => !r.cookie.includes(`mount_only=${expected === 'A' ? 'B' : 'A'}`)));
  }
  assert.equal(requests.filter(r => r.host === new URL(config.external).host && r.path === '/external.js').length, 3,
    'only three deliberate external documents request the hostile script; CSP imports issue no network request');
  console.log(`BUNDLE_ORIGIN_PASS: random *.localhost, same listener, exact origin/source, isolated storage, CSP, finite routing. Evidence: ${output}`);
} catch (error) {
  if (observations) {
    await writeFile(join(output, 'observations.json'), JSON.stringify(observations, null, 2));
    console.error(JSON.stringify(observations, null, 2));
  }
  console.error(JSON.stringify({ failure: String(error), config, requests, evidence: output }, null, 2));
  throw error;
} finally {
  if (runtime && runtime.exitCode === null) {
    runtime.kill('SIGTERM');
    try { await deadline(runtimeExit, 5000, 'Aegis shutdown'); }
    catch { runtime.kill('SIGKILL'); await runtimeExit; }
  }
  if (fixture?.listening) {
    fixture.closeAllConnections();
    await new Promise(resolve => fixture.close(resolve));
  }
  // Keep evidence on failures and successes. No shared profile or existing
  // runtime is removed; random origin names prevent state reuse between runs.
  console.log(`Owned Aegis runtime ${addr} and temporary HTTP fixture stopped.`);
}
