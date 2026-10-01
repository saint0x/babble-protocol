import {
  BrowserSurfaceHost,
  HttpRpcTransport,
  canonicalValueBytes,
  personalizeFeed,
  summarizeDiscoveryObject,
  hostBinding,
  hostSurfaceBinding,
  type BridgeDispatch,
  type JsonValue,
  type LocalUserModelInput,
  type MountedSurface,
  type PersonalizationPersonalizationTrace_PersonalizedCandidate,
  type ProtocolTypes,
  type RpcBinding,
  type RpcInput,
  type RpcOutput,
  type RpcRequestEnvelope,
} from "@babel-protocol/sdk";
import { parseProfileIdentity, parseProfilePage, profileJson } from "./profile-response";
import { resolveCardMedia, type MediaCollection } from "./media-resource";
import type { QuotePage } from "./quotes";
import { InvocationApi, type InvocationMethod, type InvocationResult } from "./invocations";
import { BrowserInvocationApi } from "./browser-invocations";

type DiscoveryTemporalEvaluation = ProtocolTypes["discovery.TemporalResult"];

export type FeedTemporalEvaluation = Omit<DiscoveryTemporalEvaluation, "scores"> & {
  readonly score: DiscoveryTemporalEvaluation["scores"][number];
};

export interface FeedCard extends MediaCollection {
  readonly id: string;
  readonly kind: string;
  readonly author: string;
  readonly title: string;
  readonly content: string;
  readonly source: string;
  readonly createdAt: string;
  readonly schema: string;
  readonly protocol: string;
  readonly score: number | null;
  readonly rankingProvider: ProtocolTypes["lens.RankingProviderVersion"] | null;
  readonly temporal?: FeedTemporalEvaluation | undefined;
  readonly signals: {
    readonly relevance: number | null;
    readonly novelty: number | null;
    readonly evidence: number | null;
    readonly reputation: number | null;
    readonly contradiction: number | null;
  };
  readonly lineage: readonly LineageItem[];
  readonly relations: readonly RelationSummary[];
  readonly surfaces: readonly SurfaceSummary[];
  readonly capabilities: readonly CapabilitySummary[];
  readonly resourceCount: number;
  readonly reasons: readonly string[];
  readonly object: RpcOutput<"babel.object.get.v1">["object"];
}

export type PublicIdentity = ProtocolTypes["api.CreateIdentityResponse"]["identity"];
export interface ProfilePage {
  readonly identity: PublicIdentity;
  readonly cards: readonly FeedCard[];
  readonly nextCursor: string | null;
}

export class ProfileRequestError extends Error {
  constructor(readonly status: number) {
    super(status === 404 ? "This identity is not available on this node."
      : status === 409 ? "This profile has changed. Refresh to see the latest Objects."
      : `Profile request failed (${status}). Please try again.`);
  }
}

export type ObjectJudgment = ProtocolTypes["api.ObjectJudgmentsResponse"]["judgments"][number];

export interface ObjectJudgmentResult {
  readonly objectId: string;
  readonly judgments: readonly ObjectJudgment[];
}

export interface FeedResult {
  readonly cards: readonly FeedCard[];
  readonly catalogMethods: number;
  readonly source: "discovery" | "search";
  readonly personalization: FeedPersonalization;
  readonly diversity: FeedDiversity;
}

export interface FeedPersonalization {
  readonly boundary: "local_only" | "none";
  readonly filtered: number;
  readonly modelRevision: string | null;
}

export interface FeedDiversity {
  readonly active: boolean;
  readonly filtered: number;
  readonly floors: readonly string[];
  readonly maxSourceShare: number | null;
}

export type LensMode = "balanced" | "following" | "research" | "weird";
type DiscoveryLensStack = NonNullable<RpcInput<"babel.discovery.candidates.v1">["lens"]>;

export interface PlatformOverview {
  readonly capabilities: RpcOutput<"babel.capabilities.list.v1">["capabilities"];
  readonly lenses: RpcOutput<"babel.lenses.list.v1">["lenses"];
  readonly providers: RpcOutput<"babel.judgment.providers.list.v1">["providers"];
}

export type SocialTextKind = "reply" | "share";

export interface LineageItem {
  readonly label: string;
  readonly objectId: string;
}

export interface RelationSummary {
  readonly relation: string;
  readonly count: number;
}

export interface SurfaceSummary {
  readonly role: string;
  readonly target: string;
  readonly integrity: string | null;
}

export interface CapabilitySummary {
  readonly id: string;
  readonly version: number;
}

export class BabelFrontendClient {
  private static readonly actionDocuments = new Map<string, { documentId: string; createdAt: number }>();
  readonly apiUrl: URL;
  readonly transport: HttpRpcTransport;
  readonly binding = hostBinding("babel-web-runtime", globalThis.location?.origin ?? "browser://babel");
  private readonly socialControllers = new Map<string, SocialController>();
  private readonly fetchImpl: typeof fetch;

  constructor(apiUrl: string, fetchImpl?: typeof fetch, private readonly operationId?: string) {
    this.apiUrl = new URL(apiUrl);
    this.fetchImpl = fetchImpl ?? globalThis.fetch.bind(globalThis);
    this.transport = new HttpRpcTransport(new URL("/rpc", this.apiUrl), fetchImpl);
  }

  invocationApi(): InvocationApi { return new InvocationApi(this.apiUrl, this.fetchImpl); }
  browserInvocationApi(): BrowserInvocationApi { return new BrowserInvocationApi(this.apiUrl, this.fetchImpl); }

