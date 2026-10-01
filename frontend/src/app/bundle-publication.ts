import { canonicalValueBytes, type ProtocolTypes, type RpcInput, type RpcOutput } from "@babble-protocol/sdk";
import type { BabbleFrontendClient } from "./protocol";

export interface BundleAttachment {
  readonly files: readonly File[];
  readonly entryPath: string;
  readonly capabilitiesText?: string;
}

type Manifest = ProtocolTypes["object.BundleManifest"];
type BundleFile = Manifest["files"][number];
type Draft = RpcInput<"babble.object.publish.v1">["draft"];
export type CapabilityRequest = Draft["capabilities"][number];
type BlobReceipt = RpcOutput<"babble.media.blob.put.v1">["blob"];
type SelectedFile = Pick<BundleFile, "path" | "media_type" | "kind" | "size_bytes"> & { readonly file: File };

// Keep aligned with object/src/bundle.rs and api/src/execution.rs.
const MAX_FILES = 256;
const MAX_FILE_BYTES = 8 * 1024 * 1024;
const MAX_TOTAL_BYTES = 32 * 1024 * 1024;
const MAX_MANIFEST_BYTES = 256 * 1024;
const MAX_REQUEST_BYTES = 16 * 1024 * 1024 + 64 * 1024;
const BLOB_PREFIX = "babble://blobs/";
const HASH_LENGTH = 64;
const encoder = new TextEncoder();
const INVALID_SURROGATE = /[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u;

/** Structural authoring checks only; backend capability and scope policy remains authoritative. */
export function parseBundleCapabilities(text: string): readonly CapabilityRequest[] {
  if (typeof text !== "string" || text.length > 64 * 1024 || encoder.encode(text).byteLength > 64 * 1024) {
    fail("capability declarations exceed the 64 KiB UTF-8 limit or are not text");
  }
  let parsed: CapabilityRequest["scope"];
  try { parsed = JSON.parse(text); }
  catch { fail("capability declarations must be valid JSON"); }
  validateCapabilityJsonTokens(text);
  if (!Array.isArray(parsed) || parsed.length > 64) fail("capabilities must be an array of at most 64 declarations");
  const seen = new Set<string>();
  const capabilities = parsed.map((value, index): CapabilityRequest => {
    const label = `capability ${index + 1}`;
    if (!value || typeof value !== "object" || Array.isArray(value)) fail(`${label} must be an object`);
    if (Object.keys(value).length !== 3 || !Object.hasOwn(value, "id")
      || !Object.hasOwn(value, "version") || !Object.hasOwn(value, "scope")) {
      fail(`${label} must contain exactly id, version, and scope`);
    }
    const { id, version, scope } = value;
    if (typeof id !== "string" || !/^[a-z0-9-]+(?:\.[a-z0-9-]+)+$/.test(id)) fail(`${label} has an invalid namespaced id`);
    if (typeof version !== "number" || !Number.isInteger(version) || version < 1 || version > 0xffffffff) {
      fail(`${label} version must be a positive u32 integer`);
    }
    if (!scope || typeof scope !== "object" || Array.isArray(scope)) fail(`${label} scope must be a JSON object`);
    const declaration = { id, version, scope };
    const fingerprint = Array.from(canonicalValueBytes(declaration)).join(",");
    if (seen.has(fingerprint)) fail(`${label} duplicates an earlier declaration`);
    seen.add(fingerprint);
    freezeJson(scope);
    return Object.freeze(declaration);
  });
  return Object.freeze(capabilities);
}

function freezeJson(value: CapabilityRequest["scope"]): void {
  if (value && typeof value === "object") {
    Object.values(value).forEach(freezeJson);
    Object.freeze(value);
  }
}

// JSON.parse validates syntax, but discards duplicate keys and numeric spelling.
// Check those tokens before admitting values that will be serialized for signing.
function validateCapabilityJsonTokens(text: string): void {
  const tokens = text.match(/"(?:[^"\\]|\\.)*"|-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?|[{}\[\]:,]/g) ?? [];
  const containers: (Set<string> | null)[] = [];
  for (let index = 0; index < tokens.length; index++) {
    const token = tokens[index]!;
    if (token === "{" || token === "[") {
      containers.push(token === "{" ? new Set() : null);
      if (containers.length > 16) fail("capability JSON exceeds the maximum depth of 16");
    } else if (token === "}" || token === "]") containers.pop();
    else if (token.startsWith('"')) {
      const value: string = JSON.parse(token);
      if (INVALID_SURROGATE.test(value)) fail("capability JSON contains an invalid Unicode surrogate");
      if (tokens[index + 1] === ":") {
        const keys = containers.at(-1)!;
        if (keys?.has(value)) fail("capability JSON contains a duplicate object key");
        keys?.add(value);
      }
    } else if (/^-?\d/.test(token)) {
      const number = Number(token);
      if (!Number.isFinite(number) || Object.is(number, -0)
        || (Number.isInteger(number) && !Number.isSafeInteger(number))
        || decimalValue(token) !== decimalValue(JSON.stringify(number))) {
        fail("capability JSON number cannot be represented without loss");
      }
    }
  }
}

