import type { ProtocolTypes } from "@babble-protocol/sdk";
import { Accounts, AccountError, newPasswordError, type AccountSession } from "./accounts";

export type AccountSessionInfo = ProtocolTypes["api.AccountSessionInfo"];
export type AccountSessionsResponse = ProtocolTypes["api.AccountSessionsResponse"];
export type ChangePasswordRequest = ProtocolTypes["api.ChangePasswordRequest"];
export type RevokeTarget = AccountSessionInfo | "others" | "current";
const sessionId = (id: unknown): id is string => typeof id === "string" && /^account_[a-f0-9]{64}$/.test(id);
const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const timestamp = (value: unknown): value is string => typeof value === "string"
  && /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/i.test(value)
  && Number.isFinite(Date.parse(value));

export function parseAccountSessions(value: unknown): AccountSessionInfo[] {
  const invalid = () => new Error("The session list could not be verified. Refresh to try again.");
  if (!record(value) || Object.keys(value).some((key) => key !== "sessions")
    || !Array.isArray(value.sessions) || value.sessions.length > 16) throw invalid();
  const ids = new Set<string>();
  let current = 0;
  const sessions = value.sessions.map((item: unknown) => {
    if (!record(item) || Object.keys(item).some((key) => !["id", "created_at", "expires_at", "current"].includes(key))
      || !sessionId(item.id) || ids.has(item.id) || typeof item.current !== "boolean"
      || !(item.created_at === null || timestamp(item.created_at)) || !timestamp(item.expires_at)
      || (item.created_at !== null && Date.parse(item.created_at) > Date.parse(item.expires_at))) throw invalid();
    ids.add(item.id);
    if (item.current && ++current > 1) throw invalid();
    return { id: item.id, created_at: item.created_at, expires_at: item.expires_at, current: item.current };
  });
  if (sessions.length > 0 && current !== 1) throw invalid();
  return sessions;
}

function abortable<T>(promise: Promise<T>, signal: AbortSignal): Promise<T> {
  return new Promise((resolve, reject) => {
    const abort = () => reject(signal.reason);
    signal.addEventListener("abort", abort, { once: true });
    if (signal.aborted) abort();
    promise.then(resolve, reject).finally(() => signal.removeEventListener("abort", abort));
  });
}

async function boundedJson(response: Response, signal: AbortSignal): Promise<unknown> {
  const reader = response.body?.getReader();
  if (!reader) throw new Error("Missing response");
  const decoder = new TextDecoder("utf-8", { fatal: true });
  let size = 0, text = "";
  try {
    for (;;) {
      const { value, done } = await abortable(reader.read(), signal);
      if (done) break;
      size += value.byteLength;
      if (size > 16_384) throw new Error("Session response exceeded its size limit");
      text += decoder.decode(value, { stream: true });
    }
    return JSON.parse(text + decoder.decode()) as unknown;
  } finally {
    void reader.cancel().catch(() => undefined);
    reader.releaseLock();
  }
}

/** Requests remain bound to the host login that initiated them, including body reads. */
export class AccountSecurityClient {
  constructor(private readonly accounts: Accounts, private readonly timeoutMs = 15_000) {}

  async sessions(session: AccountSession, signal: AbortSignal): Promise<AccountSessionInfo[]> {
    return parseAccountSessions(await this.request(session, signal, "/auth/sessions"));
  }

  async revoke(session: AccountSession, signal: AbortSignal, target: RevokeTarget): Promise<void> {
    if (typeof target !== "string" && !sessionId(target.id)) throw new Error("Invalid session identifier");
    await this.request(session, signal, target === "others" ? "/auth/sessions/revoke-others"
      : target === "current" ? "/auth/session" : `/auth/sessions/${target.id}`,
      target === "others" ? "POST" : "DELETE");
    if (target === "current" || (typeof target !== "string" && target.current)) this.accounts.forgetSession(session);
  }

  async password(session: AccountSession, signal: AbortSignal, payload: ChangePasswordRequest): Promise<void> {
    const error = newPasswordError(payload.new_password);
    if (error) throw new AccountError(error, 0);
    await this.request(session, signal, "/auth/password", "POST", payload);
    this.accounts.forgetSession(session);
  }

