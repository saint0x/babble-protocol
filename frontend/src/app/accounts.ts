export interface AccountIdentity {
  readonly id: string;
  readonly handle: string;
}

export interface AccountSession {
  readonly identity: AccountIdentity;
  readonly token: string;
  readonly expires_at: string;
}

export class AccountError extends Error {
  constructor(message: string, readonly status: number) {
    super(message);
    this.name = "AccountError";
  }
}

export class Accounts extends EventTarget {
  private session: AccountSession | null = null;
  private revision = 0;
  private readonly storageKey: string;
  readonly origin: URL;

  constructor(
    apiUrl: string,
    private readonly storage: Storage | null,
    private readonly request: typeof fetch = globalThis.fetch.bind(globalThis),
    private readonly timeoutMs = 30_000,
  ) {
    super();
    this.origin = new URL(apiUrl);
    this.storageKey = `babel.session.v1:${this.origin.origin}`;
    try {
      const value: unknown = JSON.parse(storage?.getItem(this.storageKey) ?? "null");
      if (isSession(value) && Date.parse(value.expires_at) > Date.now()) this.session = value;
      else storage?.removeItem(this.storageKey);
    } catch {
      // Storage can be disabled; authentication still works for this page's lifetime.
      this.session = null;
    }
  }

  get current(): AccountSession | null {
    if (this.session && Date.parse(this.session.expires_at) <= Date.now()) this.replace(null);
    return this.session;
  }

  localDataKey(kind: "preferences" | "seen"): string {
    return `babel.local.v2:${JSON.stringify([this.origin.origin, this.current?.identity.id ?? null, kind])}`;
  }

  /** Private endpoints never run as a guest or deliver a previous session's data. */
  readonly authenticatedFetch: typeof fetch = async (input, init) => {
    const token = this.current?.token;
    if (!token) throw new AccountError("Sign in to continue", 401);
    const revision = this.revision;
    const response = await this.fetch(input, init);
    if (revision !== this.revision || token !== this.current?.token) {
      throw new AccountError("The signed-in account changed. Please retry.", 401);
    }
    return response;
  };

  /** Only the host owns this transport; a Surface never receives its credential. */
  readonly fetch: typeof fetch = async (input, init = {}) => {
    const url = new URL(input instanceof Request ? input.url : input.toString());
    if (url.origin !== this.origin.origin || url.username || url.password) {
      throw new AccountError("Refusing to send a Babel session to another origin", 0);
    }
    const token = this.current?.token;
    const headers = new Headers(input instanceof Request ? input.headers : undefined);
    new Headers(init.headers).forEach((value, key) => headers.set(key, value));
    headers.delete("authorization");
    if (token) headers.set("authorization", `Bearer ${token}`);
    const response = await this.timedRequest(input, { ...init, headers, redirect: "error", credentials: "omit" });
    if (response.status === 401 && token && this.session?.token === token) this.replace(null);
    if (!response.ok) {
      let message = response.status === 401 ? "Sign in to continue" : `Request failed (${response.status})`;
      try {
        const body: unknown = await accountErrorBody(response);
        if (isRecord(body) && typeof body.message === "string") message = body.message;
      } catch { /* A proxy may return a non-JSON error. Keep the HTTP status. */ }
      throw new AccountError(message, response.status);
    }
    return response;
  };

  async restore(): Promise<void> {
    const session = this.current;
    if (!session) return;
    const revision = this.revision;
    const response = await this.fetch(new URL("/auth/session", this.origin));
    const body: unknown = await response.json();
    if (!isRecord(body) || !isIdentity(body.identity) || typeof body.expires_at !== "string"
      || body.identity.id !== session.identity.id || !Number.isFinite(Date.parse(body.expires_at))) {
      throw new AccountError("Invalid account session response", 0);
    }
    if (this.revision === revision) this.replace({ ...session, identity: body.identity, expires_at: body.expires_at });
  }

  register(handle: string, password: string): Promise<AccountSession> {
    const error = newPasswordError(password);
    if (error) return Promise.reject(new AccountError(error, 0));
    return this.authenticate("register", { handle, kind: "Person", password });
  }

