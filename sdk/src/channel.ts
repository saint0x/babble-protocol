import { BrowserBridgeHost, BrowserBridgeTransport, type BridgeDispatch, type BridgeEndpoint, type BridgeMessageEvent, type BridgeMessageHandler } from "./bridge.js";
import { rpcCatalog } from "./generated/protocol.js";
import type { SurfaceFrame, SurfaceHostWindow, SurfaceWindowMessageEvent } from "./host.js";
import type { BabbleTransport, RpcRequestOptions } from "./transport.js";
import type { RpcRequestEnvelope, RpcResponseEnvelope } from "./generated/protocol.js";

export const SURFACE_BRIDGE_VERSION = 1;
type Control = "connect" | "accept" | "confirm" | "ready" | "close";

/** Only the connect offer travels through Window.postMessage. */
export function surfaceBridgeControl(type: Control) {
  return { type: `babble.surface.${type}`, protocol: rpcCatalog.protocol, version: SURFACE_BRIDGE_VERSION };
}

function isControl(value: unknown, type: Control): boolean {
  if (typeof value !== "object" || value === null) return false;
  const message = value as Record<string, unknown>;
  return message.type === `babble.surface.${type}` && message.protocol === rpcCatalog.protocol
    && message.version === SURFACE_BRIDGE_VERSION && Object.keys(message).length === 3;
}

export interface SurfaceMessagePort {
  postMessage(message: unknown): void;
  addEventListener(type: "message" | "messageerror" | "close", handler: BridgeMessageHandler): void;
  removeEventListener(type: "message" | "messageerror" | "close", handler: BridgeMessageHandler): void;
  start(): void;
  close(): void;
}

export interface SurfaceConnectorWindow {
  readonly parent: { postMessage(message: unknown, targetOrigin: string, transfer: Transferable[]): void };
  addEventListener(type: "pagehide", handler: () => void): void;
  removeEventListener(type: "pagehide", handler: () => void): void;
}

export interface ConnectSurfaceBridgeOptions {
  /** Exact HTTP(S) origin of the embedding host. Wildcards are not accepted. */
  readonly parentOrigin: string;
  readonly timeoutMs?: number;
  /** Aborting also closes an already connected transport. */
  readonly signal?: AbortSignal;
  readonly window?: SurfaceConnectorWindow;
  readonly createChannel?: () => { readonly port1: SurfaceMessagePort; readonly port2: SurfaceMessagePort };
}

function handshakeTimeout(value = 10_000): number {
  if (!Number.isSafeInteger(value) || value <= 0 || value > 120_000) {
    throw new Error("Babble Surface handshake timeout must be an integer between 1 and 120000 ms");
  }
  return value;
}

