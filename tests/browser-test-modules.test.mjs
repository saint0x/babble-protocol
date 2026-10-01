import assert from "node:assert/strict";
import { readFile, readdir } from "node:fs/promises";
import test from "node:test";
import { rewriteBrowserTestImports } from "./browser-test-modules.mjs";

test("production probes use explicit built entries, including the SDK bridge", () => {
  for (const name of ["accounts", "bundle-publication", "conversations", "image-viewer", "media-gallery",
    "media-player", "moderation", "protocol", "surfaces", "local-preferences"]) {
    const source = `import('/src/app/${name}.ts')`;
    const built = `import('/__test_modules/${name}.js')`;
    assert.equal(rewriteBrowserTestImports(source), built);
    assert.equal(rewriteBrowserTestImports(built), built);
  }
  const bridge = `/@fs${new URL("../sdk/src/bridge.ts", import.meta.url).pathname}`;
  assert.equal(rewriteBrowserTestImports(`import(${JSON.stringify(bridge)})`), 'import("/__test_modules/sdk-bridge.js")');
});

test("unknown source modules fail before production browser evaluation", () => {
  for (const path of ["/src/app/not-built.ts", "/src/app/new-probe.tsx", "/@fs/private/source.ts"]) {
    assert.throws(() => rewriteBrowserTestImports(`import('${path}')`), /unbuilt source module/);
  }
  const source = "fetch('/rpc'); import('./sdk/sdk.js'); document.querySelector('[data-app]')";
  assert.equal(rewriteBrowserTestImports(source), source);
});

test("nested evaluation strings and all current fixture source imports are covered", async () => {
  assert.equal(rewriteBrowserTestImports('win.eval("import(\'/src/app/moderation.ts\')")'),
    'win.eval("import(\'/__test_modules/moderation.js\')")');
  const files = (await readdir(new URL("./", import.meta.url))).filter(path =>
    path.endsWith("-browser.mjs") || path.endsWith("-fixture.mjs"));
  const inspected = new Set();
  for (const file of files) {
    const source = await readFile(new URL(file, import.meta.url), "utf8");
    for (const path of source.match(/\/src\/app\/[a-zA-Z0-9_./-]+\.tsx?\b/g) ?? []) {
      assert.match(rewriteBrowserTestImports(`import('${path}')`), /\/__test_modules\//, file);
      inspected.add(file);
    }
  }
  for (const file of ["browser-invocation-fixture.mjs", "document-registration-browser.mjs", "media-albums-browser.mjs"]) {
    assert.ok(inspected.has(file), `Fixture import audit must include ${file}`);
  }
});
