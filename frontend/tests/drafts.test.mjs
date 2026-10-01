import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createHash, webcrypto } from "node:crypto";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import * as sdk from "@babble-protocol/sdk";
import { mediaKinds, mediaResource } from "./media-modules.mjs";

const globals = { URL, Request, Response, Headers, EventTarget, Event, TextEncoder, TextDecoder, Uint8Array,
  crypto: webcrypto, fetch, AbortController, AbortSignal, structuredClone, console, Date, Error };
function module(name, dependencies = {}) {
  const exports = {};
  const code = ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText;
  vm.runInNewContext(code, { ...globals, exports, require: (id) => dependencies[id] });
  return exports;
}
const { Drafts, draftTransport } = module("drafts");
const { composerAlbumError } = module("composer", { lucide: {}, "./media-kind": mediaKinds });
const { BabbleFrontendClient } = module("protocol", { "@babble-protocol/sdk": sdk, "./media-resource": mediaResource,
  "./browser-invocations": module("browser-invocations", { "@babble-protocol/sdk": sdk }),
  "./invocations": module("invocations", { "@babble-protocol/sdk": sdk }) });
const { Accounts } = module("accounts");
const owner = { origin: "https://babble.test", identityId: "alice" };
const publish = { mode: "publish", parent: null };
const reply = (parent = "a") => ({ mode: "reply", parent });
function storage() {
  const entries = new Map();
  return { entries, getItem: (key) => entries.get(key) ?? null,
    setItem: (key, value) => entries.set(key, value), removeItem: (key) => entries.delete(key) };
}
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}
function blobReceipt(payload) {
  const bytes = Buffer.from(payload.bytes_hex, "hex");
  // Stable fixture IDs distinguish changed uploads without claiming backend hash validation.
  const integrity = createHash("sha256").update(bytes).digest("hex");
  return { integrity, uri: `babble://blobs/${integrity}`, media_type: payload.media_type, size_bytes: bytes.length };
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
      assert.notEqual(body.method, "babble.capabilities.grant.v1", "social consent must never request a reusable grant");
      assert.ok(!/^babble\.social\.(follow|unfollow|share|reply)\.v/.test(body.method), "host social actions use invocation routes");
      const result = body.method === "babble.media.blob.put.v1" ? { blob: blobReceipt(body.payload) }
        : { object: { id: body.method === "babble.object.publish.v1" ? "controller" : "published" } };
      return response({ protocol: body.protocol, id: body.id, result, error: null });
    }
    assert.ok(session, "invocation routes require an authenticated session");
    assert.ok(init.signal instanceof AbortSignal);
    assert.equal(init.signal.aborted, false);
    if (path === "/invocations/v1/recover") {
      assert.equal(init.method, "POST");
      assert.equal(headers.get("x-babble-host-document"), null);
      assert.equal(headers.get("x-babble-surface-document"), null);
      const existing = byKey.get(`${session.token}:${body.request_key}`);
      if (existing?.value.state.kind !== "completed") return new Response(null, { status: 204 });
      const payload = { target_object_id: body.payload.target_object_id ?? body.object_id,
        text: /^babble\.social\.(reply|share)$/.test(body.method) ? (body.payload.text ?? "").trim() : null,
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
    const documentId = headers.get("x-babble-host-document"), document = documents.get(documentId);
    assert.ok(document && !document.closed); assert.equal(document.token, session.token);
    assert.equal(headers.get("x-babble-surface-document"), null);
    if (path === "/invocations/v1/prepare") {
      assert.equal(init.method, "POST"); assert.equal(body.origin.kind, "host_action");
      assert.equal(body.origin.document_id, documentId); assert.equal(body.object_id, document.objectId);
      assert.match(body.method, /^babble\.social\.(follow|unfollow|share|reply)$/);
      assert.equal(body.payload.author_id, session.identity.id);
      assert.equal(body.timeout_ms, 30_000); assert.ok(body.request_key);
      const textAction = /^babble\.social\.(reply|share)$/.test(body.method);
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
      kind: value.payload.media ? "babble.media" : "babble.text", schema: value.payload.media ? "babble.schema.media.v1" : "babble.schema.text.v1",
      protocol: { name: "babble", version: 1 }, payload: { text: value.payload.text },
      provenance: { parent: value.payload.target_object_id, forked_from: null, remixed_from: [] }, relations: [],
      resources: value.payload.media?.resources ?? [], surfaces: [], capabilities: [] };
    const edge = { id: `edge_${value.invocation_id}`, source: object.id, target: value.payload.target_object_id,
      relation: value.method === "babble.social.reply" ? "reply_to" : "quotes", author: value.actor_id,
      created_at: value.created_at, signature, origin: "HumanAssertion", metadata: {} };
    const receipt = { request: { id: value.invocation_id, fingerprint: digest(value.payload), author: value.actor_id },
      outcome: { object: object.id, edges: [edge.id], event: `evt_${value.invocation_id}` } };
    value.result = { object, edge, receipt }; value.state = { kind: "completed", outcome: { kind: "publication", receipt: receipt.request.id } };
    value.revision = 2; effects.push(structuredClone(value.result));
    if (h.afterExecute) await h.afterExecute(value);
    return response(value);
  } };
}

test("mode and parent have independent drafts; cancellation and reload restore text", () => {
  const store = storage();
  const drafts = new Drafts(owner, store);
  const targets = [publish, reply(), reply("b"), { mode: "share", parent: "a" }];
  targets.forEach((target, index) => {
    drafts.open(target);
    assert.equal(drafts.current.text, "");
    drafts.edit(`draft ${index}`, []);
    drafts.close();
  });
  for (const controller of [drafts, new Drafts(owner, store)]) {
    targets.forEach((target, index) => {
      controller.open(target);
      assert.equal(controller.current.text, `draft ${index}`);
    });
  }
});

test("account, guest, and origin ownership never migrate draft content", () => {
  const drafts = new Drafts(owner, storage());
  drafts.open(reply()); drafts.edit("alice", []);
  for (const next of [{ ...owner, identityId: null }, { ...owner, identityId: "bob" },
    { ...owner, origin: "https://other.test" }]) {
    drafts.setOwner(next);
    assert.equal(drafts.current.text, "");
    drafts.edit("other content", []);
  }
  drafts.setOwner(owner);
  assert.equal(drafts.current.text, "alice");
});

