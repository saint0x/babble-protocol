import assert from "node:assert/strict";
import { module, deferred, plain, settle } from "./safety-helpers.mjs";
export { module, deferred, plain, settle };
export const api = module("moderation");
let uuid = 0;
export const control = module("moderation-controller", { "./moderation": api }, { crypto: { randomUUID: () => `uuid-${++uuid}` } });
export const id = (prefix, char) => `${prefix}_${char.repeat(64)}`;
export const owner = id("id", "a"), author = id("id", "b"), reviewer = id("id", "c"), second = id("id", "d");
export const object = id("obj", "e"), reportId = id("report", "f"), signalId = id("jud", "1");
export const text = "This is a complete explanation for this moderation case.";
export const date = "2026-09-30T12:00:00Z";
export const access = (actor_id = owner, can_review = false) => ({ actor_id, can_review, policy_version: "babble.integrity.v1", reasons: [...api.moderationReasons] });
export const decision = (outcome = "restrict", reviewer_id = reviewer) => ({ reviewer_id, outcome, reason: "spam", explanation: text, policy_version: "babble.integrity.v1", source_signals: [], created_at: date });
export const item = (patch = {}) => ({ id: reportId, sequence: 10, object_id: object, subject_author_id: author, reporter_id: owner, reason: "spam", details: text,
  created_at: date, updated_at: date, revision: 1, status: "pending", decisions: [], appeal: null, ...patch });
export const decided = (outcome = "restrict") => item({ status: "decided", revision: 2, decisions: [decision(outcome)] });
export const appealed = () => item({ status: "appealed", revision: 3, decisions: [decision()], appeal: { appellant_id: author, details: text, created_at: date } });
export const closed = () => ({ ...appealed(), status: "closed", revision: 4, decisions: [decision(), decision("no_action", second)] });
export function harness(actor = owner, review = false) {
  const calls = { access: [], list: [], detail: [], report: [], decide: [], appeal: [] }, changes = [];
  let current = item(), failure = null;
  const source = {
    async access(owner, signal) { calls.access.push({ owner, signal }); return access(owner, review); },
    async list(owner, scope, before, signal) { calls.list.push({ owner, scope, before, signal }); return { items: [current], next_before: null }; },
    async detail(owner, id, signal) { calls.detail.push({ owner, id, signal }); return current; },
    async report(owner, intent, signal) { calls.report.push({ owner, intent, signal }); if (failure) throw failure;
      current = item({ object_id: intent.object_id, reporter_id: owner, reason: intent.reason, details: intent.details }); return current; },
    async decide(owner, id, intent, signal) { calls.decide.push({ owner, id, intent, signal }); if (failure) throw failure;
      current = { ...current, revision: current.revision + 1, status: current.appeal ? "closed" : "decided", decisions: [...current.decisions,
        { reviewer_id: owner, outcome: intent.outcome, reason: intent.reason, explanation: intent.explanation, policy_version: intent.policy_version, source_signals: [...intent.source_signals], created_at: date }] }; return current; },
    async appeal(owner, id, intent, signal) { calls.appeal.push({ owner, id, intent, signal }); if (failure) throw failure;
      current = { ...current, revision: current.revision + 1, status: "appealed", appeal: { appellant_id: owner, details: intent.details, created_at: date } }; return current; },
  };
  let key = 0;
  const controller = new control.ModerationController(source, record => changes.push(record), () => {}, () => `key-${++key}`);
  controller.account(actor);
  return { source, calls, changes, controller, get current() { return current; }, set current(v) { current = v; }, set failure(v) { failure = v; } };
}

export function domHarness(actor = owner, review = false) {
  const h = harness(actor, review), walk = node => [node, ...node.children.flatMap(walk)];
  const matches = (node, selector) => {
    const attr = /^(\w+)?\[([^=\]]+)(?:="([^"]*)")?\]$/.exec(selector);
    return attr ? (!attr[1] || node.tagName.toLowerCase() === attr[1]) && attr[2] in node.attributes && (attr[3] === undefined || node.attributes[attr[2]] === attr[3]) : node.tagName.toLowerCase() === selector;
  };
  class Element {
    children = []; attributes = {}; listeners = {}; parentElement = null; ownText = ""; className = ""; disabled = false; open = false; value = "";
    classList = { add: name => { this.className += ` ${name}`; } };
    constructor(tag) { this.tagName = tag.toUpperCase(); }
    get textContent() { return this.ownText + this.children.map(child => child.textContent).join(""); }
    set textContent(value) { this.ownText = value; this.replaceChildren(); }
    set innerHTML(_) { throw new Error("Unsafe HTML assignment"); }
    get isConnected() { return this === document.body || Boolean(this.parentElement?.isConnected); }
    append(...nodes) { for (const node of nodes) { node.parentElement = this; this.children.push(node); } }
    replaceChildren(...nodes) { for (const child of this.children) child.parentElement = null; this.children = []; this.append(...nodes); }
    remove() { if (this.parentElement) this.parentElement.children = this.parentElement.children.filter(child => child !== this); this.parentElement = null; }
    setAttribute(name, value) { this.attributes[name] = String(value); }
    getAttribute(name) { return this.attributes[name] ?? null; }
    contains(node) { return walk(this).includes(node); }
    querySelectorAll(selector) { return walk(this).slice(1).filter(node => matches(node, selector)); }
    querySelector(selector) { return this.querySelectorAll(selector)[0] ?? null; }
    addEventListener(name, fn) { (this.listeners[name] ??= []).push(fn); }
    focus() { document.activeElement = this; }
    showModal() { this.open = true; }
    close() { this.open = false; this.emit("close"); }
    emit(name, props = {}) {
      const event = { defaultPrevented: false, preventDefault() { this.defaultPrevented = true; }, ...props };
      if (this.disabled && ["click", "input", "change"].includes(name)) return event;
      for (const fn of this.listeners[name] ?? []) fn(event);
      if (["input", "change"].includes(name)) this.parentElement?.emit(name, event);
      return event;
    }
  }
  const document = { createElement: tag => new Element(tag), activeElement: null, body: null }; document.body = new Element("body");
  const { ModerationControls } = module("moderation-view", { "./moderation": api, "./moderation-controller": control, lucide: { createElement: () => new Element("svg") } }, { document, HTMLElement: Element });
  const changes = [], opens = []; let signIns = 0;
  const controls = new ModerationControls({ source: h.source, changed: record => changes.push(record), signIn: () => { signIns++; }, openObject: (id, opener) => opens.push({ id, opener, closed: !dialog.open }) });
  const dialog = document.body.querySelector("dialog"), opener = document.createElement("button"); document.body.append(opener); opener.focus();
  const all = action => dialog.querySelectorAll(`[data-moderation-action="${action}"]`);
  const one = action => { const results = all(action); assert.equal(results.length, 1, `Expected one ${action}, got ${results.length}`); return results[0]; };
  const form = kind => dialog.querySelector(`[data-moderation-form="${kind}"]`);
  const field = (name, kind = "report") => form(kind)?.querySelector(`[data-moderation-field="${name}"]`);
  const enter = (name, value, kind = "report") => { const node = field(name, kind); assert.ok(node, name); node.value = value; node.emit(node.tagName === "SELECT" ? "change" : "input"); };
  return { ...h, get current() { return h.current; }, set current(v) { h.current = v; }, set failure(v) { h.failure = v; }, controls, document, dialog, opener, changes, opens, all, one, form, field, enter,
    click: action => one(action).emit("click"), submit: kind => form(kind).emit("submit"), get signIns() { return signIns; } };
}
