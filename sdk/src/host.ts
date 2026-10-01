import { type BridgeDispatch } from "./bridge.js";
import { verifiedBundleEntry } from "./bundle.js";
import { SurfaceChannelHost, type SurfaceMessagePort } from "./channel.js";
import { rpcCatalog, type RpcOutput, type RpcRequestEnvelope } from "./generated/protocol.js";
import { createSurfaceLifecycle, type SurfaceLifecycleController } from "./lifecycle.js";

type SurfacePlan = RpcOutput<"babble.runtime.surface.prepare.v1">["plan"];
type BrowserSurfaceTarget = "Static" | "Web" | "WebGpu";

export interface SurfaceHostWindow {
  readonly location?: { readonly origin: string };
  addEventListener(type: "message", handler: (event: SurfaceWindowMessageEvent) => void): void;
  removeEventListener(type: "message", handler: (event: SurfaceWindowMessageEvent) => void): void;
}

export interface SurfaceWindowMessageEvent {
  readonly data: unknown;
  readonly origin: string;
  readonly source: SurfaceFrameWindow | null;
  readonly ports?: readonly SurfaceMessagePort[];
}

export interface SurfaceFrameWindow {
  postMessage(message: unknown, targetOrigin: string): void;
}

export interface SurfaceFrame extends Node {
  src: string;
  title: string;
  loading: "eager" | "lazy";
  referrerPolicy: string;
  readonly sandbox: { add(token: string): void };
  readonly contentWindow: SurfaceFrameWindow | null;
  setAttribute(name: string, value: string): void;
  remove(): void;
}

export interface SurfaceDocument {
  createElement(tagName: "iframe"): SurfaceFrame;
}

export interface SurfaceContainer {
  appendChild<T extends Node>(node: T): T;
}

export interface SurfaceMountOptions {
  readonly container: SurfaceContainer;
  /** Use the host's authenticated session RPC plan, never a plan supplied by the embedded document. */
  readonly plan: SurfacePlan;
  readonly dispatch: BridgeDispatch;
  /** Register the host-generated document through the authenticated session API before any RPC. */
  readonly registerDocument: (documentId: string, signal: AbortSignal) => Promise<void>;
  readonly window?: SurfaceHostWindow;
  readonly document?: SurfaceDocument;
  readonly hostOrigin?: string;
  readonly surfaceOrigin?: string;
  readonly surfaceSessionId: string;
  readonly currentIdentityId?: string | null;
  readonly title?: string;
  readonly className?: string;
  readonly handshakeTimeoutMs?: number;
}

export type SurfacePressureLevel = "normal" | "moderate" | "critical";

export interface SurfacePressureBudget {
  readonly memory_bytes?: number;
  readonly cpu_ms_per_minute?: number;
  readonly network_bytes_per_minute?: number;
  readonly persistent_storage_bytes?: number;
  readonly realtime_connections?: number;
}

export interface SurfacePressureSignal {
  readonly level: SurfacePressureLevel;
  readonly budget?: SurfacePressureBudget;
  readonly reason?: string;
}

export interface MountedSurface {
  /** Resolves after confirmation and document registration, or immediately for a Static Surface. */
  readonly ready: Promise<void>;
  readonly plan: SurfacePlan;
  readonly frame: SurfaceFrame;
  readonly lifecycle: SurfaceLifecycleController;
  readonly surfaceOrigin: string;
  readonly surfaceSessionId: string;
  /** A disposed mount cannot reactivate; restore a checkpoint through a fresh mount. */
  activate(): void;
  /** Removes the iframe and closes its bridge. This does not pause browser execution. */
  suspend(reason?: string): void;
  evict(reason?: string): void;
  applyPressure(signal: SurfacePressureSignal): void;
  unmount(reason?: string): void;
}

