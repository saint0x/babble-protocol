import type { RpcOutput } from "./generated/protocol.js";

type SurfacePlan = RpcOutput<"babble.runtime.surface.prepare.v1">["plan"];

/**
 * Admit a native-verified gateway assignment from the host's trusted session RPC.
 * This checks descriptor bindings, not signatures or BLAKE3: the SDK has no
 * BLAKE3 verifier. A descriptor supplied by an embedded document is not authority.
 */
export function verifiedBundleEntry(
  plan: SurfacePlan,
  sessionId: string,
  hostOrigin: string,
  windowOrigin: string | undefined,
  surfaceOrigin: string | undefined,
): URL | null {
  const bundle = plan.surface.bundle;
  const verification = plan.bundle_verification;
  const mount = plan.verified_mount;
  if (bundle == null) {
    if (verification != null || mount != null) fail("assignment requires a bundle");
    return null;
  }
  if (!record(verification) || !record(mount)) fail("missing verification receipt or mount descriptor");
  if (verification.policy_version !== 1 || mount.version !== 1) fail("unsupported verification or mount version");
  if (!hash(verification.manifest_hash) || !hash(mount.manifest_hash)
    || verification.manifest_hash !== mount.manifest_hash) fail("manifest hash mismatch");
  if (!identifier(sessionId) || mount.session_id !== sessionId) fail("session binding mismatch");
  if (!identifier(plan.object_id) || mount.object_id !== plan.object_id) fail("object binding mismatch");
  if (!roles.has(plan.surface.role) || mount.role !== plan.surface.role) fail("role binding mismatch");
  if (plan.surface.target !== "Web" && plan.surface.target !== "WebGpu") fail("unsupported bundle target");
  if (plan.sandbox.isolated_origin !== true || plan.sandbox.host_cookies !== false
    || plan.sandbox.top_navigation !== false || plan.sandbox.capability_bridge !== true) {
    fail("bundle requires an isolated, cookieless capability sandbox without top navigation");
  }
  if (plan.sandbox.iframe_sandbox != null && plan.sandbox.iframe_sandbox !== "allow-scripts allow-same-origin") {
    fail("unsupported verified iframe sandbox policy");
  }

  const entryPath = manifestEntryPath(bundle, plan.surface.entry, plan.surface.integrity);
  const host = exactOrigin(hostOrigin);
  if (windowOrigin !== undefined && exactOrigin(windowOrigin).origin !== host.origin) fail("host origin override mismatch");
  const origin = exactOrigin(mount.origin);
  if (origin.origin === host.origin || origin.hostname === host.hostname) fail("gateway must be separate from the host");
  if (origin.protocol !== "https:" && !(origin.protocol === "http:" && localGateway(origin.hostname))) {
    fail("gateway requires HTTPS or an explicit *.localhost HTTP origin");
  }
  if (surfaceOrigin !== undefined && surfaceOrigin !== origin.origin) fail("surface origin override mismatch");
  if (typeof mount.entry_url !== "string" || mount.entry_url !== `${origin.origin}/${entryPath}`) {
    fail("entry URL must exactly match the assigned origin and bundle entry_path");
  }
  const entry = new URL(mount.entry_url);
  if (entry.href !== mount.entry_url || entry.origin !== origin.origin || entry.pathname !== `/${entryPath}`
    || entry.username !== "" || entry.password !== "" || entry.search !== "" || entry.hash !== "") {
    fail("noncanonical gateway entry URL");
  }
  return entry;
}

const roles: ReadonlySet<string> = new Set(["Preview", "Feed", "Expanded", "Fullscreen", "Background"]);

function record(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function hash(value: unknown): value is string {
  return typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
}

function identifier(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 256 && /^[a-zA-Z0-9_-]+$/.test(value);
}

function logicalPath(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= 1024
    && /^[a-zA-Z0-9/._~-]+$/.test(value)
    && value.split("/").every((part) => part.length > 0 && part.length <= 255 && part !== "." && part !== "..");
}

function manifestEntryPath(bundle: unknown, surfaceEntry: unknown, integrity: unknown): string {
  if (!record(bundle) || bundle.version !== 1 || !logicalPath(bundle.entry_path)
    || !Array.isArray(bundle.files) || bundle.files.length < 1 || bundle.files.length > 256) {
    fail("invalid bundle manifest");
  }
  let previous = "";
  let entry: Record<string, unknown> | undefined;
  for (const file of bundle.files) {
    if (!record(file) || !logicalPath(file.path) || file.path <= previous) fail("invalid or duplicate bundle path");
    previous = file.path;
    if (file.path === bundle.entry_path) entry = file;
  }
  if (!entry || entry.kind !== "document" || entry.media_type !== "text/html"
    || !hash(entry.integrity) || entry.integrity !== integrity
    || typeof entry.source_uri !== "string" || entry.source_uri !== surfaceEntry
    || !Number.isSafeInteger(entry.size_bytes) || (entry.size_bytes as number) < 0
    || (entry.size_bytes as number) > 8 * 1024 * 1024) {
    fail("bundle entry does not match the Surface");
  }
  return bundle.entry_path;
}

function exactOrigin(value: unknown): URL {
  if (typeof value !== "string" || value.length > 2048) fail("invalid canonical HTTP(S) origin");
  let url: URL;
  try { url = new URL(value); }
  catch { return fail("invalid canonical HTTP(S) origin"); }
  if ((url.protocol !== "https:" && url.protocol !== "http:") || url.origin !== value
    || url.origin === "null" || url.username !== "" || url.password !== ""
    || url.pathname !== "/" || url.search !== "" || url.hash !== ""
    || url.hostname.endsWith(".")) fail("invalid canonical HTTP(S) origin");
  return url;
}

function localGateway(hostname: string): boolean {
  return hostname.endsWith(".localhost") && hostname.length <= 253
    && hostname.split(".").every((label) => /^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$/.test(label));
}

function fail(reason: string): never {
  throw new Error(`Babble verified bundle execution gateway: ${reason}`);
}
