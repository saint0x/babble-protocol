import { BabbleClient, type BabbleClientOptions } from "./client.js";
import { BrowserBridgeTransport, type BridgeEndpoint, type BrowserBridgeTransportOptions } from "./bridge.js";
import type { RpcInput, RpcMethodName, RpcOutput } from "./generated/protocol.js";
import { createSurfaceLifecycle, type SurfaceLifecycleController, type SurfaceLifecycleState } from "./lifecycle.js";
import { objectBinding, type BabbleTransport, type RpcRequestOptions } from "./transport.js";

type RequestOptions = Omit<RpcRequestOptions, "idempotencyKey">;
type MutationOptions = RpcRequestOptions & { readonly idempotencyKey: string };

export interface BabbleSDKOptions extends BabbleClientOptions {}

interface SurfaceSDKBindingOptions {
  readonly plan: RpcOutput<"babble.runtime.surface.prepare.v1">["plan"];
  readonly runtimeId: string;
  readonly surfaceSessionId: string;
  readonly origin: string;
  readonly currentIdentityId?: string | null;
}

/** Prefer the transport returned by connectSurfaceBridge for browser Surfaces. */
export type SurfaceSDKOptions = SurfaceSDKBindingOptions & (
  | { readonly transport: BabbleTransport; readonly endpoint?: never; readonly targetOrigin?: never; readonly allowedOrigins?: never }
  | (BrowserBridgeTransportOptions & { readonly endpoint: BridgeEndpoint; readonly transport?: never })
);

