import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createHash, webcrypto } from "node:crypto";
import { fileURLToPath } from "node:url";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import * as sdk from "@babble-protocol/sdk";
import { mediaResource } from "./media-modules.mjs";

const globals = { URL, File, Response, TextEncoder, TextDecoder, Uint8Array, ArrayBuffer, ReadableStream,
  crypto: webcrypto, fetch, AbortController, AbortSignal, structuredClone, console, Date, Error };
function module(name, dependencies = {}) {
  const exports = {};
  const code = ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText;
  vm.runInNewContext(code, { ...globals, exports, require: (id) => {
    assert.ok(id in dependencies, `Unexpected dependency: ${id}`);
    return dependencies[id];
  } });
  return exports;
}
const { inspectBundleFiles, bundleEntries, publishBundle, parseBundleCapabilities } = module("bundle-publication", { "@babble-protocol/sdk": sdk });
const { BabbleFrontendClient } = module("protocol", {
  "@babble-protocol/sdk": sdk, "./profile-response": module("profile-response"),
  "./media-resource": mediaResource,
  "./invocations": module("invocations", { "@babble-protocol/sdk": sdk }),
  "./browser-invocations": module("browser-invocations", { "@babble-protocol/sdk": sdk }),
});
const { draftTransport } = module("drafts");
const AUTHOR = `id_${"a".repeat(64)}`;
const MIB = 1024 * 1024;
const plain = (value) => JSON.parse(JSON.stringify(value));
function file(path, content = `contents of ${path}`, type = "application/x-incorrect-browser-mime") {
  const result = new File([content], path.split("/").at(-1), { type });
  if (path.includes("/")) Object.defineProperty(result, "webkitRelativePath", { value: path });
  return result;
}
const html = () => file("index.html", "<!doctype html><script src='./app.js'></script>");
function declaredSize(path, size) {
  const result = file(path);
  Object.defineProperty(result, "size", { value: size });
  return result;
}
function streamFile(path, size, chunks, error) {
  const result = declaredSize(path, size);
  Object.defineProperty(result, "stream", { value: () => new ReadableStream({ start(controller) {
    if (error) { controller.error(error); return; }
    for (const chunk of chunks) controller.enqueue(chunk);
    controller.close();
  } }) });
  return result;
}

function harness(options = {}) {
  const requests = [], uploaded = [];
  const published = { id: `obj_${"b".repeat(64)}` };
  let authorized = true;
  const transport = async (_url, init) => {
    assert.equal(new Headers(init.headers).get("authorization"), "Bearer session-token");
    const request = JSON.parse(init.body);
    requests.push(request);
    if (request.method === "babble.media.blob.put.v1") {
      const bytes = Buffer.from(request.payload.bytes_hex, "hex");
      uploaded.push(bytes);
      // Deterministic receipt fixture only. Production trusts the server's BLAKE3.
      const integrity = createHash("sha256").update(bytes).digest("hex");
      const receipt = { integrity, uri: `babble://blobs/${integrity}`,
        size_bytes: bytes.length, media_type: request.payload.media_type };
      await options.upload?.(receipt, uploaded.length);
      return Response.json({ result: { blob: receipt }, error: null });
    }
    assert.equal(request.method, "babble.object.publish.v1");
    await options.publish?.(request);
    return Response.json({ result: { object: published }, error: null });
  };
  const authenticated = (url, init) => transport(url, { ...init,
    headers: { ...init.headers, authorization: "Bearer session-token" } });
  const client = new BabbleFrontendClient("https://babble.test", draftTransport(authenticated, () => authorized, "stable-operation"));
  return { client, requests, uploaded, published, revoke() { authorized = false; } };
}