  async publicIdentity(identityId: string, signal: AbortSignal): Promise<PublicIdentity> {
    const response = await this.fetchImpl(new URL(`/identities/${encodeURIComponent(identityId)}`, this.apiUrl), {
      signal: AbortSignal.any([signal, AbortSignal.timeout(15_000)]), headers: { accept: "application/json" },
    });
    if (!response.ok) throw new ProfileRequestError(response.status);
    return parseProfileIdentity(await profileJson(response), identityId);
  }

  async profileObjects(identityId: string, cursor: string | null, signal: AbortSignal): Promise<ProfilePage> {
    const url = new URL(`/identities/${encodeURIComponent(identityId)}/objects`, this.apiUrl);
    url.searchParams.set("limit", "20");
    if (cursor !== null) url.searchParams.set("cursor", cursor);
    const response = await this.fetchImpl(url, {
      signal: AbortSignal.any([signal, AbortSignal.timeout(15_000)]), headers: { accept: "application/json" },
    });
    if (!response.ok) throw new ProfileRequestError(response.status);
    const page = parseProfilePage(await profileJson(response), identityId);
    const cards = await Promise.all(page.objects.map((object) => this.objectToCard({
      object, score: null, source: "profile", signals: null, reasons: [],
    })));
    signal.throwIfAborted();
    return { identity: page.identity, cards, nextCursor: page.next_cursor };
  }

  async loadFeed(
    query: string,
    lensMode: LensMode = "balanced",
    localModel: LocalUserModelInput | null = null,
  ): Promise<FeedResult> {
    if (lensMode === "following") throw new Error("Following requires the authenticated chronological feed.");
    const catalog = await this.catalog();
    const discovery = await this.rpc("babel.discovery.candidates.v1", {
      search: query.trim() || null,
      anchors: [],
      followed_objects: [],
      // Private filters need a bounded pool before the final nine-card selection.
      limit: localModel ? 200 : 9,
      exploration_slots: 2,
      lens: lensStack(lensMode),
    });
    const temporal = discovery.discovery.temporal;
    const rankedIds = new Set(discovery.discovery.ranked.map((entry) => entry.candidate.object_id));
    const temporalByObject = new Map<string, FeedTemporalEvaluation>(
      temporal?.scores.filter((score) => rankedIds.has(score.object_id)).map((score) => [score.object_id, {
        provider: temporal.provider, reference_time: temporal.reference_time, score,
      }]) ?? [],
    );
    const diversityByObject = new Map(
      discovery.discovery.diversity_trace.candidates.map((candidate) => [candidate.object_id, candidate]),
    );
    const diversity = feedDiversity(discovery.discovery.diversity_trace);
    if (localModel) {
      const personalized = personalizeFeed(
        localModel,
        discovery.discovery.ranked,
        discovery.discovery.objects.map(summarizeDiscoveryObject),
        discovery.discovery.diversity_trace.policy,
        9,
      );
      const finalDiversity = new Map(personalized.diversity_trace.candidates.map((entry) => [entry.object_id, entry]));
      const objectsById = new Map(discovery.discovery.objects.map((object) => [object.id, object]));
      return {
        cards: await asyncFlatMap(personalized.ranked, async (entry) => {
          const object = objectsById.get(entry.ranked.candidate.object_id);
          if (!object) {
            return [];
          }
          const card = await this.objectToCard({
            object,
            score: entry.personalized_score,
            rankingProvider: discovery.discovery.ranking_provider,
            temporal: temporalByObject.get(object.id),
            source: "local",
            signals: entry.ranked.candidate.signals,
            reasons: [
              ...diversityReasons(finalDiversity.get(object.id) ?? null, "local.diversity"),
              ...personalizedReasons(entry),
            ],
          });
          return [card];
        }),
        catalogMethods: catalog.methods.length,
        source: "discovery",
        personalization: {
          boundary: personalizationBoundary(personalized.privacy_boundary),
          filtered: personalized.filtered.length,
          modelRevision: personalized.model_revision ?? null,
        },
        diversity: feedDiversity(personalized.diversity_trace),
      };
    }
    return {
      cards: await Promise.all(discovery.discovery.objects.map((object) => {
        const trace = discovery.discovery.trace.candidates.find((candidate) => candidate.object_id === object.id);
        const ranked = discovery.discovery.ranked.find((candidate) => candidate.candidate.object_id === object.id);
        const diversityTrace = diversityByObject.get(object.id) ?? null;
        return this.objectToCard({
          object,
          score: ranked?.score ?? trace?.score ?? null,
          rankingProvider: discovery.discovery.ranking_provider,
          temporal: temporalByObject.get(object.id),
          source: "discovery",
          signals: ranked?.candidate.signals ?? null,
          reasons: [
            ...diversityReasons(diversityTrace),
            ...(ranked?.reasons
              .filter((reason) => !reason.signal.startsWith("diversity:"))
              .map((reason) => `${reason.signal} ${percentValue(reason.contribution)}`) ?? []),
          ],
        });
      })),
      catalogMethods: catalog.methods.length,
      source: "discovery",
      personalization: noPersonalization(),
      diversity,
    };
  }

  async putMediaBlob(
    mediaType: string,
    bytes: Uint8Array,
  ): Promise<RpcOutput<"babel.media.blob.put.v1">["blob"]> {
    const response = await this.rpc("babel.media.blob.put.v1", {
      media_type: mediaType,
      bytes_hex: bytesToHex(bytes),
    });
    return response.blob;
  }

