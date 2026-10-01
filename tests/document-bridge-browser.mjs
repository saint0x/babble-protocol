import assert from "node:assert/strict";
import { createServer } from "node:http";

// These documents are adversarial transport fixtures, not production Surfaces.
// The live-stack suite separately checks the signed, content-addressed client.
// Test-only observers and acknowledgements are trusted instrumentation: they
// observe the selected native send path, not arbitrary future tasks or remote
// resource integrity. Negative controls deliberately corrupt only this fixture.
export async function verifyDocumentBridge(execute, waitFor, originalPlan) {
  await verifyCase(execute, waitFor, originalPlan, "secure");
  for (const [mode, rule] of [
    ["leaky-window", "old-reply-confidentiality"],
    ["duplicate-admission", "replacement-dispatch"],
  ]) {
    await assert.rejects(() => verifyCase(execute, waitFor, originalPlan, mode),
      error => error.code === "ERR_ASSERTION" && error.securityRule === rule,
      `${mode} must fail the required security assertion`);
    console.log(`Document-bound bridge negative control PASS: ${mode} rejected by ${rule}`);
  }
}

async function verifyCase(execute, waitFor, originalPlan, mode) {
  const held = new Set();
  let markHoldArrived;
  const holdArrived = new Promise(resolve => { markHoldArrived = resolve; });
  const server = createServer((request, response) => {
    response.setHeader("Allow-CSP-From", "*");
    response.setHeader("Cache-Control", "no-store");
    const path = new URL(request.url, "http://fixture.test").pathname;
    if (path === "/hold.js") {
      held.add(response);
      markHoldArrived();
      response.on("close", () => held.delete(response));
      return;
    }
    if (path === "/initial" || path === "/replacement") {
      response.setHeader("Content-Type", "text/html");
      response.end(`<!doctype html><html><head><meta charset="utf-8"><title>Document bridge regression</title></head><body><script src="${path}.js"></script>${path === "/replacement" ? '<script src="/hold.js"></script>' : ''}</body></html>`);
      return;
    }
    response.setHeader("Content-Type", "text/javascript");
    if (path === "/initial.js") response.end(initialDocument());
    else if (path === "/replacement.js") response.end(replacementDocument());
    else { response.statusCode = 404; response.end(); }
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const origin = `http://127.0.0.1:${server.address().port}`;
  const plan = {
    ...originalPlan,
    surface: { ...originalPlan.surface, entry: `${origin}/initial`, integrity: null },
    sandbox: { ...originalPlan.sandbox, isolated_origin: true,
      csp: `default-src 'none'; script-src ${origin}; object-src 'none'; base-uri 'none'; form-action 'none'` },
  };
  try {
    await execute([{ type: "eval", code: `(() => {
      const state = window.__documentBridgeTest = {
        calls: [], observations: [], loads: 0, ready: false, offers: 0, offerOrigins: [],
        acknowledgements: {}, duplicateSends: [], ports: [], restore: [],
      };
      const mode = ${JSON.stringify(mode)};
      const control = type => ({ type: 'babble.surface.' + type, protocol: 'babble.rpc.v1', version: 1 });
      import('/src/app/protocol.ts').then(({ mountSurface }) => {
        const container = document.createElement('div');
        container.style.cssText = 'position:fixed;top:0;left:0;width:400px;height:240px;z-index:9999;background:white';
        document.body.append(container);
        state.container = container;
        const barrier = id => state.mounted.frame.contentWindow.postMessage({ type: 'babble.test.barrier', id }, '*');
        // Installed before mountSurface registers its admission listener. Native
        // sends/closes complete before checkpoints are exposed to the test.
        state.observe = event => {
          if (event.source !== state.mounted?.frame.contentWindow) return;
          if (event.data?.type === 'babble.test.observation') {
            state.observations.push(event.data.value);
          } else if (event.data?.type === 'babble.test.ack') {
            state.acknowledgements[event.data.id] = event.data;
          } else if (event.data?.type === 'babble.surface.connect' && event.ports.length === 1) {
            const port = event.ports[0];
            state.offerOrigins.push(event.origin);
            const duplicate = ++state.offers > 1;
            const send = port.postMessage;
            const close = port.close;
            state.ports.push(port);
            state.restore.push(() => { port.postMessage = send; port.close = close; });
            port.postMessage = function(message, ...args) {
              send.call(this, message, ...args);
              if (duplicate && message?.type?.startsWith('babble.surface.')) state.duplicateSends.push(message.type);
              if (message?.type === 'babble.rpc.response' && message.response?.id === 'old-pending') {
                state.oldReplySent = true;
                if (mode === 'leaky-window') state.mounted.frame.contentWindow.postMessage(message, '*');
                // Successful native send is a checkpoint, not proof that the
                // old document received it. Any injected window leak is queued
                // before this barrier on the same parent-to-replacement path.
                barrier('old-reply');
              }
            };
            port.close = function() {
              close.call(this);
              if (duplicate) { state.duplicateClosed = true; barrier('duplicate'); }
            };
            if (duplicate && mode === 'duplicate-admission') {
              // Deliberately faulty second admission, over a real transferred
              // port. Exercise every handshake stage before dispatching RPC.
              event.stopImmediatePropagation();
              let confirmed = false;
              port.onmessage = message => {
                const data = message.data;
                if (!confirmed && data?.type === 'babble.surface.confirm'
                  && data.protocol === 'babble.rpc.v1' && data.version === 1 && Object.keys(data).length === 3) {
                  confirmed = true;
                  state.duplicateConfirmed = true;
                  port.postMessage(control('ready'));
                } else if (confirmed && data?.type === 'babble.rpc.request') {
                  state.calls.push({ id: data.envelope.id, binding: data.envelope.binding });
                  port.postMessage({ type: 'babble.rpc.response', protocol: 'babble.rpc.v1', response: {
                    protocol: 'babble.rpc.v1', id: data.envelope.id, result: { duplicate: true }, error: null, trace_id: null,
                  } });
                  // The port barrier follows ready and the RPC response on the
                  // same port, so its acknowledgement proves child processing.
                  port.postMessage({ type: 'babble.test.barrier', id: 'duplicate' });
                }
              };
              port.start();
              port.postMessage(control('accept'));
            }
          }
        };
        window.addEventListener('message', state.observe);
        state.mounted = mountSurface({ container, plan: ${JSON.stringify(plan)},
          surfaceSessionId: 'document-bridge-regression', currentIdentityId: 'test-principal',
          // This adversarial port fixture has no API session. Server document
          // registration is covered by the separate real-stack regression.
          registerDocument: async documentId => { state.documentId = documentId; },
          dispatch: request => {
            state.calls.push({ id: request.id, binding: request.binding });
            const response = result => ({ protocol: 'babble.rpc.v1', id: request.id, result, error: null, trace_id: null });
            if (request.id === 'old-pending') return new Promise(resolve => {
              state.release = () => { state.resolved = true; resolve(response({ secret: 'old-document-only' })); };
            });
            return response({ navigate: ${JSON.stringify(`${origin}/replacement`)} });
          }
        });
        state.mounted.frame.addEventListener('load', () => state.loads++);
        state.mounted.ready.then(() => { state.ready = true; }, error => { state.error = String(error); });
      }).catch(error => { state.error = String(error); });
      return { started: true };
    })()` }]);
    const started = await waitFor(`(() => {
      const s = window.__documentBridgeTest;
      return { ready: s.ready, error: s.error, replacement: s.observations.includes('replacement-started'),
        loads: s.loads, calls: s.calls };
    })()`, state => Boolean(state.error) || (state.ready && state.replacement));
    assert.equal(started.error, undefined, JSON.stringify(started));
    assert.equal(started.loads, 1, "replacement has executed but has not completed load");
    await bounded(holdArrived, "replacement hold request did not arrive");
    assert.ok(held.size > 0, "replacement load is held by the fixture server");
    for (const call of started.calls.filter(call => ["old-pending", "navigate"].includes(call.id))) {
      assert.equal(call.binding.object_id, originalPlan.object_id);
      assert.equal(call.binding.surface_session_id, "document-bridge-regression");
      assert.equal(call.binding.identity_id, "test-principal");
    }
    await execute([{ type: "eval", code: `(() => {
      const state = window.__documentBridgeTest;
      state.release();
      return { released: true };
    })()` }]);
    // Required order: hold received -> pending dispatch released -> native send
    // returned -> replacement acknowledges the window barrier. Separately await
    // duplicate close + window ack, or faulty admission + response + port ack.
    // Only after both chains finish may assertions run and the load be released.
    const duringNavigation = await waitFor(`(() => {
      const s = window.__documentBridgeTest;
      return { calls: s.calls.map(call => call.id), observations: s.observations,
        loads: s.loads, connected: s.mounted.frame.isConnected, resolved: s.resolved,
        oldReplySent: s.oldReplySent, acknowledgements: s.acknowledgements,
        offerOrigins: s.offerOrigins,
        duplicateSends: s.duplicateSends, duplicateClosed: s.duplicateClosed,
        duplicateConfirmed: s.duplicateConfirmed };
    })()`, state => state.oldReplySent && state.acknowledgements["old-reply"] && state.acknowledgements.duplicate);
    assert.equal(duringNavigation.loads, 1);
    assert.equal(duringNavigation.connected, true, "test targets the interval before the load-based eviction");
    assert.equal(duringNavigation.resolved, true);
    assert.equal(duringNavigation.oldReplySent, true);
    assert.deepEqual(duringNavigation.offerOrigins, ["null", "null"], "both documents must have opaque origins");
    const oldReplyAck = duringNavigation.acknowledgements["old-reply"];
    const duplicateAck = duringNavigation.acknowledgements.duplicate;
    assert.equal(oldReplyAck.path, "window");
    if (mode === "leaky-window") {
      assert.equal(oldReplyAck.windowReplies[0]?.response?.result?.secret, "old-document-only",
        "negative control must deliver the actual secret to the replacement");
    }
    if (mode === "duplicate-admission") {
      assert.equal(duringNavigation.duplicateConfirmed, true);
      assert.equal(duplicateAck.path, "port");
      assert.equal(duplicateAck.connected, true);
      assert.deepEqual(duplicateAck.portMessages, ["babble.surface.accept", "babble.surface.ready", "babble.rpc.response"]);
      assert.equal(duplicateAck.portReplies[0]?.response?.id, "replacement-port");
    } else {
      assert.equal(duringNavigation.duplicateClosed, true);
      assert.equal(duplicateAck.path, "window");
    }
    securityEqual(duringNavigation.calls, ["old-pending", "navigate"], "replacement-dispatch");
    securityEqual(oldReplyAck.windowReplies, [], "old-reply-confidentiality");
    securityEqual(duringNavigation.duplicateSends, [], "duplicate-admission");
    securityEqual(duplicateAck.portMessages, [], "replacement-port-delivery");

    for (const response of held) response.end("/* Load released by the regression. */");
    await waitFor(`(() => {
      const s = window.__documentBridgeTest;
      return { connected: s.mounted.frame.isConnected, lifecycle: s.mounted.lifecycle.state };
    })()`, state => !state.connected && state.lifecycle === "evicted");
    console.log("Document-bound bridge PASS: real opaque-origin navigation, acknowledged send barriers, pre-load RPC rejection, private pending reply, terminal load eviction");
  } finally {
    try {
      await execute([{ type: "eval", code: `(() => {
        const s = window.__documentBridgeTest;
        if (s) {
          for (const restore of s.restore) restore();
          s.mounted?.unmount('document bridge regression finished');
          s.release?.();
          for (const port of s.ports) { port.onmessage = null; port.close(); }
          window.removeEventListener('message', s.observe);
          s.container?.remove();
          delete window.__documentBridgeTest;
        }
        return { cleaned: true };
      })()` }]);
    } finally {
      for (const response of held) response.end();
      server.closeAllConnections();
      await new Promise(resolve => server.close(resolve));
    }
  }
}

function securityEqual(actual, expected, rule) {
  try { assert.deepEqual(actual, expected, `Surface security assertion: ${rule}`); }
  catch (error) { error.securityRule = rule; throw error; }
}

async function bounded(promise, message) {
  let timer;
  try {
    return await Promise.race([promise, new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(message)), 15_000);
    })]);
  } finally { clearTimeout(timer); }
}