test("public signatures type-check against current generated protocol contracts without a build", () => {
  const root = fileURLToPath(new URL("../src/app/bundle-publication.ts", import.meta.url));
  const contractPath = fileURLToPath(new URL("./bundle-publication.contract.ts", import.meta.url));
  const contract = `
    import type { RpcInput, RpcOutput } from '@babble-protocol/sdk';
    import { BabbleFrontendClient } from '../src/app/protocol';
    import { inspectBundleFiles, bundleEntries, publishBundle, parseBundleCapabilities, type CapabilityRequest, type BundleAttachment } from '../src/app/bundle-publication';
    declare const client: BabbleFrontendClient;
    declare const files: readonly File[];
    declare const draft: RpcInput<'babble.object.publish.v1'>['draft'];
    const attachment: BundleAttachment = inspectBundleFiles(files);
    const capabilities: readonly CapabilityRequest[] = parseBundleCapabilities('[]');
    const declared: typeof draft.capabilities = capabilities;
    const edited: BundleAttachment = { ...attachment, capabilitiesText: '[' };
    const entries: readonly string[] = bundleEntries(attachment);
    const result: Promise<RpcOutput<'babble.object.publish.v1'>['object']> = publishBundle(client, 'author', 'text', attachment);
    const published: typeof result = client.publishDraft('author', draft);
  `;
  const options = { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler, strict: true, noUncheckedIndexedAccess: true,
    exactOptionalPropertyTypes: true, noEmit: true, skipLibCheck: true, types: [],
    paths: { "@babble-protocol/sdk": [fileURLToPath(new URL("../../sdk/src/index.ts", import.meta.url))] } };
  const host = ts.createCompilerHost(options);
  const getSourceFile = host.getSourceFile.bind(host);
  host.getSourceFile = (path, ...args) => path === contractPath
    ? ts.createSourceFile(path, contract, options.target, true) : getSourceFile(path, ...args);
  const program = ts.createProgram([root, contractPath], options, host);
  const diagnostics = ts.getPreEmitDiagnostics(program);
  assert.equal(diagnostics.length, 0, ts.formatDiagnosticsWithColorAndContext(diagnostics, {
    getCanonicalFileName: (path) => path, getCurrentDirectory: () => process.cwd(), getNewLine: () => "\n",
  }));
});

test("capability parser accepts protocol and custom namespaces with immutable nested scope data", () => {
  const input = [
    { id: "babble.clipboard.write", version: 1, scope: {} },
    { id: "babble.ui.fullscreen", version: 1, scope: {} },
    { id: "vendor-2.custom.capability", version: 0xffffffff,
      scope: { origins: ["https://example.test"], nested: { enabled: true, empty: null, ratio: 0.125 }, text: "cafe\u0301 \ud83c\udf04" } },
  ];
  const parsed = parseBundleCapabilities(JSON.stringify(input));
  assert.deepEqual(plain(parsed), input);
  assert.ok(Object.isFrozen(parsed));
  assert.ok(Object.isFrozen(parsed[2]) && Object.isFrozen(parsed[2].scope));
  assert.ok(Object.isFrozen(parsed[2].scope.origins) && Object.isFrozen(parsed[2].scope.nested));
  assert.throws(() => { parsed[2].scope.nested.enabled = false; }, TypeError);
  assert.deepEqual(plain(parseBundleCapabilities("[]")), []);
});

test("capability declarations reject invalid envelopes, IDs, versions, scopes and ignored extra fields", () => {
  const valid = { id: "vendor.test", version: 1, scope: {} };
  for (const invalid of [null, {}, "[]", true, [null], [[]], [{ ...valid, extra: true }],
    [{ id: valid.id, version: 1 }], [{ version: 1, scope: {} }], [{ id: valid.id, scope: {} }]]) {
    assert.throws(() => parseBundleCapabilities(JSON.stringify(invalid)), /capabilit/i);
  }
  for (const id of ["", "babble", "babble..test", ".babble.test", "babble.test.", "Babble.test", "babble.test_name", "babble.a/b", "babble.t\u00e9st", "babble.test\n"]) {
    assert.throws(() => parseBundleCapabilities(JSON.stringify([{ ...valid, id }])), /namespaced id/);
  }
  for (const version of [0, -1, 0.5, 0x100000000, "1", null, false]) {
    assert.throws(() => parseBundleCapabilities(JSON.stringify([{ ...valid, version }])), /positive u32/);
  }
  for (const scope of [null, [], "text", 0, false]) {
    assert.throws(() => parseBundleCapabilities(JSON.stringify([{ ...valid, scope }])), /scope must be a JSON object/);
  }
  for (const raw of ["", "[", '[{"id":}]', "[NaN]", "[] trailing"]) {
    assert.throws(() => parseBundleCapabilities(raw), /valid JSON/);
  }
});

