import assert from "node:assert/strict";
import test from "node:test";
import { BrowserBridgeTransport, HttpRpcTransport, hostBinding, hostSurfaceBinding, rpcBridgeResponse } from "../dist/index.js";
import { request, response, FakePort } from "./channel-fixtures.mjs";

const documentId = "01020304-0506-4708-890a-0b0c0d0e0f10";

function httpHarness() {
  const calls = [];
  const transport = new HttpRpcTransport("https://host.test/rpc", async (url, init) => {
    calls.push({ url, init });
    return { ok: true, json: async () => response(JSON.parse(init.body)) };
  });
  return { calls, transport };
}

test("HTTP document binding uses a single header and forwards the dispatch abort signal", async () => {
  const h = httpHarness();
  const controller = new AbortController();
  const envelope = request();
  await h.transport.request(envelope, { surfaceDocumentId: documentId, signal: controller.signal });
  assert.equal(h.calls.length, 1);
  const { init } = h.calls[0];
  assert.deepEqual([...new Headers(init.headers)], [
    ["content-type", "application/json"], ["x-babel-surface-document", documentId],
  ]);
  assert.equal(init.signal, controller.signal);
  assert.deepEqual(JSON.parse(init.body), envelope);
  assert.equal(init.body.includes(documentId), false);
  controller.abort();
  assert.equal(init.signal.aborted, true);
});

test("HTTP rejects missing, malformed and multiple document values before fetch", async () => {
  const h = httpHarness();
  const malformed = [undefined, null, false, 12, {}, [documentId], "", "doc-id", documentId.toUpperCase(),
    ` ${documentId}`, `${documentId} `, `${documentId}\n`, `${documentId}\r\nX-Forged: yes`,
    `${documentId},${documentId}`, `${documentId} ${documentId}`, documentId.replaceAll("-", ""),
  ];
  for (const surfaceDocumentId of malformed) {
    await assert.rejects(h.transport.request(request(), { surfaceDocumentId }), /canonical lowercase UUID/);
  }
  const forged = request();
  forged.binding.document_id = documentId;
  forged.binding.surfaceDocumentId = documentId;
  await assert.rejects(h.transport.request(forged), /canonical lowercase UUID/);
  assert.equal(h.calls.length, 0);
});

test("HTTP host management needs no document and rejects stray document authority", async () => {
  const h = httpHarness();
  for (const binding of [hostBinding("runtime", "https://host.test"),
    hostSurfaceBinding({ runtimeId: "runtime", origin: "https://host.test", surfaceSessionId: "session" }),
    { ...request().binding, surface_session_id: null },
  ]) {
    const envelope = { ...request(), binding };
    await h.transport.request(envelope);
    assert.equal(new Headers(h.calls.at(-1).init.headers).has("x-babel-surface-document"), false);
    for (const surfaceDocumentId of [documentId, "", null]) {
      await assert.rejects(h.transport.request(envelope, { surfaceDocumentId }), /requires both Object and Surface/);
    }
  }
  assert.equal(h.calls.length, 3);
});

test("child bridge request options never serialize document authority", async (t) => {
  const port = new FakePort();
  const transport = new BrowserBridgeTransport(port);
  t.after(() => transport.close());
  const envelope = request();
  const pending = transport.request(envelope, { surfaceDocumentId: documentId });
  const wire = port.messages[0];
  assert.deepEqual(Object.keys(wire).sort(), ["envelope", "protocol", "type"]);
  assert.equal(JSON.stringify(wire).includes(documentId), false);
  assert.deepEqual(wire.envelope.binding, envelope.binding);
  port.emit(rpcBridgeResponse(response(wire.envelope)));
  assert.equal((await pending).id, envelope.id);
});
