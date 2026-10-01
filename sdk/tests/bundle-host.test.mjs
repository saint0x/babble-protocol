import assert from "node:assert/strict";
import test, { afterEach } from "node:test";
import { BrowserSurfaceHost, objectBinding, surfaceBridgeControl } from "../dist/index.js";

const origin = "https://session.bundle.test";
const hostOrigin = "https://host.test";
const manifestHash = "a".repeat(64);
const entryHash = "b".repeat(64);
const mounts = [];
afterEach(() => { for (const mount of mounts.splice(0)) mount.unmount(); });

test("verified bundles mount the gateway entry with exact origin and script/same-origin sandbox", async () => {
  const h = harness();
  const plan = bundlePlan();
  const mounted = mount(h, plan);
  assert.equal(h.frame.src, `${origin}/app/index.html`);
  assert.equal(plan.surface.entry, `babble://blobs/${entryHash}`);
  assert.deepEqual([...h.frame.sandbox.tokens].sort(), ["allow-same-origin", "allow-scripts"]);
  assert.equal(h.frame.attributes.credentialless, "");
  assert.equal(h.frame.attributes.csp, undefined);
  assert.equal(mounted.surfaceOrigin, origin);
  const child = offer(h, origin);
  assert.equal(child.messages[0].type, "babble.surface.accept");
  child.postMessage(request("before-confirm"));
  assert.equal(h.requests.length, 0);
  child.postMessage(surfaceBridgeControl("confirm"));
  await mounted.ready;
  child.postMessage(request("after-confirm"));
  await Promise.resolve();
  assert.equal(h.requests.length, 1);
  assert.equal(h.requests[0].binding.origin, origin);
  assert.equal(h.requests[0].binding.object_id, "obj_surface");
  assert.equal(h.requests[0].binding.surface_session_id, "surface_session_1");
});

test("verified bundle channel rejects null, another origin, and another window without losing admission", async () => {
  const h = harness();
  const mounted = mount(h);
  for (const wrongOrigin of ["null", hostOrigin, "https://other.bundle.test", `${origin}:444`, "*"]) {
    const child = offer(h, wrongOrigin);
    assert.equal(child.peer.closed, true, wrongOrigin);
    assert.equal(child.messages.length, 0);
  }
  const wrongWindow = offer(h, origin, {});
  assert.equal(wrongWindow.messages.length, 0);
  const child = offer(h, origin);
  child.postMessage(surfaceBridgeControl("confirm"));
  await mounted.ready;
  const replacement = offer(h, origin);
  assert.equal(replacement.peer.closed, true);
  replacement.postMessage(request("replacement"));
  child.postMessage(request("original"));
  assert.deepEqual(h.requests.map((value) => value.id), ["original"]);
});

test("verified bundle navigation revokes the admitted port and suppresses outstanding responses", async () => {
  const h = harness();
  let resolve;
  h.options.dispatch = (value, context) => {
    h.requests.push({ value, context });
    return new Promise((done) => { resolve = done; });
  };
  const mounted = mount(h);
  h.frame.dispatchEvent(new Event("load"));
  const child = offer(h, origin);
  child.postMessage(surfaceBridgeControl("confirm"));
  await mounted.ready;
  child.postMessage(request("pending"));
  assert.equal(h.requests.length, 1);
  h.frame.dispatchEvent(new Event("load"));
  assert.equal(mounted.lifecycle.state, "evicted");
  assert.equal(h.frame.removed, true);
  assert.equal(child.peer.closed, true);
  assert.equal(h.requests[0].context.signal.aborted, true);
  resolve(response(h.requests[0].value));
  await Promise.resolve();
  assert.equal(child.messages.filter((value) => value.type === "babble.rpc.response").length, 0);
  assert.equal(h.window.listeners.size, 0);
});

