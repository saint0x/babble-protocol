import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { api, access, item, decided, appealed, closed, owner, author, reviewer, second, object, reportId, signalId, text, deferred, harness, settle } from "./moderation-fixtures.test.mjs";
const abort = () => new AbortController().signal;
const select = { kind: "detail", id: reportId };

test("canonical backend fixtures parse without local wire-type substitutes", () => {
  const fixture = JSON.parse(readFileSync(new URL("../../fixtures/protocol/v1/fixtures.json", import.meta.url), "utf8")).moderation;
  assert.ok(fixture);
  for (const name of ["pending", "reviewer_view", "author_view", "reporter_view"]) {
    const parsed = api.parseModerationCase(fixture[name]);
    assert.deepEqual(JSON.parse(JSON.stringify(parsed)), fixture[name]);
  }
  assert.equal(api.parseModerationCase(fixture.author_view).details, null);
  assert.equal(api.parseModerationCase(fixture.reporter_view).appeal.details, null);
  const source = readFileSync(new URL("../src/app/moderation.ts", import.meta.url), "utf8");
  assert.match(source, /ProtocolTypes\["moderation.ModerationCase"\]/);
  assert.match(source, /ProtocolTypes\["api.ReportRequest"\]/);
  assert.doesNotMatch(source, /interface ModerationCase/);
});

