import assert from "node:assert/strict";
import test from "node:test";
import {
  BabelError,
  createBabelSDK,
  createSurfaceLifecycle,
  createBabelClient,
  hostBinding,
  hostSurfaceBinding,
  objectBinding,
  protocol,
  requestId,
  rpcCatalog,
  HttpRpcTransport,
} from "../dist/index.js";

test("generated catalog exposes the Rust protocol contract", () => {
  assert.equal(protocol, "babel.v2");
  assert.equal(rpcCatalog.protocol, "babel.rpc.v1");
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.object.publish.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.object.publish_text.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.graph.relationship.infer.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.graph.evidence.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.graph.traverse.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.identity.current.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.object.fork.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.object.remix.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.runtime.surface.session.start.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.runtime.surface.session.state.checkpoint.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.runtime.surface.session.state.get.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.realtime.session.start.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.realtime.session.leave.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.storage.object.set.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.storage.local.set.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.personalization.sync.put.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.personalization.sync.list.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.personalization.sync.get.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.personalization.sync.delete.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.social.follow.v2"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.social.unfollow.v2"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.social.share.v2"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.social.reply.v2"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.events.list.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.events.import.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.network.fetch.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.payments.checkout.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.notifications.request.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.media.camera.request.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.media.microphone.request.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.clipboard.write.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.fullscreen.enter.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.clipboard.write.v2"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.fullscreen.enter.v2"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.lenses.list.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.capabilities.list.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.judgment.definitions.list.v1"));
  assert.ok(rpcCatalog.methods.some((method) => method.method === "babel.judgment.providers.list.v1"));
});

test("host SDK renews its bound Surface lease through the canonical method", async () => {
  const lease = { session_id: "surf_live", expires_at: "2026-09-30T09:00:00Z", ttl_ms: 60_000, renew_after_ms: 15_000 };
  const transport = new CaptureTransport({ protocol: "babel.rpc.v1", id: "heartbeat", result: { lease }, error: null, trace_id: null });
  const sdk = createBabelSDK({ transport, binding: hostSurfaceBinding({ runtimeId: "host", origin: "https://host.test", surfaceSessionId: lease.session_id }) });
  assert.deepEqual(await sdk.runtime.heartbeatSurfaceSession(undefined, { id: "heartbeat" }), { lease });
  assert.equal(transport.envelopes[0].method, "babel.runtime.surface.session.heartbeat.v1");
  assert.equal(transport.envelopes[0].binding.object_id, null);
  assert.equal(transport.envelopes[0].binding.surface_session_id, lease.session_id);
  assert.deepEqual(transport.envelopes[0].payload, {});
});

test("reaction SDK preserves independent dimensions and durable mutation keys", async () => {
  const object_id = `obj_${"a".repeat(64)}`;
  const actor_id = `id_${"b".repeat(64)}`;
  const value = { appreciation: "like", engagement: "engaging", stance: "uncertain", certainty: 0 };
  const state = { author_id: actor_id, object_id, value, revision: 1 };
  for (const [name, payload, options] of [
    ["summary", { object_id }, {}],
    ["record", { object_id, actor_id }, {}],
    ["mine", { object_id }, {}],
    ["set", { object_id, value, expected_revision: 0 }, { idempotencyKey: "reaction-retry" }],
  ]) {
    const transport = new CaptureTransport({ protocol: "babel.rpc.v1", id: name, result: state, error: null, trace_id: null });
    const sdk = createBabelSDK({ transport, binding: hostBinding({ runtimeId: "host", origin: "https://host.test", identityId: actor_id }) });
    assert.deepEqual(await sdk.social.reactions[name](payload, { id: name, ...options }), state);
    const envelope = transport.envelopes[0];
    assert.equal(envelope.method, `babel.social.reactions.${name}.v1`);
    assert.deepEqual(envelope.payload, payload);
    assert.equal(envelope.binding.object_id, null);
    assert.equal(envelope.binding.surface_session_id, null);
    assert.equal(envelope.idempotency_key, options.idempotencyKey ?? null);
  }
});

test("client builds envelopes and returns typed RPC results", async () => {
  const transport = new CaptureTransport({
    protocol: "babel.rpc.v1",
    id: "publish-1",
    result: {
      object: {
        id: "obj_abc",
        kind: "babel.text",
        payload: { text: "hello" },
      },
    },
    error: null,
    trace_id: "trace-1",
  });
  const client = createBabelClient({
    transport,
    binding: hostBinding("runtime", "https://example.test"),
  });
  const result = await client.request(
    "babel.object.publish_text.v1",
    { author_id: "id_abc", text: "hello" },
    { id: "publish-1", idempotencyKey: "idem-1", traceId: "trace-1" },
  );

  assert.equal(result.object.id, "obj_abc");
  assert.equal(transport.envelopes.length, 1);
  assert.equal(transport.envelopes[0].idempotency_key, "idem-1");
  assert.equal(transport.envelopes[0].binding.runtime_id, "runtime");
  assert.equal(transport.envelopes[0].method, "babel.object.publish_text.v1");
});

test("client turns structured RPC errors into BabelError", async () => {
  const transport = new CaptureTransport({
    protocol: "babel.rpc.v1",
    id: "missing",
    result: null,
    error: {
      code: "NOT_FOUND",
      message: "object not found",
      retryable: false,
      retry_after_ms: null,
      details: { object_id: "obj_missing" },
    },
    trace_id: null,
  });
  const client = createBabelClient({
    transport,
    binding: objectBinding({
      objectId: "obj_bound",
      surfaceSessionId: "surface",
      runtimeId: "runtime",
      origin: "babel://test",
      capabilityGrants: ["grant_1"],
    }),
  });

  await assert.rejects(
    client.request("babel.object.get.v1", { object_id: "obj_missing" }, { id: "missing" }),
    (error) => {
      assert.ok(error instanceof BabelError);
      assert.equal(error.code, "NOT_FOUND");
      assert.deepEqual(error.details, { object_id: "obj_missing" });
      return true;
    },
  );
});

test("request id generation is deterministic for method and payload", () => {
  assert.equal(
    requestId("babel.object.get.v1", { object_id: "obj_1" }),
    requestId("babel.object.get.v1", { object_id: "obj_1" }),
  );
  assert.notEqual(
    requestId("babel.object.get.v1", { object_id: "obj_1" }),
    requestId("babel.object.get.v1", { object_id: "obj_2" }),
  );
});

test("SDK exposes capability-bound object storage helpers", async () => {
  const transport = new CaptureTransport({
    protocol: "babel.rpc.v1",
    id: "storage-set-1",
    result: {
      entry: {
        key: "settings/theme",
        value: { mode: "dark" },
        updated_at: "2026-09-27T00:00:00Z",
        size_bytes: 15,
      },
      receipt: capabilityReceiptFixture("babel.storage.object"),
    },
    error: null,
    trace_id: null,
  });
  const sdk = createBabelSDK({
    transport,
    binding: objectBinding({
      objectId: "obj_bound",
      surfaceSessionId: "surface",
      runtimeId: "runtime",
      origin: "babel://test",
      capabilityGrants: ["grant_storage"],
    }),
  });

  const response = await sdk.storage.object.set(
    { key: "settings/theme", value: { mode: "dark" } },
    { id: "storage-set-1", idempotencyKey: "storage-set-key" },
  );

  assert.equal(response.entry.key, "settings/theme");
  assert.equal(response.receipt.capability, "babel.storage.object");
  assert.equal(transport.envelopes[0].method, "babel.storage.object.set.v1");
  assert.equal(transport.envelopes[0].idempotency_key, "storage-set-key");
  assert.deepEqual(transport.envelopes[0].binding.capability_grants, ["grant_storage"]);
});