  async publishMedia(
    authorId: string,
    title: string,
    description: string | null,
    resources: RpcInput<"babel.object.publish_media.v1">["resources"],
  ): Promise<RpcOutput<"babel.object.publish_media.v1">["object"]> {
    const response = await this.rpc(
      "babel.object.publish_media.v1",
      {
        author_id: authorId,
        title,
        description,
        resources,
      },
      this.binding,
      mutationKey("media", `${authorId}:${title}:${resources.map((resource) => resource.integrity).join(",")}`),
    );
    return response.object;
  }

  async prepareSurface(objectId: string): Promise<RpcOutput<"babel.runtime.surface.prepare.v1">["plan"]> {
    const response = await this.rpc("babel.runtime.surface.prepare.v1", {
      object_id: objectId,
      role: "Feed",
    });
    return response.plan;
  }

  async surfaceRuntimeHealth(): Promise<RpcOutput<"babel.runtime.surface.health.v1">["health"]> {
    const response = await this.rpc("babel.runtime.surface.health.v1", {});
    return response.health;
  }

  async platformOverview(): Promise<PlatformOverview> {
    const [capabilities, lenses, providers] = await Promise.all([
      this.rpc("babel.capabilities.list.v1", {}),
      this.rpc("babel.lenses.list.v1", {}),
      this.rpc("babel.judgment.providers.list.v1", {}),
    ]);
    return {
      capabilities: capabilities.capabilities,
      lenses: lenses.lenses,
      providers: providers.providers,
    };
  }

  async publishDraft(
    authorId: string,
    draft: RpcInput<"babel.object.publish.v1">["draft"],
  ): Promise<RpcOutput<"babel.object.publish.v1">["object"]> {
    const response = await this.rpc(
      "babel.object.publish.v1",
      { author_id: authorId, draft },
      this.binding,
      mutationKey("draft", JSON.stringify([authorId, draft])),
    );
    return response.object;
  }

  async publishText(
    authorId: string,
    text: string,
  ): Promise<RpcOutput<"babel.object.publish_text.v1">["object"]> {
    const response = await this.rpc(
      "babel.object.publish_text.v1",
      {
        author_id: authorId,
        text,
      },
      this.binding,
      mutationKey("text", `${authorId}:${text}`),
    );
    return response.object;
  }

  async socialFollow(
    authorId: string,
    targetObjectId: string,
  ): Promise<RpcOutput<"babel.social.follow.v2">["edge"]> {
    const response = await this.socialAction("babel.social.follow.v2", authorId, targetObjectId,
      { author_id: authorId, target_object_id: targetObjectId });
    return response.edge;
  }

  async replies(objectId: string, cursor: string | null = null): Promise<{
    readonly replies: readonly FeedCard[];
    readonly nextCursor: string | null;
  }> {
    const page = await this.rpc("babel.social.replies.list.v1", {
      object_id: objectId,
      cursor,
      limit: 20,
    });
    return {
      replies: await Promise.all(page.replies.map(({ object }) => this.objectToCard({
        object, score: null, source: "conversation", signals: null, reasons: [],
      }))),
      nextCursor: page.next_cursor,
    };
  }

  async describeObject(object: FeedCard["object"]): Promise<FeedCard> {
    return this.objectToCard({ object, score: null, source: "conversation", signals: null, reasons: [] });
  }

  async quotes(objectId: string, cursor: string | null, signal: AbortSignal): Promise<QuotePage> {
    const page = await this.rpc("babel.social.quotes.list.v1", {
      object_id: objectId, cursor, limit: 10,
    }, this.binding, null, signal);
    if (page.object_id !== objectId || page.quotes.length > 10 || page.quotes.some(({ edge, object }) =>
      edge.source !== objectId || edge.relation !== "quotes" || !edge.author || !edge.signature
      || (object !== null && object.id !== edge.target))) {
      throw new Error("The server returned invalid shared-post context.");
    }
    const items = await Promise.all(page.quotes.map(async ({ edge, object }) => ({
      targetId: edge.target,
      card: object === null ? null : await this.objectToCard({
        object, score: null, source: "quote", signals: null, reasons: [],
      }),
    })));
    signal.throwIfAborted();
    return { objectId, items, nextCursor: page.next_cursor };
  }

  async publicObject(objectId: string): Promise<FeedCard> {
    const { object } = await this.rpc("babel.object.get.v1", { object_id: objectId });
    if (object.id !== objectId) throw new Error("The server returned a different Object.");
    return this.objectToCard({ object, score: null, source: "object", signals: null, reasons: [] });
  }

  async socialText(
    kind: SocialTextKind,
    authorId: string,
    targetObjectId: string,
    text: string,
  ): Promise<NonNullable<RpcOutput<"babel.social.reply.v2">["object"]>> {
    return this.socialPublication(kind, authorId, targetObjectId, text);
  }

  async socialMedia(
    kind: SocialTextKind,
    authorId: string,
    targetObjectId: string,
    text: string,
    media: NonNullable<RpcInput<"babel.social.reply.v2">["media"]>,
  ): Promise<NonNullable<RpcOutput<"babel.social.reply.v2">["object"]>> {
    return this.socialPublication(kind, authorId, targetObjectId, text, media);
  }

  private async socialPublication(
    kind: SocialTextKind,
    authorId: string,
    targetObjectId: string,
    text: string,
    media?: NonNullable<RpcInput<"babel.social.reply.v2">["media"]>,
  ): Promise<NonNullable<RpcOutput<"babel.social.reply.v2">["object"]>> {
    const method = kind === "reply" ? "babel.social.reply.v2" : "babel.social.share.v2";
    const response = await this.socialAction(method, authorId, targetObjectId, {
        author_id: authorId,
        target_object_id: targetObjectId,
        text,
        ...(media ? { media } : {}),
    } as unknown as JsonValue);
    if (!response.object) throw new Error("The publication completed without its Object.");
    return response.object;
  }

