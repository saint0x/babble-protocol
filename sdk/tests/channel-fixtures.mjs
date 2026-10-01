import { BrowserSurfaceHost, objectBinding, surfaceBridgeControl } from "../dist/index.js";

export const control = surfaceBridgeControl;
export const response = (request) => ({ protocol: "babble.rpc.v1", id: request.id, result: { ok: true }, error: null, trace_id: null });
export const request = (id = "request") => ({
  protocol: "babble.rpc.v1", id, method: "babble.search.objects.v1",
  binding: objectBinding({ objectId: "spoofed", surfaceSessionId: "spoofed", runtimeId: "test", origin: "https://evil.test", capabilityGrants: ["spoofed"] }),
  deadline: { timeout_ms: 30000, client_started_at: null }, payload: { q: "test" }, idempotency_key: null, trace_id: null,
});
export const rpc = (envelope) => ({ type: "babble.rpc.request", protocol: "babble.rpc.v1", envelope });
export const plan = () => ({
  object_id: "object", surface: { role: "Feed", target: "Web", entry: "https://object.test/main.html", integrity: "hash" },
  lifecycle: "cold", admission: "ready", budget: { memory_bytes: 10000, cpu_ms_per_minute: 100, network_bytes_per_minute: 1000, persistent_storage_bytes: 0, realtime_connections: 0, gpu_expected: false, background_eligible: false },
  sandbox: { isolated_origin: true, capability_bridge: true, csp: "default-src 'none'", host_cookies: false, top_navigation: false },
  capability_decisions: [], blocked_reasons: [],
});

export class FakePort {
  listeners = new Map([['message', new Set()], ['messageerror', new Set()], ['close', new Set()]]);
  messages = [];
  closed = 0;
  started = 0;
  postMessage(data) { this.messages.push(data); }
  start() { this.started++; }
  close() { this.closed++; }
  addEventListener(type, handler) { this.listeners.get(type).add(handler); }
  removeEventListener(type, handler) { this.listeners.get(type).delete(handler); }
  emit(data, type = "message") { for (const handler of this.listeners.get(type)) handler({ data }); }
  get listenerCount() { return [...this.listeners.values()].reduce((total, handlers) => total + handlers.size, 0); }
}

export function harness(t, options = {}) {
  const listeners = new Set();
  const windowMessages = [];
  const frame = Object.assign(new EventTarget(), {
    sandbox: { add() {} }, setAttribute() {}, removed: 0,
    remove() { this.removed++; },
    contentWindow: { postMessage(...args) { windowMessages.push(args); } },
  });
  const window = {
    location: { origin: "https://host.test" },
    addEventListener(type, handler) { listeners.add(handler); },
    removeEventListener(type, handler) { listeners.delete(handler); },
    emit(event) { for (const handler of listeners) handler(event); },
  };
  const calls = [];
  const mounted = new BrowserSurfaceHost().mount({
    container: { appendChild(frame) { return frame; } }, document: { createElement() { return frame; } },
    window, plan: plan(), surfaceSessionId: "session", currentIdentityId: "identity",
    registerDocument: async () => {},
    dispatch(envelope) { calls.push(envelope); return response(envelope); }, ...options,
  });
  t.after(() => mounted.unmount());
  const offer = (ports, data = control("connect"), extra = {}) => window.emit({ data, ports, source: frame.contentWindow, origin: "null", ...extra });
  const pagehide = new Set();
  const offers = [];
  const child = {
    parent: {
      postMessage(data, origin, ports) {
        offers.push({ data, origin });
        // Transfer ownership for real, as Window.postMessage does in the browser.
        const transferred = structuredClone({ data, ports }, { transfer: ports });
        offer(transferred.ports, transferred.data);
      },
    },
    addEventListener(type, handler) { pagehide.add(handler); },
    removeEventListener(type, handler) { pagehide.delete(handler); },
  };
  return { mounted, frame, window, listeners, windowMessages, calls, offer, child, pagehide, offers };
}

export function fakeChild(channel, send = () => {}) {
  const pagehide = new Set();
  return {
    window: {
      parent: { postMessage: send },
      addEventListener(type, handler) { pagehide.add(handler); },
      removeEventListener(type, handler) { pagehide.delete(handler); },
    },
    createChannel: () => channel,
    parentOrigin: "https://host.test", pagehide,
  };
}

export function trackedTimers(t) {
  t.mock.timers.enable({ apis: ["setTimeout", "Date"], now: 0 });
  // Handshake timer delivery and monotonic deadline checks share virtual time.
  t.mock.method(globalThis.performance, "now", () => Date.now());
  const timers = new Set();
  const set = globalThis.setTimeout;
  const clear = globalThis.clearTimeout;
  t.mock.method(globalThis, "setTimeout", (callback, delay, ...args) => {
    const timer = set(() => { timers.delete(timer); callback(...args); }, delay);
    timers.add(timer);
    return timer;
  });
  t.mock.method(globalThis, "clearTimeout", (timer) => { timers.delete(timer); clear(timer); });
  return timers;
}