export class BrowserSurfaceHost {
  mount(options: SurfaceMountOptions): MountedSurface {
    assertReadyPlan(options.plan);
    assertBrowserTarget(options.plan.surface.target);
    const registerDocument = options.registerDocument;
    if (options.plan.surface.target !== "Static" && typeof registerDocument !== "function") {
      throw new Error("executable Babble Surfaces require a document registration callback");
    }

    const hostWindow = options.window ?? globalWindow();
    const document = options.document ?? globalDocument();
    const hostOrigin = options.hostOrigin ?? hostWindow.location?.origin;
    if (!hostOrigin) {
      throw new Error("Babble Surface host requires an explicit host origin outside a browser window");
    }

    const bundleEntry = verifiedBundleEntry(
      options.plan, options.surfaceSessionId, hostOrigin, hostWindow.location?.origin, options.surfaceOrigin,
    );
    const entry = bundleEntry ?? surfaceEntry(options.plan.surface.entry, hostOrigin);
    const surfaceOrigin = options.surfaceOrigin ?? entry.origin;
    const frame = document.createElement("iframe");
    configureFrame(frame, options.plan, entry, options.title, options.className, bundleEntry !== null);
    const allowedOrigins = bundleEntry !== null || !options.plan.sandbox.isolated_origin
      ? [surfaceOrigin] : [surfaceOrigin, "null"];
    const lifecycle = createSurfaceLifecycle(options.plan.lifecycle, options.plan.budget);
    let bridge: SurfaceChannelHost | null = null;
    if (options.plan.surface.target !== "Static") {
      const surfaceDocumentId = crypto.randomUUID();
      bridge = new SurfaceChannelHost(
        hostWindow, frame, allowedOrigins, scopedSurfaceDispatch(options, surfaceOrigin, surfaceDocumentId),
        (signal) => registerDocument(surfaceDocumentId, signal), options.handshakeTimeoutMs,
        (error) => mounted.evict(error.message),
      );
    }
    const mounted = new MountedBrowserSurface({
      plan: options.plan,
      frame,
      bridge,
      lifecycle,
      surfaceOrigin,
      surfaceSessionId: options.surfaceSessionId,
    });
    try {
      options.container.appendChild(frame);
      if (lifecycle.state === "cold") {
        lifecycle.transition("prefetched", "surface iframe mounted");
      }
      return mounted;
    } catch (error) {
      mounted.unmount("surface mount failed");
      throw error;
    }
  }
}

