export interface LocalPreferences {
  readonly interests: readonly string[];
  readonly expertise: readonly string[];
  readonly mutedTerms: readonly string[];
  readonly hiddenTerms: readonly string[];
  readonly hiddenAuthors: readonly string[];
  readonly creatorAffinity: Record<string, number>;
  readonly noveltyTolerance: number | null;
  readonly explorationPreference: number | null;
  readonly evidencePreference: number | null;
  readonly contradictionTolerance: number | null;
}

const MAX_BYTES = 64 * 1024;
const AUTHOR_ID = /^id_[0-9a-f]{64}$/;
const FIELDS = new Set([
  "interests", "expertise", "mutedTerms", "hiddenTerms", "hiddenAuthors",
  "creatorAffinity", "noveltyTolerance", "explorationPreference",
  "evidencePreference", "contradictionTolerance",
]);

export function defaultPreferences(): LocalPreferences {
  return {
    interests: [], expertise: [], mutedTerms: [], hiddenTerms: [], hiddenAuthors: [],
    creatorAffinity: Object.create(null) as Record<string, number>,
    noveltyTolerance: null, explorationPreference: null,
    evidencePreference: null, contradictionTolerance: null,
  };
}

function record(value: unknown, field: string): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error(`${field} must be a plain object.`);
  }
  const prototype: unknown = Object.getPrototypeOf(value);
  if (prototype !== null && prototype !== Object.prototype) {
    throw new Error(`${field} must be a plain object without inherited fields.`);
  }
  for (const key of Reflect.ownKeys(value)) {
    const descriptor = Object.getOwnPropertyDescriptor(value, key);
    if (typeof key !== "string" || key === "__proto__" || key === "constructor" || key === "prototype"
      || !descriptor?.enumerable || !("value" in descriptor)) {
      throw new Error(`${field} contains an unsupported property.`);
    }
  }
  return value as Record<string, unknown>;
}

function strings(value: unknown, field: string, authors = false): string[] {
  const limit = authors ? 256 : 128;
  if (!Array.isArray(value) || value.length > limit) {
    throw new Error(`${field} must be an array with at most ${limit} entries.`);
  }
  // Reject sparse arrays, accessors, and extra properties instead of discarding them.
  if (Reflect.ownKeys(value).length !== value.length + 1) {
    throw new Error(`${field} must contain only string entries.`);
  }
  const result = new Set<string>();
  const words = new Set<string>();
  for (let index = 0; index < value.length; index += 1) {
    const descriptor = Object.getOwnPropertyDescriptor(value, String(index));
    if (!descriptor?.enumerable || !("value" in descriptor) || typeof descriptor.value !== "string") {
      throw new Error(`${field}[${index}] must be a string.`);
    }
    const item: string = descriptor.value;
    if (authors) {
      if (!AUTHOR_ID.test(item)) throw new Error(`${field}[${index}] must be a canonical author ID (id_ plus 64 lowercase hex digits).`);
      result.add(item);
    } else {
      if (item.length > MAX_BYTES) throw new Error(`${field}[${index}] exceeds the term length limit.`);
      const term = item.trim().normalize("NFC").toLowerCase().normalize("NFC");
      if (!term || Array.from(term).length > 128) {
        throw new Error(`${field}[${index}] must contain 1 to 128 Unicode code points after normalization.`);
      }
      if (/[\uD800-\uDFFF]/u.test(term)) {
        throw new Error(`${field}[${index}] contains an invalid Unicode surrogate.`);
      }
      result.add(term);
      for (const word of term.match(/[\p{L}\p{N}\p{M}]+/gu) ?? []) words.add(word);
      if (words.size > 128) throw new Error(`${field} must contain at most 128 distinct words.`);
    }
  }
  return [...result];
}

function score(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isFinite(value) || value < 0 || value > 1) {
    throw new Error(`${field} must be a finite number between 0 and 1.`);
  }
  return value;
}

function nullableScore(value: unknown, field: string): number | null {
  return value === null ? null : score(value, field);
}