  private async socialAction(method: InvocationMethod, authorId: string, targetObjectId: string, payload: JsonValue): Promise<InvocationResult> {
    const controller = await this.socialController(authorId, targetObjectId);
    const operation = this.operationId ?? crypto.randomUUID();
    const digest = await crypto.subtle.digest("SHA-256", new Uint8Array(canonicalValueBytes([operation, method, payload])));
    const requestKey = `web-social-${Array.from(new Uint8Array(digest), byte => byte.toString(16).padStart(2, "0")).join("")}`;
    const key = `${this.apiUrl.origin}:${authorId}:${requestKey}`;
    const now = Date.now();
    for (const [id, value] of BabelFrontendClient.actionDocuments) {
      if (now - value.createdAt > 300_000) BabelFrontendClient.actionDocuments.delete(id);
    }
    let document = BabelFrontendClient.actionDocuments.get(key);
    if (!document) {
      if (BabelFrontendClient.actionDocuments.size >= 256) throw new Error("Too many actions are awaiting confirmation. Please wait.");
      document = { documentId: crypto.randomUUID(), createdAt: now };
      BabelFrontendClient.actionDocuments.set(key, document);
    }
    const result = await this.invocationApi().performHost({
      actorId: authorId, objectId: controller.objectId, method, payload,
      origin: { kind: "host_action", document_id: document.documentId }, requestKey,
    }, AbortSignal.timeout(30_000));
    BabelFrontendClient.actionDocuments.delete(key);
    return result;
  }

  async objectJudgments(objectId: string): Promise<ObjectJudgmentResult> {
    const response = await fetch(new URL(`/objects/${encodeURIComponent(objectId)}/judgments`, this.apiUrl), {
      headers: { accept: "application/json" },
    });
    if (!response.ok) {
      throw new Error(`Babel Object Judgment request failed with status ${response.status}`);
    }
    const body = (await response.json()) as ProtocolTypes["api.ObjectJudgmentsResponse"];
    return {
      objectId: body.object_id,
      judgments: body.judgments,
    };
  }

  async evaluateObject(
    objectId: string,
    definition: JudgmentDefinition,
    parameters: Record<string, JsonValue> = {},
  ): Promise<RpcOutput<"babel.judgment.object.evaluate.v1">["judgment"]> {
    const response = await this.rpc("babel.judgment.object.evaluate.v1", {
      object_id: objectId,
      definition,
      parameters,
    });
    return response.judgment;
  }

  async judgmentInput(judgmentId: string): Promise<NonNullable<ProtocolTypes["api.JudgeObjectResponse"]["input"]> | null> {
    const response = await fetch(new URL(`/judgments/${encodeURIComponent(judgmentId)}`, this.apiUrl), {
      headers: { accept: "application/json" }, signal: AbortSignal.timeout(15_000),
    });
    if (!response.ok) throw new Error(`Judgment input request failed with status ${response.status}`);
    const body = await response.json() as ProtocolTypes["api.JudgeObjectResponse"];
    if (body.judgment.id !== judgmentId || (body.input && body.input.judgment_id !== judgmentId)) {
      throw new Error("Judgment input response does not match the requested evaluation");
    }
    return body.input ?? null;
  }

  async startSurfaceSession(objectId: string): Promise<RpcOutput<"babel.runtime.surface.session.start.v1">["session"]> {
    const response = await this.rpc("babel.runtime.surface.session.start.v1", {
      object_id: objectId,
      role: "Feed",
      session_id: null,
    });
    return response.session;
  }

  async inspectPermissions(objectId: string): Promise<RpcOutput<"babel.capabilities.inspect.v1">> {
    return this.rpc("babel.capabilities.inspect.v1", { object_id: objectId });
  }

  async approvePermission(authorId: string, objectId: string, capability: RpcInput<"babel.capabilities.grant.v1">["capability"]): Promise<void> {
    await this.rpc("babel.capabilities.grant.v1", {
      author_id: authorId, object_id: objectId, capability, decision: "approved",
    }, this.binding, `web-permission-${randomSuffix()}`);
  }

  async revokePermission(authorId: string, objectId: string, grantId: string): Promise<void> {
    await this.rpc("babel.capabilities.revoke.v1", {
      author_id: authorId, object_id: objectId, grant_id: grantId,
    }, this.binding, `web-permission-${randomSuffix()}`);
  }

  async transitionSurfaceSession(
    sessionId: string,
    lifecycle: RpcInput<"babel.runtime.surface.session.transition.v1">["lifecycle"],
    reason: string,
  ): Promise<RpcOutput<"babel.runtime.surface.session.transition.v1">> {
    return this.rpc(
      "babel.runtime.surface.session.transition.v1",
      {
        lifecycle,
        reason,
      },
      hostSurfaceBinding({
        runtimeId: this.binding.runtime_id,
        origin: this.binding.origin,
        surfaceSessionId: sessionId,
      }),
    );
  }

  async heartbeatSurfaceSession(
    sessionId: string,
    signal?: AbortSignal,
  ): Promise<RpcOutput<"babel.runtime.surface.session.heartbeat.v1">["lease"]> {
    const response = await this.rpc(
      "babel.runtime.surface.session.heartbeat.v1", {},
      hostSurfaceBinding({
        runtimeId: this.binding.runtime_id,
        origin: this.binding.origin,
        surfaceSessionId: sessionId,
      }),
      null, signal,
    );
    return response.lease;
  }

