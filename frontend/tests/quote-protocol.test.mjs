import assert from "node:assert/strict";
import { webcrypto } from "node:crypto";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import * as sdk from "@babble-protocol/sdk";
import { mediaResource } from "./media-modules.mjs";

function load(path, dependencies = {}) {
  const context = { exports: {}, URL, AbortController, AbortSignal, TextEncoder, TextDecoder, structuredClone, console, Error, crypto: webcrypto,
    location: { origin: "https://frontend.babble.test" },
    require(name) {
      assert.ok(name in dependencies, `Unexpected runtime dependency: ${name}`);
      return dependencies[name];
    } };
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL(path, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, context);
  return context.exports;
}
const transport = load("../../sdk/src/transport.ts");
const profiles = load("../src/app/profile-response.ts");
const { BabbleFrontendClient } = load("../src/app/protocol.ts", {
  "@babble-protocol/sdk": {
    ...transport,
    BrowserSurfaceHost: class { constructor() { assert.fail("Resolving quoted cards cannot construct a Surface host"); } },
  },
  "./profile-response": profiles,
  "./media-resource": mediaResource,
  "./invocations": load("../src/app/invocations.ts", { "@babble-protocol/sdk": sdk }),
  "./browser-invocations": load("../src/app/browser-invocations.ts", { "@babble-protocol/sdk": sdk }),
});
const sourceId = `obj_${"a".repeat(64)}`;
const targetId = `obj_${"b".repeat(64)}`;
const absentId = `obj_${"c".repeat(64)}`;
const authorId = `id_${"d".repeat(64)}`;
const hash = "e".repeat(64);
const signature = () => ({ algorithm: "Ed25519", bytes: "f".repeat(128) });
const object = (id = targetId) => ({ id, author: authorId, created_at: "2026-09-30T12:00:00Z",
  kind: "babble.text", schema: "babble.schema.text.v1", protocol: { name: "babble", version: 1 },
  payload: { text: "An original post\nIts complete content." }, signature: signature(),
  provenance: { parent: null, forked_from: null, remixed_from: [] }, relations: [],
  resources: [], surfaces: [], capabilities: [] });
const quote = (record = object()) => ({
  edge: { id: `edge_${"1".repeat(64)}`, source: sourceId, target: record?.id ?? absentId,
    relation: "quotes", author: authorId, signature: signature(), origin: "HumanAssertion",
    created_at: "2026-09-30T12:01:00Z", metadata: {} },
  object: record,
});
const page = (quotes = [quote()], next = null) => ({ object_id: sourceId, quotes, next_cursor: next });
const envelope = (result) => Response.json({ protocol: "babble.rpc.v1", result, error: null });
const deferred = () => {
  let resolve, reject;
  const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
};
const settle = () => new Promise((resolve) => setImmediate(resolve));
const abortError = (error) => error?.name === "AbortError";

test("object conversion forwards the complete ordered collection and preserves its primary", async () => {
  const client = new BabbleFrontendClient("https://api.babble.test", () => assert.fail("Conversion must not fetch media"));
  const image = { integrity: "a".repeat(64), media_type: "image/png", uri: "https://media.test/image.png" };
  const audio = { integrity: "b".repeat(64), media_type: "audio/mpeg", uri: "https://media.test/audio.mp3" };
  const video = { integrity: "c".repeat(64), media_type: "video/mp4", uri: "https://media.test/video.mp4" };
  const surface = { integrity: "d".repeat(64), media_type: "image/png", uri: "https://media.test/surface.png" };
  const outer = { ...object(), kind: "babble.media", resources: [surface, video, image, audio] };
  const card = await client.describeObject({ ...outer,
    payload: { text: "Mixed album", resources: [image, audio, video], primary_resource: video } });
  assert.equal(card.media, video.uri);
  assert.equal(card.mediaKind, "video");
  assert.deepEqual(Array.from(card.mediaItems, item => item.integrity), [image.integrity, audio.integrity, video.integrity]);
  const mismatch = { ...audio, uri: "https://media.test/unsigned.mp3" };
  const filtered = await client.describeObject({ ...outer,
    payload: { resources: [image, mismatch, video], primary_resource: video } });
  assert.deepEqual(Array.from(filtered.mediaItems, item => item.integrity), [image.integrity, video.integrity]);
  assert.equal(filtered.media, video.uri);
  for (const primary_resource of [surface, mismatch]) {
    const rejected = await client.describeObject({ ...outer,
      payload: { resources: [image, audio, video], primary_resource } });
    assert.equal(rejected.media, null);
    assert.equal(rejected.mediaItems.length, 0);
  }
  const generic = await client.describeObject({ ...outer, kind: "custom.game", payload: { resources: { gold: 20 } } });
  assert.deepEqual(Array.from(generic.mediaItems, item => item.integrity), [surface.integrity, video.integrity, image.integrity, audio.integrity]);
  const empty = await client.describeObject(object());
  assert.equal(empty.media, null);
  assert.equal(empty.mediaItems.length, 0);
});
function harness(result = page(), respond = null) {
  const requests = [];
  const client = new BabbleFrontendClient("https://api.babble.test", async (url, init) => {
    const request = { url, init, body: JSON.parse(init.body) };
    requests.push(request);
    assert.equal(url.href, "https://api.babble.test/rpc", "No eager media or Surface requests");
    assert.equal(request.body.method, "babble.social.quotes.list.v1", "Only the public quote-list RPC is allowed");
    return respond ? respond(request) : envelope(result);
  });
  return { client, requests, controller: new AbortController() };
}