test("bundle dispatch bindings stay pinned after caller mutates the original mount options", async () => {
  const h = harness();
  const plan = bundlePlan();
  h.options.plan = plan;
  const mounted = new BrowserSurfaceHost().mount(h.options);
  mounts.push(mounted);
  const child = offer(h, origin);
  child.postMessage(surfaceBridgeControl("confirm"));
  await mounted.ready;
  plan.object_id = "obj_replacement";
  h.options.surfaceSessionId = "surface_session_replacement";
  h.options.currentIdentityId = "id_replacement";
  child.postMessage(request("pinned"));
  assert.equal(h.requests[0].binding.object_id, "obj_surface");
  assert.equal(h.requests[0].binding.surface_session_id, "surface_session_1");
  assert.equal(h.requests[0].binding.identity_id, null);
});

test("bundles require both verified fields with every binding and supported version", () => {
  const mutations = [
    (p) => { delete p.bundle_verification; },
    (p) => { delete p.verified_mount; },
    (p) => { p.bundle_verification.policy_version = 2; },
    (p) => { p.bundle_verification.policy_version = "1"; },
    (p) => { p.verified_mount.version = "1"; },
    (p) => { p.verified_mount.version = 2; },
    (p) => { p.verified_mount.session_id = "surface_session_other"; },
    (p) => { p.verified_mount.session_id = null; },
    (p) => { p.verified_mount.object_id = "obj_other"; },
    (p) => { p.verified_mount.object_id = 1; },
    (p) => { p.verified_mount.role = "Expanded"; },
    (p) => { p.verified_mount.role = {}; },
    (p) => { p.verified_mount.manifest_hash = "c".repeat(64); },
    (p) => { p.bundle_verification.manifest_hash = "c".repeat(64); },
    (p) => { p.verified_mount.manifest_hash = p.bundle_verification.manifest_hash = "A".repeat(64); },
    (p) => { p.verified_mount.manifest_hash = p.bundle_verification.manifest_hash = {}; },
    (p) => { p.verified_mount.manifest_hash = p.bundle_verification.manifest_hash = "short"; },
    (p) => { p.surface.bundle = null; },
    (p) => { p.surface.target = "Static"; },
    (p) => { p.surface.role = p.verified_mount.role = "unknown"; },
    (p) => { p.object_id = p.verified_mount.object_id = ""; },
  ];
  for (const field of ["bundle_verification", "verified_mount"]) {
    for (const value of [null, undefined, false, 1, "verified", [], {}]) {
      mutations.push((p) => { p[field] = value; });
    }
  }
  for (const mutate of mutations) rejected(mutate);
  for (const id of ["", null, {}, 1, "with space"]) rejected(() => {}, { surfaceSessionId: id });
  rejected((p) => {
    delete p.bundle_verification;
    delete p.verified_mount;
    p.surface.entry = "https://unverified.test/legacy.html";
    p.surface.bundle.files[0].source_uri = p.surface.entry;
  });
});

test("gateway URLs reject opaque, noncanonical, host and insecure assignments", () => {
  for (const value of [
    null, 1, {}, "", "null", "*", "data:text/html,ok", "blob:https://other.test/id", "file:///tmp/index.html",
    "javascript:alert(1)", "https://session.bundle.test/", "https://SESSION.bundle.test", "https://session.bundle.test:443",
    "https://user:pass@session.bundle.test", "https://session.bundle.test?", "https://session.bundle.test#",
    "https://session.bundle.test/path", " https://session.bundle.test", "https://session.bundle.test\n",
    "https://session.bundle.test.", "https://session%2ebundle.test", "https:\\session.bundle.test",
    hostOrigin, `${hostOrigin}:444`, "http://host.test", "http://session.bundle.test", "http://localhost:8030",
    "http://127.0.0.1:8030", "http://[::1]:8030", "http://session.localhost.evil.test", "http://bad_label.localhost",
  ]) {
    rejected((p) => { p.verified_mount.origin = value; p.verified_mount.entry_url = `${value}/app/index.html`; });
  }
  rejected(() => {}, { surfaceOrigin: "https://override.test" });
  rejected(() => {}, { surfaceOrigin: "null" });
  rejected(() => {}, { hostOrigin: "https://invented-host.test" });
});

