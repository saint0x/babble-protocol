import type { ProtocolTypes } from "@babel-protocol/sdk";

type Identity = ProtocolTypes["api.CreateIdentityResponse"]["identity"];
type Page = ProtocolTypes["node.AuthorObjectsPage"];
const invalid = () => new Error("The node returned an invalid profile response. Please retry.");
const record = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);
const string = (value: unknown): value is string => typeof value === "string";
const id = (value: unknown, prefix: string): boolean => string(value) && new RegExp(`^${prefix}[a-f0-9]{64}$`).test(value);
const date = (value: unknown): boolean => string(value) && Number.isFinite(Date.parse(value));
const optionalId = (value: unknown): boolean => value === null || value === undefined || id(value, "obj_");
const array = (value: unknown, predicate: (entry: unknown) => boolean): boolean => Array.isArray(value) && value.every(predicate);

export function identity(value: unknown): value is Identity {
  return record(value) && id(value.id, "id_") && string(value.handle) && value.handle.length > 0
    && string(value.kind) && ["Person", "Pseudonym", "Organization", "Service", "Application", "Agent"].includes(value.kind)
    && date(value.created_at) && record(value.public_key) && record(value.signature);
}

export function object(value: unknown, author: string): value is Page["objects"][number] {
  if (!record(value) || !id(value.id, "obj_") || value.author !== author || !date(value.created_at)
    || !string(value.kind) || !string(value.schema) || !record(value.protocol)
    || !string(value.protocol.name) || !Number.isInteger(value.protocol.version)
    || !record(value.provenance)) return false;
  return optionalId(value.provenance.parent) && optionalId(value.provenance.forked_from)
    && array(value.provenance.remixed_from, (entry) => id(entry, "obj_"))
    && array(value.relations, (entry) => record(entry) && (string(entry.relation)
      || (record(entry.relation) && string(entry.relation.custom))))
    && array(value.resources, (entry) => record(entry) && string(entry.uri) && string(entry.media_type))
    && array(value.surfaces, (entry) => record(entry) && string(entry.role) && string(entry.target)
      && (entry.integrity === null || entry.integrity === undefined || string(entry.integrity)))
    && array(value.capabilities, (entry) => record(entry) && string(entry.id) && Number.isInteger(entry.version));
}

export function parseProfileIdentity(value: unknown, requested: string): Identity {
  if (!record(value) || !identity(value.identity) || value.identity.id !== requested) throw invalid();
  return value.identity;
}

export function parseProfilePage(value: unknown, requested: string): Page {
  parseProfileIdentity(value, requested);
  if (!record(value) || !Array.isArray(value.objects) || value.objects.length > 50
    || !value.objects.every((entry) => object(entry, requested))
    || !(value.next_cursor === null || (string(value.next_cursor) && value.next_cursor.length > 0 && value.next_cursor.length <= 256))) {
    throw invalid();
  }
  return value as unknown as Page;
}

/** Bound network time and bytes independently of the server's page limit. */
export async function profileJson(response: Response): Promise<unknown> {
  if (!response.body) throw invalid();
  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let size = 0;
  let text = "";
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;
      size += value.byteLength;
      if (size > 32 * 1024 * 1024) throw new Error("This profile page is too large to display.");
      text += decoder.decode(value, { stream: true });
    }
    text += decoder.decode();
    try { return JSON.parse(text) as unknown; } catch { throw invalid(); }
  } finally {
    await reader.cancel().catch(() => undefined);
    reader.releaseLock();
  }
}
