import type { ProtocolTypes } from "@babble-protocol/sdk";

export const moderationReasons = ["spam", "malware", "fraud", "harassment", "illegal_content", "other_integrity"] as const satisfies readonly ModerationReason[];
export type ModerationReason = ProtocolTypes["moderation.ModerationReason"];
export type ModerationOutcome = ProtocolTypes["moderation.ModerationOutcome"];
export type ModerationScope = ProtocolTypes["moderation.ModerationScope"];
export type ModerationAccess = ProtocolTypes["moderation.ModerationAccess"];
export type ModerationDecision = ProtocolTypes["moderation.ModerationDecision"];
export type ModerationAppeal = ProtocolTypes["moderation.ModerationAppeal"];
export type ModerationCase = ProtocolTypes["moderation.ModerationCase"];
export type ModerationPage = ProtocolTypes["moderation.ModerationPage"];
export type ReportIntent = ProtocolTypes["api.ReportRequest"];
export type DecisionIntent = ProtocolTypes["api.DecisionRequest"];
export type AppealIntent = ProtocolTypes["api.AppealRequest"];
export type ModerationSource = Pick<ModerationClient, "access" | "list" | "detail" | "report" | "decide" | "appeal">;

const record = (v: unknown): v is Record<string, unknown> => !!v && typeof v === "object" && !Array.isArray(v);
export const moderationId = (v: unknown, prefix: "id" | "obj" | "jud" | "report"): v is string => typeof v === "string" && new RegExp(`^${prefix}_[a-f0-9]{64}$`).test(v);
const positive = (v: unknown): v is number => typeof v === "number" && Number.isSafeInteger(v) && v > 0;
const reason = (v: unknown): v is ModerationReason => moderationReasons.includes(v as ModerationReason);
const timestamp = (v: unknown): v is string => typeof v === "string" && v.length <= 64 && /^\d{4}-\d\d-\d\dT/.test(v) && Number.isFinite(Date.parse(v));
const policy = (v: unknown): v is string => v === "babble.integrity.v1";
const key = (v: unknown): v is string => typeof v === "string" && /^[\x21-\x7e]{1,256}$/.test(v);
const invalid = () => new Error("The node returned invalid moderation data. Refresh and try again.");

