import type { ProtocolTypes } from "@babble-protocol/sdk";
import { identity, object, profileJson } from "./profile-response";

export type FollowState = ProtocolTypes["node.FollowState"];
export type FollowingPage = ProtocolTypes["node.FollowingPage"];
export type FollowListPage = ProtocolTypes["node.FollowListPage"];
export type FollowingQuery = ProtocolTypes["node.FollowingQuery"];
export interface FollowIntent {
  readonly following: boolean;
  readonly expected_revision: number;
  readonly idempotency_key: string;
}
const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const validId = (value: unknown): value is string => typeof value === "string" && /^id_[a-f0-9]{64}$/.test(value);
const cursor = (value: unknown): value is string | null => value === null || (typeof value === "string" && value.length > 0 && value.length <= 2048);
const invalid = () => new Error("The node returned an invalid Following response. Refresh and try again.");

export class FollowingError extends Error {
  constructor(readonly status: number) {
    super(status === 401 ? "Sign in to see Following."
      : status === 404 ? "This author is not available on this node."
      : status === 409 ? "Following changed. Refresh to see the latest state."
      : status === 400 ? "This Following request is no longer valid. Refresh and try again."
      : `Following request failed (${status}). Please retry.`);
  }
}

export function parseFollowState(value: unknown, owner: string, target: string): FollowState {
  if (!record(value) || !validId(value.author_id) || !validId(value.target_id)
    || value.author_id !== owner || value.target_id !== target || typeof value.following !== "boolean"
    || typeof value.revision !== "number" || !Number.isSafeInteger(value.revision) || value.revision < 0) throw invalid();
  return { author_id: value.author_id, target_id: value.target_id, following: value.following, revision: value.revision };
}

export function parseFollowingPage(value: unknown): FollowingPage {
  if (!record(value) || !Array.isArray(value.objects) || value.objects.length > 50 || !cursor(value.next_cursor)) throw invalid();
  const objects: FollowingPage["objects"][number][] = [];
  for (const entry of value.objects) {
    if (!record(entry) || !validId(entry.author) || !object(entry, entry.author)) throw invalid();
    objects.push(entry);
  }
  // Preserve the node's deterministic order, including its same-time tie breaker.
  if (objects.some((entry, index) => index > 0 && Date.parse(entry.created_at) > Date.parse(objects[index - 1]!.created_at))) throw invalid();
  return { objects, next_cursor: value.next_cursor };
}

export function parseFollowList(value: unknown): FollowListPage {
  if (!record(value) || !Array.isArray(value.identities) || value.identities.length > 50
    || !value.identities.every(identity) || !cursor(value.next_cursor)) throw invalid();
  return { identities: value.identities, next_cursor: value.next_cursor };
}

/** Host-only REST client. The private graph is never exposed to a Surface. */
export class FollowingClient {
  private readonly origin: URL;
  constructor(apiUrl: string, private readonly authenticatedFetch: typeof fetch) { this.origin = new URL(apiUrl); }

  async state(owner: string, target: string, signal: AbortSignal): Promise<FollowState> {
    return parseFollowState(await this.request(`/social/following/${encodeURIComponent(target)}`, signal), owner, target);
  }

  async update(owner: string, target: string, intent: FollowIntent, signal: AbortSignal): Promise<FollowState> {
    return parseFollowState(await this.request(`/social/following/${encodeURIComponent(target)}`, signal, {
      method: "PUT", body: JSON.stringify(intent),
    }), owner, target);
  }

  async feed(search: string, next: string | null, signal: AbortSignal): Promise<FollowingPage> {
    return parseFollowingPage(await this.request(this.pageUrl("/feed/following", next, search), signal));
  }

  async list(next: string | null, signal: AbortSignal): Promise<FollowListPage> {
    return parseFollowList(await this.request(this.pageUrl("/social/following", next), signal));
  }

  private pageUrl(path: string, next: string | null, search = ""): URL {
    const url = new URL(path, this.origin);
    url.searchParams.set("limit", "20");
    if (next !== null) url.searchParams.set("cursor", next);
    if (search.trim()) url.searchParams.set("search", search.trim());
    return url;
  }

  private async request(path: string | URL, signal: AbortSignal, init: RequestInit = {}): Promise<unknown> {
    try {
      const response = await this.authenticatedFetch(new URL(path, this.origin), {
        ...init, credentials: "omit", redirect: "error", cache: "no-store",
        headers: { accept: "application/json", ...(init.body ? { "content-type": "application/json" } : {}) },
        signal: AbortSignal.any([signal, AbortSignal.timeout(15_000)]),
      });
      if (!response.ok) throw new FollowingError(response.status);
      return await profileJson(response);
    } catch (cause) {
      if (record(cause) && typeof cause.status === "number") throw new FollowingError(cause.status);
      if (signal.aborted) throw cause;
      throw new Error("Could not load Following. Check your connection and retry.");
    }
  }
}

export interface FollowView {
  readonly owner: string | null;
  readonly target: string | null;
  readonly state: FollowState | null;
  readonly pending: boolean;
  readonly retry: boolean | null;
  readonly message: string;
}

/** CAS intent survives uncertain transport failure, but never an account change. */
export class AuthorFollow {
  private generation = 0;
  private controller: AbortController | null = null;
  private readonly intents = new Map<string, FollowIntent>();
  view: FollowView = { owner: null, target: null, state: null, pending: false, retry: null, message: "" };
  constructor(private readonly source: Pick<FollowingClient, "state" | "update">,
    private readonly changed: (view: FollowView) => void,
    private readonly mutated: () => void,
    private readonly key: () => string = () => crypto.randomUUID()) {}

