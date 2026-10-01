import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";

function load(name, dependencies = {}) {
  const context = { exports: {}, URL, require: id => {
    if (!(id in dependencies)) throw new Error(`Unexpected media dependency ${id}`);
    return dependencies[id];
  } };
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, context);
  return context.exports;
}
export const mediaKinds = load("media-kind");
export const mediaResource = load("media-resource", { "./media-kind": mediaKinds });
