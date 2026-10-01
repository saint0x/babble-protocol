import type {
  ApiDiscoveryResponse_DiversityPolicy,
  ApiDiscoveryResponse_DiversityTrace,
  ApiDiscoveryResponse_Object,
  ApiDiscoveryResponse_RankedCandidate,
  PersonalizationEncryptedLocalUserModel,
  PersonalizationLocalUserModel,
  PersonalizationPersonalizationObjectSummary,
  PersonalizationPersonalizationSyncRecipient,
  PersonalizationPersonalizationTrace,
  PersonalizationPersonalizationTrace_PersonalizedCandidate,
  PersonalizationPersonalizationTrace_RankedCandidate,
  PersonalizationPersonalizationTrace_Reason,
} from "./generated/protocol.js";
import { assertFiniteNumbers, compareIds, diversifyRanked, validateDiversityInput } from "./diversity.js";

export type LocalUserModelInput = Partial<PersonalizationLocalUserModel>;
export type PersonalizationSyncRecipientInput = PersonalizationPersonalizationSyncRecipient;

export const PERSONALIZATION_SYNC_VERSION = "babble.personalization.sync.v1";
export const PERSONALIZATION_SYNC_DATA_CLASS = "encrypted_synchronized_state";
export const PERSONALIZATION_SYNC_ALGORITHM = "XChaCha20-Poly1305";

export function createLocalUserModel(input: LocalUserModelInput = {}): PersonalizationLocalUserModel {
  const modelRevision = normalizedOptionalString(input.model_revision);
  return {
    ...(modelRevision !== null ? { model_revision: modelRevision } : {}),
    interests: normalizedTerms(input.interests ?? []),
    expertise: normalizedTerms(input.expertise ?? []),
    muted_terms: normalizedTerms(input.muted_terms ?? []),
    hidden_terms: normalizedTerms(input.hidden_terms ?? []),
    hidden_authors: uniqueSorted(input.hidden_authors ?? []),
    creator_affinity: normalizedScoreRecord(input.creator_affinity ?? {}),
    seen_objects: normalizedCountRecord(input.seen_objects ?? {}),
    novelty_tolerance: clamp(input.novelty_tolerance ?? 0),
    exploration_preference: clamp(input.exploration_preference ?? 0),
    evidence_preference: clamp(input.evidence_preference ?? 0),
    contradiction_tolerance: clamp(input.contradiction_tolerance ?? 0),
  };
}

export function createPersonalizationFilter(
  input: LocalUserModelInput,
): (summary: PersonalizationPersonalizationObjectSummary | null) => boolean {
  const model = createLocalUserModel(input);
  return (summary) => candidateFilterReasons(model, summary).length === 0;
}

export function createPersonalizationSyncRecipient(
  input: PersonalizationSyncRecipientInput,
): PersonalizationPersonalizationSyncRecipient {
  assertIdentityId(input.identity_id);
  assertDeviceId(input.device_id);
  return {
    identity_id: input.identity_id,
    device_id: input.device_id,
  };
}

export function validateEncryptedLocalUserModelEnvelope(
  envelope: PersonalizationEncryptedLocalUserModel,
  expectedRecipient?: PersonalizationPersonalizationSyncRecipient,
): PersonalizationEncryptedLocalUserModel {
  if (envelope.version !== PERSONALIZATION_SYNC_VERSION) {
    throw new Error(`unsupported personalization sync version: ${envelope.version}`);
  }
  if (envelope.data_class !== PERSONALIZATION_SYNC_DATA_CLASS) {
    throw new Error(`unexpected personalization sync data class: ${envelope.data_class}`);
  }
  if (envelope.algorithm !== PERSONALIZATION_SYNC_ALGORITHM) {
    throw new Error(`unsupported personalization sync algorithm: ${envelope.algorithm}`);
  }
  const recipient = createPersonalizationSyncRecipient(envelope.recipient);
  if (
    expectedRecipient !== undefined &&
    (recipient.identity_id !== expectedRecipient.identity_id || recipient.device_id !== expectedRecipient.device_id)
  ) {
    throw new Error("personalization sync envelope recipient mismatch");
  }
  if (envelope.model_revision !== undefined && envelope.model_revision !== null) {
    assertModelRevision(envelope.model_revision);
  }
  assertTimestamp(envelope.exported_at);
  assertHexBytes(envelope.nonce, 24, "nonce");
  if (!isEvenHex(envelope.ciphertext) || envelope.ciphertext.length === 0) {
    throw new Error("personalization sync ciphertext must be non-empty hex");
  }
  return envelope;
}