test("Files stay in memory and never enter durable storage", () => {
  const store = storage();
  const drafts = new Drafts(owner, store);
  const media = new File(["private image bytes"], "private-name.png", { type: "image/png" });
  drafts.open(publish); drafts.edit("caption", [media]);
  drafts.open(reply()); drafts.open(publish);
  assert.deepEqual(Array.from(drafts.current.media), [media]);
  const persisted = [...store.entries.values()].join("");
  assert.ok(!persisted.includes("private"));
  const restored = new Drafts(owner, store); restored.open(publish);
  assert.equal(restored.current.text, "caption");
  assert.deepEqual(Array.from(restored.current.media), []);
});

test("album snapshots preserve file identity and order without accepting caller mutation", () => {
  const drafts = new Drafts(owner, storage()); drafts.open(publish);
  const a = new File(["a"], "same.png", { type: "image/png" });
  const b = new File(["b"], "same.png", { type: "image/png" });
  const files = [a, b]; drafts.edit("album", files);
  const snapshot = drafts.current.media;
  const revision = drafts.current.revision;
  assert.ok(Object.isFrozen(snapshot)); assert.notEqual(snapshot, files);
  files.reverse(); files.pop();
  assert.equal(snapshot[0], a); assert.equal(snapshot[1], b);
  drafts.edit("album", [a, b]);
  assert.equal(drafts.current.revision, revision);
  assert.equal(drafts.current.media, snapshot, "equivalent reference order is a no-op");
  const pending = drafts.begin();
  assert.equal(pending.media, snapshot); assert.ok(Object.isFrozen(pending.media));
  drafts.edit("album", [b, a]);
  assert.equal(drafts.current.revision, revision + 1);
  assert.equal(pending.media[0], a); assert.equal(drafts.current.media[0], b);
  assert.equal(drafts.finish(pending, true), false, "completion cannot erase a reordered album");
  assert.deepEqual(Array.from(drafts.current.media), [b, a]);
  const replacement = new File(["b"], "same.png", { type: "image/png" });
  drafts.edit("album", [replacement, a]);
  assert.equal(drafts.current.revision, revision + 2, "equal metadata does not make distinct File references identical");
});

test("album failures and reopened pending sessions preserve order, identity and retry operation", () => {
  const drafts = new Drafts(owner, storage()); drafts.open(publish);
  const files = [new File(["first"], "first.png"), new File(["second"], "second.webm")];
  drafts.edit("caption", files);
  const first = drafts.begin(); drafts.finish(first, false);
  const retry = drafts.begin();
  assert.equal(retry.media, first.media); assert.equal(retry.operationId, first.operationId);
  assert.equal(drafts.begin(), null);
  drafts.close(); drafts.open(publish);
  assert.equal(drafts.finish(retry, true), false);
  assert.deepEqual(Array.from(drafts.current.media), files);
  const final = drafts.begin(); drafts.finish(final, true);
  assert.deepEqual(Array.from(drafts.current.media), []);
  assert.ok(Object.isFrozen(drafts.current.media));
  assert.notEqual(drafts.current.operationId, first.operationId);
});

test("albums and bundles are exclusive while clearing media preserves an existing bundle", () => {
  const drafts = new Drafts(owner, storage()); drafts.open(publish);
  const files = [new File(["a"], "a.png"), new File(["b"], "b.png")];
  drafts.edit("caption", files);
  drafts.editBundle({ files: [new File(["app"], "index.html")], entryPath: "index.html" });
  assert.deepEqual(Array.from(drafts.current.media), []);
  const bundle = drafts.current.bundle;
  drafts.edit("updated caption", []); assert.equal(drafts.current.bundle, bundle);
  drafts.edit("updated caption", files);
  assert.equal(drafts.current.bundle, null); assert.deepEqual(Array.from(drafts.current.media), files);
});

test("application drafts freeze attachments, isolate owners, and never persist files", () => {
  const store = storage();
  const drafts = new Drafts(owner, store);
  const file = new File(["private application bytes"], "index.html", { type: "text/html" });
  const attachment = { files: [file], entryPath: "index.html" };
  drafts.open(publish);
  drafts.edit("caption", [new File(["image"], "image.png")]);
  drafts.editBundle(attachment);
  attachment.files.length = 0;
  assert.equal(drafts.current.bundle.files[0], file);
  assert.deepEqual(Array.from(drafts.current.media), []);
  const ticket = drafts.begin();
  assert.ok(Object.isFrozen(ticket.bundle) && Object.isFrozen(ticket.bundle.files));
  drafts.finish(ticket, false);
  drafts.close(); drafts.open(publish);
  assert.equal(drafts.current.bundle.files[0], file);
  drafts.setOwner({ ...owner, identityId: "bob" });
  assert.equal(drafts.current.bundle, null);
  drafts.setOwner(owner);
  assert.equal(drafts.current.bundle.files[0], file);
  drafts.open(reply()); drafts.editBundle({ files: [file], entryPath: "index.html" });
  assert.equal(drafts.current.bundle, null);
  assert.ok(![...store.entries.values()].join("").includes("private"));
  assert.ok(![...store.entries.values()].join("").includes("index.html"));
  const restored = new Drafts(owner, store); restored.open(publish);
  assert.equal(restored.current.bundle, null);
});

test("bundle success cannot erase a changed application and images replace bundle selection", () => {
  const drafts = new Drafts(owner, storage());
  const file = new File(["app"], "index.html");
  drafts.open(publish);
  drafts.editBundle({ files: [file], entryPath: "index.html" });
  const pending = drafts.begin();
  drafts.editBundle({ files: [file], entryPath: "other.html" });
  assert.equal(drafts.finish(pending, true), false);
  assert.equal(drafts.current.bundle.entryPath, "other.html");
  const complete = drafts.begin();
  assert.equal(drafts.finish(complete, true), true);
  assert.equal(drafts.current.bundle, null);
  drafts.editBundle({ files: [file], entryPath: "index.html" });
  drafts.edit("image caption", [new File(["image"], "image.png")]);
  assert.equal(drafts.current.bundle, null);
});

