import type { ProtocolTypes } from "@babel-protocol/sdk";
import { Accounts, type AccountSession } from "./accounts";

export type ReactionValue = ProtocolTypes["graph.ReactionValue"];
export type ReactionState = ProtocolTypes["graph.ReactionState"];
export type ReactionSummary = ProtocolTypes["graph.ReactionSummary"];
export interface ReactionIntent {
  readonly value: ReactionValue;
  readonly expected_revision: number;
  readonly idempotency_key: string;
}
export const emptyReaction = (): ReactionValue => ({ appreciation: null, engagement: null, stance: null, certainty: null });
const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const safe = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const id = (value: unknown, prefix: string): value is string => typeof value === "string" && new RegExp(`^${prefix}_[a-f0-9]{64}$`).test(value);
const invalid = () => new Error("The node returned an invalid reaction response. Refresh and try again.");

export function parseReactionValue(value: unknown): ReactionValue {
  if (!record(value)) throw invalid();
  const { appreciation, engagement, stance, certainty } = value;
  if (!(appreciation === null || appreciation === "like" || appreciation === "dislike")
    || !(engagement === null || engagement === "engaging" || engagement === "not_engaging")
    || !(stance === null || stance === "support" || stance === "oppose" || stance === "uncertain")
    || !(certainty === null || (safe(certainty) && certainty <= 100 && stance !== null))) throw invalid();
  return { appreciation, engagement, stance, certainty };
}

export function parseReactionState(value: unknown, actor: string, object: string): ReactionState {
  if (!record(value) || !id(value.author_id, "id") || !id(value.object_id, "obj")
    || value.author_id !== actor || value.object_id !== object || !safe(value.revision)) throw invalid();
  const parsed = parseReactionValue(value.value);
  if (value.revision === 0 && Object.values(parsed).some((choice) => choice !== null)) throw invalid();
  return { author_id: value.author_id, object_id: value.object_id, revision: value.revision, value: parsed };
}

export function parseReactionSummary(value: unknown, object: string): ReactionSummary {
  if (!record(value) || !id(value.object_id, "obj") || value.object_id !== object) throw invalid();
  const keys = ["participants", "likes", "dislikes", "engaging", "not_engaging", "support", "oppose", "uncertain", "certainty_responses"] as const;
  for (const key of keys) if (!safe(value[key])) throw invalid();
  const count = (key: typeof keys[number]) => BigInt(value[key] as number);
  const appreciation = count("likes") + count("dislikes");
  const engagement = count("engaging") + count("not_engaging");
  const stance = count("support") + count("oppose") + count("uncertain");
  const participants = count("participants");
  if ([appreciation, engagement, stance].some((total) => total > participants)
    || count("certainty_responses") > stance || participants > appreciation + engagement + stance) throw invalid();
  return Object.fromEntries([["object_id", object], ...keys.map((key) => [key, value[key]])]) as ReactionSummary;
}

export class ReactionError extends Error {
  constructor(readonly status: number) {
    super(status === 401 ? "Sign in again to change your reaction."
      : status === 409 ? "Your reaction changed elsewhere. Choose again after refreshing."
      : status === 404 ? "This post is no longer available."
      : `Reaction request failed (${status}). Please retry.`);
  }
}

/** Captures a login, not merely an identity. A logout/login invalidates old work. */
export interface ReactionTransport {
  readonly actor: string | null;
  summary(object: string, signal: AbortSignal): Promise<ReactionSummary>;
  state(object: string, signal: AbortSignal): Promise<ReactionState>;
  update(object: string, intent: ReactionIntent, signal: AbortSignal): Promise<ReactionState>;
}

export class ReactionClient {
  constructor(private readonly accounts: Accounts,
    private readonly publicFetch: typeof fetch = globalThis.fetch.bind(globalThis),
    private readonly timeoutMs = 15_000) {}