  private async request(session: AccountSession, signal: AbortSignal, path: string,
    method = "GET", payload?: ChangePasswordRequest): Promise<unknown> {
    signal.throwIfAborted();
    if (!this.accounts.isCurrentSession(session)) throw new AccountError("Sign in again to continue", 401);
    const abort = new AbortController();
    const cancel = () => abort.abort(signal.reason);
    signal.addEventListener("abort", cancel, { once: true });
    const timeout = setTimeout(() => abort.abort(new Error("Request timed out")), this.timeoutMs);
    try {
      const response = await abortable(this.accounts.authenticatedFetch(new URL(path, this.accounts.origin), {
        method, signal: abort.signal, cache: "no-store", credentials: "omit", redirect: "error",
        headers: { accept: "application/json", ...(payload ? { "content-type": "application/json" } : {}) },
        ...(payload ? { body: JSON.stringify(payload) } : {}),
      }), abort.signal);
      if (!this.accounts.isCurrentSession(session)) throw new AccountError("Sign in again to continue", 401);
      if (method !== "GET") {
        void response.body?.cancel().catch(() => undefined);
        if (response.status !== 204) throw new Error("Unconfirmed security change");
        return null;
      }
      const body = await boundedJson(response, abort.signal);
      if (!this.accounts.isCurrentSession(session)) throw new AccountError("Sign in again to continue", 401);
      return body;
    } finally {
      clearTimeout(timeout);
      signal.removeEventListener("abort", cancel);
    }
  }
}

export interface SecurityState {
  readonly sessions: readonly AccountSessionInfo[];
  readonly loading: boolean;
  readonly loaded: boolean;
  readonly sessionBusy: boolean;
  readonly passwordBusy: boolean;
  readonly status: string;
  readonly statusState: "ready" | "error";
  readonly passwordStatus: string;
  readonly passwordState: "ready" | "error";
  readonly uncertainPassword: boolean;
  readonly confirmation: RevokeTarget | null;
}
const empty = (): SecurityState => ({ sessions: [], loading: false, loaded: false, sessionBusy: false,
  passwordBusy: false, status: "", statusState: "ready", passwordStatus: "", passwordState: "ready", uncertainPassword: false, confirmation: null });
export const uncertainPasswordMessage = "The password change was not confirmed. It may have succeeded. Sign in with your new password to check; if it is not accepted, try your previous password.";
export class SecurityNotice extends Event {
  constructor(readonly message: string) { super("notice"); }
}

export class AccountSecurity extends EventTarget {
  private state: SecurityState = empty();
  private session: AccountSession | null = null;
  private generation = 0;
  private readGeneration = 0;
  private abort = new AbortController();
  private opened = false;
  private passwordSignoutGeneration = -1;
  private readonly changed = () => {
    if (this.session && this.accounts.isCurrentSession(this.session)) return;
    this.passwordSignoutGeneration = this.state.passwordBusy && !this.accounts.current ? this.generation : -1;
    const reopen = this.opened;
    this.close();
    if (reopen) this.open();
  };

  constructor(private readonly accounts: Accounts,
    private readonly client = new AccountSecurityClient(accounts)) {
    super();
    accounts.addEventListener("change", this.changed);
  }

  get view(): SecurityState { return this.state; }
  get passwordSignoutPending(): boolean { return this.passwordSignoutGeneration === this.generation - 1; }
  open(): void {
    this.opened = true;
    if (this.session && this.accounts.isCurrentSession(this.session)) return;
    this.session = this.accounts.current;
    if (this.session) void this.refresh();
  }
  close(): void {
    this.opened = false;
    this.generation++;
    this.readGeneration++;
    this.abort.abort();
    this.abort = new AbortController();
    this.session = null;
    this.state = empty();
    this.emit();
  }
  dispose(): void { this.close(); this.accounts.removeEventListener("change", this.changed); }