test("raw capability edits, including malformed JSON, survive close/reopen and account changes without leaking or persisting", () => {
  const store = storage();
  const drafts = new Drafts(owner, store);
  const file = new File(["app"], "index.html");
  drafts.open(publish);
  const attachment = { files: [file], entryPath: "index.html", capabilitiesText: "[]" };
  drafts.editBundle(attachment);
  const submitted = drafts.begin();
  const malformed = '[{"id":"private.capability","scope":';
  attachment.capabilitiesText = malformed;
  drafts.editBundle(attachment);
  attachment.capabilitiesText = "[]";
  attachment.files.length = 0;
  assert.equal(submitted.bundle.capabilitiesText, "[]");
  assert.equal(drafts.current.bundle.capabilitiesText, malformed);
  assert.equal(drafts.current.bundle.files[0], file);
  assert.ok(Object.isFrozen(drafts.current.bundle));
  assert.equal(drafts.finish(submitted, true), false);
  drafts.close(); drafts.open(publish);
  assert.equal(drafts.current.bundle.capabilitiesText, malformed);
  for (const next of [{ ...owner, identityId: "bob" }, { ...owner, identityId: null }, { ...owner, origin: "https://other.test" }]) {
    drafts.setOwner(next);
    assert.equal(drafts.current.bundle, null);
    drafts.editBundle({ files: [file], entryPath: "index.html", capabilitiesText: "other raw edit" });
  }
  drafts.setOwner(owner);
  assert.equal(drafts.current.bundle.capabilitiesText, malformed);
  const retry = drafts.begin();
  assert.equal(retry.operationId, submitted.operationId);
  assert.equal(retry.bundle.capabilitiesText, malformed);
  drafts.finish(retry, false);
  const repeated = drafts.begin();
  assert.equal(repeated.bundle, retry.bundle);
  assert.equal(repeated.operationId, retry.operationId);
  for (const value of store.entries.values()) {
    assert.deepEqual(Object.keys(JSON.parse(value)).sort(), ["operationId", "text"]);
    assert.ok(!value.includes("private.capability") && !value.includes("capabilitiesText") && !value.includes("index.html"));
  }
  const restored = new Drafts(owner, store); restored.open(publish);
  assert.equal(restored.current.bundle, null);
});

test("capability text defaults only when omitted, never for an empty or invalid editor value", () => {
  const drafts = new Drafts(owner, storage());
  drafts.open(publish);
  const attachment = { files: [new File(["app"], "index.html")], entryPath: "index.html" };
  drafts.editBundle(attachment);
  assert.equal(drafts.current.bundle.capabilitiesText, "[]");
  for (const capabilitiesText of ["", "[", "null", " \n "]) {
    drafts.editBundle({ ...attachment, capabilitiesText });
    const submitted = drafts.begin();
    assert.equal(submitted.bundle.capabilitiesText, capabilitiesText);
    drafts.finish(submitted, false);
    drafts.close(); drafts.open(publish);
    assert.equal(drafts.current.bundle.capabilitiesText, capabilitiesText);
  }
});

test("failure and duplicate submission retain the exact retry snapshot", () => {
  const drafts = new Drafts(owner, storage());
  drafts.open(reply()); drafts.edit("  keep whitespace  ", []);
  const first = drafts.begin();
  assert.equal(drafts.begin(), null);
  assert.equal(drafts.finish(first, false), true);
  assert.equal(drafts.current.text, "  keep whitespace  ");
  const retry = drafts.begin();
  assert.equal(retry.operationId, first.operationId);
  assert.equal(retry.text, first.text);
  assert.equal(drafts.finish(first, true), false);
  assert.equal(drafts.current.pending, retry);
});

test("success clears only the submitted draft and preserves in-flight edits and reopened sessions", () => {
  for (const change of ["switch", "edit", "reopen", "account"]) {
    const store = storage(); const drafts = new Drafts(owner, store);
    drafts.open(reply()); drafts.edit("original", []);
    const ticket = drafts.begin();
    if (change === "switch") { drafts.open(reply("b")); drafts.edit("new parent", []); }
    if (change === "edit") drafts.edit("new edit", []);
    if (change === "reopen") { drafts.close(); drafts.open(reply()); }
    if (change === "account") { drafts.setOwner({ ...owner, identityId: "bob" }); drafts.edit("bob", []); }
    assert.equal(drafts.finish(ticket, true), false);
    assert.equal(drafts.current.text, { switch: "new parent", edit: "new edit", reopen: "original", account: "bob" }[change]);
    if (change === "switch") { drafts.open(reply()); assert.equal(drafts.current.text, ""); }
    if (change === "account") {
      const restored = new Drafts({ ...owner, identityId: "bob" }, store); restored.open(reply());
      assert.equal(restored.current.text, "bob");
    }
  }
});

test("logout and same-account relogin invalidate old publication authority", () => {
  const drafts = new Drafts(owner, storage());
  drafts.open(reply()); drafts.edit("pending", []);
  const ticket = drafts.begin();
  drafts.setOwner({ ...owner, identityId: null }); drafts.setOwner(owner);
  assert.equal(drafts.owns(ticket), false);
  drafts.finish(ticket, true);
  assert.equal(drafts.current.text, "pending");
});

test("disabled, malformed and oversized storage stay recoverable in memory", () => {
  const store = storage(); const drafts = new Drafts(owner, store);
  drafts.open(publish); drafts.edit("saved", []);
  drafts.edit("x".repeat(65537), []);
  assert.match(drafts.storageError, /limit/);
  assert.equal(drafts.current.text.length, 65537);
  assert.equal(JSON.parse([...store.entries.values()][0]).text, "saved");
  store.setItem(drafts.current.key, "{");
  const malformed = new Drafts(owner, store); malformed.open(publish);
  assert.equal(malformed.current.text, "");
  assert.match(malformed.storageError, /storage unavailable/);
  const denied = new Drafts(owner, { getItem() { throw Error("denied"); }, setItem() { throw Error("denied"); } });
  denied.open(publish); denied.edit("intact", []);
  assert.equal(denied.current.text, "intact");
});

test("idempotency survives reload and retry, varies with payload, and rotates after success", async () => {
  const store = storage(); const drafts = new Drafts(owner, store);
  drafts.open(publish); drafts.edit("hello", []);
  const ticket = drafts.begin();
  const requests = [];
  const capture = async (_url, init) => { requests.push(JSON.parse(init.body)); return new Response("{}"); };
  const send = async (operationId, text, generatedKey) => draftTransport(capture, () => true, operationId)(`${owner.origin}/rpc`, {
    body: JSON.stringify({ protocol: "babble.rpc.v1", method: "publish", payload: { text }, idempotency_key: generatedKey }),
  });
  await send(ticket.operationId, "hello", "random-1");
  drafts.finish(ticket, false);
  const restored = new Drafts(owner, store); restored.open(publish);
  await send(restored.begin().operationId, "hello", "random-2");
  assert.equal(requests[0].idempotency_key, requests[1].idempotency_key);
  await send(ticket.operationId, "edited", "random-3");
  assert.notEqual(requests[0].idempotency_key, requests[2].idempotency_key);
  const retry = drafts.begin(); drafts.finish(retry, true);
  drafts.edit("hello", []);
  assert.notEqual(drafts.begin().operationId, ticket.operationId);
});

