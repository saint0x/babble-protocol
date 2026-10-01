export type DraftTarget = { readonly mode: "publish"; readonly parent: null }
  | { readonly mode: "reply" | "share"; readonly parent: string };

export interface DraftOwner {
  readonly origin: string;
  readonly identityId: string | null;
}

const MAX_STORED_TEXT_LENGTH = 64 * 1024;
const EMPTY_DRAFT_CACHE_LIMIT = 32;

interface Draft {
  readonly key: string;
  readonly persistent: boolean;
  readonly target: DraftTarget;
  text: string;
  media: readonly File[];
  bundle: BundleAttachment | null;
  operationId: string;
  revision: number;
  pending: DraftSubmission | null;
}

export interface DraftSubmission {
  readonly owner: DraftOwner;
  readonly target: DraftTarget;
  readonly text: string;
  readonly media: readonly File[];
  readonly bundle: BundleAttachment | null;
  readonly revision: number;
  readonly epoch: number;
  readonly view: number;
  readonly key: string;
  readonly operationId: string;
}

/** Draft content and submission lifetimes are independent of the composer DOM. */
export class Drafts {
  private readonly entries = new Map<string, Draft>();
  private owner: DraftOwner;
  private active: Draft | null = null;
  private epoch = 0;
  private view = 0;
  private visible = false;
  storageError: string | null = null;

  constructor(owner: DraftOwner, private readonly storage: Storage | null) {
    this.owner = { ...owner, origin: new URL(owner.origin).origin };
  }

  get current(): Readonly<Draft> | null { return this.active; }
  get isOpen(): boolean { return this.visible; }
  get viewRevision(): number { return this.view; }

  setOwner(owner: DraftOwner): boolean {
    const origin = new URL(owner.origin).origin;
    if (origin === this.owner.origin && owner.identityId === this.owner.identityId) return false;
    const target = this.active?.target;
    this.owner = { origin, identityId: owner.identityId };
    this.invalidate();
    this.active = target ? this.read(target) : null;
    if (this.active?.pending) this.active.revision += 1;
    return true;
  }

  invalidate(): void {
    this.epoch += 1;
    this.view += 1;
  }

  open(target: DraftTarget): void {
    this.active = this.read(target);
    // Reopening an in-flight draft establishes a new editing session.
    if (this.active.pending) this.active.revision += 1;
    this.visible = true;
    this.view += 1;
  }

  close(): void { this.visible = false; this.view += 1; }

  edit(text: string, media: readonly File[]): void {
    const draft = this.active;
    if (!draft || (draft.text === text && draft.media.length === media.length
      && draft.media.every((file, index) => file === media[index]))) return;
    draft.text = text;
    draft.media = Object.freeze([...media]);
    if (media.length) draft.bundle = null;
    draft.revision += 1;
    this.persist(draft);
  }

  editBundle(bundle: BundleAttachment | null): void {
    const draft = this.active;
    if (!draft || draft.target.mode !== "publish" || draft.bundle === bundle) return;
    draft.bundle = bundle ? Object.freeze({
      files: Object.freeze([...bundle.files]), entryPath: bundle.entryPath,
      capabilitiesText: bundle.capabilitiesText ?? "[]",
    }) : null;
    if (bundle) draft.media = Object.freeze([]);
    draft.revision += 1;
    this.persist(draft);
  }

  begin(): DraftSubmission | null {
    const draft = this.active;
    if (!draft || !this.visible || !this.owner.identityId || draft.pending) return null;
    const submission = Object.freeze({
      owner: Object.freeze({ ...this.owner }), target: draft.target,
      text: draft.text, media: draft.media, bundle: draft.bundle, revision: draft.revision,
      epoch: this.epoch, view: this.view, key: draft.key,
      operationId: draft.operationId,
    });
    draft.pending = submission;
    return submission;
  }

  owns(submission: DraftSubmission): boolean {
    return submission.epoch === this.epoch && submission.owner.origin === this.owner.origin
      && submission.owner.identityId === this.owner.identityId;
  }