test("capability count and UTF-8 byte limits are exact", () => {
  const declarations = Array.from({ length: 64 }, (_, index) => ({ id: `custom.cap${index}`, version: 1, scope: {} }));
  assert.equal(parseBundleCapabilities(JSON.stringify(declarations)).length, 64);
  assert.throws(() => parseBundleCapabilities(JSON.stringify([...declarations, declarations[0]])), /at most 64/);
  const frame = JSON.stringify([{ id: "custom.test", version: 1, scope: { text: "" } }]);
  const available = 65536 - Buffer.byteLength(frame);
  const raw = (text) => JSON.stringify([{ id: "custom.test", version: 1, scope: { text } }]);
  assert.equal(parseBundleCapabilities(raw("x".repeat(available)))[0].scope.text.length, available);
  assert.throws(() => parseBundleCapabilities(raw("x".repeat(available + 1))), /64 KiB/);
  const multibyte = "\u00e9".repeat(Math.floor(available / 2)) + (available % 2 ? "x" : "");
  assert.equal(Buffer.byteLength(raw(multibyte)), 65536);
  assert.equal(parseBundleCapabilities(raw(multibyte))[0].scope.text, multibyte);
  assert.throws(() => parseBundleCapabilities(raw(multibyte + "\u00e9")), /64 KiB/);
});

test("capability depth includes the declaration array and scope, with a maximum of 16 containers", () => {
  const nested = (count) => `[{'id':'custom.test','version':1,'scope':{'values':${'['.repeat(count)}0${']'.repeat(count)}}}]`.replaceAll("'", '"');
  assert.equal(parseBundleCapabilities(nested(13)).length, 1);
  assert.throws(() => parseBundleCapabilities(nested(14)), /depth of 16/);
});

test("duplicate declarations are detected independent of object key ordering, not by capability ID alone", () => {
  const one = { id: "vendor.test", version: 1, scope: { a: [1, 2], b: { c: true, d: null } } };
  const reordered = { scope: { b: { d: null, c: true }, a: [1, 2] }, version: 1, id: "vendor.test" };
  assert.throws(() => parseBundleCapabilities(JSON.stringify([one, reordered])), /duplicates an earlier/);
  assert.equal(parseBundleCapabilities(JSON.stringify([one, { ...one, version: 2 }, { ...one, scope: { a: [2, 1] } }])).length, 3);
});

test("ambiguous keys, invalid Unicode and lossy numeric values are rejected before JSON can silently change them", () => {
  const raw = (scope) => `[{"id":"vendor.test","version":1,"scope":${scope}}]`;
  for (const scope of ['{"a":1,"a":2}', '{"a":1,"\\u0061":2}', '{"nested":{"x":1,"x":1}}']) {
    assert.throws(() => parseBundleCapabilities(raw(scope)), /duplicate object key/);
  }
  for (const number of ["1e400", "-1e400", "1e-400", "9007199254740993", "9007199254740992", "0.10000000000000001", "-0", "-0.0"]) {
    assert.throws(() => parseBundleCapabilities(raw(`{"n":${number}}`)), /without loss/, number);
  }
  for (const number of ["0", "1.0", "1e0", "0.1", "1.2300e-2", "9007199254740991", "5e-324"]) {
    assert.equal(parseBundleCapabilities(raw(`{"n":${number}}`))[0].scope.n, Number(number));
  }
  for (const scope of ['{"s":"\\ud800"}', '{"\\udfff":1}', '{"s":"\ud800"}']) {
    assert.throws(() => parseBundleCapabilities(raw(scope)), /Unicode surrogate/);
  }
  const special = parseBundleCapabilities(raw('{"__proto__":{"x":1},"constructor":true,"a":[{"x":1},{"x":2}],"text":"[ { 9e999 \\""}'));
  assert.equal(Object.hasOwn(special[0].scope, "__proto__"), true);
  assert.deepEqual(plain(special[0].scope.a), [{ x: 1 }, { x: 2 }]);
});

test("invalid capability edits abort before file reading, upload or publication", async () => {
  const selected = html();
  Object.defineProperty(selected, "stream", { value: () => { throw Error("must not read file"); } });
  const h = harness();
  for (const capabilitiesText of ["[", '{"id":"vendor.test"}', '[{"id":"bad","version":1,"scope":{}}]']) {
    await assert.rejects(publishBundle(h.client, AUTHOR, "caption", {
      files: [selected], entryPath: "index.html", capabilitiesText,
    }), /capabilit/i);
    assert.equal(h.requests.length, 0);
  }
});

