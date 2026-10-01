import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createHash, webcrypto } from "node:crypto";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import * as sdk from "@babel-protocol/sdk";

const source = (name) => readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8");
const transpile = (code) => ts.transpileModule(code, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;
const globals = { URL, File, Request, Response, Headers, EventTarget, Event, TextEncoder, TextDecoder, Uint8Array,
  crypto: webcrypto, fetch, AbortController, AbortSignal, structuredClone, console, Date, Error };
const main = ts.createSourceFile("main.ts", source("main"), ts.ScriptTarget.Latest, true);
const handlers = new Set(["publishFromComposer", "startPublishComposer", "startSocialTextComposer", "saveComposerDraft",
  "renderComposerDraft", "toggleComposer", "syncAccount", "publishMediaWithAuthor", "selectedMediaFiles", "mediaTitle", "mediaDescription"]);
const handlerSource = main.statements.filter(node => ts.isFunctionDeclaration(node) && handlers.has(node.name?.text))
  .map(node => node.getText(main)).join("\n");
const mainText = source("main");
const listeners = mainText.slice(mainText.indexOf('composeForm.addEventListener("submit"'), mainText.indexOf("for (const button of lensButtons)"));
const plain = (value) => JSON.parse(JSON.stringify(value));

function harness(options = {}) {
  let activeElement;
  class Element extends EventTarget {
    value = ""; textContent = ""; disabled = false; hidden = false; isConnected = true;
    files = null; style = {}; dataset = {}; attributes = new Map(); children = [];
    scrollHeight = 176; nodes = new Map();
    paused = true; pauseCount = 0; loadCount = 0; readyState = 0; error = null; playable = "probably";
    get src() { return this.getAttribute("src") ?? ""; }
    set src(value) { this.setAttribute("src", value); }
    hasAttribute(name) { return this.attributes.has(name); }
    pause() { this.paused = true; this.pauseCount++; }
    load() { this.loadCount++; this.readyState = 0; this.error = null; }
    canPlayType() { return this.playable; }
    querySelector(selector) {
      if (this.nodes.has(selector)) return this.nodes.get(selector);
      const match = selector.match(/^\[([^=\]]+)(?:=['"]?([^'"\]]+)['"]?)?\]$/);
      for (const child of this.children) {
        if (match && child.hasAttribute(match[1]) && (match[2] === undefined || child.getAttribute(match[1]) === match[2])) return child;
        const found = child.querySelector(selector);
        if (found) return found;
      }
      for (const child of this.nodes.values()) {
        if (child === this || child.nodes === this.nodes) continue;
        const found = child.querySelector(selector);
        if (found) return found;
      }
      return null;
    }
    setAttribute(name, value) { this.attributes.set(name, String(value)); }
    getAttribute(name) { return this.attributes.get(name) ?? null; }
    removeAttribute(name) { this.attributes.delete(name); }
    replaceChildren(...children) { this.children = children; }
    append(...children) { this.children.push(...children); }
    closest(selector) { return selector === "label" ? this.label : null; }
    contains(node) { return node === this || [...this.nodes.values()].includes(node); }
    focus() { activeElement = this; }
    scrollIntoView(options) { this.lastScroll = options; }
    emit(type, data = {}) { const event = new Event(type, { cancelable: true }); Object.assign(event, data); this.dispatchEvent(event); return event; }
  }
  const root = new Element(); root.hidden = true;
  const names = ["compose-form", "compose-text", "compose-media", "compose-submit", "author-handle", "author-status",
    "compose-title", "compose-avatar", "compose-sign-in", "compose-context", "compose-attachments", "compose-preview",
    "compose-preview-image", "compose-preview-audio", "compose-preview-video", "compose-preview-status", "remove-compose-media", "compose-image-icon", "compose-bundle-icon",
    "close-composer", "compose-media-name", "compose-media-size", "compose-bundle", "bundle-entry", "clear-bundle", "bundle-status", "bundle-picker",
    "bundle-permissions", "bundle-capabilities", "bundle-permission-status", "bundle-permission-count", "bundle-clipboard", "bundle-fullscreen"];
  for (const name of names) root.nodes.set(`[data-${name}]`, new Element());
  root.nodes.set("[data-composer-panel]", root);
  const el = (name) => root.querySelector(`[data-${name}]`);
  el("bundle-picker").nodes = root.nodes;
  el("bundle-entry").label = new Element();
  const opener = new Element(); opener.focus();
  const document = Object.assign(new EventTarget(), { hidden: false,
    querySelector: (selector) => root.querySelector(selector), createElement: () => new Element() });
  Object.defineProperty(document, "activeElement", { get: () => activeElement });
  const created = [], revoked = [];
  class PreviewURL extends URL {
    static createObjectURL(file) {
      if (options.failPreview) throw new Error("Preview storage unavailable");
      const url = `blob:composer-${created.length}`; created.push({ file, url }); return url;
    }
    static revokeObjectURL(url) { revoked.push(url); }
  }
  const icons = { createElement: () => new Element(), X: [], AppWindow: [], ImagePlus: [], ArrowLeft: [], ArrowRight: [], FileImage: [], Film: [], Music2: [] };
  const module = (name, dependencies = {}) => {
    const exports = {};
    vm.runInNewContext(transpile(source(name)), { ...globals, URL: PreviewURL, document, HTMLElement: Element, exports,
      require(id) { assert.ok(id in dependencies, `Unexpected dependency ${id}`); return dependencies[id]; } });
    return exports;
  };
  const { Drafts, draftTransport } = module("drafts");
  const mediaKinds = module("media-kind");
  const composer = module("composer", { lucide: icons, "./media-kind": mediaKinds });
  const bundlePublication = module("bundle-publication", { "@babel-protocol/sdk": sdk });
  const { BundlePicker } = module("bundle-picker", { lucide: icons, "./bundle-publication": bundlePublication });
  const { BabelFrontendClient } = module("protocol", { "@babel-protocol/sdk": sdk, "./profile-response": module("profile-response"),
    "./invocations": module("invocations", { "@babel-protocol/sdk": sdk }),
    "./browser-invocations": module("browser-invocations", { "@babel-protocol/sdk": sdk }),
    "./media-kind": mediaKinds, "./media-resource": module("media-resource", { "./media-kind": mediaKinds }) });
  const stored = new Map();
  const storage = { getItem: (key) => stored.get(key) ?? null, setItem: (key, value) => stored.set(key, value), removeItem: (key) => stored.delete(key) };
  const requests = [], httpRequests = [], navigations = [], refreshed = [];
  const h = { ...globals, ...composer, ...bundlePublication, document, window: new Element(),
    feedPreferences: { account() {} },
    safetyControls: { account() {}, ensure: async () => null }, moderationControls: { account() {} },
    safetyUnavailable: false, ensureFeedSafety: async () => {},
    drafts: new Drafts({ origin: "https://babel.test", identityId: "alice" }, storage), draftTransport, BabelFrontendClient,
    composerPanel: root, composeForm: el("compose-form"), composeText: el("compose-text"), composeMedia: el("compose-media"),
    composeSubmit: el("compose-submit"), authorHandleInput: el("author-handle"),
    accounts: { origin: new URL("https://babel.test"), current: { identity: { id: "alice", handle: "Alice" }, token: "session-a" } },
    accountPanel: { open() { h.loginOpened = true; } },
    author: { identityId: "alice", handle: "Alice" }, profileDropdown: null, toggleProfileButton: null,
    profiles: { close() {} }, objectVisits: { clear() {} }, quotes: { clear() {}, refresh() {} }, followingDirty: false, followingControls: { account() {} }, followingFeed: { clear() {} }, profileReturn: null,
    reactions: { select() {} }, deck: { replaceChildren() {} }, accountFeedRefresh: Promise.resolve(),
    searchInput: { value: "" }, cards: [], currentIndex: 0, loadSequence: 0, seenThisSession: new Set(),
    setAnimatedVisibility(element, open) { element.hidden = !open; }, isHidden: (element) => element.hidden,
    toggleProfile() {}, toggleSettings() {}, toggleHelp() {}, closeSurface() {},
    setAuthorStatus(message, state) { el("author-status").textContent = message; el("author-status").dataset.state = state; },
    compactId: (id) => id, errorMessage: (error) => error.message,
    loadFeed: async (...args) => { navigations.push(args); },
    client: { describeObject: async object => object, objectToCard: async ({ object }) => object },
    conversations: { clear() {}, refresh: async (...args) => { refreshed.push(args); } },
    DataTransfer: class {
      values = [];
      items = { add: file => this.values.push(file) };
      get files() { return Object.assign([...this.values], { item: index => this.values[index] ?? null }); }
    },
  };
  h.composerView = new composer.ComposerView(root, files => {
    h.drafts.edit(h.composeText.value, files); h.renderComposerDraft();
  });
  h.bundlePicker = new BundlePicker(el("bundle-picker"), (value) => {
    h.saveComposerDraft(); h.drafts.editBundle(value); h.renderComposerDraft();
  });
  const server = socialFixture(h, requests, httpRequests);
  h.accounts.fetch = server.fetch;
  vm.createContext(h); vm.runInContext(transpile(handlerSource + "\n" + listeners), h);
  h.type = (text) => { h.composeText.value = text; h.composeText.emit("input"); };
  h.select = (name, files) => { el(name).files = Object.assign([...files], { item: index => files[index] ?? null }); el(name).emit("change"); };
  h.changeAccount = (id) => { h.accounts.current = id ? { identity: { id, handle: id }, token: `session-${id}` } : null; h.syncAccount(); };
  h.syncAccount();
  return { h, el, root, opener, created, revoked, stored, requests, httpRequests, server, navigations, refreshed, document };
}

