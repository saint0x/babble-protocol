import { canonicalValueBytes, type JsonValue, type ProtocolTypes } from "@babble-protocol/sdk";
import type { InvocationSource } from "./invocations";

type BrowserInvocationWire = ProtocolTypes["api.BrowserInvocationResponse"];
export type BrowserInvocation = Omit<BrowserInvocationWire, "result" | "execution_ticket"> & {
  readonly result: Exclude<BrowserInvocationWire["result"], undefined>;
  readonly execution_ticket: Exclude<BrowserInvocationWire["execution_ticket"], undefined>;
};
export type BrowserInvocationResult = NonNullable<BrowserInvocation["result"]>;
export type BrowserInvocationMethod = "babble.clipboard.write" | "babble.fullscreen.enter";
export interface BrowserInvocationExpectation {
  readonly actorId: string;
  readonly objectId: string;
  readonly origin: InvocationSource;
  readonly method: BrowserInvocationMethod;
  readonly requestKey: string;
  readonly payload: JsonValue;
}
const base = "/invocations/v1/browser";
const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const hash = (value: unknown) => typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
const uuid = (value: unknown) => typeof value === "string" && /^[0-9a-f]{8}(?:-[0-9a-f]{4}){3}-[0-9a-f]{12}$/.test(value);
const invalid = () => new Error("The browser action could not be verified. It has not been retried.");
const same = (left: unknown, right: unknown): boolean => {
  const a = canonicalValueBytes(left as JsonValue), b = canonicalValueBytes(right as JsonValue);
  return a.length === b.length && a.every((byte, index) => byte === b[index]);
};
export function isBrowserInvocationMethod(method: string): method is BrowserInvocationMethod {
  return method === "babble.clipboard.write" || method === "babble.fullscreen.enter";
}
export function normalizedBrowserPayload(method: BrowserInvocationMethod, payload: JsonValue): JsonValue {
  if (!record(payload)) throw invalid();
  if (method === "babble.clipboard.write") {
    if (typeof payload.text !== "string" || new TextEncoder().encode(payload.text).byteLength > 65_536
      || Object.keys(payload).some(key => key !== "text")) throw invalid();
    const normalized = { text: payload.text };
    // Match the journal's limits, including canonical framing and JSON escapes.
    if (canonicalValueBytes(normalized).byteLength > 65_536
      || new TextEncoder().encode(JSON.stringify(normalized)).byteLength > 65_536) throw invalid();
    return normalized;
  }
  const navigation = payload.navigation_ui ?? "auto", target = payload.target_hint ?? null;
  if (typeof navigation !== "string" || !["auto", "hide", "show"].includes(navigation)
    || (target !== null && (typeof target !== "string" || !target.trim() || new TextEncoder().encode(target).byteLength > 256))
    || Object.keys(payload).some(key => key !== "navigation_ui" && key !== "target_hint")) throw invalid();
  return { navigation_ui: navigation, target_hint: target } as JsonValue;
}
function validResult(value: unknown, method: BrowserInvocationMethod): value is BrowserInvocationResult {
  if (!record(value)) return false;
  if (value.kind === "failed") return Object.keys(value).length === 2
    && typeof value.code === "string" && ["not_allowed", "unavailable", "context_lost", "native_error"].includes(value.code);
  return Object.keys(value).length === 2 && (method === "babble.clipboard.write"
    ? value.kind === "clipboard_write" && value.written === true : value.kind === "fullscreen_enter" && value.entered === true);
}

/** Tickets are valid only in the first dispatch response, never in state/history. */
export function parseBrowserInvocation(value: unknown, expected: BrowserInvocationExpectation, previous?: BrowserInvocation,
  allowTicket = false): BrowserInvocation {
  if (!record(value) || !hash(value.invocation_id) || value.actor_id !== expected.actorId || value.object_id !== expected.objectId
    || value.method !== expected.method || value.request_key !== expected.requestKey || !same(value.origin, expected.origin)
    || !same(value.payload, normalizedBrowserPayload(expected.method, expected.payload))
    || typeof value.created_at !== "string" || typeof value.deadline !== "string"
    || !Number.isFinite(Date.parse(value.created_at)) || !Number.isFinite(Date.parse(value.deadline))
    || Date.parse(value.deadline) <= Date.parse(value.created_at) || Date.parse(value.deadline) - Date.parse(value.created_at) > 300_000
    || !Number.isInteger(value.revision) || (value.revision as number) < 0 || (value.revision as number) > 4
    || !record(value.state) || typeof value.state.kind !== "string"
    || !["pending", "approved", "running", "unknown", "completed", "failed", "denied", "cancelled", "expired", "invalidated"].includes(value.state.kind)) throw invalid();
  if (previous && (value.invocation_id !== previous.invocation_id || value.created_at !== previous.created_at
    || value.deadline !== previous.deadline || (value.revision as number) < previous.revision
    || (value.revision === previous.revision && (!same(value.state, previous.state) || !same(value.result, previous.result))))) throw invalid();
  if (previous) {
    const prior = previous.state;
    const terminal = !["pending", "approved", "running", "unknown"].includes(prior.kind);
    if (terminal && (!same(value.state, prior) || !same(value.result, previous.result) || value.revision !== previous.revision)) throw invalid();
    if (prior.kind === "approved" && value.state.kind === "pending") throw invalid();
    if (prior.kind === "running" || prior.kind === "unknown") {
      if (!["running", "unknown", "completed", "failed"].includes(value.state.kind)
        || (prior.kind === "unknown" && value.state.kind === "running")) throw invalid();
      const nextDispatch = value.state.kind === "completed" && record(value.state.outcome)
        ? value.state.outcome.dispatch_id : value.state.dispatch_id;
      if (value.state.kind !== "failed" && nextDispatch !== prior.dispatch_id) throw invalid();
    }
  }
  if (["running", "unknown"].includes(value.state.kind) && !hash(value.state.dispatch_id)) throw invalid();
  if (value.state.kind === "completed") {
    if (!validResult(value.result, expected.method) || value.result.kind === "failed" || !record(value.state.outcome)
      || value.state.outcome.kind !== "external" || !hash(value.state.outcome.dispatch_id)
      || !same(value.state.outcome.result, value.result)) throw invalid();
  } else if (value.state.kind === "failed") {
    if (!validResult(value.result, expected.method) || value.result.kind !== "failed" || value.state.code !== value.result.code) throw invalid();
  } else if (value.result !== null) throw invalid();
  if (value.execution_ticket !== null) {
    if (!allowTicket || previous?.state.kind !== "approved" || value.state.kind !== "running" || !record(value.execution_ticket)
      || Object.keys(value.execution_ticket).length !== 2 || value.execution_ticket.executor !== "babble.browser.v1"
      || !hash(value.execution_ticket.dispatch_id) || value.execution_ticket.dispatch_id !== value.state.dispatch_id) throw invalid();
  }
  return structuredClone(value) as unknown as BrowserInvocation;
}