  isCurrent(submission: DraftSubmission): boolean {
    return this.owns(submission) && this.visible && submission.view === this.view
      && submission.key === this.active?.key && submission.revision === this.active.revision;
  }

  finish(submission: DraftSubmission, success: boolean): boolean {
    const draft = this.entries.get(submission.key);
    if (draft?.pending !== submission) return false;
    const current = this.isCurrent(submission);
    draft.pending = null;
    if (success && draft.revision === submission.revision) {
      draft.text = "";
      draft.media = Object.freeze([]);
      draft.bundle = null;
      draft.operationId = crypto.randomUUID();
      draft.revision += 1;
      this.persist(draft);
    }
    return current;
  }

  private read(target: DraftTarget): Draft {
    const key = `babble.draft.v1:${JSON.stringify([this.owner.origin, this.owner.identityId, target.mode, target.parent])}`;
    const existing = this.entries.get(key);
    if (existing) return existing;
    for (const [cachedKey, draft] of this.entries) {
      if (this.entries.size < EMPTY_DRAFT_CACHE_LIMIT) break;
      if (!draft.text && !draft.media.length && !draft.bundle && !draft.pending && draft !== this.active) this.entries.delete(cachedKey);
    }
    let text = "";
    let operationId: string = crypto.randomUUID();
    if (this.owner.identityId && this.storage) {
      try {
        const stored: unknown = JSON.parse(this.storage.getItem(key) ?? "null");
        if (typeof stored === "object" && stored !== null && "text" in stored && typeof stored.text === "string") {
          if (stored.text.length <= MAX_STORED_TEXT_LENGTH) text = stored.text;
          else this.storageError = "Saved draft exceeds the text storage limit";
          if ("operationId" in stored && typeof stored.operationId === "string"
            && /^[0-9a-f-]{36}$/.test(stored.operationId)) operationId = stored.operationId;
        }
      } catch { this.storageError = "Draft storage unavailable; changes stay in this tab"; }
    }
    const draft: Draft = { key, persistent: this.owner.identityId !== null,
      target: Object.freeze({ ...target }), text, operationId, media: Object.freeze([]), bundle: null, revision: 0, pending: null };
    this.entries.set(key, draft);
    return draft;
  }

  private persist(draft: Draft): void {
    // Guest content, File objects, and bundle capability declarations stay in memory.
    if (!draft.persistent) return;
    if (!this.storage) { this.storageError = "Draft storage unavailable; changes stay in this tab"; return; }
    if (draft.text.length > MAX_STORED_TEXT_LENGTH) {
      this.storageError = "Draft exceeds the 64 KiB text limit; changes stay in this tab";
      return;
    }
    try {
      if (draft.text || draft.media.length || draft.bundle) this.storage.setItem(draft.key, JSON.stringify({ text: draft.text, operationId: draft.operationId }));
      else this.storage.removeItem(draft.key);
      this.storageError = null;
    } catch { this.storageError = "Draft storage unavailable; changes stay in this tab"; }
  }
}

/** Check every request, including later capability and blob publication steps. */
export function draftTransport(request: typeof fetch, isAuthorized: () => boolean, operationId: string): typeof fetch {
  return async (input, init) => {
    if (!isAuthorized()) throw new Error("Account changed; draft was not submitted with the new account");
    if (typeof init?.body === "string") {
      const envelope = JSON.parse(init.body);
      if (envelope.protocol === "babble.rpc.v1" && envelope.idempotency_key) {
        const material = JSON.stringify([operationId, envelope.method, envelope.payload]);
        const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(material)));
        envelope.idempotency_key = `web-draft-${Array.from(digest, (byte) => byte.toString(16).padStart(2, "0")).join("")}`;
        init = { ...init, body: JSON.stringify(envelope) };
      }
    }
    if (!isAuthorized()) throw new Error("Account changed; draft was not submitted with the new account");
    return request(input, init);
  };
}
import type { BundleAttachment } from "./bundle-publication";