test("quotes maps both page cursors to a bounded public RPC request and forwards the caller's exact signal", async () => {
  for (const cursor of [null, "opaque|+/cursor=="]) {
    const h = harness(page([], "server-next"));
    const result = await h.client.quotes(sourceId, cursor, h.controller.signal);
    assert.equal(h.requests.length, 1);
    const { body, init } = h.requests[0];
    assert.equal(init.method, "POST");
    assert.equal(init.headers["content-type"], "application/json");
    assert.equal(init.signal, h.controller.signal);
    assert.equal(body.protocol, "babble.rpc.v1");
    assert.deepEqual(body.payload, { object_id: sourceId, cursor, limit: 10 });
    assert.equal(body.idempotency_key, null);
    assert.equal(body.binding.identity_id, null);
    assert.equal(body.binding.object_id, null);
    assert.equal(body.binding.surface_session_id, null);
    assert.deepEqual(body.binding.capability_grants, []);
    assert.equal(body.binding.origin, "https://frontend.babble.test");
    assert.equal(result.objectId, sourceId);
    assert.equal(result.nextCursor, "server-next");
    assert.equal(result.items.length, 0);
  }
});

test("available originals become unranked quote cards; missing originals remain null in their original order", async () => {
  const original = object();
  const h = harness(page([quote(null), quote(original)]));
  const result = await h.client.quotes(sourceId, null, h.controller.signal);
  assert.equal(result.items.length, 2);
  assert.equal(result.items[0].targetId, absentId);
  assert.equal(result.items[0].card, null);
  const card = result.items[1].card;
  assert.equal(result.items[1].targetId, targetId);
  assert.equal(card.id, targetId);
  assert.equal(card.content, original.payload.text);
  assert.equal(card.author, authorId);
  assert.equal(card.source, "quote");
  assert.equal(card.score, null);
  assert.equal(card.rankingProvider, null);
  assert.ok(Object.values(card.signals).every((value) => value === null));
  assert.equal(card.reasons.length, 0);
  assert.equal(card.temporal, undefined);
  assert.equal(result.nextCursor, null);
  assert.equal(h.requests.length, 1, "Missing targets cannot trigger extra lookups");
});

test("media and interactive originals resolve inert descriptors without fetching blobs, preparing capabilities or executing Surfaces", async () => {
  const original = { ...object(), kind: "babble.media", payload: { title: "Original video", description: "Caption" },
    resources: [{ uri: `babble://blobs/${hash}`, media_type: "video/webm", integrity: hash }],
    surfaces: [{ role: "Feed", target: "Web", entry: "https://app.babble.test/game.html", integrity: hash }],
    capabilities: [{ id: "babble.clipboard.write", version: 1, scope: {} }] };
  const h = harness(page([quote(original)]));
  const { items } = await h.client.quotes(sourceId, null, h.controller.signal);
  assert.equal(items[0].card.media, `https://api.babble.test/objects/${targetId}/media/${hash}`);
  assert.equal(items[0].card.mediaKind, "video");
  assert.equal(items[0].card.mediaType, "video/webm");
  assert.equal(items[0].card.surfaces[0].target, "Web");
  assert.equal(items[0].card.capabilities[0].id, "babble.clipboard.write");
  assert.equal(items[0].card.resourceCount, 1);
  assert.equal(h.requests.length, 1);
});