  async refresh(): Promise<void> {
    const session = this.session, generation = this.generation, read = ++this.readGeneration;
    if (!session || !this.accounts.isCurrentSession(session)) return;
    this.update({ loading: true, status: "Loading sessions...", statusState: "ready" });
    try {
      const sessions = await this.client.sessions(session, this.abort.signal);
      if (this.valid(session, generation) && read === this.readGeneration) {
        this.update({ sessions, loaded: true, status: sessions.length ? "" : "No active sessions returned. Refresh to check again." });
      }
    } catch {
      if (this.valid(session, generation) && read === this.readGeneration) this.update({ status: "Could not refresh sessions. Try again.", statusState: "error" });
    } finally {
      if (this.valid(session, generation) && read === this.readGeneration) this.update({ loading: false });
    }
  }

  requestRevoke(target: RevokeTarget): void {
    if (!this.session || this.state.sessionBusy || this.state.passwordBusy) return;
    if (typeof target !== "string" && !this.state.sessions.some((session) => session.id === target.id && session.current === target.current)) return;
    this.update({ confirmation: target });
  }
  cancelRevoke(): void { if (!this.state.sessionBusy) this.update({ confirmation: null }); }
  async confirmRevoke(): Promise<void> {
    const session = this.session, generation = this.generation, target = this.state.confirmation;
    if (!session || !target || this.state.sessionBusy || this.state.passwordBusy) return;
    // Do not let a list fetched before a mutation overwrite its authoritative readback.
    ++this.readGeneration;
    this.update({ sessionBusy: true, confirmation: null, loading: false, status: "Signing out...", statusState: "ready" });
    let failed = false;
    try { await this.client.revoke(session, this.abort.signal, target); }
    catch { failed = true; }
    if (!this.valid(session, generation)) return;
    await this.refresh();
    if (this.valid(session, generation)) this.update({ sessionBusy: false,
      statusState: failed ? "error" : this.state.statusState,
      status: failed ? "Sign-out was not confirmed. Check the refreshed list before trying again."
        : this.state.status || "Session sign-out confirmed." });
  }

  async changePassword(current: string, next: string, confirmation: string): Promise<void> {
    const session = this.session, generation = this.generation;
    if (!session || this.state.passwordBusy || this.state.sessionBusy || this.state.uncertainPassword) return;
    const validation = newPasswordError(next) ?? (current.length === 0 ? "Enter your current password."
      : next === current ? "Choose a new password different from your current password."
      : next !== confirmation ? "The new passwords do not match." : null);
    if (validation) { this.update({ passwordStatus: validation, passwordState: "error" }); return; }
    this.update({ passwordBusy: true, passwordStatus: "Changing password and signing out every session...", passwordState: "ready", confirmation: null });
    try {
      await this.client.password(session, this.abort.signal, { current_password: current, new_password: next });
      if (this.passwordEndedThisLogin(generation)) this.dispatchEvent(new SecurityNotice("Password changed. Every session is signed out. Sign in with your new password."));
    } catch (cause) {
      if (this.passwordEndedThisLogin(generation)) this.dispatchEvent(new SecurityNotice(uncertainPasswordMessage));
      if (!this.valid(session, generation)) return;
      const status = cause instanceof AccountError ? cause.status : 0;
      this.update({ passwordState: "error", passwordStatus: status === 403 ? "Your current password was not accepted. You are still signed in."
        : status === 400 || status === 422 ? "The new password was not accepted. Check its length and try again."
        : uncertainPasswordMessage,
      uncertainPassword: ![400, 403, 422].includes(status) });
    } finally {
      if (this.valid(session, generation)) this.update({ passwordBusy: false });
    }
  }

  recoverSignIn(): void {
    if (this.state.uncertainPassword && this.session) {
      const generation = this.generation;
      this.accounts.forgetSession(this.session);
      if (!this.accounts.current && this.generation === generation + 1) this.dispatchEvent(new SecurityNotice(uncertainPasswordMessage));
    }
  }
  private valid(session: AccountSession, generation: number): boolean {
    return this.generation === generation && this.accounts.isCurrentSession(session);
  }
  private passwordEndedThisLogin(generation: number): boolean {
    return !this.accounts.current && this.passwordSignoutGeneration === generation && this.generation === generation + 1;
  }
  private update(patch: Partial<SecurityState>): void { this.state = { ...this.state, ...patch }; this.emit(); }
  private emit(): void { this.dispatchEvent(new Event("change")); }
}