  async scheduleSurfaceSession(
    sessionId: string,
    input: RpcInput<"babel.runtime.surface.session.schedule.v1">["input"],
  ): Promise<RpcOutput<"babel.runtime.surface.session.schedule.v1">["decision"]> {
    const response = await this.rpc(
      "babel.runtime.surface.session.schedule.v1",
      { input },
      hostSurfaceBinding({
        runtimeId: this.binding.runtime_id,
        origin: this.binding.origin,
        surfaceSessionId: sessionId,
      }),
    );
    return response.decision;
  }

  async applySurfaceSchedule(
    sessionId: string,
    input: RpcInput<"babel.runtime.surface.session.apply_schedule.v1">["input"],
  ): Promise<RpcOutput<"babel.runtime.surface.session.apply_schedule.v1">> {
    return this.rpc(
      "babel.runtime.surface.session.apply_schedule.v1",
      { input },
      hostSurfaceBinding({
        runtimeId: this.binding.runtime_id,
        origin: this.binding.origin,
        surfaceSessionId: sessionId,
      }),
    );
  }

  async checkpointSurfaceState(
    sessionId: string,
    state: RpcInput<"babel.runtime.surface.session.state.checkpoint.v1">["state"],
    reason: string,
  ): Promise<RpcOutput<"babel.runtime.surface.session.state.checkpoint.v1">> {
    return this.rpc(
      "babel.runtime.surface.session.state.checkpoint.v1",
      { state, reason },
      hostSurfaceBinding({
        runtimeId: this.binding.runtime_id,
        origin: this.binding.origin,
        surfaceSessionId: sessionId,
      }),
    );
  }

  async getSurfaceState(
    sessionId: string,
  ): Promise<RpcOutput<"babel.runtime.surface.session.state.get.v1">["checkpoint"]> {
    const response = await this.rpc("babel.runtime.surface.session.state.get.v1", {
      session_id: sessionId,
    });
    return response.checkpoint;
  }

  bridgeDispatch(): BridgeDispatch {
    return (request: RpcRequestEnvelope, context) => this.transport.request(request, context);
  }

  async registerSurfaceDocument(sessionId: string, documentId: string, signal: AbortSignal): Promise<void> {
    signal.throwIfAborted();
    const response = await this.fetchImpl(
      new URL(`/runtime/surfaces/sessions/${encodeURIComponent(sessionId)}/document`, this.apiUrl),
      {
        method: "PUT",
        headers: { "content-type": "application/json", accept: "application/json" },
        body: JSON.stringify({ document_id: documentId }),
        signal,
      },
    );
    signal.throwIfAborted();
    if (!response.ok) throw new Error(`Surface document registration failed (${response.status}). Reopen the Object to retry.`);
    const binding: unknown = await response.json();
    signal.throwIfAborted();
    if (typeof binding !== "object" || binding === null || Array.isArray(binding)
      || !("session_id" in binding) || binding.session_id !== sessionId
      || !("document_id" in binding) || binding.document_id !== documentId) {
      throw new Error("Surface document registration returned a mismatched binding.");
    }
  }

  async objectToCard(input: {
    readonly object: RpcOutput<"babel.object.get.v1">["object"];
    readonly score: number | null;
    readonly rankingProvider?: ProtocolTypes["lens.RankingProviderVersion"];
    readonly temporal?: FeedTemporalEvaluation | undefined;
    readonly source: string;
    readonly signals: DiscoverySignals | null;
    readonly reasons: readonly string[];
  }): Promise<FeedCard> {
    return objectToCard({
      ...input,
      ...resolveCardMedia(input.object, this.apiUrl),
    });
  }

  private async socialController(
    authorId: string,
    targetObjectId: string,
  ): Promise<SocialController> {
    const key = `${authorId}:${targetObjectId}`;
    let controller = this.socialControllers.get(key);
    if (!controller) {
      const published = await this.rpc(
        "babel.object.publish.v1",
        {
          author_id: authorId,
          draft: {
            kind: "babel.text",
            schema: "babel.schema.text.v1",
            payload: {
              text: `Social action controller for ${targetObjectId}`,
              metadata: {
                purpose: "social_action_controller",
                target_object_id: targetObjectId,
              },
            },
            provenance: {
              parent: targetObjectId,
              forked_from: null,
              remixed_from: [],
            },
            resources: [],
            surfaces: [],
            capabilities: socialCapabilities(targetObjectId),
          },
        },
        this.binding,
        mutationKey("social-controller", `${authorId}:${targetObjectId}`),
      );
      controller = {
        objectId: published.object.id,
        targetObjectId,
      };
      this.socialControllers.set(key, controller);
    }
    return controller;
  }

  private async catalog(): Promise<ProtocolTypes["rpc.RpcCatalog"]> {
    const response = await fetch(new URL("/rpc/catalog", this.apiUrl), {
      headers: { accept: "application/json" },
    });
    if (!response.ok) {
      throw new Error(`Babel RPC catalog request failed with status ${response.status}`);
    }
    return (await response.json()) as ProtocolTypes["rpc.RpcCatalog"];
  }

  private rpc<M extends keyof FrontendRpc>(
    method: M,
    input: FrontendRpc[M]["input"],
    binding: RpcBinding = this.binding,
    idempotencyKey: string | null = null,
    signal?: AbortSignal,
  ): Promise<FrontendRpc[M]["output"]> {
    return this.transport.request(
      {
        protocol: "babel.rpc.v1",
        id: requestId(method),
        method,
        binding,
        payload: input as JsonValue,
        idempotency_key: idempotencyKey,
        deadline: {
          timeout_ms: 30000,
          client_started_at: new Date().toISOString(),
        },
        trace_id: null,
      },
      signal ? { signal } : {},
    ).then((response) => {
      if (response.error) {
        throw new Error(response.error.message);
      }
      if (response.result === null || response.result === undefined) {
        throw new Error(`Babel RPC response did not include a result for ${method}`);
      }
      return response.result as unknown as FrontendRpc[M]["output"];
    });
  }
}