function decimalValue(token: string): string {
  const [mantissa, exponent = "0"] = token.toLowerCase().split("e");
  const [integer, fraction = ""] = mantissa!.split(".");
  const digits = `${integer!.replace("-", "")}${fraction}`.replace(/^0+/, "");
  if (!digits) return "0";
  const coefficient = digits.replace(/0+$/, "");
  const power = BigInt(exponent) - BigInt(fraction.length) + BigInt(digits.length - coefficient.length);
  return `${token.startsWith("-") ? "-" : ""}${coefficient}e${power}`;
}

// Filename mappings use the protocol's BundleFileKind::accepts_media_type allowlist.
// Browser File.type is deliberately ignored; Web bundles do not admit WASM.
const formats: Readonly<Record<string, readonly [BundleFile["kind"], string]>> = {
  html: ["document", "text/html"], htm: ["document", "text/html"],
  js: ["script", "text/javascript"], mjs: ["script", "text/javascript"],
  css: ["stylesheet", "text/css"],
  json: ["asset", "application/json"], map: ["asset", "application/json"],
  bin: ["asset", "application/octet-stream"], txt: ["asset", "text/plain"],
  png: ["asset", "image/png"], jpg: ["asset", "image/jpeg"], jpeg: ["asset", "image/jpeg"],
  gif: ["asset", "image/gif"], webp: ["asset", "image/webp"], avif: ["asset", "image/avif"],
  svg: ["asset", "image/svg+xml"], ico: ["asset", "image/x-icon"],
  woff: ["asset", "font/woff"], woff2: ["asset", "font/woff2"],
  ttf: ["asset", "font/ttf"], otf: ["asset", "font/otf"],
  mp3: ["asset", "audio/mpeg"], oga: ["asset", "audio/ogg"], ogg: ["asset", "audio/ogg"],
  wav: ["asset", "audio/wav"], m4a: ["asset", "audio/mp4"],
  mp4: ["asset", "video/mp4"], webm: ["asset", "video/webm"], ogv: ["asset", "video/ogg"],
};

export function inspectBundleFiles(files: readonly File[]): BundleAttachment {
  const selected = inspect(files);
  const entries = htmlEntries(selected);
  const indexes = entries.filter((path) => path === "index.html" || path.endsWith("/index.html"));
  const entryPath = entries.includes("index.html") ? "index.html"
    : indexes.length === 1 ? indexes[0]! : entries[0]!;
  checkManifestSize(selected, entryPath);
  return Object.freeze({ files: Object.freeze(selected.map(({ file }) => file)), entryPath });
}

export function bundleEntries(attachment: BundleAttachment): readonly string[] {
  return Object.freeze(htmlEntries(inspect(attachment.files)));
}

