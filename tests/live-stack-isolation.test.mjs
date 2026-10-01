import assert from "node:assert/strict";
import { once } from "node:events";
import { createServer } from "node:net";
import test from "node:test";
import { assertLiveStackIsolation, liveStackEndpoints } from "./live-stack-isolation.mjs";

test("source freeze and exact disposable endpoints precede any fixture writes", async () => {
  const variable = "BABEL_ISOLATION_TEST_FROZEN";
  const original = process.env[variable];
  try {
    delete process.env[variable];
    await assert.rejects(assertLiveStackIsolation(liveStackEndpoints, variable), /source freeze required/);
    process.env[variable] = "1";
    for (const [key, value] of Object.entries({ apiPort: 8787, gatewayPort: 8788, frontendPort: 4321,
      aegisAddr: "127.0.0.1:7878", observedApiPort: 8789 })) {
      await assert.rejects(assertLiveStackIsolation({ ...liveStackEndpoints, [key]: value }, variable),
        /Disposable.*port|Acceptance Aegis/);
    }
  } finally {
    if (original === undefined) delete process.env[variable]; else process.env[variable] = original;
  }
});

test("the internal observed API port is reserved without stopping an existing listener", async () => {
  const variable = "BABEL_ISOLATION_TEST_FROZEN";
  const original = process.env[variable];
  const server = createServer(socket => socket.end("existing service"));
  server.listen(18789, "127.0.0.1");
  await once(server, "listening");
  try {
    process.env[variable] = "1";
    await assert.rejects(assertLiveStackIsolation({ ...liveStackEndpoints, observedApiPort: 18789 }, variable),
      error => error.code === "EADDRINUSE");
    assert.equal(server.listening, true);
  } finally {
    await new Promise(resolve => server.close(resolve));
    if (original === undefined) delete process.env[variable]; else process.env[variable] = original;
  }
});