test("request-size preflight accounts for declarations before uploading any bytes", async () => {
  const h = harness();
  h.client.binding.origin = "x".repeat(16 * MIB + 64 * 1024 - 4096);
  let uploads = 0;
  h.client.putMediaBlob = async (media_type, bytes) => {
    uploads++;
    const integrity = "a".repeat(64);
    return { integrity, uri: `babble://blobs/${integrity}`, media_type, size_bytes: bytes.length };
  };
  h.client.publishDraft = async () => h.published;
  const attachment = inspectBundleFiles([html()]);
  await publishBundle(h.client, AUTHOR, "caption", attachment);
  assert.equal(uploads, 1);
  await assert.rejects(publishBundle(h.client, AUTHOR, "caption", { ...attachment,
    capabilitiesText: JSON.stringify([{ id: "vendor.test", version: 1, scope: { value: "x".repeat(8192) } }]),
  }), /transport limit/);
  assert.equal(uploads, 1);
});

test("capability declarations are captured before awaiting file reads and included in the signed draft", async () => {
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  const selected = html();
  const stream = selected.stream.bind(selected);
  let beganRead = false;
  Object.defineProperty(selected, "stream", { value: () => {
    beganRead = true;
    return { getReader() {
      const reader = stream().getReader();
      return { read: async () => { await gate; return reader.read(); }, cancel: () => reader.cancel(), releaseLock: () => reader.releaseLock() };
    } };
  } });
  const declarations = [{ id: "babble.clipboard.write", version: 1, scope: {} }];
  const attachment = { files: [selected], entryPath: "index.html", capabilitiesText: JSON.stringify(declarations) };
  const h = harness();
  const pending = publishBundle(h.client, AUTHOR, "caption", attachment);
  assert.equal(beganRead, true);
  attachment.capabilitiesText = "[";
  attachment.files.length = 0;
  release();
  await pending;
  assert.deepEqual(h.requests.at(-1).payload.draft.capabilities, declarations);
});

test("unchanged declared capability retries retain publication keys and edited declarations change only the final key", async () => {
  const h = harness();
  const attachment = { ...inspectBundleFiles([html()]),
    capabilitiesText: JSON.stringify([{ id: "vendor.test", version: 1, scope: { value: "first" } }]) };
  await publishBundle(h.client, AUTHOR, "caption", attachment);
  await publishBundle(h.client, AUTHOR, "caption", attachment);
  attachment.capabilitiesText = JSON.stringify([{ id: "vendor.test", version: 1, scope: { value: "second" } }]);
  await publishBundle(h.client, AUTHOR, "caption", attachment);
  const uploads = h.requests.filter((request) => request.method === "babble.media.blob.put.v1");
  const publications = h.requests.filter((request) => request.method === "babble.object.publish.v1");
  assert.equal(publications[0].idempotency_key, publications[1].idempotency_key);
  assert.notEqual(publications[1].idempotency_key, publications[2].idempotency_key);
  assert.equal(uploads[0].idempotency_key, uploads[2].idempotency_key);
  assert.equal(h.requests.some((request) => request.method.includes("grant")), false);
});

test("directory normalization strips exactly one common root, sorts canonical paths and retains Files", () => {
  const selected = [file("Selected folder/assets/app.js"), file("Selected folder/index.html"), file("Selected folder/pages/about.html")];
  const attachment = inspectBundleFiles(selected);
  assert.equal(attachment.entryPath, "index.html");
  assert.deepEqual([...bundleEntries(attachment)], ["index.html", "pages/about.html"]);
  assert.equal(attachment.files[0], selected[0]);
  assert.equal(attachment.files[1], selected[1]);
  assert.ok(Object.isFrozen(attachment.files));
  assert.ok(Object.isFrozen(attachment));
  const nested = inspectBundleFiles([file("root/site/a.html"), file("root/site/index.html")]);
  assert.equal(nested.entryPath, "site/index.html");
  const ambiguousIndexes = inspectBundleFiles([file("root/b/index.html"), file("root/a/a.html"), file("root/c/index.html")]);
  assert.equal(ambiguousIndexes.entryPath, "a/a.html");
  const flat = inspectBundleFiles([file("z.html"), html(), file("app.js")]);
  assert.equal(flat.entryPath, "index.html");
  assert.deepEqual(flat.files.map((value) => value.name).join(","), "app.js,index.html,z.html");
});