export interface BabbleSDK {
  readonly client: BabbleClient;
  readonly rpc: {
    call<M extends RpcMethodName>(
      method: M,
      input: RpcInput<M>,
      options?: RpcRequestOptions,
    ): Promise<RpcOutput<M>>;
  };
  readonly identity: {
    create(
      input: RpcInput<"babble.identity.create.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.identity.create.v1">>;
    current(
      input?: RpcInput<"babble.identity.current.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.identity.current.v1">>;
  };
  readonly object: {
    publish(
      input: RpcInput<"babble.object.publish.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.object.publish.v1">>;
    publishText(
      input: RpcInput<"babble.object.publish_text.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.object.publish_text.v1">>;
    publishMedia(
      input: RpcInput<"babble.object.publish_media.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.object.publish_media.v1">>;
    fork(
      input: RpcInput<"babble.object.fork.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.object.fork.v1">>;
    remix(
      input: RpcInput<"babble.object.remix.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.object.remix.v1">>;
    get(
      input: RpcInput<"babble.object.get.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.object.get.v1">>;
  };
  readonly media: {
    putBlob(
      input: RpcInput<"babble.media.blob.put.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.media.blob.put.v1">>;
    getBlob(
      input: RpcInput<"babble.media.blob.get.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.media.blob.get.v1">>;
    readonly camera: {
      request(
        input: RpcInput<"babble.media.camera.request.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babble.media.camera.request.v1">>;
    };
    readonly microphone: {
      request(
        input: RpcInput<"babble.media.microphone.request.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babble.media.microphone.request.v1">>;
    };
  };
  readonly graph: {
    publishEdge(
      input: RpcInput<"babble.graph.edge.publish.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.graph.edge.publish.v1">>;
    traverse(
      input: RpcInput<"babble.graph.traverse.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.graph.traverse.v1">>;
    inferRelationship(
      input: RpcInput<"babble.graph.relationship.infer.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.graph.relationship.infer.v1">>;
    evidence(
      input: RpcInput<"babble.graph.evidence.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.graph.evidence.v1">>;
  };
  readonly social: {
    readonly reactions: {
      summary(input: RpcInput<"babble.social.reactions.summary.v1">, options?: RequestOptions): Promise<RpcOutput<"babble.social.reactions.summary.v1">>;
      record(input: RpcInput<"babble.social.reactions.record.v1">, options?: RequestOptions): Promise<RpcOutput<"babble.social.reactions.record.v1">>;
      mine(input: RpcInput<"babble.social.reactions.mine.v1">, options?: RequestOptions): Promise<RpcOutput<"babble.social.reactions.mine.v1">>;
      set(input: RpcInput<"babble.social.reactions.set.v1">, options: MutationOptions): Promise<RpcOutput<"babble.social.reactions.set.v1">>;
    };
    follow(
      input: RpcInput<"babble.social.follow">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.social.follow">>;
    unfollow(
      input: RpcInput<"babble.social.unfollow">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.social.unfollow">>;
    share(
      input: RpcInput<"babble.social.share">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.social.share">>;
    reply(
      input: RpcInput<"babble.social.reply">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.social.reply">>;
  };
  readonly events: {
    list(
      input: RpcInput<"babble.events.list.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.events.list.v1">>;
    bundle(
      input: RpcInput<"babble.events.bundle.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.events.bundle.v1">>;
    importBundle(
      input: RpcInput<"babble.events.import.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.events.import.v1">>;
  };
  readonly consensus: {
    previewCheckpoint(
      input: RpcInput<"babble.consensus.checkpoint.preview.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.consensus.checkpoint.preview.v1">>;
    publishCheckpoint(
      input: RpcInput<"babble.consensus.checkpoint.publish.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.consensus.checkpoint.publish.v1">>;
  };
  readonly judgment: {
    listDefinitions(
      input?: RpcInput<"babble.judgment.definitions.list.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.judgment.definitions.list.v1">>;
    listProviders(
      input?: RpcInput<"babble.judgment.providers.list.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.judgment.providers.list.v1">>;
    evaluateObject(
      input: RpcInput<"babble.judgment.object.evaluate.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.judgment.object.evaluate.v1">>;
  };
  readonly ai: {
    judge(
      input: RpcInput<"babble.ai.judge.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.ai.judge.v1">>;
    generate(
      input: RpcInput<"babble.ai.generate.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.ai.generate.v1">>;
    embed(
      input: RpcInput<"babble.ai.embed.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.ai.embed.v1">>;
    transcribe(
      input: RpcInput<"babble.ai.transcribe.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.ai.transcribe.v1">>;
  };
  readonly search: {
    objects(
      input: RpcInput<"babble.search.objects.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.search.objects.v1">>;
  };
  readonly lenses: {
    list(
      input?: RpcInput<"babble.lenses.list.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.lenses.list.v1">>;
  };
  readonly discovery: {
    candidates(
      input: RpcInput<"babble.discovery.candidates.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.discovery.candidates.v1">>;
  };
  readonly personalization: {
    readonly sync: {
      put(
        input: RpcInput<"babble.personalization.sync.put.v1">,
        options: MutationOptions,
      ): Promise<RpcOutput<"babble.personalization.sync.put.v1">>;
      list(
        input: RpcInput<"babble.personalization.sync.list.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babble.personalization.sync.list.v1">>;
      get(
        input: RpcInput<"babble.personalization.sync.get.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babble.personalization.sync.get.v1">>;
      delete(
        input: RpcInput<"babble.personalization.sync.delete.v1">,
        options: MutationOptions,
      ): Promise<RpcOutput<"babble.personalization.sync.delete.v1">>;
    };
  };
  readonly capabilities: {
    list(
      input?: RpcInput<"babble.capabilities.list.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.capabilities.list.v1">>;
    inspect(
      input: RpcInput<"babble.capabilities.inspect.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.capabilities.inspect.v1">>;
    grant(
      input: RpcInput<"babble.capabilities.grant.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.capabilities.grant.v1">>;
    revoke(
      input: RpcInput<"babble.capabilities.revoke.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.capabilities.revoke.v1">>;
  };
  readonly storage: {
    readonly object: {
      get(
        input: RpcInput<"babble.storage.object.get.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babble.storage.object.get.v1">>;
      set(
        input: RpcInput<"babble.storage.object.set.v1">,
        options: MutationOptions,
      ): Promise<RpcOutput<"babble.storage.object.set.v1">>;
      delete(
        input: RpcInput<"babble.storage.object.delete.v1">,
        options: MutationOptions,
      ): Promise<RpcOutput<"babble.storage.object.delete.v1">>;
      list(
        input: RpcInput<"babble.storage.object.list.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babble.storage.object.list.v1">>;
    };
    readonly local: {
      get(
        input: RpcInput<"babble.storage.local.get.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babble.storage.local.get.v1">>;
      set(
        input: RpcInput<"babble.storage.local.set.v1">,
        options: MutationOptions,
      ): Promise<RpcOutput<"babble.storage.local.set.v1">>;
      delete(
        input: RpcInput<"babble.storage.local.delete.v1">,
        options: MutationOptions,
      ): Promise<RpcOutput<"babble.storage.local.delete.v1">>;
      list(
        input: RpcInput<"babble.storage.local.list.v1">,
        options?: RequestOptions,
      ): Promise<RpcOutput<"babble.storage.local.list.v1">>;
    };
  };
  readonly network: {
    fetch(
      input: RpcInput<"babble.network.fetch.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.network.fetch.v1">>;
  };
  readonly payments: {
    checkout(
      input: RpcInput<"babble.payments.checkout.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.payments.checkout.v1">>;
  };
  readonly notifications: {
    request(
      input: RpcInput<"babble.notifications.request.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.notifications.request.v1">>;
  };
  readonly clipboard: {
    write(
      input: RpcInput<"babble.clipboard.write">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.clipboard.write">>;
  };
  readonly fullscreen: {
    enter(
      input: RpcInput<"babble.fullscreen.enter">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.fullscreen.enter">>;
  };
  readonly observability: {
    snapshot(
      input?: RpcInput<"babble.observability.snapshot.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.observability.snapshot.v1">>;
  };
  readonly runtime: {
    prepareSurface(
      input: RpcInput<"babble.runtime.surface.prepare.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.runtime.surface.prepare.v1">>;
    surfaceHealth(
      input?: RpcInput<"babble.runtime.surface.health.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.runtime.surface.health.v1">>;
    startSurfaceSession(
      input: RpcInput<"babble.runtime.surface.session.start.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.runtime.surface.session.start.v1">>;
    getSurfaceSession(
      input: RpcInput<"babble.runtime.surface.session.get.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.runtime.surface.session.get.v1">>;
    heartbeatSurfaceSession(
      input?: RpcInput<"babble.runtime.surface.session.heartbeat.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.runtime.surface.session.heartbeat.v1">>;
    transitionSurfaceSession(
      input: RpcInput<"babble.runtime.surface.session.transition.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.runtime.surface.session.transition.v1">>;
    changeSurfaceBudget(
      input: RpcInput<"babble.runtime.surface.session.budget.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.runtime.surface.session.budget.v1">>;
    scheduleSurfaceSession(
      input: RpcInput<"babble.runtime.surface.session.schedule.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.runtime.surface.session.schedule.v1">>;
    applySurfaceSchedule(
      input: RpcInput<"babble.runtime.surface.session.apply_schedule.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.runtime.surface.session.apply_schedule.v1">>;
    checkpointSurfaceState(
      input: RpcInput<"babble.runtime.surface.session.state.checkpoint.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.runtime.surface.session.state.checkpoint.v1">>;
    getSurfaceState(
      input: RpcInput<"babble.runtime.surface.session.state.get.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.runtime.surface.session.state.get.v1">>;
    lifecycle(initial?: SurfaceLifecycleState): SurfaceLifecycleController;
  };
  readonly realtime: {
    defineRoom(
      input: RpcInput<"babble.realtime.room.define.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.realtime.room.define.v1">>;
    startSession(
      input: RpcInput<"babble.realtime.session.start.v1">,
      options?: RequestOptions,
    ): Promise<RpcOutput<"babble.realtime.session.start.v1">>;
    leaveSession(
      input: RpcInput<"babble.realtime.session.leave.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.realtime.session.leave.v1">>;
    publishMessage(
      input: RpcInput<"babble.realtime.message.publish.v1">,
      options: MutationOptions,
    ): Promise<RpcOutput<"babble.realtime.message.publish.v1">>;
  };
  close(): void;
}

export function createBabbleSDK(options: BabbleSDKOptions): BabbleSDK {
  const client = new BabbleClient(options);
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
      create: (input, requestOptions) => call("babble.identity.create.v1", input, requestOptions),
      current: (input = {}, requestOptions) => call("babble.identity.current.v1", input, requestOptions),
    },
    object: {
      publish: (input, requestOptions) => call("babble.object.publish.v1", input, requestOptions),
      publishText: (input, requestOptions) => call("babble.object.publish_text.v1", input, requestOptions),
      publishMedia: (input, requestOptions) => call("babble.object.publish_media.v1", input, requestOptions),
      fork: (input, requestOptions) => call("babble.object.fork.v1", input, requestOptions),
      remix: (input, requestOptions) => call("babble.object.remix.v1", input, requestOptions),
      get: (input, requestOptions) => call("babble.object.get.v1", input, requestOptions),
    },
    media: {
      putBlob: (input, requestOptions) => call("babble.media.blob.put.v1", input, requestOptions),
      getBlob: (input, requestOptions) => call("babble.media.blob.get.v1", input, requestOptions),
      camera: {
        request: (input, requestOptions) => call("babble.media.camera.request.v1", input, requestOptions),
      },
      microphone: {
        request: (input, requestOptions) => call("babble.media.microphone.request.v1", input, requestOptions),
      },
    },
    graph: {
      publishEdge: (input, requestOptions) => call("babble.graph.edge.publish.v1", input, requestOptions),
      traverse: (input, requestOptions) => call("babble.graph.traverse.v1", input, requestOptions),
      inferRelationship: (input, requestOptions) => call("babble.graph.relationship.infer.v1", input, requestOptions),
      evidence: (input, requestOptions) => call("babble.graph.evidence.v1", input, requestOptions),
    },
    social: {
      reactions: {
        summary: (input, requestOptions) => call("babble.social.reactions.summary.v1", input, requestOptions),
        record: (input, requestOptions) => call("babble.social.reactions.record.v1", input, requestOptions),
        mine: (input, requestOptions) => call("babble.social.reactions.mine.v1", input, requestOptions),
        set: (input, requestOptions) => call("babble.social.reactions.set.v1", input, requestOptions),
      },
      follow: (input, requestOptions) => call("babble.social.follow", input, requestOptions),
      unfollow: (input, requestOptions) => call("babble.social.unfollow", input, requestOptions),
      share: (input, requestOptions) => call("babble.social.share", input, requestOptions),
      reply: (input, requestOptions) => call("babble.social.reply", input, requestOptions),
    },
    events: {
      list: (input, requestOptions) => call("babble.events.list.v1", input, requestOptions),
      bundle: (input, requestOptions) => call("babble.events.bundle.v1", input, requestOptions),
      importBundle: (input, requestOptions) => call("babble.events.import.v1", input, requestOptions),
    },
    consensus: {
      previewCheckpoint: (input, requestOptions) => call("babble.consensus.checkpoint.preview.v1", input, requestOptions),
      publishCheckpoint: (input, requestOptions) => call("babble.consensus.checkpoint.publish.v1", input, requestOptions),
    },
    judgment: {
      listDefinitions: (input = {}, requestOptions) => call("babble.judgment.definitions.list.v1", input, requestOptions),
      listProviders: (input = {}, requestOptions) => call("babble.judgment.providers.list.v1", input, requestOptions),
      evaluateObject: (input, requestOptions) => call("babble.judgment.object.evaluate.v1", input, requestOptions),
    },
    ai: {
      judge: (input, requestOptions) => call("babble.ai.judge.v1", input, requestOptions),
      generate: (input, requestOptions) => call("babble.ai.generate.v1", input, requestOptions),
      embed: (input, requestOptions) => call("babble.ai.embed.v1", input, requestOptions),
      transcribe: (input, requestOptions) => call("babble.ai.transcribe.v1", input, requestOptions),
    },
    search: {
      objects: (input, requestOptions) => call("babble.search.objects.v1", input, requestOptions),
    },
    lenses: {
      list: (input = {}, requestOptions) => call("babble.lenses.list.v1", input, requestOptions),
    },
    discovery: {
      candidates: (input, requestOptions) => call("babble.discovery.candidates.v1", input, requestOptions),
    },
    personalization: {
      sync: {
        put: (input, requestOptions) => call("babble.personalization.sync.put.v1", input, requestOptions),
        list: (input, requestOptions) => call("babble.personalization.sync.list.v1", input, requestOptions),
        get: (input, requestOptions) => call("babble.personalization.sync.get.v1", input, requestOptions),
        delete: (input, requestOptions) => call("babble.personalization.sync.delete.v1", input, requestOptions),
      },
    },
    capabilities: {
      list: (input = {}, requestOptions) => call("babble.capabilities.list.v1", input, requestOptions),
      inspect: (input, requestOptions) => call("babble.capabilities.inspect.v1", input, requestOptions),
      grant: (input, requestOptions) => call("babble.capabilities.grant.v1", input, requestOptions),
      revoke: (input, requestOptions) => call("babble.capabilities.revoke.v1", input, requestOptions),
    },
    storage: {
      object: {
        get: (input, requestOptions) => call("babble.storage.object.get.v1", input, requestOptions),
        set: (input, requestOptions) => call("babble.storage.object.set.v1", input, requestOptions),
        delete: (input, requestOptions) => call("babble.storage.object.delete.v1", input, requestOptions),
        list: (input, requestOptions) => call("babble.storage.object.list.v1", input, requestOptions),
      },
      local: {
        get: (input, requestOptions) => call("babble.storage.local.get.v1", input, requestOptions),
        set: (input, requestOptions) => call("babble.storage.local.set.v1", input, requestOptions),
        delete: (input, requestOptions) => call("babble.storage.local.delete.v1", input, requestOptions),
        list: (input, requestOptions) => call("babble.storage.local.list.v1", input, requestOptions),
      },
    },
    network: {
      fetch: (input, requestOptions) => call("babble.network.fetch.v1", input, requestOptions),
    },
    payments: {
      checkout: (input, requestOptions) => call("babble.payments.checkout.v1", input, requestOptions),
    },
    notifications: {
      request: (input, requestOptions) => call("babble.notifications.request.v1", input, requestOptions),
    },
    clipboard: {
      write: (input, requestOptions) => call("babble.clipboard.write", input, requestOptions),
    },
    fullscreen: {
      enter: (input, requestOptions) => call("babble.fullscreen.enter", input, requestOptions),
    },
    observability: {
      snapshot: (input = {}, requestOptions) => call("babble.observability.snapshot.v1", input, requestOptions),
    },
    runtime: {
      prepareSurface: (input, requestOptions) => call("babble.runtime.surface.prepare.v1", input, requestOptions),
      surfaceHealth: (input = {}, requestOptions) => call("babble.runtime.surface.health.v1", input, requestOptions),
      startSurfaceSession: (input, requestOptions) => call("babble.runtime.surface.session.start.v1", input, requestOptions),
      getSurfaceSession: (input, requestOptions) => call("babble.runtime.surface.session.get.v1", input, requestOptions),
      heartbeatSurfaceSession: (input = {}, requestOptions) => call("babble.runtime.surface.session.heartbeat.v1", input, requestOptions),
      transitionSurfaceSession: (input, requestOptions) => call("babble.runtime.surface.session.transition.v1", input, requestOptions),
      changeSurfaceBudget: (input, requestOptions) => call("babble.runtime.surface.session.budget.v1", input, requestOptions),
      scheduleSurfaceSession: (input, requestOptions) => call("babble.runtime.surface.session.schedule.v1", input, requestOptions),
      applySurfaceSchedule: (input, requestOptions) => call("babble.runtime.surface.session.apply_schedule.v1", input, requestOptions),
      checkpointSurfaceState: (input, requestOptions) => call("babble.runtime.surface.session.state.checkpoint.v1", input, requestOptions),
      getSurfaceState: (input, requestOptions) => call("babble.runtime.surface.session.state.get.v1", input, requestOptions),
      lifecycle: createSurfaceLifecycle,
    },
    realtime: {
      defineRoom: (input, requestOptions) => call("babble.realtime.room.define.v1", input, requestOptions),
      startSession: (input, requestOptions) => call("babble.realtime.session.start.v1", input, requestOptions),
      leaveSession: (input, requestOptions) => call("babble.realtime.session.leave.v1", input, requestOptions),
      publishMessage: (input, requestOptions) => call("babble.realtime.message.publish.v1", input, requestOptions),
    },
    close: () => {
      client.close();
    },
  };
}

export function createSurfaceSDK(options: SurfaceSDKOptions): BabbleSDK {
  if (options.plan.admission !== "ready") {
    throw new Error(`cannot bind Babble SDK for non-ready Surface admission: ${options.plan.admission}`);
  }
  const transportOptions: BrowserBridgeTransportOptions = {
    ...(options.targetOrigin !== undefined ? { targetOrigin: options.targetOrigin } : {}),
    ...(options.allowedOrigins !== undefined ? { allowedOrigins: options.allowedOrigins } : {}),
  };

  return createBabbleSDK({
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