  /** Clear only the login captured by an acknowledged security operation. */
  forgetSession(captured: AccountSession): boolean {
    if (!this.isCurrentSession(captured)) return false;
    this.replace(null);
    return true;
  }

  isCurrentSession(captured: AccountSession): boolean {
    const current = this.current;
    return current !== null && current.token === captured.token && current.identity.id === captured.identity.id;
  }

  login(identityId: string, password: string): Promise<AccountSession> {
    return this.authenticate("login", { identity_id: identityId, password });
  }

  async logout(): Promise<void> {
    const token = this.current?.token;
    if (!token) return;
    try {
      await this.fetch(new URL("/auth/session", this.origin), { method: "DELETE" });
    } catch (cause) {
      if (!(cause instanceof AccountError && cause.status === 401)) throw cause;
    }
    if (this.session?.token === token) this.replace(null);
  }

  private async authenticate(action: "register" | "login", payload: Record<string, string>): Promise<AccountSession> {
    if (this.current) throw new AccountError("Sign out before switching accounts", 409);
    const revision = ++this.revision;
    const response = await this.timedRequest(new URL(`/auth/${action}`, this.origin), {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify(payload), credentials: "omit", redirect: "error",
    });
    let body: unknown;
    try { body = await response.json(); }
    catch { throw new AccountError("Account service returned an invalid response", response.status); }
    if (!response.ok) {
      throw new AccountError(isRecord(body) && typeof body.message === "string"
        ? body.message : "Could not sign in", response.status);
    }
    if (!isSession(body) || Date.parse(body.expires_at) <= Date.now()) {
      throw new AccountError("Invalid account session response", 0);
    }
    if (this.revision !== revision) throw new AccountError("This sign-in was superseded by another account operation", 409);
    this.replace(body);
    return body;
  }

  private replace(session: AccountSession | null): void {
    // Metadata refresh is not an account switch; private reads retain their owner.
    const sameCredentials = session !== null && this.session !== null
      && session.token === this.session.token && session.identity.id === this.session.identity.id;
    if (!sameCredentials) this.revision += 1;
    this.session = session;
    try {
      if (session) this.storage?.setItem(this.storageKey, JSON.stringify(session));
      else this.storage?.removeItem(this.storageKey);
    } catch { /* A denied storage write must not invalidate a live server session. */ }
    this.dispatchEvent(new Event("change"));
  }

  private timedRequest(input: RequestInfo | URL, init: RequestInit): Promise<Response> {
    const signal = init.signal ?? (input instanceof Request ? input.signal : null);
    const deadline = AbortSignal.timeout(this.timeoutMs);
    return this.request(input, { ...init, signal: signal ? AbortSignal.any([signal, deadline]) : deadline });
  }
}

export function newPasswordError(password: string): string | null {
  // JS length counts UTF-16 units, not the Unicode scalars used by the node.
  const scalars = Array.from(password);
  if (scalars.some((value) => { const point = value.codePointAt(0)!; return point >= 0xd800 && point <= 0xdfff; })) {
    return "Use valid Unicode characters in your password.";
  }
  if (scalars.length < 15) return "Use at least 15 characters for your new password.";
  const bytes = new TextEncoder().encode(password).byteLength;
  return bytes > 1024 ? "Your password must be at most 1024 UTF-8 bytes." : null;
}

async function accountErrorBody(response: Response): Promise<unknown> {
  const reader = response.body?.getReader();
  if (!reader) return null;
  let text = "";
  let size = 0;
  const decoder = new TextDecoder();
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > 16_384) return null;
      text += decoder.decode(value, { stream: true });
    }
    return JSON.parse(text + decoder.decode()) as unknown;
  } finally {
    await reader.cancel().catch(() => undefined);
    reader.releaseLock();
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isIdentity(value: unknown): value is AccountIdentity {
  return isRecord(value) && typeof value.id === "string" && value.id.length > 0
    && typeof value.handle === "string" && value.handle.length > 0;
}

function isSession(value: unknown): value is AccountSession {
  return isRecord(value) && isIdentity(value.identity) && typeof value.token === "string"
    && value.token.length >= 32 && !/\s/.test(value.token)
    && typeof value.expires_at === "string" && Number.isFinite(Date.parse(value.expires_at));
}