const main = ts.createSourceFile("main.ts", readFileSync(new URL("../src/app/main.ts", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true);
const names = new Set(["publishFromComposer", "startPublishComposer", "startSocialTextComposer", "saveComposerDraft",
  "renderComposerDraft", "toggleComposer", "syncAccount", "publishMediaWithAuthor", "selectedMediaFiles", "mediaTitle", "mediaDescription"]);
const handlers = ts.transpileModule(main.statements.filter((node) => ts.isFunctionDeclaration(node) && names.has(node.name?.text))
  .map((node) => node.getText(main)).join("\n"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None },
}).outputText;

function harness() {
  const field = () => ({ value: "", textContent: "", disabled: false, files: { item: () => null }, focus() {} });
  const requests = [], httpRequests = [], navigations = [], refreshed = [];
  const h = {
    ...globals, BabbleFrontendClient, draftTransport, composerAlbumError,
    composerView: { render(draft) { h.renderedComposerTarget = draft?.target; }, visibility() {} },
    drafts: new Drafts(owner, storage()),
    feedPreferences: { account() {} }, updateEmptyFeed() {},
    bundlePicker: { error: null, render() {} },
    composeText: field(), composeMedia: field(), composeSubmit: field(), authorHandleInput: field(), composeForm: {},
    composerPanel: { hidden: true }, profileDropdown: null, toggleProfileButton: null,
    author: { identityId: "alice", handle: "Alice" },
    accounts: { origin: new URL(owner.origin), current: { identity: { id: "alice", handle: "Alice" }, token: "session-a" } },
    accountPanel: { open() { h.loginOpened = true; } },
    profiles: { close() { h.profileClosed = true; } },
    objectVisits: { clear() {} }, quotes: { clear() {}, refresh() {} }, followingDirty: false, followingControls: { account() {} }, followingFeed: { clear() {} }, profileReturn: null,
    safetyControls: { account() {}, ensure: async () => null }, moderationControls: { account() {} },
    safetyUnavailable: false, ensureFeedSafety: async () => {},
    reactions: { select() {} },
    deck: { replaceChildren() {} }, accountFeedRefresh: Promise.resolve(),
    followingToolbar: { hidden: true }, followingMore: { hidden: true }, followingSignIn: { hidden: true }, followingStatus: { textContent: "" },
    searchInput: { value: "" }, cards: [], currentIndex: 0, loadSequence: 0, seenThisSession: new Set(),
    setAnimatedVisibility: (element, open) => { element.hidden = !open; },
    toggleProfile() {}, toggleSettings() {}, toggleHelp() {}, closeSurface() {},
    isHidden: (element) => element.hidden,
    setAuthorStatus: (message, state) => { h.status = { message, state }; },
    compactId: (id) => id, errorMessage: (error) => error.message,
    loadFeed: async (...args) => { navigations.push(args); },
    client: { describeObject: async (object) => object, objectToCard: async ({ object }) => object },
    conversations: { clear() {}, refresh: async (...args) => { refreshed.push(args); } },
    DataTransfer: class {
      values = [];
      items = { add: (file) => this.values.push(file) };
      get files() { return { item: (index) => this.values[index] ?? null }; }
    },
  };
  const server = socialFixture(h, requests, httpRequests);
  h.accounts.fetch = server.fetch;
  vm.createContext(h); vm.runInContext(handlers, h);
  h.type = (text) => { h.composeText.value = text; h.saveComposerDraft(); };
  h.changeAccount = (identityId, token = "session-b") => {
    h.accounts.current = identityId ? { identity: { id: identityId, handle: identityId }, token } : null;
    h.syncAccount();
  };
  return { h, requests, httpRequests, server, navigations, refreshed };
}

function installProductionPanelAnimation(h) {
  const frames = new Map();
  const timers = new Map();
  let nextId = 0;
  h.composerPanel.dataset = {};
  h.composerPanel.getAttribute = (name) => name === "hidden" && h.composerPanel.hidden ? "" : null;
  Object.assign(h, {
    PANEL_ANIMATION_MS: 220,
    panelAnimations: new WeakMap(),
    requestAnimationFrame: (callback) => { frames.set(++nextId, callback); return nextId; },
    cancelAnimationFrame: (id) => frames.delete(id),
    window: {
      matchMedia: () => ({ matches: false }),
      setTimeout: (callback) => { timers.set(++nextId, callback); return nextId; },
      clearTimeout: (id) => timers.delete(id),
    },
  });
  const source = main.statements.filter((node) => ts.isFunctionDeclaration(node)
    && ["setAnimatedVisibility", "isHidden"].includes(node.name?.text)).map((node) => node.getText(main)).join("\n");
  vm.runInContext(ts.transpileModule(source, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None },
  }).outputText, h);
  const flush = (queue) => { const callbacks = [...queue.values()]; queue.clear(); callbacks.forEach((callback) => callback()); };
  return { flushFrames: () => flush(frames), flushTimers: () => flush(timers) };
}

test("account status survives login with no draft and account changes with a closed draft", () => {
  const { h } = harness();
  h.changeAccount(null); h.changeAccount("alice");
  assert.equal(h.status.message, "Signed in as alice");
  h.startPublishComposer(); h.type("private Alice draft"); h.toggleComposer(false);
  h.changeAccount("bob");
  assert.equal(h.status.message, "Signed in as bob");
  assert.equal(h.composeText.value, "");
  assert.equal(h.composeMedia.value, "");
});

