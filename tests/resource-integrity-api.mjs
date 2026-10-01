import assert from "node:assert/strict";
import { open, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

// Only the live suite's disposable store is mutated. Restore the original entry
// before its browser mount, including when a failure interrupts the assertions.
export async function verifyResourceDelivery(api, storeRoot, surface) {
  const url = new URL(surface.entry);
  const hash = surface.integrity;
  assert.equal(url.origin, api);
  assert.match(hash, /^[a-f0-9]{64}$/);
  assert.equal(url.pathname, `/runtime/surfaces/blobs/${hash}`);
  const path = join(storeRoot, "blobs", hash);
  const original = await readFile(path);
  const request = (url, options = {}) => fetch(url, { ...options, signal: AbortSignal.timeout(15_000) });
  try {
    await writeFile(path, "Unverified executable corruption fixture");
    const corrupt = await request(url);
    assert.equal(corrupt.status, 409);
    const error = await corrupt.json();
    assert.equal(error.code, "conflict");
    assert.match(error.message, /integrity mismatch/);
    assert.doesNotMatch(JSON.stringify(error), /Unverified executable corruption fixture/);

    const file = await open(path, "r+");
    try { await file.truncate(8 * 1024 * 1024 + 1); }
    finally { await file.close(); }
    for (const endpoint of [url, `${api}/media/blobs/${hash}?media_type=text/html`]) {
      const oversized = await request(endpoint);
      assert.equal(oversized.status, 413);
      assert.equal((await oversized.json()).code, "payload_too_large");
    }
    const rpc = await request(`${api}/rpc`, {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ protocol: "babel.rpc.v1", id: "bounded-resource",
        method: "babel.media.blob.get.v1",
        binding: { object_id: null, surface_session_id: null, runtime_id: "resource-regression",
          origin: api, identity_id: null, capability_grants: [] },
        payload: { hash, media_type: "text/html" }, idempotency_key: null,
        deadline: { timeout_ms: 10_000, client_started_at: null }, trace_id: null }),
    });
    assert.equal(rpc.status, 200);
    const response = await rpc.json();
    assert.equal(response.result, null);
    assert.equal(response.error.code, "QUOTA_EXCEEDED");
  } finally {
    await writeFile(path, original);
  }
  const restored = await request(url);
  assert.equal(restored.status, 200);
  assert.equal(restored.headers.get("content-type"), "text/html");
  assert.equal(restored.headers.get("x-content-type-options"), "nosniff");
  assert.deepEqual(Buffer.from(await restored.arrayBuffer()), original);
  console.log("Resource delivery PASS: live HTTP/RPC corruption and size rejection, exact restored Surface bytes");
}