  show(owner: string | null, target: string | null): void {
    this.controller?.abort();
    ++this.generation;
    if (owner !== this.view.owner) this.intents.clear();
    this.view = { owner, target, state: null, pending: false, retry: target ? this.intents.get(target)?.following ?? null : null, message: "" };
    this.changed(this.view);
    if (owner && target && owner !== target) void this.read();
  }

  async read(): Promise<void> {
    const { owner, target } = this.view;
    if (!owner || !target || owner === target || this.view.pending) return;
    const generation = this.generation;
    const controller = new AbortController();
    this.controller = controller;
    this.publish({ pending: true, message: "Loading follow state..." });
    try {
      const state = await this.source.state(owner, target, controller.signal);
      if (generation === this.generation) this.publish({ state, message: "" });
    } catch (cause) {
      if (generation === this.generation) this.publish({ message: followingMessage(cause) });
    } finally {
      if (generation === this.generation) this.publish({ pending: false });
    }
  }

  async toggle(): Promise<void> {
    const { owner, target, state } = this.view;
    if (!owner || !target || owner === target || this.view.pending) return;
    if (!state && !this.intents.has(target)) return this.read();
    const intent = this.intents.get(target) ?? {
      following: !state!.following, expected_revision: state!.revision, idempotency_key: this.key(),
    };
    this.intents.set(target, intent);
    const generation = this.generation;
    const controller = new AbortController();
    this.controller = controller;
    const current = () => generation === this.generation && !controller.signal.aborted;
    this.publish({ pending: true, retry: intent.following, message: "Saving follow state..." });
    let failure: unknown = null;
    let accepted = false;
    try {
      await this.source.update(owner, target, intent, controller.signal);
      accepted = true;
    } catch (cause) { failure = cause; }
    if (!current()) return;
    const conflict = failure instanceof FollowingError && failure.status === 409;
    const rejected = failure instanceof FollowingError && failure.status >= 400 && failure.status < 500;
    if (accepted || rejected) this.intents.delete(target);
    // A durable retry receipt may describe an old revision. Only a fresh GET
    // may drive the button, even when the PUT timed out or returned a conflict.
    try {
      const fresh = await this.source.state(owner, target, controller.signal);
      if (!current()) return;
      this.publish({ state: fresh, retry: this.intents.get(target)?.following ?? null,
        message: conflict ? "Follow state changed elsewhere. Current state refreshed; choose again."
          : failure ? `${followingMessage(failure)}${rejected ? "" : " Retry the same change to confirm it."}` : "" });
    } catch (cause) {
      if (!current()) return;
      this.publish({ state: null, retry: this.intents.get(target)?.following ?? null,
        message: conflict ? "Follow state changed elsewhere. Refresh before choosing again."
          : accepted ? "Change saved. Refresh to load current follow state." : followingMessage(cause) });
    } finally {
      if (current()) {
        this.publish({ pending: false });
        this.mutated();
      }
    }
  }

  private publish(patch: Partial<FollowView>): void { this.view = { ...this.view, ...patch }; this.changed(this.view); }
}

export interface FollowingSlice<T> { readonly items: readonly T[]; readonly next: string | null; }
export interface FollowingView<T> extends FollowingSlice<T> {
  readonly phase: "idle" | "guest" | "loading" | "ready" | "error";
  readonly message: string;
  readonly restart: boolean;
}

/** Both private lists use the same account-scoped, snapshot-aware pagination. */
export class FollowingPages<T extends { readonly id: string }> {
  private generation = 0;
  private controller: AbortController | null = null;
  private owner: string | null = null;
  private query = "";
  private readonly cursors = new Set<string>();
  view: FollowingView<T> = { items: [], next: null, phase: "idle", message: "", restart: false };
  constructor(private readonly source: (cursor: string | null, query: string, signal: AbortSignal) => Promise<FollowingSlice<T>>,
    private readonly changed: (view: FollowingView<T>) => void) {}

  clear(): void {
    ++this.generation;
    this.controller?.abort();
    this.owner = null;
    this.cursors.clear();
    this.view = { items: [], next: null, phase: "idle", message: "", restart: false };
    this.changed(this.view);
  }

  async start(owner: string | null, query = ""): Promise<void> {
    this.clear();
    this.owner = owner;
    this.query = query;
    if (!owner) { this.publish({ phase: "guest", message: "Sign in to see Following." }); return; }
    await this.more();
  }

  async more(): Promise<void> {
    if (!this.owner || this.view.phase === "loading") return;
    if (this.view.restart) return this.start(this.owner, this.query);
    if (this.view.phase === "ready" && this.view.next === null) return;
    const generation = this.generation;
    const controller = new AbortController();
    this.controller = controller;
    const next = this.view.next;
    this.publish({ phase: "loading", message: "Loading Following..." });
    try {
      const page = await this.source(next, this.query, controller.signal);
      if (generation !== this.generation || controller.signal.aborted) return;
      if (page.next !== null && (page.next === next || this.cursors.has(page.next))) {
        this.publish({ restart: true });
        throw invalid();
      }
      if (next !== null) this.cursors.add(next);
      const unique = new Map(this.view.items.map((item) => [item.id, item]));
      for (const item of page.items) if (!unique.has(item.id)) unique.set(item.id, item);
      this.publish({ items: [...unique.values()], next: page.next, phase: "ready", message: "" });
    } catch (cause) {
      if (generation !== this.generation || controller.signal.aborted) return;
      this.publish({ phase: "error", message: followingMessage(cause),
        restart: this.view.restart || cause instanceof FollowingError && [400, 409].includes(cause.status) });
    }
  }

  private publish(patch: Partial<FollowingView<T>>): void { this.view = { ...this.view, ...patch }; this.changed(this.view); }
}

export function followingMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message.slice(0, 240) : "Could not load Following. Please retry.";
}
