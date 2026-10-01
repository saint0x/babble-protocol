import {
  rpcCatalog,
  type JsonValue,
  type RpcInput,
  type RpcMethodName,
  type RpcOutput,
  type RpcRequestEnvelope,
} from "./generated/protocol.js";
import { BabelError, type BabelTransport, type RpcBinding, type RpcRequestOptions, requestId } from "./transport.js";

export interface BabelClientOptions {
  readonly transport: BabelTransport;
  readonly binding: RpcBinding;
}

export class BabelClient {
  readonly transport: BabelTransport;
  readonly binding: RpcBinding;

  constructor(options: BabelClientOptions) {
    this.transport = options.transport;
    this.binding = options.binding;
  }

  async request<M extends RpcMethodName>(
    method: M,
    input: RpcInput<M>,
    options: RpcRequestOptions = {},
  ): Promise<RpcOutput<M>> {
    const definition = rpcCatalog.methods.find((entry) => entry.method === method);
    if (!definition) {
      throw new Error(`unsupported Babel RPC method: ${method}`);
    }
    const payload = input as JsonValue;
    const envelope: RpcRequestEnvelope = {
      protocol: rpcCatalog.protocol,
      id: options.id ?? requestId(method, payload),
      method,
      binding: this.binding,
      payload,
      idempotency_key: options.idempotencyKey ?? null,
      deadline: {
        timeout_ms: options.timeoutMs ?? definition.timeout_ms,
        client_started_at: new Date().toISOString(),
      },
      trace_id: options.traceId ?? null,
    };
    const response = await this.transport.request(envelope, options);
    if (response.protocol !== rpcCatalog.protocol) {
      throw new Error(`unsupported Babel RPC response protocol: ${response.protocol}`);
    }
    if (response.id !== envelope.id) {
      throw new Error(`Babel RPC response id mismatch: expected ${envelope.id} got ${response.id}`);
    }
    if (response.error) {
      throw new BabelError(response.error);
    }
    if (response.result === null) {
      throw new Error(`Babel RPC response for ${method} did not include a result`);
    }
    return response.result as RpcOutput<M>;
  }

  close(): void {
    this.transport.close();
  }
}

export function createBabelClient(options: BabelClientOptions): BabelClient {
  return new BabelClient(options);
}