export function mountSurface(input: {
  readonly container: HTMLElement;
  readonly plan: RpcOutput<"babel.runtime.surface.prepare.v1">["plan"];
  readonly dispatch: BridgeDispatch;
  readonly surfaceSessionId: string;
  readonly currentIdentityId?: string | null;
  readonly registerDocument: (documentId: string, signal: AbortSignal) => Promise<void>;
}): MountedSurface {
  return new BrowserSurfaceHost().mount({
    container: input.container,
    plan: input.plan,
    dispatch: input.dispatch,
    surfaceSessionId: input.surfaceSessionId,
    currentIdentityId: input.currentIdentityId ?? null,
    registerDocument: input.registerDocument,
  });
}

interface FrontendRpc {
  readonly "babel.social.quotes.list.v1": {
    readonly input: RpcInput<"babel.social.quotes.list.v1">;
    readonly output: RpcOutput<"babel.social.quotes.list.v1">;
  };
  readonly "babel.object.get.v1": {
    readonly input: RpcInput<"babel.object.get.v1">;
    readonly output: RpcOutput<"babel.object.get.v1">;
  };
  readonly "babel.object.publish_text.v1": {
    readonly input: RpcInput<"babel.object.publish_text.v1">;
    readonly output: RpcOutput<"babel.object.publish_text.v1">;
  };
  readonly "babel.object.publish.v1": {
    readonly input: RpcInput<"babel.object.publish.v1">;
    readonly output: RpcOutput<"babel.object.publish.v1">;
  };
  readonly "babel.object.publish_media.v1": {
    readonly input: RpcInput<"babel.object.publish_media.v1">;
    readonly output: RpcOutput<"babel.object.publish_media.v1">;
  };
  readonly "babel.media.blob.put.v1": {
    readonly input: RpcInput<"babel.media.blob.put.v1">;
    readonly output: RpcOutput<"babel.media.blob.put.v1">;
  };
  readonly "babel.media.blob.get.v1": {
    readonly input: RpcInput<"babel.media.blob.get.v1">;
    readonly output: RpcOutput<"babel.media.blob.get.v1">;
  };
  readonly "babel.judgment.object.evaluate.v1": {
    readonly input: RpcInput<"babel.judgment.object.evaluate.v1">;
    readonly output: RpcOutput<"babel.judgment.object.evaluate.v1">;
  };
  readonly "babel.discovery.candidates.v1": {
    readonly input: RpcInput<"babel.discovery.candidates.v1">;
    readonly output: RpcOutput<"babel.discovery.candidates.v1">;
  };
  readonly "babel.search.objects.v1": {
    readonly input: RpcInput<"babel.search.objects.v1">;
    readonly output: RpcOutput<"babel.search.objects.v1">;
  };
  readonly "babel.lenses.list.v1": {
    readonly input: RpcInput<"babel.lenses.list.v1">;
    readonly output: RpcOutput<"babel.lenses.list.v1">;
  };
  readonly "babel.judgment.providers.list.v1": {
    readonly input: RpcInput<"babel.judgment.providers.list.v1">;
    readonly output: RpcOutput<"babel.judgment.providers.list.v1">;
  };
  readonly "babel.capabilities.list.v1": {
    readonly input: RpcInput<"babel.capabilities.list.v1">;
    readonly output: RpcOutput<"babel.capabilities.list.v1">;
  };
  readonly "babel.capabilities.grant.v1": {
    readonly input: RpcInput<"babel.capabilities.grant.v1">;
    readonly output: RpcOutput<"babel.capabilities.grant.v1">;
  };
  readonly "babel.capabilities.inspect.v1": {
    readonly input: RpcInput<"babel.capabilities.inspect.v1">;
    readonly output: RpcOutput<"babel.capabilities.inspect.v1">;
  };
  readonly "babel.capabilities.revoke.v1": {
    readonly input: RpcInput<"babel.capabilities.revoke.v1">;
    readonly output: RpcOutput<"babel.capabilities.revoke.v1">;
  };
  readonly "babel.social.replies.list.v1": {
    readonly input: RpcInput<"babel.social.replies.list.v1">;
    readonly output: RpcOutput<"babel.social.replies.list.v1">;
  };
  readonly "babel.runtime.surface.prepare.v1": {
    readonly input: RpcInput<"babel.runtime.surface.prepare.v1">;
    readonly output: RpcOutput<"babel.runtime.surface.prepare.v1">;
  };
  readonly "babel.runtime.surface.health.v1": {
    readonly input: RpcInput<"babel.runtime.surface.health.v1">;
    readonly output: RpcOutput<"babel.runtime.surface.health.v1">;
  };
  readonly "babel.runtime.surface.session.start.v1": {
    readonly input: RpcInput<"babel.runtime.surface.session.start.v1">;
    readonly output: RpcOutput<"babel.runtime.surface.session.start.v1">;
  };
  readonly "babel.runtime.surface.session.heartbeat.v1": {
    readonly input: RpcInput<"babel.runtime.surface.session.heartbeat.v1">;
    readonly output: RpcOutput<"babel.runtime.surface.session.heartbeat.v1">;
  };
  readonly "babel.runtime.surface.session.transition.v1": {
    readonly input: RpcInput<"babel.runtime.surface.session.transition.v1">;
    readonly output: RpcOutput<"babel.runtime.surface.session.transition.v1">;
  };
  readonly "babel.runtime.surface.session.schedule.v1": {
    readonly input: RpcInput<"babel.runtime.surface.session.schedule.v1">;
    readonly output: RpcOutput<"babel.runtime.surface.session.schedule.v1">;
  };
  readonly "babel.runtime.surface.session.apply_schedule.v1": {
    readonly input: RpcInput<"babel.runtime.surface.session.apply_schedule.v1">;
    readonly output: RpcOutput<"babel.runtime.surface.session.apply_schedule.v1">;
  };
  readonly "babel.runtime.surface.session.state.checkpoint.v1": {
    readonly input: RpcInput<"babel.runtime.surface.session.state.checkpoint.v1">;
    readonly output: RpcOutput<"babel.runtime.surface.session.state.checkpoint.v1">;
  };
  readonly "babel.runtime.surface.session.state.get.v1": {
    readonly input: RpcInput<"babel.runtime.surface.session.state.get.v1">;
    readonly output: RpcOutput<"babel.runtime.surface.session.state.get.v1">;
  };
}

