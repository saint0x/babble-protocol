import { canonicalValueBytes, type JsonValue, type ProtocolTypes } from "@babel-protocol/sdk";

export type Invocation = ProtocolTypes["api.InvocationResponse"];
export type InvocationSource = Invocation["origin"];
export type InvocationResult = NonNullable<Invocation["result"]>;
export type InvocationMethod = `babel.social.${"follow" | "unfollow" | "reply" | "share"}.v2`;
export interface InvocationExpectation {
  readonly actorId: string;
  readonly objectId: string;
  readonly origin: InvocationSource;
  readonly method: InvocationMethod;
  readonly requestKey: string;
  readonly payload: JsonValue;
}

const base = "/invocations/v1";
const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const hash = (value: unknown) => typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
const uuid = (value: unknown) => typeof value === "string" && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(value);
const invalid = () => new Error("The action response does not match this request. Its outcome could not be confirmed.");

export function isInvocationMethod(method: string): method is InvocationMethod {
  return /^babel\.social\.(follow|unfollow|reply|share)\.v2$/.test(method);
}

function same(a: unknown, b: unknown): boolean {
  const left = canonicalValueBytes(a as JsonValue), right = canonicalValueBytes(b as JsonValue);
  return left.length === right.length && left.every((byte, index) => byte === right[index]);
}

export function normalizedInvocationPayload(method: InvocationMethod, payload: JsonValue, objectId: string): JsonValue {
  if (!record(payload)) throw invalid();
  const textAction = method === "babel.social.reply.v2" || method === "babel.social.share.v2";
  const media = payload.media;
  if (textAction && payload.text != null && typeof payload.text !== "string") throw invalid();
  if (media != null && (!record(media) || typeof media.title !== "string" || !Array.isArray(media.resources))) throw invalid();
  return {
    target_object_id: payload.target_object_id ?? objectId,
    text: textAction ? (typeof payload.text === "string" ? payload.text.trim() : "") : null,
    media: media == null ? null : { ...media, title: (media.title as string).trim() },
  } as JsonValue;
}

/** Compare all authority and intent fields before displaying or deciding a challenge. */
export function parseInvocation(value: unknown, expected: InvocationExpectation, previous?: Invocation): Invocation {
  if (!record(value) || !hash(value.invocation_id) || value.actor_id !== expected.actorId
    || value.object_id !== expected.objectId || value.method !== expected.method || value.request_key !== expected.requestKey
    || !same(value.origin, expected.origin) || !same(value.payload, normalizedInvocationPayload(expected.method, expected.payload, expected.objectId))
    || typeof value.created_at !== "string" || typeof value.deadline !== "string"
    || !Number.isFinite(Date.parse(value.created_at)) || !Number.isFinite(Date.parse(value.deadline))
    || Date.parse(value.deadline) <= Date.parse(value.created_at) || Date.parse(value.deadline) - Date.parse(value.created_at) > 300_000
    || !Number.isInteger(value.revision) || (value.revision as number) < 0 || (value.revision as number) > 4
    || !record(value.state) || !["pending", "approved", "completed", "denied", "cancelled", "expired", "invalidated"].includes(String(value.state.kind))) throw invalid();
  if (previous && (value.invocation_id !== previous.invocation_id || value.created_at !== previous.created_at
    || value.deadline !== previous.deadline || (value.revision as number) < previous.revision)) throw invalid();
  if (value.state.kind === "completed") {
    if (!record(value.result) || !record(value.result.edge) || !record(value.result.receipt)
      || value.result.edge.author !== expected.actorId || typeof value.result.edge.id !== "string") throw invalid();
    if (["babel.social.reply.v2", "babel.social.share.v2"].includes(expected.method)
      && (!record(value.result.object) || value.result.object.author !== expected.actorId || typeof value.result.object.id !== "string")) throw invalid();
    const receipt = value.result.receipt;
    if (!record(receipt.request) || !record(receipt.outcome) || receipt.request.author !== expected.actorId
      || !hash(receipt.request.id) || !hash(receipt.request.fingerprint)
      || !record(value.state.outcome) || value.state.outcome.kind !== "publication" || value.state.outcome.receipt !== receipt.request.id
      || !Array.isArray(receipt.outcome.edges) || !receipt.outcome.edges.includes(value.result.edge.id)
      || typeof receipt.outcome.event !== "string"
      || receipt.outcome.object !== (record(value.result.object) ? value.result.object.id : null)) throw invalid();
  } else if (value.result !== null) throw invalid();
  return structuredClone(value) as unknown as Invocation;
}

export class InvocationApi {
  constructor(private readonly origin: URL, private readonly fetcher: typeof fetch) {}

