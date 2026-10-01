import assert from "node:assert/strict";
import { createServer } from "node:net";

export const liveStackEndpoints = Object.freeze({
  apiPort: 18787, gatewayPort: 18788, frontendPort: 14329, aegisAddr: "127.0.0.1:17878",
});

/** Check ownership before creating stores, servers or browser state. */
export async function assertLiveStackIsolation(config, freezeVariable) {
  assert.equal(process.env[freezeVariable], "1", `Parent source freeze required: ${freezeVariable}`);
  assert.deepEqual([config.apiPort, config.gatewayPort, config.frontendPort],
    [18787, 18788, 14329], "Disposable test ports required");
  assert.equal(config.aegisAddr, liveStackEndpoints.aegisAddr, "Acceptance Aegis address required");
  const ports = [config.apiPort, config.gatewayPort, config.frontendPort];
  if (config.observedApiPort !== undefined) {
    assert.equal(config.observedApiPort, 18789, "Disposable observed API port required");
    ports.push(config.observedApiPort);
  }
  for (const port of ports) {
    await new Promise((resolve, reject) => {
      const server = createServer();
      server.once("error", reject);
      server.listen(port, "127.0.0.1", () => server.close(error => error ? reject(error) : resolve()));
    });
  }
}