function affinity(value: unknown): Record<string, number> {
  const source = record(value, "creatorAffinity");
  const keys = Object.keys(source);
  if (keys.length > 128) throw new Error("creatorAffinity must contain at most 128 entries.");
  const result = Object.create(null) as Record<string, number>;
  for (const key of keys) {
    if (!AUTHOR_ID.test(key)) throw new Error("creatorAffinity keys must be canonical author IDs (id_ plus 64 lowercase hex digits).");
    result[key] = score(source[key], "creatorAffinity score");
  }
  return result;
}

function checkSize(serialized: string): void {
  if (serialized.length > MAX_BYTES || new TextEncoder().encode(serialized).byteLength > MAX_BYTES) {
    throw new Error("Local preferences exceed the 64 KiB storage limit.");
  }
}

export function parsePreferences(value: unknown): LocalPreferences {
  const source = record(value, "Local preferences");
  for (const field of Object.keys(source)) {
    if (!FIELDS.has(field)) throw new Error("Local preferences contain an unknown field.");
  }
  for (const field of FIELDS) {
    if (!Object.hasOwn(source, field)) throw new Error(`Local preferences are missing ${field}.`);
  }
  const preferences: LocalPreferences = {
    interests: strings(source["interests"], "interests"),
    expertise: strings(source["expertise"], "expertise"),
    mutedTerms: strings(source["mutedTerms"], "mutedTerms"),
    hiddenTerms: strings(source["hiddenTerms"], "hiddenTerms"),
    hiddenAuthors: strings(source["hiddenAuthors"], "hiddenAuthors", true),
    creatorAffinity: affinity(source["creatorAffinity"]),
    noveltyTolerance: nullableScore(source["noveltyTolerance"], "noveltyTolerance"),
    explorationPreference: nullableScore(source["explorationPreference"], "explorationPreference"),
    evidencePreference: nullableScore(source["evidencePreference"], "evidencePreference"),
    contradictionTolerance: nullableScore(source["contradictionTolerance"], "contradictionTolerance"),
  };
  checkSize(JSON.stringify(preferences));
  return preferences;
}

export function readPreferences(
  storage: Pick<Storage, "getItem"> | null,
  key: string,
): { preferences: LocalPreferences; warning: string | null } {
  const fallback = (warning: string | null) => ({ preferences: defaultPreferences(), warning });
  if (!storage) return fallback("Local preference storage is unavailable.");
  let serialized: string | null;
  try {
    serialized = storage.getItem(key);
  } catch {
    return fallback("Local preferences could not be read because storage is unavailable.");
  }
  if (serialized === null) return fallback(null);
  let value: unknown;
  try {
    checkSize(serialized);
  } catch {
    return fallback("Saved local preferences exceed the 64 KiB storage limit. Saved data was left unchanged.");
  }
  try {
    value = JSON.parse(serialized) as unknown;
  } catch {
    return fallback("Saved local preferences contain invalid JSON. Saved data was left unchanged.");
  }
  try {
    return { preferences: parsePreferences(value), warning: null };
  } catch (error) {
    const detail = error instanceof Error ? error.message : "Invalid preference data.";
    return fallback(`Saved local preferences are invalid: ${detail} Saved data was left unchanged.`);
  }
}

export function writePreferences(
  storage: Pick<Storage, "setItem"> | null,
  key: string,
  value: unknown,
): LocalPreferences {
  const preferences = parsePreferences(value);
  if (!storage) throw new Error("Local preference storage is unavailable; preferences were not saved.");
  try {
    storage.setItem(key, JSON.stringify(preferences));
  } catch (cause) {
    throw new Error("Local preferences could not be saved. Storage may be unavailable or full.", { cause });
  }
  return preferences;
}

export function clearLocalHistory(storage: Pick<Storage, "removeItem"> | null, key: string): void {
  if (!storage) throw new Error("Local history storage is unavailable; history was not cleared.");
  try {
    storage.removeItem(key);
  } catch (cause) {
    throw new Error("Local history could not be cleared because storage is unavailable.", { cause });
  }
}
