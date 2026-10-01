import type { BridgeDispatch, JsonValue, RpcRequestEnvelope, RpcResponseEnvelope } from "@babble-protocol/sdk";
import { InvocationApi, isInvocationMethod, parseInvocation, type Invocation, type InvocationExpectation } from "./invocations";
import { InvocationPrompt } from "./invocation-prompt";

interface SurfaceInvocationOptions {
  readonly target: HTMLElement;
  readonly title: string;
  readonly identity: { readonly id: string; readonly handle: string };
  readonly authorized: () => boolean;
  readonly api: InvocationApi;
  readonly acquireConsent?: () => (() => void) | null;
}

/** Host-only decisions; no credentials or challenge-decision API enter the iframe. */
export class SurfaceInvocations {
  readonly #prompt: InvocationPrompt;
  #pending: AbortController | null = null;
  #disposed = false;

  constructor(private readonly options: SurfaceInvocationOptions) {
    this.#prompt = new InvocationPrompt({ target: options.target, authorized: () => this.#live() });
  }

  wrap(dispatch: BridgeDispatch): BridgeDispatch {
    return (request, context) => {
      if (!isInvocationMethod(request.method)) {
        return dispatch(request, context);
      }
      if (!this.#live() || context?.signal.aborted) return failure(request, "CANCELLED", "The Object is no longer active.");
      if (this.#pending) return failure(request, "RATE_LIMITED", "Another permission request is in progress.");
      if (!context?.surfaceDocumentId || !request.binding.surface_session_id || !request.binding.object_id
        || !request.idempotency_key || request.binding.identity_id !== this.options.identity.id) {
        return failure(request, "CAPABILITY_DENIED", "The action is missing its authenticated document context.");
      }
      const expected: InvocationExpectation = {
        actorId: this.options.identity.id, objectId: request.binding.object_id, method: request.method,
        requestKey: request.idempotency_key, payload: structuredClone(request.payload),
        origin: { kind: "surface", session_id: request.binding.surface_session_id, document_id: context.surfaceDocumentId },
      };
      const controller = new AbortController();
      const release = this.options.acquireConsent?.();
      if (release === null) return failure(request, "RATE_LIMITED", "Another permission request is in progress.");
      this.#pending = controller;
      const abort = () => controller.abort();
      context.signal.addEventListener("abort", abort, { once: true });
      const timer = setTimeout(abort, Math.min(60_000, Math.max(1, request.deadline.timeout_ms)));
      const watch = setInterval(() => { if (!this.#live()) abort(); }, 150);
      return this.#run(dispatch, request, { ...context, signal: controller.signal }, expected).finally(() => {
        clearTimeout(timer);
        clearInterval(watch);
        context.signal.removeEventListener("abort", abort);
        if (this.#pending === controller) this.#pending = null;
        release?.();
      });
    };
  }

  dispose(): void {
    this.#disposed = true;
    this.#pending?.abort();
    this.#prompt.dispose();
  }

  #live(): boolean {
    try { return !this.#disposed && this.options.authorized() && this.options.target.isConnected
      && this.options.target.ownerDocument.visibilityState === "visible"; }
    catch { return false; }
  }

  async #run(dispatch: BridgeDispatch, request: RpcRequestEnvelope, context: NonNullable<Parameters<BridgeDispatch>[1]>,
    expected: InvocationExpectation): Promise<RpcResponseEnvelope> {
    let invocation: Invocation | null = null;
    const check = () => { context.signal.throwIfAborted(); if (!this.#live()) throw new Error("Object closed"); };
    try {
      check();
      const response = await dispatch(request, context);
      check();
      if (response.id !== request.id || response.protocol !== request.protocol) throw new Error("Mismatched action response");
      if (response.error?.code !== "PERMISSION_REQUIRED") return response;
      const details = response.error.details;
      if (typeof details !== "object" || details === null || Array.isArray(details) || !("invocation" in details)) throw new Error("Missing action challenge");
      invocation = parseInvocation(details.invocation, expected);
      // Read the authenticated stored intent before displaying it, not child message fields.
      invocation = await this.options.api.advance("status", invocation, expected, context.signal);
      check();
      if (invocation.state.kind === "pending") {
        const payload = invocation.payload as { target_object_id: string; text: string | null;
          media: { title: string; resources: { uri: string; media_type: string; size_bytes: number; integrity: string }[] } | null };
        const decision = await this.#prompt.prompt({
          method: expected.method,
          actor: { id: invocation.actor_id, label: this.options.identity.handle },
          requester: { id: invocation.object_id, title: this.options.title },
          recipient: { id: payload.target_object_id },
          ...(payload.text === null ? {} : { text: payload.text }),
          ...(payload.media === null ? {} : { media: payload.media.resources.map((resource, index) => ({
            id: resource.uri, title: `${payload.media!.title} (${index + 1})`, mimeType: resource.media_type,
            sizeBytes: resource.size_bytes, digest: resource.integrity,
          })) }),
          deadlineEpochMs: Date.parse(invocation.deadline),
        }, { signal: context.signal });
        check();
        invocation = await this.options.api.advance(decision === "allow" ? "allow_once" : decision === "deny" ? "deny" : "cancel",
          invocation, expected, context.signal);
        check();
      }
      if (invocation.state.kind === "approved") {
        invocation = await this.options.api.advance("execute", invocation, expected, context.signal);
        check();
      }
      if (invocation.state.kind === "completed" && invocation.result) {
        return { ...response, error: null, result: invocation.result as unknown as JsonValue };
      }
      return failure(request, invocation.state.kind === "denied" ? "CAPABILITY_DENIED" : "CANCELLED", `The action was ${invocation.state.kind}.`);
    } catch (error) {
      if (invocation && ["pending", "approved"].includes(invocation.state.kind)) {
        try {
          const reconciled = await this.options.api.advance("cancel", invocation, expected, AbortSignal.timeout(5000));
          // Execution may have committed before its acknowledgement was lost.
          if (reconciled.state.kind === "completed" && reconciled.result && !context.signal.aborted && this.#live()) {
            return { protocol: request.protocol, id: request.id, trace_id: request.trace_id ?? null,
              error: null, result: reconciled.result as unknown as JsonValue };
          }
        }
        catch (cause) { console.warn("Babble could not confirm action cancellation", cause); }
      }
      return failure(request, context.signal.aborted || !this.#live() ? "CANCELLED" : "INTERNAL",
        context.signal.aborted || !this.#live() ? "The action was cancelled." : error instanceof Error ? error.message : "The action could not be confirmed.");
    }
  }
}

function failure(request: RpcRequestEnvelope, code: NonNullable<RpcResponseEnvelope["error"]>["code"], message: string): RpcResponseEnvelope {
  return { protocol: request.protocol, id: request.id, result: null, trace_id: request.trace_id ?? null,
    error: { code, message, retryable: code === "RATE_LIMITED", retry_after_ms: code === "RATE_LIMITED" ? 250 : null, details: null } };
}