test("logout during the real closing animation preserves account status and clears private fields immediately", () => {
  const { h } = harness();
  const animation = installProductionPanelAnimation(h);
  h.startPublishComposer(); h.type("private Alice draft");
  h.drafts.edit(h.composeText.value, [new File(["private"], "private.png", { type: "image/png" })]);
  h.renderComposerDraft();
  animation.flushFrames();
  assert.equal(h.composerPanel.dataset.state, "open");
  h.drafts.invalidate();
  h.toggleComposer(false);
  assert.equal(h.composerPanel.hidden, false, "closing animation has not hidden the panel yet");
  h.changeAccount(null);
  assert.equal(h.status.message, "Not signed in");
  assert.equal(h.composeText.value, "");
  assert.equal(h.composeMedia.value, "");
  assert.deepEqual(Array.from(h.drafts.current.media), []);
  assert.equal(h.composeSubmit.disabled, true);
  animation.flushTimers();
  assert.equal(h.composerPanel.hidden, true);
  assert.equal(h.status.message, "Not signed in");
  h.changeAccount("alice");
  assert.equal(h.status.message, "Signed in as alice");
  h.startPublishComposer();
  assert.equal(h.composeText.value, "private Alice draft");
});

test("reopening during close restores logical visibility before the opening animation frame", () => {
  const { h } = harness();
  const animation = installProductionPanelAnimation(h);
  h.startSocialTextComposer("reply", "a");
  animation.flushFrames();
  h.toggleComposer(false);
  h.startSocialTextComposer("reply", "b");
  h.type("private Alice reply");
  h.changeAccount("bob");
  assert.equal(h.renderedComposerTarget.mode, "reply");
  assert.equal(h.renderedComposerTarget.parent, "b");
  assert.equal(h.composeText.value, "");
  animation.flushTimers(); animation.flushFrames();
  assert.equal(h.composerPanel.hidden, false);
  assert.equal(h.composerPanel.dataset.state, "open");
});

test("production start/close controls reproduce and fix cross-target text leakage", () => {
  const { h } = harness();
  h.startSocialTextComposer("reply", "a"); h.type("reply A"); h.toggleComposer(false);
  h.startSocialTextComposer("reply", "b"); assert.equal(h.composeText.value, ""); h.type("reply B");
  h.startSocialTextComposer("share", "a"); assert.equal(h.composeText.value, ""); h.type("share A");
  h.startPublishComposer(); assert.equal(h.composeText.value, ""); h.type("top level");
  for (const [mode, target, expected] of [["reply", "a", "reply A"], ["reply", "b", "reply B"], ["share", "a", "share A"]]) {
    h.startSocialTextComposer(mode, target); assert.equal(h.composeText.value, expected);
    assert.equal(h.composeSubmit.textContent, mode === "reply" ? "Reply" : "Share");
  }
  h.startPublishComposer(); assert.equal(h.composeText.value, "top level");
});

test("production handlers freeze actor/target and ignore repeated submits while another composer opens", async () => {
  const { h, requests, navigations, refreshed } = harness(); const gate = deferred(); const started = deferred();
  h.respond = async () => { started.resolve(); await gate.promise; };
  h.startSocialTextComposer("reply", "a"); h.type("for A");
  const pending = h.publishFromComposer(); await started.promise;
  await h.publishFromComposer();
  h.startSocialTextComposer("share", "b"); h.type("for B");
  gate.resolve(); await pending;
  assert.equal(requests.length, 2);
  assert.equal(requests.at(-1).payload.target_object_id, "a");
  assert.equal(requests.at(-1).payload.author_id, "alice");
  assert.equal(requests.at(-1).payload.text, "for A");
  assert.equal(h.composeText.value, "for B"); assert.equal(h.composeSubmit.textContent, "Share");
  assert.equal(h.composerPanel.hidden, false); assert.equal(h.composeSubmit.disabled, false);
  assert.equal(navigations.length + refreshed.length, 0);
});

test("production in-flight edits and close/reopen sessions survive completion without navigation", async () => {
  for (const edit of [true, false]) {
    const { h, navigations } = harness(); const gate = deferred(); const started = deferred();
    h.respond = async () => { started.resolve(); await gate.promise; };
    h.startPublishComposer(); h.type("original");
    const pending = h.publishFromComposer(); await started.promise;
    if (edit) h.type("new content");
    else { h.toggleComposer(false); h.startPublishComposer(); }
    gate.resolve(); await pending;
    assert.equal(h.composeText.value, edit ? "new content" : "original");
    assert.equal(h.composerPanel.hidden, false); assert.equal(h.composeSubmit.disabled, false);
    assert.equal(navigations.length, 0);
  }
});

test("production retry retains text and idempotency; success clears only its draft", async () => {
  const { h, requests, navigations } = harness();
  h.startSocialTextComposer("reply", "a"); h.type("unrelated reply");
  h.startPublishComposer(); h.type("retry me");
  h.respond = async () => { throw Error("connection lost after write"); };
  await h.publishFromComposer();
  assert.equal(h.composeText.value, "retry me"); assert.match(h.status.message, /failed/);
  assert.equal(h.composeSubmit.disabled, false);
  h.respond = null; await h.publishFromComposer();
  assert.equal(requests[0].idempotency_key, requests[1].idempotency_key);
  assert.equal(h.composeText.value, ""); assert.equal(h.composerPanel.hidden, true);
  assert.equal(navigations.length, 1);
  h.startSocialTextComposer("reply", "a"); assert.equal(h.composeText.value, "unrelated reply");
});

test("invalid application selection and missing caption cannot fall back to text publication", async () => {
  const { h, requests } = harness();
  h.startPublishComposer(); h.type("valid caption");
  h.bundlePicker.error = "Bundle requires an HTML entry";
  await h.publishFromComposer();
  assert.equal(requests.length, 0);
  assert.match(h.status.message, /HTML entry/);
  assert.equal(h.drafts.current.pending, null);
  h.bundlePicker.error = null;
  h.type("");
  h.drafts.editBundle({ files: [new File(["app"], "index.html")], entryPath: "index.html" });
  await h.publishFromComposer();
  assert.equal(requests.length, 0);
  assert.match(h.status.message, /Object text/);
  assert.equal(h.drafts.current.pending, null);
});

test("production composer keeps the selected application on publication failure", async () => {
  const { h, requests, navigations } = harness();
  const file = new File(["app"], "index.html");
  h.startPublishComposer(); h.type("application");
  h.drafts.editBundle({ files: [file], entryPath: "index.html" });
  let calls = 0;
  h.publishBundle = async (_publisher, identity, text, bundle) => {
    calls++;
    assert.equal(identity, "alice");
    assert.equal(text, "application");
    assert.equal(bundle.files[0], file);
    throw Error("upload interrupted");
  };
  await h.publishFromComposer();
  assert.equal(calls, 1);
  assert.equal(requests.length + navigations.length, 0);
  assert.equal(h.drafts.current.bundle.files[0], file);
  assert.match(h.status.message, /upload interrupted/);
  assert.equal(h.composeSubmit.disabled, false);
});

