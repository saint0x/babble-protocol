import type { BridgeDispatch, JsonValue, RpcRequestEnvelope, RpcResponseEnvelope } from "@babble-protocol/sdk";
import { promptBrowserAction, type BrowserAction } from "./browser-action-prompt";
import { BrowserInvocationApi, isBrowserInvocationMethod, normalizedBrowserPayload, parseBrowserInvocation,
  type BrowserInvocation, type BrowserInvocationExpectation, type BrowserInvocationResult } from "./browser-invocations";

type ErrorCode = NonNullable<RpcResponseEnvelope["error"]>["code"];
interface HostActionOptions {
  readonly target: HTMLElement;
  readonly label: string;
  readonly identity: { readonly id: string; readonly handle: string };
  readonly api: BrowserInvocationApi;
  readonly authorized: () => boolean;
  readonly acquireConsent?: () => (() => void) | null;
}
interface PendingAction { readonly cancel: () => void; }

/** One durable dispatch ticket admits one native attempt in its originating document. */
export class HostActions {
  readonly #document: Document;
  readonly #window: Window;
  readonly #watch: ReturnType<typeof setInterval>;
  #pending: PendingAction | null = null;
  #disposed = false;
  #ownsFullscreen = false;

  constructor(private readonly options: HostActionOptions) {
    this.#document = options.target.ownerDocument;
    const window = this.#document.defaultView;
    if (!window) throw new Error("Host actions require a browser document");
    this.#window = window;
    this.#document.addEventListener("visibilitychange", this.#checkLifecycle);
    this.#document.addEventListener("fullscreenchange", this.#fullscreenChanged);
    this.#window.addEventListener("pagehide", this.#pagehide);
    this.#watch = setInterval(this.#checkLifecycle, 150);
  }