function request(id) {
  return { type: "babble.rpc.request", protocol: "babble.rpc.v1", envelope: {
    protocol: "babble.rpc.v1", id, method: "babble.search.objects.v1",
    binding: { object_id: null, surface_session_id: null, runtime_id: "document-regression",
      origin: "https://spoof.invalid", identity_id: "spoofed-principal", capability_grants: [] },
    payload: { q: null, author: null, kind: null, limit: 1 },
    idempotency_key: null, deadline: { timeout_ms: 10000, client_started_at: null }, trace_id: null,
  } };
}

function initialDocument() {
  return `window.addEventListener('load', () => {
    const channel = new MessageChannel();
    channel.port1.onmessage = event => {
      const data = event.data;
      if (data?.type === 'babble.surface.accept') {
        channel.port1.postMessage({ type: 'babble.surface.confirm', protocol: 'babble.rpc.v1', version: 1 });
      } else if (data?.type === 'babble.surface.ready') {
        parent.postMessage({ type: 'babble.test.observation', value: 'initial-connected' }, '*');
        channel.port1.postMessage(${JSON.stringify(request("old-pending"))});
        channel.port1.postMessage(${JSON.stringify(request("navigate"))});
      } else if (data?.type === 'babble.rpc.response' && data.response?.result?.navigate) {
        location.replace(data.response.result.navigate);
      }
    };
    parent.postMessage({ type: 'babble.surface.connect', protocol: 'babble.rpc.v1', version: 1 }, '*', [channel.port2]);
  }, { once: true });`;
}

