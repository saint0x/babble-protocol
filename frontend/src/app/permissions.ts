import { canonicalValueBytes, type RpcInput, type RpcOutput } from "@babble-protocol/sdk";
import type { BabbleFrontendClient } from "./protocol";

export type PermissionRequest = RpcInput<"babble.capabilities.grant.v1">["capability"];
export type PermissionReview = RpcOutput<"babble.capabilities.inspect.v1">;
type Decision = PermissionReview["decisions"][number];
export type PermissionSource = Pick<BabbleFrontendClient, "inspectPermissions" | "approvePermission" | "revokePermission">;
export interface PermissionState {
  readonly objectId: string | null;
  readonly review: PermissionReview | null;
  readonly busy: boolean;
  readonly error: string | null;
}

export function samePermission(a: PermissionRequest, b: PermissionRequest): boolean {
  if (a.id !== b.id || a.version !== b.version) return false;
  const left = canonicalValueBytes(a.scope), right = canonicalValueBytes(b.scope);
  return left.length === right.length && left.every((byte, index) => byte === right[index]);
}

export function mayApprove(decision: Decision): boolean {
  return decision.status === "requires_user" && decision.definition != null
    && !usesInvocationConsent(decision)
    && (decision.definition.permission === "ask_once" || decision.definition.permission === "ask_each_time");
}

export function usesInvocationConsent(decision: Decision): boolean {
  return decision.request.version === 1 && decision.definition?.permission === "ask_each_time"
    && ["babble.social.follow", "babble.social.unfollow", "babble.social.reply", "babble.social.share",
      "babble.clipboard.write", "babble.fullscreen.enter"].includes(decision.request.id);
}

export function permitsSurfaceStart(decision: Decision): boolean {
  return decision.status === "granted" || (decision.status === "requires_user" && usesInvocationConsent(decision));
}

export function matchingGrants(review: PermissionReview, request: PermissionRequest): PermissionReview["grants"] {
  return review.grants.filter(grant => grant.decision === "approved" && !grant.revoked_at
    && (!grant.expires_at || Date.parse(grant.expires_at) > Date.now())
    && samePermission(request, { id: grant.capability, version: grant.version, scope: grant.scope }));
}

function validateReview(review: PermissionReview, objectId: string): PermissionReview {
  if (review.manifest.object_id !== objectId || review.decisions.length !== review.manifest.requests.length
    || review.decisions.some((decision, index) => !samePermission(decision.request, review.manifest.requests[index]!))
    || review.grants.some(grant => grant.object_id !== objectId)) {
    throw new Error("Permission response does not match this Object. Refresh to try again.");
  }
  // Keep a UI selection from altering the scope used for a later approval.
  return structuredClone(review);
}

/** One review is bound to one login and Object. No grants are issued by reading it. */
export class Permissions {
  private generation = 0;
  private context: { source: PermissionSource; authorId: string; authorized: () => boolean } | null = null;
  private current: PermissionState = { objectId: null, review: null, busy: false, error: null };
  constructor(private readonly changed: (state: PermissionState) => void) {}
  get state(): PermissionState { return this.current; }

  async open(objectId: string, authorId: string, source: PermissionSource, authorized: () => boolean): Promise<void> {
    this.close();
    this.context = { authorId, source, authorized };
    this.current = { objectId, review: null, busy: false, error: null };
    await this.refresh();
  }

  close(): void {
    ++this.generation;
    this.context = null;
    this.set({ objectId: null, review: null, busy: false, error: null });
  }

  async refresh(): Promise<void> {
    const context = this.context, objectId = this.current.objectId;
    if (!context || !objectId || this.current.busy || !context.authorized()) return;
    const generation = ++this.generation;
    this.set({ ...this.current, busy: true, error: null });
    try {
      const review = validateReview(await context.source.inspectPermissions(objectId), objectId);
      if (this.live(generation)) this.set({ objectId, review, busy: false, error: null });
    } catch (cause) {
      if (this.live(generation)) this.set({ objectId, review: null, busy: false, error: message(cause) });
    }
  }

  async change(index: number, action: "approve" | "revoke"): Promise<void> {
    const context = this.context, { objectId, review, busy } = this.current;
    const decision = review?.decisions[index];
    if (!context || !objectId || !review || !decision || busy || !context.authorized()) return;
    const grants = matchingGrants(review, decision.request);
    if (action === "approve" ? !mayApprove(decision) : grants.length === 0) return;
    const generation = ++this.generation;
    this.set({ ...this.current, busy: true, error: null });
    let error: string | null = null;
    try {
      if (action === "approve") {
        await context.source.approvePermission(context.authorId, objectId, structuredClone(decision.request));
      } else {
        for (const grant of grants) {
          if (!this.live(generation)) return;
          await context.source.revokePermission(context.authorId, objectId, grant.id);
        }
      }
    } catch (cause) {
      error = `Permission change was not confirmed: ${message(cause)}`;
    }
    if (!this.live(generation)) return;
    // Reconcile an uncertain write before offering another mutation.
    try {
      const next = validateReview(await context.source.inspectPermissions(objectId), objectId);
      if (!error && action === "revoke" && matchingGrants(next, decision.request).length > 0) {
        error = "Access is still active. Another approval may have been issued; review the current permission.";
      }
      if (!error && action === "approve" && !next.decisions.some(item => samePermission(item.request, decision.request) && item.status === "granted")) {
        error = "Permission remains unavailable. Review the current host decision.";
      }
      if (this.live(generation)) this.set({ objectId, review: next, busy: false, error });
    } catch (cause) {
      if (this.live(generation)) this.set({ objectId, review: null, busy: false,
        error: `${error ?? "Permission change sent."} Refresh to confirm current access: ${message(cause)}` });
    }
  }

  private live(generation: number): boolean {
    return generation === this.generation && this.context?.authorized() === true;
  }
  private set(state: PermissionState): void { this.current = state; this.changed(state); }
}

function message(cause: unknown): string { return cause instanceof Error ? cause.message : String(cause); }