/** Owns a port, including all listeners, and reports terminal failures once. */
class PortEndpoint implements BridgeEndpoint {
  #closed = false;
  #handlers = new Set<BridgeMessageHandler>();
  #message = (event: BridgeMessageEvent): void => {
    if (this.#closed) return;
    if (isControl(event.data, "close")) {
      this.close(new Error("Babble Surface peer closed the channel"), false);
      return;
    }
    if (this.control(event.data)) return;
    for (const handler of this.#handlers) {
      if (this.#closed) break;
      handler({ data: event.data });
    }
  };
  #error = (): void => this.close(new Error("Babble Surface channel messageerror"));
  #peerClosed = (): void => this.close(new Error("Babble Surface peer port closed"), false);

  constructor(readonly port: SurfaceMessagePort, readonly control: (value: unknown) => boolean, readonly ended: (error: Error) => void) {
    port.addEventListener("message", this.#message);
    port.addEventListener("messageerror", this.#error);
    port.addEventListener("close", this.#peerClosed);
  }

  start(): void { this.port.start(); }

  postMessage(message: unknown): void {
    if (this.#closed) throw new Error("Babble Surface channel is closed");
    try {
      this.port.postMessage(message);
    } catch (error) {
      const failure = asError(error, "Babble Surface channel send failed");
      this.close(failure);
      throw failure;
    }
  }

  addEventListener(_type: "message", handler: BridgeMessageHandler): void { this.#handlers.add(handler); }
  removeEventListener(_type: "message", handler: BridgeMessageHandler): void { this.#handlers.delete(handler); }

  close(error = new Error("Babble Surface channel closed"), notify = true): void {
    if (this.#closed) return;
    this.#closed = true;
    this.port.removeEventListener("message", this.#message);
    this.port.removeEventListener("messageerror", this.#error);
    this.port.removeEventListener("close", this.#peerClosed);
    this.#handlers.clear();
    try {
      if (notify) this.port.postMessage(surfaceBridgeControl("close"));
    } catch (failure) {
      error = asError(failure, "Babble Surface channel close notification failed");
    } finally {
      this.port.close();
      this.ended(error);
    }
  }
}

/**
 * Binds to the first admitted document's port, never to its navigation-stable
 * WindowProxy. This is not integrity verification of remote bytes: initial
 * redirects and intentional delegation of the port remain within that trust.
 */
export class SurfaceChannelHost {
  readonly ready: Promise<void>;
  #resolve!: () => void;
  #reject!: (error: Error) => void;
  #timer: ReturnType<typeof setTimeout>;
  #deadline: number;
  #closed = false;
  #admitted = false;
  #confirmed = false;
  #registration = new AbortController();
  #endpoint: PortEndpoint | undefined;
  #bridge: BrowserBridgeHost | undefined;
  #offer = (event: SurfaceWindowMessageEvent): void => {
    if (event.source === null || event.source !== this.frame.contentWindow) return;
    const ports = event.ports ?? [];
    if (this.#closed || this.#admitted || !this.origins.includes(event.origin)
      || !isControl(event.data, "connect") || ports.length !== 1) {
      for (const port of ports) port.close();
      return;
    }
    this.#admitted = true;
    const endpoint = new PortEndpoint(ports[0]!, (value) => {
      if (this.#closed) return true;
      if (this.#bridge) return false;
      if (!this.#confirmed && isControl(value, "confirm")) {
        this.#confirmed = true;
        void this.#register(endpoint);
      }
      // No RPC may dispatch until this document's registration succeeds.
      return true;
    }, (error) => this.close(error));
    this.#endpoint = endpoint;
    try {
      endpoint.start();
      endpoint.postMessage(surfaceBridgeControl("accept"));
    } catch (error) {
      this.close(asError(error, "Babble Surface handshake failed"));
    }
  };

  constructor(readonly window: SurfaceHostWindow, readonly frame: SurfaceFrame, readonly origins: readonly string[], readonly dispatch: BridgeDispatch,
    readonly registerDocument: (signal: AbortSignal) => Promise<void>, timeoutMs: number | undefined, readonly ended: (error: Error) => void) {
    const timeout = handshakeTimeout(timeoutMs);
    this.#deadline = performance.now() + timeout;
    this.ready = new Promise<void>((resolve, reject) => { this.#resolve = resolve; this.#reject = reject; });
    // Mount callers may attach readiness handlers after synchronous teardown.
    void this.ready.catch(() => undefined);
    this.#timer = setTimeout(() => this.close(new Error("Babble Surface handshake timed out")), timeout);
    window.addEventListener("message", this.#offer);
  }

  async #register(endpoint: PortEndpoint): Promise<void> {
    try {
      if (performance.now() >= this.#deadline) throw new Error("Babble Surface handshake timed out");
      const registration = this.registerDocument(this.#registration.signal);
      if (!registration || typeof registration.then !== "function") {
        throw new Error("Babble Surface document registration callback must return a promise");
      }
      await registration;
      if (this.#closed) return;
      // A delayed timer task must not let an overdue completion acknowledge readiness.
      if (performance.now() >= this.#deadline) throw new Error("Babble Surface handshake timed out");
      this.#bridge = new BrowserBridgeHost(endpoint, this.dispatch);
      endpoint.postMessage(surfaceBridgeControl("ready"));
      if (this.#closed) return;
      clearTimeout(this.#timer);
      this.#resolve();
    } catch (error) {
      this.close(asError(error, "Babble Surface document registration failed"));
    }
  }

  close(error = new Error("Babble Surface mount torn down before channel readiness"), notifyOwner = true): void {
    if (this.#closed) return;
    this.#closed = true;
    clearTimeout(this.#timer);
    this.window.removeEventListener("message", this.#offer);
    this.#bridge?.close();
    this.#registration.abort(error);
    this.#endpoint?.close(error);
    this.#reject(error);
    if (notifyOwner) this.ended(error);
  }
}

/** Child-created capability channel. The parent never sends authority over a window message. */
export async function connectSurfaceBridge(options: ConnectSurfaceBridgeOptions): Promise<BabbleTransport> {
  const timeout = handshakeTimeout(options.timeoutMs);
  const origin = new URL(options.parentOrigin);
  if (!/^https?:$/.test(origin.protocol) || origin.origin !== options.parentOrigin) {
    throw new Error("Babble Surface connector requires an exact HTTP(S) parent origin");
  }
  if (options.signal?.aborted) throw asError(options.signal.reason, "Babble Surface connection aborted");
  const child = options.window ?? globalThis.window;
  if (!child || child.parent === child) throw new Error("Babble Surface connector requires an embedding parent");
  const channel = options.createChannel?.() ?? new MessageChannel();
  return new Promise<BabbleTransport>((resolve, reject) => {
    let transport: BrowserBridgeTransport | undefined;
    let accepted = false;
    let closed = false;
    const finish = (error: Error): void => {
      if (closed) return;
      closed = true;
      clearTimeout(timer);
      child.removeEventListener("pagehide", pagehide);
      options.signal?.removeEventListener("abort", abort);
      transport?.close();
      endpoint.close(error);
      channel.port2.close();
      reject(error);
    };
    const endpoint = new PortEndpoint(channel.port1, (value) => {
      if (!accepted && isControl(value, "accept")) {
        accepted = true;
        try { endpoint.postMessage(surfaceBridgeControl("confirm")); }
        catch (error) { finish(asError(error, "Babble Surface confirmation failed")); }
        return true;
      }
      if (!transport && accepted && isControl(value, "ready")) {
        clearTimeout(timer);
        transport = new BrowserBridgeTransport(endpoint);
        resolve({
          request: (envelope: RpcRequestEnvelope, requestOptions?: RpcRequestOptions): Promise<RpcResponseEnvelope> => transport!.request(envelope, requestOptions),
          close: () => finish(new Error("Babble Surface transport closed")),
        });
        return true;
      }
      return !transport;
    }, finish);
    const pagehide = (): void => finish(new Error("Babble Surface document pagehide"));
    const abort = (): void => finish(asError(options.signal?.reason, "Babble Surface connection aborted"));
    const timer = setTimeout(() => finish(new Error("Babble Surface handshake timed out")), timeout);
    child.addEventListener("pagehide", pagehide);
    options.signal?.addEventListener("abort", abort, { once: true });
    if (options.signal?.aborted) { abort(); return; }
    try {
      endpoint.start();
      // Replies are accepted only on our port1: no window listener can be spoofed
      // by a sibling or by a new document in the parent's WindowProxy.
      child.parent.postMessage(surfaceBridgeControl("connect"), options.parentOrigin, [channel.port2 as MessagePort]);
    } catch (error) {
      finish(asError(error, "Babble Surface port transfer failed"));
    }
  });
}

function asError(error: unknown, fallback: string): Error {
  return error instanceof Error ? error : new Error(typeof error === "string" ? error : fallback);
}
