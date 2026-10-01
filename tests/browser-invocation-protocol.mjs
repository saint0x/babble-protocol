import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";

// Real API lifecycle checks with no native effect. Reporting context_lost tests
// durable failure/ack replay without fabricating a native-success result.
export async function verifyBrowserDispatchProtocol({ task, invocation }) {
  const results = [];
  for (const method of ["babble.clipboard.write", "babble.fullscreen.enter"]) {
    const input = { method, key: randomUUID(), payload: method.includes("clipboard") ? { text: "Protocol-only unexecuted intent" } : {}, timeoutMs: 30000 };
    const prepared = await task((s, input) => s.prepare(input), input);
    assert.equal(prepared.status, 200);
    const id = prepared.body.invocation_id;
    if (method.includes("fullscreen")) assert.deepEqual(prepared.body.payload, { target_hint: null, navigation_ui: "auto" });
    assert.ok((await invocation(id, "dispatch")).status >= 400, "unapproved dispatch must fail");
    assert.equal((await invocation(id, "decision", { decision: "allow_once" })).body.state.kind, "approved");
    const winners = await task(async (s, id) => Promise.all([s.invocation(id, "dispatch"), s.invocation(id, "dispatch")]), id);
    assert.ok(winners.every(result => result.status === 200));
    const tickets = winners.filter(result => result.body.execution_ticket);
    assert.equal(tickets.length, 1, "only first winning response receives execution authority");
    const ticket = tickets[0].body.execution_ticket;
    assert.equal(ticket.executor, "babble.browser.v1");
    assert.equal((await invocation(id)).body.execution_ticket, null, "lost dispatch response cannot recover native authority through status");
    assert.equal((await invocation(id, "dispatch")).body.execution_ticket, null, "retry cannot recover native authority");
    const ack = { dispatch_id: ticket.dispatch_id, result: { kind: "failed", code: "context_lost" } };
    const committed = await invocation(id, "ack", ack);
    assert.equal(committed.status, 200);
    assert.equal(committed.body.state.kind, "failed");
    assert.deepEqual(committed.body.result, ack.result);
    assert.deepEqual(await invocation(id, "ack", ack), committed, "lost ack response can be recovered by exact retry");
    assert.deepEqual((await invocation(id)).body, committed.body);
    assert.equal((await invocation(id, "ack", { ...ack, result: { kind: "failed", code: "native_error" } })).status, 409);
    assert.equal((await invocation(id, "ack", { ...ack, dispatch_id: "b".repeat(64) })).status, 409);
    const late = await invocation(id, "dispatch");
    assert.ok(late.status >= 400 || late.body.execution_ticket === null);
    results.push({ method, invocationId: id, concurrentDispatch: true, lostDispatch: true, lostAck: true, nativeEffect: false });
  }
  return results;
}
