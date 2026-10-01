import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { readFile } from "node:fs/promises";
import { assertLiveStackIsolation } from "./live-stack-isolation.mjs";
import { installBrowserInvocationFixture, browserInvocationApplication } from "./browser-invocation-fixture.mjs";
import { verifyBrowserDispatchProtocol } from "./browser-invocation-protocol.mjs";
import { publishPermissionApp } from "./bundle-permissions-browser.mjs";

export async function assertBrowserInvocationIsolation(config) {
  await assertLiveStackIsolation(config, "BABEL_BROWSER_INVOCATION_SOURCE_FROZEN");
}

// Requires parent freeze, disposable live-stack and the generated/built v2 SDK.
// Clipboard stops before native execution. Failure-ack protocol checks below
// are separate evidence from actual fullscreen and SDK adapter coverage.
export async function verifyHostActions({ execute, waitFor, apiUrl, password, readSurfaceDocument, rpc, log = console.log }) {
  assert.equal(apiUrl, "http://127.0.0.1:18787", "disposable fixture API required");
  assert.equal(typeof password, "string", "fixture account password required for same-actor/login isolation");
  assert.equal(typeof readSurfaceDocument, "function", "disposable registration observer required");
  const evaluate = async code => {
    const response = await execute([{ type: "eval", code }]);
    assert.equal(response.results?.[0]?.ok, true, "browser invocation Aegis evaluation failed");
    return response.results[0].value;
  };
  const state = "window.__browserInvocationTest?.snapshot()";
  const until = async predicate => {
    const value = await waitFor(state, value => value && (value.failure || predicate(value)));
    assert.equal(value.failure, null, JSON.stringify(value.failure));
    return value;
  };
  const task = async (fn, ...args) => {
    await evaluate(`window.__browserInvocationTest.task(${fn.toString()}, ${JSON.stringify(args)}); true`);
    return (await until(value => value.taskDone)).taskResult;
  };
  const marker = `Browser invocation ${randomUUID()}`;
  const keys = new Map();
  const send = (id, method, payload, { key = id, timeoutMs = 30000 } = {}) => {
    if (!keys.has(key)) keys.set(key, randomUUID());
    return task((s, input) => s.send(input), { id, method, payload, key: keys.get(key), timeoutMs });
  };
  const result = async id => (await until(value => value.results[id])).results[id];
  const click = selector => evaluate(`document.querySelector(${JSON.stringify(selector)}).click(); true`);
  const invocation = (id, action = "status", body) => task((s, ...args) => s.invocation(...args), id, action, body);
  const prompt = async (id, literal) => {
    const value = await until(value => value.prompt?.open || value.results[id]);
    assert.equal(value.results[id], undefined, `${id}: expected durable consent prompt`);
    assert.equal(value.prompt.hostOwned, true);
    assert.equal(value.prompt.modal, true);
    assert.equal(value.prompt.cancelFocused, true);
    assert.equal(value.prompt.injectedMarkup, false);
    assert.ok(value.prompt.text.includes(value.actorHandle), "prompt must identify the approving actor");
    assert.ok(value.prompt.text.includes(marker), "prompt must identify the requesting Object");
    if (literal !== undefined) assert.equal(value.prompt.preview, literal, "clipboard preview must be literal");
    assert.equal(typeof value.prompt.stage, "string", "v2 stage marker required before any clipboard decision click");
    assert.equal(value.prompt.stage, "consent");
    const prepared = await task((s, id) => s.prepare(id), id);
    assert.equal(prepared.status, 200);
    assert.equal(prepared.body.state.kind, "pending");
    assert.equal(prepared.body.execution_ticket, null);
    assert.equal(prepared.body.result, null);
    assert.equal(prepared.body.actor_id, value.actorId);
    assert.deepEqual(prepared.body.origin, { kind: "surface", session_id: value.sessionId, document_id: value.documentId });
    return prepared.body;
  };
  const terminal = async (id, kinds) => {
    const response = await task(async (s, id, kinds) => {
      const deadline = Date.now() + 5000;
      let response;
      do {
        response = await s.invocation(id);
        if (response.status !== 200 || kinds.includes(response.body.state.kind)) return response;
        await new Promise(resolve => setTimeout(resolve, 50));
      } while (Date.now() < deadline);
      return response;
    }, id, kinds);
    assert.equal(response.status, 200);
    assert.ok(kinds.includes(response.body.state.kind), JSON.stringify(response.body));
    assert.equal(response.body.execution_ticket, null);
    return response.body;
  };
  const paths = ["sdk.js", "client.js", "bridge.js", "transport.js", "channel.js", "lifecycle.js", "generated/protocol.js"];
  const files = await Promise.all(paths.map(async path => ({ path: `sdk/${path}`,
    content: await readFile(new URL(`../sdk/dist/${path}`, import.meta.url), "utf8") })));
  for (const method of ["clipboard.write", "fullscreen.enter"]) {
    assert.ok(files[0].content.includes(`babel.${method}.v2`), "parent must build the integrated v2 SDK first");
  }
  const evidence = { nativeClipboardWriteTested: false, nativeFullscreen: null, protocolOnly: [], operations: [] };
  let installed = false;
  let acceptanceFailure;
  try {
    // Preserve the existing composer declaration/draft regression in full runs.
    if (rpc) await publishPermissionApp({ evaluate, waitFor, rpc, marker: `${marker} authoring`, files: [
      { path: "index.html", media_type: "text/html", content: '<!doctype html><html><head><meta charset="utf-8"><title>Permission declaration fixture</title></head><body>Permission declaration fixture</body></html>' },
    ] });
    await evaluate(`(${installBrowserInvocationFixture.toString()})(${JSON.stringify({ apiUrl, marker, password })}); true`);
    installed = true;
    await until(value => value.ready);
    const setup = await task((s, ...args) => s.publish(...args), files, browserInvocationApplication.toString());
    assert.equal(setup.signature.algorithm, "Ed25519");
    assert.match(setup.signature.bytes, /^[0-9a-f]{128}$/);
    assert.deepEqual(await task(s => s.grants()), []);
    await waitFor("({ id: document.querySelector('.post-card[data-offset=\"0\"]')?.dataset.objectId })", value => value.id === setup.objectId);
    await evaluate(`(() => {
      const card = document.querySelector('.post-card[data-offset="0"]');
      card.querySelector('.action-popover[data-kind="protocol"] > button').click();
      card.querySelector('[data-action="permissions"]').click(); return true;
    })()`);
    const storage = '[data-permission-id="babel.storage.local"] [data-permission-action="approve"]';
    await waitFor(`({ ready: !!document.querySelector(${JSON.stringify(storage)}) && document.querySelector('[data-permission-dialog]')?.getAttribute('aria-busy') === 'false' })`, value => value.ready);
    await click(storage);
    await waitFor("({ enabled: !document.querySelector('[data-permission-open]')?.disabled })", value => value.enabled);
    await click("[data-permission-open]");
    const active = await until(value => value.phase === "active");
    const binding = await readSurfaceDocument({ actorId: active.actorId, objectId: setup.objectId });
    const admitted = await task((s, binding) => s.connect(binding), binding);
    evidence.sessionReadWithDocumentHeader = admitted.documentReadProbe;
    log("Session GET header probe", JSON.stringify(admitted.documentReadProbe));
    assert.match(new URL(admitted.origin).hostname, /^m-[0-9a-f]{48}\.localhost$/);
    assert.deepEqual(admitted.sandbox, ["allow-same-origin", "allow-scripts"]);
    await until(value => value.childReady);
    const mounted = await evaluate(state);
    const grants = await task(s => s.grants());
    assert.equal(grants.length, 1, "only the existing storage capability receives a grant");
    const effects = await task(s => s.effects());

    await send("store", "babel.storage.local.set.v1", { key: "host-actions/counter", value: { count: 3 } });
    assert.equal((await result("store")).error, null);
    await send("read", "babel.storage.local.get.v1", { key: "host-actions/counter" });
    assert.deepEqual((await result("read")).result?.entry?.value, { count: 3 });
    for (const method of ["clipboard.write", "fullscreen.enter"]) {
      await send(`legacy-${method}`, `babel.${method}.v1`, method.startsWith("clipboard") ? { text: "legacy rejected" } : {});
      assert.equal((await result(`legacy-${method}`)).error?.code, "UNSUPPORTED_VERSION");
    }

    const literal = '<b data-browser-injected>Copy this exact text</b>\nSpacing & punctuation.';
    await send("denied", "babel.clipboard.write.v2", { text: literal });
    const denied = await prompt("denied", literal);
    const geometry = await task(async () => {
      const dialog = document.querySelector('[data-host-action-dialog]');
      const animations = [];
      for (let node = dialog; node; node = node.parentElement) animations.push(...node.getAnimations());
      await Promise.allSettled(animations.filter(animation => animation.effect?.getComputedTiming().iterations !== Infinity).map(animation => animation.finished));
      const box = dialog.getBoundingClientRect();
      const buttons = [...dialog.querySelectorAll('button')].map(button => button.getBoundingClientRect());
      return { contained: box.left >= 0 && box.right <= innerWidth && box.top >= 0 && box.bottom <= innerHeight,
        overflow: dialog.scrollWidth > dialog.clientWidth,
        targets: buttons.every(button => button.width >= 43.99 && button.height >= 43.99),
        buttonsInside: buttons.every(button => button.left >= box.left && button.right <= box.right) };
    });
    assert.deepEqual(geometry, { contained: true, overflow: false, targets: true, buttonsInside: true });
    assert.deepEqual((await task((s, id) => s.prepare(id), "denied")).body, denied, "prepare retry preserves intent/deadline");
    const isolation = await task((s, id) => s.isolation(id), denied.invocation_id);
    for (const codes of Object.values(isolation)) assert.deepEqual(codes, { anonymous: 401, otherActor: 403, otherLogin: 403, wrongDocument: 403 });
    assert.equal((await invocation(denied.invocation_id)).body.state.kind, "pending", "unauthorized decisions have no effect");
    await task(s => { s.port.postMessage({ type: "mutate", id: "denied", text: "Changed child intent" }); return true; });
    await until(value => value.mutated === "denied");
    assert.equal((await evaluate(state)).prompt.preview, literal);
    assert.equal((await task(s => s.prepare("denied", { text: "Changed child intent" }))).status, 409);
    await send("browser-busy", "babel.fullscreen.enter.v2", {});
    assert.equal((await result("browser-busy")).error?.code, "RATE_LIMITED");
    await send("social-busy", "babel.social.reply.v2", { author_id: mounted.actorId, target_object_id: mounted.objectId, text: `${marker} blocked social` });
    assert.equal((await result("social-busy")).error?.code, "RATE_LIMITED");
    await click("[data-host-action-cancel]");
    assert.ok((await result("denied")).error);
    assert.equal((await terminal(denied.invocation_id, ["denied"])).result, null);
    await send("denied-retry", "babel.clipboard.write.v2", { text: literal }, { key: "denied" });
    assert.ok((await result("denied-retry")).error);
    assert.equal((await evaluate(state)).prompt, null, "terminal retry must not reprompt");

    await send("social-pending", "babel.social.reply.v2", { author_id: mounted.actorId, target_object_id: mounted.objectId, text: `${marker} cancelled social` });
    await until(value => value.social?.open);
    await send("browser-during-social", "babel.clipboard.write.v2", { text: "Must stay blocked" });
    assert.equal((await result("browser-during-social")).error?.code, "RATE_LIMITED");
    await click('[data-invocation-prompt] [aria-label="Cancel request"]');
    assert.ok((await result("social-pending")).error);

    await send("cancelled", "babel.clipboard.write.v2", { text: "Cancel before approval" });
    const cancelled = await prompt("cancelled", "Cancel before approval");
    await task(s => s.cancel("cancelled"));
    assert.ok((await result("cancelled")).error);
    await until(value => !value.prompt);
    await terminal(cancelled.invocation_id, ["cancelled"]);

    await send("ready-cancel", "babel.clipboard.write.v2", { text: literal });
    const staged = await prompt("ready-cancel", literal);
    await click("[data-host-action-allow]");
    const ready = await until(value => value.prompt?.stage === "ready" || value.results["ready-cancel"]);
    assert.equal(ready.results["ready-cancel"], undefined, "allow-once must await a separate native button");
    assert.equal(ready.prompt.preview, literal);
    const stagedStatus = await terminal(staged.invocation_id, ["running"]);
    assert.equal(stagedStatus.result, null);
    assert.equal((await invocation(staged.invocation_id, "dispatch")).body.execution_ticket, null);
    await send("ready-social-busy", "babel.social.reply.v2", { author_id: mounted.actorId, target_object_id: mounted.objectId, text: `${marker} blocked while ready` });
    assert.equal((await result("ready-social-busy")).error?.code, "RATE_LIMITED");
    // Intentionally never click the ready-stage clipboard button.
    await click("[data-host-action-cancel]");
    assert.ok((await result("ready-cancel")).error);
    assert.deepEqual((await terminal(staged.invocation_id, ["failed"])).result, { kind: "failed", code: "context_lost" });
    evidence.operations.push("literal immutable intent", "same actor/different login and document isolation", "busy both domains", "deny/cancel", "allow-once then separate native stage");

    evidence.protocolOnly = await verifyBrowserDispatchProtocol({ task, invocation });
    await send("fullscreen", "babel.fullscreen.enter.v2", { target_hint: "#untrusted-child-selector", navigation_ui: "show" });
    if (!(await evaluate(state)).fullscreenAvailable) {
      assert.equal((await result("fullscreen")).error?.code, "CAPABILITY_UNAVAILABLE");
      assert.equal((await evaluate(state)).prompt, null);
      evidence.nativeFullscreen = { success: false, trustedClick: false, limitation: "Fullscreen unavailable before RPC in this Aegis browser" };
    } else {
    const fullscreenView = await prompt("fullscreen");
    await click("[data-host-action-allow]");
    await until(value => value.prompt?.stage === "ready" || value.results.fullscreen);
    const beforeNative = await evaluate(state);
    if (!beforeNative.results.fullscreen) {
      const { label } = await evaluate("({ label: document.querySelector('[data-host-action-stage=ready] [data-host-action-allow]').textContent.trim() })");
      assert.equal(label, "Enter fullscreen");
      const nativeClick = await execute([{ type: "click", match: { control_type: "button", name: label, actionable: true } }]);
      assert.equal(nativeClick.results?.[0]?.ok, true, `Aegis native fullscreen input failed: ${JSON.stringify(nativeClick)}`);
    }
    const fullscreen = await result("fullscreen");
    const native = await evaluate(state);
    const trusted = native.clicks.some(click => click.stage === "ready" && click.trusted);
    if (fullscreen.error) {
      assert.equal(fullscreen.result, null);
      assert.equal(native.ownedFullscreen, false);
      await terminal(fullscreenView.invocation_id, ["failed"]);
      evidence.nativeFullscreen = { success: false, trustedClick: trusted, error: fullscreen.error.code,
        limitation: "Native fullscreen was rejected or unavailable in this Aegis browser" };
    } else {
      assert.equal(trusted, true, "fullscreen success requires a real trusted Aegis click");
      assert.deepEqual(fullscreen.result, { kind: "fullscreen_enter", entered: true });
      assert.equal(native.ownedFullscreen, true, "host-reported success must match the real fullscreen element");
      const completed = await terminal(fullscreenView.invocation_id, ["completed"]);
      assert.deepEqual(completed.result, fullscreen.result);
      await send("fullscreen-retry", "babel.fullscreen.enter.v2", { target_hint: "#untrusted-child-selector", navigation_ui: "show" }, { key: "fullscreen" });
      assert.deepEqual(await result("fullscreen-retry"), fullscreen);
      assert.equal((await evaluate(state)).prompt, null);
      evidence.nativeFullscreen = { success: true, trustedClick: true, acknowledgment: "host-reported; server does not observe native effects" };
    }
    }
    await send("closing", "babel.clipboard.write.v2", { text: "Cancelled on Surface close" });
    const closing = await prompt("closing", "Cancelled on Surface close");
    await click("[data-close-surface]");
    await until(value => !value.prompt && !value.frame && !value.fullscreen);
    await terminal(closing.invocation_id, ["cancelled", "invalidated"]);
    assert.ok((await invocation(closing.invocation_id, "dispatch")).status >= 400);
    assert.deepEqual(await task(s => s.effects()), effects, "denied/busy social calls must not publish");
    assert.deepEqual(await task(s => s.grants()), grants, "allow-once must not create reusable grants");
  } catch (error) {
    acceptanceFailure = error;
    throw error;
  } finally {
    if (installed) {
      const cleanup = await task(s => s.cleanup());
      await evaluate("delete window.__browserInvocationTest; true");
      if (acceptanceFailure && cleanup.errors.length) log("Acceptance cleanup errors", JSON.stringify(cleanup.errors));
      else assert.deepEqual(cleanup.errors, []);
      assert.equal(cleanup.parentAccountPreserved, true);
    }
  }
  log("Browser invocation acceptance PASS", JSON.stringify(evidence));
  return evidence;
}

