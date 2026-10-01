import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";

export function galleryHarness() {
  const mutations = [], intersections = [], viewers = [], frames = [];
  let document;
  class Element {
    children = []; parentElement = null; dataset = {}; attributes = new Map(); listeners = new Map();
    className = ""; text = ""; hidden = false; scrollLeft = 0; offsetLeft = 0; offsetWidth = 52; clientWidth = 240;
    constructor(tag) { this.tag = tag; }
    get isConnected() { return this === document || (this.parentElement?.isConnected ?? false); }
    get firstElementChild() { return this.children[0] ?? null; }
    get textContent() { return this.text + this.children.map(child => child.textContent).join(""); }
    set textContent(value) { this.replaceChildren(); this.text = value; }
    set src(value) { this.setAttribute("src", value); }
    get src() { return this.getAttribute("src") ?? ""; }
    setAttribute(name, value) { this.attributes.set(name, String(value)); }
    getAttribute(name) { return this.attributes.get(name) ?? null; }
    hasAttribute(name) { return this.attributes.has(name); }
    removeAttribute(name) { this.attributes.delete(name); }
    append(...children) { for (const child of children) { child.remove(); child.parentElement = this; this.children.push(child); } }
    remove() {
      if (this.parentElement) this.parentElement.children.splice(this.parentElement.children.indexOf(this), 1);
      this.parentElement = null;
    }
    replaceChildren(...children) { for (const child of [...this.children]) child.remove(); this.text = ""; this.append(...children); }
    matches(selector) {
      if (selector === "audio, video") return this.tag === "audio" || this.tag === "video";
      if (selector === "[data-babble-playback]") return this.dataset.babblePlayback !== undefined;
      if (selector === '[hidden], [inert], details:not([open])') {
        return this.hidden || this.hasAttribute("inert") || this.tag === "details" && !this.hasAttribute("open");
      }
      if (selector === "img") return this.tag === "img";
      throw new Error(`Unsupported selector: ${selector}`);
    }
    closest(selector) {
      for (let node = this; node; node = node.parentElement) if (node.matches(selector)) return node;
      return null;
    }
    querySelectorAll(selector) { return this.children.flatMap(child => [...(child.matches(selector) ? [child] : []), ...child.querySelectorAll(selector)]); }
    addEventListener(type, callback, options) {
      const listeners = this.listeners.get(type) ?? [];
      listeners.push({ callback, capture: options === true });
      this.listeners.set(type, listeners);
    }
    emit(type, properties = {}) {
      const path = [];
      for (let node = this; node; node = node.parentElement) path.push(node);
      const event = { type, target: this, defaultPrevented: false, stopped: false,
        preventDefault() { this.defaultPrevented = true; }, stopPropagation() { this.stopped = true; }, ...properties };
      for (const node of [...path].reverse()) for (const listener of node.listeners.get(type) ?? []) {
        if (listener.capture) listener.callback(event);
      }
      for (const node of path) {
        for (const listener of node.listeners.get(type) ?? []) if (!listener.capture) listener.callback(event);
        if (event.stopped) break;
      }
      return event;
    }
    click() { this.focus(); return this.emit("click"); }
    focus(options) { document.activeElement = this; this.focusOptions = options; }
  }
  class HTMLMediaElement extends Element {
    pauseCalls = 0; loadCalls = 0; paused = true;
    pause() { this.paused = true; this.pauseCalls++; }
    load() { this.loadCalls++; }
  }
  class HTMLVideoElement extends HTMLMediaElement {}
  document = new Element("document");
  document.documentElement = new Element("html");
  document.body = new Element("body");
  document.append(document.documentElement);
  document.documentElement.append(document.body);
  document.createElement = tag => tag === "video" ? new HTMLVideoElement(tag)
    : tag === "audio" ? new HTMLMediaElement(tag) : new Element(tag);
  class MutationObserver {
    constructor(callback) { this.callback = callback; mutations.push(this); }
    observe() {}
  }
  class IntersectionObserver {
    observed = new Set();
    constructor(callback) { this.callback = callback; intersections.push(this); }
    observe(player) { this.observed.add(player); }
    unobserve(player) { this.observed.delete(player); }
  }
  const dependencies = { lucide: { createElement: () => new Element("svg") },
    "./image-viewer": { openImageViewer: (media, trigger) => viewers.push({ media, trigger }) } };
  function load(name) {
    const context = { exports: {}, document, window: new Element("window"), Element, HTMLMediaElement, HTMLVideoElement,
      requestAnimationFrame: callback => frames.push(callback),
      MutationObserver, IntersectionObserver, require(id) { assert.ok(id in dependencies, id); return dependencies[id]; } };
    vm.runInNewContext(ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
      compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
    }).outputText, context);
    return context.exports;
  }
  dependencies["./media-player"] = load("media-player");
  const gallery = load("media-gallery");
  return { ...gallery, document, viewers, intersections, Element,
    flushFrames() { for (const frame of frames.splice(0)) frame(); },
    flush(removedNodes = [], addedNodes = []) { for (const observer of mutations) observer.callback([{ removedNodes, addedNodes }]); } };
}

export const albumItems = [
  { media: "https://media.test/one.png", mediaKind: "image", mediaType: "image/png", integrity: "a" },
  { media: "https://media.test/two.mp3", mediaKind: "audio", mediaType: "audio/mpeg", integrity: "b" },
  { media: "https://media.test/three.mp4", mediaKind: "video", mediaType: "video/mp4", integrity: "c" },
];
export const album = { ...albumItems[0], mediaItems: albumItems, title: "A mixed album", content: "Caption" };
