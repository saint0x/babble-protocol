import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const source = ts.createSourceFile("main.ts", readFileSync(new URL("../src/app/main.ts", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true);
const declaration = source.statements.find(node => ts.isFunctionDeclaration(node) && node.name?.text === "releaseSurfacePlacement");
assert.ok(declaration);
const code = ts.transpileModule(declaration.getText(source), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None },
}).outputText;

function element() {
  return {
    dataset: { surfaceOpen: "true" }, isConnected: true, focused: false,
    attributes: new Map(), children: [],
    removeAttribute(name) { this.attributes.delete(name); },
    setAttribute(name, value) { this.attributes.set(name, value); },
    replaceChildren(...children) { this.children = children; },
    querySelector() { return null; },
    focus() { this.focused = true; },
  };
}

function harness(offDeck) {
  const column = element(), restoredColumn = element(), opener = element();
  const reading = { anchor: "reply:current", offset: 30, top: 300 };
  const previousReading = { anchor: "reply:previous", offset: 12, top: 650 };
  const restored = [];
  const placement = { column, primary: element(), opener, reading,
    ...(offDeck ? { returnFeed: { cards: [{ id: "original" }], index: 0, source: "Following", reading: previousReading } } : {}) };
  const context = {
    surfacePlacement: placement, surfaceRestoreFocus: true, surfacePanel: element(),
    surfaceActions: null, surfaceInvocations: null,
    surfaceHome: { after() {} }, deck: element(), expandSurfaceButton: element(),
    surfaceHost: element(), surfaceMeta: element(), sourceLabel: element(),
    createElement() { return element(); }, Maximize2: {}, setBridgeStatus() {},
    restoreReading: (target, position) => restored.push({ target, position }),
    render() { opener.isConnected = false; },
    cards: [{ id: "temporary" }], currentIndex: 0, activeSurfaceLabel: "Surface",
  };
  context.deck.querySelector = () => restoredColumn;
  vm.createContext(context);
  vm.runInContext(code, context);
  return { context, placement, column, restoredColumn, opener, restored, reading, previousReading };
}

test("closing an inline Surface restores the original column and opener", () => {
  const h = harness(false);
  h.context.releaseSurfacePlacement();
  assert.deepEqual(h.restored, [{ target: h.column, position: h.reading }]);
  assert.equal(h.opener.focused, true);
  assert.equal(h.context.surfacePlacement, null);
  assert.equal(h.context.surfacePanel.hidden, true);
  assert.equal(h.placement.primary.dataset.surfaceOpen, undefined);
});

test("off-feed Surface exit restores the prior feed column, not the retiring Object", () => {
  const h = harness(true);
  h.context.releaseSurfacePlacement();
  assert.equal(h.context.cards[0].id, "original");
  assert.equal(h.context.sourceLabel.textContent, "Following");
  assert.deepEqual(h.restored, [{ target: h.restoredColumn, position: h.previousReading }]);
  assert.equal(h.restoredColumn.focused, true);
  assert.equal(h.column.focused, false);
});

test("background close restores reading without stealing focus", () => {
  const h = harness(false);
  h.context.surfaceRestoreFocus = false;
  h.context.releaseSurfacePlacement();
  assert.equal(h.restored.length, 1);
  assert.equal(h.opener.focused, false);
  assert.equal(h.column.focused, false);
});

test("releasing the Surface disposes pending host actions before detaching its panel", () => {
  const h = harness(false);
  let disposed = 0;
  h.context.surfaceActions = { dispose() {
    assert.equal(h.context.surfacePlacement, h.placement);
    disposed++;
  } };
  h.context.releaseSurfacePlacement();
  assert.equal(disposed, 1);
  assert.equal(h.context.surfaceActions, null);
  h.context.releaseSurfacePlacement();
  assert.equal(disposed, 1);
});

test("releasing the Surface cancels invocation consent before detaching and does not dispose twice", () => {
  const h = harness(false);
  let disposed = 0;
  h.context.surfaceInvocations = { dispose() {
    assert.equal(h.context.surfacePlacement, h.placement);
    disposed++;
  } };
  h.context.releaseSurfacePlacement();
  assert.equal(disposed, 1);
  assert.equal(h.context.surfaceInvocations, null);
  h.context.releaseSurfacePlacement();
  assert.equal(disposed, 1);
});
