import { appealEligible, moderationId, moderationMessage, moderationSignals, moderationText, ModerationError,
  parseModerationAccess, parseModerationCase, parseModerationPage, reviewEligible, uncertainModeration } from "./moderation";
import type { AppealIntent, DecisionIntent, ModerationAccess, ModerationCase, ModerationReason, ModerationScope, ModerationSource, ReportIntent } from "./moderation";

export type ModerationSelection = { kind: "report"; objectId: string } | { kind: "inbox"; scope: ModerationScope } | { kind: "detail"; id: string };
type Write = { kind: "report"; intent: ReportIntent } | { kind: "decision"; id: string; intent: DecisionIntent } | { kind: "appeal"; id: string; intent: AppealIntent };
export interface ModerationView {
  readonly selection: ModerationSelection | null;
  readonly access: ModerationAccess | null;
  readonly items: readonly ModerationCase[];
  readonly nextBefore: number | null;
  readonly detail: ModerationCase | null;
  readonly loading: boolean;
  readonly message: string;
  readonly error: boolean;
}
const empty = (): ModerationView => ({ selection: null, access: null, items: [], nextBefore: null, detail: null, loading: false, message: "", error: false });

/** Writes and exact retry intents belong to an account, reads to a dialog selection. */
export class ModerationController {
  private ownerId: string | null = null;
  private generation = 0;
  private context = 0;
  private lifetime = new AbortController();
  private reading: AbortController | null = null;
  private intent: Write | null = null;
  private writing = false;
  private disposed = false;
  view: ModerationView = empty();
  constructor(private readonly source: ModerationSource, private readonly changed: (record: ModerationCase) => void,
    private readonly rendered: () => void = () => undefined, private readonly key: () => string = () => crypto.randomUUID()) {}
  get owner(): string | null { return this.ownerId; }
  get pending(): boolean { return this.writing; }
  get uncertain(): boolean { return this.intent !== null && !this.writing; }
  get retryKind(): Write["kind"] | null { return this.intent?.kind ?? null; }

  account(owner: string | null): void {
    if (owner !== null && !moderationId(owner, "id")) throw new Error("Invalid moderation account.");
    if (this.disposed || owner === this.ownerId) return;
    ++this.generation; ++this.context; this.lifetime.abort(); this.reading?.abort();
    this.lifetime = new AbortController(); this.reading = null; this.ownerId = owner;
    this.intent = null; this.writing = false; this.view = empty(); this.rendered();
  }
  async show(selection: ModerationSelection): Promise<void> {
    if (this.disposed) return;
    this.hide(); this.view = { ...empty(), selection }; this.rendered();
    if (this.ownerId) await this.refresh();
  }
  hide(): void {
    ++this.context; this.reading?.abort(); this.reading = null; this.view = empty(); this.rendered();
  }
  dispose(): void { this.account(null); this.hide(); this.lifetime.abort(); this.disposed = true; }

  async refresh(more = false, reconcile = false): Promise<void> {
    const selection = this.view.selection, owner = this.ownerId;
    if (!selection || !owner || this.disposed || this.writing || (more && (this.view.loading || this.view.nextBefore === null))) return;
    this.reading?.abort(); const reading = new AbortController(); this.reading = reading;
    const generation = this.generation, context = this.context, before = more ? this.view.nextBefore : null;
    const current = () => generation === this.generation && context === this.context && !reading.signal.aborted;
    const signal = AbortSignal.any([reading.signal, this.lifetime.signal]);
    this.publish({ loading: true, message: "Loading moderation...", error: false });
    try {
      const access = parseModerationAccess(await this.source.access(owner, signal), owner);
      if (!current()) return;
      this.publish({ access });
      if (selection.kind === "inbox") {
        if (selection.scope === "queue" && !access.can_review) throw new ModerationError(403);
        const page = parseModerationPage(await this.source.list(owner, selection.scope, before, signal), before);
        for (const item of page.items) {
          this.readable(item, access);
          if ((selection.scope === "mine" && item.reporter_id !== owner) || (selection.scope === "affected" && (item.subject_author_id !== owner || !item.decisions.length))) throw new Error("Invalid case scope");
        }
        if (!current()) return;
        const existing = more ? this.view.items : [];
        const ids = new Set(existing.map(item => item.id));
        if (page.items.some(item => ids.has(item.id))) throw new Error("Duplicate case page");
        this.publish({ items: [...existing, ...page.items], nextBefore: page.next_before, message: "" });
      } else if (selection.kind === "detail") {
        const item = parseModerationCase(await this.source.detail(owner, selection.id, signal));
        this.readable(item, access);
        if (item.id !== selection.id || (this.view.detail && item.revision < this.view.detail.revision)) throw new Error("Invalid case receipt");
        if (!current()) return;
        this.publish({ detail: item, message: "" });
        if (reconcile) this.changed(item);
      } else if (!moderationId(selection.objectId, "obj")) throw new Error("Invalid Object ID");
      else this.publish({ message: "" });
    } catch (cause) {
      if (current()) this.publish({ message: moderationMessage(cause), error: true, ...(more ? {} : { detail: null, items: [], nextBefore: null }) });
    } finally { if (current()) { this.reading = null; this.publish({ loading: false }); } }
  }