test("production account change during social capability setup prevents subsequent writes", async () => {
  const { h, requests, refreshed } = harness(); const gate = deferred(); const started = deferred();
  h.respond = async () => { started.resolve(); await gate.promise; };
  h.startSocialTextComposer("reply", "a"); h.type("alice reply");
  const pending = h.publishFromComposer(); await started.promise;
  h.changeAccount(null); h.changeAccount("bob"); h.type("bob reply");
  gate.resolve(); await pending;
  assert.equal(requests.length, 1); assert.equal(requests[0].token, "session-a");
  assert.equal(h.composeText.value, "bob reply"); assert.equal(refreshed.length, 0);
  h.changeAccount("alice"); assert.equal(h.composeText.value, "alice reply");
});

test("production media read is bound to its original session before blob or Object writes", async () => {
  const { h, requests } = harness(); const bytes = deferred();
  h.startPublishComposer(); h.type("caption");
  const file = { name: "image.png", type: "image/png", size: 1, arrayBuffer: () => bytes.promise };
  h.drafts.edit("caption", [file]); h.renderComposerDraft();
  const pending = h.publishFromComposer();
  h.changeAccount("bob"); h.type("bob caption");
  bytes.resolve(new ArrayBuffer(1)); await pending;
  assert.equal(requests.length, 0); assert.equal(h.composeText.value, "bob caption");
});

test("late successful write cannot clear another owner's persisted draft or navigate", async () => {
  const { h, navigations } = harness(); const gate = deferred(); const started = deferred();
  h.respond = async () => { started.resolve(); await gate.promise; };
  h.startPublishComposer(); h.type("alice text");
  const pending = h.publishFromComposer(); await started.promise;
  h.changeAccount("bob"); h.type("bob text");
  gate.resolve(); await pending;
  assert.equal(h.composeText.value, "bob text"); assert.equal(navigations.length, 0);
  h.startPublishComposer(); assert.equal(h.composeText.value, "bob text");
});

test("late real login completion preserves the latest target without adopting guest text", async () => {
  const { h } = harness(); const gate = deferred();
  const accounts = new Accounts(owner.origin, null, () => gate.promise);
  h.accounts = accounts; accounts.addEventListener("change", h.syncAccount); h.syncAccount();
  h.startSocialTextComposer("reply", "a"); h.type("guest A");
  const pending = accounts.login("alice", "test-password");
  h.startSocialTextComposer("share", "b"); h.type("guest B");
  gate.resolve(Response.json({ identity: { id: "alice", handle: "Alice" }, token: "a".repeat(64),
    expires_at: new Date(Date.now() + 3600000).toISOString() }));
  await pending;
  assert.equal(h.drafts.current.target.mode, "share"); assert.equal(h.drafts.current.target.parent, "b");
  assert.equal(h.composeText.value, ""); assert.equal(h.composerPanel.hidden, false);
  assert.equal(h.composeSubmit.textContent, "Share");
});

test("delayed post-publication feed results cannot redirect a newly opened composer", async () => {
  const loadFeedSource = main.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === "loadFeed");
  const code = ts.transpileModule(loadFeedSource.getText(main), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None },
  }).outputText;
  for (const fail of [false, true]) {
    const { h } = harness(); const feed = deferred(); const loading = deferred();
    Object.assign(h, {
      cards: [{ id: "existing" }], currentIndex: 0, activeLens: "balanced",
      error: { dataset: {}, hidden: true }, empty: { hidden: true, querySelector: () => null }, feedSummary: { textContent: "Existing feed" },
      setStatus() {}, lensName: (name) => name, syncSearchUrl() {}, localModel: () => null,
    });
    h.client.loadFeed = () => { loading.resolve(); return feed.promise; };
    vm.runInContext(code, h);
    h.startPublishComposer(); h.type("published first");
    const pending = h.publishFromComposer(); await loading.promise;
    h.startSocialTextComposer("reply", "existing"); h.type("new reply");
    if (fail) feed.reject(Error("late feed failure"));
    else feed.resolve({ cards: [{ id: "published" }] });
    await pending;
    assert.equal(h.cards[0].id, "existing"); assert.equal(h.currentIndex, 0);
    assert.equal(h.composeText.value, "new reply"); assert.equal(h.composerPanel.hidden, false);
    assert.equal(h.feedSummary.textContent, "Existing feed");
  }
});

test("social retries recover a committed reply without another approval or effect", async () => {
  const { h, requests, httpRequests, server } = harness();
  h.startSocialTextComposer("reply", "a"); h.type("reply once");
  h.afterExecute = async () => { throw Error("lost reply response"); };
  await h.publishFromComposer();
  assert.equal(h.composeText.value, "reply once");
  h.afterExecute = null; await h.publishFromComposer();
  assert.equal(requests.length, 3);
  assert.equal(requests[0].idempotency_key, requests[2].idempotency_key);
  assert.equal(requests[1].method, "babble.social.reply");
  const recoveries = httpRequests.filter(request => request.action === "recover");
  assert.equal(recoveries.length, 2);
  assert.deepEqual(recoveries[1].body, recoveries[0].body);
  assert.equal(recoveries[1].body.request_key, requests[1].request_key);
  assert.equal(requests[1].origin.kind, "host_action");
  assert.equal(requests[1].object_id, "controller");
  assert.deepEqual(requests[0].binding.capability_grants, []);
  assert.equal(httpRequests.filter(request => request.action === "decision").length, 1);
  assert.equal(httpRequests.filter(request => request.action === "execute").length, 1);
  assert.equal(server.effects.length, 1);
  assert.equal(h.composeText.value, "", JSON.stringify(h.status));
});

