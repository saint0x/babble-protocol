import assert from "node:assert/strict";
import { createServer, request } from "node:http";

/** Passive HTTP observation for disposable browser fixtures. No response substitution. */
export async function startHttpObserver({ port, upstreamPort, capture, maxBytes = 64 * 1024 * 1024 }) {
  assert.equal(typeof capture, "function");
  const records = [];
  let sequence = 0, retainedBytes = 0, failure = null, closePromise;
  const connections = new Set();
  const upstreamRequests = new Set();
  const server = createServer((incoming, outgoing) => {
    const pathname = new URL(incoming.url, "http://127.0.0.1").pathname;
    const record = capture(pathname, incoming.method) ? {
      sequence: ++sequence, url: `http://${incoming.headers.host}${incoming.url}`,
      method: incoming.method, requestBody: null, responseBody: null, status: null,
    } : null;
    if (record) records.push(record);
    const body = { request: [], response: [] };
    const observe = (kind, chunk) => {
      if (!record || failure) return;
      retainedBytes += chunk.length;
      if (retainedBytes > maxBytes) {
        failure = "HTTP observation byte budget exceeded";
        body.request.length = body.response.length = 0;
        return;
      }
      body[kind].push(Buffer.from(chunk));
    };
    incoming.on("data", chunk => observe("request", chunk));
    incoming.on("end", () => {
      if (record && !failure) record.requestBody = Buffer.concat(body.request).toString("utf8");
    });
    const upstream = request({ hostname: "127.0.0.1", port: upstreamPort,
      method: incoming.method, path: incoming.url, headers: incoming.headers }, response => {
      if (record) record.status = response.statusCode;
      outgoing.writeHead(response.statusCode, response.statusMessage, response.headers);
      response.on("data", chunk => observe("response", chunk));
      response.on("end", () => {
        if (record && !failure) record.responseBody = Buffer.concat(body.response).toString("utf8");
      });
      response.on("error", error => {
        if (record) record.error = error.message;
        outgoing.destroy(error);
      });
      response.pipe(outgoing);
    });
    upstreamRequests.add(upstream);
    upstream.once("close", () => upstreamRequests.delete(upstream));
    upstream.on("error", error => {
      if (record) record.error = error.message;
      // Transport failure stays a network error, never a fabricated API result.
      outgoing.destroy(error);
    });
    incoming.once("aborted", () => {
      if (record) record.error = "Client request aborted";
      upstream.destroy();
    });
    incoming.on("error", error => {
      if (record) record.error = error.message;
      upstream.destroy(error);
    });
    outgoing.once("close", () => { if (!outgoing.writableFinished) upstream.destroy(); });
    incoming.pipe(upstream);
  });
  server.on("connection", socket => {
    connections.add(socket);
    socket.once("close", () => connections.delete(socket));
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () => { server.off("error", reject); resolve(); });
  });
  return {
    port: server.address().port,
    readRequests({ since = 0 } = {}) {
      assert.ok(Number.isSafeInteger(since) && since >= 0 && since <= sequence, "Invalid observation cursor");
      if (failure) throw new Error(failure);
      return { cursor: sequence, requests: structuredClone(records.filter(record => record.sequence > since)) };
    },
    close() {
      return closePromise ??= (async () => {
        const stopped = new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve()));
        for (const upstream of upstreamRequests) upstream.destroy();
        for (const socket of connections) socket.destroy();
        await stopped;
      })();
    },
  };
}