// Exercise the real InvocationApi against a stateful HTTP boundary, including immutable retries.
function socialFixture(h, requests, httpRequests) {
  const documents = new Map(), byKey = new Map(), byId = new Map(), effects = [];
  const digest = value => createHash("sha256").update(sdk.canonicalValueBytes(value)).digest("hex");
  const response = value => Response.json(structuredClone(value));
  return { effects, byKey, async fetch(url, init) {
    const path = new URL(url).pathname, body = init.body === undefined ? undefined : JSON.parse(init.body);
    const session = h.accounts.current, headers = new Headers(init.headers);
    const route = { path, method: init.method, body, headers, token: session?.token, action: path.split("/").at(-1) };
    httpRequests.push(route);
    if (path === "/rpc" || path === "/invocations/v1/prepare") requests.push({ ...body, token: session?.token });
    if (h.respond) await h.respond(body, route);
    if (path === "/rpc") {
      assert.equal(init.method, "POST");
      assert.notEqual(body.method, "babel.capabilities.grant.v1", "social consent must never request a reusable grant");
      assert.ok(!/^babel\.social\.(follow|unfollow|share|reply)\.v/.test(body.method), "host social actions use invocation routes");
      const result = body.method === "babel.media.blob.put.v1" ? { blob: blobReceipt(body.payload) }
        : { object: { id: body.method === "babel.object.publish.v1" ? "controller" : "published" } };
      return response({ protocol: body.protocol, id: body.id, result, error: null });
    }
    assert.ok(session, "invocation routes require an authenticated session");
    assert.ok(init.signal instanceof AbortSignal);
    assert.equal(init.signal.aborted, false);
    if (path === "/invocations/v1/recover") {
      assert.equal(init.method, "POST");
      assert.equal(headers.get("x-babel-host-document"), null);
      assert.equal(headers.get("x-babel-surface-document"), null);
      const existing = byKey.get(`${session.token}:${body.request_key}`);
      if (existing?.value.state.kind !== "completed") return new Response(null, { status: 204 });
      const payload = { target_object_id: body.payload.target_object_id ?? body.object_id,
        text: /\.(reply|share)\./.test(body.method) ? (body.payload.text ?? "").trim() : null,
        media: body.payload.media == null ? null : { ...body.payload.media, title: body.payload.media.title.trim() } };
      if (existing.intent.actor_id !== session.identity.id || existing.intent.object_id !== body.object_id
        || existing.intent.method !== body.method || digest(existing.intent.payload) !== digest(payload)) return new Response(null, { status: 409 });
      return response(existing.value);
    }
    const registration = path.match(/^\/invocations\/v1\/documents\/([0-9a-f-]{36})$/);
    if (registration) {
      const id = registration[1], previous = documents.get(id);
      if (previous && previous.token !== session.token) return new Response(null, { status: 403 });
      if (init.method === "DELETE") {
        assert.ok(previous); previous.closed = true; return new Response(null, { status: 204 });
      }
      assert.equal(init.method, "PUT"); assert.deepEqual(body, { object_id: "controller" });
      if (previous?.closed) return new Response(null, { status: 409 });
      documents.set(id, previous ?? { token: session.token, objectId: body.object_id, closed: false });
      return response({ document_id: id, object_id: body.object_id, expires_at: new Date(Date.now() + 60_000).toISOString(), renew_after_ms: 15_000 });
    }
    const documentId = headers.get("x-babel-host-document"), document = documents.get(documentId);
    assert.ok(document && !document.closed); assert.equal(document.token, session.token);
    assert.equal(headers.get("x-babel-surface-document"), null);
    if (path === "/invocations/v1/prepare") {
      assert.equal(init.method, "POST"); assert.equal(body.origin.kind, "host_action");
      assert.equal(body.origin.document_id, documentId); assert.equal(body.object_id, document.objectId);
      assert.match(body.method, /^babel\.social\.(follow|unfollow|share|reply)\.v2$/);
      assert.equal(body.payload.author_id, session.identity.id);
      assert.equal(body.timeout_ms, 30_000); assert.ok(body.request_key);
      const textAction = /\.(reply|share)\./.test(body.method);
      const payload = { target_object_id: body.payload.target_object_id ?? body.object_id,
        text: textAction ? (body.payload.text ?? "").trim() : null,
        media: body.payload.media == null ? null : { ...body.payload.media, title: body.payload.media.title.trim() } };
      const intent = { actor_id: session.identity.id, object_id: body.object_id, origin: body.origin,
        method: body.method, request_key: body.request_key, payload };
      const key = `${session.token}:${body.request_key}`, existing = byKey.get(key);
      if (existing) return digest(existing.intent) === digest(intent) ? response(existing.value) : new Response(null, { status: 409 });
      const value = { ...intent, invocation_id: digest(intent), created_at: new Date().toISOString(),
        deadline: new Date(Date.now() + 30_000).toISOString(), state: { kind: "pending" }, revision: 0, result: null };
      const entry = { intent: structuredClone(intent), value, token: session.token };
      byKey.set(key, entry); byId.set(value.invocation_id, entry); return response(value);
    }
    const match = path.match(/^\/invocations\/v1\/([0-9a-f]{64})\/(decision|execute|status|cancel)$/);
    assert.ok(match, `Unexpected route ${path}`);
    const entry = byId.get(match[1]); assert.ok(entry); assert.equal(entry.token, session.token);
    const value = entry.value; assert.equal(value.origin.document_id, documentId);
    if (match[2] === "status") { assert.equal(init.method, "GET"); return response(value); }
    assert.equal(init.method, "POST");
    if (match[2] === "decision") {
      assert.deepEqual(body, { decision: "allow_once" }); assert.equal(value.state.kind, "pending");
      value.state = { kind: "approved" }; value.revision = 1; return response(value);
    }
    assert.equal(match[2], "execute"); assert.deepEqual(body, {}); assert.equal(value.state.kind, "approved");
    const signature = { algorithm: "Ed25519", bytes: "a".repeat(128) };
    const object = { id: "published", author: value.actor_id, created_at: value.created_at, signature,
      kind: value.payload.media ? "babel.media" : "babel.text", schema: value.payload.media ? "babel.schema.media.v1" : "babel.schema.text.v1",
      protocol: { name: "babel", version: 1 }, payload: { text: value.payload.text },
      provenance: { parent: value.payload.target_object_id, forked_from: null, remixed_from: [] }, relations: [],
      resources: value.payload.media?.resources ?? [], surfaces: [], capabilities: [] };
    const edge = { id: `edge_${value.invocation_id}`, source: object.id, target: value.payload.target_object_id,
      relation: value.method.includes(".reply.") ? "reply_to" : "quotes", author: value.actor_id,
      created_at: value.created_at, signature, origin: "HumanAssertion", metadata: {} };
    const receipt = { request: { id: value.invocation_id, fingerprint: digest(value.payload), author: value.actor_id },
      outcome: { object: object.id, edges: [edge.id], event: `evt_${value.invocation_id}` } };
    value.result = { object, edge, receipt }; value.state = { kind: "completed", outcome: { kind: "publication", receipt: receipt.request.id } };
    value.revision = 2; effects.push(structuredClone(value.result));
    if (h.afterExecute) await h.afterExecute(value);
    return response(value);
  } };
}

