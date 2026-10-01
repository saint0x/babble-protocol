import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { safety, owner, target, other, state, identity, snapshot, deferred, settle, domHarness } from "./safety-helpers.mjs";

function harness({ initial = state(), person = identity(), snapshotError = false } = {}) {
  let current = initial, revision = initial.revision, failSnapshot = snapshotError, failWrite = null;
  const calls = { writes: [], reads: [], snapshots: [], identities: [] };
  const source = {
    async state(owner, target, signal) { calls.reads.push({ owner, target, signal }); return { ...current, author_id: owner, target_id: target }; },
    async identity(target, signal) { calls.identities.push({ target, signal }); return { ...person, id: target }; },
    async snapshot(owner, signal) { calls.snapshots.push({ owner, signal }); if (failSnapshot) throw new Error("offline"); return snapshot([current], revision, owner); },
    async update(owner, target, intent, signal) {
      calls.writes.push({ owner, target, intent, signal });
      if (failWrite) throw failWrite;
      current = state(intent.blocked, intent.muted, ++revision, owner, target); return current;
    },
  };
  const dom = domHarness(source);
  return { ...dom, get signIns() { return dom.signIns; }, source, calls, set failSnapshot(value) { failSnapshot = value; }, set failWrite(value) { failWrite = value; } };
}
async function open(h) { h.controls.account(owner); await h.controls.ensure(); h.controls.openAuthor(target, h.opener); await settle(); }

test("guest opens sign-in only, sign-in closes, and later account activation never submits", async () => {
  const h = harness(); h.controls.openAuthor(target, h.opener);
  assert.equal(h.all("block").length, 0); assert.equal(h.dialog.open, true);
  h.click("sign-in"); assert.equal(h.signIns, 1); assert.equal(h.dialog.open, false);
  assert.equal(h.document.activeElement, h.opener); h.controls.account(owner); await settle();
  assert.equal(h.calls.writes.length + h.calls.reads.length + h.calls.snapshots.length, 0);
});

test("block has explicit scope confirmation, conservative focus and exactly one explicit submission", async () => {
  const h = harness(); await open(h);
  assert.equal(h.document.activeElement, h.one("close"));
  assert.match(h.dialog.textContent, /Author/); assert.equal(h.calls.identities.length, 1);
  h.click("block"); assert.equal(h.calls.writes.length, 0); assert.equal(h.document.activeElement, h.one("cancel-block"));
  for (const scope of [/local-node/, /in either direction/, /Existing follows and signed history are retained/, /Public posts stay public/, /other nodes/]) assert.match(h.dialog.textContent, scope);
  h.click("cancel-block"); assert.equal(h.document.activeElement, h.one("block")); assert.equal(h.calls.writes.length, 0);
  h.click("block"); h.click("confirm-block"); await settle();
  assert.equal(h.calls.writes.length, 1); assert.equal(h.calls.writes[0].intent.blocked, true);
  assert.equal(h.controls.blocked(target), true); assert.equal(h.all("unblock").length, 1);
});

test("mute-only has clear scope and unblocking preserves mute", async () => {
  const h = harness(); await open(h);
  assert.match(h.dialog.textContent, /Explicit profiles and interactions remain available/);
  h.click("mute"); await settle(); assert.equal(h.controls.hidden(target), true); assert.equal(h.controls.blocked(target), false);
  h.click("block"); h.click("confirm-block"); await settle();
  h.click("unblock"); await settle();
  assert.equal(h.controls.blocked(target), false); assert.equal(h.controls.hidden(target), true);
  assert.equal(h.calls.writes.at(-1).intent.muted, true);
  h.click("unmute"); await settle(); assert.equal(h.controls.hidden(target), false);
});

test("management view covers loading, error, retry, empty and direct independent removals", async () => {
  const h = harness({ initial: state(true, true, 2), snapshotError: true }); h.controls.account(owner);
  h.controls.openList(h.opener); assert.match(h.dialog.textContent, /Loading/); await settle();
  assert.match(h.dialog.textContent, /Could not load/); assert.equal(h.dialog.querySelector("[data-safety-list]") !== null, true);
  h.failSnapshot = false; h.click("retry"); await settle();
  assert.match(h.dialog.textContent, /Blocked and Muted/); assert.equal(h.all("unblock").length, 1); assert.equal(h.all("unmute").length, 1);
  h.click("unblock"); await settle(); assert.equal(h.calls.writes.length, 1); assert.equal(h.calls.writes[0].intent.muted, true);
  h.controls.openList(); await settle(); assert.equal(h.all("unblock").length, 0);
  h.click("unmute"); await settle(); h.controls.openList(); await settle();
  assert.match(h.dialog.textContent, /No blocked or muted authors/);
});