export function personalizeCandidates(
  modelInput: LocalUserModelInput,
  candidates: readonly ApiDiscoveryResponse_RankedCandidate[],
  summaries: readonly PersonalizationPersonalizationObjectSummary[],
): PersonalizationPersonalizationTrace {
  const model = createLocalUserModel(modelInput);
  const summariesByObject = new Map(summaries.map((summary) => [summary.object_id, summary]));
  const maxPublicScore = Math.max(1, ...candidates.map((candidate) => finitePositive(candidate.score)));
  const ranked: PersonalizationPersonalizationTrace_PersonalizedCandidate[] = [];
  const filtered: PersonalizationPersonalizationTrace["filtered"] extends readonly (infer Item)[] ? Item[] : never[] = [];

  for (const candidate of candidates) {
    const objectId = candidate.candidate.object_id;
    const summary = summariesByObject.get(objectId) ?? null;
    const filterReasons = candidateFilterReasons(model, summary);
    if (filterReasons.length > 0) {
      filtered.push({ object_id: objectId, reasons: filterReasons });
      continue;
    }

    const publicScore = finitePositive(candidate.score) / maxPublicScore;
    const reasons = personalizationReasons(model, candidate, summary, publicScore);
    ranked.push({
      ranked: rankedCandidate(candidate),
      public_score: publicScore,
      personalized_score: clamp(reasons.reduce((total, reason) => total + reason.contribution, 0)),
      reasons,
    });
  }

  ranked.sort((left, right) => {
    return (
      compareNumbersDescending(right.personalized_score, left.personalized_score) ||
      compareStringsDescending(right.ranked.candidate.created_at, left.ranked.candidate.created_at) ||
      left.ranked.candidate.object_id.localeCompare(right.ranked.candidate.object_id)
    );
  });
  filtered.sort((left, right) => left.object_id.localeCompare(right.object_id));

  return {
    ...(model.model_revision !== undefined && model.model_revision !== null
      ? { model_revision: model.model_revision }
      : {}),
    privacy_boundary: "local_only",
    ranked,
    filtered,
  };
}

export function personalizeFeed(
  modelInput: LocalUserModelInput,
  candidates: readonly ApiDiscoveryResponse_RankedCandidate[],
  summaries: readonly PersonalizationPersonalizationObjectSummary[],
  policy: ApiDiscoveryResponse_DiversityPolicy,
  limit: number,
): PersonalizationPersonalizationTrace & { diversity_trace: ApiDiscoveryResponse_DiversityTrace } {
  validateDiversityInput(candidates, policy, limit);
  assertFiniteNumbers(modelInput);
  if (summaries.length > 200) throw new Error("personalizeFeed accepts at most 200 summaries");
  if (new Set(summaries.map((summary) => summary.object_id)).size !== summaries.length) {
    throw new Error("personalizeFeed contains duplicate summary IDs");
  }
  const trace = personalizeCandidates(modelInput, candidates, summaries);
  const byId = new Map(trace.ranked.map((entry) => [entry.ranked.candidate.object_id, entry]));
  const diversity_trace = diversifyRanked(
    trace.ranked.map((entry) => ({ ...entry.ranked, score: entry.personalized_score })), policy, limit,
  );
  return {
    ...trace,
    filtered: [...trace.filtered].sort((left, right) => compareIds(left.object_id, right.object_id)),
    ranked: diversity_trace.candidates.map((entry) => {
      const personalized = byId.get(entry.object_id)!;
      return {
        ...personalized,
        personalized_score: entry.diversified_score,
        reasons: [...personalized.reasons, ...entry.reasons.map((reason) => ({
          signal: `private.diversity.${reason.signal}`,
          contribution: reason.contribution,
        }))],
      };
    }),
    diversity_trace,
  };
}

