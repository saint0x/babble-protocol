import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const source = ts.createSourceFile("main.ts", readFileSync(new URL("../src/app/main.ts", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true);
const selected = source.statements.filter((node) => ts.isFunctionDeclaration(node)
  && ["judgmentConfidence", "percent"].includes(node.name?.text));
assert.equal(selected.length, 2);
const code = ts.transpileModule(selected.map((node) => node.getText(source)).join("\n"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None },
}).outputText;
const context = {};
vm.runInNewContext(code, context);

test("Judgment confidence distinguishes calibration status from a numeric score", () => {
  const judgment = (provider, output, confidence = 0.72) => ({ provider: { provider }, output, confidence });
  assert.equal(context.judgmentConfidence(judgment("babble-python", { confidence_status: "uncalibrated" }, 0)), "Uncalibrated");
  assert.equal(context.judgmentConfidence(judgment("babble-python", { confidence_status: "legacy_heuristic" })), "Heuristic");
  assert.equal(context.judgmentConfidence(judgment("babble-local", {})), "Heuristic");
  assert.equal(context.judgmentConfidence(judgment("babble-constant", {})), "Uncalibrated");
  assert.equal(context.judgmentConfidence(judgment("measured-provider", {})), "72%");
  assert.equal(context.judgmentConfidence(judgment("measured-provider", null)), "72%");
});
