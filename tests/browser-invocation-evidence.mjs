import assert from "node:assert/strict";
import { runLiveStackEvidence } from "./live-stack-evidence.mjs";

const mode = process.argv[2];
assert.ok(["focus", "full"].includes(mode), "expected focus or full");
process.exitCode = (await runLiveStackEvidence({
  focus: mode === "focus" ? "browser-invocations" : null, mode,
  artifactDirectory: "browser-invocations", freezeVariable: "BABEL_BROWSER_INVOCATION_SOURCE_FROZEN",
})).code;