export function summarizeDiscoveryObject(
  object: ApiDiscoveryResponse_Object,
): PersonalizationPersonalizationObjectSummary {
  return {
    object_id: object.id,
    author: object.author,
    kind: object.kind,
    text: objectText(object),
    topics: objectTopics(object),
  };
}

function personalizationReasons(
  model: PersonalizationLocalUserModel,
  candidate: ApiDiscoveryResponse_RankedCandidate,
  summary: PersonalizationPersonalizationObjectSummary | null,
  publicScore: number,
): PersonalizationPersonalizationTrace_Reason[] {
  const terms = summaryTerms(summary);
  const seenCount = model.seen_objects[candidate.candidate.object_id] ?? 0;
  const authorAffinity = summary ? (model.creator_affinity[summary.author] ?? 0) : 0;
  const saturationPenalty = Math.min(seenCount / 6, 0.18);
  const noveltyFit = 1 - Math.abs(candidate.candidate.signals.novelty - model.novelty_tolerance);
  const contradictionPenalty =
    candidate.candidate.signals.contradiction > model.contradiction_tolerance
      ? (candidate.candidate.signals.contradiction - model.contradiction_tolerance) * 0.18
      : 0;

  return [
    reason("public_lens_score", 0.44 * publicScore),
    reason("private.interest_match", 0.18 * termOverlap(model.interests, terms)),
    reason("private.expertise_match", 0.09 * termOverlap(model.expertise, terms)),
    reason("private.creator_affinity", 0.08 * authorAffinity),
    reason("private.novelty_fit", 0.08 * clamp(noveltyFit)),
    reason(
      "private.exploration_preference",
      0.06 * model.exploration_preference * candidate.candidate.signals.exploration,
    ),
    reason(
      "private.evidence_preference",
      0.05 * model.evidence_preference * candidate.candidate.signals.evidence_quality,
    ),
    reason("private.saturation_penalty", -saturationPenalty),
    reason("private.contradiction_penalty", -contradictionPenalty),
  ];
}

function candidateFilterReasons(
  model: PersonalizationLocalUserModel,
  summary: PersonalizationPersonalizationObjectSummary | null,
): string[] {
  if (!summary) {
    return [];
  }
  const terms = summaryTerms(summary);
  const reasons: string[] = [];
  if (model.hidden_authors.includes(summary.author)) {
    reasons.push("private.hidden_author");
  }
  if (intersects(model.hidden_terms, terms)) {
    reasons.push("private.hidden_term");
  }
  if (intersects(model.muted_terms, terms)) {
    reasons.push("private.muted_term");
  }
  return reasons;
}

function rankedCandidate(candidate: ApiDiscoveryResponse_RankedCandidate): PersonalizationPersonalizationTrace_RankedCandidate {
  return {
    candidate: {
      object_id: candidate.candidate.object_id,
      source: candidate.candidate.source,
      sources: candidate.candidate.sources.map((source) => ({
        source: source.source,
        weight: source.weight,
      })),
      created_at: candidate.candidate.created_at,
      signals: candidate.candidate.signals,
    },
    score: candidate.score,
    reasons: candidate.reasons.map((entry) => reason(entry.signal, entry.contribution)),
  };
}

function objectText(object: ApiDiscoveryResponse_Object): string {
  const payload = object.payload;
  if (!isRecord(payload)) {
    return "";
  }
  const textFields = [payload.text, payload.title, payload.description, payload.alt];
  return [...new Set(textFields.filter((value): value is string => typeof value === "string" && value.length > 0))]
    .join("\n");
}

function objectTopics(object: ApiDiscoveryResponse_Object): string[] {
  const payload = object.payload;
  if (!isRecord(payload)) {
    return [];
  }
  const rawTopics = payload.topics;
  if (!Array.isArray(rawTopics)) {
    return [];
  }
  return rawTopics.filter((topic): topic is string => typeof topic === "string");
}

