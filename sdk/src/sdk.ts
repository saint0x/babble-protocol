import { BabelClient, type BabelClientOptions } from "./client.js";
import { BrowserBridgeTransport, type BridgeEndpoint, type BrowserBridgeTransportOptions } from "./bridge.js";
import type { RpcInput, RpcMethodName, RpcOutput } from "./generated/protocol.js";
import { createSurfaceLifecycle, type SurfaceLifecycleController, type SurfaceLifecycleState } from "./lifecycle.js";
import { objectBinding, type BabelTransport, type RpcRequestOptions } from "./transport.js";

type RequestOptions = Omit<RpcRequestOptions, "idempotencyKey">;
type MutationOptions = RpcRequestOptions & { readonly idempotencyKey: string };

export interface BabelSDKOptions extends BabelClientOptions {}

interface SurfaceSDKBindingOptions {
  readonly plan: RpcOutput<"babel.runtime.surface.prepare.v1">["plan"];
  readonly runtimeId: string;
  readonly surfaceSessionId: string;
  readonly origin: string;
  readonly currentIdentityId?: string | null;
}

/** Prefer the transport returned by connectSurfaceBridge for browser Surfaces. */
export type SurfaceSDKOptions = SurfaceSDKBindingOptions & (
  | { readonly transport: BabelTransport; readonly endpoint?: never; readonly targetOrigin?: never; readonly allowedOrigins?: never }
  | (BrowserBridgeTransportOptions & { readonly endpoint: BridgeEndpoint; readonly transport?: never })
);