test("uncertain mutation offers exact retry alone and never displays success", async () => {
  const h = harness(); await open(h); h.failWrite = new safety.SafetyError(503);
  h.click("mute"); await settle(); assert.match(h.dialog.textContent, /Retry previous change/);
  assert.equal(h.all("block").length, 0); assert.equal(h.all("mute").length, 0);
  const intent = h.calls.writes[0].intent; h.failWrite = null; h.click("retry"); await settle();
  assert.equal(h.calls.writes[1].intent, intent); assert.equal(h.all("unmute").length, 1);
});

test("account loss clears dialog, confirmation, identity and private snapshot immediately", async () => {
  const h = harness({ initial: state(false, true, 1), person: { ...identity(), handle: "Private handle" } }); await open(h);
  h.click("block"); const detached = h.one("confirm-block"); h.controls.account(other);
  assert.equal(h.dialog.open, false); assert.equal(h.controls.snapshot, null); assert.equal(h.controls.hidden(target), false);
  assert.equal(h.dialog.textContent.includes("Private handle"), false); detached.emit("click"); await settle();
  assert.equal(h.calls.writes.length, 0); assert.equal(h.document.activeElement, h.opener);
});

test("late dialog reads and stale confirmation callbacks cannot act on another author or closed dialog", async () => {
  const h = harness(); await open(h); h.click("block"); const detached = h.one("confirm-block");
  h.controls.openAuthor(other); await settle(); detached.emit("click"); assert.equal(h.calls.writes.length, 0);
  const pending = deferred(); h.source.state = () => pending.promise;
  h.controls.openAuthor(target); h.controls.close(); pending.resolve(state(true, false, 1)); await settle();
  assert.equal(h.dialog.open, false); assert.equal(h.dialog.textContent, "");
});

test("management removal interrupted during fresh read never submits after close", async () => {
  const h = harness({ initial: state(true, false, 1) }); h.controls.account(owner); h.controls.openList(h.opener); await settle();
  const pending = deferred(); h.source.state = () => pending.promise;
  h.click("unblock"); h.controls.close(); pending.resolve(state(true, false, 1)); await settle();
  assert.equal(h.calls.writes.length, 0);
});

test("Escape restores focus, self has no actions, malicious identity handles render literally", async () => {
  const attack = '<img src=x onerror="alert(1)">';
  const h = harness({ person: { ...identity(), handle: attack } }); await open(h);
  assert.equal(h.dialog.textContent.includes(attack), true); assert.equal(h.dialog.querySelector("img"), null);
  assert.equal(h.dialog.emit("cancel").defaultPrevented, true); assert.equal(h.dialog.open, false); assert.equal(h.document.activeElement, h.opener);
  h.controls.openAuthor(owner, h.opener); assert.match(h.dialog.textContent, /own account/); assert.equal(h.all("block").length, 0);
  h.controls.dispose(); assert.equal(h.dialog.isConnected, false);
});

test("account switch suppresses late management errors and dispose prevents reopening", async () => {
  const h = harness(); const pending = deferred(); h.source.snapshot = () => pending.promise;
  h.controls.account(owner); h.controls.openList(); h.controls.account(other); pending.reject(new Error("old private error")); await settle();
  assert.equal(h.dialog.open, false); assert.equal(h.dialog.textContent.includes("error"), false);
  h.controls.dispose(); h.controls.openAuthor(target); h.controls.openList(); assert.equal(h.dialog.open, false);
});

test("CSS retains rounded dialog, generous spacing, 44px controls, mobile bounds, focus and restrained motion", () => {
  const css = readFileSync(new URL("../src/styles/safety.css", import.meta.url), "utf8");
  for (const pattern of [/border-radius: 24px/, /padding: 24px/, /min-height: 44px/, /min-width: 44px/,
    /calc\(100vw - 32px\)/, /100dvh/, /overflow-wrap: anywhere/, /focus-visible/, /letter-spacing: 0/,
    /@media \(hover: hover\) and \(pointer: fine\)/, /prefers-reduced-motion/, /\[hidden\]/]) assert.match(css, pattern);
  assert.doesNotMatch(css, /transition: all|box-shadow|font-size:[^;]*vw/);
  const source = readFileSync(new URL("../src/app/safety-view.ts", import.meta.url), "utf8");
  assert.doesNotMatch(source, /innerHTML|localStorage|className = ["'][^"']*card/);
  assert.match(source, /from "lucide"/);
});