if (process.argv[2] === "--contract-check") {
  const saved = process.env.BABEL_BROWSER_INVOCATION_SOURCE_FROZEN;
  const ports = { apiPort: 18787, gatewayPort: 18788, frontendPort: 14329, aegisAddr: "127.0.0.1:17878" };
  try {
    delete process.env.BABEL_BROWSER_INVOCATION_SOURCE_FROZEN;
    await assert.rejects(assertBrowserInvocationIsolation(ports), /source freeze required/);
    process.env.BABEL_BROWSER_INVOCATION_SOURCE_FROZEN = "1";
    for (const override of [{ apiPort: 8787 }, { gatewayPort: 8788 }, { frontendPort: 4321 }]) {
      await assert.rejects(assertBrowserInvocationIsolation({ ...ports, ...override }), /Disposable test ports/);
    }
    await assert.rejects(assertBrowserInvocationIsolation({ ...ports, aegisAddr: "127.0.0.1:7878" }), /Acceptance Aegis/);
    await assert.rejects(verifyHostActions({ apiUrl: "http://127.0.0.1:8787" }), /disposable fixture API/);
    await assert.rejects(verifyHostActions({ apiUrl: "http://127.0.0.1:18787" }), /fixture account password/);
    await assert.rejects(verifyBrowserDispatchProtocol({ task: async () => ({ status: 403 }), invocation: async () => { throw new Error("must not dispatch after failed prepare"); } }), /403 !== 200/);
    console.log("Browser invocation harness negative checks PASS (no browser/API/native evidence)");
  } finally {
    if (saved === undefined) delete process.env.BABEL_BROWSER_INVOCATION_SOURCE_FROZEN;
    else process.env.BABEL_BROWSER_INVOCATION_SOURCE_FROZEN = saved;
  }
}
