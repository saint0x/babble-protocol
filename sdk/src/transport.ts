import type {
  JsonValue,
  RpcError,
  RpcMethodName,
  RpcRequestEnvelope,
  RpcResponseEnvelope,
} from "./generated/protocol.js";

export type RpcBinding = RpcRequestEnvelope["binding"];

export interface RpcRequestOptions {
  readonly id?: string;
  readonly idempotencyKey?: string;
  readonly timeoutMs?: number;
  readonly traceId?: string;
  readonly signal?: AbortSignal;
  /** Trusted host context only; the browser bridge never serializes this option. */
  readonly surfaceDocumentId?: string;
}

export interface BabbleTransport {
  request(envelope: RpcRequestEnvelope, options?: RpcRequestOptions): Promise<RpcResponseEnvelope>;
  close(): void;
}

export class BabbleError extends Error {
  readonly code: RpcError["code"];
  readonly retryable: boolean;
  readonly retryAfterMs: number | null;
  readonly details: JsonValue;

  constructor(error: RpcError) {
    super(error.message);
    this.name = "BabbleError";
    this.code = error.code;
    this.retryable = error.retryable;
    this.retryAfterMs = error.retry_after_ms ?? null;
    this.details = error.details;
  }
}

export class HttpRpcTransport implements BabbleTransport {
  readonly endpoint: URL;
  readonly fetchImpl: typeof fetch;

  constructor(endpoint: string | URL, fetchImpl?: typeof fetch) {
    this.endpoint = new URL(endpoint);
    this.fetchImpl = fetchImpl ?? globalThis.fetch.bind(globalThis);
  }

  async request(envelope: RpcRequestEnvelope, options: RpcRequestOptions = {}): Promise<RpcResponseEnvelope> {
    const documentId = options.surfaceDocumentId;
    const surfaceBound = Boolean(envelope.binding.object_id && envelope.binding.surface_session_id);
    const headers: Record<string, string> = { "content-type": "application/json" };
    if (surfaceBound) {
      if (typeof documentId !== "string" || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(documentId)) {
        throw new Error("Babble Object+Surface RPC requires a canonical lowercase UUID surface document ID");
      }
      headers["x-babble-surface-document"] = documentId;
    } else if (documentId !== undefined) {
      throw new Error("Babble surface document ID requires both Object and Surface session bindings");
    }
    const init: RequestInit = {
      method: "POST",
      headers,
      body: JSON.stringify(envelope),
    };
    if (options.signal) {
      init.signal = options.signal;
    }
    const response = await this.fetchImpl(this.endpoint, init);
    if (!response.ok) {
      throw new Error(`Babble RPC HTTP transport failed with status ${response.status}`);
    }
    return (await response.json()) as RpcResponseEnvelope;
  }

  close(): void {}
}

export function hostBinding(runtimeId: string, origin: string): RpcBinding {
  return {
    object_id: null,
    surface_session_id: null,
    runtime_id: runtimeId,
    origin,
    capability_grants: [],
    identity_id: null,
  };
}

export function hostSurfaceBinding(input: {
  readonly runtimeId: string;
  readonly origin: string;
  readonly surfaceSessionId: string;
}): RpcBinding {
  return {
    object_id: null,
    surface_session_id: input.surfaceSessionId,
    runtime_id: input.runtimeId,
    origin: input.origin,
    capability_grants: [],
    identity_id: null,
  };
}

export function objectBinding(input: {
  readonly objectId: string;
  readonly surfaceSessionId: string;
  readonly runtimeId: string;
  readonly origin: string;
  readonly capabilityGrants?: readonly string[];
  readonly identityId?: string | null;
}): RpcBinding {
  return {
    object_id: input.objectId,
    surface_session_id: input.surfaceSessionId,
    runtime_id: input.runtimeId,
    origin: input.origin,
    capability_grants: [...(input.capabilityGrants ?? [])],
    identity_id: input.identityId ?? null,
  };
}

export function requestId(method: RpcMethodName, payload: JsonValue): string {
  const encoded = JSON.stringify([method, payload]);
  let hash = 0x811c9dc5;
  for (let index = 0; index < encoded.length; index += 1) {
    hash ^= encoded.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return `sdk-${hash.toString(16).padStart(8, "0")}`;
}