export async function publishBundle(
  publisher: BabbleFrontendClient,
  authorId: string,
  text: string,
  attachment: BundleAttachment,
): Promise<RpcOutput<"babble.object.publish.v1">["object"]> {
  const capabilities = parseBundleCapabilities(attachment.capabilitiesText ?? "[]");
  const selected = inspect(attachment.files);
  const entryPath = attachment.entryPath;
  if (!htmlEntries(selected).includes(entryPath)) fail("entry must name a selected HTML file");
  if (!/^id_[0-9a-f]{64}$/.test(authorId)) fail("invalid author identity");
  if (typeof text !== "string" || !text.trim()) fail("text must not be empty");
  if (text.length > MAX_REQUEST_BYTES) fail("text exceeds the upload transport limit");
  const content = text.trim();
  if (/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/u.test(content)) {
    fail("text contains an invalid Unicode surrogate");
  }
  checkManifestSize(selected, entryPath);

  // Empty receipt strings are used only to measure widths before any upload.
  // Canonical hashes and blob URIs have fixed ASCII widths; no invented hash is published.
  const measured = measuredManifest(selected, entryPath);
  const receiptWidths = selected.length * (HASH_LENGTH * 2 + BLOB_PREFIX.length);
  const draftBytes = jsonBytes({ author_id: authorId, draft: textDraft(content, measured, capabilities) })
    + receiptWidths + HASH_LENGTH * 2 + BLOB_PREFIX.length;
  // protocol.ts supplies bounded IDs/deadlines; draftTransport's key is 74 ASCII bytes.
  // Reserve 1 KiB for those fields and JSON framing, plus the actual binding width.
  const envelopeBytes = 1024 + jsonBytes(publisher.binding);
  checkRequestSize(draftBytes + envelopeBytes);
  for (const file of selected) {
    checkRequestSize(2 * file.size_bytes + jsonBytes({ media_type: file.media_type, bytes_hex: "" }) + envelopeBytes);
  }

  const captured: { selected: SelectedFile; bytes: Uint8Array; fingerprint: string }[] = [];
  const metadataByContent = new Map<string, SelectedFile>();
  for (const file of selected) {
    const bytes = await capture(file);
    // SHA-256 identifies byte aliases locally, not the protocol's BLAKE3 integrity.
    const fingerprint = Array.from(new Uint8Array(await crypto.subtle.digest("SHA-256", bytes)),
      (byte) => byte.toString(16).padStart(2, "0")).join("");
    const previous = metadataByContent.get(fingerprint);
    if (previous && (previous.media_type !== file.media_type || previous.kind !== file.kind)) {
      fail("identical file bytes require identical MIME and kind");
    }
    metadataByContent.set(fingerprint, file);
    captured.push({ selected: file, bytes, fingerprint });
  }

  const files: BundleFile[] = [];
  const contentByHash = new Map<string, string>();
  const hashByContent = new Map<string, string>();
  for (const { selected: file, bytes, fingerprint } of captured) {
    const receipt = await publisher.putMediaBlob(file.media_type, bytes);
    validateReceipt(receipt, file);
    if ((contentByHash.has(receipt.integrity) && contentByHash.get(receipt.integrity) !== fingerprint)
      || (hashByContent.has(fingerprint) && hashByContent.get(fingerprint) !== receipt.integrity)) {
      fail("inconsistent blob receipt hashes");
    }
    contentByHash.set(receipt.integrity, fingerprint);
    hashByContent.set(fingerprint, receipt.integrity);
    files.push({ path: file.path, kind: file.kind, media_type: file.media_type,
      size_bytes: bytes.byteLength, integrity: receipt.integrity, source_uri: receipt.uri });
  }
  const manifest: Manifest = { version: 1, entry_path: entryPath, files };
  return publisher.publishDraft(authorId, textDraft(content, manifest, capabilities));
}

function inspect(files: readonly File[]): SelectedFile[] {
  if (files.length < 1 || files.length > MAX_FILES) fail("select between 1 and 256 files");
  const directory = Boolean(files[0]!.webkitRelativePath);
  let root: string | undefined;
  let total = 0;
  const selected = files.map((file): SelectedFile => {
    if (!(file instanceof File)) fail("selection must contain Files");
    const relative = file.webkitRelativePath || "";
    if (Boolean(relative) !== directory) fail("cannot mix directory and flat file selections");
    let path = file.name;
    if (directory) {
      const parts = relative.split("/");
      const selectedRoot = parts.shift()!;
      if (!selectedRoot || selectedRoot === "." || selectedRoot === ".." || /[\\\x00-\x1f\x7f]/.test(selectedRoot)
        || parts.length === 0 || parts.at(-1) !== file.name) fail("ambiguous selected directory path");
      root ??= selectedRoot;
      if (root !== selectedRoot) fail("select files from one common directory root");
      path = parts.join("/");
    } else if (file.name.includes("/")) fail("flat filenames cannot contain directory paths");
    if (!path || path.length > 1024 || !/^[a-zA-Z0-9/._~-]+$/.test(path)
      || path.split("/").some((part) => !part || part === "." || part === ".." || part.length > 255)) {
      fail("paths require URL-safe ASCII segments without traversal (1024 bytes total, 255 per segment)");
    }
    const extension = path.split("/").at(-1)!.split(".").at(-1)!.toLowerCase();
    if (extension === "wasm") fail("WASM is not supported by the Web bundle profile");
    const format = Object.hasOwn(formats, extension) ? formats[extension] : undefined;
    if (!format || !file.name.includes(".")) fail(`unsupported MIME for file: ${path}`);
    if (!Number.isSafeInteger(file.size) || file.size < 0 || file.size > MAX_FILE_BYTES) fail("file exceeds the 8 MiB limit or has invalid size");
    total += file.size;
    if (total > MAX_TOTAL_BYTES) fail("bundle exceeds the 32 MiB total limit");
    return { file, path, media_type: format[1], kind: format[0], size_bytes: file.size };
  });
  selected.sort((left, right) => left.path < right.path ? -1 : left.path > right.path ? 1 : 0);
  const paths = new Set<string>();
  for (const { path } of selected) {
    if (paths.has(path)) fail(`duplicate bundle path: ${path}`);
    const parts = path.split("/");
    parts.pop();
    while (parts.length) {
      if (paths.has(parts.join("/"))) fail("a bundle path cannot be both a file and directory");
      parts.pop();
    }
    paths.add(path);
  }
  htmlEntries(selected);
  return selected;
}