const invalidPages = [
  ["wrong page Object", () => ({ ...page(), object_id: absentId })],
  ["wrong edge source", () => { const entry = quote(); entry.edge.source = absentId; return page([entry]); }],
  ["non-quote relationship", () => { const entry = quote(); entry.edge.relation = "reply_to"; return page([entry]); }],
  ["missing edge author", () => { const entry = quote(); delete entry.edge.author; return page([entry]); }],
  ["null edge author", () => { const entry = quote(); entry.edge.author = null; return page([entry]); }],
  ["missing edge signature", () => { const entry = quote(); delete entry.edge.signature; return page([entry]); }],
  ["null edge signature", () => { const entry = quote(); entry.edge.signature = null; return page([entry]); }],
  ["Object that is not the signed target", () => { const entry = quote(); entry.object.id = absentId; return page([entry]); }],
  ["oversized quote page", () => page(Array.from({ length: 11 }, () => quote()))],
];
for (const [name, invalid] of invalidPages) {
  test(`quotes rejects ${name} before converting any card`, async () => {
    const h = harness(invalid());
    h.client.objectToCard = () => assert.fail("An invalid page must not be partially converted");
    await assert.rejects(h.client.quotes(sourceId, null, h.controller.signal), /invalid shared-post context/);
    assert.equal(h.requests.length, 1);
  });
}

test("an invalid later edge rejects the whole page before even its valid first Object is converted", async () => {
  const invalid = quote(null); invalid.edge.signature = null;
  const h = harness(page([quote(), invalid]));
  let conversions = 0;
  h.client.objectToCard = async () => { conversions += 1; return {}; };
  await assert.rejects(h.client.quotes(sourceId, null, h.controller.signal), /invalid shared-post context/);
  assert.equal(conversions, 0);
});

test("a full ten-original page preserves the server ordering and pagination cursor", async () => {
  const entries = Array.from({ length: 10 }, (_, index) => quote(object(`obj_${index.toString().repeat(64)}`)));
  const h = harness(page(entries, "next-ten"));
  const result = await h.client.quotes(sourceId, "previous-ten", h.controller.signal);
  assert.deepEqual(Array.from(result.items, (item) => item.targetId), entries.map((entry) => entry.edge.target));
  assert.equal(result.nextCursor, "next-ten");
});

test("aborting in-flight HTTP rejects the adapter with the caller's abort rather than returning partial context", async () => {
  const h = harness(null, ({ init }) => new Promise((resolve, reject) => {
    init.signal.addEventListener("abort", () => reject(init.signal.reason), { once: true });
  }));
  const request = h.client.quotes(sourceId, null, h.controller.signal);
  const rejected = assert.rejects(request, abortError);
  h.controller.abort(); await rejected;
  assert.equal(h.requests[0].init.signal.aborted, true);
});

test("an already cancelled signal reaches transport and cannot produce cards", async () => {
  const h = harness(null, ({ init }) => { init.signal.throwIfAborted(); return envelope(page()); });
  h.controller.abort();
  await assert.rejects(h.client.quotes(sourceId, null, h.controller.signal), abortError);
});

test("a late successful HTTP response is suppressed even if transport does not honor abort", async () => {
  const response = deferred();
  const h = harness(null, () => response.promise);
  const request = h.client.quotes(sourceId, null, h.controller.signal);
  const rejected = assert.rejects(request, abortError);
  h.controller.abort(); response.resolve(envelope(page()));
  await rejected;
});

test("cancellation during asynchronous card conversion suppresses the completed page", async () => {
  const h = harness();
  const conversion = deferred();
  const actualConversion = h.client.objectToCard.bind(h.client);
  let started = false;
  h.client.objectToCard = async (input) => { started = true; await conversion.promise; return actualConversion(input); };
  const request = h.client.quotes(sourceId, null, h.controller.signal);
  const rejected = assert.rejects(request, abortError);
  await settle(); assert.equal(started, true);
  h.controller.abort(); conversion.resolve();
  await rejected;
});

test("transport and RPC errors remain failures rather than looking like an empty quote page", async () => {
  for (const [respond, message] of [
    [() => new Response("Unavailable", { status: 503 }), /status 503/],
    [() => Response.json({ result: null, error: { message: "Refresh shared posts" } }), /Refresh shared posts/],
    [() => Response.json({ result: null, error: null }), /did not include a result/],
  ]) {
    const h = harness(null, respond);
    await assert.rejects(h.client.quotes(sourceId, null, h.controller.signal), message);
  }
});
