import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const context = { exports: {} };
vm.runInNewContext(ts.transpileModule(readFileSync(new URL("../src/app/reading.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText, context);
const { captureReading, restoreReading } = context.exports;

function column(top = 150) {
  const anchors = [];
  const element = { scrollTop: top, clientHeight: 500, querySelectorAll: () => anchors };
  const row = (key, offsetTop, offsetHeight = 100) => {
    const item = { dataset: { readingAnchor: key }, offsetTop, offsetHeight, offsetParent: element };
    anchors.push(item);
    return item;
  };
  return { element, anchors, row };
}

test("a reply anchor preserves the exact reading offset after earlier content grows", () => {
  const h = column();
  h.row("content", 0);
  const reply = h.row("reply:one", 120, 200);
  const position = captureReading(h.element);
  assert.equal(position.anchor, "reply:one");
  assert.equal(position.offset, -30);
  reply.offsetTop += 180;
  restoreReading(h.element, position);
  assert.equal(h.element.scrollTop, 330);
});

test("anchors survive replacement nodes and do not interpolate IDs into selectors", () => {
  const h = column();
  const key = 'reply:[id="untrusted"]';
  h.row(key, 120, 200);
  const saved = captureReading(h.element);
  h.anchors.length = 0;
  h.row(key, 300, 200);
  restoreReading(h.element, saved);
  assert.equal(h.element.scrollTop, 330);
});

test("missing anchors fall back to saved position; top padding stays at zero", () => {
  const h = column(0);
  h.row("content", 12, 400);
  const saved = captureReading(h.element);
  h.element.scrollTop = 200;
  restoreReading(h.element, saved);
  assert.equal(h.element.scrollTop, 0);
  h.anchors.length = 0;
  h.element.scrollTop = 450;
  const fallback = captureReading(h.element);
  h.element.scrollTop = 0;
  restoreReading(h.element, fallback);
  assert.equal(h.element.scrollTop, 450);
});

test("nested positioned ancestors are summed without transformed viewport coordinates", () => {
  const h = column(410);
  const reply = h.row("reply:nested", 30, 200);
  const list = { offsetTop: 400, offsetParent: h.element };
  reply.offsetParent = list;
  const saved = captureReading(h.element);
  assert.equal(saved.offset, 20);
  list.offsetTop = 600;
  restoreReading(h.element, saved);
  assert.equal(h.element.scrollTop, 610);
});

test("detached and offscreen anchors do not become reading positions", () => {
  const h = column();
  h.row("detached", 200).offsetParent = null;
  h.row("far-below", 1000);
  assert.equal(captureReading(h.element).anchor, null);
});