function htmlEntries(files: readonly SelectedFile[]): string[] {
  const entries = files.filter(({ kind }) => kind === "document").map(({ path }) => path);
  if (!entries.length) fail("bundle requires an HTML entry file");
  return entries;
}

function measuredManifest(files: readonly SelectedFile[], entryPath: string): Manifest {
  return { version: 1, entry_path: entryPath, files: files.map(({ path, media_type, kind, size_bytes }) => ({
    path, media_type, kind, size_bytes, integrity: "", source_uri: "",
  })) };
}

function checkManifestSize(files: readonly SelectedFile[], entryPath: string): void {
  const bytes = canonicalValueBytes(measuredManifest(files, entryPath)).byteLength
    + files.length * (HASH_LENGTH * 2 + BLOB_PREFIX.length);
  if (bytes > MAX_MANIFEST_BYTES) fail("canonical manifest exceeds the 256 KiB limit");
}

function textDraft(text: string, manifest: Manifest, capabilities: readonly CapabilityRequest[]): Draft {
  const entry = manifest.files.find(({ path }) => path === manifest.entry_path)!;
  return {
    kind: "babble.text", schema: "babble.schema.text.v1", payload: { text, metadata: {} },
    provenance: { parent: null, forked_from: null, remixed_from: [] },
    resources: [], capabilities,
    surfaces: [{ role: "Feed", target: "Web", entry: entry.source_uri, integrity: entry.integrity, bundle: manifest }],
  };
}

async function capture(file: SelectedFile): Promise<Uint8Array<ArrayBuffer>> {
  const bytes = new Uint8Array(file.size_bytes);
  const reader = file.file.stream().getReader();
  let offset = 0;
  let complete = false;
  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) { complete = true; break; }
      if (!(value instanceof Uint8Array) || offset + value.byteLength > bytes.byteLength) {
        fail(`actual bytes exceed the declared size or file limit: ${file.path}`);
      }
      bytes.set(value, offset);
      offset += value.byteLength;
    }
    if (offset !== file.size_bytes) fail(`actual bytes do not match the declared size: ${file.path}`);
    return bytes;
  } finally {
    try { if (!complete) await reader.cancel(); }
    finally { reader.releaseLock(); }
  }
}

function validateReceipt(receipt: BlobReceipt, file: SelectedFile): void {
  if (!receipt || typeof receipt.integrity !== "string" || !/^[0-9a-f]{64}$/.test(receipt.integrity)
    || receipt.uri !== `${BLOB_PREFIX}${receipt.integrity}` || receipt.size_bytes !== file.size_bytes
    || receipt.media_type !== file.media_type) fail(`invalid blob receipt for ${file.path}`);
}

function jsonBytes(value: unknown): number { return encoder.encode(JSON.stringify(value)).byteLength; }
function checkRequestSize(bytes: number): void {
  if (bytes > MAX_REQUEST_BYTES) fail("request exceeds the 16 MiB + 64 KiB transport limit");
}
function fail(message: string): never { throw new Error(`Bundle publication: ${message}`); }
