import type { RpcOutput } from "@babble-protocol/sdk";
import { mediaKind, type MediaKind } from "./media-kind";

type MediaObject = RpcOutput<"babble.object.get.v1">["object"];
type Resource = MediaObject["resources"][number];
export interface CardMedia {
  readonly media: string | null;
  readonly mediaKind: MediaKind | null;
  readonly mediaType: string | null;
}

export interface MediaItem {
  readonly media: string;
  readonly mediaKind: MediaKind;
  readonly mediaType: string;
  readonly integrity: string;
}

export interface MediaCollection extends CardMedia {
  readonly mediaItems: readonly MediaItem[];
}

export function resolveCardMedia(object: MediaObject, apiUrl: URL): MediaCollection {
  const empty: MediaCollection = { media: null, mediaKind: null, mediaType: null, mediaItems: [] };
  const payload = object.payload;
  const hasPayload = payload !== null && typeof payload === "object" && !Array.isArray(payload);
  const hasAlbum = object.kind === "babble.media" && hasPayload && Object.hasOwn(payload, "resources");
  if (hasAlbum && !Array.isArray(payload.resources)) return empty;
  // Payload resources define the album; outer resources also carry unrelated Surface assets.
  let resources: readonly (Resource | undefined)[] = object.resources;
  if (hasAlbum && Array.isArray(payload.resources)) {
    const byHash = new Map<string, Resource>();
    for (const candidate of object.resources) {
      if (byHash.has(candidate.integrity)) return empty;
      byHash.set(candidate.integrity, candidate);
    }
    resources = payload.resources.map(declaration => {
      if (!declaration || typeof declaration !== "object" || Array.isArray(declaration)
        || typeof declaration.integrity !== "string") return undefined;
      const candidate = byHash.get(declaration.integrity);
      return candidate && matchesResource(declaration, candidate) ? candidate : undefined;
    });
  }
  const hasPrimary = hasPayload && Object.hasOwn(payload, "primary_resource");
  const primary = hasPrimary ? payload.primary_resource : null;
  const primaryHash = primary && typeof primary === "object" && !Array.isArray(primary) ? primary.integrity : null;
  const resource = hasPrimary
    ? resources.find((candidate) => candidate && typeof primaryHash === "string" && candidate.integrity === primaryHash
      && (!hasAlbum || matchesResource(primary, candidate)))
    : resources.find((candidate) => candidate && mediaKind(candidate.media_type) !== null);
  if (!resource) return empty;
  const selected = resolveResource(resource, object.id, apiUrl);
  if (!selected) return empty;
  const seen = new Set<string>();
  const mediaItems: MediaItem[] = [];
  for (const candidate of resources) {
    if (!candidate || seen.has(candidate.integrity)) continue;
    const item = candidate === resource ? selected : resolveResource(candidate, object.id, apiUrl);
    if (!item) continue;
    seen.add(item.integrity);
    mediaItems.push(item);
  }
  return { media: selected.media, mediaKind: selected.mediaKind, mediaType: selected.mediaType, mediaItems };
}

function matchesResource(declaration: unknown, resource: Resource): boolean {
  return declaration !== null && typeof declaration === "object" && !Array.isArray(declaration)
    && "integrity" in declaration && declaration.integrity === resource.integrity
    && "uri" in declaration && declaration.uri === resource.uri
    && "media_type" in declaration && declaration.media_type === resource.media_type;
}

function resolveResource(resource: Resource, objectId: string, apiUrl: URL): MediaItem | null {
  const kind = mediaKind(resource.media_type);
  if (!kind) return null;
  const result = { mediaKind: kind, mediaType: resource.media_type, integrity: resource.integrity };
  if (resource.uri === `babble://blobs/${resource.integrity}`) {
    return { ...result, media: new URL(`/objects/${encodeURIComponent(objectId)}/media/${encodeURIComponent(resource.integrity)}`, apiUrl).href };
  }
  try {
    const url = new URL(resource.uri);
    if ((url.protocol === "https:" || url.protocol === "http:") && !url.username && !url.password) {
      return { ...result, media: url.href };
    }
    if (url.protocol === "data:" && resource.uri.startsWith(`data:${resource.media_type};base64,`)) {
      return { ...result, media: resource.uri };
    }
  } catch { /* Unsupported resource locations have no browser representation. */ }
  return null;
}
