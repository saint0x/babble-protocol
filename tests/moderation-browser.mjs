import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { assertLiveStackIsolation } from "./live-stack-isolation.mjs";

const stages = ["setup", "cancel", "layout-320", "layout-390", "layout-1280", "comment-report", "report", "privacy", "review", "restriction", "restart", "appeal", "reversal", "receipts", "comment-restrict", "comment-appeal", "comment-reversal"];

export function assertModerationIsolation(config) {
  return assertLiveStackIsolation(config, "BABBLE_MODERATION_SOURCE_FROZEN");
}

// These accounts exist only under live-stack's mkdtemp store. No SQLite edits or
// operator credentials are needed: the subsequent API restart sets reviewer IDs.
export async function prepareModerationAccounts(apiUrl) {
  assert.equal(apiUrl, "http://127.0.0.1:18787");
  const accounts = {};
  for (const role of ["subject", "reviewer", "second", "outsider"]) {
    const response = await fetch(`${apiUrl}/auth/register`, {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ kind: "Person", handle: `moderation-${role}-${randomUUID()}`, password: `Moderation fixture ${randomUUID()}!` }),
      signal: AbortSignal.timeout(20_000),
    });
    assert.equal(response.status, 200, `Could not register disposable ${role}`);
    accounts[role] = await response.json();
    assert.match(accounts[role].identity.id, /^id_[a-f0-9]{64}$/);
    const access = await fetch(`${apiUrl}/moderation/access`, { headers: { authorization: `Bearer ${accounts[role].token}` } });
    assert.equal(access.status, 200);
    assert.equal((await access.json()).can_review, false, "Empty reviewer configuration must grant nobody authority");
  }
  return { ...accounts, reviewerIds: [accounts.reviewer.identity.id, accounts.second.identity.id] };
}

export async function verifyModeration({ execute, waitFor, readRequests, apiUrl, accounts, seed, restart, log = console.log }) {
  assert.equal(apiUrl, "http://127.0.0.1:18787");
  for (const callback of [execute, waitFor, readRequests, restart]) assert.equal(typeof callback, "function");
  const evaluate = async code => {
    const response = await execute([{ type: "eval", code }]);
    // Do not echo the installation expression: it contains disposable sessions.
    assert.equal(response.results?.[0]?.ok, true, "Aegis moderation evaluation failed");
    const value = response.results[0].value;
    assert.ok(value && typeof value === "object", "Aegis probes must return objects");
    return value;
  };
  const run = async stage => {
    await evaluate(`(() => {
      window.__moderationResult = null;
      Promise.resolve().then(() => window.__moderationHarness.run(${JSON.stringify(stage)}))
        .then(result => { window.__moderationResult = { result }; })
        .catch(error => { window.__moderationResult = { error: String(error), step: window.__moderationStep }; });
      return { started: true };
    })()`);
    let snapshot;
    do {
      snapshot = await waitFor("({ outcome: window.__moderationResult, step: window.__moderationStep, networkRead: window.__moderationNetworkRead?.response === null ? window.__moderationNetworkRead : null })", value => value?.outcome != null || value?.networkRead != null);
      if (snapshot.outcome != null) break;
      const { id, since, path } = snapshot.networkRead;
      let response;
      try {
        assert.ok(Number.isSafeInteger(id) && id > 0 && Number.isSafeInteger(since) && since >= 0, "Invalid moderation observation cursor");
        assert.match(path, /^\/moderation\/reports(?:\/report_[a-f0-9]{64}\/(?:decisions|appeals))?$/);
        const observed = await readRequests({ since });
        assert.ok(Number.isSafeInteger(observed?.cursor) && observed.cursor >= since && Array.isArray(observed.requests), "Invalid moderation observer result");
        response = { cursor: observed.cursor, requests: observed.requests.filter(item => item.method === "POST" && item.url === `${apiUrl}${path}`) };
      } catch { response = { error: "Moderation HTTP observer failed" }; }
      await evaluate(`(() => {
        const pending = window.__moderationNetworkRead;
        if (!pending || pending.id !== ${JSON.stringify(id)} || pending.response !== null) throw Error('Moderation observation mailbox changed');
        pending.response = ${JSON.stringify(response)};
        return { observed: true };
      })()`);
    } while (snapshot.outcome == null);
    assert.equal(snapshot.outcome.error, undefined, JSON.stringify(snapshot));
    assert.equal(snapshot.outcome.result?.verified, true, `${stage} did not complete`);
    return snapshot.outcome.result;
  };
  const coverage = {};
  let failure, installed = false;
  try {
    await evaluate(`(() => {
      if (window.__moderationHarness) throw Error('Moderation harness already active');
      window.__moderationHarness = (${moderationHarness.toString()})(${JSON.stringify({ apiUrl, accounts, seed, marker: `moderation-${randomUUID()}` })}, (${readModerationMutation.toString()}));
      return { installed: true };
    })()`);
    installed = true;
    for (const stage of stages) {
      if (stage === "restart") await restart();
      coverage[stage] = await run(stage);
      log(`[moderation] ${stage}`, JSON.stringify(coverage[stage]));
    }
  } catch (cause) { failure = cause; }
  finally {
    if (installed) {
      try { coverage.cleanup = await run("cleanup"); }
      catch (cause) { failure = failure ? new AggregateError([failure, cause], "Moderation acceptance and cleanup failed") : cause; }
      try {
        await evaluate("(() => { if (window.__moderationHarness?.busy) throw Error('Moderation stage still active'); delete window.__moderationHarness; delete window.__moderationResult; delete window.__moderationStep; delete window.__moderationNetworkRead; return { cleaned: true }; })()");
      } catch (cause) { failure = failure ? new AggregateError([failure, cause], "Moderation teardown failed") : cause; }
    }
  }
  if (failure) throw failure;
  log("Moderation real browser PASS", JSON.stringify(coverage));
  return coverage;
}