function summaryTerms(summary: PersonalizationPersonalizationObjectSummary | null): Set<string> {
  if (!summary) {
    return new Set();
  }
  return new Set([
    ...tokenize(summary.kind),
    ...tokenize(summary.text),
    ...summary.topics.flatMap((topic) => tokenize(topic)),
  ]);
}

function normalizedTerms(values: readonly string[]): string[] {
  return uniqueSorted(values.flatMap((value) => tokenize(value))).slice(0, 128);
}

function tokenize(value: string): string[] {
  // Whole Unicode tokens only; this does not segment words within CJK text.
  return value.normalize("NFC").toLowerCase().normalize("NFC").match(/[\p{L}\p{N}\p{M}]+/gu) ?? [];
}

function termOverlap(privateTerms: readonly string[], objectTerms: ReadonlySet<string>): number {
  if (privateTerms.length === 0 || objectTerms.size === 0) {
    return 0;
  }
  const matches = privateTerms.filter((term) => objectTerms.has(term)).length;
  return clamp(matches / privateTerms.length);
}

function intersects(privateTerms: readonly string[], objectTerms: ReadonlySet<string>): boolean {
  return privateTerms.some((term) => objectTerms.has(term));
}

function normalizedScoreRecord(input: Record<string, number>): Record<string, number> {
  const entries: [string, number][] = Object.entries(input)
    .filter(([key]) => key.length > 0)
    .map(([key, value]) => [key, clamp(value)]);
  entries.sort(([left], [right]) => left.localeCompare(right));
  return Object.fromEntries(entries);
}

function normalizedCountRecord(input: Record<string, number>): Record<string, number> {
  const entries: [string, number][] = Object.entries(input).filter(
    (entry): entry is [string, number] => {
      const [key, value] = entry;
      return key.length > 0 && Number.isSafeInteger(value) && value > 0;
    },
  );
  entries.sort(([left], [right]) => left.localeCompare(right));
  return Object.fromEntries(entries);
}

function uniqueSorted(values: readonly string[]): string[] {
  return [...new Set(values.filter((value) => value.length > 0))].sort((left, right) => left.localeCompare(right));
}

function normalizedOptionalString(value: string | null | undefined): string | null {
  if (value === null || value === undefined) {
    return null;
  }
  const trimmed = value.trim();
  return trimmed.length > 0 ? trimmed : null;
}

function assertIdentityId(value: string): void {
  if (!/^id_[0-9a-f]{64}$/u.test(value)) {
    throw new Error("personalization sync identity_id must be a canonical IdentityId");
  }
}

function assertDeviceId(value: string): void {
  if (!/^[A-Za-z0-9._:-]{1,128}$/u.test(value)) {
    throw new Error("personalization sync device_id must be 1..128 ASCII token characters");
  }
}

function assertModelRevision(value: string): void {
  if (value.length === 0 || value.length > 128 || /[\u0000-\u001f\u007f]/u.test(value)) {
    throw new Error("personalization sync model_revision must be 1..128 non-control characters");
  }
}

function assertTimestamp(value: string): void {
  const timestamp = Date.parse(value);
  if (!Number.isFinite(timestamp)) {
    throw new Error("personalization sync exported_at must be an RFC3339 timestamp");
  }
}

function assertHexBytes(value: string, bytes: number, label: string): void {
  if (!isEvenHex(value) || value.length !== bytes * 2) {
    throw new Error(`personalization sync ${label} must be ${bytes} bytes of hex`);
  }
}

function isEvenHex(value: string): boolean {
  return value.length % 2 === 0 && /^[0-9a-f]*$/u.test(value);
}

function reason(signal: string, contribution: number): PersonalizationPersonalizationTrace_Reason {
  return { signal, contribution };
}

function finitePositive(value: number): number {
  return Number.isFinite(value) ? Math.max(0, value) : 0;
}

function clamp(value: number): number {
  if (Number.isNaN(value)) {
    return 0;
  }
  return Math.min(1, Math.max(0, value));
}

function compareNumbersDescending(right: number, left: number): number {
  return right === left ? 0 : right > left ? 1 : -1;
}

function compareStringsDescending(right: string, left: string): number {
  return right === left ? 0 : right > left ? 1 : -1;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
