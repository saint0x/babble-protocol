import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

// Execute the production handlers without starting a browser or the network client.
const source = ts.createSourceFile("main.ts", readFileSync(new URL("../src/app/main.ts", import.meta.url), "utf8"), ts.ScriptTarget.Latest, true);
const functions = new Set(["installDeckInteractions", "isInteractiveTarget", "deckNavigationBlocked", "setAnimatedVisibility", "isHidden"]);
const selected = source.statements.filter((node) =>
  (ts.isFunctionDeclaration(node) && functions.has(node.name?.text))
  || (ts.isExpressionStatement(node) && ts.isCallExpression(node.expression)
    && node.expression.expression.getText(source) === "window.addEventListener"
    && node.expression.arguments[0]?.text === "keydown"));
assert.equal(selected.length, functions.size + 1);
const code = ts.transpileModule(selected.map((node) => node.getText(source)).join("\n"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None },
}).outputText;

class Events {
  listeners = new Map();
  addEventListener(type, callback, capture = false) {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push({ callback, capture });
    this.listeners.set(type, listeners);
  }
  emit(type, values = {}) {
    const event = Object.assign(new FakePointerEvent(), {
      target: this, pointerId: 1, isPrimary: true, button: 0,
      clientX: 100, clientY: 100, timeStamp: 100, detail: 1,
      defaultPrevented: false, stopped: false,
      preventDefault() { this.defaultPrevented = true; },
      stopImmediatePropagation() { this.stopped = true; },
    }, values);
    for (const { callback } of [...(this.listeners.get(type) ?? [])].sort((a, b) => Number(b.capture) - Number(a.capture))) {
      callback(event);
      if (event.stopped) break;
    }
    return event;
  }
}

class FakePointerEvent {}
class Element extends Events {
  constructor(tag = "div", parent = null, attributes = {}) {
    super();
    Object.assign(this, { tag, parent, attributes, hidden: true, dataset: {}, isContentEditable: false, captures: new Set() });
  }
  closest(selectors) {
    const matches = selectors.split(", ").some((selector) => {
      if (selector.startsWith(".")) return this.attributes.class === selector.slice(1);
      if (!selector.startsWith("[")) return this.tag === selector;
      if (selector.startsWith("[contenteditable]")) return this.attributes.contenteditable !== undefined && this.attributes.contenteditable !== "false";
      if (selector.startsWith("[tabindex]")) return this.attributes.tabindex !== undefined && this.attributes.tabindex !== "-1";
      if (selector === "[data-surface-panel]") return this.attributes["data-surface-panel"] !== undefined;
      return selector === `[role='${this.attributes.role}']`;
    });
    return matches ? this : this.parent?.closest(selectors) ?? null;
  }
  getAttribute(name) { return name === "hidden" ? (this.hidden ? "" : null) : this.attributes[name] ?? null; }
  hasPointerCapture(id) { return this.captures.has(id); }
  setPointerCapture(id) { this.captures.add(id); }
  releasePointerCapture(id) { this.captures.delete(id); }
}

function harness() {
  const window = new Events();
  const document = new Events();
  const deck = new Element();
  const frames = new Map();
  const timers = new Map();
  const moves = [];
  const renders = [];
  let id = 0;
  const context = {
    window, document, deck, Element, HTMLElement: Element, PointerEvent: FakePointerEvent,
    cards: [{ id: "one" }, { id: "two" }], currentIndex: 1,
    PANEL_ANIMATION_MS: 220, panelAnimations: new WeakMap(),
    composerPanel: new Element(), profileDropdown: new Element(), settingsPanel: new Element(),
    helpPanel: new Element(), surfacePanel: new Element(), judgmentPanel: new Element(),
    move: (direction) => moves.push(direction), renderDeck: (...args) => renders.push(args),
    toggleComposer() {}, toggleProfile() {}, toggleSettings() {}, toggleHelp() {}, closeSurface() {}, closeJudgments() {},
    requestAnimationFrame: (callback) => { frames.set(++id, callback); return id; },
    cancelAnimationFrame: (frame) => frames.delete(frame),
  };
  document.querySelector = () => document.openDialog ?? null;
  window.matchMedia = () => ({ matches: window.reducedMotion ?? false });
  window.setTimeout = (callback) => { timers.set(++id, callback); return id; };
  window.clearTimeout = (timer) => timers.delete(timer);
  vm.createContext(context);
  vm.runInContext(code + "\ninstallDeckInteractions();", context);
  const flush = (queue) => { const callbacks = [...queue.values()]; queue.clear(); callbacks.forEach((callback) => callback()); };
  return { ...context, context, moves, renders, frames, timers, flushFrames: () => flush(frames), flushTimers: () => flush(timers) };
}