test("social media drafts isolate every owner, target and mode while persisting only caption and operation ID", () => {
  const store = storage(), drafts = new Drafts(owner, store);
  const targets = [reply("a"), reply("b"), { mode: "share", parent: "a" }, { mode: "share", parent: "b" }];
  const files = targets.map((_, index) => new File([`private bytes ${index}`], `private-${index}.png`, { type: "image/png" }));
  const operationIds = [];
  targets.forEach((target, index) => {
    drafts.open(target); drafts.edit(`caption ${index}`, [files[index]]);
    operationIds.push(drafts.current.operationId);
    const ticket = drafts.begin();
    assert.deepEqual(Array.from(ticket.media), [files[index]]); assert.equal(ticket.bundle, null);
    assert.ok(Object.isFrozen(ticket)); assert.ok(Object.isFrozen(ticket.target));
    drafts.finish(ticket, false); drafts.close();
  });
  assert.equal(new Set(operationIds).size, targets.length);
  for (const otherOwner of [{ ...owner, identityId: "bob" }, { ...owner, identityId: null }, { ...owner, origin: "https://other.test" }]) {
    drafts.setOwner(otherOwner);
    for (const target of targets) {
      drafts.open(target); assert.deepEqual(Array.from(drafts.current.media), []); assert.equal(drafts.current.text, "");
      drafts.edit("other owner's caption", [new File(["other"], "other.ogg", { type: "audio/ogg" })]);
    }
  }
  drafts.setOwner(owner);
  const restored = new Drafts(owner, store);
  targets.forEach((target, index) => {
    drafts.open(target); restored.open(target);
    assert.deepEqual(Array.from(drafts.current.media), [files[index]]); assert.equal(drafts.current.text, `caption ${index}`);
    assert.equal(drafts.current.operationId, operationIds[index]);
    assert.deepEqual(Array.from(restored.current.media), []); assert.equal(restored.current.text, `caption ${index}`);
    assert.equal(restored.current.operationId, operationIds[index]);
    drafts.editBundle({ files: [new File(["app"], "index.html")], entryPath: "index.html" });
    assert.equal(drafts.current.bundle, null); assert.deepEqual(Array.from(drafts.current.media), [files[index]]);
  });
  for (const value of store.entries.values()) {
    assert.deepEqual(Object.keys(JSON.parse(value)).sort(), ["operationId", "text"]);
    assert.equal(value.includes("private"), false);
  }
});

for (const mode of ["publish", "reply", "share"]) {
  test(`${mode} account changes between album reads prevent all remaining uploads and publication`, async () => {
    const { h, requests, navigations, refreshed } = harness();
    const gate = deferred(), started = deferred();
    const files = [new File(["a"], "a.png", { type: "image/png" }),
      new File(["b"], "b.ogg", { type: "audio/ogg" }), new File(["c"], "c.mp4", { type: "video/mp4" })];
    Object.defineProperty(files[1], "arrayBuffer", { value: () => { started.resolve(); return gate.promise; } });
    if (mode === "publish") h.startPublishComposer(); else h.startSocialTextComposer(mode, "target");
    h.drafts.edit("Alice album", files); h.renderComposerDraft();
    const pending = h.publishFromComposer(); await started.promise;
    assert.equal(requests.length, 1); assert.equal(requests[0].method, "babble.media.blob.put.v1");
    h.changeAccount("bob");
    const bobFiles = [new File(["bob"], "bob.png", { type: "image/png" })];
    h.drafts.edit("Bob album", bobFiles); h.renderComposerDraft();
    await h.accountFeedRefresh;
    const accountNavigations = navigations.length;
    gate.resolve(new Uint8Array([98]).buffer); await pending;
    assert.equal(requests.length, 1); assert.equal(requests[0].token, "session-a");
    assert.deepEqual(Array.from(h.drafts.current.media), bobFiles); assert.equal(h.composeText.value, "Bob album");
    assert.equal(navigations.length, accountNavigations); assert.equal(refreshed.length, 0);
    h.changeAccount("alice");
    assert.deepEqual(Array.from(h.drafts.current.media), files); assert.equal(h.composeText.value, "Alice album");
    assert.equal(h.drafts.current.pending, null);
  });

  test(`${mode} reordered album retries retain their own payload order and mutation key`, async () => {
    const { h, requests } = harness();
    const files = [new File(["a"], "a.png", { type: "image/png" }), new File(["b"], "b.png", { type: "image/png" })];
    if (mode === "publish") h.startPublishComposer(); else h.startSocialTextComposer(mode, "target");
    const method = mode === "publish" ? "babble.object.publish_media.v1" : `babble.social.${mode}`;
    h.respond = async (request, route) => { if (route.action !== "recover" && request?.method === method) throw new Error("response lost"); };
    h.drafts.edit("caption", files); h.renderComposerDraft(); await h.publishFromComposer();
    const original = requests.at(-1), operationId = h.drafts.current.operationId;
    const originalResources = mode === "publish" ? original.payload.resources : original.payload.media.resources;
    h.drafts.edit("caption", [files[1], files[0]]); h.renderComposerDraft(); await h.publishFromComposer();
    const reordered = requests.at(-1);
    const reorderedResources = mode === "publish" ? reordered.payload.resources : reordered.payload.media.resources;
    assert.deepEqual(reorderedResources, [...originalResources].reverse());
    const key = mode === "publish" ? "idempotency_key" : "request_key";
    assert.notEqual(reordered[key], original[key]);
    assert.equal(h.drafts.current.operationId, operationId);
    await h.publishFromComposer();
    assert.deepEqual(requests.at(-1).payload, reordered.payload);
    assert.equal(requests.at(-1)[key], reordered[key]);
    assert.deepEqual(Array.from(h.drafts.current.media), [files[1], files[0]]);
  });
}