class MountedBrowserSurface implements MountedSurface {
  readonly ready: Promise<void>;
  readonly plan: SurfacePlan;
  readonly frame: SurfaceFrame;
  readonly lifecycle: SurfaceLifecycleController;
  readonly surfaceOrigin: string;
  readonly surfaceSessionId: string;
  readonly #bridge: SurfaceChannelHost | null;
  readonly #unsubscribeLifecycle: () => void;
  readonly #unsubscribeBudget: () => void;
  #mounted = true;
  #loaded = false;
  #onLoad = (): void => {
    if (this.#loaded) {
      // WindowProxy identity survives navigation. Revoke the old document's bridge.
      this.evict("surface document navigated; a fresh mount is required");
    }
    this.#loaded = true;
  };

  constructor(input: {
    readonly plan: SurfacePlan;
    readonly frame: SurfaceFrame;
    readonly bridge: SurfaceChannelHost | null;
    readonly lifecycle: SurfaceLifecycleController;
    readonly surfaceOrigin: string;
    readonly surfaceSessionId: string;
  }) {
    this.plan = input.plan;
    this.frame = input.frame;
    this.#bridge = input.bridge;
    this.ready = input.bridge?.ready ?? Promise.resolve();
    this.lifecycle = input.lifecycle;
    this.surfaceOrigin = input.surfaceOrigin;
    this.surfaceSessionId = input.surfaceSessionId;
    this.#unsubscribeLifecycle = this.lifecycle.onChange(() => {
      syncFrameLifecycle(this.frame, this.lifecycle);
      if (this.lifecycle.state === "suspended" || this.lifecycle.state === "evicted") {
        this.#disposeFrame();
      }
      if (this.lifecycle.state === "evicted") {
        this.#unsubscribeLifecycle();
        this.#unsubscribeBudget();
      }
    });
    this.#unsubscribeBudget = this.lifecycle.onBudgetChange(() => {
      syncFrameBudget(this.frame, this.lifecycle);
    });
    syncFrameLifecycle(this.frame, this.lifecycle);
    syncFrameBudget(this.frame, this.lifecycle);
    this.frame.addEventListener("load", this.#onLoad);
  }

  activate(): void {
    if (!this.#mounted) {
      throw new Error("Babble Surface execution stopped; restore a checkpoint through a fresh mount");
    }
    if (this.lifecycle.state === "prefetched") {
      this.lifecycle.transition("warm", "surface resources warmed");
    }
    if (this.lifecycle.state === "warm" || this.lifecycle.state === "cold") {
      if (this.lifecycle.state === "cold") {
        this.lifecycle.transition("prefetched", "surface activated before prefetch");
        this.lifecycle.transition("warm", "surface resources warmed");
      }
      this.lifecycle.transition("active", "surface activated");
    }
  }

  suspend(reason = "surface suspended"): void {
    if (this.lifecycle.state !== "evicted" && this.lifecycle.state !== "suspended") {
      this.lifecycle.transition("suspended", reason);
    }
  }

  evict(reason = "surface evicted"): void {
    this.unmount(reason);
  }

  applyPressure(signal: SurfacePressureSignal): void {
    const reason = signal.reason ?? `surface ${signal.level} resource pressure`;
    const nextBudget = reducedBudget(this.lifecycle.budget, signal.budget);
    if (nextBudget !== null) {
      this.lifecycle.updateBudget(nextBudget, reason);
    }
    if (signal.level === "moderate") {
      this.suspend(reason);
    }
    if (signal.level === "critical") {
      this.evict(reason);
    }
  }

  unmount(reason = "surface unmounted"): void {
    this.#disposeFrame();
    if (this.lifecycle.state !== "evicted") {
      this.lifecycle.transition("evicted", reason);
    }
    syncFrameLifecycle(this.frame, this.lifecycle);
    this.#unsubscribeLifecycle();
    this.#unsubscribeBudget();
  }

  #disposeFrame(): void {
    if (!this.#mounted) {
      return;
    }
    this.#mounted = false;
    this.frame.removeEventListener("load", this.#onLoad);
    this.#bridge?.close(undefined, false);
    this.frame.remove();
  }
}

export function createBrowserSurfaceHost(): BrowserSurfaceHost {
  return new BrowserSurfaceHost();
}

function configureFrame(
  frame: SurfaceFrame,
  plan: SurfacePlan,
  entry: URL,
  title: string | undefined,
  className: string | undefined,
  verifiedBundle: boolean,
): void {
  frame.src = entry.href;
  frame.title = title ?? `${plan.surface.role} Surface ${plan.object_id}`;
  frame.loading = "eager";
  frame.referrerPolicy = "no-referrer";
  // Verified bundles receive their enforced CSP from the gateway response.
  // An embedded-CSP attribute would introduce a separate browser negotiation.
  if (!verifiedBundle) {
    const csp = iframeCsp(plan.sandbox.csp);
    if (csp.length > 0) {
      frame.setAttribute("csp", csp);
    }
  }
  frame.setAttribute("allow", iframePermissionsPolicy(plan));
  frame.setAttribute("data-babble-object", plan.object_id);
  frame.setAttribute("data-babble-surface-role", plan.surface.role);
  frame.setAttribute("data-babble-surface-target", plan.surface.target);
  if (!plan.sandbox.host_cookies) {
    frame.setAttribute("credentialless", "");
  }
  if (className !== undefined && className.length > 0) {
    frame.setAttribute("class", className);
  }
  for (const token of sandboxTokens(plan, verifiedBundle)) {
    frame.sandbox.add(token);
  }
}

function syncFrameLifecycle(frame: SurfaceFrame, lifecycle: SurfaceLifecycleController): void {
  frame.setAttribute("data-babble-lifecycle", lifecycle.state);
  frame.setAttribute("data-babble-suspended", lifecycle.state === "suspended" ? "true" : "false");
}