const image = (name = "photo.png") => new File(["image bytes"], name, { type: "image/png" });
const app = () => new File(["<!doctype html>hello"], "index.html", { type: "text/html" });
function deferred() { let resolve; const promise = new Promise(yes => { resolve = yes; }); return { promise, resolve }; }
// Deterministic RPC fixture, not a substitute for backend content-integrity verification.
function blobReceipt(payload) {
  const bytes = Buffer.from(payload.bytes_hex, "hex");
  const integrity = createHash("sha256").update(bytes).digest("hex");
  return { integrity, uri: `babel://blobs/${integrity}`, media_type: payload.media_type, size_bytes: bytes.length };
}
const socialModes = ["reply", "share"];
const socialMediaCases = [
  { kind: "image", name: "scene.png", type: "image/png" },
  { kind: "audio", name: "voice.ogg", type: "audio/ogg" },
  { kind: "video", name: "clip.webm", type: "video/webm" },
];

test("publish, reply and share keep author, title, target, text and attachment visibility in sync", () => {
  const { h, el, document } = harness();
  h.startPublishComposer(); h.type("My original post");
  assert.equal(el("compose-title").textContent, "New post");
  assert.equal(el("author-handle").value, "Alice");
  assert.equal(el("compose-avatar").textContent, "A");
  assert.equal(el("compose-sign-in").hidden, true);
  assert.equal(document.activeElement, h.composeText);
  h.startSocialTextComposer("reply", "post-a"); h.type("A reply");
  assert.equal(el("compose-title").textContent, "Write a reply");
  assert.equal(el("compose-context").textContent, "Replying to post-a");
  assert.equal(el("compose-attachments").hidden, false);
  assert.equal(h.composeMedia.disabled, false);
  assert.equal(el("bundle-picker").hidden, true);
  h.startSocialTextComposer("share", "post-b");
  assert.equal(el("compose-title").textContent, "Share this post");
  assert.equal(h.composeSubmit.textContent, "Share");
  h.startSocialTextComposer("reply", "post-a"); assert.equal(h.composeText.value, "A reply");
  h.startPublishComposer(); assert.equal(h.composeText.value, "My original post");
  assert.equal(el("compose-attachments").hidden, false);
});

test("native input cancellation keeps the album; append and removal revoke only obsolete preview URLs", () => {
  const { h, el, created, revoked, document } = harness();
  h.startPublishComposer(); h.type("caption");
  const first = image("<b>literal.png"); h.select("compose-media", [first]);
  assert.equal(el("compose-media-name").textContent, first.name);
  assert.equal(el("compose-media-size").textContent, "11 B");
  assert.equal(el("compose-preview").dataset.state, "loading");
  el("compose-preview-image").emit("load");
  assert.equal(el("compose-preview").dataset.state, "ready");
  h.select("compose-media", []); assert.deepEqual(Array.from(h.drafts.current.media), [first]);
  assert.equal(created.length, 1); h.renderComposerDraft(); assert.equal(created.length, 1);
  const second = image("second.png"); h.select("compose-media", [second]);
  assert.deepEqual(Array.from(h.drafts.current.media), [first, second]);
  assert.deepEqual(revoked, [], "appending retains the first thumbnail URL");
  el("compose-media-remove").emit("click");
  assert.deepEqual(Array.from(h.drafts.current.media), [second]);
  assert.deepEqual(revoked, [created[0].url]);
  el("remove-compose-media").emit("click");
  assert.deepEqual(Array.from(h.drafts.current.media), []); assert.equal(h.composeMedia.value, "");
  assert.equal(el("compose-preview").hidden, true); assert.equal(h.composeText.value, "caption");
  assert.equal(el("compose-preview-image").getAttribute("src"), null);
  assert.deepEqual(revoked, created.map(item => item.url));
  assert.equal(document.activeElement, h.composeMedia);
});

test("invalid, empty and oversized images show errors and never start publishing", async () => {
  for (const file of [new File(["text"], "text.txt", { type: "text/plain" }),
    new File([], "empty.png", { type: "image/png" }),
    new File([new Uint8Array(4 * 1024 * 1024 + 1)], "large.png", { type: "image/png" })]) {
    const { h, el, created, requests } = harness(); h.startPublishComposer();
    h.select("compose-media", [file]);
    assert.equal(created.length, 0);
    assert.equal(el("author-status").dataset.state, "error");
    assert.equal(h.composeMedia.getAttribute("aria-invalid"), "true");
    assert.equal(el("compose-preview").dataset.state, "error");
    await h.publishFromComposer(); assert.equal(requests.length, 0);
    el("remove-compose-media").emit("click");
    assert.equal(h.composeMedia.getAttribute("aria-invalid"), null);
    assert.equal(el("author-status").dataset.state, "ready");
  }
});

test("image decode failure is visible and the file remains removable", () => {
  const { h, el } = harness(); h.startPublishComposer(); h.select("compose-media", [image()]);
  el("compose-preview-image").emit("error");
  assert.equal(el("compose-preview").dataset.state, "error");
  assert.match(el("compose-preview-status").textContent, /could not be previewed/);
  el("remove-compose-media").emit("click"); assert.deepEqual(Array.from(h.drafts.current.media), []);
});

test("application picker remains real and mutually exclusive with images, including invalid selection recovery", async () => {
  const { h, el, revoked, requests } = harness(); h.startPublishComposer(); h.type("An app");
  h.select("compose-media", [image()]); h.select("compose-bundle", [app()]);
  assert.deepEqual(Array.from(h.drafts.current.media), []); assert.equal(revoked.length, 1);
  assert.equal(h.drafts.current.bundle.entryPath, "index.html");
  assert.equal(el("bundle-entry").label.hidden, false); assert.match(el("bundle-status").textContent, /1 files/);
  h.select("compose-media", [image()]); assert.equal(h.drafts.current.bundle, null);
  assert.equal(el("bundle-entry").label.hidden, true);
  h.select("compose-bundle", [new File(["x"], "app.js")]);
  assert.match(h.bundlePicker.error, /HTML/); assert.equal(el("bundle-status").dataset.state, "error");
  await h.publishFromComposer(); assert.equal(requests.length, 0);
  el("clear-bundle").emit("click"); assert.equal(h.bundlePicker.error, null);
});

test("common permission controls preserve advanced scopes and reflect declarations after reopening", () => {
  const { h, el } = harness(); h.startPublishComposer(); h.select("compose-bundle", [app()]);
  const scoped = { id: "babel.storage.local", version: 1, scope: { namespace: "my-app" } };
  const editor = el("bundle-capabilities"); editor.value = JSON.stringify([scoped]); editor.selectionStart = 9;
  editor.emit("input");
  assert.equal(editor.selectionStart, 9);
  el("bundle-clipboard").checked = true; el("bundle-clipboard").emit("change");
  el("bundle-fullscreen").checked = true; el("bundle-fullscreen").emit("change");
  assert.deepEqual(JSON.parse(h.drafts.current.bundle.capabilitiesText), [scoped,
    { id: "babel.clipboard.write", version: 1, scope: {} }, { id: "babel.fullscreen.enter", version: 1, scope: {} }]);
  h.toggleComposer(false); h.startPublishComposer();
  assert.equal(el("bundle-permission-count").textContent, "3");
  assert.equal(el("bundle-clipboard").checked, true); assert.equal(el("bundle-fullscreen").checked, true);
  el("bundle-clipboard").checked = false; el("bundle-clipboard").emit("change");
  assert.deepEqual(JSON.parse(editor.value), [scoped, { id: "babel.fullscreen.enter", version: 1, scope: {} }]);
});

