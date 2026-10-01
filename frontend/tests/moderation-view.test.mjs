import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { api, access, item, decided, appealed, owner, author, reviewer, second, object, text, deferred, settle, domHarness } from "./moderation-fixtures.test.mjs";
async function report(h) { h.controls.account(owner); h.controls.openReport(object, h.opener); await settle(); }
async function detail(h, actor = owner) { h.controls.account(actor); h.controls.openInbox(h.opener); await settle(); h.click("detail"); await settle(); }

test("guest sign-in closes, restores focus, and never submits after account activation", async () => {
  const h = domHarness(); h.controls.openReport(object, h.opener); assert.equal(h.all("submit-report").length, 0);
  h.click("sign-in"); assert.equal(h.dialog.open, false); assert.equal(h.signIns, 1); assert.equal(h.document.activeElement, h.opener);
  h.controls.account(owner); await settle(); assert.equal(h.calls.report.length, 0);
});
test("report form validates scalar length, reason and real submit, with explicit private scope", async () => {
  const h = domHarness(); await report(h);
  assert.equal(h.document.activeElement, h.one("close")); assert.equal(h.one("submit-report").disabled, true);
  assert.match(h.dialog.textContent, /does not restrict/); assert.match(h.dialog.textContent, /private/);
  h.enter("reason", "spam"); h.enter("details", "short"); assert.equal(h.one("submit-report").disabled, true);
  h.enter("details", text); assert.equal(h.one("submit-report").disabled, false); h.submit("report"); await settle();
  assert.equal(h.calls.report.length, 1); assert.equal(h.changes.length, 1); assert.match(h.dialog.textContent, /Report submitted/);
});
test("uncertain report preserves draft and exact retry; account switch erases draft", async () => {
  const h = domHarness(); await report(h); h.enter("reason", "fraud"); h.enter("details", text); h.failure = new api.ModerationError(503);
  h.submit("report"); await settle(); assert.equal(h.field("details").value, text); assert.equal(h.field("reason").value, "fraud");
  assert.equal(h.one("submit-report").disabled, true); const intent = h.calls.report[0].intent;
  h.controls.close(); h.controls.openReport(object, h.opener); await settle(); assert.equal(h.field("details").value, text);
  h.failure = null; h.click("retry"); await settle(); assert.equal(h.calls.report[1].intent, intent);
  h.controls.account(author); h.controls.openReport(object); await settle(); assert.equal(h.field("details").value, "");
});
test("correcting a definitively rejected report enables a new explicit submission", async () => {
  const h = domHarness(); await report(h); h.enter("reason", "spam"); h.enter("details", text); h.failure = new api.ModerationError(400);
  h.submit("report"); await settle(); h.enter("details", text + " More evidence."); h.failure = null; h.submit("report"); await settle();
  assert.equal(h.calls.report.length, 2); assert.notEqual(h.calls.report[0].intent.idempotency_key, h.calls.report[1].intent.idempotency_key);
});
test("queue tab uses server access; tabs have keyboard navigation and linked panel", async () => {
  const h = domHarness(); h.controls.account(owner); h.controls.openInbox(h.opener); await settle(); assert.equal(h.all("queue").length, 0);
  assert.equal(h.one("mine").getAttribute("role"), "tab"); assert.equal(h.one("mine").getAttribute("aria-selected"), "true");
  h.source.list = async () => ({ items: [], next_before: null }); h.one("mine").emit("keydown", { key: "ArrowRight" }); await settle();
  assert.equal(h.one("affected").getAttribute("aria-selected"), "true"); assert.equal(h.document.activeElement, h.one("affected"));
  const r = domHarness(reviewer, true); r.controls.account(reviewer); r.controls.openInbox(); await settle(); assert.equal(r.all("queue").length, 1);
});
test("affected author sees redacted evidence and real appeal form with retained restriction", async () => {
  const h = domHarness(author); h.current = { ...decided(), reporter_id: null, details: null, reason: null };
  h.controls.account(author); h.controls.openInbox(h.opener); await settle(); h.click("affected"); await settle(); h.click("detail"); await settle();
  assert.match(h.dialog.textContent, /report evidence are private/); assert.equal(h.all("submit-decision").length, 0); assert.equal(h.all("submit-appeal").length, 1);
  assert.match(h.dialog.textContent, /not deletion or a federation-wide ban/); h.enter("details", text, "appeal"); h.submit("appeal"); await settle();
  assert.equal(h.calls.appeal.length, 1); assert.match(h.dialog.textContent, /existing restriction remains/);
});
test("decision form requires outcome, reason, explanation and bounded unique source Judgments", async () => {
  const h = domHarness(reviewer, true); h.source.list = async () => ({ items: [item()], next_before: null });
  h.controls.account(reviewer); h.controls.openInbox(); await settle(); h.click("queue"); await settle(); h.click("detail"); await settle();
  h.enter("outcome", "restrict", "decision"); h.enter("reason", "spam", "decision"); h.enter("explanation", text, "decision");
  h.enter("source_signals", "not-a-judgment", "decision"); assert.equal(h.one("submit-decision").disabled, true);
  h.enter("source_signals", "", "decision"); assert.equal(h.one("submit-decision").disabled, false); h.submit("decision"); await settle();
  assert.equal(h.calls.decide.length, 1); assert.equal(h.calls.decide[0].intent.policy_version, "babble.integrity.v1"); assert.equal(h.calls.decide[0].intent.expected_revision, 1);
});
test("initial reviewer cannot resolve own appeal; independent second reviewer can", async () => {
  for (const [actor, eligible] of [[reviewer, false], [second, true]]) {
    const h = domHarness(actor, true); h.current = appealed(); h.controls.account(actor); h.controls.openInbox(); await settle(); h.click("queue"); await settle(); h.click("detail"); await settle();
    assert.equal(h.all("submit-decision").length, eligible ? 1 : 0);
    if (!eligible) assert.match(h.dialog.textContent, /independent reviewer/);
  }
});
test("detached form and late reads cannot act after account switch or dialog close", async () => {
  const h = domHarness(); await report(h); h.enter("reason", "spam"); h.enter("details", text); const form = h.form("report");
  h.controls.account(author); form.emit("submit"); await settle(); assert.equal(h.calls.report.length, 0); assert.equal(h.dialog.textContent, "");
  const pending = deferred(); h.source.access = () => pending.promise; h.controls.openReport(object); h.controls.close(); pending.resolve(access(author)); await settle();
  assert.equal(h.dialog.open, false); assert.equal(h.dialog.textContent, "");
});
test("post navigation closes dialog first and passes original opener", async () => {
  const h = domHarness(); await detail(h); h.click("open-object");
  assert.equal(h.dialog.open, false); assert.equal(h.opens[0].closed, true); assert.equal(h.opens[0].id, object); assert.equal(h.opens[0].opener, h.opener);
});
test("malicious evidence is literal text; Escape restores focus; dispose prevents reopening", async () => {
  const h = domHarness(); h.current = item({ details: '<img src=x onerror="alert(1)"> literal report evidence' }); await detail(h);
  assert.match(h.dialog.textContent, /<img/); assert.equal(h.dialog.querySelector("img"), null);
  assert.equal(h.dialog.emit("cancel").defaultPrevented, true); assert.equal(h.document.activeElement, h.opener);
  h.controls.dispose(); h.controls.openReport(object); assert.equal(h.dialog.isConnected, false); assert.equal(h.dialog.open, false);
});
test("styles match SafetyControls rounded spacing, mobile fit, focus and reduced motion", () => {
  const css = readFileSync(new URL("../src/styles/moderation.css", import.meta.url), "utf8");
  for (const pattern of [/border-radius: 24px/, /padding: 24px/, /min-height: 44px/, /min-width: 44px/, /calc\(100vw - 32px\)/, /100dvh/, /overflow-wrap: anywhere/, /focus-visible/, /letter-spacing: 0/, /prefers-reduced-motion/]) assert.match(css, pattern);
  assert.doesNotMatch(css, /transition: all|font-size:[^;]*vw/);
  const source = readFileSync(new URL("../src/app/moderation-view.ts", import.meta.url), "utf8"); assert.doesNotMatch(source, /innerHTML|localStorage/);
});
test("compact identifiers retain full accessible IDs and a deterministic Object attribute", async () => {
  const h = domHarness(); await detail(h);
  const identifier = h.dialog.querySelectorAll(`[data-moderation-object="${object}"]`).find(node => node.tagName === "P");
  assert.ok(identifier); assert.equal(identifier.title, object); assert.equal(identifier.getAttribute("aria-label"), `Post: ${object}`);
  assert.equal(identifier.textContent.includes(object), false); assert.match(identifier.textContent, /\.\.\./);
  assert.equal(h.dialog.querySelector("summary").textContent, "Local restriction scope");
  assert.match(h.dialog.textContent, /Following, and conversations/);
  assert.match(h.dialog.textContent, /quoted post previews are hidden/);
  assert.match(h.dialog.textContent, /Signed history, quote relationships, and explicit public reads remain available/);
});
