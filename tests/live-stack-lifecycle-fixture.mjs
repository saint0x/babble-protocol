import { spawn } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:http";
import { LiveStackLifecycle } from "./live-stack-lifecycle.mjs";

const [role, storeRoot, mode] = process.argv.slice(2);
if (role === "wrapper") {
  const owner = spawn(process.execPath, [import.meta.filename, "owner", storeRoot, "normal"], {
    stdio: ["ignore", "pipe", "pipe", "ipc"],
  });
  owner.stdout.on("data", chunk => process.stdout.write(chunk));
  owner.stderr.on("data", chunk => process.stderr.write(chunk));
  const [ready] = await once(owner, "message");
  process.send({ ...ready, pids: [owner.pid, ...ready.pids] });
  await once(process, "message");
} else if (role === "owner") {
  const lifecycle = new LiveStackLifecycle(storeRoot, { graceMs: 150, killMs: 1500 });
  lifecycle.installSignalHandlers();
  try {
    const service = lifecycle.spawn(process.execPath, [import.meta.filename, "service", storeRoot,
      mode === "before-ready" ? "delayed" : "tree"], { stdio: ["ignore", "ignore", "pipe", "ipc"] });
    service.stderr.on("data", chunk => process.stderr.write(chunk));
    service.on("exit", () => process.stderr.write("Owned service exited\n"));
    if (mode === "before-ready") process.send({ stage: "spawned", pids: [service.pid] });
    if (mode === "startup-failure") {
      const failed = lifecycle.spawn(process.execPath, ["-e", "process.exit(7)"], { stdio: "ignore" });
      process.send({ stage: "spawned", pids: [service.pid, failed.pid] });
      await once(failed, "exit");
      throw new Error("fixture failed before readiness");
    }
    const [ready] = await once(service, "message");
    if (mode !== "before-ready") process.send({ stage: "ready", pids: [service.pid, ready.descendant.pid],
      ports: [ready.port, ready.descendant.port] });
    await once(process, "message");
  } finally { await lifecycle.cleanup(); }
} else {
  const server = createServer((_request, response) => response.end("live fixture"));
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  let descendant;
  if (mode === "tree") {
    const child = spawn(process.execPath, [import.meta.filename, "service", storeRoot, "stubborn"], {
      stdio: ["ignore", "ignore", "inherit", "ipc"],
    });
    const [ready] = await once(child, "message");
    descendant = { pid: child.pid, port: ready.port };
  }
  const stop = () => { if (mode !== "stubborn") process.exit(0); };
  process.on("SIGINT", stop);
  process.on("SIGTERM", stop);
  const ready = () => process.send?.({ port: server.address().port, descendant });
  if (mode === "delayed") setTimeout(ready, 10_000);
  else ready();
}