  capture(valid: () => boolean): ReactionTransport {
    const session = this.accounts.current;
    const actor = session?.identity.id ?? null;
    const current = () => valid() && sameSession(session, this.accounts.current);
    const request = async (object: string, privateRead: boolean, signal: AbortSignal, intent?: ReactionIntent): Promise<unknown> => {
      if (!id(object, "obj")) throw invalid();
      if (!current()) throw new ReactionError(401);
      if (privateRead && !actor) throw new ReactionError(401);
      signal.throwIfAborted();
      const abort = new AbortController();
      const cancel = () => abort.abort(signal.reason);
      signal.addEventListener("abort", cancel, { once: true });
      if (signal.aborted) cancel();
      const timer = setTimeout(() => abort.abort(new Error("Reaction request timed out. Please retry.")), this.timeoutMs);
      try {
        const transport = privateRead ? this.accounts.authenticatedFetch : this.publicFetch;
        const response = await untilAbort(transport(new URL(`/objects/${encodeURIComponent(object)}/reactions${privateRead ? "/mine" : ""}`, this.accounts.origin), {
          method: intent ? "PUT" : "GET", ...(intent ? { body: JSON.stringify(intent) } : {}),
          headers: { accept: "application/json", ...(intent ? { "content-type": "application/json" } : {}) },
          signal: abort.signal, credentials: "omit", redirect: "error", cache: "no-store",
        }), abort.signal);
        if (!response.ok) { void response.body?.cancel().catch(() => undefined); throw new ReactionError(response.status); }
        const body = await reactionJson(response, abort.signal);
        if (!current()) throw new ReactionError(401);
        return body;
      } catch (cause) {
        if (record(cause) && typeof cause.status === "number") throw new ReactionError(cause.status);
        throw cause;
      } finally {
        clearTimeout(timer);
        signal.removeEventListener("abort", cancel);
      }
    };
    return {
      actor,
      summary: async (object, signal) => parseReactionSummary(await request(object, false, signal), object),
      state: async (object, signal) => parseReactionState(await request(object, true, signal), actor!, object),
      update: async (object, intent, signal) => {
        parseReactionValue(intent.value);
        if (!safe(intent.expected_revision) || !/^[!-~]{1,256}$/.test(intent.idempotency_key)) throw invalid();
        return parseReactionState(await request(object, true, signal, intent), actor!, object);
      },
    };
  }
}

function sameSession(a: AccountSession | null, b: AccountSession | null): boolean {
  return a?.token === b?.token && a?.identity.id === b?.identity.id;
}

function untilAbort<T>(operation: Promise<T>, signal: AbortSignal): Promise<T> {
  return new Promise((resolve, reject) => {
    const cancel = () => reject(signal.reason ?? new Error("Reaction request cancelled."));
    if (signal.aborted) cancel();
    else signal.addEventListener("abort", cancel, { once: true });
    operation.then(resolve, reject).finally(() => signal.removeEventListener("abort", cancel));
  });
}

async function reactionJson(response: Response, signal: AbortSignal): Promise<unknown> {
  const reader = response.body?.getReader();
  if (!reader) throw invalid();
  let text = "", size = 0;
  const decoder = new TextDecoder("utf-8", { fatal: true });
  try {
    for (;;) {
      const { value, done } = await untilAbort(reader.read(), signal);
      if (done) break;
      size += value.byteLength;
      if (size > 16_384) throw invalid();
      text += decoder.decode(value, { stream: true });
    }
    return JSON.parse(text + decoder.decode()) as unknown;
  } finally {
    // Cancellation must not wait for an uncooperative remote stream.
    void reader.cancel().catch(() => undefined);
    reader.releaseLock();
  }
}

export interface ReactionViewState {
  readonly object: string | null;
  readonly actor: string | null;
  readonly generation: number;
  readonly summary: ReactionSummary | null;
  readonly state: ReactionState | null;
  readonly busy: boolean;
  readonly retry: boolean;
  readonly confirmation: ReactionValue | null;
  readonly message: string;
  readonly needsRefresh: boolean;
}

/** One active Object. Uncertain writes survive swipes, never account changes. */
export class Reactions {
  private generation = 0;
  private sessionGeneration = 0;
  private operation: AbortController | null = null;
  private source: ReactionTransport | null = null;
  private readonly intents = new Map<string, ReactionIntent>();
  private consent = false;
  private disposed = false;
  private selectedSession: AccountSession | null = null;
  private selectedSessionGeneration = -1;
  view: ReactionViewState = { object: null, actor: null, generation: 0, summary: null, state: null,
    busy: false, retry: false, confirmation: null, message: "", needsRefresh: false };

  constructor(private readonly accounts: Accounts, private readonly changed: (state: ReactionViewState) => void,
    private readonly client = new ReactionClient(accounts), private readonly key = () => crypto.randomUUID()) {
    accounts.addEventListener("change", this.accountChanged);
  }

  private readonly accountChanged = (): void => {
    ++this.sessionGeneration;
    this.intents.clear();
    this.consent = false;
    void this.select(this.view.object);
  };