test("taps and nested interactive targets keep clicks and never capture", () => {
  for (const target of [new Element(), ...["button", "a", "input", "textarea", "select", "label", "summary", "video"].map((tag) => new Element("svg", new Element(tag))), new Element("span", new Element("div", null, { contenteditable: "true" }))]) {
    const h = harness();
    h.deck.emit("pointerdown", { target });
    assert.equal(h.deck.captures.size, 0);
    h.window.emit("pointerup");
    assert.equal(h.deck.emit("click", { target }).defaultPrevented, false);
    assert.deepEqual(h.moves, []);
  }
});

test("dragging a control does not navigate", () => {
  const h = harness();
  h.deck.emit("pointerdown", { target: new Element("input") });
  h.window.emit("pointermove", { clientX: 10 });
  h.window.emit("pointerup", { clientX: 10 });
  assert.deepEqual(h.moves, []);
  assert.equal(h.deck.captures.size, 0);
});

test("image taps open normally while image swipes navigate without opening the viewer", () => {
  const h = harness();
  const button = new Element("button", null, { class: "post-image-open" });
  const image = new Element("img", button);
  h.deck.emit("pointerdown", { target: image });
  h.window.emit("pointerup", { target: image });
  assert.equal(h.deck.emit("click", { target: image }).defaultPrevented, false);
  assert.deepEqual(h.moves, []);
  h.deck.emit("pointerdown", { target: image });
  h.window.emit("pointermove", { clientX: 10 });
  h.window.emit("pointerup", { clientX: 10 });
  assert.deepEqual(h.moves, [1]);
  assert.equal(h.deck.emit("click", { target: image }).defaultPrevented, true);
  assert.equal(h.window.emit("keydown", { key: "ArrowRight", target: button }).defaultPrevented, false);
  assert.deepEqual(h.moves, [1]);
});

test("focusable reading regions retain vertical scrolling and allow horizontal swipes", () => {
  const h = harness();
  const target = new Element("div", null, { tabindex: "0" });
  h.document.activeElement = target;
  for (const key of ["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight"]) {
    assert.equal(h.window.emit("keydown", { key, target }).defaultPrevented, false);
  }
  assert.deepEqual(h.moves, []);
  h.deck.emit("pointerdown", { target });
  assert.equal(h.window.emit("pointermove", { clientY: 150 }).defaultPrevented, false);
  h.window.emit("pointerup", { clientY: 180 });
  assert.deepEqual(h.moves, []);
  h.deck.emit("pointerdown", { target });
  h.window.emit("pointermove", { clientX: 10 });
  h.window.emit("pointerup", { clientX: 10 });
  assert.deepEqual(h.moves, [1]);
});

test("horizontal swipes capture only after intent, navigate once, and suppress the resulting click", () => {
  for (const [clientX, expected] of [[10, 1], [190, -1]]) {
    const h = harness();
    h.deck.emit("pointerdown");
    assert.equal(h.deck.captures.size, 0);
    assert.equal(h.window.emit("pointermove", { clientX }).defaultPrevented, true);
    assert.equal(h.deck.hasPointerCapture(1), true);
    h.deck.emit("lostpointercapture", { target: new Element("p") });
    h.window.emit("pointerup", { clientX });
    h.window.emit("pointerup", { clientX });
    assert.deepEqual(h.moves, [expected]);
    assert.equal(h.deck.captures.size, 0);
    const click = h.deck.emit("click");
    assert.equal(click.defaultPrevented, true);
    assert.equal(click.stopped, true);
    h.deck.emit("pointerdown");
    h.window.emit("pointerup");
    assert.equal(h.deck.emit("click").defaultPrevented, false);
  }
});