test("malformed declaration edits persist across close and owner changes and block all publication", async () => {
  const { h, el, requests, stored } = harness(); h.startPublishComposer(); h.type("An app"); h.select("compose-bundle", [app()]);
  const editor = el("bundle-capabilities"); editor.value = '[{"private-draft":'; editor.emit("input");
  assert.equal(editor.getAttribute("aria-invalid"), "true");
  assert.equal(el("bundle-clipboard").disabled, true);
  assert.equal(el("bundle-permissions").open, true);
  h.toggleComposer(false); h.startPublishComposer();
  assert.equal(editor.value, '[{"private-draft":');
  await h.publishFromComposer(); assert.equal(requests.length, 0);
  assert.equal(el("author-status").dataset.state, "error");
  h.changeAccount("bob"); assert.equal(editor.value, "[]"); assert.equal(el("bundle-permissions").hidden, true);
  h.changeAccount("alice"); assert.equal(editor.value, '[{"private-draft":');
  assert.ok(h.bundlePicker.error);
  assert.equal(JSON.stringify([...stored.values()]).includes("private-draft"), false);
  editor.value = "[]"; editor.emit("input");
  assert.equal(h.bundlePicker.error, null); assert.equal(editor.getAttribute("aria-invalid"), null);
  assert.equal(el("bundle-clipboard").disabled, false);
});

test("pending app submission locks declaration controls and removal clears permission state", () => {
  const { h, el } = harness(); h.startPublishComposer(); h.select("compose-bundle", [app()]);
  el("bundle-fullscreen").checked = true; el("bundle-fullscreen").emit("change");
  const submission = h.drafts.begin(); h.renderComposerDraft();
  const original = submission.bundle.capabilitiesText;
  assert.equal(el("bundle-capabilities").disabled, true); assert.equal(el("bundle-fullscreen").disabled, true);
  el("bundle-capabilities").value = "[]"; el("bundle-capabilities").emit("input");
  el("bundle-fullscreen").checked = false; el("bundle-fullscreen").emit("change");
  assert.equal(h.drafts.current.bundle.capabilitiesText, original);
  h.drafts.finish(submission, false); h.renderComposerDraft();
  assert.equal(el("bundle-fullscreen").checked, true);
  el("clear-bundle").emit("click");
  assert.equal(el("bundle-capabilities").value, "[]"); assert.equal(el("bundle-permissions").hidden, true);
  assert.equal(el("bundle-permissions").open, false); assert.equal(h.bundlePicker.error, null);
});

test("draft cancellation restores focus, reopening preserves image/text, page lifecycle releases and renews the preview", () => {
  const { h, el, opener, document, created, revoked } = harness();
  h.startPublishComposer(); h.type("kept"); h.select("compose-media", [image()]);
  h.toggleComposer(false); assert.equal(document.activeElement, opener);
  h.startPublishComposer(); assert.equal(h.composeText.value, "kept");
  assert.equal(el("compose-preview").hidden, false);
  h.window.emit("pagehide"); assert.equal(revoked.length, 1);
  h.window.emit("pageshow", { persisted: true }); assert.equal(created.length, 2);
  assert.equal(h.drafts.current.media[0].name, "photo.png");
});

test("owner changes clear private fields and previews; guest sign-in is actionable", () => {
  const { h, el, revoked } = harness(); h.startPublishComposer(); h.type("Alice's text"); h.select("compose-media", [image()]);
  h.changeAccount("bob");
  assert.equal(el("author-handle").value, "bob"); assert.equal(el("compose-avatar").textContent, "B");
  assert.equal(h.composeText.value, ""); assert.equal(el("compose-preview").hidden, true); assert.equal(revoked.length, 1);
  h.changeAccount(null); assert.equal(el("compose-sign-in").hidden, false); assert.equal(h.composeSubmit.disabled, true);
  el("compose-sign-in").emit("click"); assert.equal(h.loginOpened, true);
  h.changeAccount("alice"); assert.equal(h.composeText.value, "Alice's text"); assert.equal(h.drafts.current.media[0].name, "photo.png");
});

test("pending publication disables attachment mutation and duplicate sends while retaining later text edits", async () => {
  const { h, el, requests, navigations } = harness(); const gate = deferred(), started = deferred();
  h.startPublishComposer(); h.type("original"); h.select("compose-media", [image()]);
  h.respond = async () => { started.resolve(); await gate.promise; };
  const pending = h.publishFromComposer(); await started.promise;
  assert.equal(h.composeSubmit.textContent, "Publishing..."); assert.equal(h.composeSubmit.disabled, true);
  assert.equal(h.composeSubmit.getAttribute("aria-busy"), "true");
  assert.equal(h.composeMedia.disabled, true); assert.equal(el("compose-bundle").disabled, true);
  assert.equal(el("remove-compose-media").disabled, true);
  el("remove-compose-media").emit("click"); assert.equal(h.drafts.current.media[0].name, "photo.png");
  h.select("compose-media", [image("replacement.png")]); assert.equal(h.drafts.current.media[0].name, "photo.png");
  await h.publishFromComposer(); assert.equal(requests.length, 1);
  h.type("later edit"); gate.resolve(); await pending;
  assert.equal(h.composeText.value, "later edit"); assert.equal(h.composeSubmit.disabled, false);
  assert.equal(navigations.length, 0); assert.equal(h.composerPanel.hidden, false);
});

test("publish failure retains the attachment and retry key; success clears only the submitted draft", async () => {
  const { h, el, requests, navigations } = harness();
  h.startSocialTextComposer("reply", "other"); h.type("keep reply");
  h.startPublishComposer(); h.type("retry caption"); h.select("compose-media", [image()]);
  h.respond = async () => { throw new Error("connection lost"); };
  await h.publishFromComposer();
  assert.equal(el("author-status").dataset.state, "error"); assert.match(el("author-status").textContent, /connection lost/);
  assert.equal(el("compose-preview").hidden, false); assert.equal(h.composeSubmit.disabled, false);
  h.respond = null; await h.publishFromComposer();
  assert.equal(requests[0].idempotency_key, requests[1].idempotency_key);
  assert.equal(h.composeText.value, ""); assert.deepEqual(Array.from(h.drafts.current.media), []); assert.equal(navigations.length, 1);
  h.startSocialTextComposer("reply", "other"); assert.equal(h.composeText.value, "keep reply");
});

test("account changes during publication prevent later writes and preserve the new owner's draft", async () => {
  const { h, requests, refreshed } = harness(); const gate = deferred(), started = deferred();
  h.startSocialTextComposer("reply", "a"); h.type("Alice reply");
  h.respond = async () => { started.resolve(); await gate.promise; };
  const pending = h.publishFromComposer(); await started.promise;
  h.changeAccount("bob"); h.type("Bob reply"); gate.resolve(); await pending;
  assert.equal(requests.length, 1); assert.equal(h.composeText.value, "Bob reply"); assert.equal(refreshed.length, 0);
  h.changeAccount("alice"); assert.equal(h.composeText.value, "Alice reply");
});

test("textarea growth is bounded and visual updates do not overwrite the cursor or draft", () => {
  const { h } = harness(); h.startPublishComposer();
  h.composeText.scrollHeight = 900; h.composeText.selectionStart = 3; h.type("long post");
  assert.equal(h.composeText.style.height, "280px"); assert.equal(h.composeText.selectionStart, 3);
  assert.equal(h.drafts.current.text, "long post");
  h.composeText.scrollHeight = 40; h.type("short"); assert.equal(h.composeText.style.height, "176px");
});

test("image size boundaries preserve the 4 MiB contract and attachments never enter durable storage", () => {
  const { h, stored } = harness();
  assert.equal(h.composerMediaError(new File([new Uint8Array(4 * 1024 * 1024)], "limit.png", { type: "image/png" })), null);
  assert.equal(h.composerFileSize(1024), "1.0 KiB"); assert.equal(h.composerFileSize(4 * 1024 * 1024), "4.0 MiB");
  h.startPublishComposer(); h.type("caption"); h.select("compose-media", [image("private.png")]);
  const entry = JSON.parse([...stored.values()][0]); assert.equal(entry.text, "caption");
  assert.deepEqual(Object.keys(entry).sort(), ["operationId", "text"]);
  assert.equal(JSON.stringify(plain([...stored.values()])).includes("private.png"), false);
});