export class BrowserInvocationApi {
  constructor(private readonly origin: URL, private readonly fetcher: typeof fetch) {}

  async advance(action: "status" | "dispatch" | "cancel" | "allow_once" | "deny", current: BrowserInvocation,
    expected: BrowserInvocationExpectation, signal: AbortSignal): Promise<BrowserInvocation> {
    current = structuredClone(current);
    expected = structuredClone(expected);
    const decision = action === "allow_once" || action === "deny";
    const value = await this.request(`${base}/${current.invocation_id}/${decision ? "decision" : action}`,
      action === "status" ? "GET" : "POST", action === "status" ? undefined : decision ? { decision: action } : {}, expected.origin, signal);
    return parseBrowserInvocation(value, expected, current, action === "dispatch");
  }

  async acknowledge(current: BrowserInvocation, expected: BrowserInvocationExpectation, dispatchId: string,
    result: BrowserInvocationResult, signal: AbortSignal): Promise<BrowserInvocation> {
    current = structuredClone(current);
    expected = structuredClone(expected);
    result = structuredClone(result);
    if (!hash(dispatchId) || !validResult(result, expected.method)) throw invalid();
    const state = current.state;
    if ((state.kind !== "running" && state.kind !== "unknown") || state.dispatch_id !== dispatchId) throw invalid();
    // An acknowledgement can be retried; an external effect cannot.
    const payload = { dispatch_id: dispatchId, result };
    let cause: unknown;
    for (let attempt = 0; attempt < 2; attempt++) {
      try {
        const value = await this.request(`${base}/${current.invocation_id}/ack`, "POST", payload, expected.origin, signal);
        const acknowledged = parseBrowserInvocation(value, expected, current);
        if (!same(acknowledged.result, result)) throw invalid();
        if ((result.kind === "failed" && acknowledged.state.kind !== "failed")
          || (result.kind !== "failed" && acknowledged.state.kind !== "completed")) throw invalid();
        if (acknowledged.state.kind === "completed" && acknowledged.state.outcome.kind === "external"
          && acknowledged.state.outcome.dispatch_id !== dispatchId) throw invalid();
        return acknowledged;
      } catch (error) {
        cause = error;
        if (signal.aborted || (error instanceof BrowserInvocationRequestError && error.status < 500)) throw error;
      }
    }
    throw cause;
  }

  private async request(path: string, method: string, body: unknown, source: InvocationSource, signal: AbortSignal): Promise<unknown> {
    signal.throwIfAborted();
    if (!uuid(source.document_id)) throw invalid();
    const headers: Record<string, string> = { accept: "application/json",
      [source.kind === "surface" ? "x-babble-surface-document" : "x-babble-host-document"]: source.document_id };
    if (body !== undefined) headers["content-type"] = "application/json";
    const response = await this.fetcher(new URL(path, this.origin), {
      method, headers, ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal,
    });
    signal.throwIfAborted();
    if (!response.ok) throw new BrowserInvocationRequestError(response.status);
    const value: unknown = await response.json();
    signal.throwIfAborted();
    return value;
  }
}
export class BrowserInvocationRequestError extends Error {
  constructor(readonly status: number) {
    super(status === 401 ? "Sign in again to continue."
      : status === 403 ? "The Object is no longer authorized."
      : status === 409 ? "The action has expired or changed. It has not been retried."
      : status === 429 ? "Too many actions are in progress. Please wait."
      : "The browser action outcome could not be confirmed. It has not been repeated.");
  }
}