test("vertical intent stays vertical even after horizontal drift; ambiguous diagonals do not navigate", () => {
  for (const [x, y] of [[90, 140], [130, 128]]) {
    const h = harness();
    h.deck.emit("pointerdown");
    assert.equal(h.window.emit("pointermove", { clientX: x, clientY: y }).defaultPrevented, false);
    h.window.emit("pointerup", { clientX: x === 90 ? 0 : 150, clientY: x === 90 ? 150 : 147 });
    assert.deepEqual(h.moves, []);
    assert.equal(h.deck.captures.size, 0);
  }
});

test("short horizontal drags suppress clicks but do not navigate; keyboard clicks remain available", () => {
  const h = harness();
  h.deck.emit("pointerdown");
  h.window.emit("pointermove", { clientX: 120 });
  h.window.emit("pointerup", { clientX: 120 });
  assert.deepEqual(h.moves, []);
  assert.equal(h.deck.emit("click", { detail: 0 }).defaultPrevented, false);
  assert.equal(h.deck.emit("click").defaultPrevented, true);
});

test("cancellation, lost capture, blur, hidden document, resize, and second touch abort a swipe", () => {
  for (const reason of ["pointercancel", "lostpointercapture", "blur", "visibilitychange", "resize", "second-touch"]) {
    const h = harness();
    h.deck.emit("pointerdown");
    h.window.emit("pointermove", { clientX: 30 });
    if (reason === "second-touch") h.deck.emit("pointerdown", { pointerId: 2, isPrimary: false });
    else if (reason === "visibilitychange") { h.document.hidden = true; h.document.emit(reason); }
    else if (reason === "lostpointercapture") h.deck.emit(reason);
    else h.window.emit(reason);
    h.window.emit("pointerup", { clientX: 0 });
    assert.deepEqual(h.moves, [], reason);
    assert.equal(h.deck.captures.size, 0, reason);
  }
});

test("unrelated pointer events, right clicks, nonprimary pointers and empty feeds do not navigate", () => {
  const h = harness();
  h.deck.emit("pointerdown");
  h.window.emit("pointercancel", { pointerId: 2 });
  h.window.emit("pointerup", { pointerId: 2, clientX: 0 });
  h.window.emit("pointerup", { clientX: 0 });
  assert.deepEqual(h.moves, [1]);
  for (const options of [{ button: 2 }, { isPrimary: false }, { defaultPrevented: true }]) {
    const next = harness();
    next.deck.emit("pointerdown", options);
    next.window.emit("pointerup", { clientX: 0 });
    assert.deepEqual(next.moves, []);
  }
  h.context.cards = [];
  h.deck.emit("pointerdown");
  h.window.emit("pointerup", { clientX: 0 });
  assert.deepEqual(h.moves, [1]);
});

test("arrow shortcuts respect focused controls, composition, modifiers, dialogs and popovers", () => {
  const h = harness();
  assert.equal(h.window.emit("keydown", { key: "ArrowRight" }).defaultPrevented, true);
  assert.deepEqual(h.moves, [1]);
  for (const options of [{ target: new Element("input") }, { defaultPrevented: true }, { isComposing: true }, { ctrlKey: true }, { metaKey: true }, { altKey: true }, { shiftKey: true }]) {
    h.window.emit("keydown", { key: "ArrowRight", ...options });
  }
  h.document.activeElement = new Element("textarea");
  h.window.emit("keydown", { key: "ArrowRight" });
  h.document.activeElement = null;
  for (const name of ["composerPanel", "profileDropdown", "settingsPanel", "helpPanel", "judgmentPanel"]) {
    h[name].hidden = false;
    h.window.emit("keydown", { key: "ArrowRight" });
    h.deck.emit("pointerdown");
    h.window.emit("pointerup", { clientX: 0 });
    h[name].hidden = true;
  }
  h.document.openDialog = new Element("dialog");
  h.window.emit("keydown", { key: "ArrowRight" });
  assert.deepEqual(h.moves, [1]);
  h.document.openDialog = null;
  h.window.emit("keydown", { key: "ArrowLeft" });
  assert.deepEqual(h.moves, [1, -1]);
});