test("gateway entry must be the exact declared logical document on the assigned origin", () => {
  for (const value of [
    null, 1, {}, "", "app/index.html", "//session.bundle.test/app/index.html", `${origin}/other.html`,
    `${hostOrigin}/app/index.html`, `${origin}/app/../app/index.html`, `${origin}/app/%69ndex.html`,
    `${origin}/app/index.html?q=1`, `${origin}/app/index.html#fragment`, `${origin}/app/index.html?`,
    `${origin}/app/index.html#`, `${origin}/app//index.html`, `${origin}/app/index.html/`,
    "https://user@session.bundle.test/app/index.html", ` ${origin}/app/index.html`,
  ]) rejected((p) => { p.verified_mount.entry_url = value; });
  for (const value of [null, 1, {}, false, [], { version: 1, files: [], entry_path: "app/index.html" }]) {
    rejected((p) => { p.surface.bundle = value; });
  }
  for (const value of ["../index.html", "/index.html", "app//index.html", "app/%69ndex.html", "app/index.html?x", "app\\index.html"]) {
    rejected((p) => {
      p.surface.bundle.entry_path = p.surface.bundle.files[0].path = value;
      p.verified_mount.entry_url = `${origin}/${value}`;
    });
  }
  rejected((p) => { p.surface.bundle.files.push({ ...p.surface.bundle.files[0] }); });
  rejected((p) => { p.surface.bundle.files[0].integrity = "c".repeat(64); });
  rejected((p) => { p.surface.bundle.files[0].source_uri = "https://different.test/entry"; });
  rejected((p) => { p.surface.bundle.files[0].kind = "script"; });
  rejected((p) => { p.surface.bundle.files[0].media_type = "text/javascript"; });
});

test("only the verified gateway admits same-origin scripts and local HTTP requires *.localhost", async () => {
  for (const gateway of [origin, `http://m-${"a".repeat(48)}.localhost:8030`, "http://session-123.localhost:8030", "https://session-123.localhost:8030"]) {
    const h = harness();
    const plan = bundlePlan();
    plan.verified_mount.origin = gateway;
    plan.verified_mount.entry_url = `${gateway}/app/index.html`;
    const mounted = mount(h, plan, { surfaceOrigin: gateway });
    const child = offer(h, gateway);
    child.postMessage(surfaceBridgeControl("confirm"));
    await mounted.ready;
    assert.equal(mounted.surfaceOrigin, gateway);
  }
  for (const field of ["isolated_origin", "capability_bridge"]) rejected((p) => { p.sandbox[field] = false; });
  for (const field of ["host_cookies", "top_navigation"]) rejected((p) => { p.sandbox[field] = true; });
  rejected((p) => { p.sandbox.iframe_sandbox = "allow-scripts allow-same-origin allow-top-navigation"; });
  for (const admission of ["blocked", "needs_permission"]) rejected((p) => { p.admission = admission; }, {}, /admission status/);
  for (const lifecycle of ["suspended", "evicted"]) rejected((p) => { p.lifecycle = lifecycle; }, {}, /stopped lifecycle/);
});

function rejected(mutate, overrides = {}, error = /verified bundle execution gateway|capability bridge/) {
  const h = harness();
  const plan = bundlePlan();
  mutate(plan);
  assert.throws(() => mount(h, plan, overrides), error);
  assert.equal(h.container.children.length, 0);
  assert.equal(h.created, 0);
  assert.equal(h.window.listeners.size, 0);
  assert.equal(h.requests.length, 0);
}

