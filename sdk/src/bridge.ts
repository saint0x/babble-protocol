import { rpcCatalog, type JsonValue, type RpcRequestEnvelope, type RpcResponseEnvelope } from "./generated/protocol.js";
import type { BabbleTransport, RpcRequestOptions } from "./transport.js";

type RpcErrorCode = NonNullable<RpcResponseEnvelope["error"]>["code"];
const rpcMethodNames = new Set<string>(rpcCatalog.methods.map((method) => method.method));

export interface BridgeMessageEvent {
  readonly data: unknown;
  readonly origin?: string;
}

export type BridgeMessageHandler = (event: BridgeMessageEvent) => void;

export interface BridgeEndpoint {
  postMessage(message: unknown, targetOrigin?: string): void;
  addEventListener(type: "message", handler: BridgeMessageHandler): void;
  removeEventListener(type: "message", handler: BridgeMessageHandler): void;
}

export interface BrowserBridgeTransportOptions {
  readonly targetOrigin?: string;
  readonly allowedOrigins?: readonly string[];
}

export interface BrowserBridgeHostOptions {
  readonly targetOrigin?: string;
  readonly allowedOrigins?: readonly string[];
  readonly maxInboundBytes?: number;
  readonly maxInFlightRequests?: number;
  readonly maxDispatchMs?: number;
}

export interface BridgeDispatchContext {
  readonly signal: AbortSignal;
  /** Host-owned document binding; never read from a child message. */
  readonly surfaceDocumentId?: string;
}

/** Cancellation is cooperative; dispatchers must forward or observe the signal. */
export type BridgeDispatch = (
  request: RpcRequestEnvelope,
  context?: BridgeDispatchContext,
) => Promise<RpcResponseEnvelope> | RpcResponseEnvelope;

interface InFlightDispatch {
  readonly controller: AbortController;
  readonly timer: ReturnType<typeof setTimeout>;
  readonly origin: string | undefined;
  expired: boolean;
}

export interface RpcBridgeRequest {
  readonly type: "babble.rpc.request";
  readonly protocol: typeof rpcCatalog.protocol;
  readonly envelope: RpcRequestEnvelope;
}

export interface RpcBridgeResponse {
  readonly type: "babble.rpc.response";
  readonly protocol: typeof rpcCatalog.protocol;
  readonly response: RpcResponseEnvelope;
}

export interface RpcBridgeCancel {
  readonly type: "babble.rpc.cancel";
  readonly protocol: typeof rpcCatalog.protocol;
  readonly id: string;
}

interface PendingRequest {
  readonly resolve: (response: RpcResponseEnvelope) => void;
  readonly cancel: (reason: Error) => void;
}

export class BrowserBridgeTransport implements BabbleTransport {
  readonly endpoint: BridgeEndpoint;
  readonly targetOrigin: string | undefined;
  readonly allowedOrigins: ReadonlySet<string>;