test("invalid, ambiguous, duplicate, traversal and unsupported paths are rejected", () => {
  for (const path of ["root/../index.html", "root/./index.html", "root//index.html", "/index.html",
    "../index.html", "root/a\\b.html", "root/a%2fb.html", "root/a b.html", "root/caf\u00e9.html",
    "root/a?b.html", "root/a#b.html", `root/${"a".repeat(251)}.html`,
    `root/${Array(5).fill("a".repeat(220)).join("/")}/index.html`]) {
    assert.throws(() => inspectBundleFiles([file(path)]), /path|ASCII|directory/, path);
  }
  assert.throws(() => inspectBundleFiles([file("one/a.html"), file("two/b.html")]), /common directory/);
  assert.throws(() => inspectBundleFiles([file("root/a.html"), html()]), /mix/);
  assert.throws(() => inspectBundleFiles([html(), html()]), /duplicate/);
  assert.throws(() => inspectBundleFiles([file("root/a.html"), file("root/a.html/index.html")]), /file and directory/);
  assert.throws(() => inspectBundleFiles([file("app.js")]), /HTML/);
  for (const path of ["module.wasm", "main.ts", "component.jsx", "source.tsx", "unknown.xyz", "LICENSE", "constructor"]) {
    assert.throws(() => inspectBundleFiles([html(), file(path)]), /unsupported MIME|WASM/);
  }
  const mismatch = file("root/index.html");
  Object.defineProperty(mismatch, "name", { value: "other.html" });
  assert.throws(() => inspectBundleFiles([mismatch]), /ambiguous/);
});

test("exact count, file, aggregate, path and canonical manifest bounds are enforced", () => {
  assert.throws(() => inspectBundleFiles([]), /between 1 and 256/);
  assert.equal(inspectBundleFiles([html(), ...Array.from({ length: 255 }, (_, i) => file(`${i}.js`))]).files.length, 256);
  assert.throws(() => inspectBundleFiles([html(), ...Array.from({ length: 256 }, (_, i) => file(`${i}.js`))]), /256/);
  for (const size of [-1, NaN, Infinity, 0.5, 8 * MIB + 1]) {
    assert.throws(() => inspectBundleFiles([declaredSize("index.html", size)]), /8 MiB|invalid size/);
  }
  const maximum = [declaredSize("index.html", 8 * MIB), ...[1, 2, 3].map((i) => declaredSize(`${i}.bin`, 8 * MIB))];
  assert.equal(inspectBundleFiles(maximum).files.length, 4);
  assert.throws(() => inspectBundleFiles([...maximum, file("extra.bin", "a")]), /32 MiB/);
  const longest = `${"a".repeat(255)}/${"b".repeat(255)}/${"c".repeat(255)}/${"d".repeat(245)}/index.html`;
  assert.equal(longest.length, 1024);
  assert.equal(inspectBundleFiles([file(`root/${longest}`)]).entryPath, longest);
  const manyLongPaths = Array.from({ length: 256 }, (_, i) => file(`root/${i}/${"a".repeat(250)}/${"b".repeat(250)}/${"c".repeat(250)}/index.html`));
  assert.throws(() => inspectBundleFiles(manyLongPaths), /canonical manifest.*256 KiB/);
});

test("all metadata and actual file bytes are validated before the first upload", async () => {
  for (const attachment of [
    { files: [html(), file("unsupported.ts")], entryPath: "index.html" },
    { files: [html(), file("app.js")], entryPath: "app.js" },
    { files: [html()], entryPath: "missing.html" },
    { files: [html()], entryPath: "../index.html" },
    { files: [html(), streamFile("z.js", 1, [new Uint8Array(2)])], entryPath: "index.html" },
    { files: [html(), streamFile("z.js", 2, [new Uint8Array(1)])], entryPath: "index.html" },
    { files: [html(), streamFile("z.js", 1, [], Error("read failed"))], entryPath: "index.html" },
    { files: [file("index.html", "same"), file("app.js", "same")], entryPath: "index.html" },
  ]) {
    const h = harness();
    await assert.rejects(publishBundle(h.client, AUTHOR, "caption", attachment));
    assert.equal(h.requests.length, 0);
  }
  for (const [author, text] of [["", "caption"], [AUTHOR, " \n "], [AUTHOR, "\ud800"], [AUTHOR, "x".repeat(17 * MIB)], [AUTHOR, "\u0000".repeat(3 * MIB)]]) {
    const h = harness();
    await assert.rejects(publishBundle(h.client, author, text, inspectBundleFiles([html()])));
    assert.equal(h.requests.length, 0);
  }
  const h = harness();
  h.client.binding.origin = "x".repeat(17 * MIB);
  await assert.rejects(publishBundle(h.client, AUTHOR, "caption", inspectBundleFiles([html()])), /transport limit/);
  assert.equal(h.requests.length, 0);
});