// Only completed wire responses qualify as receipts. This also rejects duplicate
// submissions, wrong actors, changed form intent and unrelated/stale observations.
export function readModerationMutation(observed, { since, apiUrl, path, actor, expected }) {
  const check = (condition, message) => { if (!condition) throw Error(message); };
  const canonical = value => Array.isArray(value) ? value.map(canonical) : value && typeof value === "object"
    ? Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])])) : value;
  const equal = (actual, wanted) => JSON.stringify(canonical(actual)) === JSON.stringify(canonical(wanted));
  check(Number.isSafeInteger(observed?.cursor) && observed.cursor >= since && Array.isArray(observed.requests), "Invalid moderation observation batch");
  const matches = observed.requests.filter(item => item.method === "POST" && item.url === `${apiUrl}${path}`);
  check(matches.every(item => Number.isSafeInteger(item.sequence) && item.sequence > since && item.sequence <= observed.cursor), "Stale moderation observation");
  check(matches.length <= 1, "UI submitted duplicate moderation mutations");
  if (!matches.length) return null;
  const item = matches[0];
  check(!item.error, "Observed moderation request failed");
  if (item.status === null || item.responseBody === null) return null;
  check(item.status === 200, `UI moderation request returned ${item.status}`);
  let body, value;
  try { body = JSON.parse(item.requestBody); value = JSON.parse(item.responseBody); }
  catch { throw Error("Observed moderation request/response was not JSON"); }
  check(body && typeof body === "object" && !Array.isArray(body), "Invalid moderation request body");
  const { idempotency_key: idempotencyKey, ...intent } = body;
  check(typeof idempotencyKey === "string" && idempotencyKey.length > 0, "UI omitted moderation idempotency key");
  check(equal(intent, expected), "UI moderation request differs from submitted form intent");
  check(value && /^report_[a-f0-9]{64}$/.test(value.id), "Invalid moderation response identity");
  if (path === "/moderation/reports") {
    check(value.reporter_id === actor && value.object_id === expected.object_id && value.reason === expected.reason && value.details === expected.details
      && value.revision === 1 && value.status === "pending", "UI report response actor or intent mismatch");
  } else {
    check(path.split("/")[3] === value.id && value.revision === expected.expected_revision + 1, "UI mutation response case or revision mismatch");
    if (path.endsWith("/appeals")) {
      check(value.appeal?.appellant_id === actor && value.appeal.details === expected.details && value.status === "appealed", "UI appeal response actor or intent mismatch");
    } else {
      const { expected_revision: _revision, ...decision } = expected;
      const actual = value.decisions?.at(-1);
      check(actual?.reviewer_id === actor && Object.entries(decision).every(([key, value]) => equal(actual[key], value)), "UI decision response actor or intent mismatch");
      check(value.status === (expected.expected_revision === 1 ? "decided" : "closed"), "UI decision response status mismatch");
    }
  }
  return { actor, path, body, value };
}