test("media validation accepts common browser containers with explicit image and playback limits", () => {
  const { h } = harness();
  for (const type of ["image/jpeg", "image/png", "image/gif", "image/webp", "image/avif", "image/bmp", "image/x-icon", "image/vnd.microsoft.icon",
    "audio/mpeg", "audio/mp4", "audio/aac", "audio/ogg", "audio/wav", "audio/x-wav", "audio/webm", "audio/flac",
    "video/mp4", "video/webm", "video/ogg", "video/quicktime"]) {
    assert.equal(h.composerMediaError(new File(["bytes"], "media", { type })), null, type);
    assert.match(h.composerMediaError(new File([], "empty", { type })), /empty/, type);
  }
  for (const type of ["", "application/octet-stream", "image/svg+xml", "image/tiff", "audio/midi", "video/x-msvideo", "video/mp4;codecs=avc1"]) {
    assert.match(h.composerMediaError(new File(["bytes"], "media.mp4", { type })), /supported/, type);
  }
  for (const type of ["audio/mpeg", "video/mp4"]) {
    assert.equal(h.composerMediaError(new File([new Uint8Array(8 * 1024 * 1024)], "limit", { type })), null);
    assert.match(h.composerMediaError(new File([new Uint8Array(8 * 1024 * 1024 + 1)], "oversize", { type })), /8 MiB/);
  }
});

test("audio and video previews expose native controls without autoplay and only their own loading events update status", () => {
  for (const kind of ["audio", "video"]) {
    const { h, el, created } = harness(); h.startPublishComposer();
    const file = new File(["media bytes"], `${kind}.mp4`, { type: `${kind}/mp4` });
    h.select("compose-media", [file]);
    const player = el(`compose-preview-${kind}`);
    assert.equal(player.hidden, false); assert.equal(player.controls, true);
    assert.equal(player.autoplay, false); assert.equal(player.preload, "metadata");
    assert.equal(player.src, created[0].url); assert.equal(player.loadCount, 1);
    assert.equal(player.getAttribute("aria-label"), `Preview of ${file.name}`);
    assert.equal(el("compose-preview-image").hidden, true);
    assert.equal(el(`compose-preview-${kind === "audio" ? "video" : "audio"}`).hidden, true);
    if (kind === "video") assert.equal(player.playsInline, true);
    el("compose-preview-image").emit("load");
    assert.equal(el("compose-preview").dataset.state, "loading");
    player.emit("loadedmetadata"); assert.equal(el("compose-preview").dataset.state, "loading");
    player.readyState = 1; player.emit("loadedmetadata");
    assert.equal(el("compose-preview").dataset.state, "ready");
    assert.equal(el("compose-preview-status").textContent, "");
    assert.deepEqual(Array.from(h.drafts.current.media), [file]);
  }
});

test("unsupported codecs show honest feedback and errors retain a removable attachment", () => {
  for (const kind of ["audio", "video"]) {
    const { h, el } = harness(); h.startPublishComposer();
    const player = el(`compose-preview-${kind}`); player.playable = "";
    const file = new File(["encoded bytes"], `${kind}.mp4`, { type: `${kind}/mp4` });
    h.select("compose-media", [file]);
    assert.equal(el("compose-preview").dataset.state, "unsupported");
    assert.match(el("compose-preview-status").textContent, /may not support/);
    assert.equal(h.composerMediaError(file), null);
    player.error = { code: 4 }; player.emit("error");
    assert.equal(el("compose-preview").dataset.state, "error");
    assert.match(el("compose-preview-status").textContent, /codec may be unsupported/);
    player.readyState = 2; player.emit("loadeddata");
    assert.equal(el("compose-preview").dataset.state, "error");
    assert.deepEqual(Array.from(h.drafts.current.media), [file]);
    el("remove-compose-media").emit("click");
    assert.equal(player.src, ""); assert.equal(player.hidden, true); assert.deepEqual(Array.from(h.drafts.current.media), []);
  }
});

test("closing or backgrounding pauses native playback but preserves the selected File and position", () => {
  const { h, el, created, document } = harness(); h.startPublishComposer();
  const file = new File(["audio"], "voice.ogg", { type: "audio/ogg" }); h.select("compose-media", [file]);
  const player = el("compose-preview-audio");
  player.currentTime = 3; player.paused = false; player.emit("play"); assert.equal(player.paused, false);
  h.toggleComposer(false); assert.equal(player.paused, true);
  assert.equal(player.currentTime, 3); assert.deepEqual(Array.from(h.drafts.current.media), [file]);
  player.paused = false; player.emit("play"); assert.equal(player.paused, true);
  h.startPublishComposer(); assert.equal(created.length, 1); assert.equal(player.paused, true);
  player.paused = false; document.hidden = true; document.dispatchEvent(new Event("visibilitychange"));
  assert.equal(player.paused, true);
  player.paused = false; player.emit("play"); assert.equal(player.paused, true);
  document.hidden = false; document.dispatchEvent(new Event("visibilitychange"));
  assert.equal(player.paused, true); assert.equal(player.currentTime, 3);
});

test("replacement, account switching and page lifecycle unload playback and renew only retained drafts", () => {
  const { h, el, created, revoked, document } = harness(); h.startPublishComposer();
  const file = new File(["video"], "clip.webm", { type: "video/webm" }); h.select("compose-media", [file]);
  const video = el("compose-preview-video"), audio = el("compose-preview-audio");
  video.paused = false; h.select("compose-media", [new File(["audio"], "voice.wav", { type: "audio/wav" })]);
  el("compose-media-remove").emit("click");
  assert.equal(video.paused, true); assert.equal(video.src, ""); assert.equal(video.loadCount, 2);
  assert.deepEqual(revoked, [created[0].url]);
  video.error = { code: 4 }; video.emit("error");
  assert.equal(el("compose-preview").dataset.state, "loading");
  audio.paused = false; h.changeAccount("bob");
  assert.equal(audio.paused, true); assert.equal(audio.src, ""); assert.equal(audio.loadCount, 2);
  assert.deepEqual(revoked, created.map(item => item.url));
  h.changeAccount("alice"); assert.equal(audio.src, created[2].url);
  audio.paused = false; h.window.emit("pagehide");
  assert.equal(audio.paused, true); assert.equal(audio.src, "");
  const paused = audio.pauseCount;
  document.hidden = true; document.dispatchEvent(new Event("visibilitychange"));
  assert.equal(audio.pauseCount, paused, "disposed preview removes the page listener");
  h.window.emit("pageshow", { persisted: true });
  assert.equal(created.length, 4); assert.equal(h.drafts.current.media[0].name, "voice.wav");
  assert.equal(audio.src, created[3].url); assert.equal(audio.paused, true);
  h.composerView.dispose(); h.composerView.dispose();
  assert.deepEqual(revoked, created.map(item => item.url));
});

test("pending audio upload locks attachments and retains the preview after failure", async () => {
  const { h, el, requests } = harness(); const gate = deferred(), started = deferred();
  h.startPublishComposer(); h.type("Voice note");
  const file = new File(["audio"], "voice.mp3", { type: "audio/mpeg" }); h.select("compose-media", [file]);
  h.respond = async () => { started.resolve(); await gate.promise; throw new Error("upload offline"); };
  const pending = h.publishFromComposer(); await started.promise;
  assert.equal(h.composeMedia.disabled, true); assert.equal(el("remove-compose-media").disabled, true);
  el("remove-compose-media").emit("click"); assert.deepEqual(Array.from(h.drafts.current.media), [file]);
  assert.equal(requests[0].payload.media_type, "audio/mpeg");
  gate.resolve(); await pending;
  assert.deepEqual(Array.from(h.drafts.current.media), [file]); assert.equal(el("compose-preview-audio").hidden, false);
  assert.equal(h.composeMedia.disabled, false); assert.match(el("author-status").textContent, /upload offline/);
});

test("invalid playback attachments block upload and recover after a supported selection", async () => {
  for (const file of [new File([], "empty.mp4", { type: "video/mp4" }),
    new File(["unknown"], "unknown.mp3", { type: "audio/unknown" }),
    new File([new Uint8Array(8 * 1024 * 1024 + 1)], "large.webm", { type: "video/webm" })]) {
    const { h, el, created, requests } = harness(); h.startPublishComposer();
    h.select("compose-media", [file]); await h.publishFromComposer();
    assert.equal(created.length, 0); assert.equal(requests.length, 0);
    assert.equal(h.composeMedia.getAttribute("aria-invalid"), "true");
    h.select("compose-media", [new File(["audio"], "valid.ogg", { type: "audio/ogg" })]);
    assert.equal(h.composeMedia.getAttribute("aria-invalid"), "true", "appending cannot hide an invalid earlier attachment");
    el("compose-media-remove").emit("click");
    assert.equal(h.composeMedia.getAttribute("aria-invalid"), null);
    assert.equal(created.length, 1); assert.equal(el("compose-preview-audio").hidden, false);
  }
});