test("capture enforces streamed bytes, cancels oversized reads and accepts an exact 8 MiB file", async () => {
  let cancelled = false;
  const oversized = declaredSize("index.html", 1);
  Object.defineProperty(oversized, "stream", { value: () => new ReadableStream({
    start(controller) { controller.enqueue(new Uint8Array(2)); }, cancel() { cancelled = true; },
  }) });
  const h = harness();
  await assert.rejects(publishBundle(h.client, AUTHOR, "caption", inspectBundleFiles([oversized])), /actual bytes/);
  assert.equal(cancelled, true);
  assert.equal(h.requests.length, 0);
  const exact = file("index.html", new Uint8Array(8 * MIB));
  let uploaded = 0;
  h.client.putMediaBlob = async (media_type, bytes) => {
    uploaded = bytes.length;
    return { media_type, size_bytes: bytes.length, integrity: "c".repeat(64), uri: `babble://blobs/${"c".repeat(64)}` };
  };
  await publishBundle(h.client, AUTHOR, "caption", inspectBundleFiles([exact]));
  assert.equal(uploaded, 8 * MIB);
  const empty = file("empty.html", "");
  await publishBundle(harness().client, AUTHOR, "caption", inspectBundleFiles([empty]));
});

test("malformed hash, URI, MIME and size receipts prevent signed publication", async () => {
  for (const change of [
    { integrity: "A".repeat(64) }, { integrity: "g".repeat(64) }, { integrity: "a".repeat(63) },
    { uri: `https://external.test/${"a".repeat(64)}` }, { uri: `babble://blobs/${"f".repeat(64)}` },
    { size_bytes: -1 }, { size_bytes: 999 }, { size_bytes: "1" }, { media_type: "text/plain" },
  ]) {
    const h = harness({ upload(receipt) { Object.assign(receipt, change); } });
    await assert.rejects(publishBundle(h.client, AUTHOR, "caption", inspectBundleFiles([html(), file("app.js")])) , /invalid blob receipt/);
    assert.equal(h.requests.length, 1);
  }
  const h = harness();
  h.client.putMediaBlob = async () => null;
  await assert.rejects(publishBundle(h.client, AUTHOR, "caption", inspectBundleFiles([html()])), /invalid blob receipt/);
  assert.equal(h.requests.length, 0);
});

test("hash alias inconsistencies in receipts prevent publication", async () => {
  for (const sameBytes of [true, false]) {
    const selected = inspectBundleFiles([file("a.html", "first"), file("b.html", sameBytes ? "first" : "other")]);
    const h = harness({ upload(receipt, index) {
      receipt.integrity = (sameBytes && index === 2 ? "b" : "a").repeat(64);
      receipt.uri = `babble://blobs/${receipt.integrity}`;
    } });
    await assert.rejects(publishBundle(h.client, AUTHOR, "caption", selected), /inconsistent.*hashes/);
    assert.equal(h.requests.length, 2);
  }
});

test("uploads are sequential, all bytes are captured first, and failures stop publication", async () => {
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  let started;
  const firstUpload = new Promise((resolve) => { started = resolve; });
  let read = 0;
  const selected = [html(), file("app.js")];
  for (const file of selected) {
    const stream = file.stream.bind(file);
    Object.defineProperty(file, "stream", { value: () => { read++; return stream(); } });
  }
  const h = harness({ async upload(_receipt, index) {
    assert.equal(read, 2);
    if (index === 1) { started(); await gate; }
    else throw Error("second upload failed");
  } });
  const pending = publishBundle(h.client, AUTHOR, "caption", inspectBundleFiles(selected));
  await firstUpload;
  assert.equal(h.requests.length, 1);
  release();
  await assert.rejects(pending, /second upload failed/);
  assert.equal(h.requests.length, 2);
});