  wrap(dispatch: BridgeDispatch): BridgeDispatch {
    return (request, context) => {
      if (["babble.clipboard.write.v1", "babble.fullscreen.enter.v1"].includes(request.method)) {
        return failure(request, "UNSUPPORTED_VERSION", "This action requires one-use consent with the unversioned Babble browser action method.");
      }
      if (!isBrowserInvocationMethod(request.method)) return dispatch(request, context);
      if (!this.#live() || context?.signal.aborted) return failure(request, "CANCELLED", "The Object is no longer active.");
      if (this.#pending) return failure(request, "RATE_LIMITED", "Another permission request is in progress.");
      if (!context?.surfaceDocumentId || !request.binding.surface_session_id || !request.binding.object_id
        || !request.idempotency_key || request.binding.identity_id !== this.options.identity.id) {
        return failure(request, "CAPABILITY_DENIED", "The action is missing its authenticated document context.");
      }
      let payload: JsonValue;
      try { payload = normalizedBrowserPayload(request.method, request.payload); }
      catch { return failure(request, "INVALID_INPUT", "Invalid browser action payload."); }
      const action = actionFrom(request.method, payload);
      if (!this.#available(action)) return failure(request, "CAPABILITY_UNAVAILABLE", "This browser action is not available here.");
      const release = this.options.acquireConsent?.();
      if (release === null) return failure(request, "RATE_LIMITED", "Another permission request is in progress.");
      const expected: BrowserInvocationExpectation = {
        actorId: this.options.identity.id, objectId: request.binding.object_id, method: request.method,
        requestKey: request.idempotency_key, payload,
        origin: { kind: "surface", session_id: request.binding.surface_session_id, document_id: context.surfaceDocumentId },
      };
      return this.#run(dispatch, structuredClone(request), context, expected, action, release);
    };
  }

  dispose(): void {
    if (this.#disposed) return;
    this.#disposed = true;
    clearInterval(this.#watch);
    this.#document.removeEventListener("visibilitychange", this.#checkLifecycle);
    this.#document.removeEventListener("fullscreenchange", this.#fullscreenChanged);
    this.#window.removeEventListener("pagehide", this.#pagehide);
    this.#pending?.cancel();
    this.#exitOwnedFullscreen();
  }

  #live(): boolean {
    try { return !this.#disposed && this.options.target.isConnected
      && !this.options.target.closest('[inert], [hidden], [aria-hidden="true"]')
      && this.#document.visibilityState === "visible" && this.options.authorized(); }
    catch { return false; }
  }
  #available(action: BrowserAction): boolean {
    return action.kind === "clipboard" ? typeof this.#window.navigator.clipboard?.writeText === "function"
      : typeof this.options.target.requestFullscreen === "function" && this.#document.fullscreenEnabled;
  }
  #checkLifecycle = (): void => {
    if (this.#live()) return;
    this.#pending?.cancel();
    this.#exitOwnedFullscreen();
  };
  #pagehide = (): void => { this.dispose(); };
  #fullscreenChanged = (): void => {
    if (this.#document.fullscreenElement !== this.options.target) this.#ownsFullscreen = false;
  };
  #exitOwnedFullscreen(): void {
    if (!this.#ownsFullscreen || this.#document.fullscreenElement !== this.options.target) return;
    this.#ownsFullscreen = false;
    void this.#document.exitFullscreen().catch(error => console.warn("Babble could not exit its Object fullscreen", error));
  }

  #run(dispatch: BridgeDispatch, request: RpcRequestEnvelope, context: NonNullable<Parameters<BridgeDispatch>[1]>,
    expected: BrowserInvocationExpectation, action: BrowserAction, release?: () => void): Promise<RpcResponseEnvelope> {
    const controller = new AbortController();
    let resolveCancellation!: (response: RpcResponseEnvelope) => void;
    const cancelled = new Promise<RpcResponseEnvelope>(resolve => { resolveCancellation = resolve; });
    const abort = () => {
      controller.abort();
      resolveCancellation(failure(request, "CANCELLED", "The action was cancelled. An already-started clipboard write cannot be undone."));
    };
    const pending = { cancel: abort };
    this.#pending = pending;
    context.signal.addEventListener("abort", abort, { once: true });
    const timer = setTimeout(abort, Math.min(30_000, Math.max(1, request.deadline.timeout_ms)));
    let invocation: BrowserInvocation | null = null, ticket: NonNullable<BrowserInvocation["execution_ticket"]> | null = null;
    let nativeStarted = false, reported = false;
    const live = () => this.#live() && !controller.signal.aborted && (!invocation || Date.now() < Date.parse(invocation.deadline));
    const check = () => { if (!live()) { abort(); throw new Error("The action is no longer active."); } };
    const deadlineWatch = setInterval(() => { if (!live()) abort(); }, 100);
    const report = async (result: BrowserInvocationResult) => {
      if (!invocation || !ticket) throw new Error("Missing browser dispatch authority.");
      invocation = await this.options.api.acknowledge(invocation, expected, ticket.dispatch_id, result, AbortSignal.timeout(5000));
      reported = true;
    };
    const reconcile = async () => {
      if (!invocation) return;
      try {
        if (ticket && !nativeStarted && !reported) await report({ kind: "failed", code: "context_lost" });
        else if (["pending", "approved"].includes(invocation.state.kind)) {
          invocation = await this.options.api.advance("cancel", invocation, expected, AbortSignal.timeout(5000));
        }
      } catch (cause) { console.warn("Babble could not confirm browser action cancellation", cause); }
    };
    const work = (async (): Promise<RpcResponseEnvelope> => {
      try {
        if (context.signal.aborted) abort();
        check();
        const response = await dispatch(request, { ...context, signal: controller.signal });
        check();
        if (response.id !== request.id || response.protocol !== request.protocol) throw new Error("Mismatched browser action response.");
        if (response.error?.code !== "PERMISSION_REQUIRED") {
          if (!response.error && !isCompletedResult(response.result, action)) throw new Error("The browser action has no confirmed outcome.");
          return response;
        }
        const details = response.error.details;
        if (!record(details) || !("invocation" in details)) throw new Error("Missing browser action challenge.");
        invocation = parseBrowserInvocation(details.invocation, expected);
        invocation = await this.options.api.advance("status", invocation, expected, controller.signal);
        check();
        if (invocation.state.kind === "completed" && invocation.result) return success(request, invocation.result);
        if (invocation.state.kind === "failed") return nativeFailure(request, invocation.result);
        if (invocation.state.kind !== "pending" && invocation.state.kind !== "approved") {
          return failure(request, invocation.state.kind === "denied" ? "CAPABILITY_DENIED" : "CONFLICT",
            "This action cannot run again. Its existing outcome has not been changed.");
        }
        const outcome = await promptBrowserAction({ target: this.options.target, label: this.options.label,
          actor: this.options.identity.handle, live }, action, {
          signal: controller.signal, cancel: abort,
          authorize: async () => {
            check();
            if (invocation!.state.kind === "pending") invocation = await this.options.api.advance("allow_once", invocation!, expected, controller.signal);
            check();
            if (invocation!.state.kind !== "approved") throw new Error("The action is no longer approved.");
            invocation = await this.options.api.advance("dispatch", invocation!, expected, controller.signal);
            ticket = invocation.execution_ticket;
            check();
            if (!ticket || invocation.state.kind !== "running") throw new Error("This action was already dispatched. It will not run again.");
          },
          execute: () => {
            check();
            if (!ticket || nativeStarted || !this.#available(action)) throw new Error("Browser dispatch is unavailable.");
            // Consume in memory before invoking native, including synchronous failures.
            nativeStarted = true;
            if (action.kind === "clipboard") return this.#window.navigator.clipboard.writeText(action.text);
            if (this.#document.fullscreenElement !== this.options.target) this.#ownsFullscreen = true;
            return this.options.target.requestFullscreen({ navigationUI: action.navigationUI }).then(() => {
              if (this.#document.fullscreenElement !== this.options.target) throw new Error("The browser did not enter Object fullscreen.");
              if (!live()) this.#exitOwnedFullscreen();
            });
          },
        });
        if (outcome.kind === "completed") {
          const result: BrowserInvocationResult = action.kind === "clipboard" ? { kind: "clipboard_write", written: true }
            : { kind: "fullscreen_enter", entered: true };
          await report(result);
          check();
          return success(request, result);
        }
        if (outcome.kind === "failed" && nativeStarted) {
          if (action.kind === "fullscreen" && this.#document.fullscreenElement !== this.options.target) this.#ownsFullscreen = false;
          const error = outcome.error;
          const code = error instanceof Error && (error.name === "NotAllowedError" || error.name === "SecurityError") ? "not_allowed" : "native_error";
          const result: BrowserInvocationResult = { kind: "failed", code };
          await report(result);
          return nativeFailure(request, result);
        }
        if (outcome.kind === "denied") {
          invocation = await this.options.api.advance("deny", invocation!, expected, controller.signal);
          return failure(request, "CAPABILITY_DENIED", "The browser action was declined.");
        }
        await reconcile();
        return failure(request, outcome.kind === "cancelled" ? "CANCELLED" : "CONFLICT",
          outcome.kind === "cancelled" ? "The browser action was cancelled." : "The action could not be authorized. It has not been repeated.");
      } catch (cause) {
        await reconcile();
        return failure(request, !live() ? "CANCELLED" : "CONFLICT",
          !live() ? "The browser action was cancelled." : nativeStarted
            ? "The browser action outcome could not be confirmed. It has not been repeated."
            : cause instanceof Error ? cause.message : "The browser action could not be confirmed.");
      } finally {
        clearTimeout(timer);
        clearInterval(deadlineWatch);
        context.signal.removeEventListener("abort", abort);
        if (this.#pending === pending) this.#pending = null;
        release?.();
      }
    })();
    return Promise.race([work, cancelled]);
  }
}

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
function actionFrom(method: string, payload: JsonValue): BrowserAction {
  const value = payload as Record<string, JsonValue>;
  return method === "babble.clipboard.write" ? { kind: "clipboard", text: value.text as string }
    : { kind: "fullscreen", navigationUI: value.navigation_ui as FullscreenNavigationUI, targetHint: value.target_hint as string | null };
}
function isCompletedResult(value: unknown, action: BrowserAction): boolean {
  return record(value) && Object.keys(value).length === 2 && (action.kind === "clipboard"
    ? value.kind === "clipboard_write" && value.written === true : value.kind === "fullscreen_enter" && value.entered === true);
}
function success(request: RpcRequestEnvelope, result: BrowserInvocationResult): RpcResponseEnvelope {
  return { protocol: request.protocol, id: request.id, trace_id: request.trace_id ?? null, error: null, result: result as JsonValue };
}
function nativeFailure(request: RpcRequestEnvelope, result: BrowserInvocationResult | null): RpcResponseEnvelope {
  return failure(request, result?.kind === "failed" && result.code === "not_allowed" ? "CAPABILITY_DENIED" : "CAPABILITY_UNAVAILABLE",
    "The browser could not complete this action. It has not been repeated.");
}
function failure(request: RpcRequestEnvelope, code: ErrorCode, message: string): RpcResponseEnvelope {
  return { protocol: request.protocol, id: request.id, result: null, trace_id: request.trace_id ?? null,
    error: { code, message, retryable: code === "RATE_LIMITED", retry_after_ms: code === "RATE_LIMITED" ? 250 : null, details: null } };
}
