import type { RpcOutput } from "./generated/protocol.js";

export type SurfaceLifecycleState = RpcOutput<"babble.runtime.surface.prepare.v1">["plan"]["lifecycle"];
export type SurfaceResourceBudget = RpcOutput<"babble.runtime.surface.session.get.v1">["session"]["budget"];
export type SurfaceRuntimeSession = RpcOutput<"babble.runtime.surface.session.get.v1">["session"];
export type SurfaceRuntimeEvent = RpcOutput<"babble.runtime.surface.session.transition.v1">["event"];

export interface SurfaceLifecycleEvent {
  readonly previous: SurfaceLifecycleState;
  readonly current: SurfaceLifecycleState;
  readonly reason: string | null;
  readonly at: Date;
}

export type SurfaceLifecycleHandler = (event: SurfaceLifecycleEvent) => void;

export interface SurfaceBudgetEvent {
  readonly previous: SurfaceResourceBudget | null;
  readonly current: SurfaceResourceBudget;
  readonly reason: string | null;
  readonly at: Date;
}

export type SurfaceBudgetHandler = (event: SurfaceBudgetEvent) => void;

const allowedTransitions: ReadonlyMap<SurfaceLifecycleState, ReadonlySet<SurfaceLifecycleState>> = new Map([
  ["cold", new Set(["prefetched", "warm", "active", "suspended", "evicted"])],
  ["prefetched", new Set(["warm", "active", "suspended", "evicted"])],
  ["warm", new Set(["active", "suspended", "evicted"])],
  ["active", new Set(["suspended", "evicted"])],
  ["suspended", new Set(["warm", "active", "evicted"])],
  ["evicted", new Set()],
]);

export class SurfaceLifecycleController {
  #state: SurfaceLifecycleState;
  #budget: SurfaceResourceBudget | null;
  #abortController: AbortController;
  #handlers = new Set<SurfaceLifecycleHandler>();
  #budgetHandlers = new Set<SurfaceBudgetHandler>();

  constructor(initial: SurfaceLifecycleState = "cold", budget: SurfaceResourceBudget | null = null) {
    this.#state = initial;
    this.#budget = budget;
    this.#abortController = new AbortController();
    if (initial === "evicted") {
      this.#abortController.abort(new Error("surface lifecycle initialized as evicted"));
    }
  }

  get state(): SurfaceLifecycleState {
    return this.#state;
  }

  get budget(): SurfaceResourceBudget | null {
    return this.#budget;
  }

  get signal(): AbortSignal {
    return this.#abortController.signal;
  }

  onChange(handler: SurfaceLifecycleHandler): () => void {
    this.#handlers.add(handler);
    return () => {
      this.#handlers.delete(handler);
    };
  }

  onBudgetChange(handler: SurfaceBudgetHandler): () => void {
    this.#budgetHandlers.add(handler);
    return () => {
      this.#budgetHandlers.delete(handler);
    };
  }

  transition(next: SurfaceLifecycleState, reason: string | null = null): SurfaceLifecycleEvent {
    if (next === this.#state) {
      return {
        previous: this.#state,
        current: this.#state,
        reason,
        at: new Date(),
      };
    }

    if (!allowedTransitions.get(this.#state)?.has(next)) {
      throw new Error(`invalid Babble Surface lifecycle transition: ${this.#state} -> ${next}`);
    }

    const previous = this.#state;
    this.#state = next;
    if (next === "evicted" && !this.#abortController.signal.aborted) {
      this.#abortController.abort(new Error(reason ?? "surface evicted"));
    }

    const event = {
      previous,
      current: next,
      reason,
      at: new Date(),
    };
    for (const handler of this.#handlers) {
      handler(event);
    }
    return event;
  }

  applySession(session: SurfaceRuntimeSession, reason: string | null = "host session snapshot"): void {
    if (session.lifecycle !== this.#state) {
      this.transition(session.lifecycle, reason);
    }
    this.updateBudget(session.budget, reason, new Date(session.updated_at));
  }

  applyRuntimeEvent(event: SurfaceRuntimeEvent): void {
    if (event.lifecycle !== this.#state) {
      this.transition(event.lifecycle, event.reason);
    }
    if (event.kind === "budget_changed") {
      this.updateBudget(event.budget, event.reason, new Date(event.at), event.previous_budget ?? null);
    }
  }

  updateBudget(
    next: SurfaceResourceBudget,
    reason: string | null = null,
    at: Date = new Date(),
    previous: SurfaceResourceBudget | null = this.#budget,
  ): SurfaceBudgetEvent {
    this.#budget = next;
    const event = {
      previous,
      current: next,
      reason,
      at,
    };
    for (const handler of this.#budgetHandlers) {
      handler(event);
    }
    return event;
  }
}

export function createSurfaceLifecycle(
  initial?: SurfaceLifecycleState,
  budget?: SurfaceResourceBudget | null,
): SurfaceLifecycleController {
  return new SurfaceLifecycleController(initial, budget ?? null);
}