test("SDK exposes capability-bound local storage helpers", async () => {
  const transport = new CaptureTransport({
    protocol: "babel.rpc.v1",
    id: "local-storage-set-1",
    result: {
      entry: {
        key: "settings/theme",
        value: { mode: "dark" },
        updated_at: "2026-09-27T00:00:00Z",
        size_bytes: 15,
      },
      receipt: capabilityReceiptFixture("babel.storage.local"),
    },
    error: null,
    trace_id: null,
  });
  const sdk = createBabelSDK({
    transport,
    binding: objectBinding({
      objectId: "obj_bound",
      surfaceSessionId: "surface",
      runtimeId: "runtime",
      origin: "babel://test",
      capabilityGrants: ["grant_local"],
      identityId: "id_alice",
    }),
  });

  const response = await sdk.storage.local.set(
    { key: "settings/theme", value: { mode: "dark" } },
    { id: "local-storage-set-1", idempotencyKey: "local-storage-set-key" },
  );

  assert.equal(response.entry.key, "settings/theme");
  assert.equal(response.receipt.capability, "babel.storage.local");
  assert.equal(transport.envelopes[0].method, "babel.storage.local.set.v1");
  assert.equal(transport.envelopes[0].idempotency_key, "local-storage-set-key");
  assert.equal(transport.envelopes[0].binding.identity_id, "id_alice");
  assert.deepEqual(transport.envelopes[0].binding.capability_grants, ["grant_local"]);
});