export function moderationText(value: unknown): value is string {
  if (typeof value !== "string" || value !== value.trim() || value.length > 8000) return false;
  const scalars = [...value];
  return scalars.length >= 20 && scalars.length <= 4000 && !scalars.some(c => c.length === 1 && c.charCodeAt(0) >= 0xd800 && c.charCodeAt(0) <= 0xdfff)
    && new TextEncoder().encode(value).length <= 16000;
}
export function moderationSignals(value: unknown): value is readonly string[] {
  return Array.isArray(value) && value.length <= 20 && value.every(v => moderationId(v, "jud")) && new Set(value).size === value.length;
}
export function parseModerationAccess(value: unknown, owner: string): ModerationAccess {
  if (!record(value) || !moderationId(owner, "id") || value.actor_id !== owner || typeof value.can_review !== "boolean" || !policy(value.policy_version)
    || !Array.isArray(value.reasons) || !value.reasons.length || value.reasons.length > 6 || !value.reasons.every(reason) || new Set(value.reasons).size !== value.reasons.length) throw invalid();
  return Object.freeze({ actor_id: owner, can_review: value.can_review, policy_version: value.policy_version, reasons: Object.freeze([...value.reasons]) });
}
export function parseModerationCase(value: unknown): ModerationCase {
  if (!record(value) || !moderationId(value.id, "report") || !positive(value.sequence) || !moderationId(value.object_id, "obj") || !moderationId(value.subject_author_id, "id")
    || !positive(value.revision) || !timestamp(value.created_at) || !timestamp(value.updated_at) || Date.parse(value.updated_at) < Date.parse(value.created_at)
    || !["pending", "decided", "appealed", "closed"].includes(value.status as string) || !Array.isArray(value.decisions) || value.decisions.length > 2) throw invalid();
  const redacted = value.reporter_id === null && value.reason === null && value.details === null;
  if (!redacted && (!moderationId(value.reporter_id, "id") || !reason(value.reason) || !moderationText(value.details))) throw invalid();
  const decisions = value.decisions.map((d): ModerationDecision => {
    if (!record(d) || !moderationId(d.reviewer_id, "id") || (d.outcome !== "restrict" && d.outcome !== "no_action") || !reason(d.reason)
      || !moderationText(d.explanation) || !policy(d.policy_version) || !moderationSignals(d.source_signals) || !timestamp(d.created_at)
      || d.reviewer_id === value.reporter_id || d.reviewer_id === value.subject_author_id) throw invalid();
    return Object.freeze({ reviewer_id: d.reviewer_id, outcome: d.outcome, reason: d.reason, explanation: d.explanation,
      policy_version: d.policy_version, source_signals: Object.freeze([...d.source_signals]), created_at: d.created_at });
  });
  let appeal: ModerationAppeal | null = null;
  if (value.appeal !== null) {
    const a = value.appeal;
    if (!record(a) || !timestamp(a.created_at) || !((a.appellant_id === null && a.details === null) || (moderationId(a.appellant_id, "id") && moderationText(a.details)))) throw invalid();
    appeal = Object.freeze({ appellant_id: a.appellant_id as string | null, details: a.details as string | null, created_at: a.created_at });
  }
  if ((value.status === "pending" && (decisions.length !== 0 || appeal !== null))
    || (value.status === "decided" && (decisions.length !== 1 || appeal !== null))
    || (value.status === "appealed" && (decisions.length !== 1 || appeal === null))
    || (value.status === "closed" && (decisions.length !== 2 || appeal === null))
    || (decisions.length === 2 && decisions[0]!.reviewer_id === decisions[1]!.reviewer_id)) throw invalid();
  return Object.freeze({ id: value.id, sequence: value.sequence, object_id: value.object_id, subject_author_id: value.subject_author_id,
    reporter_id: value.reporter_id as string | null, reason: value.reason as ModerationReason | null, details: value.details as string | null,
    created_at: value.created_at, updated_at: value.updated_at, revision: value.revision, status: value.status as ModerationCase["status"],
    decisions: Object.freeze(decisions), appeal });
}
export function parseModerationPage(value: unknown, before: number | null = null, limit = 25): ModerationPage {
  if (!record(value) || !Array.isArray(value.items) || value.items.length > limit || !(value.next_before === null || positive(value.next_before))) throw invalid();
  const items = value.items.map(parseModerationCase);
  const ids = new Set<string>(); let last = before ?? Infinity;
  for (const item of items) { if (item.sequence >= last || ids.has(item.id)) throw invalid(); last = item.sequence; ids.add(item.id); }
  if (value.next_before !== null && (!items.length || value.next_before !== last)) throw invalid();
  return Object.freeze({ items: Object.freeze(items), next_before: value.next_before });
}
export function reviewEligible(access: ModerationAccess | null, item: ModerationCase): boolean {
  return !!access?.can_review && item.reporter_id !== null && access.actor_id !== item.reporter_id && access.actor_id !== item.subject_author_id
    && (item.status === "pending" || (item.status === "appealed" && access.actor_id !== item.decisions[0]?.reviewer_id && access.actor_id !== item.appeal?.appellant_id));
}
export function appealEligible(owner: string | null, item: ModerationCase): boolean {
  return !!owner && item.status === "decided" && !item.appeal && ((item.decisions[0]?.outcome === "no_action" && item.reporter_id === owner)
    || (item.decisions[0]?.outcome === "restrict" && item.subject_author_id === owner));
}
export class ModerationError extends Error {
  constructor(readonly status: number, readonly code = "") {
    super(status === 409 && code === "report_intake_limit" ? "This account has reached the 1,000-report limit on this node. Existing cases can still be reviewed or appealed."
      : status === 401 ? "Sign in again to manage reports."
      : status === 403 ? "This account is not eligible for that moderation action."
      : status === 404 ? "This post or report is not available to your account."
      : status === 409 ? "This report changed elsewhere. Review the refreshed case before choosing again."
      : status === 400 || status === 422 ? "The node rejected this submission. Check the fields and any Judgment IDs."
      : status === 429 ? "Too many requests. Wait a moment before retrying."
      : "The node could not confirm the request. Retry to confirm its outcome.");
  }
}
export const moderationMessage = (cause: unknown): string => cause instanceof ModerationError ? cause.message : "Could not confirm moderation data. Check your connection and retry.";
export const uncertainModeration = (cause: unknown): boolean => !(cause instanceof ModerationError) || cause.status >= 500 || cause.status === 408;

async function boundedJson(response: Response, maxBytes: number): Promise<unknown> {
  if (!response.body) throw invalid();
  const reader = response.body.getReader(), decoder = new TextDecoder("utf-8", { fatal: true });
  let size = 0, text = "";
  try {
    while (true) { const chunk = await reader.read(); if (chunk.done) break; size += chunk.value.byteLength;
      if (size > maxBytes) throw invalid(); text += decoder.decode(chunk.value, { stream: true }); }
    return JSON.parse(text + decoder.decode()) as unknown;
  } finally { await reader.cancel().catch(() => undefined); reader.releaseLock(); }
}