test("object URL failure keeps the draft recoverable and page restoration retries preview creation", () => {
  const options = { failPreview: true };
  const { h, el, created, revoked, document } = harness(options); h.startPublishComposer();
  const file = new File(["video"], "clip.mp4", { type: "video/mp4" }); h.select("compose-media", [file]);
  assert.deepEqual(Array.from(h.drafts.current.media), [file]); assert.equal(created.length, 0);
  assert.equal(el("compose-preview").dataset.state, "error");
  assert.match(el("compose-preview-status").textContent, /still attached/);
  const player = el("compose-preview-video"), count = player.pauseCount;
  document.hidden = true; document.dispatchEvent(new Event("visibilitychange"));
  assert.equal(player.pauseCount, count);
  options.failPreview = false;
  h.window.emit("pagehide"); h.window.emit("pageshow", { persisted: true });
  assert.equal(created.length, 1); assert.equal(player.src, created[0].url);
  assert.equal(el("compose-preview").dataset.state, "loading");
  el("remove-compose-media").emit("click"); assert.deepEqual(revoked, [created[0].url]);
});

for (const mode of socialModes) {
  for (const { kind, name, type } of socialMediaCases) {
    for (const caption of ["  First line\nSecond line  ", ""]) {
      test(`${mode} publishes ${kind} ${caption ? "with the entire caption" : "without a caption"} through one-use social consent`, async () => {
        const { h, el, requests, httpRequests, server, navigations, refreshed } = harness();
        const file = new File([`${kind} attachment bytes`], name, { type });
        h.startSocialTextComposer(mode, "target-post"); h.type(caption); h.select("compose-media", [file]);
        assert.equal(el(`compose-preview-${kind}`).hidden, false);
        assert.equal(el("compose-attachments").hidden, false);
        assert.equal(el("bundle-picker").hidden, true);
        await h.publishFromComposer();
        assert.deepEqual(requests.map(request => request.method), ["babel.media.blob.put.v1", "babel.object.publish.v1", `babel.social.${mode}.v2`]);
        const [upload, controller, publication] = requests;
        assert.equal(upload.payload.media_type, type);
        assert.equal(upload.payload.bytes_hex, Buffer.from(await file.arrayBuffer()).toString("hex"));
        assert.equal(controller.payload.author_id, "alice");
        assert.equal(controller.payload.draft.provenance.parent, "target-post");
        assert.equal(controller.payload.draft.payload.metadata.target_object_id, "target-post");
        assert.ok(controller.payload.draft.capabilities.some(capability => capability.id === `babel.social.${mode}`
          && capability.version === 1 && capability.scope.object_id === "target-post"));
        assert.deepEqual(publication.payload, {
          author_id: "alice", target_object_id: "target-post", text: caption.trim(),
          media: { title: caption ? "First line" : name.replace(/\.[^.]+$/, ""), resources: [blobReceipt(upload.payload)] },
        });
        assert.equal(publication.object_id, "controller");
        assert.equal(publication.origin.kind, "host_action");
        assert.match(publication.origin.document_id, /^[0-9a-f-]{36}$/);
        assert.ok(publication.request_key.startsWith("web-social-"));
        assert.deepEqual(controller.binding.capability_grants, []);
        assert.deepEqual(httpRequests.map(request => request.action), ["rpc", "rpc", "recover", publication.origin.document_id,
          "prepare", "decision", "execute", publication.origin.document_id]);
        assert.deepEqual(httpRequests.find(request => request.action === "decision").body, { decision: "allow_once" });
        assert.equal(server.effects.length, 1);
        assert.deepEqual(Array.from(h.drafts.current.media), []); assert.equal(h.composeText.value, "");
        assert.equal(h.composerPanel.hidden, true);
        if (mode === "reply") {
          assert.deepEqual(refreshed, [["target-post", server.effects[0].object]]);
          assert.equal(navigations.length, 0);
        } else {
          assert.equal(refreshed.length, 0); assert.equal(navigations.length, 1);
          assert.equal(navigations[0][1], "published");
          assert.deepEqual(navigations[0][3], server.effects[0].object);
          assert.equal(navigations[0][2](), true);
        }
      });
    }
  }

  for (const failure of ["upload", "publication"]) {
    test(`${mode} ${failure} failure retains the File and exact RPC payloads and mutation keys on retry`, async () => {
      const { h, el, requests, navigations, refreshed } = harness();
      const file = new File(["retained media"], "private.webm", { type: "video/webm" });
      h.startSocialTextComposer(mode, "retry-target"); h.type("retry caption"); h.select("compose-media", [file]);
      const failedMethod = failure === "upload" ? "babel.media.blob.put.v1" : `babel.social.${mode}.v2`;
      h.respond = async (request, route) => { if (route.action !== "recover" && request?.method === failedMethod) throw new Error("response lost"); };
      await h.publishFromComposer();
      const first = [...requests];
      assert.equal(first.length, failure === "upload" ? 1 : 3);
      assert.deepEqual(Array.from(h.drafts.current.media), [file]); assert.equal(h.composeText.value, "retry caption");
      assert.equal(h.composerPanel.hidden, false); assert.equal(h.composeMedia.disabled, false);
      assert.equal(el("compose-preview-video").hidden, false);
      assert.match(el("author-status").textContent, /response lost/);
      assert.equal(navigations.length + refreshed.length, 0);
      h.respond = null; await h.publishFromComposer();
      const retried = requests.slice(first.length);
      assert.equal(retried.length, 3);
      for (let index = 0; index < first.length; index++) {
        assert.equal(retried[index].method, first[index].method);
        assert.deepEqual(retried[index].payload, first[index].payload);
        assert.equal(retried[index].idempotency_key, first[index].idempotency_key);
        assert.equal(retried[index].request_key, first[index].request_key);
        assert.deepEqual(retried[index].origin, first[index].origin);
      }
      assert.deepEqual(Array.from(h.drafts.current.media), []);
      assert.equal(navigations.length + refreshed.length, 1);
    });
  }

  test(`${mode} upload locks attachment mutation and duplicate sends without discarding later caption edits`, async () => {
    const { h, el, requests, navigations, refreshed } = harness(); const gate = deferred(), started = deferred();
    const file = new File(["audio bytes"], "voice.ogg", { type: "audio/ogg" });
    h.startSocialTextComposer(mode, "locked-target"); h.type("original caption"); h.select("compose-media", [file]);
    assert.deepEqual(Array.from(h.drafts.current.media), [file], "social attachment picker must accept the file before upload starts");
    h.respond = async request => { if (request?.method === "babel.media.blob.put.v1") { started.resolve(); await gate.promise; } };
    const pending = h.publishFromComposer(); await started.promise;
    assert.equal(h.composeMedia.disabled, true); assert.equal(h.composeSubmit.disabled, true);
    assert.equal(el("remove-compose-media").disabled, true); assert.equal(el("compose-bundle").disabled, true);
    assert.equal(h.composeSubmit.getAttribute("aria-busy"), "true");
    el("remove-compose-media").emit("click"); h.select("compose-media", [image()]);
    assert.deepEqual(Array.from(h.drafts.current.media), [file]);
    await h.publishFromComposer(); assert.equal(requests.length, 1);
    h.type("later caption"); gate.resolve(); await pending;
    assert.equal(requests.at(-1).payload.text, "original caption");
    assert.equal(h.composeText.value, "later caption"); assert.deepEqual(Array.from(h.drafts.current.media), [file]);
    assert.equal(h.composeMedia.disabled, false); assert.equal(h.composerPanel.hidden, false);
    assert.equal(navigations.length + refreshed.length, 0);
  });

  test(`${mode} account switch during upload prevents controller, consent and publication writes`, async () => {
    const { h, el, requests, navigations, refreshed } = harness(); const gate = deferred(), started = deferred();
    const aliceFile = image("alice.png"), bobFile = image("bob.png");
    h.startSocialTextComposer(mode, "same-target"); h.type("Alice caption"); h.select("compose-media", [aliceFile]);
    assert.deepEqual(Array.from(h.drafts.current.media), [aliceFile], "social attachment picker must accept the file before upload starts");
    h.respond = async request => { if (request?.method === "babel.media.blob.put.v1") { started.resolve(); await gate.promise; } };
    const pending = h.publishFromComposer(); await started.promise;
    h.changeAccount("bob"); h.type("Bob caption"); h.select("compose-media", [bobFile]);
    gate.resolve(); await pending;
    assert.deepEqual(requests.map(request => request.method), ["babel.media.blob.put.v1"]);
    assert.deepEqual(Array.from(h.drafts.current.media), [bobFile]); assert.equal(h.composeText.value, "Bob caption");
    assert.equal(el("author-status").dataset.state, "ready");
    assert.equal(navigations.length + refreshed.length, 0);
    h.changeAccount("alice");
    assert.deepEqual(Array.from(h.drafts.current.media), [aliceFile]); assert.equal(h.composeText.value, "Alice caption");
    assert.equal(h.composeSubmit.disabled, false);
  });

  test(`${mode} refresh failure reports publication success and does not preserve an already-published attachment`, async () => {
    const { h, el, requests } = harness();
    h.startSocialTextComposer(mode, "target"); h.select("compose-media", [image()]);
    if (mode === "reply") h.conversations.refresh = async () => { throw new Error("refresh offline"); };
    else h.loadFeed = async () => { throw new Error("refresh offline"); };
    await h.publishFromComposer();
    assert.equal(requests.filter(request => request.method === `babel.social.${mode}.v2`).length, 1);
    assert.match(el("author-status").textContent, /Published published; refresh failed: refresh offline/);
    assert.deepEqual(Array.from(h.drafts.current.media), []); assert.equal(h.composerPanel.hidden, true);
    h.startSocialTextComposer(mode, "target"); await h.publishFromComposer();
    assert.equal(requests.length, 3, "empty reopened draft must not retry a successful media publication");
  });

  test(`${mode} empty and invalid attachments cannot silently fall back to text`, async () => {
    const { h, el, requests } = harness(); h.startSocialTextComposer(mode, "target");
    h.type("   "); await h.publishFromComposer();
    assert.equal(requests.length, 0); assert.equal(el("author-status").dataset.state, "error");
    for (const file of [new File([], "empty.png", { type: "image/png" }),
      new File(["script"], "payload.svg", { type: "image/svg+xml" })]) {
      el("remove-compose-media").emit("click");
      h.type("valid text does not excuse invalid attachment"); h.select("compose-media", [file]);
      await h.publishFromComposer(); assert.equal(requests.length, 0);
      assert.deepEqual(Array.from(h.drafts.current.media), [file]); assert.equal(h.drafts.current.pending, null);
    }
    el("remove-compose-media").emit("click"); await h.publishFromComposer();
    assert.equal(requests.length, 2);
    assert.equal(requests.at(-1).method, `babel.social.${mode}.v2`);
    assert.equal(requests.at(-1).payload.media ?? null, null);
  });
}

