import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const code = ts.transpileModule(readFileSync(new URL("../src/app/image-viewer.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;

function harness() {
  const elements = [], observers = [];
  class Element {
    children = []; listeners = {}; attributes = {}; dataset = {}; style = {}; captures = new Set();
    hidden = false; disabled = false; isConnected = true; className = ""; open = false;
    clientWidth = 600; clientHeight = 400; naturalWidth = 1200; naturalHeight = 800;
    scrollLeft = 0; scrollTop = 0; classList = { add: name => { this.className += ` ${name}`; } };
    constructor(tag) { this.tag = tag; elements.push(this); }
    get width() { return parseFloat(this.style.width) || 0; }
    get height() { return parseFloat(this.style.height) || 0; }
    append(...children) { this.children.push(...children); }
    setAttribute(key, value) { this.attributes[key] = value; }
    removeAttribute(key) { delete this.attributes[key]; }
    addEventListener(type, callback) { (this.listeners[type] ??= []).push(callback); }
    emit(type, values = {}) {
      const event = { preventDefault() {}, stopPropagation() {}, ...values };
      for (const callback of this.listeners[type] ?? []) callback(event);
    }
    showModal() { this.open = true; }
    close() { this.open = false; this.emit("close"); }
    remove() { this.isConnected = false; }
    focus() { document.activeElement = this; }
    closest() { return this.inertParent ?? null; }
    setPointerCapture(id) { this.captures.add(id); }
    hasPointerCapture(id) { return this.captures.has(id); }
    releasePointerCapture(id) { this.captures.delete(id); }
  }
  const document = { body: new Element("body"), createElement: tag => new Element(tag), activeElement: null };
  class ResizeObserver {
    constructor(callback) { this.callback = callback; observers.push(this); }
    observe() {}
    disconnect() { this.disconnected = true; }
  }
  const context = { exports: {}, document, ResizeObserver, require: name => name === "lucide"
    ? { createElement: () => new Element("svg") } : {} };
  vm.runInNewContext(code, context);
  const button = name => elements.findLast(element => element.attributes["aria-label"] === name);
  const trigger = new Element("button");
  return { ...context.exports, elements, observers, button, document, trigger,
    open() { return context.exports.openImageViewer({ src: "https://example.test/image.png", alt: "A landscape", caption: "<b>Literal caption</b>" }, trigger); } };
}

test("image sizing preserves aspect ratio, caps fit at native size, and bounds zoom", () => {
  const { imageSize } = harness();
  assert.equal(JSON.stringify(imageSize(1200, 800, 600, 500, 1)), JSON.stringify({ width: 600, height: 400 }));
  assert.equal(imageSize(100, 50, 600, 500, 1).width, 100);
  assert.equal(imageSize(1200, 800, 600, 500, 999).width, 2400);
  assert.equal(imageSize(1200, 800, 600, 500, 0.25).width, 600);
  for (const invalid of [0, -1, Infinity, NaN]) assert.equal(imageSize(invalid, 800, 600, 500, 1), null);
});

test("viewer loads full image, supports zoom/reset and restores focus on close", () => {
  const h = harness(), dialog = h.open();
  const image = h.elements.find(element => element.tag === "img");
  assert.equal(dialog.open, true);
  assert.equal(h.document.activeElement, h.button("Close image"));
  assert.equal(h.button("Zoom in").disabled, true);
  assert.equal(image.src, "https://example.test/image.png");
  assert.equal(image.referrerPolicy, "no-referrer");
  image.emit("load");
  assert.equal(image.width, 600);
  h.button("Zoom in").emit("click");
  assert.equal(image.width, 1200);
  h.button("Zoom in").emit("click");
  assert.equal(image.width, 2400);
  assert.equal(h.button("Zoom in").disabled, true);
  h.button("Fit image").emit("click");
  assert.equal(image.width, 600);
  assert.equal(h.button("Zoom out").disabled, true);
  assert.equal(h.elements.find(element => element.className === "image-viewer-caption").textContent, "<b>Literal caption</b>");
  h.button("Close image").emit("click");
  assert.equal(h.document.activeElement, h.trigger);
  assert.equal(dialog.isConnected, false);
  assert.equal(h.observers[0].disconnected, true);
});

test("error is visible and retry starts a real reload without zooming broken content", () => {
  const h = harness(); h.open();
  const image = h.elements.find(element => element.tag === "img");
  image.emit("error");
  assert.equal(h.button("Retry image").hidden, false);
  assert.equal(h.elements.find(element => element.attributes.role === "status").textContent, "Image could not be loaded");
  assert.equal(h.button("Zoom in").disabled, true);
  h.button("Retry image").emit("click");
  assert.equal(h.button("Retry image").hidden, true);
  assert.equal(h.elements.find(element => element.attributes.role === "status").textContent, "Loading image");
  image.emit("load");
  assert.equal(h.button("Zoom in").disabled, false);
});

test("mouse panning releases capture and keyboard handling cannot move the feed", () => {
  const h = harness(), dialog = h.open();
  h.elements.find(element => element.tag === "img").emit("load");
  let stopped = 0;
  dialog.emit("keydown", { key: "+", stopPropagation: () => stopped++ });
  const stage = h.elements.find(element => element.className === "image-viewer-stage");
  stage.emit("pointerdown", { pointerType: "mouse", button: 0, isPrimary: true, pointerId: 9, clientX: 50, clientY: 50 });
  const left = stage.scrollLeft;
  stage.emit("pointermove", { pointerId: 9, clientX: 20, clientY: 20 });
  assert.equal(stage.scrollLeft, left + 30);
  assert.equal(stage.hasPointerCapture(9), true);
  dialog.emit("keydown", { key: "Escape", stopPropagation: () => stopped++ });
  assert.equal(stage.hasPointerCapture(9), false);
  assert.equal(stopped, 2);
});

test("replacing a viewer closes the old instance; late loads and detached focus cannot revive it", () => {
  const h = harness(), first = h.open();
  const oldImage = h.elements.find(element => element.tag === "img");
  const next = h.open();
  assert.equal(first.isConnected, false);
  oldImage.emit("load");
  assert.equal(oldImage.hidden, true);
  h.trigger.isConnected = false;
  next.close();
  assert.notEqual(h.document.activeElement, h.trigger);
});