  async report(reason: ModerationReason, details: string): Promise<void> {
    const selection = this.view.selection, access = this.view.access;
    if (!this.ready() || selection?.kind !== "report" || !access) return;
    if (!moderationId(selection.objectId, "obj") || !access.reasons.includes(reason) || !moderationText(details.trim())) return this.validation();
    this.intent = Object.freeze({ kind: "report", intent: Object.freeze({ object_id: selection.objectId, reason, details: details.trim(), idempotency_key: this.key() }) });
    await this.write();
  }
  async decide(values: Pick<DecisionIntent, "outcome" | "reason" | "explanation" | "source_signals">): Promise<void> {
    const item = this.view.detail, access = this.view.access;
    if (!this.ready() || !item || !access || !reviewEligible(access, item)) return;
    if (!access.reasons.includes(values.reason) || !["restrict", "no_action"].includes(values.outcome) || !moderationText(values.explanation.trim()) || !moderationSignals(values.source_signals)) return this.validation();
    this.intent = Object.freeze({ kind: "decision", id: item.id, intent: Object.freeze({ ...values, explanation: values.explanation.trim(),
      source_signals: Object.freeze([...values.source_signals]), policy_version: access.policy_version, expected_revision: item.revision, idempotency_key: this.key() }) });
    await this.write();
  }
  async appeal(details: string): Promise<void> {
    const item = this.view.detail;
    if (!this.ready() || !item || !appealEligible(this.ownerId, item)) return;
    if (!moderationText(details.trim())) return this.validation();
    this.intent = Object.freeze({ kind: "appeal", id: item.id, intent: Object.freeze({ details: details.trim(), expected_revision: item.revision, idempotency_key: this.key() }) });
    await this.write();
  }
  async retry(): Promise<void> { if (this.intent) await this.write(); else await this.refresh(); }
  private ready(): boolean { return !!this.ownerId && !this.disposed && !this.writing && !this.intent && !this.view.loading; }
  private validation(): void { this.publish({ message: "Choose a reason and enter 20 to 4,000 characters. Judgment IDs must be unique, with at most 20.", error: true }); }
  private async write(): Promise<void> {
    const intent = this.intent, owner = this.ownerId;
    if (!intent || !owner || this.writing || this.disposed) return;
    const generation = this.generation, context = this.context, access = this.view.access;
    this.writing = true; this.reading?.abort();
    this.publish({ loading: false, message: "Submitting...", error: false });
    let receipt: ModerationCase | null = null;
    try {
      const signal = this.lifetime.signal;
      receipt = parseModerationCase(await (intent.kind === "report" ? this.source.report(owner, intent.intent, signal)
        : intent.kind === "decision" ? this.source.decide(owner, intent.id, intent.intent, signal) : this.source.appeal(owner, intent.id, intent.intent, signal)));
      if (intent.kind === "report") {
        if (receipt.object_id !== intent.intent.object_id || receipt.reporter_id !== owner || receipt.reason !== intent.intent.reason || receipt.details !== intent.intent.details) throw new Error("Invalid report receipt");
      } else {
        if (receipt.id !== intent.id || receipt.revision !== intent.intent.expected_revision + 1) throw new Error("Invalid revision receipt");
        if (intent.kind === "appeal" && (receipt.appeal?.appellant_id !== owner || receipt.appeal.details !== intent.intent.details)) throw new Error("Invalid appeal receipt");
        if (intent.kind === "decision") {
          const d = receipt.decisions.at(-1), i = intent.intent;
          if (!d || d.reviewer_id !== owner || d.outcome !== i.outcome || d.reason !== i.reason || d.explanation !== i.explanation || d.policy_version !== i.policy_version
            || JSON.stringify(d.source_signals) !== JSON.stringify(i.source_signals)) throw new Error("Invalid decision receipt");
        }
      }
    } catch (cause) {
      if (generation !== this.generation) return;
      if (!uncertainModeration(cause)) this.intent = null;
      this.writing = false;
      if (context === this.context) {
        if (cause instanceof ModerationError && cause.status === 409 && cause.code !== "report_intake_limit") await this.refresh(false, true);
        if (context === this.context && generation === this.generation) this.publish({ message: moderationMessage(cause), error: true });
      } else { this.rendered(); if (this.view.selection) void this.refresh(); }
      return;
    }
    if (generation !== this.generation) return;
    // An idempotent receipt proves this write, but another actor may have advanced the case.
    this.intent = null;
    let fresh: ModerationCase | null = null;
    try {
      fresh = parseModerationCase(await this.source.detail(owner, receipt.id, this.lifetime.signal));
      if (fresh.id !== receipt.id || fresh.object_id !== receipt.object_id || fresh.revision < receipt.revision) throw new Error("Stale moderation readback");
      if (access) this.readable(fresh, access);
    } catch { fresh = null; }
    if (generation !== this.generation) return;
    this.writing = false;
    if (context === this.context) this.publish({ selection: { kind: "detail", id: receipt.id }, detail: fresh,
      message: fresh ? intent.kind === "report" ? "Report submitted." : intent.kind === "appeal" ? "Appeal submitted." : "Decision recorded."
        : "Submission confirmed. The current case could not be refreshed. Refresh before taking another action.", error: fresh === null });
    else { this.rendered(); if (this.view.selection) void this.refresh(); }
    this.changed(fresh ?? receipt);
  }
  private readable(item: ModerationCase, access: ModerationAccess): void {
    if (access.can_review) return;
    const reporter = item.reporter_id === access.actor_id;
    if (!reporter && (item.subject_author_id !== access.actor_id || !item.decisions.length || item.reporter_id !== null || item.reason !== null || item.details !== null)) throw new Error("Invalid private case scope");
    if (item.appeal?.details !== null && item.appeal?.details !== undefined && item.appeal.appellant_id !== access.actor_id) throw new Error("Invalid private appeal scope");
  }
  private publish(patch: Partial<ModerationView>): void { this.view = { ...this.view, ...patch }; this.rendered(); }
}
