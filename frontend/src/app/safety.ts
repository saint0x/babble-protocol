import type { ProtocolTypes } from "@babble-protocol/sdk";
import { identity } from "./profile-response";

export type SafetyIdentity = ProtocolTypes["identity.Identity"];
export type SafetyState = ProtocolTypes["node.SafetyState"];
export type SafetySnapshot = ProtocolTypes["node.SafetySnapshot"];
export type SafetyIntent = ProtocolTypes["api.SetSafetyRequest"];
export type SafetySource = Pick<SafetyClient, "state" | "snapshot" | "update"> & Partial<Pick<SafetyClient, "identity">>;
export type SafetyField = "blocked" | "muted";
const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
export const safetyIdentityId = (value: unknown): value is string => typeof value === "string" && /^id_[a-f0-9]{64}$/.test(value);
const revision = (value: unknown): value is number => typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
const invalid = () => new Error("The node returned invalid safety state. Refresh and try again.");

export class SafetyError extends Error {
  constructor(readonly status: number) {
    super(status === 401 ? "Sign in again to manage blocked and muted authors."
      : status === 403 ? "This account cannot change that safety state."
      : status === 404 ? "This author is not available on this node."
      : status === 409 ? "Safety state changed elsewhere. Refresh and choose again."
      : status === 400 || status === 422 ? "This change was rejected. Refresh the current state and try again."
      : status === 429 ? "Too many requests. Wait a moment, then retry."
      : `Safety request failed (${status}). Please retry.`);
  }
}

export function parseSafetyState(value: unknown, owner: string, target: string): SafetyState {
  if (!record(value) || !safetyIdentityId(owner) || !safetyIdentityId(target) || owner === target
    || value.author_id !== owner || value.target_id !== target || typeof value.blocked !== "boolean"
    || typeof value.muted !== "boolean" || !revision(value.revision)
    || (value.revision === 0 && (value.blocked || value.muted))) throw invalid();
  return Object.freeze({ author_id: owner, target_id: target, blocked: value.blocked, muted: value.muted, revision: value.revision });
}

export function parseSafetySnapshot(value: unknown, owner: string): SafetySnapshot {
  if (!record(value) || !safetyIdentityId(owner) || value.author_id !== owner || !revision(value.revision)
    || !Array.isArray(value.entries) || value.entries.length > 1000) throw invalid();
  let previous = "";
  const entries = value.entries.map((entry) => {
    if (!record(entry)) throw invalid();
    const person = parseSafetyIdentity(entry.identity);
    if (person.id <= previous) throw invalid();
    previous = person.id;
    const state = parseSafetyState(entry.state, owner, person.id);
    if ((!state.blocked && !state.muted) || state.revision > (value.revision as number)) throw invalid();
    return Object.freeze({ identity: person, state });
  });
  return Object.freeze({ author_id: owner, revision: value.revision, entries: Object.freeze(entries) });
}

export function parseSafetyIdentity(value: unknown): SafetyIdentity {
  if (!identity(value) || value.handle.length > 1024 || value.created_at.length > 64
    || value.public_key.algorithm !== "Ed25519" || typeof value.public_key.bytes !== "string"
    || value.public_key.bytes.length === 0 || value.public_key.bytes.length > 256
    || value.signature.algorithm !== "Ed25519" || typeof value.signature.bytes !== "string"
    || value.signature.bytes.length === 0 || value.signature.bytes.length > 512) throw invalid();
  return Object.freeze({ id: value.id, handle: value.handle, kind: value.kind, created_at: value.created_at,
    public_key: Object.freeze({ algorithm: value.public_key.algorithm, bytes: value.public_key.bytes }),
    signature: Object.freeze({ algorithm: value.signature.algorithm, bytes: value.signature.bytes }) });
}

