import type { BridgeDispatch, RpcOutput } from "@babble-protocol/sdk";
import { mountSurface, type BabbleFrontendClient } from "./protocol";

export type SurfacePlan = RpcOutput<"babble.runtime.surface.prepare.v1">["plan"];
export type SurfaceSession = RpcOutput<"babble.runtime.surface.session.start.v1">["session"];
type SurfaceLease = RpcOutput<"babble.runtime.surface.session.heartbeat.v1">["lease"];
export type SurfacePhase = "idle" | "preparing" | "blocked" | "mounting" | "active" | "error";
export interface SurfaceState {
  readonly phase: SurfacePhase;
  readonly objectId: string | null;
  readonly plan?: SurfacePlan;
  readonly session?: SurfaceSession;
  readonly message?: string;
}
export interface SurfaceOpen {
  readonly objectId: string;
  readonly container: HTMLElement;
  readonly currentIdentityId: string | null;
  readonly source: Pick<BabbleFrontendClient, "prepareSurface" | "startSurfaceSession" | "transitionSurfaceSession" | "heartbeatSurfaceSession" | "registerSurfaceDocument">;
  readonly dispatch: BridgeDispatch;
  readonly authorized: () => boolean;
}
export interface SurfaceHooks {
  readonly onState: (state: SurfaceState) => void;
  readonly onCleanupError: (error: Error) => void;
  readonly mount?: typeof mountSurface;
  readonly clock?: SurfaceClock;
}

export interface SurfaceClock {
  readonly now: () => number;
  readonly setTimeout: (callback: () => void, milliseconds: number) => ReturnType<typeof setTimeout>;
  readonly clearTimeout: (timer: ReturnType<typeof setTimeout>) => void;
}

export function createSurfaceClock(
  monotonic: () => number = () => performance.now(),
  wall: () => number = Date.now,
): SurfaceClock {
  let previousMonotonic = monotonic(), previousWall = wall(), elapsed = 0;
  return {
    now: () => {
      const nextMonotonic = monotonic(), nextWall = wall();
      // Some browser monotonic clocks pause during OS sleep. Wall jumps may
      // shorten a lease, but a clock correction must never extend it.
      elapsed += Math.max(0, nextMonotonic - previousMonotonic, nextWall - previousWall);
      previousMonotonic = nextMonotonic;
      previousWall = nextWall;
      return elapsed;
    },
    setTimeout: (callback, milliseconds) => setTimeout(callback, milliseconds),
    clearTimeout: (timer) => clearTimeout(timer),
  };
}
const heartbeatTimeoutMs = 30_000;

interface Opening {
  readonly input: SurfaceOpen;
  done: Promise<void>;
  stopped: boolean;
  reason: string;
  plan?: SurfacePlan;
  session?: SurfaceSession;
  mounted?: ReturnType<typeof mountSurface>;
  bridgeReady?: boolean;
  cancelReady?: () => void;
  unsubscribe?: () => void;
  cleanup?: Promise<void>;
  leaseDeadline?: number;
  heartbeatDeadline?: number;
  heartbeat?: AbortController;
  renewalTimer?: ReturnType<typeof setTimeout>;
  watchdogTimer?: ReturnType<typeof setTimeout>;
}

/** One operation owns each session; UI hooks are synchronous notifications. */
export class Surfaces {
  readonly #hooks: SurfaceHooks;
  readonly #mount: typeof mountSurface;
  readonly #clock: SurfaceClock;
  readonly #draining = new Set<Promise<void>>();
  #current: Opening | null = null;
  #state: SurfaceState = { phase: "idle", objectId: null };

  constructor(hooks: SurfaceHooks) {
    this.#hooks = hooks;
    this.#mount = hooks.mount ?? mountSurface;
    this.#clock = hooks.clock ?? createSurfaceClock();
  }

