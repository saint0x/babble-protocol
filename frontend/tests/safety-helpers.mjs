import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import vm from "node:vm";
import ts from "typescript";

export function module(name, imports = {}, globals = {}) {
  const context = { exports: {}, require(name) { assert.ok(name in imports, `Unexpected import ${name}`); return imports[name]; },
    AbortController, AbortSignal, URL, Response, Request, Headers, TextDecoder, TextEncoder, EventTarget, Event,
    Error, console, crypto: { randomUUID: () => "test-key" }, ...globals };
  vm.runInNewContext(ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
  }).outputText, context);
  return context.exports;
}
export const safety = module("safety", { "./profile-response": module("profile-response") });
export const id = (char) => `id_${char.repeat(64)}`;
export const owner = id("a"), target = id("b"), other = id("c");
export const state = (blocked = false, muted = false, revision = 0, author_id = owner, target_id = target) => ({ author_id, target_id, blocked, muted, revision });
export const identity = (id = target) => ({ id, handle: "Author", kind: "Person", created_at: "2026-09-30T12:00:00Z",
  public_key: { algorithm: "Ed25519", bytes: "a".repeat(64) }, signature: { algorithm: "Ed25519", bytes: "a".repeat(128) } });
export const snapshot = (states = [], revision = Math.max(0, ...states.map(s => s.revision)), author_id = owner) => ({ author_id, revision,
  entries: states.filter(s => s.blocked || s.muted).map(s => ({ identity: identity(s.target_id), state: s })).sort((a,b) => a.identity.id.localeCompare(b.identity.id)) });
export const plain = (v) => JSON.parse(JSON.stringify(v));
export const deferred = () => { let resolve, reject; const promise = new Promise((a, b) => { resolve = a; reject = b; }); return { promise, resolve, reject }; };
export const settle = () => new Promise(resolve => setImmediate(resolve));

export function controllerHarness() {
  const reads = [], writes = [], snapshots = [], changes = [], renders = [];
  const request = (list, args) => { const r = { ...deferred(), ...args }; list.push(r); return r.promise; };
  const source = {
    state(owner, target, signal) { return request(reads, { owner, target, signal }); },
    update(owner, target, intent, signal) { return request(writes, { owner, target, intent, signal }); },
    snapshot(owner, signal) { return request(snapshots, { owner, signal }); },
  };
  let key = 0;
  const controller = new safety.SafetyController(source, value => changes.push(value), () => renders.push(true), () => `key-${++key}`);
  controller.account(owner);
  return { controller, reads, writes, snapshots, changes, renders, source };
}
export async function ready(h, current = state()) {
  const ensure = h.controller.ensure(); h.snapshots.at(-1).resolve(snapshot([current], current.revision)); await ensure;
  const show = h.controller.show(target); h.reads.at(-1).resolve(current); await show;
}
export async function finish(h, operation, fresh, { receipt = fresh, error, snapshotError } = {}) {
  if (error) h.writes.at(-1).reject(error); else h.writes.at(-1).resolve(receipt);
  await settle(); h.reads.at(-1).resolve(fresh); await settle();
  if (snapshotError) h.snapshots.at(-1).reject(snapshotError); else h.snapshots.at(-1).resolve(snapshot([fresh], fresh.revision));
  await operation;
}

export function domHarness(source) {
  const walk = node => [node, ...node.children.flatMap(walk)];
  const matches = (node, selector) => {
    const attr = /^\[([^=\]]+)(?:="([^"]*)")?\]$/.exec(selector);
    return attr ? attr[1] in node.attributes && (attr[2] === undefined || node.attributes[attr[1]] === attr[2]) : node.tagName.toLowerCase() === selector;
  };
  class Element {
    children = []; attributes = {}; listeners = {}; parentElement = null; ownText = ""; className = ""; disabled = false; open = false;
    classList = { add: name => { this.className += ` ${name}`; } };
    constructor(tag) { this.tagName = tag.toUpperCase(); }
    get textContent() { return this.ownText + this.children.map(child => child.textContent).join(""); }
    set textContent(value) { this.ownText = value; this.replaceChildren(); }
    set innerHTML(_) { throw new Error("Unsafe HTML assignment"); }
    get isConnected() { return this === document.body || Boolean(this.parentElement?.isConnected); }
    get lastElementChild() { return this.children.at(-1) ?? null; }
    append(...nodes) { for (const node of nodes) { node.parentElement = this; this.children.push(node); } }
    replaceChildren(...nodes) { for (const child of this.children) child.parentElement = null; this.children = []; this.append(...nodes); }
    remove() { if (this.parentElement) this.parentElement.children = this.parentElement.children.filter(child => child !== this); this.parentElement = null; }
    setAttribute(name, value) { this.attributes[name] = String(value); }
    getAttribute(name) { return this.attributes[name] ?? null; }
    contains(node) { return walk(this).includes(node); }
    querySelectorAll(selector) { return walk(this).slice(1).filter(node => matches(node, selector)); }
    querySelector(selector) { return this.querySelectorAll(selector)[0] ?? null; }
    addEventListener(name, callback) { (this.listeners[name] ??= []).push(callback); }
    focus() { document.activeElement = this; }
    showModal() { this.open = true; }
    close() { this.open = false; this.emit("close"); }
    emit(name, props = {}) {
      const event = { defaultPrevented: false, preventDefault() { this.defaultPrevented = true; }, ...props };
      if (this.disabled && name === "click") return event;
      for (const fn of this.listeners[name] ?? []) fn(event);
      return event;
    }
  }
  const document = { createElement: tag => new Element(tag), activeElement: null, body: null };
  document.body = new Element("body");
  const lucide = { createElement: () => new Element("svg") };
  const { SafetyControls } = module("safety-view", { "./safety": safety, lucide }, { document, HTMLElement: Element });
  const changes = []; let signIns = 0;
  const controls = new SafetyControls({ source, changed: s => changes.push(s), signIn: () => { signIns++; } });
  const dialog = document.body.querySelector("dialog");
  const opener = document.createElement("button"); document.body.append(opener); opener.focus();
  const all = action => dialog.querySelectorAll(`[data-safety-action="${action}"]`);
  const one = action => { const result = all(action); assert.equal(result.length, 1, `Expected one ${action}, found ${result.length}`); return result[0]; };
  return { controls, document, dialog, opener, changes, all, one, click: action => one(action).emit("click"), get signIns() { return signIns; } };
}