function replacementDocument() {
  return `const report = value => parent.postMessage({ type: 'babble.test.observation', value }, '*');
    const windowReplies = [], portMessages = [], portReplies = [];
    let accepted = false, connected = false;
    const acknowledge = (id, path) => parent.postMessage({
      type: 'babble.test.ack', id, path, connected, windowReplies, portMessages, portReplies,
    }, '*');
    window.addEventListener('message', event => {
      if (event.source !== parent) return;
      if (event.data?.type === 'babble.test.barrier') acknowledge(event.data.id, 'window');
      else if (event.data?.type?.startsWith('babble.')) windowReplies.push(event.data);
    });
    const channel = new MessageChannel();
    channel.port1.onmessage = event => {
      const data = event.data;
      if (data?.type === 'babble.test.barrier') { acknowledge(data.id, 'port'); return; }
      portMessages.push(data?.type);
      if (data?.protocol !== 'babble.rpc.v1') return;
      if (data.type === 'babble.surface.accept' && data.version === 1 && Object.keys(data).length === 3 && !accepted) {
        accepted = true;
        channel.port1.postMessage({ type: 'babble.surface.confirm', protocol: 'babble.rpc.v1', version: 1 });
      } else if (data.type === 'babble.surface.ready' && data.version === 1 && Object.keys(data).length === 3 && accepted && !connected) {
        connected = true;
        channel.port1.postMessage(${JSON.stringify(request("replacement-port"))});
      } else if (data.type === 'babble.rpc.response') portReplies.push(data);
    };
    parent.postMessage(${JSON.stringify(request("replacement-window"))}, '*');
    parent.postMessage({ type: 'babble.surface.connect', protocol: 'babble.rpc.v1', version: 1 }, '*', [channel.port2]);
    channel.port1.postMessage(${JSON.stringify(request("replacement-port-early"))});
    report('replacement-started');`;
}