test("SDK exposes encrypted personalization sync helpers", async () => {
  const envelope = encryptedEnvelopeFixture();
  const transport = new QueueTransport([
    {
      protocol: "babel.rpc.v1",
      id: "personalization-sync-put-1",
      result: {
        envelope: envelopeSummaryFixture(),
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "personalization-sync-list-1",
      result: { envelopes: [envelopeSummaryFixture()] },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "personalization-sync-get-1",
      result: { envelope, summary: envelopeSummaryFixture() },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "personalization-sync-delete-1",
      result: { deleted: envelopeSummaryFixture() },
      error: null,
      trace_id: null,
    },
  ]);
  const sdk = createBabelSDK({
    transport,
    binding: hostBinding("runtime", "https://example.test"),
  });

  const put = await sdk.personalization.sync.put(
    { envelope },
    { id: "personalization-sync-put-1", idempotencyKey: "personalization-sync-put-key" },
  );
  assert.equal(put.envelope.device_id, "desktop-main");
  assert.equal(transport.envelopes[0].method, "babel.personalization.sync.put.v1");
  assert.equal(transport.envelopes[0].idempotency_key, "personalization-sync-put-key");

  const listed = await sdk.personalization.sync.list({
    identity_id: envelope.recipient.identity_id,
    device_id: "desktop-main",
  }, { id: "personalization-sync-list-1" });
  assert.equal(listed.envelopes.length, 1);
  assert.equal(transport.envelopes[1].method, "babel.personalization.sync.list.v1");

  const fetched = await sdk.personalization.sync.get({
    identity_id: envelope.recipient.identity_id,
    device_id: "desktop-main",
    envelope_hash: envelopeSummaryFixture().envelope_hash,
  }, { id: "personalization-sync-get-1" });
  assert.equal(fetched.envelope.ciphertext, envelope.ciphertext);
  assert.equal(transport.envelopes[2].method, "babel.personalization.sync.get.v1");

  const deleted = await sdk.personalization.sync.delete({
    identity_id: envelope.recipient.identity_id,
    device_id: "desktop-main",
    envelope_hash: envelopeSummaryFixture().envelope_hash,
  }, { id: "personalization-sync-delete-1", idempotencyKey: "personalization-sync-delete-key" });
  assert.equal(deleted.deleted.device_id, "desktop-main");
  assert.equal(transport.envelopes[3].method, "babel.personalization.sync.delete.v1");
  assert.equal(transport.envelopes[3].idempotency_key, "personalization-sync-delete-key");
});

test("SDK exposes capability-bound current identity helper", async () => {
  const transport = new CaptureTransport({
    protocol: "babel.rpc.v1",
    id: "identity-current-1",
    result: {
      identity: identityFixture("id_alice", "alice"),
      receipt: capabilityReceiptFixture("babel.identity.current"),
    },
    error: null,
    trace_id: null,
  });
  const sdk = createBabelSDK({
    transport,
    binding: objectBinding({
      objectId: "obj_bound",
      surfaceSessionId: "surface",
      runtimeId: "runtime",
      origin: "babel://test",
      capabilityGrants: ["grant_identity"],
      identityId: "id_alice",
    }),
  });

  const response = await sdk.identity.current({}, { id: "identity-current-1" });

  assert.equal(response.identity.handle, "alice");
  assert.equal(response.receipt.capability, "babel.identity.current");
  assert.equal(transport.envelopes[0].method, "babel.identity.current.v1");
  assert.equal(transport.envelopes[0].binding.identity_id, "id_alice");
  assert.deepEqual(transport.envelopes[0].binding.capability_grants, ["grant_identity"]);
});

test("SDK exposes capability-bound social helpers", async () => {
  const transport = new CaptureTransport({
    protocol: "babel.rpc.v1",
    id: "social-share-1",
    result: {
      object: textObjectFixture("obj_share", "Sharing with signed context."),
      edge: edgeFixture("edge_share", "obj_share", "obj_target", "quotes"),
      receipt: capabilityReceiptFixture("babel.social.share", { object_id: "obj_target" }),
    },
    error: null,
    trace_id: null,
  });
  const sdk = createBabelSDK({
    transport,
    binding: objectBinding({
      objectId: "obj_bound",
      surfaceSessionId: "surface",
      runtimeId: "runtime",
      origin: "babel://test",
      capabilityGrants: ["grant_share"],
    }),
  });

  const response = await sdk.social.share(
    {
      author_id: "id_alice",
      target_object_id: "obj_target",
      text: "Sharing with signed context.",
    },
    { id: "social-share-1", idempotencyKey: "social-share-key" },
  );

  assert.equal(response.edge.relation, "quotes");
  assert.equal(response.receipt.capability, "babel.social.share");
  assert.equal(transport.envelopes[0].method, "babel.social.share.v2");
  assert.equal(transport.envelopes[0].idempotency_key, "social-share-key");
  assert.deepEqual(transport.envelopes[0].binding.capability_grants, ["grant_share"]);
});

test("SDK exposes capability-bound realtime leave helper", async () => {
  const transport = new CaptureTransport({
    protocol: "babel.rpc.v1",
    id: "leave-1",
    result: {
      session: realtimeSessionFixture("closed"),
      event: {
        id: "event_leave",
        actor: "id_alice",
        kind: "realtime_session_closed",
        target: { Object: "obj_bound" },
        payload: { session: realtimeSessionFixture("closed") },
        parents: [],
        created_at: "2026-09-27T00:00:00Z",
        signature: null,
      },
      receipt: capabilityReceiptFixture("babel.realtime.leave", { room: "main" }),
    },
    error: null,
    trace_id: null,
  });
  const sdk = createBabelSDK({
    transport,
    binding: objectBinding({
      objectId: "obj_bound",
      surfaceSessionId: "surface",
      runtimeId: "runtime",
      origin: "babel://test",
      capabilityGrants: ["grant_leave"],
    }),
  });

  const response = await sdk.realtime.leaveSession(
    { author_id: "id_alice", session_id: "session_1", object_id: "obj_bound" },
    { id: "leave-1", idempotencyKey: "leave-key" },
  );

  assert.equal(response.session.state, "closed");
  assert.equal(response.event.kind, "realtime_session_closed");
  assert.equal(response.receipt.capability, "babel.realtime.leave");
  assert.equal(transport.envelopes[0].method, "babel.realtime.session.leave.v1");
  assert.equal(transport.envelopes[0].idempotency_key, "leave-key");
});

test("SDK exposes capability-bound network fetch helper", async () => {
  const transport = new CaptureTransport({
    protocol: "babel.rpc.v1",
    id: "network-fetch-1",
    result: {
      status: 200,
      headers: {
        "content-type": "text/plain",
        "x-babel-test": "ok",
      },
      body_hex: "68656c6c6f",
      receipt: capabilityReceiptFixture("babel.network.fetch"),
    },
    error: null,
    trace_id: null,
  });
  const sdk = createBabelSDK({
    transport,
    binding: objectBinding({
      objectId: "obj_bound",
      surfaceSessionId: "surface",
      runtimeId: "runtime",
      origin: "babel://test",
      capabilityGrants: ["grant_network"],
    }),
  });

  const response = await sdk.network.fetch(
    {
      method: "GET",
      url: "https://example.com/data",
      headers: { "X-Babel-Test": "request" },
      body_hex: null,
    },
    { id: "network-fetch-1", idempotencyKey: "network-fetch-key" },
  );

  assert.equal(response.status, 200);
  assert.equal(response.body_hex, "68656c6c6f");
  assert.equal(response.receipt.capability, "babel.network.fetch");
  assert.equal(transport.envelopes[0].method, "babel.network.fetch.v1");
  assert.equal(transport.envelopes[0].idempotency_key, "network-fetch-key");
  assert.deepEqual(transport.envelopes[0].binding.capability_grants, ["grant_network"]);
});

test("SDK exposes capability-bound payments and notification helpers", async () => {
  const transport = new QueueTransport([
    {
      protocol: "babel.rpc.v1",
      id: "checkout-1",
      result: {
        action: {
          kind: "payments.checkout",
          merchant_id: "merchant.babel",
          merchant_name: "Babel Merchant",
          currency: "USD",
          total_amount_minor: 2500,
          line_items: [{ label: "Creator pass", amount_minor: 2500, quantity: 1 }],
          success_url: "https://example.com/success",
          cancel_url: "https://example.com/cancel",
          reference: "order_2",
          requires_user_activation: true,
        },
        receipt: capabilityReceiptFixture("babel.payments.checkout", {
          currencies: ["USD"],
          max_amount_minor: 5000,
          merchant_id: "merchant.babel",
        }),
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "notifications-1",
      result: {
        action: {
          kind: "notifications.request",
          purpose: "Notify players and followers about Object activity.",
          categories: ["game.turn"],
          requires_user_activation: true,
        },
        receipt: capabilityReceiptFixture("babel.notifications.request", {
          categories: ["game.turn", "creator.update"],
          purpose: "Notify players and followers about Object activity.",
        }),
      },
      error: null,
      trace_id: null,
    },
  ]);
  const sdk = createBabelSDK({
    transport,
    binding: objectBinding({
      objectId: "obj_bound",
      surfaceSessionId: "surface",
      runtimeId: "runtime",
      origin: "babel://test",
      capabilityGrants: ["grant_payment", "grant_notifications"],
    }),
  });

  const checkout = await sdk.payments.checkout(
    {
      merchant_id: "merchant.babel",
      merchant_name: "Babel Merchant",
      currency: "USD",
      total_amount_minor: 2500,
      line_items: [{ label: "Creator pass", amount_minor: 2500, quantity: 1 }],
      success_url: "https://example.com/success",
      cancel_url: "https://example.com/cancel",
      reference: "order_2",
    },
    { id: "checkout-1", idempotencyKey: "checkout-key" },
  );
  const notifications = await sdk.notifications.request(
    {
      purpose: "Notify players and followers about Object activity.",
      categories: ["game.turn"],
    },
    { id: "notifications-1" },
  );

  assert.equal(checkout.action.kind, "payments.checkout");
  assert.equal(checkout.action.total_amount_minor, 2500);
  assert.equal(checkout.receipt.capability, "babel.payments.checkout");
  assert.equal(notifications.action.kind, "notifications.request");
  assert.deepEqual(notifications.action.categories, ["game.turn"]);
  assert.equal(notifications.receipt.capability, "babel.notifications.request");
  assert.deepEqual(
    transport.envelopes.map((envelope) => [envelope.method, envelope.idempotency_key ?? null]),
    [
      ["babel.payments.checkout.v1", "checkout-key"],
      ["babel.notifications.request.v1", null],
    ],
  );
});

test("SDK exposes capability-bound media capture helpers", async () => {
  const transport = new QueueTransport([
    {
      protocol: "babel.rpc.v1",
      id: "camera-1",
      result: {
        action: {
          kind: "media.camera.request",
          purpose: "Attach a profile photo to this Object.",
          mode: "photo",
          media_types: ["image/jpeg"],
          max_duration_ms: 5000,
          facing_mode: "user",
          width: 1280,
          height: 720,
          requires_user_activation: true,
        },
        receipt: capabilityReceiptFixture("babel.media.camera", {
          modes: ["photo"],
          media_types: ["image/jpeg"],
          max_duration_ms: 30000,
          facing_modes: ["user"],
        }),
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "microphone-1",
      result: {
        action: {
          kind: "media.microphone.request",
          purpose: "Record a short spoken reply.",
          mode: "audio_clip",
          media_types: ["audio/webm"],
          max_duration_ms: 5000,
          echo_cancellation: true,
          noise_suppression: true,
          requires_user_activation: true,
        },
        receipt: capabilityReceiptFixture("babel.media.microphone", {
          modes: ["audio_clip"],
          media_types: ["audio/webm"],
          max_duration_ms: 30000,
        }),
      },
      error: null,
      trace_id: null,
    },
  ]);
  const sdk = createBabelSDK({
    transport,
    binding: objectBinding({
      objectId: "obj_bound",
      surfaceSessionId: "surface",
      runtimeId: "runtime",
      origin: "babel://test",
      capabilityGrants: ["grant_camera", "grant_microphone"],
    }),
  });

  const camera = await sdk.media.camera.request(
    {
      purpose: "Attach a profile photo to this Object.",
      mode: "photo",
      media_types: ["image/jpeg"],
      max_duration_ms: 5000,
      facing_mode: "user",
      width: 1280,
      height: 720,
    },
    { id: "camera-1" },
  );
  const microphone = await sdk.media.microphone.request(
    {
      purpose: "Record a short spoken reply.",
      mode: "audio_clip",
      media_types: ["audio/webm"],
      max_duration_ms: 5000,
      echo_cancellation: true,
      noise_suppression: true,
    },
    { id: "microphone-1" },
  );

  assert.equal(camera.action.kind, "media.camera.request");
  assert.equal(camera.action.facing_mode, "user");
  assert.equal(camera.receipt.capability, "babel.media.camera");
  assert.equal(microphone.action.kind, "media.microphone.request");
  assert.equal(microphone.action.echo_cancellation, true);
  assert.equal(microphone.receipt.capability, "babel.media.microphone");
  assert.deepEqual(
    transport.envelopes.map((envelope) => [envelope.method, envelope.idempotency_key ?? null]),
    [
      ["babel.media.camera.request.v1", null],
      ["babel.media.microphone.request.v1", null],
    ],
  );
});

test("SDK browser helpers use v2 completed results and explicit idempotency keys", async () => {
  const transport = new QueueTransport([
    {
      protocol: "babel.rpc.v1",
      id: "clipboard-1",
      result: {
        kind: "clipboard_write",
        written: true,
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "fullscreen-1",
      result: {
        kind: "fullscreen_enter",
        entered: true,
      },
      error: null,
      trace_id: null,
    },
  ]);
  const sdk = createBabelSDK({
    transport,
    binding: objectBinding({
      objectId: "obj_bound",
      surfaceSessionId: "surface",
      runtimeId: "runtime",
      origin: "babel://test",
      capabilityGrants: [],
    }),
  });

  const clipboard = await sdk.clipboard.write(
    { text: "copy from Object" },
    { id: "clipboard-1", idempotencyKey: "clipboard-key" },
  );
  const fullscreen = await sdk.fullscreen.enter(
    { target_hint: "surface-root", navigation_ui: "hide" },
    { id: "fullscreen-1", idempotencyKey: "fullscreen-key" },
  );

  assert.deepEqual(clipboard, { kind: "clipboard_write", written: true });
  assert.deepEqual(fullscreen, { kind: "fullscreen_enter", entered: true });
  assert.deepEqual(transport.envelopes.map(envelope => envelope.payload), [
    { text: "copy from Object" }, { target_hint: "surface-root", navigation_ui: "hide" },
  ]);
  assert.deepEqual(
    transport.envelopes.map((envelope) => [envelope.method, envelope.idempotency_key ?? null]),
    [
      ["babel.clipboard.write.v2", "clipboard-key"],
      ["babel.fullscreen.enter.v2", "fullscreen-key"],
    ],
  );
});

test("SDK browser retries preserve caller keys and propagate durable errors without retrying", async () => {
  for (const [namespace, operation, payload, completed] of [
    ["clipboard", "write", { text: "same exact text" }, { kind: "clipboard_write", written: true }],
    ["fullscreen", "enter", {}, { kind: "fullscreen_enter", entered: true }],
  ]) {
    const envelopes = [];
    let rejected = false;
    const sdk = createBabelSDK({ binding: objectBinding({ objectId: "obj_bound", surfaceSessionId: "surface",
      runtimeId: "runtime", origin: "babel://test", capabilityGrants: [] }), transport: {
      async request(envelope) {
        envelopes.push(envelope);
        return { protocol: envelope.protocol, id: envelope.id, trace_id: null,
          result: rejected ? null : completed,
          error: rejected ? { code: "CAPABILITY_DENIED", message: "Previously denied", retryable: false,
            retry_after_ms: null, details: { invocation: { state: { kind: "denied" } } } } : null };
      }, close() {},
    } });
    assert.deepEqual(await sdk[namespace][operation](payload, { id: "first", idempotencyKey: "caller-key" }), completed);
    assert.deepEqual(await sdk[namespace][operation](payload, { id: "retry", idempotencyKey: "caller-key" }), completed);
    rejected = true;
    await assert.rejects(sdk[namespace][operation](payload, { id: "denied", idempotencyKey: "denied-key" }),
      error => error instanceof BabelError && error.code === "CAPABILITY_DENIED");
    assert.equal(envelopes.length, 3, "SDK must not auto-retry a browser mutation");
    assert.deepEqual(envelopes.map(value => value.idempotency_key), ["caller-key", "caller-key", "denied-key"]);
    assert.ok(envelopes.every(value => value.method === `babel.${namespace}.${operation}.v2`));
    assert.ok(envelopes.every(value => JSON.stringify(value.payload) === JSON.stringify(payload)));
  }
});

test("SDK exposes capability-bound AI Judgment helper", async () => {
  const transport = new CaptureTransport({
    protocol: "babel.rpc.v1",
    id: "ai-judge-1",
    result: {
      judgment: judgmentFixture("jud_ai", "babel.judgment.evidence_quality.v1"),
      orchestration: null,
      receipt: capabilityReceiptFixture("babel.ai.judge", {
        definition: "babel.judgment.evidence_quality.v1",
        object_id: "obj_target",
      }),
    },
    error: null,
    trace_id: null,
  });
  const sdk = createBabelSDK({
    transport,
    binding: objectBinding({
      objectId: "obj_bound",
      surfaceSessionId: "surface",
      runtimeId: "runtime",
      origin: "babel://test",
      capabilityGrants: ["grant_ai"],
    }),
  });

  const response = await sdk.ai.judge(
    {
      object_id: "obj_target",
      definition: "babel.judgment.evidence_quality.v1",
      parameters: {},
    },
    { id: "ai-judge-1" },
  );

  assert.equal(response.judgment.definition, "babel.judgment.evidence_quality.v1");
  assert.equal(response.receipt?.capability, "babel.ai.judge");
  assert.equal(transport.envelopes[0].method, "babel.ai.judge.v1");
  assert.deepEqual(transport.envelopes[0].binding.capability_grants, ["grant_ai"]);
});

test("SDK exposes capability-bound AI host action helpers", async () => {
  const transport = new QueueTransport([
    {
      protocol: "babel.rpc.v1",
      id: "ai-generate",
      result: {
        action: {
          kind: "ai.generate",
          purpose: "Draft feed copy",
          task: "text",
          prompt: "Summarize this Object.",
          output_modalities: ["text"],
          model: "local/text-v1",
          max_output_tokens: 256,
          temperature_millis: 700,
          requires_user_activation: false,
        },
        receipt: capabilityReceiptFixture("babel.ai.generate", {
          tasks: ["text"],
          output_modalities: ["text"],
        }),
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "ai-embed",
      result: {
        action: {
          kind: "ai.embed",
          purpose: "Rank related Objects",
          input_modality: "text",
          inputs: ["Babel Objects are executable social media."],
          model: "local/embed-v1",
          dimensions: 384,
          requires_user_activation: false,
        },
        receipt: capabilityReceiptFixture("babel.ai.embed", {
          input_modalities: ["text"],
        }),
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "ai-transcribe",
      result: {
        action: {
          kind: "ai.transcribe",
          purpose: "Caption Object audio",
          media_uri: "babel://blobs/sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
          media_type: "audio/webm",
          model: "local/transcribe-v1",
          language: "en-US",
          max_duration_ms: 30000,
          requires_user_activation: false,
        },
        receipt: capabilityReceiptFixture("babel.ai.transcribe", {
          media_types: ["audio/webm"],
        }),
      },
      error: null,
      trace_id: null,
    },
  ]);
  const sdk = createBabelSDK({
    transport,
    binding: objectBinding({
      objectId: "obj_bound",
      surfaceSessionId: "surface",
      runtimeId: "runtime",
      origin: "babel://test",
      capabilityGrants: ["grant_ai_actions"],
    }),
  });

  const generated = await sdk.ai.generate(
    {
      purpose: "Draft feed copy",
      task: "text",
      prompt: "Summarize this Object.",
      output_modalities: ["text"],
      model: "local/text-v1",
      max_output_tokens: 256,
      temperature_millis: 700,
    },
    { id: "ai-generate" },
  );
  const embedded = await sdk.ai.embed(
    {
      purpose: "Rank related Objects",
      input_modality: "text",
      inputs: ["Babel Objects are executable social media."],
      model: "local/embed-v1",
      dimensions: 384,
    },
    { id: "ai-embed" },
  );
  const transcribed = await sdk.ai.transcribe(
    {
      purpose: "Caption Object audio",
      media_uri: "babel://blobs/sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      media_type: "audio/webm",
      model: "local/transcribe-v1",
      language: "en-US",
      max_duration_ms: 30000,
    },
    { id: "ai-transcribe" },
  );

  assert.equal(generated.receipt.capability, "babel.ai.generate");
  assert.equal(embedded.receipt.capability, "babel.ai.embed");
  assert.equal(transcribed.receipt.capability, "babel.ai.transcribe");
  assert.deepEqual(
    transport.envelopes.map((envelope) => envelope.method),
    ["babel.ai.generate.v1", "babel.ai.embed.v1", "babel.ai.transcribe.v1"],
  );
  assert.deepEqual(transport.envelopes[0].binding.capability_grants, ["grant_ai_actions"]);
});

test("SDK exposes event listing, bundle, and import helpers", async () => {
  const event = {
    ...eventFixture("evt_object"),
    kind: "object_published",
    target: { Object: "obj_text" },
  };
  const bundle = {
    identities: [identityFixture("id_alice", "alice")],
    objects: [textObjectFixture("obj_text", "Replicated protocol state.")],
    edges: [],
    events: [event],
  };
  const transport = new QueueTransport([
    {
      protocol: "babel.rpc.v1",
      id: "events-list",
      result: {
        events: [event],
        next_after: "evt_object",
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "events-bundle",
      result: { bundle },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "events-import",
      result: {
        report: {
          identities: 1,
          objects: 1,
          edges: 0,
          events: 1,
          duplicate_events: 0,
        },
      },
      error: null,
      trace_id: null,
    },
  ]);
  const sdk = createBabelSDK({
    transport,
    binding: hostBinding("runtime", "babel://test"),
  });

  const listed = await sdk.events.list({ after: null, limit: 25 }, { id: "events-list" });
  const bundled = await sdk.events.bundle({ events: ["evt_object"] }, { id: "events-bundle" });
  const imported = await sdk.events.importBundle(
    { bundle },
    { id: "events-import", idempotencyKey: "events-import-key" },
  );

  assert.equal(listed.next_after, "evt_object");
  assert.equal(bundled.bundle.objects[0].id, "obj_text");
  assert.equal(imported.report.events, 1);
  assert.deepEqual(
    transport.envelopes.map((envelope) => [envelope.method, envelope.idempotency_key ?? null]),
    [
      ["babel.events.list.v1", null],
      ["babel.events.bundle.v1", null],
      ["babel.events.import.v1", "events-import-key"],
    ],
  );
});

test("HTTP transport binds the default browser fetch receiver", async () => {
  const originalFetch = globalThis.fetch;
  try {
    globalThis.fetch = async function fetchWithReceiverCheck() {
      assert.equal(this, globalThis);
      return {
        ok: true,
        async json() {
          return {
            protocol: "babel.rpc.v1",
            id: "bound-fetch",
            result: { ok: true },
            error: null,
            trace_id: null,
          };
        },
      };
    };

    const transport = new HttpRpcTransport("https://babel.test/rpc");
    const response = await transport.request({
      protocol: "babel.rpc.v1",
      id: "bound-fetch",
      method: "babel.search.objects.v1",
      binding: hostBinding("runtime", "https://babel.test"),
      payload: { q: null, author: null, kind: null, limit: 1 },
      idempotency_key: null,
      deadline: {
        timeout_ms: 30000,
        client_started_at: "2026-09-27T00:00:00Z",
      },
      trace_id: null,
    });

    assert.equal(response.id, "bound-fetch");
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("SDK namespaces map to exact RPC methods and preserve idempotency", async () => {
  const transport = new QueueTransport([
    {
      protocol: "babel.rpc.v1",
      id: "identity-1",
      result: {
        identity: {
          id: "id_alice",
          kind: "Person",
          handle: "alice",
          created_at: "2026-09-27T00:00:00Z",
          public_key: { algorithm: "Ed25519", bytes: "pub" },
          signature: { algorithm: "Ed25519", bytes: "sig" },
        },
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "search-1",
      result: { results: [] },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "judgment-definitions-1",
      result: {
        definitions: [
          {
            id: "babel.judgment.relationship.v1",
            input_schema: "babel.judgment.input.object_text.v1",
            output_schema: "babel.judgment.output.relationship.v1",
            meaning: "Estimate whether the subject text supports, contradicts, or relates to context.",
            calibration: "score is bounded in [0, 1]; relation names the evaluated edge semantics",
          },
        ],
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "judgment-providers-1",
      result: {
        providers: [
          {
            provider: {
              provider: "babel-local",
              model: "rules-v1",
              version: "1",
            },
            role: "Local",
            supported_definitions: ["babel.judgment.relationship.v1"],
            privacy_policy: {
              include_subject: true,
              allowed_context_keys: ["object", "text"],
              max_text_bytes: null,
            },
            enabled: true,
          },
        ],
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "lenses-1",
      result: {
        lenses: [
          {
            id: "babel.lens.research.v1",
            lens: "Research",
            version: 1,
            name: "Research",
            description: "Prioritizes claims, sources, citations, and high-quality evidence paths.",
            execution: "LocalDeterministic",
            required_signals: ["evidence_quality", "citation_density", "semantic_overlap"],
            required_sources: ["Evidence", "SemanticNeighborhood"],
            required_permissions: [],
          },
        ],
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "capabilities-list-1",
      result: {
        capabilities: [
          {
            id: "babel.network.fetch",
            version: 1,
            request_schema: { type: "object" },
            response_schema: { type: "object" },
            permission: "ask_once",
            quota: {
              calls_per_minute: 60,
              bytes_per_minute: 2097152,
              persistent_bytes: 0,
              realtime_connections: 0,
              max_call_ms: 5000,
              background_allowed: false,
            },
          },
        ],
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "publish-object-1",
      result: {
        object: {
          ...textObjectFixture("obj_canvas", "generic Object draft"),
          kind: "babel.canvas",
          schema: "example.canvas.v1",
          payload: { title: "Collaborative canvas", layers: [] },
          state: { revision: 1 },
        },
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "fork-1",
      result: provenancePublicationFixture("forked"),
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "remix-1",
      result: provenancePublicationFixture("remixed"),
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "graph-relationship-1",
      result: {
        edge: edgeFixture(
          "edge_relationship",
          "obj_evidence",
          "obj_bound",
          "supports",
          "JudgmentDerived",
          {
            judgment_id: "jud_relationship",
            definition: "babel.judgment.relationship.v1",
          },
        ),
        judgment: judgmentFixture("jud_relationship", "babel.judgment.relationship.v1"),
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "graph-traverse-1",
      result: {
        traversal: {
          root: "obj_bound",
          direction: "incoming",
          relations: ["evidence_for"],
          max_depth: 2,
          steps: [
            {
              depth: 1,
              edge: edgeFixture("edge_evidence", "obj_evidence", "obj_bound", "evidence_for"),
              next_object: "obj_evidence",
            },
          ],
          truncated: false,
        },
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "graph-evidence-1",
      result: {
        projection: {
          claim: textObjectFixture("obj_bound", "Claim"),
          supporting: [
            {
              kind: "supports",
              edge: edgeFixture(
                "edge_relationship",
                "obj_evidence",
                "obj_bound",
                "supports",
                "JudgmentDerived",
                {
                  judgment_id: "jud_relationship",
                  definition: "babel.judgment.relationship.v1",
                },
              ),
              evidence: textObjectFixture("obj_evidence", "Evidence"),
              relationship_judgment: judgmentFixture("jud_relationship", "babel.judgment.relationship.v1"),
              evidence_judgments: [judgmentFixture("jud_evidence", "babel.judgment.evidence_quality.v1")],
            },
          ],
          contradicting: [],
          related: [],
          summary: {
            human_support: 0,
            judgment_support: 1,
            human_contradiction: 0,
            judgment_contradiction: 0,
            related: 0,
          },
        },
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "room-1",
      result: {
        event: eventFixture("event_room"),
        room: roomFixture("room_main"),
      },
      error: null,
      trace_id: null,
    },
  ]);
  const sdk = createBabelSDK({
    transport,
    binding: objectBinding({
      objectId: "obj_bound",
      surfaceSessionId: "surface",
      runtimeId: "runtime",
      origin: "babel://test",
      capabilityGrants: ["grant_realtime"],
    }),
  });

  const identity = await sdk.identity.create(
    { kind: "Person", handle: "alice" },
    { id: "identity-1", idempotencyKey: "identity-key" },
  );
  const search = await sdk.search.objects({ q: "babel", author: null, kind: null, limit: 10 }, { id: "search-1" });
  const judgmentDefinitions = await sdk.judgment.listDefinitions({}, { id: "judgment-definitions-1" });
  const judgmentProviders = await sdk.judgment.listProviders({}, { id: "judgment-providers-1" });
  const lenses = await sdk.lenses.list({}, { id: "lenses-1" });
  const capabilities = await sdk.capabilities.list({}, { id: "capabilities-list-1" });
  const published = await sdk.object.publish(
    {
      author_id: "id_alice",
      draft: {
        ...textDraft("generic Object draft"),
        kind: "babel.canvas",
        schema: "example.canvas.v1",
        payload: { title: "Collaborative canvas", layers: [] },
        state: { revision: 1 },
      },
    },
    { id: "publish-object-1", idempotencyKey: "publish-object-key" },
  );
  const fork = await sdk.object.fork(
    {
      author_id: "id_alice",
      source_object_id: "obj_source",
      draft: textDraft("forked source"),
    },
    { id: "fork-1", idempotencyKey: "fork-key" },
  );
  const remix = await sdk.object.remix(
    {
      author_id: "id_alice",
      source_object_ids: ["obj_source", "obj_other"],
      draft: textDraft("remixed source"),
    },
    { id: "remix-1", idempotencyKey: "remix-key" },
  );
  const inferredRelationship = await sdk.graph.inferRelationship(
    {
      author_id: "id_alice",
      source: "obj_evidence",
      target: "obj_bound",
      relation: "supports",
      min_score: 0.2,
    },
    { id: "graph-relationship-1", idempotencyKey: "graph-relationship-key" },
  );
  const traversal = await sdk.graph.traverse(
    {
      object_id: "obj_bound",
      direction: "incoming",
      relations: ["evidence_for"],
      max_depth: 2,
      limit: 16,
    },
    { id: "graph-traverse-1" },
  );
  const evidence = await sdk.graph.evidence({ object_id: "obj_bound" }, { id: "graph-evidence-1" });
  const room = await sdk.realtime.defineRoom(
    {
      author_id: "id_alice",
      object_id: "obj_bound",
      name: "main",
      schema: "babel.realtime.chat.v1",
      membership: "open",
      persistence: "ephemeral",
      limits: null,
    },
    { id: "room-1", idempotencyKey: "room-key" },
  );

  assert.equal(identity.identity.id, "id_alice");
  assert.deepEqual(search.results, []);
  assert.equal(judgmentDefinitions.definitions[0].id, "babel.judgment.relationship.v1");
  assert.equal(judgmentProviders.providers[0].provider.provider, "babel-local");
  assert.equal(lenses.lenses[0].id, "babel.lens.research.v1");
  assert.equal(capabilities.capabilities[0].id, "babel.network.fetch");
  assert.equal(published.object.kind, "babel.canvas");
  assert.equal(fork.event.kind, "object_forked");
  assert.equal(remix.event.kind, "object_remixed");
  assert.equal(traversal.traversal.steps[0].next_object, "obj_evidence");
  assert.equal(inferredRelationship.edge.origin, "JudgmentDerived");
  assert.equal(inferredRelationship.edge.metadata.judgment_id, "jud_relationship");
  assert.equal(evidence.projection.summary.judgment_support, 1);
  assert.equal(evidence.projection.supporting[0].relationship_judgment.id, "jud_relationship");
  assert.equal(room.room.id, "room_main");
  assert.deepEqual(
    transport.envelopes.map((envelope) => envelope.method),
    [
      "babel.identity.create.v1",
      "babel.search.objects.v1",
      "babel.judgment.definitions.list.v1",
      "babel.judgment.providers.list.v1",
      "babel.lenses.list.v1",
      "babel.capabilities.list.v1",
      "babel.object.publish.v1",
      "babel.object.fork.v1",
      "babel.object.remix.v1",
      "babel.graph.relationship.infer.v1",
      "babel.graph.traverse.v1",
      "babel.graph.evidence.v1",
      "babel.realtime.room.define.v1",
    ],
  );
  assert.deepEqual(
    transport.envelopes.map((envelope) => envelope.idempotency_key ?? null),
    [
      "identity-key",
      null,
      null,
      null,
      null,
      null,
      "publish-object-key",
      "fork-key",
      "remix-key",
      "graph-relationship-key",
      null,
      null,
      "room-key",
    ],
  );
});

test("Surface lifecycle helper enforces protocol lifecycle transitions", () => {
  const lifecycle = createSurfaceLifecycle("cold");
  const events = [];
  lifecycle.onChange((event) => events.push(event));

  lifecycle.transition("prefetched", "feed prefetch");
  lifecycle.transition("warm", "budget granted");
  lifecycle.transition("active", "visible");
  lifecycle.transition("suspended", "offscreen");

  assert.equal(lifecycle.state, "suspended");
  assert.equal(lifecycle.signal.aborted, false);
  assert.deepEqual(
    events.map((event) => [event.previous, event.current, event.reason]),
    [
      ["cold", "prefetched", "feed prefetch"],
      ["prefetched", "warm", "budget granted"],
      ["warm", "active", "visible"],
      ["active", "suspended", "offscreen"],
    ],
  );
  assert.throws(() => lifecycle.transition("prefetched"), /invalid Babel Surface lifecycle transition/);

  lifecycle.transition("evicted", "host reclaimed resources");
  assert.equal(lifecycle.signal.aborted, true);
});

test("SDK runtime namespace controls host-bound Surface sessions", async () => {
  const session = surfaceSessionFixture("prefetched", surfaceBudgetFixture());
  const lowered = {
    ...surfaceBudgetFixture(),
    memory_bytes: surfaceBudgetFixture().memory_bytes / 2,
  };
  const decision = {
    lifecycle: "active",
    budget: surfaceBudgetFixture(),
    should_serialize_state: false,
    release_gpu_resources: false,
    zero_cpu_required: false,
    reason: "surface is visible and likely to be used",
  };
  const transport = new QueueTransport([
    {
      protocol: "babel.rpc.v1",
      id: "runtime-health",
      result: {
        health: {
          at: "2026-09-27T00:00:01Z",
          session_count: 1,
          lifecycle_counts: {
            cold: 0,
            prefetched: 1,
            warm: 0,
            active: 0,
            suspended: 0,
            evicted: 0,
          },
          totals: {
            ...surfaceBudgetFixture(),
            gpu_expected_sessions: 0,
            background_eligible_sessions: 0,
            zero_cpu_sessions: 0,
          },
          sessions: [
            {
              session_id: "surf_0000000000000000000000000000000000000000000000000000000000000000",
              object_id: "obj_surface",
              role: "Feed",
              target: "Web",
              lifecycle: "prefetched",
              admission: "ready",
              budget: surfaceBudgetFixture(),
              event_count: 1,
              last_event_reason: "Surface session admitted by host runtime",
              granted_capability_count: 0,
              blocked_reason_count: 0,
              zero_cpu_required: false,
              updated_at: "2026-09-27T00:00:01Z",
            },
          ],
        },
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "start-surface",
      result: { session },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "activate-surface",
      result: {
        session: surfaceSessionFixture("warm", surfaceBudgetFixture()),
        event: surfaceRuntimeEventFixture("lifecycle_transition", "warm", surfaceBudgetFixture()),
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "budget-surface",
      result: {
        session: surfaceSessionFixture("warm", lowered),
        event: surfaceRuntimeEventFixture("budget_changed", "warm", lowered, surfaceBudgetFixture()),
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "schedule-surface",
      result: { decision },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "apply-schedule-surface",
      result: {
        session: surfaceSessionFixture("active", surfaceBudgetFixture()),
        decision,
        events: [surfaceRuntimeEventFixture("lifecycle_transition", "active", surfaceBudgetFixture())],
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "checkpoint-surface-state",
      result: {
        session: surfaceSessionFixture("active", surfaceBudgetFixture()),
        checkpoint: surfaceStateCheckpointFixture(),
        event: surfaceRuntimeEventFixture("state_checkpointed", "active", surfaceBudgetFixture()),
      },
      error: null,
      trace_id: null,
    },
    {
      protocol: "babel.rpc.v1",
      id: "get-surface-state",
      result: {
        checkpoint: surfaceStateCheckpointFixture(),
      },
      error: null,
      trace_id: null,
    },
  ]);
  const sdk = createBabelSDK({
    transport,
    binding: hostSurfaceBinding({
      runtimeId: "runtime",
      origin: "https://host.test",
      surfaceSessionId: "surf_0000000000000000000000000000000000000000000000000000000000000000",
    }),
  });

  const health = await sdk.runtime.surfaceHealth(undefined, { id: "runtime-health" });
  const started = await sdk.runtime.startSurfaceSession(
    { object_id: "obj_surface", role: "Feed", session_id: null },
    { id: "start-surface" },
  );
  const activated = await sdk.runtime.transitionSurfaceSession(
    { lifecycle: "warm", reason: "viewport nearing" },
    { id: "activate-surface" },
  );
  const budgeted = await sdk.runtime.changeSurfaceBudget(
    { budget: lowered, reason: "thermal pressure" },
    { id: "budget-surface" },
  );
  const scheduled = await sdk.runtime.scheduleSurfaceSession(
    {
      input: {
        viewport_distance_px: 40,
        approaching_viewport: true,
        interaction_score: 900,
        memory_pressure: "normal",
        gpu_pressure: "normal",
        battery_saver: false,
        metered_network: false,
        device_class: "desktop",
      },
    },
    { id: "schedule-surface" },
  );
  const applied = await sdk.runtime.applySurfaceSchedule(
    {
      input: {
        viewport_distance_px: 40,
        approaching_viewport: true,
        interaction_score: 900,
        memory_pressure: "normal",
        gpu_pressure: "normal",
        battery_saver: false,
        metered_network: false,
        device_class: "desktop",
      },
    },
    { id: "apply-schedule-surface" },
  );
  const checkpointed = await sdk.runtime.checkpointSurfaceState(
    {
      state: { scroll: 144, active_panel: "conversation" },
      reason: "serialize before backgrounding",
    },
    { id: "checkpoint-surface-state" },
  );
  const restored = await sdk.runtime.getSurfaceState(
    { session_id: "surf_0000000000000000000000000000000000000000000000000000000000000000" },
    { id: "get-surface-state" },
  );

  assert.equal(health.health.session_count, 1);
  assert.equal(health.health.lifecycle_counts.prefetched, 1);
  assert.equal(started.session.id, "surf_0000000000000000000000000000000000000000000000000000000000000000");
  assert.equal(activated.event.kind, "lifecycle_transition");
  assert.equal(budgeted.event.kind, "budget_changed");
  assert.equal(scheduled.decision.lifecycle, "active");
  assert.equal(applied.session.lifecycle, "active");
  assert.equal(applied.events[0].kind, "lifecycle_transition");
  assert.equal(checkpointed.event.kind, "state_checkpointed");
  assert.equal(checkpointed.checkpoint.state.scroll, 144);
  assert.equal(restored.checkpoint.state_hash, checkpointed.checkpoint.state_hash);
  assert.deepEqual(
    transport.envelopes.map((envelope) => [envelope.method, envelope.binding.object_id, envelope.binding.surface_session_id]),
    [
      ["babel.runtime.surface.health.v1", null, "surf_0000000000000000000000000000000000000000000000000000000000000000"],
      ["babel.runtime.surface.session.start.v1", null, "surf_0000000000000000000000000000000000000000000000000000000000000000"],
      ["babel.runtime.surface.session.transition.v1", null, "surf_0000000000000000000000000000000000000000000000000000000000000000"],
      ["babel.runtime.surface.session.budget.v1", null, "surf_0000000000000000000000000000000000000000000000000000000000000000"],
      ["babel.runtime.surface.session.schedule.v1", null, "surf_0000000000000000000000000000000000000000000000000000000000000000"],
      ["babel.runtime.surface.session.apply_schedule.v1", null, "surf_0000000000000000000000000000000000000000000000000000000000000000"],
      ["babel.runtime.surface.session.state.checkpoint.v1", null, "surf_0000000000000000000000000000000000000000000000000000000000000000"],
      ["babel.runtime.surface.session.state.get.v1", null, "surf_0000000000000000000000000000000000000000000000000000000000000000"],
    ],
  );
});

test("Surface lifecycle helper applies host session and budget events", () => {
  const initial = surfaceBudgetFixture();
  const lowered = { ...initial, memory_bytes: initial.memory_bytes / 2 };
  const lifecycle = createSurfaceLifecycle("prefetched", initial);
  const lifecycleEvents = [];
  const budgetEvents = [];
  lifecycle.onChange((event) => lifecycleEvents.push(event));
  lifecycle.onBudgetChange((event) => budgetEvents.push(event));

  lifecycle.applyRuntimeEvent(surfaceRuntimeEventFixture("lifecycle_transition", "warm", initial));
  lifecycle.applyRuntimeEvent(surfaceRuntimeEventFixture("budget_changed", "warm", lowered, initial));

  assert.equal(lifecycle.state, "warm");
  assert.equal(lifecycle.budget.memory_bytes, lowered.memory_bytes);
  assert.deepEqual(lifecycleEvents.map((event) => [event.previous, event.current]), [["prefetched", "warm"]]);
  assert.equal(budgetEvents.length, 1);
  assert.equal(budgetEvents[0].previous.memory_bytes, initial.memory_bytes);
  assert.equal(budgetEvents[0].current.memory_bytes, lowered.memory_bytes);
});

class CaptureTransport {
  envelopes = [];

  constructor(response) {
    this.response = response;
  }

  async request(envelope) {
    this.envelopes.push(envelope);
    return this.response;
  }

  close() {}
}

class QueueTransport {
  envelopes = [];

  constructor(responses) {
    this.responses = responses;
  }

  async request(envelope) {
    this.envelopes.push(envelope);
    const response = this.responses.shift();
    assert.ok(response, `missing queued response for ${envelope.method}`);
    return response;
  }

  close() {}
}

function eventFixture(id) {
  return {
    id,
    actor: "id_alice",
    kind: "realtime_room_defined",
    target: "Network",
    payload: {},
    parents: [],
    created_at: "2026-09-27T00:00:00Z",
    signature: null,
  };
}

function roomFixture(id) {
  return {
    id,
    object_id: "obj_bound",
    name: "main",
    schema: "babel.realtime.chat.v1",
    membership: "open",
    persistence: "ephemeral",
    limits: {
      max_members: 32,
      max_messages_per_session: 1000,
      max_payload_bytes: 4096,
    },
  };
}

function textDraft(text) {
  return {
    kind: "babel.text",
    schema: "babel.schema.text.v1",
    payload: { text, metadata: {} },
    surfaces: [],
    resources: [],
    capabilities: [],
    state: null,
    provenance: {
      parent: null,
      forked_from: null,
      remixed_from: [],
    },
  };
}

function capabilityReceiptFixture(capability, scope = { namespace: "self" }) {
  return {
    capability,
    version: 1,
    grant_id: "grant_storage",
    scope,
    quota: {
      calls_per_minute: 120,
      bytes_per_minute: 131072,
      persistent_bytes: 10485760,
      realtime_connections: 0,
      max_call_ms: 1000,
      background_allowed: false,
    },
    remaining_calls_per_minute: 119,
    remaining_bytes_per_minute: 131000,
    remaining_realtime_connections: 0,
  };
}

function judgmentFixture(id, definition) {
  return {
    id,
    definition,
    input_hash: "hash_judgment_input",
    provider: {
      provider: "babel-local",
      model: "local-rules",
      version: "1",
    },
    output: {
      score: 0.82,
      reasons: ["fixture"],
    },
    confidence: 0.82,
    created_at: "2026-09-27T00:00:00Z",
  };
}

function textObjectFixture(id, text) {
  return {
    protocol: { version: "babel.v2" },
    id,
    author: "id_alice",
    created_at: "2026-09-27T00:00:00Z",
    kind: "babel.text",
    schema: "babel.schema.text.v1",
    payload: { text, metadata: {} },
    surfaces: [],
    resources: [],
    capabilities: [],
    relations: [],
    state: null,
    provenance: {
      parent: null,
      forked_from: null,
      remixed_from: [],
    },
    signature: null,
  };
}

function edgeFixture(id, source, target, relation, origin = "human_assertion", metadata = {}) {
  return {
    id,
    source,
    target,
    relation,
    origin,
    author: "id_alice",
    created_at: "2026-09-27T00:00:00Z",
    metadata,
    signature: null,
  };
}

function identityFixture(id, handle) {
  return {
    id,
    kind: "Person",
    handle,
    created_at: "2026-09-27T00:00:00Z",
    public_key: { algorithm: "Ed25519", bytes: "pub" },
    signature: { algorithm: "Ed25519", bytes: "sig" },
  };
}

function realtimeSessionFixture(state) {
  return {
    id: "session_1",
    room_id: "room_1",
    object_id: "obj_bound",
    participant: "id_alice",
    created_at: "2026-09-27T00:00:00Z",
    state,
    sent_messages: 1,
  };
}

function provenancePublicationFixture(suffix) {
  const forked = suffix === "forked";
  return {
    object: {
      protocol: { version: "babel.v2" },
      id: `obj_${suffix}`,
      author: "id_alice",
      created_at: "2026-09-27T00:00:00Z",
      kind: "babel.text",
      schema: "babel.schema.text.v1",
      payload: { text: suffix, metadata: {} },
      surfaces: [],
      resources: [],
      capabilities: [],
      relations: [],
      state: null,
      provenance: {
        parent: forked ? "obj_source" : null,
        forked_from: forked ? "obj_source" : null,
        remixed_from: forked ? [] : ["obj_source", "obj_other"],
      },
      signature: null,
    },
    edges: [],
    event: {
      id: `event_${suffix}`,
      actor: "id_alice",
      kind: forked ? "object_forked" : "object_remixed",
      target: { Object: `obj_${suffix}` },
      payload: {},
      parents: [],
      created_at: "2026-09-27T00:00:00Z",
      signature: null,
    },
  };
}

function surfaceBudgetFixture() {
  return {
    memory_bytes: 33554432,
    cpu_ms_per_minute: 1500,
    gpu_expected: false,
    network_bytes_per_minute: 524288,
    persistent_storage_bytes: 1048576,
    realtime_connections: 1,
    background_eligible: false,
  };
}

function surfaceSessionFixture(lifecycle, budget) {
  return {
    id: "surf_0000000000000000000000000000000000000000000000000000000000000000",
    plan: {
      object_id: "obj_surface",
      surface: {
        role: "Feed",
        target: "Web",
        entry: "https://object.test/surface.html",
        integrity: "hash_surface",
      },
      lifecycle: "cold",
      admission: "ready",
      budget: surfaceBudgetFixture(),
      sandbox: {
        isolated_origin: true,
        csp: "default-src 'none'",
        host_cookies: false,
        top_navigation: false,
        wasi_filesystem: false,
        wasi_network: false,
        capability_bridge: true,
      },
      capability_decisions: [],
      blocked_reasons: [],
    },
    lifecycle,
    budget,
    created_at: "2026-09-27T00:00:00Z",
    updated_at: "2026-09-27T00:00:01Z",
    events: [],
  };
}

function surfaceRuntimeEventFixture(kind, lifecycle, budget, previousBudget = null) {
  return {
    sequence: 1,
    session_id: "surf_0000000000000000000000000000000000000000000000000000000000000000",
    object_id: "obj_surface",
    kind,
    previous_lifecycle: kind === "lifecycle_transition" ? "prefetched" : null,
    lifecycle,
    previous_budget: previousBudget,
    budget,
    reason: kind === "budget_changed" ? "thermal pressure" : "viewport nearing",
    at: "2026-09-27T00:00:01Z",
  };
}

function surfaceStateCheckpointFixture() {
  return {
    session_id: "surf_0000000000000000000000000000000000000000000000000000000000000000",
    object_id: "obj_surface",
    lifecycle: "active",
    reason: "serialize before backgrounding",
    state: { scroll: 144, active_panel: "conversation" },
    state_hash: "hash_surface_state",
    size_bytes: 44,
    created_at: "2026-09-27T00:00:02Z",
  };
}

function encryptedEnvelopeFixture() {
  return {
    version: "babel.personalization.sync.v1",
    data_class: "encrypted_synchronized_state",
    algorithm: "XChaCha20-Poly1305",
    recipient: {
      identity_id: "id_0000000000000000000000000000000000000000000000000000000000000000",
      device_id: "desktop-main",
    },
    model_revision: "sync-rev-1",
    exported_at: "2026-09-27T00:00:00Z",
    nonce: "00".repeat(24),
    ciphertext: "ab".repeat(64),
  };
}

function envelopeSummaryFixture() {
  return {
    envelope_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    identity_id: "id_0000000000000000000000000000000000000000000000000000000000000000",
    device_id: "desktop-main",
    uploaded_at: "2026-09-27T00:00:01Z",
    size_bytes: 256,
  };
}