  async select(object: string | null, session: AccountSession | null = this.accounts.current): Promise<void> {
    if (this.disposed) return;
    if (!sameSession(session, this.accounts.current)) throw new ReactionError(401);
    if (object === this.view.object && this.selectedSessionGeneration === this.sessionGeneration
      && sameSession(session, this.selectedSession)) return;
    this.operation?.abort();
    this.selectedSession = session;
    this.selectedSessionGeneration = this.sessionGeneration;
    const generation = ++this.generation;
    const sessionGeneration = this.sessionGeneration;
    this.source = this.client.capture(() => sessionGeneration === this.sessionGeneration && !this.disposed);
    this.view = { object, actor: this.source.actor, generation, summary: null, state: null, busy: false,
      retry: object !== null && this.intents.has(object), confirmation: null, message: "", needsRefresh: false };
    this.changed(this.view);
    if (object) await this.refresh();
  }

  async refresh(): Promise<void> {
    if (!this.view.object || this.view.busy || this.disposed) return;
    const operation = this.start("Loading reactions...");
    await this.read(operation, "");
  }

  async publish(value: ReactionValue): Promise<void> {
    if (!this.view.actor || !this.view.object || !this.view.state || this.view.busy || this.view.retry || this.disposed) return;
    const parsed = parseReactionValue(value);
    if (!this.consent) {
      this.emit({ confirmation: parsed, message: "" });
      return;
    }
    await this.save({ value: parsed, expected_revision: this.view.state.revision, idempotency_key: this.key() });
  }

  async confirm(): Promise<void> {
    const value = this.view.confirmation;
    if (!value || this.view.busy) return;
    this.consent = true;
    this.emit({ confirmation: null });
    await this.publish(value);
  }

  cancelConfirmation(): void { this.emit({ confirmation: null }); }

  async retry(): Promise<void> {
    const intent = this.view.object ? this.intents.get(this.view.object) : null;
    if (intent && this.view.actor && !this.view.busy && !this.disposed) await this.save(intent);
  }

  dispose(): void {
    this.disposed = true;
    ++this.generation;
    ++this.sessionGeneration;
    this.operation?.abort();
    this.intents.clear();
    this.accounts.removeEventListener("change", this.accountChanged);
  }

  private async save(intent: ReactionIntent): Promise<void> {
    const object = this.view.object!;
    this.intents.set(object, intent);
    this.emit({ retry: true, confirmation: null });
    const operation = this.start("Publishing reaction...");
    let failure: unknown;
    try { await operation.source.update(object, intent, operation.abort.signal); }
    catch (cause) { failure = cause; }
    if (!this.current(operation)) return;
    const rejected = failure instanceof ReactionError && failure.status >= 400 && failure.status < 500;
    if (!failure || rejected) this.intents.delete(object);
    this.emit({ state: null, summary: null, retry: this.intents.has(object) });
    const message = !failure ? "Reaction saved."
      : failure instanceof ReactionError && failure.status === 409 ? "Your reaction changed elsewhere; choose again after loading current choices."
      : rejected && failure instanceof ReactionError ? failure.message : "Could not confirm publication. Retry the same change to confirm it.";
    // PUT may return a historic durable receipt. Only fresh reads drive the UI.
    await this.read(operation, message);
  }

  private start(message: string) {
    const abort = new AbortController();
    this.operation?.abort();
    this.operation = abort;
    const operation = { abort, generation: this.generation, object: this.view.object!, source: this.source! };
    this.emit({ busy: true, confirmation: null, message });
    return operation;
  }

  private current(operation: { abort: AbortController; generation: number }): boolean {
    return !this.disposed && this.generation === operation.generation && !operation.abort.signal.aborted;
  }

  private async read(operation: { abort: AbortController; generation: number; object: string; source: ReactionTransport }, message: string): Promise<void> {
    let failed = false;
    try {
      const summary = await operation.source.summary(operation.object, operation.abort.signal);
      if (!this.current(operation)) return;
      this.emit({ summary });
    } catch {
      if (!this.current(operation)) return;
      failed = true;
      this.emit({ summary: null });
    }
    if (!this.current(operation)) return;
    if (operation.source.actor) {
      try {
        const state = await operation.source.state(operation.object, operation.abort.signal);
        if (!this.current(operation)) return;
        this.emit({ state });
      } catch {
        if (!this.current(operation)) return;
        failed = true;
        this.emit({ state: null });
      }
    }
    if (this.current(operation)) this.emit({ busy: false, needsRefresh: failed,
      message: failed ? `${message ? `${message} ` : ""}Current reactions could not be loaded. Refresh to try again.`
        : message || (this.view.retry ? "Publication is unconfirmed. Retry the same change to confirm it." : "") });
  }

  private emit(patch: Partial<ReactionViewState>): void {
    this.view = { ...this.view, ...patch };
    this.changed(this.view);
  }
}