function syncFrameBudget(frame: SurfaceFrame, lifecycle: SurfaceLifecycleController): void {
  const budget = lifecycle.budget;
  if (!budget) {
    return;
  }
  frame.setAttribute("data-babble-memory-budget", String(budget.memory_bytes));
  frame.setAttribute("data-babble-cpu-budget", String(budget.cpu_ms_per_minute));
  frame.setAttribute("data-babble-network-budget", String(budget.network_bytes_per_minute));
  frame.setAttribute("data-babble-realtime-budget", String(budget.realtime_connections));
  frame.setAttribute("data-babble-storage-budget", String(budget.persistent_storage_bytes));
  frame.setAttribute("data-babble-gpu-expected", budget.gpu_expected ? "true" : "false");
}

function iframeCsp(csp: string): string {
  return csp
    .split(";")
    .map((directive) => directive.trim())
    .filter((directive) => directive.length > 0 && !directive.toLowerCase().startsWith("frame-ancestors"))
    .join("; ");
}

function sandboxTokens(plan: SurfacePlan, verifiedBundle: boolean): string[] {
  const tokens = new Set<string>();
  if (plan.surface.target === "Web" || plan.surface.target === "WebGpu") {
    tokens.add("allow-scripts");
  }
  if (verifiedBundle || !plan.sandbox.isolated_origin) {
    tokens.add("allow-same-origin");
  }
  if (plan.sandbox.top_navigation) {
    tokens.add("allow-top-navigation-by-user-activation");
  }
  return [...tokens].sort();
}

function iframePermissionsPolicy(plan: SurfacePlan): string {
  const denied = [
    "accelerometer",
    "ambient-light-sensor",
    "autoplay",
    "bluetooth",
    "camera",
    "clipboard-read",
    "clipboard-write",
    "display-capture",
    "encrypted-media",
    "geolocation",
    "gyroscope",
    "hid",
    "idle-detection",
    "local-fonts",
    "magnetometer",
    "microphone",
    "midi",
    "payment",
    "picture-in-picture",
    "publickey-credentials-get",
    "serial",
    "speaker-selection",
    "storage-access",
    "usb",
    "xr-spatial-tracking",
  ];
  const policies = denied.map((feature) => `${feature} 'none'`);
  const webGpuPolicy = plan.surface.target === "WebGpu" && plan.budget.gpu_expected ? "webgpu *" : "webgpu 'none'";
  return [...policies, webGpuPolicy].sort().join("; ");
}

function reducedBudget(
  current: SurfaceLifecycleController["budget"],
  requested: SurfacePressureBudget | undefined,
): SurfaceLifecycleController["budget"] {
  if (!current || requested === undefined) {
    return current;
  }
  return {
    ...current,
    memory_bytes: reducedInteger(current.memory_bytes, requested.memory_bytes, "memory_bytes"),
    cpu_ms_per_minute: reducedInteger(current.cpu_ms_per_minute, requested.cpu_ms_per_minute, "cpu_ms_per_minute"),
    network_bytes_per_minute: reducedInteger(
      current.network_bytes_per_minute,
      requested.network_bytes_per_minute,
      "network_bytes_per_minute",
    ),
    persistent_storage_bytes: reducedInteger(
      current.persistent_storage_bytes,
      requested.persistent_storage_bytes,
      "persistent_storage_bytes",
    ),
    realtime_connections: reducedInteger(current.realtime_connections, requested.realtime_connections, "realtime_connections"),
  };
}

function reducedInteger(current: number, requested: number | undefined, field: string): number {
  if (requested === undefined) {
    return current;
  }
  if (!Number.isSafeInteger(requested) || requested < 0) {
    throw new Error(`invalid Babble Surface pressure budget ${field}: ${requested}`);
  }
  if (requested > current) {
    throw new Error(`Babble Surface pressure budget cannot increase ${field}: ${current} -> ${requested}`);
  }
  return requested;
}

function assertReadyPlan(plan: SurfacePlan): void {
  if (plan.admission !== "ready") {
    throw new Error(`cannot mount Babble Surface with admission status: ${plan.admission}`);
  }
  if (!plan.sandbox.capability_bridge && plan.surface.target !== "Static") {
    throw new Error("executable Babble Surfaces require a capability bridge");
  }
  if (plan.lifecycle === "suspended" || plan.lifecycle === "evicted") {
    throw new Error(`cannot mount Babble Surface with stopped lifecycle: ${plan.lifecycle}`);
  }
}

