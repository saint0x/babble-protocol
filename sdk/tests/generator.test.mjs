import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { copyFile, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

async function fixture(t, schemas) {
  const root = await mkdtemp(join(tmpdir(), "babel-generator-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  for (const path of ["sdk/scripts", "sdk/src/generated", "fixtures/protocol/v1"]) {
    await mkdir(join(root, path), { recursive: true });
  }
  const script = join(root, "sdk/scripts/generate-protocol.mjs");
  await copyFile(new URL("../scripts/generate-protocol.mjs", import.meta.url), script);
  await writeFile(join(root, "fixtures/protocol/v1/schema-bundle.json"), JSON.stringify({
    version: 1, protocol: "babel.v2", schemas: {
      "rpc.RpcError": true, "rpc.RpcRequestEnvelope": true, "rpc.RpcResponseEnvelope": true, ...schemas,
    }, fixtures: { rpc_catalog: { protocol: "babel.rpc.v1", methods: [] } },
  }));
  const generate = spawnSync(process.execPath, [script], { encoding: "utf8" });
  assert.equal(generate.status, 0, generate.stderr);
  const check = spawnSync(process.execPath, [script, "--check"], { encoding: "utf8" });
  assert.equal(check.status, 0, check.stderr);
  return { root, output: await readFile(join(root, "sdk/src/generated/protocol.ts"), "utf8") };
}

test("generator retains primitive const literals including falsy values", async t => {
  const values = { Empty: "", Zero: 0, Negative: -1.5, Exponent: 1e21, True: true, False: false,
    Null: null, Escaped: 'a "quote"\nbackslash \\', String: "clipboard_write" };
  const schemas = Object.fromEntries(Object.entries(values).map(([name, value]) => [
    `literal.${name}`, { type: value === null ? ["string", "null"] : typeof value, const: value },
  ]));
  schemas["literal.Untyped"] = { const: "without type" };
  const { output } = await fixture(t, schemas);
  for (const [name, value] of Object.entries(values)) {
    assert.ok(output.includes(`export type Literal${name} = ${JSON.stringify(value)};`), `${name} literal was widened`);
  }
  assert.ok(output.includes('export type LiteralUntyped = "without type";'));
});

test("generated constants type-check as nested literals and discriminated unions", async t => {
  const { root } = await fixture(t, {
    "literal.Null": { const: null },
    "literal.Number": { const: 0 },
    "literal.Boolean": { const: false },
    "literal.Tuple": { const: ["one", 2, null, { enabled: false }] },
    "literal.EmptyArray": { const: [] },
    "literal.EmptyObject": { const: {} },
    "literal.Object": { const: { mode: "ready", count: 0, nested: { done: true } } },
    "literal.Result": { oneOf: [
      { type: "object", properties: { kind: { type: "string", const: "clipboard_write" }, written: { const: true } }, required: ["kind", "written"], additionalProperties: false },
      { type: "object", properties: { kind: { type: "string", const: "failed" }, code: { enum: ["context_lost", "native_error"] } }, required: ["kind", "code"], additionalProperties: false },
    ] },
  });
  const source = join(root, "types.ts");
  await writeFile(source, `import type { LiteralNull, LiteralNumber, LiteralBoolean, LiteralTuple,
    LiteralEmptyArray, LiteralEmptyObject, LiteralObject, LiteralResult } from './sdk/src/generated/protocol.js';
const nil: LiteralNull = null;
const zero: LiteralNumber = 0;
const disabled: LiteralBoolean = false;
const tuple: LiteralTuple = ['one', 2, null, { enabled: false }];
const empty: LiteralEmptyArray = [];
const emptyObject: LiteralEmptyObject = {};
const object: LiteralObject = { mode: 'ready', count: 0, nested: { done: true } };
// @ts-expect-error null must not widen to JsonValue
const wrongNull: LiteralNull = 'null';
// @ts-expect-error numeric const must not widen to number
const wrongNumber: LiteralNumber = 1;
// @ts-expect-error false must not widen to boolean
const wrongBoolean: LiteralBoolean = true;
// @ts-expect-error tuple length is fixed
const wrongTuple: LiteralTuple = ['one', 2];
// @ts-expect-error empty tuple is exact
const wrongArray: LiteralEmptyArray = [1];
// @ts-expect-error empty object must reject properties
const wrongObject: LiteralEmptyObject = { extra: true };
// @ts-expect-error object const values are literals
const wrongMode: LiteralObject = { mode: 'other', count: 0, nested: { done: true } };
// @ts-expect-error nested object const values are readonly
object.nested.done = false;
function consume(result: LiteralResult): true | 'context_lost' | 'native_error' {
  if (result.kind === 'clipboard_write') return result.written;
  return result.code;
}
// @ts-expect-error discriminator and payload cannot be mixed
consume({ kind: 'failed', written: true });
consume({ kind: 'clipboard_write', written: true });
`);
  const compiled = spawnSync("tsc", ["--noEmit", "--strict", "--skipLibCheck", "--target", "ES2022",
    "--module", "NodeNext", "--moduleResolution", "NodeNext", source], { encoding: "utf8" });
  assert.equal(compiled.status, 0, compiled.error?.message ?? compiled.stdout + compiled.stderr);
});
