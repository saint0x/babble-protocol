import { createRequire } from "node:module";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const frontend = fileURLToPath(new URL("../frontend/", import.meta.url));
const require = createRequire(new URL("../frontend/package.json", import.meta.url));
const names = ["accounts", "bundle-publication", "conversations", "image-viewer", "media-gallery",
  "media-player", "moderation", "protocol", "surfaces", "local-preferences"];
const bridge = fileURLToPath(new URL("../sdk/src/bridge.ts", import.meta.url));
const personalization = fileURLToPath(new URL("../sdk/src/personalization.ts", import.meta.url));
const sources = new Map([
  ...names.map(name => [`/src/app/${name}.ts`, name]),
  [`/@fs${bridge}`, "sdk-bridge"],
]);

/** Auxiliary setup/component probes; these never replace the built application's modules. */
export async function buildBrowserTestModules(outDir) {
  const { build } = await import(require.resolve("vite"));
  await build({
    configFile: false,
    root: frontend,
    publicDir: false,
    logLevel: "warn",
    build: {
      outDir: join(outDir, "__test_modules"),
      emptyOutDir: true,
      minify: false,
      lib: {
        entry: Object.fromEntries([...names.map(name => [name, join(frontend, "src/app", `${name}.ts`)]),
          ["sdk-bridge", bridge], ["sdk-personalization", personalization]]),
        formats: ["es"],
        fileName: (_format, name) => `${name}.js`,
      },
      rolldownOptions: {
        output: { chunkFileNames: "chunks/[name]-[hash].js", assetFileNames: "assets/[name]-[hash][extname]" },
      },
    },
  });
}

/** Rewrite only the enumerated probe module URLs; unknown source imports fail closed. */
export function rewriteBrowserTestImports(code) {
  return code.replace(/\/src\/app\/[a-zA-Z0-9_./-]+\.tsx?\b|\/@fs[^'"`\s\\)]+/g, source => {
    const name = sources.get(source);
    if (!name) throw new Error(`Production browser probe has an unbuilt source module: ${source}`);
    return `/__test_modules/${name}.js`;
  });
}