function assertBrowserTarget(target: string): asserts target is BrowserSurfaceTarget {
  if (target !== "Static" && target !== "Web" && target !== "WebGpu") {
    throw new Error(`Babble browser host cannot mount Surface target: ${target}`);
  }
}

function scopedSurfaceDispatch(options: SurfaceMountOptions, surfaceOrigin: string, surfaceDocumentId: string): BridgeDispatch {
  const objectId = options.plan.object_id;
  const surfaceSessionId = options.surfaceSessionId;
  const identityId = options.currentIdentityId ?? null;
  const grantedCapabilityIds = options.plan.capability_decisions
    .filter((decision) => decision.status === "granted" && decision.grant !== null && decision.grant !== undefined)
    .map((decision) => decision.grant?.id)
    .filter((grantId): grantId is string => typeof grantId === "string" && grantId.length > 0)
    .sort();

  return (request: RpcRequestEnvelope, context = { signal: new AbortController().signal }) => {
    const method = rpcCatalog.methods.find((candidate) => candidate.method === request.method);
    // The host's authenticated transport is not a delegation of its signing or
    // administration authority to an embedded application.
    if (!method || (!surfaceReadMethods.has(method.method) && !method.capability?.required)) {
      return {
        protocol: request.protocol,
        id: request.id,
        trace_id: request.trace_id ?? null,
        result: null,
        error: {
          code: "CAPABILITY_DENIED",
          message: "This operation requires the host's own account controls",
          retryable: false,
          retry_after_ms: null,
          details: null,
        },
      };
    }
    return options.dispatch(
      bindSurfaceRequest(
        request,
        objectId,
        surfaceSessionId,
        surfaceOrigin,
        grantedCapabilityIds,
        identityId,
      ),
      { signal: context.signal, surfaceDocumentId },
    );
  };
}

const surfaceReadMethods: ReadonlySet<string> = new Set([
  "babble.object.get.v1", "babble.media.blob.get.v1", "babble.graph.evidence.v1",
  "babble.graph.traverse.v1", "babble.social.replies.list.v1",
  "babble.social.reactions.summary.v1", "babble.social.reactions.record.v1",
  "babble.judgment.object.list.v1", "babble.judgment.definitions.list.v1",
  "babble.judgment.providers.list.v1", "babble.search.objects.v1", "babble.lenses.list.v1",
  "babble.discovery.candidates.v1", "babble.capabilities.list.v1", "babble.capabilities.inspect.v1",
  "babble.runtime.surface.prepare.v1", "babble.runtime.surface.session.get.v1",
  "babble.runtime.surface.session.state.get.v1",
]);

function bindSurfaceRequest(
  request: RpcRequestEnvelope,
  objectId: string,
  surfaceSessionId: string,
  surfaceOrigin: string,
  grantedCapabilityIds: readonly string[],
  currentIdentityId: string | null,
): RpcRequestEnvelope {
  return {
    ...request,
    binding: {
      object_id: objectId,
      surface_session_id: surfaceSessionId,
      runtime_id: request.binding.runtime_id || "babble-browser-surface-host",
      origin: surfaceOrigin,
      capability_grants: [...grantedCapabilityIds],
      identity_id: currentIdentityId,
    },
  };
}

function surfaceEntry(entry: string, hostOrigin: string): URL {
  const url = new URL(entry, hostOrigin);
  if (url.protocol !== "https:" && url.protocol !== "http:") {
    throw new Error(`Babble browser host requires an HTTP(S) Surface entry: ${entry}`);
  }
  return url;
}

function globalWindow(): SurfaceHostWindow {
  if (typeof window === "undefined") {
    throw new Error("Babble Surface host requires a browser window or explicit window adapter");
  }
  return window;
}

function globalDocument(): SurfaceDocument {
  if (typeof document === "undefined") {
    throw new Error("Babble Surface host requires a browser document or explicit document adapter");
  }
  return document;
}