  #closed = false;
  #pending = new Map<string, PendingRequest>();
  #clientIds = new Set<string>();
  #handler = (event: BridgeMessageEvent): void => {
    this.#receive(event);
  };

  constructor(endpoint: BridgeEndpoint, options: BrowserBridgeTransportOptions = {}) {
    this.endpoint = endpoint;
    this.targetOrigin = options.targetOrigin;
    this.allowedOrigins = new Set(options.allowedOrigins ?? []);
    this.endpoint.addEventListener("message", this.#handler);
  }

  request(envelope: RpcRequestEnvelope, options: RpcRequestOptions = {}): Promise<RpcResponseEnvelope> {
    if (this.#closed) {
      return Promise.reject(new Error("Babble browser bridge transport is closed"));
    }
    if (!validToken(envelope.id, 128)) {
      return Promise.reject(new Error("Babble browser bridge request id must be 1-128 characters"));
    }
    if (this.#clientIds.has(envelope.id)) {
      return Promise.reject(new Error(`duplicate in-flight Babble RPC request id: ${envelope.id}`));
    }
    if (options.signal?.aborted) {
      return Promise.reject(abortError(options.signal.reason));
    }

    const timeoutMs = options.timeoutMs ?? envelope.deadline.timeout_ms;
    // A caller may reuse its deterministic RPC ID. Late replies/cancellations
    // must never match a later attempt, including on older non-cancelling hosts.
    const wireId = `bridge-${crypto.randomUUID()}`;
    const message: RpcBridgeRequest = {
      type: "babble.rpc.request",
      protocol: rpcCatalog.protocol,
      envelope: { ...envelope, id: wireId },
    };

    return new Promise<RpcResponseEnvelope>((resolve, reject) => {
      let sent = false;
      const cleanup = (): void => {
        clearTimeout(timer);
        this.#pending.delete(wireId);
        this.#clientIds.delete(envelope.id);
        if (abort) {
          options.signal?.removeEventListener("abort", abort);
        }
      };
      const cancel = (reason: Error): void => {
        if (!this.#pending.has(wireId)) return;
        cleanup();
        reject(reason);
        if (sent) this.#sendCancellation(wireId);
      };
      const timer = setTimeout(() => {
        cancel(new Error(`Babble browser bridge request timed out: ${envelope.id}`));
      }, timeoutMs);
      const abort = options.signal
        ? (): void => {
            cancel(abortError(options.signal?.reason));
          }
        : null;
      if (abort) {
        options.signal?.addEventListener("abort", abort, { once: true });
      }
      this.#clientIds.add(envelope.id);
      this.#pending.set(wireId, {
        resolve: (response) => {
          cleanup();
          resolve({ ...response, id: envelope.id });
        },
        cancel,
      });
      try {
        if (options.signal?.aborted) {
          cancel(abortError(options.signal.reason));
          return;
        }
        sent = true;
        this.endpoint.postMessage(message, this.targetOrigin);
      } catch (error) {
        // A synchronous clone/send failure has not enqueued a request.
        sent = false;
        cancel(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  close(): void {
    if (this.#closed) {
      return;
    }
    this.#closed = true;
    this.endpoint.removeEventListener("message", this.#handler);
    for (const pending of [...this.#pending.values()]) {
      pending.cancel(new Error("Babble browser bridge transport closed with request in flight"));
    }
    this.#pending.clear();
  }

  #sendCancellation(id: string): void {
    const message: RpcBridgeCancel = { type: "babble.rpc.cancel", protocol: rpcCatalog.protocol, id };
    try { this.endpoint.postMessage(message, this.targetOrigin); }
    catch {
      // A broken endpoint cannot deliver cancellation; terminate its other calls.
      this.close();
    }
  }

  #receive(event: BridgeMessageEvent): void {
    if (!this.#originAllowed(event.origin)) {
      return;
    }
    const message = parseBridgeResponse(event.data);
    if (!message) {
      return;
    }
    const pending = this.#pending.get(message.response.id);
    if (!pending) {
      return;
    }
    pending.resolve(message.response);
  }

  #originAllowed(origin: string | undefined): boolean {
    return this.allowedOrigins.size === 0 || (origin !== undefined && this.allowedOrigins.has(origin));
  }
}

export class BrowserBridgeHost {
  readonly endpoint: BridgeEndpoint;
  readonly dispatch: BridgeDispatch;
  readonly targetOrigin: string | undefined;
  readonly allowedOrigins: ReadonlySet<string>;
  readonly maxInboundBytes: number;
  readonly maxInFlightRequests: number;
  readonly maxDispatchMs: number;

  #closed = false;
  #inFlight = new Map<string, InFlightDispatch>();
  #handler = (event: BridgeMessageEvent): void => {
    void this.#receive(event);
  };

  constructor(endpoint: BridgeEndpoint, dispatch: BridgeDispatch, options: BrowserBridgeHostOptions = {}) {
    this.endpoint = endpoint;
    this.dispatch = dispatch;
    this.targetOrigin = options.targetOrigin;
    this.allowedOrigins = new Set(options.allowedOrigins ?? []);
    this.maxInboundBytes = options.maxInboundBytes ?? 256 * 1024;
    this.maxInFlightRequests = options.maxInFlightRequests ?? 32;
    this.maxDispatchMs = options.maxDispatchMs ?? 30_000;
    this.endpoint.addEventListener("message", this.#handler);
  }

  close(): void {
    if (this.#closed) {
      return;
    }
    this.#closed = true;
    this.endpoint.removeEventListener("message", this.#handler);
    const pending = [...this.#inFlight.values()];
    this.#inFlight.clear();
    for (const entry of pending) {
      clearTimeout(entry.timer);
    }
    for (const entry of pending) {
      entry.controller.abort(new Error("Babble browser bridge host is closed"));
    }
  }

  async #receive(event: BridgeMessageEvent): Promise<void> {
    if (this.#closed || !this.#originAllowed(event.origin)) {
      return;
    }
    const inboundBytes = messageBytes(event.data);
    if (inboundBytes > this.maxInboundBytes) {
      if (isRpcBridgeRequest(event.data)) {
        this.#postResponse(event, bridgeRpcError(event.data.envelope, "QUOTA_EXCEEDED", `Babble bridge request exceeds host inbound byte limit: ${inboundBytes} > ${this.maxInboundBytes}`));
      }
      return;
    }
    if (isRpcBridgeCancel(event.data)) {
      const entry = this.#inFlight.get(event.data.id);
      if (!entry || entry.expired || entry.origin !== event.origin) return;
      entry.expired = true;
      clearTimeout(entry.timer);
      entry.controller.abort(new Error(`Babble bridge request cancelled by caller: ${event.data.id}`));
      return;
    }
    if (!isRpcBridgeRequest(event.data)) {
      return;
    }

    const request = event.data.envelope;
    if (this.#inFlight.has(request.id)) {
      this.#postResponse(event, bridgeRpcError(request, "INVALID_INPUT", `duplicate in-flight Babble RPC request id: ${request.id}`));
      return;
    }
    if (this.#inFlight.size >= this.maxInFlightRequests) {
      this.#postResponse(event, bridgeRpcError(request, "RATE_LIMITED", `Babble bridge in-flight request limit reached: ${this.maxInFlightRequests}`, true, 250));
      return;
    }
    if (request.deadline.timeout_ms <= 0) {
      this.#postResponse(event, bridgeRpcError(request, "INVALID_INPUT", "Babble bridge request deadline must be positive"));
      return;
    }

    const timeoutMs = Math.min(request.deadline.timeout_ms, this.maxDispatchMs);
    const controller = new AbortController();
    const timer = setTimeout(() => {
      if (this.#closed || this.#inFlight.get(request.id) !== entry) {
        return;
      }
      // A timeout cannot free capacity until a non-cooperative dispatch settles.
      entry.expired = true;
      controller.abort(new Error(`Babble bridge host dispatch timed out: ${request.id}`));
      this.#postResponse(event, bridgeRpcError(request, "TIMEOUT", `Babble bridge host dispatch timed out: ${request.id}`, true));
    }, timeoutMs);
    const entry: InFlightDispatch = { controller, timer, origin: event.origin, expired: false };
    this.#inFlight.set(request.id, entry);

    try {
      const response = await this.dispatch(request, { signal: controller.signal });
      if (!entry.expired) {
        this.#postResponse(event, response);
      }
    } catch (error) {
      if (!entry.expired) {
        this.#postResponse(event, bridgeErrorResponse(request, error));
      }
    } finally {
      clearTimeout(timer);
      if (this.#inFlight.get(request.id) === entry) {
        this.#inFlight.delete(request.id);
      }
    }
  }

  #postResponse(event: BridgeMessageEvent, response: RpcResponseEnvelope): void {
    if (this.#closed) {
      return;
    }
    try {
      this.endpoint.postMessage(rpcBridgeResponse(response), this.targetOrigin ?? event.origin);
    } catch {
      // A detached or replaced document is no longer a usable bridge endpoint.
      this.close();
    }
  }

  #originAllowed(origin: string | undefined): boolean {
    return this.allowedOrigins.size === 0 || (origin !== undefined && this.allowedOrigins.has(origin));
  }
}

export function isRpcBridgeRequest(value: unknown): value is RpcBridgeRequest {
  if (!isRecord(value)) {
    return false;
  }
  return value.type === "babble.rpc.request" && value.protocol === rpcCatalog.protocol && isRpcRequestEnvelope(value.envelope);
}

export function isRpcBridgeCancel(value: unknown): value is RpcBridgeCancel {
  return isRecord(value) && value.type === "babble.rpc.cancel"
    && value.protocol === rpcCatalog.protocol && validToken(value.id, 128)
    && Object.keys(value).length === 3;
}

export function rpcBridgeResponse(response: RpcResponseEnvelope): RpcBridgeResponse {
  return {
    type: "babble.rpc.response",
    protocol: rpcCatalog.protocol,
    response,
  };
}

function bridgeErrorResponse(request: RpcRequestEnvelope, error: unknown): RpcResponseEnvelope {
  return bridgeRpcError(request, "INTERNAL", errorMessage(error), false, null, errorDetails(error));
}

function bridgeRpcError(
  request: RpcRequestEnvelope,
  code: RpcErrorCode,
  message: string,
  retryable = false,
  retryAfterMs: number | null = null,
  details: JsonValue = null,
): RpcResponseEnvelope {
  return {
    protocol: rpcCatalog.protocol,
    id: request.id,
    result: null,
    error: {
      code,
      message,
      retryable,
      retry_after_ms: retryAfterMs,
      details,
    },
    trace_id: request.trace_id ?? null,
  };
}

function parseBridgeResponse(value: unknown): RpcBridgeResponse | null {
  if (!isRecord(value)) {
    return null;
  }
  if (value.type !== "babble.rpc.response" || value.protocol !== rpcCatalog.protocol || !isRecord(value.response)) {
    return null;
  }
  return value as unknown as RpcBridgeResponse;
}

function isRpcRequestEnvelope(value: unknown): value is RpcRequestEnvelope {
  if (!isRecord(value)) {
    return false;
  }
  if (value.protocol !== rpcCatalog.protocol || !validToken(value.id, 128)) {
    return false;
  }
  if (typeof value.method !== "string" || !rpcMethodNames.has(value.method)) {
    return false;
  }
  if (!isRpcBinding(value.binding)) {
    return false;
  }
  if (
    !isRecord(value.deadline)
    || !validTimeout(value.deadline.timeout_ms)
    || !nullableString(value.deadline.client_started_at)
  ) {
    return false;
  }
  if (!nullableString(value.idempotency_key) || !nullableString(value.trace_id)) {
    return false;
  }
  return Object.hasOwn(value, "payload") && isJsonValue(value.payload);
}

function isRpcBinding(value: unknown): boolean {
  if (!isRecord(value)) {
    return false;
  }
  return nullableString(value.object_id)
    && nullableString(value.surface_session_id)
    && validToken(value.runtime_id, 256)
    && typeof value.origin === "string"
    && value.origin.length > 0
    && Array.isArray(value.capability_grants)
    && value.capability_grants.every((grant) => validToken(grant, 256))
    && nullableString(value.identity_id);
}

function validToken(value: unknown, maxLength: number): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= maxLength;
}

function nullableString(value: unknown): value is string | null | undefined {
  return value === null || value === undefined || typeof value === "string";
}

function validTimeout(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value > 0;
}

function isJsonValue(value: unknown, seen = new Set<object>()): value is JsonValue {
  if (value === null || typeof value === "string" || typeof value === "boolean") {
    return true;
  }
  if (typeof value === "number") {
    return Number.isFinite(value);
  }
  if (Array.isArray(value)) {
    if (seen.has(value)) {
      return false;
    }
    seen.add(value);
    return value.every((item) => isJsonValue(item, seen));
  }
  if (isRecord(value)) {
    if (seen.has(value)) {
      return false;
    }
    seen.add(value);
    return Object.values(value).every((item) => isJsonValue(item, seen));
  }
  return false;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function messageBytes(value: unknown): number {
  try {
    return new TextEncoder().encode(JSON.stringify(value)).byteLength;
  } catch {
    return Number.POSITIVE_INFINITY;
  }
}

function abortError(reason: unknown): Error {
  if (reason instanceof Error) {
    return reason;
  }
  if (typeof reason === "string" && reason.length > 0) {
    return new Error(reason);
  }
  return new Error("Babble browser bridge request was aborted");
}

function errorMessage(error: unknown): string {
  if (error instanceof Error && error.message.length > 0) {
    return error.message;
  }
  if (typeof error === "string" && error.length > 0) {
    return error;
  }
  return "Babble browser bridge host dispatch failed";
}

function errorDetails(error: unknown): JsonValue {
  if (error instanceof Error) {
    return {
      name: error.name,
    };
  }
  return null;
}