// Serialized into Aegis. API requests are real. Role frames load the production
// Astro page; session storage is restored after initialization and on cleanup.
function moderationHarness({ apiUrl, accounts, seed, marker }, readMutation) {
  const check = (condition, message) => { if (!condition) throw Error(message); };
  const canonical = value => Array.isArray(value) ? value.map(canonical) : value && typeof value === "object"
    ? Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])])) : value;
  const equal = (actual, expected, message) => check(JSON.stringify(canonical(actual)) === JSON.stringify(canonical(expected)), message);
  const sessionKey = `babble.session.v1:${apiUrl}`;
  const savedSession = sessionStorage.getItem(sessionKey);
  const reporter = JSON.parse(savedSession ?? "null");
  check(reporter?.token && reporter.identity?.id, "Disposable reporter must be signed in");
  check(location.origin === "http://127.0.0.1:14329" && document.documentElement.dataset.babbleApi === apiUrl, "Production preview must not be used");
  const restoreStorage = () => savedSession === null ? sessionStorage.removeItem(sessionKey) : sessionStorage.setItem(sessionKey, savedSession);
  const key = label => `${marker}-${label}-${crypto.randomUUID()}`;
  let frame, doc, win, object, child, sibling, childCase, quoteEdge, initialObject, initialGraph, initialOutgoing, report, session, followBefore, restrictedCase, busy = false;
  const captures = [], frames = new Set();
  const required = (selector, root = doc) => {
    const element = root?.querySelector(selector); check(element, `Missing moderation control: ${selector}`); return element;
  };
  const visible = node => !!node && !node.closest("[hidden], [inert]") && node.getClientRects().length > 0 && node.ownerDocument.defaultView.getComputedStyle(node).visibility !== "hidden";
  const until = async (label, predicate) => {
    window.__moderationStep = label;
    const deadline = Date.now() + 25000;
    while (!await predicate()) {
      check(Date.now() < deadline, `Timed out: ${label}; dialog: ${doc?.querySelector('[data-moderation-dialog]')?.textContent?.slice(0, 2500) ?? 'absent'}`);
      await new Promise(resolve => setTimeout(resolve, 50));
    }
  };
  const click = (selector, root = doc) => {
    const element = required(selector, root); check(visible(element) && !element.disabled, `Unavailable control: ${selector}`);
    element.focus(); element.click(); return element;
  };
  const fill = (selector, value) => {
    const element = required(selector); check(visible(element) && !element.disabled, `Unavailable field: ${selector}`);
    element.value = value; element.dispatchEvent(new win.Event("input", { bubbles: true })); element.dispatchEvent(new win.Event("change", { bubbles: true }));
  };
  const request = async (path, { account = reporter, method = "GET", body } = {}) => {
    const response = await fetch(new URL(path, apiUrl), {
      method, credentials: "omit", redirect: "error", signal: AbortSignal.timeout(20000),
      headers: { accept: "application/json", ...(account ? { authorization: `Bearer ${account.token}` } : {}), ...(body === undefined ? {} : { "content-type": "application/json" }) },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
    if (path.startsWith("/moderation/")) check(/\bno-store\b/.test(response.headers.get("cache-control") ?? ""), `Private response cacheable: ${method} ${path} (${response.status})`);
    const text = await response.text();
    let value; try { value = text ? JSON.parse(text) : null; } catch { throw Error(`Non-JSON response: ${method} ${path} (${response.status})`); }
    return { status: response.status, value };
  };
  const ok = async (path, options) => {
    const result = await request(path, options);
    check(result.status >= 200 && result.status < 300, `${options?.method ?? "GET"} ${path}: ${result.status} ${JSON.stringify(result.value)}`);
    return result.value;
  };
  const rpc = async (method, payload) => {
    const response = await ok("/rpc", { account: null, method: "POST", body: {
      protocol: "babble.rpc.v1", id: crypto.randomUUID(), method, payload, idempotency_key: null,
      binding: { object_id: null, surface_session_id: null, runtime_id: "moderation-acceptance", origin: location.origin, capability_grants: [] },
      deadline: { timeout_ms: 20000, client_started_at: null }, trace_id: null,
    } });
    check(response.error === null && response.result, `Public RPC ${method} failed: ${JSON.stringify(response.error)}`);
    return response.result;
  };
  const reportPath = () => `/moderation/reports/${report.id}`;
  const detail = (account = reporter) => ok(reportPath(), { account });
  const decision = (revision, outcome = "restrict") => ({ outcome, reason: "spam", explanation: "Reviewed the original Object and the submitted integrity evidence.", policy_version: "babble.integrity.v1", source_signals: [], expected_revision: revision, idempotency_key: key("decision") });
  const denied = async (path, options, status) => {
    const result = await request(path, options); check(result.status === status, `Expected ${status}: ${path}, received ${result.status}`); return result;
  };
  let observationId = 0;
  const observe = async (since, path) => {
    check(!window.__moderationNetworkRead, "Concurrent moderation observation");
    const pending = { id: ++observationId, since, path, response: null };
    window.__moderationNetworkRead = pending;
    try {
      await until("read-only HTTP observation", () => pending.response !== null);
      check(!pending.response.error, pending.response.error);
      return pending.response;
    } finally { delete window.__moderationNetworkRead; }
  };
  const submit = async (account, kind, target) => {
    const field = name => required(`[data-moderation-field="${name}"]`).value.trim();
    const path = kind === "report" ? "/moderation/reports" : `/moderation/reports/${target.id}/${kind === "decision" ? "decisions" : "appeals"}`;
    const expected = kind === "report" ? { object_id: target.id, reason: field("reason"), details: field("details") }
      : kind === "appeal" ? { details: field("details"), expected_revision: target.revision }
        : { outcome: field("outcome"), reason: field("reason"), explanation: field("explanation"), policy_version: "babble.integrity.v1", source_signals: [], expected_revision: target.revision };
    // The arrival cursor is captured immediately before this one UI action. Keep
    // reading that floor until its real response completes; never skip in-flight
    // requests when unrelated background traffic advances the observer cursor.
    const { cursor: since } = await observe(0, path);
    click(`[data-moderation-action="submit-${kind}"]`);
    let captured;
    await until("completed UI moderation HTTP mutation", async () => {
      captured = readMutation(await observe(since, path), { since, apiUrl, path, actor: account.identity.id, expected });
      return captured !== null;
    });
    equal(await ok(`/moderation/reports/${captured.value.id}`, { account }), captured.value, "UI mutation response differs from authoritative same-role readback");
    captures.push(captured);
    return captured.value;
  };
  const load = async (account, width = 1280) => {
    frame?.remove();
    frame = document.createElement("iframe"); frames.add(frame);
    frame.title = `Moderation acceptance ${width}px`;
    frame.style.cssText = `position:fixed;left:0;top:0;width:${width}px;height:${width === 320 ? 568 : 844}px;border:0;z-index:9999;background:white`;
    // The sentinel expiry lets us await Accounts.restore's real session read and
    // storage write before restoring the reporter's shared same-origin storage.
    sessionStorage.setItem(sessionKey, JSON.stringify({ ...account, expires_at: "2099-01-01T00:00:00Z" }));
    const url = new URL(location.href); url.searchParams.delete("surface"); url.searchParams.set("q", marker); url.searchParams.set("lens", "balanced");
    frame.src = url.href; document.body.append(frame);
    try {
      await until("authenticated role frame", () => frame.contentDocument?.querySelector("[data-author-status]")?.textContent === `Signed in as ${account.identity.handle}`);
      await until("role session restored by production Accounts", () => {
        const current = JSON.parse(sessionStorage.getItem(sessionKey) ?? "null");
        return current?.identity.id === account.identity.id && current.expires_at === account.expires_at;
      });
      doc = frame.contentDocument; win = frame.contentWindow;
      await doc.fonts.ready;
      check(win.innerWidth === width, "Incorrect layout viewport");
      check((await ok("/auth/session", { account })).identity.id === account.identity.id, "Role has no real API session");
    } finally { restoreStorage(); }
  };
  const ready = () => until("moderation dialog ready", () => doc.querySelector("[data-moderation-dialog]")?.open && doc.querySelector("[data-moderation-dialog]").getAttribute("aria-busy") !== "true");
  const close = async () => {
    if (doc?.querySelector("[data-moderation-dialog]")?.open) { click('[data-moderation-action="close"]'); await until("dialog closed", () => !required("[data-moderation-dialog]").open); }
  };
  const inbox = async scope => {
    await close(); click("[data-toggle-profile]");
    await until("account reports launcher", () => visible(doc.querySelector('[data-menu-action="reports"]')));
    click('[data-menu-action="reports"]'); await ready();
    if (scope) { click(`[data-moderation-action="${scope}"]`); await ready(); }
  };
  const selectCase = async (id = report.id) => {
    await until("authoritative case row", () => visible(doc.querySelector(`[data-moderation-case="${id}"] [data-moderation-action="detail"]`)));
    click(`[data-moderation-case="${id}"] [data-moderation-action="detail"]`); await ready();
  };
  const parentCard = async () => {
    await until("fixture feed loaded", () => doc.querySelector('[data-status]')?.dataset.state === "online" && doc.querySelector('.post-card[data-offset="0"]:not([data-exiting])'));
    const active = () => doc.querySelector('.post-card[data-offset="0"]:not([data-exiting])');
    const count = Number(required("[data-count-label]").textContent);
    for (let index = 0; index < count && active()?.dataset.objectId !== object.id; index++) {
      const previous = active()?.dataset.objectId; click("[data-next]");
      await until("next Object", () => active()?.dataset.objectId !== previous && !active()?.inert);
    }
    check(active()?.dataset.objectId === object.id, "Reported Object is missing from discovery before restriction");
    return active();
  };
  const openReport = async () => {
    const card = await parentCard();
    const opener = required('[data-action="report"]', card);
    if (!visible(opener)) click(":scope > button", opener.closest(".action-popover"));
    click('[data-action="report"]', card); await ready(); return opener;
  };
  const commentProjections = async restricted => {
    // A one-row page must advance past the restricted oldest child, not return
    // an empty page that conceals its still-visible sibling.
    const replyIds = [], cursors = new Set();
    let cursor = null;
    do {
      const page = await rpc("babble.social.replies.list.v1", { object_id: object.id, cursor, limit: 1 });
      check(page.object_id === object.id && page.replies.length === 1, "Reply pagination returned an empty or oversized page");
      check(page.replies[0].object.signature && page.replies[0].edge.signature, "Reply projection lost signed records");
      replyIds.push(page.replies[0].object.id);
      cursor = page.next_cursor;
      check(cursor === null || !cursors.has(cursor), "Reply cursor repeated");
      cursors.add(cursor); check(replyIds.length <= 2, "Unexpected or duplicated reply page");
    } while (cursor !== null);
    equal(replyIds, restricted ? [sibling.id] : [child.id, sibling.id], "Restricted replies must be filtered before pagination");
    const quoteRest = await ok(`/objects/${object.id}/quotes?limit=20`, { account: null });
    const quoteRpc = await rpc("babble.social.quotes.list.v1", { object_id: object.id, cursor: null, limit: 20 });
    equal(quoteRpc, quoteRest, "REST and RPC quote projections diverged");
    const quote = quoteRest.quotes.find(item => item.edge.id === quoteEdge.id);
    check(quote, "Moderation deleted the quoted relationship");
    equal(quote.edge, quoteEdge, "Moderation changed the signed quote edge");
    equal(quote.object, restricted ? null : child, "Quoted target content was not redacted/restored");
    equal((await ok(`/objects/${child.id}`, { account: null })).object, child, "Restriction changed explicit signed child read");
    equal(await ok(`/graph/objects/${object.id}/incoming`, { account: null }), initialGraph, "Reply edge history changed");
    equal(await ok(`/graph/objects/${object.id}/outgoing`, { account: null }), initialOutgoing, "Quote edge history changed");

    await parentCard();
    const active = () => doc.querySelector(`.post-card[data-offset="0"][data-object-id="${object.id}"]:not([data-exiting])`);
    await until("authoritative comment and quote presentation", () => {
      const card = active(), thread = card?.querySelector(".conversation-status"), quotes = card?.querySelector(".quotes-status");
      check(thread?.dataset.state !== "error" && quotes?.dataset.state !== "error", "Reply or quote projection failed");
      const row = card?.querySelector(`.reply-row[data-object-id="${child.id}"]`);
      const siblingRow = card?.querySelector(`.reply-row[data-object-id="${sibling.id}"]`);
      const preview = card?.querySelector(`[data-quoted-object="${child.id}"]`);
      return thread?.dataset.state === "ready" && quotes?.dataset.state === "ready" && visible(siblingRow) && visible(preview)
        && (restricted ? !row && preview.dataset.kind === "unavailable" : visible(row) && preview.dataset.kind !== "unavailable");
    });
    const quoteNode = required(`[data-quoted-object="${child.id}"]`, active());
    check(restricted ? !quoteNode.textContent.includes(child.payload.text) : quoteNode.textContent.includes(child.payload.text), "Quote UI leaked or failed to restore child text");
    return { replyIds, quoteEdgeRetained: true, quoteContentRedacted: restricted, explicitChildRetained: true, renderedSiblingVisible: true };
  };
  const geometry = async state => {
    await ready();
    const dialog = required("[data-moderation-dialog]"), box = dialog.getBoundingClientRect(), css = win.getComputedStyle(dialog);
    const radii = [css.borderTopLeftRadius, css.borderTopRightRadius, css.borderBottomLeftRadius, css.borderBottomRightRadius].map(parseFloat);
    check(radii.every(radius => radius >= 8), `${state}: dialog corners are not rounded`);
    check(box.left >= 0 && box.top >= 0 && box.right <= win.innerWidth + 1 && box.bottom <= win.innerHeight + 1, `${state}: dialog escapes viewport`);
    check(dialog.scrollWidth <= dialog.clientWidth + 1, `${state}: dialog has horizontal overflow`);
    check(dialog.contains(doc.activeElement), `${state}: focus outside modal`);
    const outside = required("[data-toggle-profile]"); outside.focus();
    check(dialog.contains(doc.activeElement), `${state}: native modal allowed focus to escape`);
    const heading = required(".moderation-header h2").getBoundingClientRect(), closeBox = required('[data-moderation-action="close"]').getBoundingClientRect();
    check(heading.right <= closeBox.left + 1, `${state}: heading overlaps close control`);
    const controls = [...dialog.querySelectorAll("button,input,select,textarea")].filter(visible);
    for (const node of controls) {
      const rect = node.getBoundingClientRect();
      check(rect.width >= 43.9 && rect.height >= 43.9, `${state}: target smaller than 44px: ${node.outerHTML.slice(0, 100)}`);
      check(rect.left >= box.left - 1 && rect.right <= box.right + 1, `${state}: control escapes dialog`);
      check(node.scrollWidth <= node.clientWidth + 1, `${state}: control text overflows`);
    }
    const walker = doc.createTreeWalker(dialog, 4);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      if (!node.textContent.trim() || !visible(node.parentElement) || node.parentElement.closest("select,textarea")) continue;
      const range = doc.createRange(); range.selectNodeContents(node);
      const parent = node.parentElement.getBoundingClientRect();
      for (const rect of range.getClientRects()) check(rect.left >= Math.max(parent.left, box.left) - 1 && rect.right <= Math.min(parent.right, box.right) + 1, `${state}: text overflow`);
    }
    return { state, width: win.innerWidth, height: win.innerHeight, radii, controls: controls.length, scope: "Production iframe responsive layout; not physical touch E2E" };
  };
  const discovery = async (restricted) => {
    for (const lens of [null, { id: "babble.lens.stack.research.v1", weights: [{ lens: "Research", weight: 1 }] }, { id: "babble.lens.stack.weird.v1", weights: [{ lens: "Weird", weight: 1 }] }]) {
      const result = await ok("/discovery/candidates", { method: "POST", body: { anchors: [], search: marker, followed_objects: [], limit: 50, exploration_slots: 1, lens } });
      check(result.discovery.objects.some(item => item.id === object.id) === !restricted, `Discovery restriction mismatch: ${lens?.id ?? "default"}`);
    }
    const following = await ok(`/feed/following?limit=50&search=${encodeURIComponent(marker)}`);
    check(following.objects.some(item => item.id === object.id) === !restricted, "Following restriction mismatch");
    equal(await ok(`/objects/${object.id}`, { account: null }), initialObject, "Moderation changed the public signed Object");
    equal(await ok(`/graph/objects/${object.id}/incoming`, { account: null }), initialGraph, "Moderation wrote private data into the public graph");
  };
  const runtime = async restricted => {
    const prepare = await request("/runtime/surfaces/prepare", { method: "POST", body: { object_id: object.id, role: "Feed" } });
    const start = await request("/runtime/surfaces/sessions", { method: "POST", body: { object_id: object.id, role: "Feed", session_id: null } });
    if (restricted) {
      check(prepare.status === 409 && start.status === 409, `Restricted Surface prepare/start admitted: ${prepare.status}/${start.status}`);
      const heartbeat = await request(`/runtime/surfaces/sessions/${session.id}/heartbeat`, { method: "POST" });
      check([403, 409].includes(heartbeat.status), `Restricted active Surface can renew: ${heartbeat.status}`);
      const inspected = await ok(`/runtime/surfaces/sessions/${session.id}`);
      check(inspected.session.lifecycle === "evicted", "Restriction did not evict existing execution");
    } else {
      check(prepare.status === 200 && prepare.value.plan.admission === "ready" && start.status === 200, "Unrestricted runtime control failed");
      session = start.value.session;
      await ok(`/runtime/surfaces/sessions/${session.id}/heartbeat`, { method: "POST" });
    }
  };
  const layout = async width => {
    await load(reporter, width); await openReport();
    const reportGeometry = await geometry("report"); await close(); await inbox("mine");
    const inboxGeometry = await geometry("inbox"); await close();
    return { verified: true, measured: [reportGeometry, inboxGeometry] };
  };
  const actions = {
    async setup() {
      for (const [name, account] of Object.entries({ reporter, subject: accounts.subject, reviewer: accounts.reviewer, second: accounts.second, outsider: accounts.outsider })) {
        const access = await ok("/moderation/access", { account });
        check(access.actor_id === account.identity.id && access.can_review === ["reviewer", "second"].includes(name), `Reviewer configuration mismatch: ${name}`);
      }
      object = (await ok("/objects", { account: accounts.subject, method: "POST", body: { author_id: accounts.subject.identity.id, draft: {
        kind: seed.kind, schema: seed.schema, payload: { text: marker, metadata: {} }, resources: seed.resources, surfaces: seed.surfaces, capabilities: [],
        provenance: { parent: null, forked_from: null, remixed_from: [] },
      } } })).object;
      check(object.signature && object.author === accounts.subject.identity.id && object.surfaces.length, "Fixture must be a real signed Surface Object");
      initialObject = await ok(`/objects/${object.id}`, { account: null });
      child = (await ok("/objects/text", { account: accounts.subject, method: "POST", body: { author_id: accounts.subject.identity.id, text: `Comment on ${marker}` } })).object;
      const edge = await ok("/graph/edges", { account: accounts.subject, method: "POST", body: {
        author_id: accounts.subject.identity.id, source: child.id, target: object.id, relation: "reply_to", origin: "HumanAssertion",
      } });
      check(child.signature && edge.edge.signature, "Comment fixture must have signed Object and reply edge");
      sibling = (await ok("/objects/text", { account: accounts.subject, method: "POST", body: { author_id: accounts.subject.identity.id, text: `Unaffected sibling comment on ${marker}` } })).object;
      await ok("/graph/edges", { account: accounts.subject, method: "POST", body: {
        author_id: accounts.subject.identity.id, source: sibling.id, target: object.id, relation: "reply_to", origin: "HumanAssertion",
      } });
      quoteEdge = (await ok("/graph/edges", { account: accounts.subject, method: "POST", body: {
        author_id: accounts.subject.identity.id, source: object.id, target: child.id, relation: "quotes", origin: "HumanAssertion",
      } })).edge;
      initialGraph = await ok(`/graph/objects/${object.id}/incoming`, { account: null });
      initialOutgoing = await ok(`/graph/objects/${object.id}/outgoing`, { account: null });
      followBefore = await ok(`/social/following/${accounts.subject.identity.id}`);
      await ok(`/social/following/${accounts.subject.identity.id}`, { method: "PUT", body: { following: true, expected_revision: followBefore.revision, idempotency_key: key("follow") } });
      await discovery(false); await runtime(false); await load(reporter);
      return { verified: true, objectId: object.id, reporterId: reporter.identity.id, subjectId: accounts.subject.identity.id, reviewerIds: accounts.reviewerIds };
    },
    async cancel() {
      const before = await ok("/moderation/reports?scope=mine");
      const opener = await openReport(); fill('[data-moderation-field="details"]', "Cancelled report draft must not create any private record.");
      await close(); check(doc.activeElement === opener, "Report close must restore launcher focus");
      equal(await ok("/moderation/reports?scope=mine"), before, "Cancel created a report");
      return { verified: true, noWrite: true, focusRestored: true };
    },
    "layout-320": () => layout(320), "layout-390": () => layout(390), "layout-1280": () => layout(1280),
    async "comment-report"() {
      await load(reporter); await openReport(); await close();
      const selector = `.post-card[data-offset="0"][data-object-id="${object.id}"] .reply-row[data-object-id="${child.id}"] [data-action="report"]`;
      await until("child comment report launcher", () => visible(doc.querySelector(selector)));
      click(selector); await ready();
      fill('[data-moderation-field="reason"]', "other_integrity"); fill('[data-moderation-field="details"]', "Comment-level report must identify this child Object instead of its parent.");
      await submit(reporter, "report", child);
      await until("child report persisted", async () => (await ok("/moderation/reports?scope=mine")).items.some(item => item.object_id === child.id));
      const reports = (await ok("/moderation/reports?scope=mine")).items;
      childCase = reports.find(item => item.object_id === child.id);
      check(!reports.some(item => item.object_id === object.id), "Reply report incorrectly targeted the parent");
      return { verified: true, childObjectId: child.id, parentObjectId: object.id };
    },
    async report() {
      await load(reporter); await openReport();
      fill('[data-moderation-field="reason"]', "spam"); fill('[data-moderation-field="details"]', `Private reporter evidence ${marker}; repeated unsolicited promotion.`);
      await submit(reporter, "report", object);
      await until("UI report committed", async () => {
        const result = await ok("/moderation/reports?scope=mine"); report = result.items.find(item => item.object_id === object.id); return !!report;
      });
      check(report.status === "pending" && report.revision === 1 && report.reporter_id === reporter.identity.id, "Invalid initial report");
      await ready(); await discovery(false);
      return { verified: true, reportId: report.id, reportAloneDoesNotRestrict: true };
    },
    async privacy() {
      for (const path of ["/moderation/access", "/moderation/reports?scope=mine", "/moderation/reports?scope=queue", reportPath()]) await denied(path, { account: null }, 401);
      await denied("/moderation/reports", { account: null, method: "POST", body: { object_id: object.id, reason: "spam", details: "Unauthenticated private report must be rejected.", idempotency_key: key("guest") } }, 401);
      await denied("/moderation/reports?scope=queue", { account: reporter }, 403);
      for (const account of [accounts.subject, accounts.outsider]) {
        await denied(reportPath(), { account }, 404);
        check((await ok("/moderation/reports?scope=affected", { account })).items.length === 0, "Pending case leaked to unrelated/affected account");
      }
      await denied(`${reportPath()}/decisions`, { account: reporter, method: "POST", body: decision(report.revision) }, 403);
      await denied(`${reportPath()}/decisions`, { account: accounts.reviewer, method: "POST", body: decision(report.revision + 1) }, 409);
      equal(await detail(), report, "Denied access mutated case");
      return { verified: true, guestAuthentication: true, pendingPrivate: true, ordinaryAccountCannotReview: true, noStore: true };
    },
    async review() {
      const selfReport = await ok("/moderation/reports", { account: accounts.reviewer, method: "POST", body: { object_id: object.id, reason: "spam", details: "Reviewer self-report must not permit a self-review decision.", idempotency_key: key("self-report") } });
      await denied(`/moderation/reports/${selfReport.id}/decisions`, { account: accounts.reviewer, method: "POST", body: decision(selfReport.revision) }, 403);
      const ownObject = (await ok("/objects/text", { account: accounts.reviewer, method: "POST", body: { author_id: accounts.reviewer.identity.id, text: "Reviewer-owned integrity fixture" } })).object;
      const ownCase = await ok("/moderation/reports", { account: accounts.outsider, method: "POST", body: { object_id: ownObject.id, reason: "spam", details: "Reviewer must not decide a case about their own authored Object.", idempotency_key: key("own-object") } });
      await denied(`/moderation/reports/${ownCase.id}/decisions`, { account: accounts.reviewer, method: "POST", body: decision(ownCase.revision) }, 403);
      await load(accounts.reviewer, 320); await inbox("queue"); await selectCase();
      const measured = await geometry("review");
      check(required("[data-moderation-dialog]").textContent.includes(report.details), "Reviewer cannot inspect private evidence");
      fill('[data-moderation-field="outcome"]', "restrict"); fill('[data-moderation-field="reason"]', "spam");
      fill('[data-moderation-field="explanation"]', "Reviewed repeated unsolicited promotion; restrict distribution on this node.");
      await submit(accounts.reviewer, "decision", report);
      await until("review restriction committed", async () => { restrictedCase = await detail(); return restrictedCase.status === "decided"; });
      check(restrictedCase.decisions[0].reviewer_id === accounts.reviewer.identity.id && restrictedCase.decisions[0].outcome === "restrict", "Wrong reviewer/outcome");
      return { verified: true, realQueueDecision: true, selfReportReviewDenied: true, ownObjectReviewDenied: true, measured };
    },
    async restriction() {
      await discovery(true); await runtime(true);
      const affected = await detail(accounts.subject);
      check(affected.reporter_id === null && affected.reason === null && affected.details === null, "Affected author received reporter evidence");
      check(affected.decisions[0].outcome === "restrict", "Affected author missing decision");
      const measured = [];
      for (const width of [320, 390, 1280]) {
        await load(accounts.subject, width); await inbox("affected"); await selectCase();
        const text = required("[data-moderation-dialog]").textContent;
        check(!text.includes(report.details) && !text.includes(reporter.identity.id), "Author UI leaked reporter identity/evidence");
        check(text.includes(restrictedCase.decisions[0].explanation), "Author notice missing decision explanation");
        measured.push(await geometry("author notice and appeal"));
      }
      return { verified: true, discoveryExcluded: true, followingExcluded: true, newAndContinuedRuntimeDenied: true, signedObjectRetained: true, redactedAuthorNotice: true, measured };
    },
    async restart() {
      equal(await detail(), restrictedCase, "Restart lost moderation history");
      await discovery(true);
      const result = await request("/runtime/surfaces/prepare", { method: "POST", body: { object_id: object.id, role: "Feed" } });
      check(result.status === 409, "Restart lost effective restriction");
      return { verified: true, decisionAndEnforcementSurviveRealApiRestart: true };
    },
    async appeal() {
      await denied(`${reportPath()}/appeals`, { account: accounts.subject, method: "POST", body: { details: "Stale author appeal must not mutate the current case.", expected_revision: restrictedCase.revision - 1, idempotency_key: key("stale-appeal") } }, 409);
      await denied(`${reportPath()}/appeals`, { method: "POST", body: { details: "Reporter cannot appeal a restriction on another author's Object.", expected_revision: restrictedCase.revision, idempotency_key: key("wrong-party") } }, 409);
      fill('[data-moderation-field="details"]', `Private affected-author appeal ${marker}; this is an original test publication.`);
      await submit(accounts.subject, "appeal", restrictedCase);
      await until("author appeal committed", async () => (await detail(accounts.subject)).status === "appealed");
      await discovery(true);
      const appealed = await detail(accounts.subject);
      await denied(`${reportPath()}/decisions`, { account: accounts.reviewer, method: "POST", body: decision(appealed.revision, "no_action") }, 403);
      const reporterView = await detail();
      check(reporterView.appeal.details === null && reporterView.appeal.appellant_id === null, "Private author appeal leaked to reporter");
      await load(accounts.reviewer); await inbox("queue"); await selectCase();
      check(!visible(doc.querySelector('[data-moderation-action="submit-decision"]')), "Original reviewer can decide appeal in UI");
      return { verified: true, realAuthorAppeal: true, restrictionMaintained: true, originalReviewerDenied: true, appealPrivate: true };
    },
    async reversal() {
      await load(accounts.second); await inbox("queue"); await selectCase();
      fill('[data-moderation-field="outcome"]', "no_action"); fill('[data-moderation-field="reason"]', "other_integrity");
      fill('[data-moderation-field="explanation"]', "Independent review of the author appeal finds no actionable integrity violation.");
      await submit(accounts.second, "decision", await detail(accounts.second));
      await until("independent reversal committed", async () => (await detail()).status === "closed");
      const closed = await detail();
      check(closed.decisions.length === 2 && closed.decisions[1].reviewer_id === accounts.second.identity.id && closed.decisions[1].outcome === "no_action", "Appeal not independently reversed");
      await discovery(false); await runtime(false);
      await load(accounts.subject); await inbox("affected"); await selectCase();
      check(required("[data-moderation-dialog]").textContent.includes(closed.decisions[1].explanation), "Author missing reversal notice");
      return { verified: true, independentReviewerReversal: true, discoveryAndFollowingRestored: true, runtimeRestored: true };
    },
    async receipts() {
      const current = await detail();
      const mainWrites = captures.filter(item => item.path === reportPath() + "/decisions" || item.path === reportPath() + "/appeals" || item.path === "/moderation/reports" && item.value.id === report.id);
      check(mainWrites.length === 4, `Expected four real UI mutations, got ${mainWrites.length}`);
      for (const captured of mainWrites) {
        const account = [reporter, ...Object.values(accounts)].find(item => item?.identity?.id === captured.actor);
        const retry = await ok(captured.path, { account, method: "POST", body: captured.body });
        equal(retry, captured.value, "Exact UI retry did not return original receipt");
        const field = Object.hasOwn(captured.body, "explanation") ? "explanation" : "details";
        await denied(captured.path, { account, method: "POST", body: { ...captured.body, [field]: `${captured.body[field]} Changed intent.` } }, 409);
        if (captured.body.expected_revision !== undefined) await denied(captured.path, { account, method: "POST", body: { ...captured.body, idempotency_key: key("stale-cas") } }, 409);
      }
      equal(await detail(), current, "Old retry or stale CAS changed final case"); await discovery(false);
      return { verified: true, realUiMutations: mainWrites.length, exactRetry: true, changedIntentConflict: true, staleCASConflict: true, oldRestrictionNotResurrected: true };
    },
    async "comment-restrict"() {
      await load(accounts.reviewer);
      await commentProjections(false);
      await inbox("queue"); await selectCase(childCase.id);
      fill('[data-moderation-field="outcome"]', "restrict"); fill('[data-moderation-field="reason"]', "other_integrity");
      fill('[data-moderation-field="explanation"]', "Restrict this child comment after reviewing its integrity report; retain signed history.");
      await submit(accounts.reviewer, "decision", childCase);
      await until("child restriction persisted", async () => {
        childCase = await ok(`/moderation/reports/${childCase.id}`);
        return childCase.status === "decided" && childCase.decisions[0]?.outcome === "restrict";
      });
      check(childCase.object_id === child.id && childCase.decisions[0].reviewer_id === accounts.reviewer.identity.id, "Child decision targeted another Object/reviewer");
      await ready(); await close();
      const projection = await commentProjections(true);
      await discovery(false);
      return { verified: true, childObjectId: child.id, parentUnrestricted: true, samePageCacheInvalidation: true, ...projection };
    },
    async "comment-appeal"() {
      await load(accounts.subject); await inbox("affected"); await selectCase(childCase.id);
      const authorView = await ok(`/moderation/reports/${childCase.id}`, { account: accounts.subject });
      check(authorView.details === null && authorView.reporter_id === null, "Child author notice leaked report evidence");
      check(required("[data-moderation-dialog]").textContent.includes(childCase.decisions[0].explanation), "Child author notice omitted explanation");
      fill('[data-moderation-field="details"]', "Please independently review this original child comment and restore its normal projection.");
      await submit(accounts.subject, "appeal", authorView);
      await until("child appeal persisted", async () => (await ok(`/moderation/reports/${childCase.id}`, { account: accounts.subject })).status === "appealed");
      await ready(); await close();
      await commentProjections(true);
      return { verified: true, realChildAuthorAppeal: true, restrictedDuringAppeal: true };
    },
    async "comment-reversal"() {
      await load(accounts.second);
      await commentProjections(true);
      await inbox("queue"); await selectCase(childCase.id);
      fill('[data-moderation-field="outcome"]', "no_action"); fill('[data-moderation-field="reason"]', "other_integrity");
      fill('[data-moderation-field="explanation"]', "Independent review accepts the child author's appeal and restores normal comment visibility.");
      await submit(accounts.second, "decision", await ok(`/moderation/reports/${childCase.id}`, { account: accounts.second }));
      await until("child reversal persisted", async () => {
        childCase = await ok(`/moderation/reports/${childCase.id}`);
        return childCase.status === "closed";
      });
      check(childCase.decisions.length === 2 && childCase.decisions[1].reviewer_id === accounts.second.identity.id && childCase.decisions[1].outcome === "no_action", "Child reversal was not independent");
      await ready(); await close();
      const projection = await commentProjections(false);
      const writes = captures.filter(item => item.value?.id === childCase.id);
      check(writes.length === 4, "Child workflow must contain four real UI mutations");
      const restrictionWrite = writes.find(item => item.body.outcome === "restrict");
      equal(await ok(restrictionWrite.path, { account: accounts.reviewer, method: "POST", body: restrictionWrite.body }), restrictionWrite.value, "Child restriction exact retry lost its receipt");
      equal(await ok(`/moderation/reports/${childCase.id}`), childCase, "Old child restriction retry resurrected restriction");
      await commentProjections(false);
      return { verified: true, independentReviewerReversal: true, realUiMutations: writes.length, samePageCacheInvalidation: true, oldRestrictionNotResurrected: true, ...projection };
    },
    async cleanup() {
      for (const item of frames) item.remove(); frames.clear(); restoreStorage();
      if (followBefore) {
        const path = `/social/following/${accounts.subject.identity.id}`, current = await ok(path);
        await ok(path, { method: "PUT", body: { following: followBefore.following, expected_revision: current.revision, idempotency_key: key("restore-follow") } });
      }
      for (const account of [accounts.subject, accounts.reviewer, accounts.second, accounts.outsider]) await ok("/auth/session", { account, method: "DELETE" });
      equal(sessionStorage.getItem(sessionKey), savedSession, "Reporter session changed");
      check((await ok("/auth/session")).identity.id === reporter.identity.id, "Reporter login invalidated");
      return { verified: true, reporterPreserved: true, roleFramesRemoved: true, temporarySessionsRevoked: true, storeCleanup: "live-stack finally stops processes and removes its temporary store" };
    },
  };
  return { get busy() { return busy; }, async run(stage) {
    check(!busy, "Moderation stage still running; refusing concurrent cleanup"); check(Object.hasOwn(actions, stage), `Unknown stage: ${stage}`);
    busy = true; try { return await actions[stage](); } finally { busy = false; }
  } };
}