async function safetyJson(response: Response, maxBytes: number): Promise<unknown> {
  if (!response.body) throw invalid();
  const reader = response.body.getReader();
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let size = 0, text = "";
  try {
    while (true) {
      const chunk = await reader.read();
      if (chunk.done) break;
      size += chunk.value.byteLength;
      if (size > maxBytes) throw invalid();
      text += decoder.decode(chunk.value, { stream: true });
    }
    return JSON.parse(text + decoder.decode()) as unknown;
  } finally {
    await reader.cancel().catch(() => undefined);
    reader.releaseLock();
  }
}

/** Authenticated host REST only. Never pass this client or its snapshot to an Object. */
export class SafetyClient {
  private readonly origin: URL;
  constructor(apiUrl: string, private readonly authenticatedFetch: typeof fetch) { this.origin = new URL(apiUrl); }

  async snapshot(owner: string, signal: AbortSignal): Promise<SafetySnapshot> {
    if (!safetyIdentityId(owner)) throw invalid();
    return parseSafetySnapshot(await this.request("/social/safety", signal, 4 * 1024 * 1024), owner);
  }

  async state(owner: string, target: string, signal: AbortSignal): Promise<SafetyState> {
    this.pair(owner, target);
    return parseSafetyState(await this.request(`/social/safety/${target}`, signal, 4096), owner, target);
  }

  async identity(target: string, signal: AbortSignal): Promise<SafetyIdentity> {
    if (!safetyIdentityId(target)) throw invalid();
    const value = await this.request(`/identities/${target}`, signal, 16 * 1024);
    if (!record(value)) throw invalid();
    const person = parseSafetyIdentity(value.identity);
    if (person.id !== target) throw invalid();
    return person;
  }

  async update(owner: string, target: string, intent: SafetyIntent, signal: AbortSignal): Promise<SafetyState> {
    this.pair(owner, target);
    if (typeof intent.blocked !== "boolean" || typeof intent.muted !== "boolean" || !revision(intent.expected_revision)
      || typeof intent.idempotency_key !== "string" || !/^[A-Za-z0-9_-]{1,128}$/.test(intent.idempotency_key)) throw invalid();
    const body = JSON.stringify({ blocked: intent.blocked, muted: intent.muted,
      expected_revision: intent.expected_revision, idempotency_key: intent.idempotency_key });
    return parseSafetyState(await this.request(`/social/safety/${target}`, signal, 4096, { method: "PUT", body }), owner, target);
  }

  private pair(owner: string, target: string): void {
    if (!safetyIdentityId(owner) || !safetyIdentityId(target) || owner === target) throw invalid();
  }

  private async request(path: string, signal: AbortSignal, maxBytes: number, init: RequestInit = {}): Promise<unknown> {
    try {
      const requestSignal = AbortSignal.any([signal, AbortSignal.timeout(15_000)]);
      const response = await this.authenticatedFetch(new URL(path, this.origin), { ...init,
        credentials: "omit", redirect: "error", cache: "no-store",
        headers: { accept: "application/json", ...(init.body ? { "content-type": "application/json" } : {}) },
        signal: requestSignal,
      });
      if (!response.ok) throw new SafetyError(response.status);
      const value = await safetyJson(response, maxBytes);
      requestSignal.throwIfAborted();
      return value;
    } catch (cause) {
      if (signal.aborted) throw cause;
      if (record(cause) && typeof cause.status === "number") throw new SafetyError(cause.status);
      throw new Error("Could not load safety state. Check your connection and retry.");
    }
  }
}

export interface SafetyAuthorView {
  readonly target: string | null;
  readonly state: SafetyState | null;
  readonly identity: SafetyIdentity | null;
  readonly pending: boolean;
  readonly retry: SafetyIntent | null;
  readonly message: string;
}

/** Account lifetime owns writes; dialog lifetime owns reads and presentation. */
export class SafetyController {
  private ownerId: string | null = null;
  private generation = 0;
  private selection = 0;
  private lifetime = new AbortController();
  private reading: AbortController | null = null;
  private snapshotRequest: { controller: AbortController; promise: Promise<SafetySnapshot | null> } | null = null;
  private snapshotValue: SafetySnapshot | null = null;
  private hiddenIds = new Set<string>();
  private blockedIds = new Set<string>();
  private intents = new Map<string, SafetyIntent>();
  private writing = false;
  view: SafetyAuthorView = { target: null, state: null, identity: null, pending: false, retry: null, message: "" };