export type JudgmentDefinition = RpcInput<"babel.judgment.object.evaluate.v1">["definition"];

export const judgmentDefinitions = [
  "babel.judgment.spam.v1",
  "babel.judgment.evidence_quality.v1",
  "babel.judgment.content_analysis.v1",
  "babel.judgment.moderation.v1",
  "babel.judgment.relevance.v1",
  "babel.judgment.relationship.v1",
  "babel.judgment.source_agreement.v1",
] as const satisfies readonly JudgmentDefinition[];

interface SocialController {
  readonly objectId: string;
  readonly targetObjectId: string;
}

function socialCapabilities(targetObjectId: string): RpcInput<"babel.object.publish.v1">["draft"]["capabilities"] {
  return (["babel.social.follow", "babel.social.reply", "babel.social.share"] as const).map((id) => ({
    id,
    version: 1,
    scope: { object_id: targetObjectId },
  }));
}

function mutationKey(kind: string, material: string): string {
  const encoded = `${kind}:${material}:${Date.now()}:${randomSuffix()}`;
  let hash = 0x811c9dc5;
  for (let index = 0; index < encoded.length; index += 1) {
    hash ^= encoded.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193) >>> 0;
  }
  return `web-${kind}-${hash.toString(16).padStart(8, "0")}`;
}

function randomSuffix(): string {
  const webCrypto = globalThis.crypto as Crypto & { readonly randomUUID?: () => string };
  if (typeof webCrypto.randomUUID === "function") {
    return webCrypto.randomUUID();
  }
  const random = webCrypto.getRandomValues(new Uint32Array(2));
  return `${random[0]?.toString(16) ?? "0"}${random[1]?.toString(16) ?? "0"}`;
}

function lensStack(mode: LensMode): DiscoveryLensStack {
  if (mode === "following") {
    return {
      id: "babel.lens.stack.following.v1",
      weights: [{ lens: "Following", weight: 1 }],
    };
  }
  if (mode === "research") {
    return {
      id: "babel.lens.stack.research.v1",
      weights: [{ lens: "Research", weight: 1 }],
    };
  }
  if (mode === "weird") {
    return {
      id: "babel.lens.stack.weird.v1",
      weights: [{ lens: "Weird", weight: 1 }],
    };
  }
  return {
    id: "babel.lens.stack.balanced.v1",
    weights: [
      { lens: "Research", weight: 0.55 },
      { lens: "Weird", weight: 0.3 },
      { lens: "Following", weight: 0.15 },
    ],
  };
}

function objectToCard(input: {
  readonly object: RpcOutput<"babel.object.get.v1">["object"];
  readonly score: number | null;
  readonly rankingProvider?: ProtocolTypes["lens.RankingProviderVersion"];
  readonly temporal?: FeedTemporalEvaluation | undefined;
  readonly source: string;
  readonly signals: DiscoverySignals | null;
  readonly reasons: readonly string[];
} & MediaCollection): FeedCard {
  const { object, score, source, signals, reasons, media } = input;
  const payload = object.payload;
  const text = stringField(payload, "text") ?? stringField(payload, "description")
    ?? stringField(payload, "title") ?? "";
  const title = stringField(payload, "title") ?? firstLine(text) ?? object.kind;
  return {
    id: object.id,
    kind: object.kind,
    author: object.author,
    title,
    content: text,
    media,
    mediaKind: input.mediaKind,
    mediaType: input.mediaType,
    mediaItems: input.mediaItems,
    source,
    createdAt: object.created_at,
    schema: object.schema,
    protocol: `${object.protocol.name}@${object.protocol.version}`,
    score,
    rankingProvider: input.rankingProvider ?? null,
    temporal: (source === "discovery" || source === "local") && input.temporal?.score.object_id === object.id
      ? input.temporal : undefined,
    signals: {
      relevance: signals?.relevance ?? normalizedScore(score),
      novelty: signals?.novelty ?? null,
      evidence: signals?.evidence_quality ?? null,
      reputation: signals ? reputationScore(signals.reputation) : null,
      contradiction: signals?.contradiction ?? null,
    },
    lineage: lineage(object.provenance),
    relations: relationSummary(object.relations),
    surfaces: object.surfaces.map((surface) => ({
      role: surface.role,
      target: surface.target,
      integrity: surface.integrity ?? null,
    })),
    capabilities: object.capabilities.map((capability) => ({
      id: capability.id,
      version: capability.version,
    })),
    resourceCount: object.resources.length,
    reasons,
    object,
  };
}