test("strict parsers enforce account binding, canonical IDs, scalar and byte bounds", () => {
  assert.equal(api.parseModerationAccess(access(), owner).actor_id, owner);
  assert.throws(() => api.parseModerationAccess(access(author), owner));
  for (const patch of [{ can_review: "true" }, { reasons: ["spam", "spam"] }, { reasons: ["unknown"] }, { policy_version: "other" }]) assert.throws(() => api.parseModerationAccess({ ...access(), ...patch }, owner));
  assert.equal(api.moderationText("😀".repeat(4000)), true);
  for (const v of ["x".repeat(19), "x".repeat(4001), "😀".repeat(4001), ` ${text}`, "\ud800".repeat(20)]) assert.equal(api.moderationText(v), false);
  assert.equal(api.moderationSignals([signalId]), true); assert.equal(api.moderationSignals([signalId, signalId]), false);
  assert.equal(api.moderationSignals([object]), false);
});
test("case parser rejects partial redactions, impossible states and non-independent reviewers", () => {
  for (const value of [item(), decided(), appealed(), closed()]) assert.equal(api.parseModerationCase(value).status, value.status);
  const redacted = { ...decided(), reporter_id: null, reason: null, details: null }; assert.equal(api.parseModerationCase(redacted).details, null);
  for (const patch of [{ reporter_id: null }, { details: null }, { sequence: 0 }, { revision: 1.1 }, { created_at: "bad" }, { decisions: decided().decisions }, { status: "closed" }]) assert.throws(() => api.parseModerationCase(item(patch)));
  assert.throws(() => api.parseModerationCase({ ...decided(), decisions: [{ ...decided().decisions[0], reviewer_id: owner }] }));
  assert.throws(() => api.parseModerationCase({ ...closed(), decisions: [closed().decisions[0], closed().decisions[0]] }));
  assert.equal(Object.isFrozen(api.parseModerationCase(closed()).decisions), true);
});
test("stable pagination rejects duplicates, non-descending pages and looping cursors", () => {
  const page = { items: [item()], next_before: 10 }; assert.equal(api.parseModerationPage(page, 11).next_before, 10);
  for (const bad of [{ items: [item(), item()], next_before: null }, { items: [], next_before: 10 }, { ...page, next_before: 11 }, { items: Array(26).fill(item()), next_before: null }]) assert.throws(() => api.parseModerationPage(bad));
  assert.throws(() => api.parseModerationPage(page, 10));
});
test("review and appeal eligibility derives from server access, parties, and initial reviewer", () => {
  assert.equal(api.reviewEligible(access(reviewer, true), item()), true);
  for (const a of [access(reviewer), access(owner, true), access(author, true), null]) assert.equal(api.reviewEligible(a, item()), false);
  assert.equal(api.reviewEligible(access(reviewer, true), appealed()), false);
  assert.equal(api.reviewEligible(access(second, true), appealed()), true);
  assert.equal(api.reviewEligible(access(second, true), closed()), false);
  assert.equal(api.appealEligible(owner, decided("no_action")), true);
  assert.equal(api.appealEligible(author, decided()), true);
  assert.equal(api.appealEligible(owner, decided()), false);
  assert.equal(api.appealEligible(author, appealed()), false);
});
test("client uses authenticated no-store REST, validates writes and binds receipts", async () => {
  const calls = [], client = new api.ModerationClient("https://node.example/rpc", async (url, init) => { calls.push({ url, init }); return Response.json(url.pathname.endsWith("access") ? access() : item()); });
  await client.access(owner, abort());
  const intent = { object_id: object, reason: "spam", details: text, idempotency_key: "report-1" };
  await client.report(owner, intent, abort());
  assert.equal(calls[1].url.href, "https://node.example/moderation/reports");
  assert.deepEqual(JSON.parse(calls[1].init.body), intent);
  for (const call of calls) { assert.equal(call.init.cache, "no-store"); assert.equal(call.init.credentials, "omit"); assert.equal(call.init.redirect, "error"); }
  await assert.rejects(client.report(owner, { ...intent, details: "short" }, abort())); assert.equal(calls.length, 2);
  await assert.rejects(client.detail(owner, "../escape", abort())); assert.equal(calls.length, 2);
  const wrong = new api.ModerationClient("https://node.example", async () => Response.json(item({ reporter_id: author })));
  await assert.rejects(wrong.report(owner, intent, abort()));
});
test("client sends exact decision/appeal CAS and rejects mismatched or stale receipts", async () => {
  const writes = [];
  const client = new api.ModerationClient("https://node.example", async (url, init) => { writes.push(JSON.parse(init.body)); return Response.json(url.pathname.endsWith("decisions") ? decided() : appealed()); });
  const d = { outcome: "restrict", reason: "spam", explanation: text, policy_version: "babble.integrity.v1", source_signals: [], expected_revision: 1, idempotency_key: "decision-1" };
  await client.decide(reviewer, reportId, d, abort());
  await client.appeal(author, reportId, { details: text, expected_revision: 2, idempotency_key: "appeal-1" }, abort());
  assert.deepEqual(writes[0], d);
  await assert.rejects(client.decide(reviewer, reportId, { ...d, expected_revision: 2 }, abort()));
  await assert.rejects(client.appeal(owner, reportId, { details: text, expected_revision: 2, idempotency_key: "appeal-1" }, abort()));
});
test("client bounds streamed JSON and does not expose arbitrary server error bodies", async () => {
  for (const response of [new Response("x".repeat(5000)), new Response(new Uint8Array([0xff])), Response.json({ error: "private secret" }, { status: 403 })]) {
    const client = new api.ModerationClient("https://node.example", async () => response);
    await assert.rejects(client.access(owner, abort()), error => !error.message.includes("private secret"));
  }
  const client = new api.ModerationClient("https://node.example", async () => { throw { status: 401 }; });
  await assert.rejects(client.access(owner, abort()), error => error.status === 401);
});
test("report receipt triggers callback only after verification, with authoritative readback", async () => {
  const h = harness(); await h.controller.show({ kind: "report", objectId: object });
  await h.controller.report("spam", text);
  assert.equal(h.calls.report.length, 1); assert.equal(h.calls.detail.length, 1); assert.equal(h.changes.length, 1);
  assert.equal(h.controller.view.detail.id, reportId); assert.equal(h.controller.uncertain, false);
});
test("lost ACK retries exact intent across dialog close; never silently creates another report", async () => {
  const h = harness(); await h.controller.show({ kind: "report", objectId: object }); h.failure = new api.ModerationError(503);
  await h.controller.report("spam", text); const intent = h.calls.report[0].intent;
  assert.equal(h.changes.length, 0); assert.equal(h.controller.uncertain, true);
  await h.controller.report("fraud", text); assert.equal(h.calls.report.length, 1);
  h.controller.hide(); await h.controller.show({ kind: "inbox", scope: "mine" }); h.failure = null;
  await h.controller.retry(); assert.equal(h.calls.report[1].intent, intent); assert.equal(h.changes.length, 1);
});
test("original idempotent receipt is never presented as current after another actor closes case", async () => {
  const h = harness(reviewer, true); await h.controller.show(select);
  h.source.decide = async () => decided(); h.source.detail = async () => closed();
  await h.controller.decide({ outcome: "restrict", reason: "spam", explanation: text, source_signals: [] });
  assert.equal(h.controller.view.detail.revision, 4); assert.equal(h.controller.view.detail.status, "closed");
  assert.equal(h.changes[0].revision, 4);
});
test("confirmed mutation plus failed readback clears retry but exposes refresh, no stale actionable case", async () => {
  const h = harness(reviewer, true); await h.controller.show(select); h.source.detail = async () => { throw new Error("offline"); };
  await h.controller.decide({ outcome: "restrict", reason: "spam", explanation: text, source_signals: [] });
  assert.equal(h.controller.uncertain, false); assert.equal(h.controller.view.detail, null); assert.match(h.controller.view.message, /Submission confirmed/);
  assert.equal(h.changes.length, 1);
  await h.controller.decide({ outcome: "no_action", reason: "spam", explanation: text, source_signals: [] }); assert.equal(h.calls.decide.length, 1);
});
test("CAS conflict refreshes revision and explicit corrected resubmission is allowed", async () => {
  const h = harness(reviewer, true); await h.controller.show(select); h.failure = new api.ModerationError(409);
  await h.controller.decide({ outcome: "restrict", reason: "spam", explanation: text, source_signals: [] });
  assert.equal(h.controller.view.detail.revision, 1); assert.equal(h.controller.uncertain, false);
  h.failure = null; await h.controller.decide({ outcome: "restrict", reason: "spam", explanation: text, source_signals: [] });
  assert.equal(h.calls.decide.length, 2); assert.notEqual(h.calls.decide[0].intent.idempotency_key, h.calls.decide[1].intent.idempotency_key);
});
test("validation and definitive rejection do not permanently block corrected forms", async () => {
  const h = harness(); await h.controller.show({ kind: "report", objectId: object });
  await h.controller.report("spam", "short"); assert.equal(h.calls.report.length, 0);
  h.failure = new api.ModerationError(400); await h.controller.report("spam", text); assert.equal(h.controller.uncertain, false);
  h.failure = null; await h.controller.report("spam", text); assert.equal(h.calls.report.length, 2); assert.equal(h.changes.length, 1);
});
test("account switch aborts writes, discards retry and suppresses late receipt callbacks", async () => {
  const h = harness(), pending = deferred(); await h.controller.show({ kind: "report", objectId: object });
  let signal; h.source.report = (_owner, _intent, s) => { signal = s; return pending.promise; };
  const write = h.controller.report("spam", text); h.controller.account(author); assert.equal(signal.aborted, true);
  pending.resolve(item()); await write;
  assert.equal(h.changes.length, 0); assert.equal(h.controller.view.detail, null); assert.equal(h.controller.uncertain, false); assert.equal(h.controller.pending, false);
});
test("close suppresses late reads while account lifetime write may finish without reopening", async () => {
  const h = harness(); await h.controller.show({ kind: "report", objectId: object });
  const pending = deferred(); h.source.report = () => pending.promise;
  const write = h.controller.report("spam", text); h.controller.hide(); pending.resolve(item()); await write;
  assert.equal(h.controller.view.selection, null); assert.equal(h.changes.length, 1);
  const read = deferred(); h.source.detail = () => read.promise;
  const show = h.controller.show(select); await Promise.resolve(); h.controller.hide(); read.resolve(item()); await show;
  assert.equal(h.controller.view.selection, null); assert.equal(h.controller.view.detail, null);
});
test("no queue request without authority; no unauthorized direct controller mutations", async () => {
  const h = harness(); await h.controller.show({ kind: "inbox", scope: "queue" }); assert.equal(h.calls.list.length, 0);
  await h.controller.show(select); await h.controller.decide({ outcome: "restrict", reason: "spam", explanation: text, source_signals: [] });
  await h.controller.appeal(text); assert.equal(h.calls.decide.length + h.calls.appeal.length, 0);
});
test("capacity errors are bounded and actionable, distinct from CAS conflict", async () => {
  const client = new api.ModerationClient("https://node.example", async () => Response.json({ code: "report_intake_limit", message: "untrusted private text" }, { status: 409 }));
  await assert.rejects(client.report(owner, { object_id: object, reason: "spam", details: text, idempotency_key: "request:key" }, abort()), error => {
    assert.equal(error.code, "report_intake_limit"); assert.match(error.message, /1,000-report limit/); assert.match(error.message, /reviewed or appealed/); assert.doesNotMatch(error.message, /untrusted/); return true;
  });
  const h = harness(); await h.controller.show({ kind: "report", objectId: object }); h.failure = new api.ModerationError(409, "report_intake_limit");
  await h.controller.report("spam", text); assert.equal(h.calls.access.length, 1); assert.equal(h.controller.uncertain, false); assert.match(h.controller.view.message, /1,000-report/);
});
test("transport sends only explicit contract fields, never client-supplied actor metadata", async () => {
  let body;
  const client = new api.ModerationClient("https://node.example", async (_url, init) => { body = JSON.parse(init.body); return Response.json(item()); });
  await client.report(owner, { object_id: object, reason: "spam", details: text, idempotency_key: "report:key", reviewer_id: reviewer, reporter_id: author }, abort());
  assert.equal("reviewer_id" in body, false); assert.equal("reporter_id" in body, false);
});
test("double submit is serialized and readback blocks another action until authoritative state arrives", async () => {
  const h = harness(), pending = deferred(); await h.controller.show({ kind: "report", objectId: object });
  h.source.detail = () => pending.promise;
  const first = h.controller.report("spam", text); await settle(); await h.controller.report("spam", text);
  assert.equal(h.calls.report.length, 1); assert.equal(h.controller.pending, true); assert.equal(h.changes.length, 0);
  pending.resolve(item()); await first; assert.equal(h.changes.length, 1); assert.equal(h.controller.pending, false);
});
test("late pre-mutation reads cannot overwrite confirmed current state", async () => {
  const h = harness(reviewer, true); await h.controller.show(select);
  const old = deferred(); let reads = 0;
  h.source.detail = () => ++reads === 1 ? old.promise : Promise.resolve(h.current);
  const refresh = h.controller.refresh(); await settle();
  // A selection switch invalidates the old read even when a source ignores its abort signal.
  await h.controller.show(select);
  await h.controller.decide({ outcome: "restrict", reason: "spam", explanation: text, source_signals: [] });
  old.resolve(item()); await refresh; assert.equal(h.controller.view.detail.revision, 2);
});
test("account change during mutation readback suppresses private result and callback", async () => {
  const h = harness(), pending = deferred(); await h.controller.show({ kind: "report", objectId: object }); h.source.detail = () => pending.promise;
  const operation = h.controller.report("spam", text); await settle(); h.controller.account(author); pending.resolve(item()); await operation;
  assert.equal(h.controller.view.selection, null); assert.equal(h.controller.pending, false); assert.equal(h.changes.length, 0);
});
test("malformed successful response remains uncertain, never notifies changed", async () => {
  const h = harness(); await h.controller.show({ kind: "report", objectId: object }); h.source.report = async () => item({ object_id: "wrong" });
  await h.controller.report("spam", text); assert.equal(h.controller.uncertain, true); assert.equal(h.changes.length, 0);
});
test("load more preserves existing rows on error and validates descending continuation", async () => {
  const h = harness(); h.source.list = async () => ({ items: [item()], next_before: 10 }); await h.controller.show({ kind: "inbox", scope: "mine" });
  h.source.list = async () => { throw new Error("offline"); }; await h.controller.refresh(true);
  assert.equal(h.controller.view.items.length, 1); assert.equal(h.controller.view.nextBefore, 10);
  h.source.list = async (_owner, _scope, before) => { assert.equal(before, 10); return { items: [item({ id: reportId.replace("f", "e"), sequence: 9 })], next_before: null }; };
  await h.controller.refresh(true); assert.equal(h.controller.view.items.length, 2); assert.equal(h.controller.view.nextBefore, null);
});
test("ordinary account never renders another party's private evidence from a malformed authorized response", async () => {
  const h = harness(author); h.current = decided(); await h.controller.show(select);
  assert.equal(h.controller.view.detail, null); assert.equal(h.controller.view.error, true);
  h.current = { ...decided(), reporter_id: null, reason: null, details: null }; await h.controller.refresh();
  assert.equal(h.controller.view.detail.details, null);
  const reporter = harness(owner); reporter.current = appealed(); await reporter.controller.show(select);
  assert.equal(reporter.controller.view.detail, null);
  reporter.current = { ...appealed(), appeal: { ...appealed().appeal, appellant_id: null, details: null } }; await reporter.controller.refresh();
  assert.equal(reporter.controller.view.detail.appeal.details, null);
});
