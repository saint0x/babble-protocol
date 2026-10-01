import { createRequire } from "node:module";
import { mkdir, mkdtemp, symlink } from "node:fs/promises";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

const frontend = fileURLToPath(new URL("../frontend/", import.meta.url));
const require = createRequire(new URL("../frontend/package.json", import.meta.url));
const port = Number(process.argv[2]);
const mode = process.argv[4] ?? "dev";
if (!Number.isInteger(port) || port < 1 || port > 65535 || !process.argv[3]) {
  throw new Error("Expected an available port and a private cache directory");
}
if (!["dev", "production"].includes(mode)) throw new Error(`Unsupported Astro server mode: ${mode}`);
const cache = resolve(process.argv[3]);
process.env.NODE_ENV = mode === "production" ? "production" : "development";
const { build, dev, preview } = await import(require.resolve("astro"));

// Own only this server: never read, replace, or stop the user's Astro CLI lock.
const config = {
  root: frontend,
  cacheDir: resolve(cache, "astro"),
  outDir: resolve(cache, "dist"),
  server: { host: "127.0.0.1", port },
  vite: {
    cacheDir: resolve(cache, "vite"),
    server: { strictPort: true },
    preview: { strictPort: true },
  },
};
if (mode === "production") {
  // Astro places prerender modules under cwd when outDir is outside it. Keep
  // that workspace private while allowing Node to resolve the existing deps.
  await mkdir(cache, { recursive: true });
  const buildRoot = await mkdtemp(resolve(cache, "build-"));
  await symlink(resolve(frontend, "node_modules"), resolve(buildRoot, "node_modules"), "dir");
  process.chdir(buildRoot);
  await build(config);
  const { buildBrowserTestModules } = await import("./browser-test-modules.mjs");
  await buildBrowserTestModules(config.outDir);
}
const server = mode === "production" ? await preview(config) : await dev(config);
let stopping = false;
const stop = async () => {
  if (stopping) return;
  stopping = true;
  try { await server.stop(); process.exit(0); }
  catch (error) { console.error(error); process.exit(1); }
};
process.once("SIGINT", stop);
process.once("SIGTERM", stop);
if (process.send) {
  process.once("disconnect", stop);
  if (!process.connected) await stop();
  else process.send({ ready: true, port: mode === "production" ? server.port : server.address.port, mode });
}