test("album selection, reorder, removal and append preserve file references and selected preview", () => {
  const { h, el, root, created, revoked } = harness(); h.startPublishComposer(); h.type("album caption");
  const a = image("first.png"), b = new File(["voice"], "voice.ogg", { type: "audio/ogg" });
  const c = new File(["clip"], "clip.mp4", { type: "video/mp4" }), d = image("last.png");
  h.select("compose-media", [a, b, c]);
  const select = index => root.querySelector(`[data-compose-media-index="${index}"]`).emit("click");
  assert.equal(el("compose-media-position").textContent, "1 / 3");
  assert.equal(el("compose-media-earlier").disabled, true);
  const revision = h.drafts.current.revision;
  select(2);
  assert.equal(el("compose-media-position").textContent, "3 / 3");
  assert.equal(h.drafts.current.revision, revision, "selection is presentation state");
  assert.equal(el("compose-media-later").disabled, true);
  const player = el("compose-preview-video"), url = player.src, loads = player.loadCount;
  el("compose-media-earlier").emit("click");
  assert.deepEqual(Array.from(h.drafts.current.media), [a, c, b]);
  assert.equal(el("compose-media-position").textContent, "2 / 3");
  assert.equal(player.src, url); assert.equal(player.loadCount, loads, "reordering keeps native playback loaded");
  el("compose-media-remove").emit("click");
  assert.deepEqual(Array.from(h.drafts.current.media), [a, b]);
  assert.equal(el("compose-media-name").textContent, b.name);
  assert.equal(player.src, ""); assert.ok(revoked.includes(url));
  h.select("compose-media", [d]);
  assert.deepEqual(Array.from(h.drafts.current.media), [a, b, d]);
  assert.equal(el("compose-media-position").textContent, "2 / 3");
  el("compose-media-later").emit("click");
  assert.deepEqual(Array.from(h.drafts.current.media), [a, d, b]);
  assert.equal(el("compose-media-position").textContent, "3 / 3");
  assert.equal(root.querySelector('[data-compose-media-index="2"]').getAttribute("aria-pressed"), "true");
  assert.equal(h.composeText.value, "album caption");
  h.composerView.dispose(); h.composerView.dispose();
  assert.deepEqual([...revoked].sort(), created.map(item => item.url).sort());
  assert.equal(new Set(revoked).size, revoked.length);
});

test("attachment strip uses one tab stop, arrow navigation and immediate scrolling while a single attachment stays compact", () => {
  const { h, el, root, document } = harness(); h.startPublishComposer();
  h.select("compose-media", [image()]);
  assert.equal(el("compose-album").hidden, true); assert.equal(el("compose-preview").hidden, false);
  h.select("compose-media", [new File(["audio"], "voice.ogg", { type: "audio/ogg" }), image("last.png")]);
  const item = index => root.querySelector(`[data-compose-media-index="${index}"]`);
  assert.equal(el("compose-album").hidden, false);
  assert.deepEqual([0, 1, 2].map(index => item(index).tabIndex), [0, -1, -1]);
  const snapshot = h.drafts.current.media;
  const event = item(0).emit("keydown", { key: "ArrowRight" });
  assert.equal(event.defaultPrevented, true); assert.equal(document.activeElement, item(1));
  assert.deepEqual([0, 1, 2].map(index => item(index).tabIndex), [-1, 0, -1]);
  assert.deepEqual(plain(item(1).lastScroll), { block: "nearest", inline: "nearest", behavior: "instant" });
  item(1).emit("keydown", { key: "End" }); assert.equal(document.activeElement, item(2));
  item(2).emit("keydown", { key: "Home" }); assert.equal(document.activeElement, item(0));
  item(0).emit("keydown", { key: "ArrowLeft" }); assert.equal(document.activeElement, item(0));
  item(0).emit("keydown", { key: "End", ctrlKey: true }); assert.equal(document.activeElement, item(0));
  assert.equal(h.drafts.current.media, snapshot);
  item(0).emit("keydown", { key: "End" }); el("compose-media-earlier").emit("click");
  assert.equal(item(1).lastScroll.inline, "nearest"); assert.equal(item(1).getAttribute("aria-pressed"), "true");
});

test("album validation enforces inclusive count and aggregate limits and rejects repeated File identity", () => {
  const { h } = harness();
  assert.equal(h.composerAlbumError([]), null);
  const twelve = Array.from({ length: 12 }, (_, index) => image(`${index}.png`));
  assert.equal(h.composerAlbumError(twelve), null);
  assert.match(h.composerAlbumError([...twelve, image()]), /12/);
  assert.match(h.composerAlbumError([twelve[0], twelve[0]]), /same file/);
  assert.equal(h.composerAlbumError([image(), image()]), null, "distinct File instances defer content equality to publication");
  const eightMiB = new Blob([new Uint8Array(8 * 1024 * 1024)]);
  const sixtyFourMiB = Array.from({ length: 8 }, (_, index) => new File([eightMiB], `${index}.mp4`, { type: "video/mp4" }));
  assert.equal(h.composerAlbumError(sixtyFourMiB), null);
  assert.match(h.composerAlbumError([...sixtyFourMiB, image()]), /64 MiB/);
  assert.match(h.composerAlbumError([image(), new File([], "empty.ogg", { type: "audio/ogg" })]), /Attachment 2.*empty/);
  assert.match(h.composerAlbumError([image(), new File([eightMiB], "large.png", { type: "image/png" })]), /Attachment 2.*4 MiB/);
});