test("success preserves raw bytes, uses filename MIME and publishes the selected Feed Web entry via authenticated RPC", async () => {
  const h = harness();
  const inputs = [file("root/z.html", "second document"), file("root/index.html", "<html>original</html>"),
    file("root/app.js", "export const unchanged = 1;"), file("root/picture.svg", "<svg/>")];
  const attachment = { ...inspectBundleFiles(inputs), entryPath: "z.html" };
  const result = await publishBundle(h.client, AUTHOR, "  caption  ", attachment);
  assert.deepEqual(plain(result), h.published);
  assert.equal(h.requests.length, inputs.length + 1);
  assert.deepEqual(h.uploaded.map((bytes) => bytes.toString()),
    ["export const unchanged = 1;", "<html>original</html>", "<svg/>", "second document"]);
  assert.deepEqual(h.requests.slice(0, -1).map((request) => request.payload.media_type),
    ["text/javascript", "text/html", "image/svg+xml", "text/html"]);
  const request = h.requests.at(-1);
  assert.match(request.idempotency_key, /^web-draft-[0-9a-f]{64}$/);
  assert.equal(request.protocol, "babble.rpc.v1");
  assert.equal(request.binding.runtime_id, "babble-web-runtime");
  assert.equal(request.payload.author_id, AUTHOR);
  const { draft } = request.payload;
  assert.equal(draft.kind, "babble.text");
  assert.equal(draft.schema, "babble.schema.text.v1");
  assert.deepEqual(draft.payload, { text: "caption", metadata: {} });
  assert.deepEqual(draft.provenance, { parent: null, forked_from: null, remixed_from: [] });
  assert.deepEqual(draft.resources, []);
  assert.deepEqual(draft.capabilities, []);
  assert.equal(draft.surfaces.length, 1);
  const surface = draft.surfaces[0];
  assert.equal(surface.role, "Feed");
  assert.equal(surface.target, "Web");
  assert.equal(surface.bundle.version, 1);
  assert.equal(surface.bundle.entry_path, "z.html");
  assert.deepEqual(surface.bundle.files.map((file) => file.path), ["app.js", "index.html", "picture.svg", "z.html"]);
  assert.equal(surface.entry, surface.bundle.files.at(-1).source_uri);
  assert.equal(surface.integrity, surface.bundle.files.at(-1).integrity);
});

test("retry retains exact Files, upload bytes, publication payload and draftTransport idempotency", async () => {
  let failed = false;
  const h = harness({ publish() { if (!failed) { failed = true; throw Error("response lost after signing"); } } });
  const selected = [html(), file("app.js")];
  const attachment = inspectBundleFiles(selected);
  await assert.rejects(publishBundle(h.client, AUTHOR, "caption", attachment), /response lost/);
  await publishBundle(h.client, AUTHOR, "caption", attachment);
  assert.equal(attachment.files[0], selected[1]);
  assert.equal(attachment.files[1], selected[0]);
  assert.deepEqual(h.uploaded.slice(0, 2), h.uploaded.slice(2));
  const publications = h.requests.filter((request) => request.method === "babble.object.publish.v1");
  assert.deepEqual(publications[0].payload, publications[1].payload);
  assert.equal(publications[0].idempotency_key, publications[1].idempotency_key);
  await publishBundle(h.client, AUTHOR, "edited", attachment);
  assert.notEqual(h.requests.at(-1).idempotency_key, publications[0].idempotency_key);
});

test("real upload transport encodes exact 8 MiB bytes across chunk boundaries", async () => {
  const bytes = new Uint8Array(8 * MIB);
  for (let index = 0; index < bytes.length; index++) bytes[index] = index & 255;
  const h = harness();
  await h.client.putMediaBlob("application/octet-stream", bytes);
  assert.equal(h.requests.length, 1);
  assert.equal(h.requests[0].payload.bytes_hex.length, bytes.length * 2);
  assert.deepEqual(h.uploaded[0], Buffer.from(bytes));
  await h.client.putMediaBlob("application/octet-stream", new Uint8Array());
  assert.equal(h.requests[1].payload.bytes_hex, "");
});

test("parent authorization checks stop subsequent uploads and signed publication after account change", async () => {
  for (const count of [1, 2]) {
    const h = harness({ upload() { h.revoke(); } });
    const attachment = inspectBundleFiles([html(), ...count === 2 ? [file("app.js")] : []]);
    await assert.rejects(publishBundle(h.client, AUTHOR, "caption", attachment), /Account changed/);
    assert.equal(h.requests.length, 1);
  }
});