// Scripted probes test orchestration failures only, never feature acceptance.
if (process.argv[2] === "--contract-check") {
  const { Script, createContext, runInContext } = await import("node:vm");
  const apiUrl = "http://127.0.0.1:18787", actor = `id_${"a".repeat(64)}`, caseId = `report_${"b".repeat(64)}`;
  const objectId = `obj_${"c".repeat(64)}`, otherActor = `id_${"d".repeat(64)}`;
  const reportIntent = { object_id: objectId, reason: "spam", details: "Original private report evidence." };
  const reportValue = { id: caseId, object_id: objectId, reporter_id: actor, ...reportIntent, revision: 1, status: "pending" };
  const options = { apiUrl, actor, since: 4, path: "/moderation/reports", expected: reportIntent };
  const record = (path, body, value) => ({ sequence: 5, url: apiUrl + path, method: "POST", requestBody: JSON.stringify({ ...body, idempotency_key: "real-ui-key" }), responseBody: JSON.stringify(value), status: 200 });
  const observed = { cursor: 5, requests: [record(options.path, reportIntent, reportValue)] };
  const read = (batch = observed, settings = options) => readModerationMutation(batch, settings);
  assert.deepEqual(read(), { actor, path: options.path, body: { ...reportIntent, idempotency_key: "real-ui-key" }, value: reportValue });
  assert.equal(read({ cursor: 4, requests: [] }), null);
  for (const pending of [{ status: null }, { responseBody: null }]) assert.equal(read({ cursor: 5, requests: [{ ...observed.requests[0], ...pending }] }), null);
  assert.throws(() => read({ cursor: 5, requests: [...observed.requests, ...observed.requests] }), /duplicate/);
  assert.throws(() => read({ ...observed, cursor: 3 }), /batch/);
  for (const change of [
    { sequence: 4 }, { sequence: 6 }, { error: "connection failed" }, { status: 409 }, { requestBody: "{" }, { responseBody: "{" },
    { requestBody: JSON.stringify(reportIntent) },
    { requestBody: JSON.stringify({ ...reportIntent, idempotency_key: "key", details: "Different intent" }) },
    { requestBody: JSON.stringify({ ...reportIntent, idempotency_key: "key", extra: true }) },
    ...[{ reporter_id: otherActor }, { object_id: `obj_${"f".repeat(64)}` }, { status: "decided" }, { revision: 2 }].map(change => ({ responseBody: JSON.stringify({ ...reportValue, ...change }) })),
  ]) assert.throws(() => read({ cursor: 5, requests: [{ ...observed.requests[0], ...change }] }));
  for (const kind of ["decision", "appeal"]) {
    const path = `/moderation/reports/${caseId}/${kind === "decision" ? "decisions" : "appeals"}`;
    const expected = kind === "decision"
      ? { outcome: "restrict", reason: "spam", explanation: "Actual submitted review explanation.", policy_version: "babble.integrity.v1", source_signals: [], expected_revision: 1 }
      : { details: "Private appeal evidence must stay redacted to reporter.", expected_revision: 2 };
    const value = { id: caseId, revision: expected.expected_revision + 1, status: kind === "decision" ? "decided" : "appealed",
      ...(kind === "decision" ? { decisions: [{ ...expected, reviewer_id: actor }] } : { reporter_id: null, reason: null, details: null, appeal: { appellant_id: actor, details: expected.details } }) };
    const batch = { cursor: 5, requests: [record(path, expected, value)] }, settings = { ...options, path, expected };
    assert.deepEqual(read(batch, settings).value, value, "Keep original actor-redacted response unchanged");
    for (const invalid of [{ ...value, id: `report_${"e".repeat(64)}` }, { ...value, revision: value.revision + 1 },
      kind === "decision" ? { ...value, decisions: [{ ...value.decisions[0], reviewer_id: otherActor }] } : { ...value, appeal: { ...value.appeal, appellant_id: otherActor } },
      kind === "decision" ? { ...value, decisions: [{ ...value.decisions[0], outcome: "no_action" }] } : { ...value, appeal: { ...value.appeal, details: "Changed" } },
    ]) assert.throws(() => read({ cursor: 5, requests: [record(path, expected, invalid)] }, settings));
  }
  const priorFreeze = process.env.BABBLE_MODERATION_SOURCE_FROZEN;
  try {
    delete process.env.BABBLE_MODERATION_SOURCE_FROZEN;
    await assert.rejects(assertModerationIsolation({ apiPort: 18787, gatewayPort: 18788, frontendPort: 14329, aegisAddr: "127.0.0.1:17878" }), /source freeze required/);
    process.env.BABBLE_MODERATION_SOURCE_FROZEN = "1";
    await assert.rejects(assertModerationIsolation({ apiPort: 8787, gatewayPort: 8788, frontendPort: 4321, aegisAddr: "127.0.0.1:17878" }), /Disposable test ports/);
    await assert.rejects(assertModerationIsolation({ apiPort: 18787, gatewayPort: 18788, frontendPort: 14329, aegisAddr: "127.0.0.1:7878" }), /Acceptance Aegis address/);
  } finally {
    if (priorFreeze === undefined) delete process.env.BABBLE_MODERATION_SOURCE_FROZEN;
    else process.env.BABBLE_MODERATION_SOURCE_FROZEN = priorFreeze;
  }
  for (const failed of [...stages, "cleanup"]) {
    const calls = [], context = createContext({ window: {} });
    const harness = { busy: false, async run(stage) { calls.push(stage); if (stage === failed) throw Error(`injected:${stage}`); return { verified: true }; } };
    const execute = async commands => {
      assert.equal(commands.length, 1); const { code } = commands[0]; new Script(code);
      if (code.includes("window.__moderationHarness = (function")) { context.window.__moderationHarness = harness; return { results: [{ ok: true, value: { installed: true } }] }; }
      const value = runInContext(code, context); assert.equal(typeof value, "object"); return { results: [{ ok: true, value }] };
    };
    const waitFor = async (code, predicate) => {
      for (let i = 0; i < 20; i++) { await new Promise(resolve => setImmediate(resolve)); const value = runInContext(code, context); if (predicate(value)) return value; }
      throw Error("Probe did not settle");
    };
    await assert.rejects(verifyModeration({ execute, waitFor, readRequests: async () => ({ cursor: 0, requests: [] }), apiUrl: "http://127.0.0.1:18787", accounts: {}, seed: {}, restart: async () => {}, log: () => {} }), error => error.message.includes(`injected:${failed}`));
    assert.equal(calls.at(-1), "cleanup"); assert.equal(context.window.__moderationHarness, undefined);
  }
  for (const failure of [null, "read-failed", "malformed-result"]) {
    const calls = [], floors = [], context = createContext({ window: {} });
    const harness = { busy: false, async run(stage) {
      calls.push(stage);
      if (stage === "report") {
        for (const since of [0, 4]) {
          const pending = { id: since + 1, since, path: options.path, response: null };
          context.window.__moderationNetworkRead = pending;
          for (let i = 0; pending.response === null && i < 20; i++) await new Promise(resolve => setImmediate(resolve));
          assert.ok(pending.response, "Observer mailbox was not serviced");
          delete context.window.__moderationNetworkRead;
          if (pending.response.error) throw Error(pending.response.error);
          assert.equal(pending.response.requests.length, 1, "RPC and other moderation paths must not enter capture");
          assert.equal(pending.response.requests[0].url, apiUrl + options.path);
        }
      }
      return { verified: true };
    } };
    const execute = async ([{ code }]) => {
      new Script(code);
      if (code.includes("window.__moderationHarness = (function")) { context.window.__moderationHarness = harness; return { results: [{ ok: true, value: { installed: true } }] }; }
      return { results: [{ ok: true, value: runInContext(code, context) }] };
    };
    const waitFor = async (code, predicate) => {
      for (let i = 0; i < 30; i++) { await new Promise(resolve => setImmediate(resolve)); const value = runInContext(code, context); if (predicate(value)) return value; }
      throw Error("Observer orchestration did not settle");
    };
    const readRequests = async ({ since }) => {
      floors.push(since);
      if (failure === "read-failed") throw Error("Injected observer error");
      if (failure === "malformed-result") return { cursor: -1, requests: null };
      return { cursor: 5, requests: [observed.requests[0], { ...observed.requests[0], url: apiUrl + "/rpc" }, { ...observed.requests[0], url: apiUrl + "/moderation/other" }] };
    };
    const result = verifyModeration({ execute, waitFor, readRequests, apiUrl, accounts: {}, seed: {}, restart: async () => {}, log: () => {} });
    if (failure) await assert.rejects(result, /HTTP observer failed/);
    else { await result; assert.deepEqual(floors, [0, 4]); }
    assert.equal(calls.at(-1), "cleanup");
    for (const name of ["__moderationHarness", "__moderationResult", "__moderationStep", "__moderationNetworkRead"]) assert.equal(context.window[name], undefined);
  }
  console.log("Moderation harness negative orchestration PASS (no browser/API evidence)");
}