test("inline Surface owns its input, while the surrounding deck remains navigable", () => {
  const h = harness();
  const surface = new Element("section", null, { "data-surface-panel": "" });
  const canvas = new Element("canvas", surface);
  h.surfacePanel.hidden = false;
  h.window.emit("keydown", { key: "ArrowRight", target: canvas });
  h.deck.emit("pointerdown", { target: canvas });
  h.window.emit("pointerup", { clientX: 0, target: canvas });
  assert.deepEqual(h.moves, []);
  h.window.emit("keydown", { key: "ArrowRight" });
  assert.deepEqual(h.moves, [1]);
  h.deck.emit("pointerdown");
  h.window.emit("pointermove", { clientX: 20 });
  h.deck.emit("surfacechange");
  h.window.emit("pointerup", { clientX: 0 });
  assert.deepEqual(h.moves, [1]);
  assert.equal(h.deck.captures.size, 0);
});

test("a panel opening during a swipe prevents navigation", () => {
  const h = harness();
  h.deck.emit("pointerdown");
  h.window.emit("pointermove", { clientX: 20 });
  h.helpPanel.hidden = false;
  h.window.emit("pointerup", { clientX: 0 });
  assert.deepEqual(h.moves, []);
  assert.equal(h.deck.captures.size, 0);
});

test("resize leaves CSS reflow, card identity and the current index intact", () => {
  const h = harness();
  h.window.emit("resize");
  h.window.emit("resize");
  assert.equal(h.frames.size, 0);
  h.flushFrames();
  assert.deepEqual(h.renders, []);
  assert.equal(h.context.currentIndex, 1);
});

test("rapid panel toggles cancel stale frames and timers", () => {
  const h = harness();
  const panel = h.helpPanel;
  h.context.setAnimatedVisibility(panel, true);
  h.context.setAnimatedVisibility(panel, false);
  h.flushFrames();
  h.flushTimers();
  assert.equal(panel.hidden, true);
  assert.equal(panel.dataset.state, undefined);
  h.context.setAnimatedVisibility(panel, true);
  h.flushFrames();
  h.context.setAnimatedVisibility(panel, false);
  h.context.setAnimatedVisibility(panel, true);
  h.flushTimers();
  h.flushFrames();
  assert.equal(panel.hidden, false);
  assert.equal(panel.dataset.state, "open");
});

test("a toggle reverses a closing panel before its exit timer hides it", () => {
  const h = harness(); const panel = h.helpPanel;
  h.context.setAnimatedVisibility(panel, true);
  assert.equal(h.context.isHidden(panel), false, "opening intent is already visible before its first frame");
  h.flushFrames();
  h.context.setAnimatedVisibility(panel, false);
  assert.equal(panel.hidden, false, "exit keeps layout until the animation finishes");
  assert.equal(h.context.isHidden(panel), true, "toggle sees closing intent, not the delayed hidden attribute");
  h.context.setAnimatedVisibility(panel, h.context.isHidden(panel));
  h.flushTimers(); h.flushFrames();
  assert.equal(panel.hidden, false); assert.equal(panel.dataset.state, "open");
});

test("reduced motion applies panel visibility immediately and cancels pending animation", () => {
  const h = harness();
  h.context.setAnimatedVisibility(h.helpPanel, true);
  h.window.reducedMotion = true;
  h.context.setAnimatedVisibility(h.helpPanel, false);
  assert.equal(h.helpPanel.hidden, true);
  assert.equal(h.frames.size + h.timers.size, 0);
  h.context.setAnimatedVisibility(h.helpPanel, true);
  assert.equal(h.helpPanel.hidden, false);
  assert.equal(h.helpPanel.dataset.state, "open");
  assert.equal(h.frames.size + h.timers.size, 0);
});