export interface BabelSDK {
  readonly client: BabelClient;
  readonly rpc: {
    call<M extends RpcMethodName>(
      method: M,
      input: RpcInput<M>,
      options?: RpcRequestOptions,
    ): Promise<RpcOutput<M>>;
  };
  readonly identity: {
    create(
      input: RpcInput<"babel.identity.create.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.identity.create.v1">>;
    current(
      input?: RpcInput<"babel.identity.current.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.identity.current.v1">>;
  };
  readonly object: {
    publish(
      input: RpcInput<"babel.object.publish.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.object.publish.v1">>;
    publishText(
      input: RpcInput<"babel.object.publish_text.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.object.publish_text.v1">>;
    publishMedia(
      input: RpcInput<"babel.object.publish_media.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.object.publish_media.v1">>;
    fork(
      input: RpcInput<"babel.object.fork.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.object.fork.v1">>;
    remix(
      input: RpcInput<"babel.object.remix.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.object.remix.v1">>;
    get(
      input: RpcInput<"babel.object.get.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.object.get.v1">>;
  };
  readonly media: {
    putBlob(
      input: RpcInput<"babel.media.blob.put.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.media.blob.put.v1">>;
    getBlob(
      input: RpcInput<"babel.media.blob.get.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.media.blob.get.v1">>;
    readonly camera: {
      request(
        input: RpcInput<"babel.media.camera.request.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babel.media.camera.request.v1">>;
    };
    readonly microphone: {
      request(
        input: RpcInput<"babel.media.microphone.request.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babel.media.microphone.request.v1">>;
    };
  };
  readonly graph: {
    publishEdge(
      input: RpcInput<"babel.graph.edge.publish.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.graph.edge.publish.v1">>;
    traverse(
      input: RpcInput<"babel.graph.traverse.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.graph.traverse.v1">>;
    inferRelationship(
      input: RpcInput<"babel.graph.relationship.infer.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.graph.relationship.infer.v1">>;
    evidence(
      input: RpcInput<"babel.graph.evidence.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.graph.evidence.v1">>;
  };
  readonly social: {
    readonly reactions: {
      summary(input: RpcInput<"babel.social.reactions.summary.v1">, options?: RequestOptions): Promise<RpcOutput<"babel.social.reactions.summary.v1">>;
      record(input: RpcInput<"babel.social.reactions.record.v1">, options?: RequestOptions): Promise<RpcOutput<"babel.social.reactions.record.v1">>;
      mine(input: RpcInput<"babel.social.reactions.mine.v1">, options?: RequestOptions): Promise<RpcOutput<"babel.social.reactions.mine.v1">>;
      set(input: RpcInput<"babel.social.reactions.set.v1">, options: MutationOptions): Promise<RpcOutput<"babel.social.reactions.set.v1">>;
    };
    follow(
      input: RpcInput<"babel.social.follow.v2">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.social.follow.v2">>;
    unfollow(
      input: RpcInput<"babel.social.unfollow.v2">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.social.unfollow.v2">>;
    share(
      input: RpcInput<"babel.social.share.v2">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.social.share.v2">>;
    reply(
      input: RpcInput<"babel.social.reply.v2">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.social.reply.v2">>;
  };
  readonly events: {
    list(
      input: RpcInput<"babel.events.list.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.events.list.v1">>;
    bundle(
      input: RpcInput<"babel.events.bundle.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.events.bundle.v1">>;
    importBundle(
      input: RpcInput<"babel.events.import.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.events.import.v1">>;
  };
  readonly consensus: {
    previewCheckpoint(
      input: RpcInput<"babel.consensus.checkpoint.preview.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.consensus.checkpoint.preview.v1">>;
    publishCheckpoint(
      input: RpcInput<"babel.consensus.checkpoint.publish.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.consensus.checkpoint.publish.v1">>;
  };
  readonly judgment: {
    listDefinitions(
      input?: RpcInput<"babel.judgment.definitions.list.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.judgment.definitions.list.v1">>;
    listProviders(
      input?: RpcInput<"babel.judgment.providers.list.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.judgment.providers.list.v1">>;
    evaluateObject(
      input: RpcInput<"babel.judgment.object.evaluate.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.judgment.object.evaluate.v1">>;
  };
  readonly ai: {
    judge(
      input: RpcInput<"babel.ai.judge.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.ai.judge.v1">>;
    generate(
      input: RpcInput<"babel.ai.generate.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.ai.generate.v1">>;
    embed(
      input: RpcInput<"babel.ai.embed.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.ai.embed.v1">>;
    transcribe(
      input: RpcInput<"babel.ai.transcribe.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.ai.transcribe.v1">>;
  };
  readonly search: {
    objects(
      input: RpcInput<"babel.search.objects.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.search.objects.v1">>;
  };
  readonly lenses: {
    list(
      input?: RpcInput<"babel.lenses.list.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.lenses.list.v1">>;
  };
  readonly discovery: {
    candidates(
      input: RpcInput<"babel.discovery.candidates.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.discovery.candidates.v1">>;
  };
  readonly personalization: {
    readonly sync: {
      put(
        input: RpcInput<"babel.personalization.sync.put.v1">,
        options: MutationOptions,
      ): Promise<RpcOutput<"babel.personalization.sync.put.v1">>;
      list(
        input: RpcInput<"babel.personalization.sync.list.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babel.personalization.sync.list.v1">>;
      get(
        input: RpcInput<"babel.personalization.sync.get.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babel.personalization.sync.get.v1">>;
      delete(
        input: RpcInput<"babel.personalization.sync.delete.v1">,
        options: MutationOptions,
      ): Promise<RpcOutput<"babel.personalization.sync.delete.v1">>;
    };
  };
  readonly capabilities: {
    list(
      input?: RpcInput<"babel.capabilities.list.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.capabilities.list.v1">>;
    inspect(
      input: RpcInput<"babel.capabilities.inspect.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.capabilities.inspect.v1">>;
    grant(
      input: RpcInput<"babel.capabilities.grant.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.capabilities.grant.v1">>;
    revoke(
      input: RpcInput<"babel.capabilities.revoke.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.capabilities.revoke.v1">>;
  };
  readonly storage: {
    readonly object: {
      get(
        input: RpcInput<"babel.storage.object.get.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babel.storage.object.get.v1">>;
      set(
        input: RpcInput<"babel.storage.object.set.v1">,
        options: MutationOptions,
      ): Promise<RpcOutput<"babel.storage.object.set.v1">>;
      delete(
        input: RpcInput<"babel.storage.object.delete.v1">,
        options: MutationOptions,
      ): Promise<RpcOutput<"babel.storage.object.delete.v1">>;
      list(
        input: RpcInput<"babel.storage.object.list.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babel.storage.object.list.v1">>;
    };
    readonly local: {
      get(
        input: RpcInput<"babel.storage.local.get.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babel.storage.local.get.v1">>;
      set(
        input: RpcInput<"babel.storage.local.set.v1">,
        options: MutationOptions,
      ): Promise<RpcOutput<"babel.storage.local.set.v1">>;
      delete(
        input: RpcInput<"babel.storage.local.delete.v1">,
        options: MutationOptions,
      ): Promise<RpcOutput<"babel.storage.local.delete.v1">>;
      list(
        input: RpcInput<"babel.storage.local.list.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babel.storage.local.list.v1">>;
    };
  };
  readonly network: {
    fetch(
      input: RpcInput<"babel.network.fetch.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.network.fetch.v1">>;
  };
  readonly payments: {
    checkout(
      input: RpcInput<"babel.payments.checkout.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.payments.checkout.v1">>;
  };
  readonly notifications: {
    request(
      input: RpcInput<"babel.notifications.request.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.notifications.request.v1">>;
  };
  readonly clipboard: {
    write(
      input: RpcInput<"babel.clipboard.write.v2">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.clipboard.write.v2">>;
  };
  readonly fullscreen: {
    enter(
      input: RpcInput<"babel.fullscreen.enter.v2">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.fullscreen.enter.v2">>;
  };
  readonly observability: {
    snapshot(
      input?: RpcInput<"babel.observability.snapshot.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.observability.snapshot.v1">>;
  };
  readonly runtime: {
    prepareSurface(
      input: RpcInput<"babel.runtime.surface.prepare.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.runtime.surface.prepare.v1">>;
    surfaceHealth(
      input?: RpcInput<"babel.runtime.surface.health.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.runtime.surface.health.v1">>;
    startSurfaceSession(
      input: RpcInput<"babel.runtime.surface.session.start.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.runtime.surface.session.start.v1">>;
    getSurfaceSession(
      input: RpcInput<"babel.runtime.surface.session.get.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.runtime.surface.session.get.v1">>;
    heartbeatSurfaceSession(
      input?: RpcInput<"babel.runtime.surface.session.heartbeat.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.runtime.surface.session.heartbeat.v1">>;
    transitionSurfaceSession(
      input: RpcInput<"babel.runtime.surface.session.transition.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.runtime.surface.session.transition.v1">>;
    changeSurfaceBudget(
      input: RpcInput<"babel.runtime.surface.session.budget.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.runtime.surface.session.budget.v1">>;
    scheduleSurfaceSession(
      input: RpcInput<"babel.runtime.surface.session.schedule.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.runtime.surface.session.schedule.v1">>;
    applySurfaceSchedule(
      input: RpcInput<"babel.runtime.surface.session.apply_schedule.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.runtime.surface.session.apply_schedule.v1">>;
    checkpointSurfaceState(
      input: RpcInput<"babel.runtime.surface.session.state.checkpoint.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.runtime.surface.session.state.checkpoint.v1">>;
    getSurfaceState(
      input: RpcInput<"babel.runtime.surface.session.state.get.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.runtime.surface.session.state.get.v1">>;
    lifecycle(initial?: SurfaceLifecycleState): SurfaceLifecycleController;
  };
  readonly realtime: {
    defineRoom(
      input: RpcInput<"babel.realtime.room.define.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.realtime.room.define.v1">>;
    startSession(
      input: RpcInput<"babel.realtime.session.start.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babel.realtime.session.start.v1">>;
    leaveSession(
      input: RpcInput<"babel.realtime.session.leave.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.realtime.session.leave.v1">>;
    publishMessage(
      input: RpcInput<"babel.realtime.message.publish.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babel.realtime.message.publish.v1">>;
  };
  close(): void;
}

export function createBabelSDK(options: BabelSDKOptions): BabelSDK {
  const client = new BabelClient(options);
  const call = <M extends RpcMethodName>(
    method: M,
    input: RpcInput<M>,
    requestOptions?: RpcRequestOptions,
  ): Promise<RpcOutput<M>> => client.request(method, input, requestOptions);

  return {
    client,
    rpc: {
      call,
    },
    identity: {
      create: (input, requestOptions) => call("babel.identity.create.v1", input, requestOptions),
      current: (input = {}, requestOptions) => call("babel.identity.current.v1", input, requestOptions),
    },
    object: {
      publish: (input, requestOptions) => call("babel.object.publish.v1", input, requestOptions),
      publishText: (input, requestOptions) => call("babel.object.publish_text.v1", input, requestOptions),
      publishMedia: (input, requestOptions) => call("babel.object.publish_media.v1", input, requestOptions),
      fork: (input, requestOptions) => call("babel.object.fork.v1", input, requestOptions),
      remix: (input, requestOptions) => call("babel.object.remix.v1", input, requestOptions),
      get: (input, requestOptions) => call("babel.object.get.v1", input, requestOptions),
    },
    media: {
      putBlob: (input, requestOptions) => call("babel.media.blob.put.v1", input, requestOptions),
      getBlob: (input, requestOptions) => call("babel.media.blob.get.v1", input, requestOptions),
      camera: {
        request: (input, requestOptions) => call("babel.media.camera.request.v1", input, requestOptions),
      },
      microphone: {
        request: (input, requestOptions) => call("babel.media.microphone.request.v1", input, requestOptions),
      },
    },
    graph: {
      publishEdge: (input, requestOptions) => call("babel.graph.edge.publish.v1", input, requestOptions),
      traverse: (input, requestOptions) => call("babel.graph.traverse.v1", input, requestOptions),
      inferRelationship: (input, requestOptions) => call("babel.graph.relationship.infer.v1", input, requestOptions),
      evidence: (input, requestOptions) => call("babel.graph.evidence.v1", input, requestOptions),
    },
    social: {
      reactions: {
        summary: (input, requestOptions) => call("babel.social.reactions.summary.v1", input, requestOptions),
        record: (input, requestOptions) => call("babel.social.reactions.record.v1", input, requestOptions),
        mine: (input, requestOptions) => call("babel.social.reactions.mine.v1", input, requestOptions),
        set: (input, requestOptions) => call("babel.social.reactions.set.v1", input, requestOptions),
      },
      follow: (input, requestOptions) => call("babel.social.follow.v2", input, requestOptions),
      unfollow: (input, requestOptions) => call("babel.social.unfollow.v2", input, requestOptions),
      share: (input, requestOptions) => call("babel.social.share.v2", input, requestOptions),
      reply: (input, requestOptions) => call("babel.social.reply.v2", input, requestOptions),
    },
    events: {
      list: (input, requestOptions) => call("babel.events.list.v1", input, requestOptions),
      bundle: (input, requestOptions) => call("babel.events.bundle.v1", input, requestOptions),
      importBundle: (input, requestOptions) => call("babel.events.import.v1", input, requestOptions),
    },
    consensus: {
      previewCheckpoint: (input, requestOptions) => call("babel.consensus.checkpoint.preview.v1", input, requestOptions),
      publishCheckpoint: (input, requestOptions) => call("babel.consensus.checkpoint.publish.v1", input, requestOptions),
    },
    judgment: {
      listDefinitions: (input = {}, requestOptions) => call("babel.judgment.definitions.list.v1", input, requestOptions),
      listProviders: (input = {}, requestOptions) => call("babel.judgment.providers.list.v1", input, requestOptions),
      evaluateObject: (input, requestOptions) => call("babel.judgment.object.evaluate.v1", input, requestOptions),
    },
    ai: {
      judge: (input, requestOptions) => call("babel.ai.judge.v1", input, requestOptions),
      generate: (input, requestOptions) => call("babel.ai.generate.v1", input, requestOptions),
      embed: (input, requestOptions) => call("babel.ai.embed.v1", input, requestOptions),
      transcribe: (input, requestOptions) => call("babel.ai.transcribe.v1", input, requestOptions),
    },
    search: {
      objects: (input, requestOptions) => call("babel.search.objects.v1", input, requestOptions),
    },
    lenses: {
      list: (input = {}, requestOptions) => call("babel.lenses.list.v1", input, requestOptions),
    },
    discovery: {
      candidates: (input, requestOptions) => call("babel.discovery.candidates.v1", input, requestOptions),
    },
    personalization: {
      sync: {
        put: (input, requestOptions) => call("babel.personalization.sync.put.v1", input, requestOptions),
        list: (input, requestOptions) => call("babel.personalization.sync.list.v1", input, requestOptions),
        get: (input, requestOptions) => call("babel.personalization.sync.get.v1", input, requestOptions),
        delete: (input, requestOptions) => call("babel.personalization.sync.delete.v1", input, requestOptions),
      },
    },
    capabilities: {
      list: (input = {}, requestOptions) => call("babel.capabilities.list.v1", input, requestOptions),
      inspect: (input, requestOptions) => call("babel.capabilities.inspect.v1", input, requestOptions),
      grant: (input, requestOptions) => call("babel.capabilities.grant.v1", input, requestOptions),
      revoke: (input, requestOptions) => call("babel.capabilities.revoke.v1", input, requestOptions),
    },
    storage: {
      object: {
        get: (input, requestOptions) => call("babel.storage.object.get.v1", input, requestOptions),
        set: (input, requestOptions) => call("babel.storage.object.set.v1", input, requestOptions),
        delete: (input, requestOptions) => call("babel.storage.object.delete.v1", input, requestOptions),
        list: (input, requestOptions) => call("babel.storage.object.list.v1", input, requestOptions),
      },
      local: {
        get: (input, requestOptions) => call("babel.storage.local.get.v1", input, requestOptions),
        set: (input, requestOptions) => call("babel.storage.local.set.v1", input, requestOptions),
        delete: (input, requestOptions) => call("babel.storage.local.delete.v1", input, requestOptions),
        list: (input, requestOptions) => call("babel.storage.local.list.v1", input, requestOptions),
      },
    },
    network: {
      fetch: (input, requestOptions) => call("babel.network.fetch.v1", input, requestOptions),
    },
    payments: {
      checkout: (input, requestOptions) => call("babel.payments.checkout.v1", input, requestOptions),
    },
    notifications: {
      request: (input, requestOptions) => call("babel.notifications.request.v1", input, requestOptions),
    },
    clipboard: {
      write: (input, requestOptions) => call("babel.clipboard.write.v2", input, requestOptions),
    },
    fullscreen: {
      enter: (input, requestOptions) => call("babel.fullscreen.enter.v2", input, requestOptions),
    },
    observability: {
      snapshot: (input = {}, requestOptions) => call("babel.observability.snapshot.v1", input, requestOptions),
    },
    runtime: {
      prepareSurface: (input, requestOptions) => call("babel.runtime.surface.prepare.v1", input, requestOptions),
      surfaceHealth: (input = {}, requestOptions) => call("babel.runtime.surface.health.v1", input, requestOptions),
      startSurfaceSession: (input, requestOptions) => call("babel.runtime.surface.session.start.v1", input, requestOptions),
      getSurfaceSession: (input, requestOptions) => call("babel.runtime.surface.session.get.v1", input, requestOptions),
      heartbeatSurfaceSession: (input = {}, requestOptions) => call("babel.runtime.surface.session.heartbeat.v1", input, requestOptions),
      transitionSurfaceSession: (input, requestOptions) => call("babel.runtime.surface.session.transition.v1", input, requestOptions),
      changeSurfaceBudget: (input, requestOptions) => call("babel.runtime.surface.session.budget.v1", input, requestOptions),
      scheduleSurfaceSession: (input, requestOptions) => call("babel.runtime.surface.session.schedule.v1", input, requestOptions),
      applySurfaceSchedule: (input, requestOptions) => call("babel.runtime.surface.session.apply_schedule.v1", input, requestOptions),
      checkpointSurfaceState: (input, requestOptions) => call("babel.runtime.surface.session.state.checkpoint.v1", input, requestOptions),
      getSurfaceState: (input, requestOptions) => call("babel.runtime.surface.session.state.get.v1", input, requestOptions),
      lifecycle: createSurfaceLifecycle,
    },
    realtime: {
      defineRoom: (input, requestOptions) => call("babel.realtime.room.define.v1", input, requestOptions),
      startSession: (input, requestOptions) => call("babel.realtime.session.start.v1", input, requestOptions),
      leaveSession: (input, requestOptions) => call("babel.realtime.session.leave.v1", input, requestOptions),
      publishMessage: (input, requestOptions) => call("babel.realtime.message.publish.v1", input, requestOptions),
    },
    close: () => {
      client.close();
    },
  };
}

export function createSurfaceSDK(options: SurfaceSDKOptions): BabelSDK {
  if (options.plan.admission !== "ready") {
    throw new Error(`cannot bind Babel SDK for non-ready Surface admission: ${options.plan.admission}`);
  }
  const transportOptions: BrowserBridgeTransportOptions = {
    ...(options.targetOrigin !== undefined ? { targetOrigin: options.targetOrigin } : {}),
    ...(options.allowedOrigins !== undefined ? { allowedOrigins: options.allowedOrigins } : {}),
  };

  return createBabelSDK({
    transport: options.transport ?? new BrowserBridgeTransport(options.endpoint!, transportOptions),
    binding: objectBinding({
      objectId: options.plan.object_id,
      surfaceSessionId: options.surfaceSessionId,
      runtimeId: options.runtimeId,
      origin: options.origin,
      capabilityGrants: grantedCapabilityIds(options.plan),
      identityId: options.currentIdentityId ?? null,
    }),
  });
}

function grantedCapabilityIds(plan: SurfaceSDKOptions["plan"]): string[] {
  return plan.capability_decisions
    .filter((decision) => decision.status === "granted" && decision.grant !== null && decision.grant !== undefined)
    .map((decision) => decision.grant?.id)
    .filter((grantId): grantId is string => typeof grantId === "string" && grantId.length > 0);
}