/** Account-authenticated host REST. Never expose this source to Object content. */
export class ModerationClient {
  private readonly origin: URL;
  constructor(apiUrl: string, private readonly authenticatedFetch: typeof fetch) { this.origin = new URL(apiUrl); }
  async access(owner: string, signal: AbortSignal): Promise<ModerationAccess> {
    this.owner(owner); return parseModerationAccess(await this.request("/moderation/access", signal, 4096), owner);
  }
  async list(owner: string, scope: ModerationScope, before: number | null, signal: AbortSignal): Promise<ModerationPage> {
    this.owner(owner); if (!["mine", "affected", "queue"].includes(scope) || !(before === null || positive(before))) throw invalid();
    return parseModerationPage(await this.request(`/moderation/reports?scope=${scope}&limit=25${before === null ? "" : `&before=${before}`}`, signal, 2 * 1024 * 1024), before);
  }
  async detail(owner: string, id: string, signal: AbortSignal): Promise<ModerationCase> {
    this.owner(owner); this.reportId(id);
    const item = parseModerationCase(await this.request(`/moderation/reports/${id}`, signal));
    if (item.id !== id) throw invalid(); return item;
  }
  async report(owner: string, intent: ReportIntent, signal: AbortSignal): Promise<ModerationCase> {
    this.owner(owner);
    if (!moderationId(intent.object_id, "obj") || !reason(intent.reason) || !moderationText(intent.details) || !key(intent.idempotency_key)) throw invalid();
    const body = { object_id: intent.object_id, reason: intent.reason, details: intent.details, idempotency_key: intent.idempotency_key };
    const item = parseModerationCase(await this.request("/moderation/reports", signal, undefined, body));
    if (item.object_id !== intent.object_id || item.reporter_id !== owner || item.reason !== intent.reason || item.details !== intent.details) throw invalid();
    return item;
  }
  async decide(owner: string, id: string, intent: DecisionIntent, signal: AbortSignal): Promise<ModerationCase> {
    this.owner(owner); this.reportId(id);
    if (!positive(intent.expected_revision) || !key(intent.idempotency_key) || !reason(intent.reason) || !moderationText(intent.explanation)
      || !policy(intent.policy_version) || !moderationSignals(intent.source_signals) || !["restrict", "no_action"].includes(intent.outcome)) throw invalid();
    const body = { outcome: intent.outcome, reason: intent.reason, explanation: intent.explanation, policy_version: intent.policy_version,
      source_signals: [...intent.source_signals], expected_revision: intent.expected_revision, idempotency_key: intent.idempotency_key };
    const item = parseModerationCase(await this.request(`/moderation/reports/${id}/decisions`, signal, undefined, body));
    const d = item.decisions.at(-1);
    if (item.id !== id || item.revision !== intent.expected_revision + 1 || !d || d.reviewer_id !== owner || d.outcome !== intent.outcome || d.reason !== intent.reason
      || d.explanation !== intent.explanation || d.policy_version !== intent.policy_version || JSON.stringify(d.source_signals) !== JSON.stringify(intent.source_signals)) throw invalid();
    return item;
  }
  async appeal(owner: string, id: string, intent: AppealIntent, signal: AbortSignal): Promise<ModerationCase> {
    this.owner(owner); this.reportId(id);
    if (!positive(intent.expected_revision) || !key(intent.idempotency_key) || !moderationText(intent.details)) throw invalid();
    const body = { details: intent.details, expected_revision: intent.expected_revision, idempotency_key: intent.idempotency_key };
    const item = parseModerationCase(await this.request(`/moderation/reports/${id}/appeals`, signal, undefined, body));
    if (item.id !== id || item.revision !== intent.expected_revision + 1 || item.appeal?.appellant_id !== owner || item.appeal.details !== intent.details) throw invalid();
    return item;
  }
  private owner(owner: string): void { if (!moderationId(owner, "id")) throw invalid(); }
  private reportId(id: string): void { if (!moderationId(id, "report")) throw invalid(); }
  private async request(path: string, signal: AbortSignal, maxBytes = 96 * 1024, body?: ReportIntent | DecisionIntent | AppealIntent): Promise<unknown> {
    try {
      const requestSignal = AbortSignal.any([signal, AbortSignal.timeout(15000)]);
      const response = await this.authenticatedFetch(new URL(path, this.origin), { method: body ? "POST" : "GET",
        ...(body ? { body: JSON.stringify(body) } : {}), credentials: "omit", redirect: "error", cache: "no-store",
        headers: { accept: "application/json", ...(body ? { "content-type": "application/json" } : {}) }, signal: requestSignal });
      if (!response.ok) {
        let code = "";
        try { const error = await boundedJson(response, 8192); if (record(error) && error.code === "report_intake_limit") code = error.code; }
        catch { /* The status still identifies a rejected request when its error body is malformed. */ }
        throw new ModerationError(response.status, code);
      }
      const value = await boundedJson(response, maxBytes); requestSignal.throwIfAborted(); return value;
    } catch (cause) {
      if (signal.aborted || cause instanceof ModerationError) throw cause;
      if (record(cause) && typeof cause.status === "number") throw new ModerationError(cause.status);
      throw new Error("Could not confirm moderation data. Check your connection and retry.");
    }
  }
}
