import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";
import * as sdk from "@babble-protocol/sdk";
import { mediaResource } from "./media-modules.mjs";

function load(path, dependencies = {}) {
  const context = { exports: {}, URL, AbortController, AbortSignal, TextEncoder, TextDecoder, structuredClone, console, Error, crypto: globalThis.crypto,
    require(name) {
      assert.ok(name in dependencies, `Unexpected runtime dependency: ${name}`);
      return dependencies[name];
    } };
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL(path, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, context);
  return context.exports;
}
const { BabbleFrontendClient } = load("../src/app/protocol.ts", {
  "@babble-protocol/sdk": load("../../sdk/src/transport.ts"),
  "./profile-response": load("../src/app/profile-response.ts"),
  "./media-resource": mediaResource,
  "./invocations": load("../src/app/invocations.ts", { "@babble-protocol/sdk": sdk }),
  "./browser-invocations": load("../src/app/browser-invocations.ts", { "@babble-protocol/sdk": sdk }),
});
const session = "surf_document-test";
const document = "def917ca-10a0-4978-bf2d-c0190709a07b";
const binding = { session_id: session, document_id: document };

test("document registration uses the authenticated client fetch, exact binding and cancellation", async () => {
  const controller = new AbortController();
  let calls = 0;
  const client = new BabbleFrontendClient("https://node.test", async (url, init) => {
    calls++;
    assert.equal(url.href, `https://node.test/runtime/surfaces/sessions/${session}/document`);
    assert.equal(init.method, "PUT");
    assert.deepEqual(JSON.parse(init.body), { document_id: document });
    assert.equal(init.signal, controller.signal);
    assert.equal(init.headers["content-type"], "application/json");
    assert.equal(init.headers["x-babble-surface-document"], undefined, "registration is host management, not an embedded call");
    return Response.json(binding);
  });
  assert.equal(await client.registerSurfaceDocument(session, document, controller.signal), undefined);
  assert.equal(calls, 1);
  controller.abort(new Error("account changed"));
  await assert.rejects(client.registerSurfaceDocument(session, document, controller.signal), /account changed/);
  assert.equal(calls, 1);
});

for (const invalid of [null, [], {}, { ...binding, session_id: "other" }, { ...binding, document_id: "other" },
  { session_id: session }, { document_id: document }]) {
  test(`registration rejects mismatched or malformed acknowledgement: ${JSON.stringify(invalid)}`, async () => {
    const client = new BabbleFrontendClient("https://node.test", async () => Response.json(invalid));
    await assert.rejects(client.registerSurfaceDocument(session, document, new AbortController().signal), /mismatched binding/);
  });
}

for (const status of [401, 403, 409, 500]) {
  test(`registration failure ${status} remains visible and is not retried automatically`, async () => {
    let calls = 0;
    const client = new BabbleFrontendClient("https://node.test", async () => { calls++; return new Response(null, { status }); });
    await assert.rejects(client.registerSurfaceDocument(session, document, new AbortController().signal), new RegExp(`failed \\(${status}\\)`));
    assert.equal(calls, 1);
  });
}

for (const phase of ["fetch", "body"]) {
  test(`a cancelled ${phase} response cannot acknowledge an abandoned document`, async () => {
    const controller = new AbortController();
    const client = new BabbleFrontendClient("https://node.test", async () => {
      if (phase === "fetch") controller.abort(new Error("Surface closed"));
      return { ok: true, async json() {
        controller.abort(new Error("Surface closed"));
        return binding;
      } };
    });
    await assert.rejects(client.registerSurfaceDocument(session, document, controller.signal), /Surface closed/);
  });
}

test("bridge dispatch forwards the host-only document context as an HTTP header, not child payload", async () => {
  const controller = new AbortController();
  const request = { protocol: "babble.rpc.v1", id: "document-call", method: "babble.object.get.v1",
    binding: { object_id: "obj_test", surface_session_id: session, capability_grants: [], identity_id: null,
      runtime_id: "untrusted-child-id", origin: "https://child.test" }, payload: { object_id: "obj_test" } };
  const client = new BabbleFrontendClient("https://node.test", async (url, init) => {
    assert.equal(url.href, "https://node.test/rpc");
    assert.equal(init.headers["x-babble-surface-document"], document);
    assert.equal(init.signal, controller.signal);
    assert.deepEqual(JSON.parse(init.body), request);
    return Response.json({ protocol: "babble.rpc.v1", id: request.id, result: {}, error: null });
  });
  await client.bridgeDispatch()(request, { signal: controller.signal, surfaceDocumentId: document });
});