  async registerDocument(objectId: string, documentId: string, signal: AbortSignal): Promise<void> {
    if (!uuid(documentId)) throw invalid();
    const value = await this.request(`${base}/documents/${documentId}`, "PUT", { object_id: objectId }, null, signal);
    if (!record(value) || value.document_id !== documentId || value.object_id !== objectId
      || typeof value.expires_at !== "string" || !Number.isFinite(Date.parse(value.expires_at))
      || Date.parse(value.expires_at) <= Date.now() || !Number.isInteger(value.renew_after_ms)
      || (value.renew_after_ms as number) <= 0) throw invalid();
  }

  async closeDocument(documentId: string, signal: AbortSignal): Promise<void> {
    if (!uuid(documentId)) throw invalid();
    await this.request(`${base}/documents/${documentId}`, "DELETE", undefined, null, signal);
  }

  async prepare(expected: InvocationExpectation, timeoutMs: number, signal: AbortSignal): Promise<Invocation> {
    const value = await this.request(`${base}/prepare`, "POST", {
      origin: expected.origin, object_id: expected.objectId, method: expected.method,
      request_key: expected.requestKey, payload: expected.payload, timeout_ms: timeoutMs,
    }, expected.origin, signal);
    return parseInvocation(value, expected);
  }

  async advance(action: "status" | "execute" | "cancel" | "allow_once" | "deny", current: Invocation,
    expected: InvocationExpectation, signal: AbortSignal): Promise<Invocation> {
    const decision = action === "allow_once" || action === "deny";
    const path = `${base}/${current.invocation_id}/${decision ? "decision" : action}`;
    const value = await this.request(path, action === "status" ? "GET" : "POST",
      action === "status" ? undefined : decision ? { decision: action } : {}, expected.origin, signal);
    return parseInvocation(value, expected, current);
  }

  /** The host composer submission is the decision for its own exact intent. */
  async performHost(expected: InvocationExpectation, signal: AbortSignal): Promise<InvocationResult> {
    if (expected.origin.kind !== "host_action") throw invalid();
    const recovered = await this.request(`${base}/recover`, "POST", {
      object_id: expected.objectId, method: expected.method, request_key: expected.requestKey, payload: expected.payload,
    }, null, signal);
    if (recovered !== null) {
      // History can outlive its document; this result never becomes execution authority.
      if (!record(recovered) || !record(recovered.origin) || recovered.origin.kind !== "host_action"
        || !uuid(recovered.origin.document_id)) throw invalid();
      const original: InvocationSource = { kind: "host_action", document_id: recovered.origin.document_id as string };
      const completed = parseInvocation(recovered, { ...expected, origin: original });
      if (completed.state.kind !== "completed" || !completed.result) throw invalid();
      return completed.result;
    }
    await this.registerDocument(expected.objectId, expected.origin.document_id, signal);
    let invocation = await this.prepare(expected, 30_000, signal);
    if (invocation.state.kind === "pending") invocation = await this.advance("allow_once", invocation, expected, signal);
    if (invocation.state.kind === "approved") invocation = await this.advance("execute", invocation, expected, signal);
    if (invocation.state.kind !== "completed" || !invocation.result) throw new Error(`This action is ${invocation.state.kind}. Start a new action to continue.`);
    // Cleanup failure must not turn an already committed publication into a failed submit.
    void this.closeDocument(expected.origin.document_id, AbortSignal.timeout(5000)).catch(error => {
      console.warn("Babel could not close a completed host action document", error);
    });
    return invocation.result;
  }

  private async request(path: string, method: string, body: unknown, source: InvocationSource | null, signal: AbortSignal): Promise<unknown> {
    signal.throwIfAborted();
    const headers: Record<string, string> = { accept: "application/json" };
    if (source) {
      if (!uuid(source.document_id)) throw invalid();
      headers[source.kind === "surface" ? "x-babel-surface-document" : "x-babel-host-document"] = source.document_id;
    }
    if (body !== undefined) headers["content-type"] = "application/json";
    const response = await this.fetcher(new URL(path, this.origin), {
      method, headers, ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal,
    });
    signal.throwIfAborted();
    if (!response.ok) throw new InvocationRequestError(response.status);
    if (response.status === 204) return null;
    const value: unknown = await response.json();
    signal.throwIfAborted();
    return value;
  }
}

export class InvocationRequestError extends Error {
  constructor(readonly status: number) {
    super(status === 401 ? "Sign in again to continue this action."
      : status === 403 ? "This action is no longer authorized. Reopen the Object."
      : status === 409 ? "This action has expired or changed. Start a new action."
      : status === 429 ? "Too many actions are in progress. Please wait."
      : `The action could not be confirmed (${status}).`);
  }
}