test("duplicate references stay visible and can be repaired by removing the selected duplicate", async () => {
  const { h, el, root, created, revoked, requests } = harness(); h.startPublishComposer();
  const file = image(); h.select("compose-media", [file, file]);
  assert.deepEqual(Array.from(h.drafts.current.media), [file, file]);
  assert.match(el("compose-album-status").textContent, /same file/);
  assert.equal(h.composeMedia.getAttribute("aria-invalid"), "true");
  await h.publishFromComposer(); assert.equal(requests.length, 0);
  root.querySelector('[data-compose-media-index="1"]').emit("click");
  el("compose-media-remove").emit("click");
  assert.deepEqual(Array.from(h.drafts.current.media), [file]);
  assert.equal(h.composeMedia.getAttribute("aria-invalid"), null);
  assert.equal(created.length, 1); assert.equal(revoked.length, 0);
  el("remove-compose-media").emit("click");
  assert.deepEqual(revoked, [created[0].url]);
});

test("pending album upload locks every edit control and failure restores the same ordered album", async () => {
  const { h, el, root, requests } = harness(); const gate = deferred(), started = deferred();
  const files = [image(), new File(["audio"], "voice.ogg", { type: "audio/ogg" })];
  h.startPublishComposer(); h.type("caption"); h.select("compose-media", files);
  root.querySelector('[data-compose-media-index="1"]').emit("click");
  h.respond = async () => { started.resolve(); await gate.promise; throw new Error("offline"); };
  const pending = h.publishFromComposer(); await started.promise;
  const snapshot = h.drafts.current.media;
  assert.equal(h.composeText.disabled, true); assert.equal(h.composeMedia.disabled, true);
  for (const control of ["compose-media-earlier", "compose-media-later", "compose-media-remove", "remove-compose-media"]) {
    assert.equal(el(control).disabled, true); el(control).emit("click");
  }
  const first = root.querySelector('[data-compose-media-index="0"]');
  assert.equal(first.disabled, true); first.emit("click");
  h.select("compose-media", [image("ignored.png")]); await h.publishFromComposer();
  assert.equal(h.drafts.current.media, snapshot); assert.equal(requests.length, 1);
  assert.equal(el("compose-media-position").textContent, "2 / 2");
  gate.resolve(); await pending;
  assert.equal(h.drafts.current.media, snapshot); assert.equal(h.composeText.disabled, false);
  assert.equal(el("compose-media-remove").disabled, false); assert.equal(el("compose-media-earlier").disabled, false);
  assert.match(el("author-status").textContent, /offline/);
});

for (const mode of ["publish", "reply", "share"]) {
  test(`${mode} album publishes every resource in order with caption and stable retry keys`, async () => {
    const { h, el, requests } = harness();
    if (mode === "publish") h.startPublishComposer(); else h.startSocialTextComposer(mode, "album-target");
    const files = [new File(["video first"], "clip.webm", { type: "video/webm" }),
      image(), new File(["audio last"], "voice.ogg", { type: "audio/ogg" })];
    h.type("First line\nFull caption"); h.select("compose-media", files);
    const publicationMethod = mode === "publish" ? "babel.object.publish_media.v1" : `babel.social.${mode}.v2`;
    h.respond = async (request, route) => { if (route.action !== "recover" && request?.method === publicationMethod) throw new Error("acknowledgement lost"); };
    await h.publishFromComposer();
    const failed = [...requests];
    assert.equal(failed.length, mode === "publish" ? 4 : 5);
    assert.deepEqual(failed.slice(0, 3).map(request => request.payload.media_type), files.map(file => file.type));
    assert.deepEqual(Array.from(h.drafts.current.media), files);
    assert.equal(h.drafts.current.pending, null); assert.equal(h.composerPanel.hidden, false);
    const media = mode === "publish" ? failed.at(-1).payload : failed.at(-1).payload.media;
    assert.deepEqual(media.resources, failed.slice(0, 3).map(request => blobReceipt(request.payload)));
    assert.equal(media.title, "First line");
    if (mode !== "publish") assert.equal(failed.at(-1).payload.text, "First line\nFull caption");
    else assert.equal(media.description, "First line\nFull caption");
    h.respond = null; await h.publishFromComposer();
    const retried = requests.slice(failed.length);
    assert.equal(retried.length, failed.length);
    retried.forEach((request, index) => {
      assert.deepEqual(request.payload, failed[index].payload);
      assert.equal(request.idempotency_key, failed[index].idempotency_key);
      assert.equal(request.request_key, failed[index].request_key);
      assert.deepEqual(request.origin, failed[index].origin);
    });
    assert.deepEqual(Array.from(h.drafts.current.media), []);
    assert.equal(el("compose-preview").hidden, true);
  });

  test(`${mode} rejects equal uploaded digests without dropping files or publishing a partial album`, async () => {
    const { h, el, requests } = harness();
    if (mode === "publish") h.startPublishComposer(); else h.startSocialTextComposer(mode, "duplicate-target");
    const files = [image("first.png"), image("same-content.png")];
    h.select("compose-media", files); await h.publishFromComposer();
    assert.deepEqual(requests.map(request => request.method), ["babel.media.blob.put.v1", "babel.media.blob.put.v1"]);
    assert.deepEqual(Array.from(h.drafts.current.media), files); assert.equal(h.drafts.current.pending, null);
    assert.match(el("author-status").textContent, /same media.*duplicate/);
    assert.equal(h.composerPanel.hidden, false);
  });
}

test("second album upload failure publishes nothing and retry starts with the entire retained album", async () => {
  const { h, el, requests } = harness(); h.startPublishComposer();
  const files = [image(), new File(["second"], "second.mp4", { type: "video/mp4" })];
  h.select("compose-media", files);
  h.respond = async request => { if (request.payload.media_type === "video/mp4") throw new Error("second upload failed"); };
  await h.publishFromComposer();
  assert.deepEqual(requests.map(request => request.method), ["babel.media.blob.put.v1", "babel.media.blob.put.v1"]);
  assert.deepEqual(Array.from(h.drafts.current.media), files); assert.match(el("author-status").textContent, /second upload failed/);
  h.respond = null; await h.publishFromComposer();
  assert.deepEqual(requests.slice(2).map(request => request.method), ["babel.media.blob.put.v1", "babel.media.blob.put.v1", "babel.object.publish_media.v1"]);
  assert.deepEqual(requests[2].payload, requests[0].payload); assert.deepEqual(requests[3].payload, requests[1].payload);
});

test("media drafts and previews remain independent across social targets while bundles remain publish-only", () => {
  const { h, el, created, revoked, stored } = harness();
  h.startPublishComposer(); h.type("app caption"); h.select("compose-bundle", [app()]);
  const bundle = h.drafts.current.bundle;
  const targets = [["reply", "a"], ["reply", "b"], ["share", "a"], ["share", "b"]];
  const files = targets.map((_, index) => new File([`private ${index}`], `private-${index}.ogg`, { type: "audio/ogg" }));
  targets.forEach(([mode, parent], index) => {
    h.startSocialTextComposer(mode, parent);
    assert.deepEqual(Array.from(h.drafts.current.media), []); assert.equal(h.composeText.value, "");
    assert.equal(el("bundle-picker").hidden, true); assert.equal(el("compose-bundle").disabled, true);
    h.select("compose-bundle", [app()]); assert.equal(h.drafts.current.bundle, null);
    h.type(`caption ${index}`); h.select("compose-media", [files[index]]);
  });
  targets.forEach(([mode, parent], index) => {
    h.startSocialTextComposer(mode, parent);
    assert.deepEqual(Array.from(h.drafts.current.media), [files[index]]); assert.equal(h.composeText.value, `caption ${index}`);
    assert.equal(el("compose-media-name").textContent, files[index].name);
    assert.equal(el("compose-preview-audio").src, created.at(-1).url);
    h.toggleComposer(false); h.startSocialTextComposer(mode, parent);
    assert.deepEqual(Array.from(h.drafts.current.media), [files[index]]);
  });
  h.startPublishComposer();
  assert.equal(h.drafts.current.bundle, bundle); assert.equal(h.composeText.value, "app caption");
  assert.equal(el("bundle-picker").hidden, false); assert.equal(el("compose-bundle").disabled, false);
  assert.equal(el("compose-preview").hidden, true);
  assert.deepEqual(revoked, created.map(item => item.url));
  for (const value of stored.values()) {
    assert.deepEqual(Object.keys(JSON.parse(value)).sort(), ["operationId", "text"]);
    assert.equal(value.includes("private-"), false);
  }
});