function bundlePlan() {
  return {
    object_id: "obj_surface", admission: "ready", lifecycle: "cold", blocked_reasons: [], capability_decisions: [],
    surface: { role: "Feed", target: "Web", entry: `babble://blobs/${entryHash}`, integrity: entryHash,
      bundle: { version: 1, entry_path: "app/index.html", files: [{ path: "app/index.html", source_uri: `babble://blobs/${entryHash}`,
        integrity: entryHash, size_bytes: 100, media_type: "text/html", kind: "document" }] } },
    bundle_verification: { policy_version: 1, manifest_hash: manifestHash },
    verified_mount: { version: 1, session_id: "surface_session_1", object_id: "obj_surface", role: "Feed",
      manifest_hash: manifestHash, origin, entry_url: `${origin}/app/index.html` },
    sandbox: { isolated_origin: true, host_cookies: false, top_navigation: false, capability_bridge: true,
      iframe_sandbox: "allow-scripts allow-same-origin",
      csp: "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'none'", wasi_filesystem: false, wasi_network: false },
    budget: { memory_bytes: 33554432, cpu_ms_per_minute: 1500, gpu_expected: false, network_bytes_per_minute: 524288,
      persistent_storage_bytes: 1048576, realtime_connections: 1, background_eligible: false },
  };
}

function harness() {
  const h = { frame: new Frame(), container: { children: [], appendChild(frame) { this.children.push(frame); return frame; } },
    window: { location: { origin: hostOrigin }, listeners: new Set(),
      addEventListener(_type, fn) { this.listeners.add(fn); }, removeEventListener(_type, fn) { this.listeners.delete(fn); } },
    created: 0, requests: [] };
  h.options = { container: h.container, window: h.window, document: { createElement() { h.created++; return h.frame; } },
    registerDocument: async () => {},
    hostOrigin, surfaceSessionId: "surface_session_1", dispatch(value) { h.requests.push(value); return response(value); } };
  return h;
}

function mount(h, plan = bundlePlan(), overrides = {}) {
  const mounted = new BrowserSurfaceHost().mount({ ...h.options, plan, ...overrides });
  mounts.push(mounted);
  return mounted;
}

function response(value) { return { protocol: "babble.rpc.v1", id: value.id, result: { results: [] }, error: null, trace_id: null }; }
function request(id) {
  return { type: "babble.rpc.request", protocol: "babble.rpc.v1", envelope: { protocol: "babble.rpc.v1", id,
    method: "babble.search.objects.v1", payload: { q: "babble", limit: 3, author: null, kind: null },
    binding: objectBinding({ objectId: "obj_spoofed", surfaceSessionId: "surface_session_spoofed", origin: "null", runtimeId: "test", capabilityGrants: [] }),
    idempotency_key: null, deadline: { timeout_ms: 30000, client_started_at: "2026-09-30T00:00:00Z" }, trace_id: null } };
}

class Frame extends EventTarget {
  src = "";
  attributes = {};
  removed = false;
  contentWindow = { postMessage() { throw new Error("window reply is forbidden"); } };
  sandbox = { tokens: new Set(), add(token) { this.tokens.add(token); } };
  setAttribute(key, value) { this.attributes[key] = value; }
  remove() { this.removed = true; }
}

class Port {
  closed = false;
  messages = [];
  listeners = new Map([ ["message", new Set()], ["messageerror", new Set()], ["close", new Set()] ]);
  addEventListener(type, fn) { this.listeners.get(type).add(fn); }
  removeEventListener(type, fn) { this.listeners.get(type).delete(fn); }
  start() {}
  close() { this.closed = true; }
  postMessage(data) {
    if (this.closed || this.peer.closed) return;
    this.peer.messages.push(data);
    for (const fn of this.peer.listeners.get("message")) fn({ data });
  }
}

function offer(h, messageOrigin, source = h.frame.contentWindow) {
  const child = new Port();
  const host = new Port();
  child.peer = host;
  host.peer = child;
  for (const fn of h.window.listeners) fn({ origin: messageOrigin, source, data: surfaceBridgeControl("connect"), ports: [host] });
  return child;
}
