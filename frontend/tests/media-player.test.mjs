import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const code = ts.transpileModule(readFileSync(new URL("../src/app/media-player.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;

function harness() {
  const mutations = [], intersections = [];
  class EventTarget {
    listeners = new Map();
    addEventListener(type, callback, options = false) {
      const listeners = this.listeners.get(type) ?? [];
      listeners.push({ callback, capture: options === true || options?.capture === true });
      this.listeners.set(type, listeners);
    }
    emit(type) {
      // Snapshot the event path, including for removal during a capture listener.
      const ancestors = [];
      for (let node = this.parentNode; node; node = node.parentNode) ancestors.push(node);
      const event = { type, target: this, bubbles: false };
      for (const node of ancestors.reverse()) {
        for (const listener of node.listeners.get(type) ?? []) if (listener.capture) listener.callback(event);
      }
      for (const listener of this.listeners.get(type) ?? []) listener.callback(event);
    }
  }
  class Element extends EventTarget {
    children = []; parentNode = null; attributes = new Map(); dataset = {}; className = ""; textContent = "";
    constructor(tag) { super(); this.tag = tag; }
    get isConnected() {
      for (let node = this; node; node = node.parentNode) if (node === document) return true;
      return false;
    }
    append(...children) {
      for (const child of children) {
        child.remove();
        child.parentNode = this;
        this.children.push(child);
      }
    }
    remove() {
      if (this.parentNode) this.parentNode.children.splice(this.parentNode.children.indexOf(this), 1);
      this.parentNode = null;
    }
    setAttribute(name, value) { this.attributes.set(name, String(value)); }
    getAttribute(name) { return this.attributes.get(name) ?? null; }
    hasAttribute(name) { return this.attributes.has(name); }
    removeAttribute(name) { this.attributes.delete(name); }
    get hidden() { return this.hasAttribute("hidden"); }
    set hidden(value) { value ? this.setAttribute("hidden", "") : this.removeAttribute("hidden"); }
    matches(selector) {
      if (selector === "audio, video") return this.tag === "audio" || this.tag === "video";
      if (selector === "[data-babble-playback]") return Object.hasOwn(this.dataset, "babblePlayback");
      if (selector === '[hidden], [inert], details:not([open])') {
        return this.hidden || this.hasAttribute("inert") || this.tag === "details" && !this.hasAttribute("open");
      }
      throw new Error(`Unsupported selector in test DOM: ${selector}`);
    }
    closest(selector) {
      for (let node = this; node instanceof Element; node = node.parentNode) if (node.matches(selector)) return node;
      return null;
    }
    querySelectorAll(selector) {
      return this.children.flatMap(child => [...(child.matches(selector) ? [child] : []), ...child.querySelectorAll(selector)]);
    }
  }
  class HTMLMediaElement extends Element {
    controls = false; autoplay = false; preload = ""; paused = true; pauseCalls = 0; loadCalls = 0; error = null;
    get src() { return this.getAttribute("src") ?? ""; }
    set src(value) { this.setAttribute("src", value); }
    pause() { this.paused = true; this.pauseCalls++; }
    load() { this.loadCalls++; }
    start() { this.paused = false; this.emit("play"); }
  }
  class HTMLVideoElement extends HTMLMediaElement { playsInline = false; }
  const document = new EventTarget();
  document.hidden = false;
  document.documentElement = new Element("html");
  document.documentElement.parentNode = document;
  document.body = new Element("body");
  document.documentElement.append(document.body);
  document.querySelectorAll = selector => document.documentElement.querySelectorAll(selector);
  document.createElement = tag => tag === "video" ? new HTMLVideoElement(tag)
    : tag === "audio" ? new HTMLMediaElement(tag) : new Element(tag);
  const window = new EventTarget();
  class MutationObserver {
    constructor(callback) { this.callback = callback; mutations.push(this); }
    observe(target, options) { this.target = target; this.options = options; }
  }
  class IntersectionObserver {
    observed = new Set(); unobserved = [];
    constructor(callback) { this.callback = callback; intersections.push(this); }
    observe(target) { this.observed.add(target); }
    unobserve(target) { this.observed.delete(target); this.unobserved.push(target); }
  }
  const context = { exports: {}, document, window, Element, HTMLMediaElement, HTMLVideoElement,
    MutationObserver, IntersectionObserver, require(name) {
      assert.equal(name, "lucide");
      return { createElement: () => new Element("svg") };
    } };
  vm.runInNewContext(code, context);
  return { ...context.exports, document, window, mutations, intersections,
    create(kind = "audio", title = "A recording") {
      const src = `https://example.test/recording.${kind === "audio" ? "mp3" : "mp4"}`;
      const frame = context.exports.createMediaPlayer({ media: src, mediaKind: kind,
        mediaType: kind === "audio" ? "audio/mpeg" : "video/mp4", title });
      const player = frame.querySelectorAll("audio, video")[0];
      const feedback = frame.children.find(child => child.className === "media-player-feedback");
      const [status, retry] = feedback.children;
      return { src, frame, player, feedback, status, retry };
    },
    flush(...records) { for (const observer of mutations) observer.callback(records); },
  };
}

for (const kind of ["audio", "video"]) {
  test(`${kind} has native controls, metadata preload and no autoplay`, () => {
    const h = harness(), { frame, player, src, status, retry } = h.create(kind, '<img src=x onerror="bad()">');
    assert.equal(player.tag, kind);
    assert.equal(player.controls, true);
    assert.equal(player.preload, "metadata");
    assert.equal(player.autoplay, false);
    assert.equal(player.paused, true);
    assert.equal(player.src, src);
    assert.equal(player.getAttribute("aria-label"), '<img src=x onerror="bad()">');
    assert.equal(frame.dataset.kind, kind);
    assert.equal(frame.dataset.state, "loading");
    assert.equal(status.textContent, "Loading media...");
    assert.equal(status.getAttribute("role"), "status");
    assert.equal(status.getAttribute("aria-live"), "polite");
    assert.equal(retry.hidden, true);
    assert.equal(retry.type, "button");
    if (kind === "video") assert.equal(player.playsInline, true);
    else {
      const title = frame.children[0].children[1];
      assert.equal(title.textContent, '<img src=x onerror="bad()">');
      assert.equal(title.children.length, 0);
    }
  });
}

test("unsupported or missing playback resources fail before installing observers", () => {
  const h = harness();
  for (const resource of [{ media: null, mediaKind: "audio" }, { media: "", mediaKind: "video" },
    { media: "https://example.test/image.png", mediaKind: "image" }, { media: "x", mediaKind: null }]) {
    assert.throws(() => h.createMediaPlayer({ ...resource, title: "Invalid", mediaType: null }), /requires an audio or video/);
  }
  assert.equal(h.mutations.length, 0);
  assert.equal(h.intersections.length, 0);
});

test("readiness events clear loading and buffering feedback", () => {
  const h = harness(), { frame, player, status, feedback, retry } = h.create();
  for (const ready of ["loadedmetadata", "canplay", "playing"]) {
    player.emit("waiting");
    assert.equal(status.textContent, "Buffering...");
    assert.equal(feedback.hidden, false);
    assert.equal(frame.dataset.state, "loading");
    player.emit(ready);
    assert.equal(status.textContent, "");
    assert.equal(feedback.hidden, true);
    assert.equal(retry.hidden, true);
    assert.equal(frame.dataset.state, "ready");
  }
});

test("network and codec failures expose feedback; retry invokes native load without autoplay", () => {
  const h = harness(), { frame, player, status, feedback, retry, src } = h.create("video");
  for (const code of [undefined, 2, 3, 4]) {
    player.error = code === undefined ? null : { code };
    player.emit("error");
    assert.equal(frame.dataset.state, "error");
    assert.equal(feedback.hidden, false);
    assert.equal(retry.hidden, false);
    assert.equal(status.textContent, code === 3 || code === 4
      ? "This browser cannot play this file, or the file is damaged." : "Media could not be loaded. Try again.");
    const loads = player.loadCalls;
    retry.emit("click");
    assert.equal(player.loadCalls, loads + 1);
    assert.equal(player.src, src);
    assert.equal(player.autoplay, false);
    assert.equal(player.paused, true);
    assert.equal(status.textContent, "Loading media...");
    assert.equal(frame.dataset.state, "loading");
    assert.equal(retry.hidden, true);
    player.emit("canplay");
    assert.equal(frame.dataset.state, "ready");
  }
});

test("a captured non-bubbling play event pauses other players, including composer media", () => {
  const h = harness(), audio = h.create(), video = h.create("video");
  const composer = h.document.createElement("video");
  h.document.body.append(audio.frame, video.frame, composer);
  assert.equal(h.document.listeners.get("play").length, 1);
  assert.equal(h.document.listeners.get("play")[0].capture, true);
  audio.player.start();
  assert.equal(audio.player.paused, false);
  video.player.start();
  assert.equal(audio.player.paused, true);
  assert.equal(video.player.paused, false);
  composer.start();
  assert.equal(video.player.paused, true);
  assert.equal(composer.paused, false);
  assert.equal(h.mutations.length, 1);
  assert.equal(h.intersections.length, 1);
});

for (const reason of ["hidden", "inert", "closed-details", "document-hidden", "disconnected-during-capture"]) {
  test(`play is rejected when ${reason}`, () => {
    const h = harness();
    if (reason === "disconnected-during-capture") h.document.addEventListener("play", event => event.target.remove(), true);
    const { frame, player } = h.create();
    const parent = h.document.createElement(reason === "closed-details" ? "details" : "section");
    h.document.body.append(parent);
    parent.append(frame);
    if (reason === "hidden" || reason === "inert") parent.setAttribute(reason, "");
    if (reason === "document-hidden") h.document.hidden = true;
    player.start();
    assert.equal(player.paused, true);
    assert.equal(player.pauseCalls, 1);
    if (reason === "disconnected-during-capture") assert.equal(player.isConnected, false);
  });
}

test("open details allow playback; visibility and pagehide pause all native media", () => {
  const h = harness(), { frame, player } = h.create();
  const details = h.document.createElement("details");
  details.setAttribute("open", "");
  h.document.body.append(details);
  details.append(frame);
  const composer = h.document.createElement("audio");
  h.document.body.append(composer);
  player.start();
  h.document.emit("visibilitychange");
  assert.equal(player.paused, false);
  h.document.hidden = true;
  h.document.emit("visibilitychange");
  assert.equal(player.paused, true);
  assert.equal(composer.pauseCalls, 2);
  h.document.hidden = false;
  player.start();
  h.window.emit("pagehide");
  assert.equal(player.paused, true);
  assert.equal(composer.pauseCalls, 4);
});

test("attribute mutations pause unavailable media but preserve playable siblings", () => {
  const h = harness(), hidden = h.create(), visible = h.create("video");
  h.document.body.append(hidden.frame, visible.frame);
  const observer = h.mutations[0];
  assert.equal(observer.target, h.document.documentElement);
  assert.equal(observer.options.subtree, true);
  assert.equal(observer.options.childList, true);
  assert.equal(observer.options.attributes, true);
  assert.deepEqual(Array.from(observer.options.attributeFilter).sort(), ["hidden", "inert", "open"]);
  for (const attribute of ["hidden", "inert"]) {
    hidden.player.start();
    hidden.frame.setAttribute(attribute, "");
    const before = visible.player.pauseCalls;
    h.flush({ removedNodes: [], addedNodes: [] });
    assert.equal(hidden.player.paused, true);
    assert.equal(visible.player.pauseCalls, before);
    hidden.frame.removeAttribute(attribute);
  }
});

for (const removeDirectly of [false, true]) {
  test(`removing ${removeDirectly ? "a player" : "a containing subtree"} unloads and unobserves; reconnect restores its source`, () => {
    const h = harness(), { frame, player, src } = h.create("video");
    h.document.body.append(frame);
    player.emit("canplay");
    player.start();
    const removed = removeDirectly ? player : frame;
    removed.remove();
    h.flush({ removedNodes: [removed], addedNodes: [] });
    assert.equal(player.paused, true);
    assert.equal(player.hasAttribute("src"), false);
    assert.equal(player.loadCalls, 1);
    assert.equal(h.intersections[0].observed.has(player), false);
    assert.deepEqual(h.intersections[0].unobserved, [player]);
    player.error = { code: 4 };
    player.emit("error");
    assert.equal(frame.dataset.state, "ready", "unload-generated errors must not become playback failures");
    h.document.body.append(removed);
    h.flush({ removedNodes: [], addedNodes: [removed] });
    assert.equal(player.src, src);
    assert.equal(h.intersections[0].observed.has(player), true);
    assert.equal(player.loadCalls, 1);
    assert.equal(player.paused, true, "reattachment must not start playback");
  });
}

test("reparenting before mutation delivery does not pause, unload or reset the player", () => {
  const h = harness(), { frame, player, src } = h.create();
  const destination = h.document.createElement("section");
  h.document.body.append(frame, destination);
  player.start();
  destination.append(frame);
  h.flush({ removedNodes: [frame], addedNodes: [] }, { removedNodes: [], addedNodes: [frame] });
  assert.equal(player.isConnected, true);
  assert.equal(player.src, src);
  assert.equal(player.paused, false);
  assert.equal(player.pauseCalls, 0);
  assert.equal(player.loadCalls, 0);
  assert.equal(h.intersections[0].unobserved.length, 0);
});

test("offscreen intersection pauses only the affected media and never auto-resumes it", () => {
  const h = harness(), audio = h.create(), video = h.create("video");
  h.document.body.append(audio.frame, video.frame);
  const observer = h.intersections[0];
  assert.equal(observer.observed.has(audio.player), true);
  assert.equal(observer.observed.has(video.player), true);
  audio.player.start();
  const otherPauses = video.player.pauseCalls;
  observer.callback([{ target: audio.player, isIntersecting: true }]);
  assert.equal(audio.player.paused, false);
  observer.callback([{ target: audio.player, isIntersecting: false }, { target: video.player, isIntersecting: true }]);
  assert.equal(audio.player.paused, true);
  assert.equal(video.player.pauseCalls, otherPauses);
  observer.callback([{ target: audio.player, isIntersecting: true }]);
  assert.equal(audio.player.paused, true);
  assert.equal(audio.player.loadCalls, 0);
});

test("pauseMedia is scoped to the supplied subtree and includes unmarked native controls", () => {
  const h = harness(), inside = h.create(), outside = h.create("video");
  const preview = h.document.createElement("audio");
  inside.frame.append(preview);
  h.document.body.append(inside.frame, outside.frame);
  const outsidePauses = outside.player.pauseCalls;
  h.pauseMedia(inside.frame);
  assert.equal(inside.player.pauseCalls, 1);
  assert.equal(preview.pauseCalls, 1);
  assert.equal(outside.player.pauseCalls, outsidePauses);
});
