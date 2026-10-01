import assert from "node:assert/strict";

// Runs on the live suite's disposable store, on both sides of a real API restart.
export async function prepareReactionRestart(apiUrl, token, objectId, actorId) {
  const path = `/objects/${objectId}/reactions`;
  const value = { appreciation: "like", engagement: "engaging", stance: "oppose", certainty: 0 };
  const intent = { value, expected_revision: 0, idempotency_key: `reaction-${objectId}` };
  const request = async (suffix, method = "GET", body = null, authenticated = false, expected = 200) => {
    const response = await fetch(`${apiUrl}${path}${suffix}`, {
      method, headers: { ...(authenticated ? { authorization: `Bearer ${token}` } : {}),
        ...(body ? { "content-type": "application/json" } : {}) },
      ...(body ? { body: JSON.stringify(body) } : {}),
    });
    assert.equal(response.status, expected, `${method} ${suffix}`);
    return response.json();
  };
  const before = await request("/mine", "GET", null, true);
  assert.equal(before.revision, 0);
  const saved = await request("/mine", "PUT", intent, true);
  assert.deepEqual(saved, { author_id: actorId, object_id: objectId, value, revision: 1 });
  const summary = await request("");
  assert.deepEqual(summary, { object_id: objectId, participants: 1, likes: 1, dislikes: 0,
    engaging: 1, not_engaging: 0, support: 0, oppose: 1, uncertain: 0, certainty_responses: 1 });
  const record = await request(`/actors/${actorId}`);
  assert.deepEqual(record.state, saved);
  assert.deepEqual(record.action.payload.state, saved);
  assert.ok(record.action.signature);
  await request("/mine", "PUT", { ...intent, idempotency_key: "stale-reaction" }, true, 409);
  await request("/mine", "PUT", intent, false, 401);

  return async () => {
    assert.deepEqual(await request("/mine", "GET", null, true), saved);
    assert.deepEqual(await request("/mine", "PUT", intent, true), saved);
    assert.deepEqual(await request(""), summary);
    const withdrawn = await request("/mine", "PUT", {
      value: { appreciation: null, engagement: null, stance: null, certainty: null },
      expected_revision: 1, idempotency_key: "withdraw-reaction",
    }, true);
    assert.equal(withdrawn.revision, 2);
    assert.equal((await request("")).participants, 0);
    assert.equal((await request(`/actors/${actorId}`)).action.payload.previous_id, record.action.id);
    assert.deepEqual(await request("/mine", "PUT", intent, true), saved);
    assert.deepEqual(await request("/mine", "GET", null, true), withdrawn,
      "an old acknowledgment must never resurrect a withdrawn reaction");
    console.log("Public reaction restart PASS", JSON.stringify({ persisted: true, exactRetry: true, withdrawn: true, signedRecord: true }));
  };
}