  constructor(private readonly source: SafetySource,
    private readonly changed: (snapshot: SafetySnapshot | null) => void,
    private readonly rendered: () => void = () => undefined,
    private readonly key: () => string = () => crypto.randomUUID()) {}

  get owner(): string | null { return this.ownerId; }
  get snapshot(): SafetySnapshot | null { return this.snapshotValue; }
  hidden(author: string): boolean { return this.hiddenIds.has(author); }
  blocked(author: string): boolean { return this.blockedIds.has(author); }

  account(owner: string | null): void {
    if (owner !== null && !safetyIdentityId(owner)) throw new Error("Invalid safety account.");
    if (owner === this.ownerId) return;
    ++this.generation;
    ++this.selection;
    this.lifetime.abort(); this.reading?.abort(); this.snapshotRequest?.controller.abort();
    this.lifetime = new AbortController(); this.reading = null; this.snapshotRequest = null;
    this.ownerId = owner; this.intents.clear(); this.writing = false;
    this.view = { target: null, state: null, identity: null, pending: false, retry: null, message: "" };
    this.commit(null);
    this.rendered();
  }

  ensure(): Promise<SafetySnapshot | null> {
    return this.snapshotRequest?.promise ?? (this.snapshotValue ? Promise.resolve(this.snapshotValue) : this.loadSnapshot());
  }

  async refresh(): Promise<void> { await this.loadSnapshot(true); }

  private loadSnapshot(force = false): Promise<SafetySnapshot | null> {
    if (!this.ownerId) return Promise.resolve(null);
    if (this.snapshotRequest && !force) return this.snapshotRequest.promise;
    this.snapshotRequest?.controller.abort();
    const controller = new AbortController(), owner = this.ownerId, generation = this.generation;
    const current = () => generation === this.generation && !controller.signal.aborted;
    const promise = (async () => {
      try {
        const snapshot = parseSafetySnapshot(await this.source.snapshot(owner,
          AbortSignal.any([this.lifetime.signal, controller.signal])), owner);
        if (!current()) return null;
        if (this.snapshotValue && snapshot.revision < this.snapshotValue.revision) throw invalid();
        this.commit(snapshot);
        return snapshot;
      } catch (cause) {
        if (!current()) return null;
        this.commit(null);
        throw cause;
      } finally {
        if (current()) this.snapshotRequest = null;
      }
    })();
    this.snapshotRequest = { controller, promise };
    return promise;
  }

  async show(target: string | null): Promise<void> {
    this.reading?.abort(); ++this.selection;
    this.view = { target, state: null, identity: null, pending: this.writing, retry: target ? this.intents.get(target) ?? null : null, message: "" };
    this.rendered();
    if (target && this.ownerId && target !== this.ownerId && !this.writing) await this.read();
  }

  async read(): Promise<void> {
    const { target } = this.view, owner = this.ownerId;
    if (!owner || !target || owner === target || this.writing) return;
    this.reading?.abort();
    const controller = new AbortController(), generation = this.generation, selection = this.selection;
    this.reading = controller;
    const current = () => generation === this.generation && selection === this.selection && !controller.signal.aborted;
    this.publish({ pending: true, message: "Loading current safety state..." });
    try {
      const signal = AbortSignal.any([controller.signal, this.lifetime.signal]);
      const [pair, person] = await Promise.allSettled([
        this.source.state(owner, target, signal),
        this.source.identity ? this.source.identity(target, signal) : Promise.resolve(null),
      ]);
      if (pair.status === "rejected") throw pair.reason;
      const state = parseSafetyState(pair.value, owner, target);
      let identity: SafetyIdentity | null = null;
      if (person.status === "fulfilled" && person.value !== null) {
        identity = parseSafetyIdentity(person.value);
        if (identity.id !== target) throw invalid();
      }
      if (current()) this.publish({ state, identity, message: "" });
    } catch (cause) {
      if (current()) this.publish({ state: null, message: safetyMessage(cause) });
    } finally { if (current()) this.publish({ pending: false }); }
  }