  get objectId(): string | null { return this.#current?.input.objectId ?? null; }
  get sessionId(): string | null { return this.#current?.session?.id ?? null; }
  get state(): SurfaceState { return this.#state; }
  get phase(): SurfacePhase { return this.#state.phase; }

  /** Recheck authority and monotonic deadlines when the host resumes visibility. */
  checkLease(): boolean {
    return this.#current !== null && this.#live(this.#current);
  }

  open(input: SurfaceOpen): Promise<void> {
    const previous = this.#current;
    const opening: Opening = {
      input, done: Promise.resolve(), stopped: false, reason: "Surface closed by browser host",
    };
    this.#current = opening;
    // Install completion before invoking external hooks, which may close or replace us.
    opening.done = Promise.resolve().then(() => this.#run(opening));
    if (previous) this.#stop(previous, "Surface replaced by browser host");
    this.#publish(opening, "preparing");
    return opening.done;
  }

  close(reason = "Surface closed by browser host"): Promise<void> {
    const opening = this.#current;
    this.#current = null;
    if (opening) this.#stop(opening, reason);
    if (!this.#current) this.#notify({ phase: "idle", objectId: null });
    return Promise.all([...this.#draining]).then(() => undefined);
  }

  async #run(opening: Opening): Promise<void> {
    const { input } = opening;
    try {
      if (!this.#live(opening)) return;
      opening.plan = await input.source.prepareSurface(input.objectId);
      if (!this.#live(opening)) return;
      this.#checkPlan(opening.plan, input.objectId);
      if (this.#blocked(opening)) return;

      // Even a stale successful start must retain its ID for remote cleanup.
      opening.session = await input.source.startSurfaceSession(input.objectId);
      opening.plan = opening.session.plan;
      if (!this.#live(opening)) return;
      this.#checkPlan(opening.plan, input.objectId);
      if (this.#blocked(opening)) return;
      if (typeof opening.session.id !== "string" || !opening.session.id.trim()) {
        throw new Error("Surface start returned an invalid session ID");
      }
      await this.#renew(opening);
      if (!this.#live(opening)) return;
      this.#publish(opening, "mounting");
      if (!this.#live(opening)) return;

      const existing = new Set(input.container.childNodes);
      const sessionId = opening.session.id;
      try {
        opening.mounted = this.#mount({
          container: input.container,
          plan: opening.plan,
          surfaceSessionId: opening.session.id,
          currentIdentityId: input.currentIdentityId,
          registerDocument: async (documentId, signal) => {
            this.#requireLive(opening);
            signal.throwIfAborted();
            await input.source.registerSurfaceDocument(sessionId, documentId, signal);
            signal.throwIfAborted();
            this.#requireLive(opening);
          },
          dispatch: async (request, context) => {
            this.#requireLive(opening);
            try {
              const response = await input.dispatch(request, context);
              this.#requireLive(opening);
              return response;
            } catch (cause) {
              this.#requireLive(opening);
              throw cause;
            }
          },
        });
      } catch (cause) {
        // A synchronous mount can append a frame before failing to return its handle.
        for (const child of Array.from(input.container.childNodes)) {
          if (!existing.has(child)) {
            try { input.container.removeChild(child); }
            catch (error) { this.#cleanupError(opening, "remove partially mounted frame", error); }
          }
        }
        throw cause;
      }
      // Observe readiness before hooks or teardown can reject it synchronously.
      const readiness = this.#readiness(opening);
      if (!this.#live(opening)) { this.#evictLocal(opening); return; }
      opening.unsubscribe = opening.mounted.lifecycle.onChange((event) => {
        if ((event.current === "suspended" || event.current === "evicted") && this.#live(opening)) {
          if (opening.bridgeReady) void this.close(event.reason ?? `Surface ${event.current}`);
          else {
            this.#stop(opening, "Surface bridge startup failed");
            this.#publish(opening, "error", bridgeFailure(event.reason ?? `Surface ${event.current} before its bridge was ready`));
          }
        }
      });
      opening.mounted.lifecycle.applySession(opening.session, "host session started");
      const failure = await readiness;
      delete opening.cancelReady;
      if (!this.#live(opening)) return;
      if (failure) throw new Error(bridgeFailure(failure.cause), { cause: failure.cause });
      opening.bridgeReady = true;

      for (const lifecycle of ["warm", "active"] as const) {
        if (!this.#live(opening)) return;
        const result = await input.source.transitionSurfaceSession(
          opening.session.id, lifecycle,
          lifecycle === "warm" ? "Surface iframe mounted by browser host" : "Surface visible in feed",
        );
        if (!this.#live(opening)) return;
        if (result.session.id !== opening.session.id || result.event.session_id !== opening.session.id
          || result.event.object_id !== input.objectId || result.event.lifecycle !== lifecycle
          || result.session.lifecycle !== lifecycle) {
          throw new Error("Surface transition returned a mismatched session or lifecycle");
        }
        opening.session = result.session;
        opening.plan = result.session.plan;
        this.#checkPlan(opening.plan, input.objectId);
        if (this.#blocked(opening)) return;
        opening.mounted.lifecycle.applyRuntimeEvent(result.event);
      }
      if (this.#live(opening)) this.#publish(opening, "active");
    } catch (cause) {
      if (this.#live(opening)) {
        this.#stop(opening, "Surface startup failed");
        this.#publish(opening, "error", message(cause));
      }
    } finally {
      if (opening.stopped) await this.#cleanup(opening);
    }
  }

  #checkPlan(plan: SurfacePlan, objectId: string): void {
    if (plan.object_id !== objectId) throw new Error("Surface plan returned a different Object");
  }

  #readiness(opening: Opening): Promise<{ cause: unknown } | undefined> {
    const ready = opening.mounted?.ready;
    if (!ready || typeof ready.then !== "function") {
      throw new Error(bridgeFailure("Surface mount returned a missing or invalid ready promise"));
    }
    // A stopped opening must drain even if a host never settles its ready promise.
    // Consume late rejection as data so it cannot escape after synchronous teardown.
    return new Promise((resolve) => {
      opening.cancelReady = () => resolve(undefined);
      Promise.resolve(ready).then(() => resolve(undefined), (cause) => resolve({ cause }));
      if (opening.stopped) opening.cancelReady();
    });
  }

  #blocked(opening: Opening): boolean {
    const plan = opening.plan;
    if (!plan || plan.admission === "ready") return false;
    this.#stop(opening, `Surface admission ${plan.admission}`);
    this.#publish(opening, "blocked", plan.blocked_reasons.join("; ") || `Surface admission ${plan.admission}`);
    return true;
  }

  #live(opening: Opening): boolean {
    if (this.#current !== opening || opening.stopped) return false;
    let authorized = false;
    try { authorized = opening.input.authorized(); }
    catch (cause) { this.#cleanupError(opening, "check Surface authority", cause); }
    if (this.#current !== opening || opening.stopped) return false;
    if (!authorized) {
      void this.close("Surface authority changed");
      return false;
    }
    const now = this.#clock.now();
    if (opening.leaseDeadline !== undefined && now >= opening.leaseDeadline) {
      this.#leaseFailed(opening, new Error("Surface session lease expired"));
      return false;
    }
    if (opening.heartbeatDeadline !== undefined && now >= opening.heartbeatDeadline) {
      this.#leaseFailed(opening, new Error("Surface session heartbeat timed out"));
      return false;
    }
    return this.#current === opening && !opening.stopped;
  }

  async #renew(opening: Opening): Promise<void> {
    if (!this.#live(opening) || opening.heartbeat || !opening.session) return;
    const sessionId = opening.session.id;
    const started = this.#clock.now();
    const controller = new AbortController();
    opening.heartbeat = controller;
    opening.heartbeatDeadline = started + heartbeatTimeoutMs;
    this.#watch(opening);
    let onAbort: () => void = () => {};
    try {
      // Cancellation settles locally even if a transport ignores its AbortSignal.
      const lease = await new Promise<SurfaceLease>((resolve, reject) => {
        onAbort = () => reject(new Error("Surface heartbeat cancelled"));
        controller.signal.addEventListener("abort", onAbort, { once: true });
        Promise.resolve(opening.input.source.heartbeatSurfaceSession(sessionId, controller.signal)).then(resolve, reject);
      });
      if (!this.#live(opening)) return;
      if (!validLease(lease, sessionId)) throw new Error("Surface heartbeat returned an invalid or mismatched lease");
      const deadline = started + lease.ttl_ms;
      if (this.#clock.now() >= deadline) throw new Error("Surface session lease expired before heartbeat arrived");
      opening.leaseDeadline = deadline;
      opening.renewalTimer = this.#clock.setTimeout(() => {
        delete opening.renewalTimer;
        void this.#renew(opening);
      }, Math.max(0, started + lease.renew_after_ms - this.#clock.now()));
    } catch (cause) {
      if (this.#live(opening)) this.#leaseFailed(opening, cause);
    } finally {
      controller.signal.removeEventListener("abort", onAbort);
      delete opening.heartbeat;
      delete opening.heartbeatDeadline;
      if (this.#live(opening)) this.#watch(opening);
    }
  }

  #watch(opening: Opening): void {
    if (opening.watchdogTimer !== undefined) this.#clock.clearTimeout(opening.watchdogTimer);
    const deadline = Math.min(opening.leaseDeadline ?? Infinity, opening.heartbeatDeadline ?? Infinity);
    opening.watchdogTimer = this.#clock.setTimeout(() => {
      delete opening.watchdogTimer;
      if (this.#live(opening)) this.#watch(opening);
    }, Math.max(0, deadline - this.#clock.now()));
  }

  #leaseFailed(opening: Opening, cause: unknown): void {
    this.#stop(opening, "Surface session lease lost");
    this.#publish(opening, "error", `${message(cause)}. Open the Surface again to retry.`);
    // Lease failure cannot wait for a hung startup transition or heartbeat.
    void this.#cleanup(opening);
  }

  #requireLive(opening: Opening): void {
    if (!this.#live(opening)) throw new Error("Surface is closed or its authority has changed");
  }

  #stop(opening: Opening, reason: string): void {
    if (opening.stopped) return;
    opening.stopped = true;
    opening.reason = reason;
    if (opening.renewalTimer !== undefined) this.#clock.clearTimeout(opening.renewalTimer);
    if (opening.watchdogTimer !== undefined) this.#clock.clearTimeout(opening.watchdogTimer);
    delete opening.renewalTimer;
    delete opening.watchdogTimer;
    opening.heartbeat?.abort();
    opening.cancelReady?.();
    delete opening.cancelReady;
    this.#evictLocal(opening);
    // Normal close drains startup first; lease loss also starts independent cleanup.
    const drain = opening.done.then(() => this.#cleanup(opening));
    this.#draining.add(drain);
    void drain.then(() => this.#draining.delete(drain));
  }

  #evictLocal(opening: Opening): void {
    const mounted = opening.mounted;
    if (!mounted) return;
    delete opening.mounted;
    const unsubscribe = opening.unsubscribe;
    delete opening.unsubscribe;
    try { unsubscribe?.(); }
    catch (cause) { this.#cleanupError(opening, "unsubscribe Surface lifecycle", cause); }
    try { mounted.evict(opening.reason); }
    catch (cause) {
      this.#cleanupError(opening, "evict local Surface", cause);
      try { mounted.unmount(opening.reason); }
      catch (error) { this.#cleanupError(opening, "unmount local Surface", error); }
      try { mounted.frame.remove(); }
      catch (error) { this.#cleanupError(opening, "remove Surface frame", error); }
    }
  }

  #cleanup(opening: Opening): Promise<void> {
    if (opening.cleanup) return opening.cleanup;
    this.#evictLocal(opening);
    opening.cleanup = Promise.resolve().then(async () => {
      const session = opening.session;
      if (!session) return;
      try {
        const result = await opening.input.source.transitionSurfaceSession(session.id, "evicted", opening.reason);
        if (result.session.id !== session.id || result.session.lifecycle !== "evicted"
          || result.event.session_id !== session.id || result.event.lifecycle !== "evicted") {
          throw new Error("Server did not confirm Surface session eviction");
        }
      } catch (cause) {
        this.#cleanupError(opening, "evict remote Surface session", cause);
      } finally {
        delete opening.session;
      }
    });
    return opening.cleanup;
  }

  #publish(opening: Opening, phase: SurfacePhase, detail?: string): void {
    if (this.#current !== opening) return;
    this.#notify({
      phase, objectId: opening.input.objectId,
      ...(opening.plan ? { plan: opening.plan } : {}),
      ...(opening.session ? { session: opening.session } : {}),
      ...(detail === undefined ? {} : { message: detail }),
    });
  }

  #notify(state: SurfaceState): void {
    this.#state = state;
    try { this.#hooks.onState(state); }
    catch (cause) { this.#report(new Error(`Could not render Surface ${state.phase}: ${message(cause)}`, { cause })); }
  }

  #cleanupError(opening: Opening, action: string, cause: unknown): void {
    this.#report(new Error(`Could not ${action} (${opening.session?.id ?? opening.input.objectId}): ${message(cause)}`, { cause }));
  }

  #report(error: Error): void {
    try { this.#hooks.onCleanupError(error); }
    catch (cause) { console.error("Surface error hook failed", cause, error); }
  }
}

function message(cause: unknown): string {
  return cause instanceof Error ? cause.message : String(cause);
}

function bridgeFailure(cause: unknown): string {
  return `Surface could not establish a secure bridge: ${message(cause)}. Reopen the Surface or update it to a compatible version.`;
}

function validLease(lease: SurfaceLease, sessionId: string): boolean {
  if (!lease || lease.session_id !== sessionId || typeof lease.expires_at !== "string"
    || !/^\d{4}-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])[Tt]([01]\d|2[0-3]):[0-5]\d:[0-5]\d(?:\.\d+)?(?:[Zz]|[+-]([01]\d|2[0-3]):[0-5]\d)$/.test(lease.expires_at)
    || !Number.isFinite(Date.parse(lease.expires_at))) return false;
  const year = Number(lease.expires_at.slice(0, 4));
  const month = Number(lease.expires_at.slice(5, 7));
  const day = Number(lease.expires_at.slice(8, 10));
  const february = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0) ? 29 : 28;
  const days = [31, february, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  if (day > (days[month - 1] ?? 0)) return false;
  return Number.isSafeInteger(lease.ttl_ms) && lease.ttl_ms > 0 && lease.ttl_ms <= 60_000
    && Number.isSafeInteger(lease.renew_after_ms) && lease.renew_after_ms > 0
    && lease.renew_after_ms < lease.ttl_ms;
}