for (const mode of ["reply", "share"]) {
  test(`${mode} retries bind immutable invocation intent and controller keys to the correct media revision`, async () => {
    const { h, requests, httpRequests, server, navigations, refreshed } = harness();
    const attach = (file, caption = h.composeText.value) => {
      h.composeText.value = caption; h.drafts.edit(caption, [file]); h.renderComposerDraft();
    };
    const firstFile = new File(["first bytes"], "same-name.ogg", { type: "audio/ogg" });
    h.startSocialTextComposer(mode, "target"); attach(firstFile, "caption");
    h.respond = async (_request, route) => { if (route.action === "execute") throw new Error("execution unavailable"); };
    await h.publishFromComposer();
    await h.publishFromComposer();
    const first = requests.slice(0, 3), retry = requests.slice(3, 6);
    assert.equal(requests.length, 6);
    assert.deepEqual(first.map(request => request.method), ["babble.media.blob.put.v1", "babble.object.publish.v1", `babble.social.${mode}`]);
    assert.equal(first[0].idempotency_key, null, "content-addressed upload is idempotent by input");
    for (let index = 0; index < first.length; index++) {
      assert.deepEqual(retry[index].payload, first[index].payload);
      assert.equal(retry[index].idempotency_key, first[index].idempotency_key);
      if (index === 1) assert.match(first[index].idempotency_key, /^web-draft-[0-9a-f]{64}$/);
    }
    assert.equal(retry[2].request_key, first[2].request_key);
    assert.deepEqual(retry[2].origin, first[2].origin);
    assert.equal(httpRequests.filter(request => request.action === "decision").length, 1, "approved retries do not ask twice");
    assert.equal(server.effects.length, 0);
    assert.deepEqual(Array.from(h.drafts.current.media), [firstFile]); assert.equal(h.drafts.current.pending, null);
    const replacement = new File(["other bytes"], "same-name.ogg", { type: "audio/ogg" });
    attach(replacement); await h.publishFromComposer();
    const changed = requests.slice(6, 9);
    assert.notEqual(changed[0].payload.bytes_hex, first[0].payload.bytes_hex);
    assert.notEqual(changed[2].payload.media.resources[0].integrity, first[2].payload.media.resources[0].integrity);
    assert.notEqual(changed[2].request_key, first[2].request_key);
    assert.equal(changed[1].idempotency_key, first[1].idempotency_key);
    attach(replacement, "edited caption"); await h.publishFromComposer();
    const recaptioned = requests.slice(9, 12);
    assert.deepEqual(recaptioned[0].payload, changed[0].payload);
    assert.notEqual(recaptioned[2].request_key, changed[2].request_key);
    h.respond = null; await h.publishFromComposer();
    const successful = requests.slice(12, 15);
    assert.equal(successful[2].request_key, recaptioned[2].request_key);
    assert.equal(server.effects.length, 1);
    assert.deepEqual(Array.from(h.drafts.current.media), []); assert.equal(navigations.length + refreshed.length, 1);
    h.startSocialTextComposer(mode, "target"); attach(replacement, "edited caption");
    await h.publishFromComposer();
    assert.deepEqual(requests.at(-1).payload, successful[2].payload);
    assert.notEqual(requests.at(-1).request_key, successful[2].request_key,
      "a deliberate new post after success must not reuse the completed publication key");
  });

  for (const sessionChange of ["switch", "logout", "same-identity-relogin"]) {
    test(`${mode} ${sessionChange} during upload suppresses later writes without taking the next session's draft`, async () => {
      const { h, requests, navigations, refreshed } = harness(); const gate = deferred(), started = deferred();
      const firstFile = new File(["private media"], "private.png", { type: "image/png" });
      h.startSocialTextComposer(mode, "target"); h.drafts.edit("Alice caption", [firstFile]); h.renderComposerDraft();
      h.respond = async request => { if (request?.method === "babble.media.blob.put.v1") { started.resolve(); await gate.promise; } };
      const pending = h.publishFromComposer(); await started.promise;
      if (sessionChange === "switch") h.changeAccount("bob");
      else if (sessionChange === "logout") h.changeAccount(null);
      else { h.changeAccount(null); h.changeAccount("alice", "renewed-session"); }
      const nextFile = new File(["next media"], "next.webm", { type: "video/webm" });
      h.drafts.edit("Next session caption", [nextFile]); h.renderComposerDraft();
      await h.accountFeedRefresh;
      const accountNavigations = navigations.length;
      gate.resolve(); await pending;
      assert.equal(requests.length, 1); assert.equal(requests[0].method, "babble.media.blob.put.v1");
      assert.equal(requests[0].token, "session-a");
      assert.equal(h.composeText.value, "Next session caption"); assert.deepEqual(Array.from(h.drafts.current.media), [nextFile]);
      assert.equal(navigations.length, accountNavigations); assert.equal(refreshed.length, 0);
      if (sessionChange !== "same-identity-relogin") {
        h.changeAccount("alice");
        assert.deepEqual(Array.from(h.drafts.current.media), [firstFile]); assert.equal(h.composeText.value, "Alice caption");
      }
    });
  }

  test(`${mode} delayed File reads cannot upload after their session changes`, async () => {
    const { h, requests, navigations, refreshed } = harness(); const bytes = deferred();
    h.startSocialTextComposer(mode, "target");
    const file = { name: "voice.ogg", type: "audio/ogg", size: 1, arrayBuffer: () => bytes.promise };
    h.drafts.edit("caption", [file]); h.renderComposerDraft();
    const pending = h.publishFromComposer();
    h.changeAccount("bob"); h.type("Bob caption");
    bytes.resolve(new ArrayBuffer(1)); await pending;
    assert.equal(requests.length, 0); assert.equal(navigations.length + refreshed.length, 0);
    assert.equal(h.composeText.value, "Bob caption");
    h.changeAccount("alice"); assert.deepEqual(Array.from(h.drafts.current.media), [file]);
  });

  test(`${mode} upload keeps its original target and attachment when another social draft opens`, async () => {
    const { h, requests, navigations, refreshed } = harness(); const gate = deferred(), started = deferred();
    const firstFile = new File(["first"], "first.png", { type: "image/png" });
    const nextFile = new File(["next"], "next.ogg", { type: "audio/ogg" });
    h.startSocialTextComposer(mode, "a"); h.drafts.edit("caption A", [firstFile]); h.renderComposerDraft();
    h.respond = async request => { if (request?.method === "babble.media.blob.put.v1") { started.resolve(); await gate.promise; } };
    const pending = h.publishFromComposer(); await started.promise;
    const otherMode = mode === "reply" ? "share" : "reply";
    h.startSocialTextComposer(otherMode, "b"); h.drafts.edit("caption B", [nextFile]); h.renderComposerDraft();
    gate.resolve(); await pending;
    assert.equal(requests.length, 3);
    assert.equal(requests.at(-1).method, `babble.social.${mode}`);
    assert.equal(requests.at(-1).payload.target_object_id, "a");
    assert.equal(requests.at(-1).payload.text, "caption A");
    assert.deepEqual(requests.at(-1).payload.media.resources, [blobReceipt(requests[0].payload)]);
    assert.deepEqual(Array.from(h.drafts.current.media), [nextFile]); assert.equal(h.composeText.value, "caption B");
    assert.equal(h.composerPanel.hidden, false); assert.equal(navigations.length + refreshed.length, 0);
    h.startSocialTextComposer(mode, "a");
    assert.deepEqual(Array.from(h.drafts.current.media), []); assert.equal(h.composeText.value, "");
  });
}
