import assert from "node:assert/strict";
import { createServer } from "node:http";

export async function verifyBridgeCancellation(execute, waitFor) {
  let arrived, disconnected;
  const started = new Promise(resolve => { arrived = resolve; });
  const closed = new Promise(resolve => { disconnected = resolve; });
  const held = new Set();
  const server = createServer((_request, response) => {
    held.add(response);
    response.setHeader("Access-Control-Allow-Origin", "*");
    response.once("close", () => { held.delete(response); disconnected(); });
    arrived();
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  const url = `http://127.0.0.1:${server.address().port}/pending`;
  const moduleUrl = `/@fs${new URL("../sdk/src/bridge.ts", import.meta.url).pathname}`;
  const evaluate = async code => {
    const response = await execute([{ type: "eval", code }]);
    assert.equal(response.results?.[0]?.ok, true, JSON.stringify(response));
    return response.results[0].value;
  };
  try {
    await evaluate(`(() => {
      const state = window.__bridgeCancellation = { ready: false, firstSettled: false, aborted: false, error: null, second: null };
      import(${JSON.stringify(moduleUrl)}).then(({ BrowserBridgeHost, BrowserBridgeTransport, rpcBridgeResponse }) => {
        const channel = new MessageChannel();
        const endpoint = port => ({
          postMessage: message => port.postMessage(message),
          addEventListener: (type, handler) => port.addEventListener(type, handler),
          removeEventListener: (type, handler) => port.removeEventListener(type, handler),
        });
        const response = (id, marker) => ({ protocol: 'babel.rpc.v1', id, result: { marker }, error: null, trace_id: null });
        state.host = new BrowserBridgeHost(endpoint(channel.port2), (request, { signal }) => {
          if (request.payload.q === 'first') {
            state.firstWireId = request.id;
            signal.addEventListener('abort', () => { state.aborted = true; });
            return fetch(${JSON.stringify(url)}, { signal }).then(result => result.text()).then(() => response(request.id, 'first'));
          }
          state.secondWireId = request.id;
          return new Promise(resolve => { state.release = () => resolve(response(request.id, 'second')); });
        });
        state.transport = new BrowserBridgeTransport(endpoint(channel.port1));
        channel.port1.start(); channel.port2.start();
        state.ports = [channel.port1, channel.port2];
        state.controller = new AbortController();
        const request = q => ({ protocol: 'babel.rpc.v1', id: 'reused', method: 'babel.search.objects.v1',
          binding: { object_id: null, surface_session_id: null, runtime_id: 'cancellation-test', origin: location.origin,
            capability_grants: [], identity_id: null },
          payload: { q, author: null, kind: null, limit: 1 }, idempotency_key: null,
          deadline: { timeout_ms: 30000, client_started_at: null }, trace_id: null });
        state.transport.request(request('first'), { signal: state.controller.signal })
          .then(() => { state.error = 'cancelled fetch unexpectedly completed'; }, error => { state.firstError = String(error); })
          .finally(() => { state.firstSettled = true; });
        state.retry = () => {
          state.transport.request(request('second')).then(result => { state.second = result; }, error => { state.error = String(error); });
          channel.port2.postMessage(rpcBridgeResponse(response(state.firstWireId, 'stale')));
        };
        state.ready = true;
      }).catch(error => { state.error = String(error); });
      return { started: true };
    })()`);
    const ready = await waitFor('({ ready: window.__bridgeCancellation.ready, error: window.__bridgeCancellation.error })', value => value.ready || value.error);
    assert.equal(ready.error, null);
    await within(started, "real host fetch did not reach the HTTP server");
    await evaluate("window.__bridgeCancellation.controller.abort(new Error('user dismissed')); ({ cancelled: true })");
    await within(closed, "caller cancellation did not disconnect the host HTTP request");
    const cancelled = await waitFor(`({ aborted: window.__bridgeCancellation.aborted,
      settled: window.__bridgeCancellation.firstSettled, error: window.__bridgeCancellation.error,
      firstError: window.__bridgeCancellation.firstError })`, value => value.aborted && value.settled);
    assert.equal(cancelled.error, null);
    assert.match(cancelled.firstError, /user dismissed/);
    await evaluate("window.__bridgeCancellation.retry(); ({ retried: true })");
    await waitFor("({ dispatched: !!window.__bridgeCancellation.release })", value => value.dispatched);
    await evaluate("window.__bridgeCancellation.release(); ({ released: true })");
    const result = await waitFor(`({ result: window.__bridgeCancellation.second, error: window.__bridgeCancellation.error,
      firstWireId: window.__bridgeCancellation.firstWireId, secondWireId: window.__bridgeCancellation.secondWireId })`, value => value.result || value.error);
    assert.equal(result.error, null);
    assert.notEqual(result.firstWireId, result.secondWireId);
    assert.equal(result.result.id, "reused");
    assert.deepEqual(result.result.result, { marker: "second" });
    console.log("Bridge cancellation PASS: native MessageChannel abort disconnects real HTTP work; reused caller ID rejects stale response");
  } finally {
    try {
      await evaluate(`(() => {
        const state = window.__bridgeCancellation;
        state?.transport?.close(); state?.host?.close(); state?.ports?.forEach(port => port.close());
        delete window.__bridgeCancellation; return { cleaned: true };
      })()`);
    } finally {
      for (const response of held) response.destroy();
      await new Promise(resolve => server.close(resolve));
    }
  }
}

async function within(promise, message) {
  let timer;
  try {
    await Promise.race([promise, new Promise((_, reject) => { timer = setTimeout(() => reject(new Error(message)), 15_000); })]);
  } finally { clearTimeout(timer); }
}