type DiscoverySignals = RpcOutput<"babel.discovery.candidates.v1">["discovery"]["ranked"][number]["candidate"]["signals"];
type DiscoveryDiversityTrace = RpcOutput<"babel.discovery.candidates.v1">["discovery"]["diversity_trace"];
type DiscoveryDiversityCandidate = DiscoveryDiversityTrace["candidates"][number];

function lineage(provenance: RpcOutput<"babel.object.get.v1">["object"]["provenance"]): readonly LineageItem[] {
  const items: LineageItem[] = [];
  if (provenance.parent) {
    items.push({ label: "Parent", objectId: provenance.parent });
  }
  if (provenance.forked_from) {
    items.push({ label: "Fork", objectId: provenance.forked_from });
  }
  for (const objectId of provenance.remixed_from) {
    items.push({ label: "Remix", objectId });
  }
  return items;
}

function relationSummary(
  relations: RpcOutput<"babel.object.get.v1">["object"]["relations"],
): readonly RelationSummary[] {
  const counts = new Map<string, number>();
  for (const relation of relations) {
    const key = relationLabel(relation.relation);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return [...counts]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([relation, count]) => ({ relation, count }));
}

function relationLabel(relation: RpcOutput<"babel.object.get.v1">["object"]["relations"][number]["relation"]): string {
  return typeof relation === "string" ? relation : relation.custom;
}

function stringField(value: JsonValue, key: string): string | null {
  if (!isRecord(value)) {
    return null;
  }
  const field = value[key];
  return typeof field === "string" && field.trim().length > 0 ? field.trim() : null;
}

function firstLine(value: string): string | null {
  const line = value.split(/\r?\n/, 1)[0]?.trim();
  return line && line.length > 0 ? line : null;
}

function bounded(value: number): number {
  return Math.max(0, Math.min(1, value));
}

function normalizedScore(value: number | null): number | null {
  if (value === null) {
    return null;
  }
  return bounded(value <= 1 ? value : value / 100);
}

function reputationScore(reputation: DiscoverySignals["reputation"]): number {
  return bounded(
    reputation.evidence_quality * 0.30
      + reputation.domain_expertise * 0.25
      + reputation.epistemic_accuracy * 0.20
      + reputation.social_constructiveness * 0.15
      + reputation.creative_contribution * 0.10,
  );
}

function percentValue(value: number): string {
  return `${Math.round(value * 100)}%`;
}

function personalizedReasons(entry: PersonalizationPersonalizationTrace_PersonalizedCandidate): readonly string[] {
  const localReasons = entry.reasons
    .filter((reason) => reason.contribution !== 0 && !reason.signal.startsWith("private.diversity."))
    .sort((left, right) => Math.abs(right.contribution) - Math.abs(left.contribution))
    .slice(0, 4)
    .map((reason) => `${reason.signal} ${signedPercentValue(reason.contribution)}`);
  if (localReasons.length > 0) {
    return localReasons;
  }
  return entry.ranked.reasons.map((reason) => `${reason.signal} ${percentValue(reason.contribution)}`);
}

function diversityReasons(candidate: DiscoveryDiversityCandidate | null, prefix = "diversity"): readonly string[] {
  if (!candidate) {
    return [];
  }
  return candidate.reasons
    .filter((reason) => reason.contribution !== 0)
    .slice(0, 2)
    .map((reason) => `${prefix}.${reason.signal} ${signedPercentValue(reason.contribution)}`);
}

function signedPercentValue(value: number): string {
  const rounded = Math.round(value * 100);
  return rounded > 0 ? `+${rounded}%` : `${rounded}%`;
}

function noPersonalization(): FeedPersonalization {
  return {
    boundary: "none",
    filtered: 0,
    modelRevision: null,
  };
}

function feedDiversity(trace: DiscoveryDiversityTrace): FeedDiversity {
  return {
    active: trace.candidates.some((candidate) => candidate.reasons.length > 0),
    filtered: trace.filtered.length,
    floors: trace.policy.source_floors.map((floor) => `${floor.source} ${floor.minimum}`),
    maxSourceShare: trace.policy.max_source_share,
  };
}

function personalizationBoundary(value: string): FeedPersonalization["boundary"] {
  return value === "local_only" ? "local_only" : "none";
}

function isRecord(value: JsonValue): value is { readonly [key: string]: JsonValue } {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function requestId(method: string): string {
  const random = crypto.getRandomValues(new Uint32Array(2));
  return `web-${method.replaceAll(".", "-")}-${random[0]?.toString(16)}${random[1]?.toString(16)}`;
}

async function asyncFlatMap<T, U>(
  values: readonly T[],
  mapper: (value: T) => Promise<readonly U[]>,
): Promise<readonly U[]> {
  const chunks = await Promise.all(values.map(mapper));
  return chunks.flat();
}

function bytesToHex(bytes: Uint8Array): string {
  const alphabet = "0123456789abcdef";
  const encoded = new Uint8Array(bytes.length * 2);
  for (let index = 0; index < bytes.length; index++) {
    const byte = bytes[index]!;
    encoded[index * 2] = alphabet.charCodeAt(byte >>> 4);
    encoded[index * 2 + 1] = alphabet.charCodeAt(byte & 15);
  }
  const chunks: string[] = [];
  for (let offset = 0; offset < encoded.length; offset += 0x8000) {
    chunks.push(String.fromCharCode(...encoded.subarray(offset, offset + 0x8000)));
  }
  return chunks.join("");
}