  async change(field: SafetyField, enabled: boolean): Promise<void> {
    const { state, target, pending } = this.view;
    if (!state || !target || pending || this.writing || !this.ownerId || target === this.ownerId) return;
    if (this.intents.has(target)) {
      this.publish({ message: "Retry the previous change to confirm its outcome before choosing another change." });
      return;
    }
    if (state[field] === enabled) return;
    const intent: SafetyIntent = Object.freeze({ blocked: state.blocked, muted: state.muted,
      [field]: enabled, expected_revision: state.revision, idempotency_key: this.key() });
    this.intents.set(target, intent);
    await this.write(target, intent);
  }

  async retry(): Promise<void> {
    const target = this.view.target, intent = target ? this.intents.get(target) : null;
    if (target && intent && !this.view.pending && !this.writing) await this.write(target, intent);
    else await this.read();
  }

  private async write(target: string, intent: SafetyIntent): Promise<void> {
    const owner = this.ownerId;
    if (!owner) return;
    const generation = this.generation, selection = this.selection, signal = this.lifetime.signal;
    const current = () => generation === this.generation && !signal.aborted;
    const selected = () => current() && selection === this.selection;
    this.writing = true;
    this.reading?.abort();
    // A pre-write snapshot must never win over a post-write readback.
    this.snapshotRequest?.controller.abort(); this.snapshotRequest = null;
    this.publish({ pending: true, retry: intent, message: "Saving safety state..." });
    let failure: unknown = null;
    try {
      parseSafetyState(await this.source.update(owner, target, intent, signal), owner, target);
      if (current()) this.intents.delete(target);
    } catch (cause) {
      failure = cause;
      if (current() && cause instanceof SafetyError && cause.status >= 400 && cause.status < 500 && cause.status !== 408 && cause.status !== 429) {
        this.intents.delete(target);
      }
    }
    if (!current()) return;
    let fresh: SafetyState | null = null;
    let readFailure: unknown = null;
    try { fresh = parseSafetyState(await this.source.state(owner, target, signal), owner, target); }
    catch (cause) { readFailure = cause; }
    if (!current()) return;
    try { await this.refresh(); } catch (cause) { readFailure = cause; }
    if (!current()) return;
    this.writing = false;
    if (selected()) {
      const conflict = failure instanceof SafetyError && failure.status === 409;
      this.publish({ state: fresh, pending: false, retry: this.intents.get(target) ?? null,
        message: readFailure ? "Could not confirm current safety state. Refresh before continuing."
          : conflict ? "Safety state changed elsewhere. Current state refreshed; choose again."
          : failure ? `${safetyMessage(failure)}${this.intents.has(target) ? " Retry the same change to confirm its outcome." : ""}` : "Current safety state refreshed." });
    } else {
      this.publish({ pending: false, retry: this.view.target ? this.intents.get(this.view.target) ?? null : null });
      if (this.view.target) await this.read();
    }
  }

  dispose(): void {
    this.account(null);
    this.lifetime.abort(); this.reading?.abort(); this.snapshotRequest?.controller.abort();
  }

  private commit(snapshot: SafetySnapshot | null): void {
    const prior = this.snapshotValue;
    this.snapshotValue = snapshot;
    this.hiddenIds = new Set(snapshot?.entries.map((entry) => entry.state.target_id));
    this.blockedIds = new Set(snapshot?.entries.filter((entry) => entry.state.blocked).map((entry) => entry.state.target_id));
    if (snapshot !== null || prior !== null) this.changed(snapshot);
    this.rendered();
  }
  private publish(patch: Partial<SafetyAuthorView>): void { this.view = { ...this.view, ...patch }; this.rendered(); }
}

export function safetyMessage(cause: unknown): string {
  return cause instanceof SafetyError ? cause.message : "Could not load safety state. Check your connection and retry.";
}
