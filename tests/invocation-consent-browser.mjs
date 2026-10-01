import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";

/**
 * Parent owns live-stack, builds, Aegis navigation and account teardown. Requires
 * the online production page, its isolated browser-author-* account, a working
 * bundle gateway, and sdk/dist built from the integrated v2 SDK. No server starts
 * here. Fixture Objects live only in the parent's disposable store.
 *
 * Covers actual SDK -> main -> host prompt -> API -> durable social effects.
 * Geometry uses snapshots of the actual prompt and production CSS in three real
 * layout viewports; it does not claim three end-to-end viewport executions.
 * Same-actor/different-login, restart, provider failures and media are not covered.
 */
export async function verifyInvocationConsent(execute, waitFor, { apiUrl, readSurfaceDocument }) {
  const api = new URL(apiUrl);
  assert.ok(["127.0.0.1", "localhost", "[::1]"].includes(api.hostname), "isolated loopback API required");
  assert.equal(typeof readSurfaceDocument, "function", "disposable registration observer required");
  const marker = `Invocation acceptance ${randomUUID()}`;
  const evaluate = async code => {
    const response = await execute([{ type: "eval", code }]);
    assert.equal(response.results?.[0]?.ok, true, "invocation acceptance Aegis evaluation failed");
    return response.results[0].value;
  };
  const state = "window.__invocationAcceptance?.snapshot()";
  const until = async predicate => {
    const value = await waitFor(state, value => value && (value.failure || predicate(value)));
    assert.equal(value.failure, null, `invocation acceptance: ${JSON.stringify(value.failure)}`);
    return value;
  };
  const task = async (fn, ...args) => {
    await evaluate(`window.__invocationAcceptance.task(${fn.toString()}, ${JSON.stringify(args)}); true`);
    const value = await until(value => value.taskDone);
    return value.taskResult;
  };
  const keys = new Map();
  const send = (id, method, text, timeoutMs = 30000, key = id) => {
    if (!keys.has(key)) keys.set(key, randomUUID());
    return task((s, input) => s.send(input), { id, method, text, timeoutMs, key: keys.get(key) });
  };
  const result = async id => (await until(value => value.results[id])).results[id];
  const prompt = async (id, method, text) => {
    const value = await until(value => value.prompt || value.results[id]);
    assert.equal(value.results[id], undefined, `${id}: SDK settled without an actual host prompt`);
    assert.equal(value.prompt.count, 1);
    assert.equal(value.prompt.hostOwned, true);
    assert.equal(value.prompt.modal, true);
    assert.equal(value.prompt.denyFocused, true, "initial focus must not approve a sensitive action");
    assert.equal(value.phase, "active", "prompt must preserve the active Surface");
    assert.ok(value.prompt.text.includes(value.actorId));
    assert.ok(value.prompt.text.includes(value.objectId));
    assert.ok(value.prompt.text.includes(value.targetId));
    if (text !== undefined) assert.equal(value.prompt.preview, text, "preview must show exact literal text");
    assert.equal(value.prompt.injectedMarkup, false);
    const prepared = await task((s, id) => s.prepare(id), id);
    assert.equal(prepared.status, 200);
    const view = prepared.body;
    assert.equal(view.state.kind, "pending");
    assert.equal(view.method, `babel.social.${method}.v2`);
    assert.equal(view.actor_id, value.actorId);
    assert.equal(view.object_id, value.objectId);
    assert.deepEqual(view.origin, { kind: "surface", session_id: value.sessionId, document_id: value.documentId });
    assert.equal(view.payload.target_object_id, value.targetId);
    if (text !== undefined) assert.equal(view.payload.text, text);
    assert.ok(Date.parse(view.deadline) > Date.parse(view.created_at));
    assert.equal(view.result, null);
    return view;
  };
  const decide = selector => evaluate(`document.querySelector(${JSON.stringify(selector)}).click(); true`);
  const inspect = () => task(s => s.effects());
  const noEffect = async before => assert.deepEqual(await inspect(), before, "non-approved invocation must not write an Object or edge");
  const terminal = async (view, kinds) => {
    const status = await task((s, id) => s.status(id), view.invocation_id);
    assert.equal(status.status, 200);
    assert.ok(kinds.includes(status.body.state.kind), JSON.stringify(status.body.state));
    assert.equal(status.body.result, null);
    return status.body.state.kind;
  };

  // Include the real prebuilt SDK modules as signed bundle assets. A stale SDK
  // fails this prerequisite instead of quietly falling back to handwritten RPC.
  const sdkPaths = ["sdk.js", "client.js", "bridge.js", "transport.js", "channel.js", "lifecycle.js", "generated/protocol.js"];
  const files = await Promise.all(sdkPaths.map(async path => ({
    path: `sdk/${path}`, content: await readFile(new URL(`../sdk/dist/${path}`, import.meta.url), "utf8"),
  })));
  assert.match(files.find(file => file.path === "sdk/sdk.js").content, /babel\.social\.reply\.v2/,
    "parent must build the integrated v2 SDK before browser acceptance");
  let installed = false;
  const evidence = { operations: [], geometry: [], remaining: ["same-actor different login", "restart", "media", "real gesture input at each viewport"] };
  try {
    await waitFor("({ online: document.querySelector('[data-status]')?.dataset.state })", value => value.online === "online");
    await evaluate(`(${install.toString()})(${JSON.stringify({ apiUrl: api.href, marker })}); true`);
    installed = true;
    await until(value => value.ready);
    const setup = await task(async (s, files, application) => s.publish(files, application), files, application.toString());
    assert.equal(setup.signature.algorithm, "Ed25519");
    assert.match(setup.signature.bytes, /^[0-9a-f]{128}$/);
    assert.equal(setup.bundle.version, 1);
    assert.deepEqual(setup.capabilities.map(item => item.id).sort(),
      ["babel.social.follow", "babel.social.reply", "babel.social.share", "babel.social.unfollow"]);
    assert.deepEqual(await task(s => s.grants()), []);
    await evaluate(`(() => {
      const search = document.querySelector('[data-search-input]');
      search.value = ${JSON.stringify(marker)}; search.form.requestSubmit(); return true;
    })()`);
    await waitFor("({ id: document.querySelector('.post-card[data-offset=\"0\"]')?.dataset.objectId })", value => value.id === setup.objectId);
    await evaluate("document.querySelector('.post-card[data-offset=\"0\"] [data-action=\"surface\"]').click(); true");
    const mounted = await until(value => value.phase === "active");
    assert.notEqual(new URL(mounted.src).origin, api.origin);
    assert.match(new URL(mounted.src).hostname, /^m-[0-9a-f]{48}\.localhost$/);
    assert.deepEqual(mounted.sandbox, ["allow-same-origin", "allow-scripts"]);
    const binding = await readSurfaceDocument({ actorId: mounted.actorId, objectId: setup.objectId });
    const admitted = await task((s, binding) => s.connect(binding), binding);
    assert.equal(admitted.plan.admission, "ready");
    assert.equal(admitted.plan.object_id, setup.objectId);
    for (const decision of admitted.plan.capability_decisions) {
      assert.equal(decision.status, "requires_user");
      assert.equal(decision.grant ?? null, null);
    }
    await until(value => value.childReady);
    let effects = await inspect();
    assert.deepEqual(effects, { objects: [], edges: [] });

    const literal = `${marker} <b data-invocation-injected>literal reply</b>\nSecond line & exact spacing.`;
    await send("reply", "reply", literal);
    const reply = await prompt("reply", "reply", literal);
    await noEffect(effects);
    const isolated = await task((s, id) => s.isolation(id), reply.invocation_id);
    assert.deepEqual(isolated, { anonymous: 401, otherActor: 403, wrongDocument: 403 });
    const frozen = await task((s, id) => s.prepare(id), "reply");
    assert.deepEqual(frozen.body, reply, "exact prepare retry must preserve challenge and deadline");
    await task(s => { s.port.postMessage({ type: "mutate", id: "reply", text: "Changed after prompting" }); return true; });
    await until(value => value.mutated === "reply");
    const unchanged = await evaluate(state);
    assert.equal(unchanged.prompt.preview, literal, "child input mutation must not change the approved preview");
    const conflict = await task(s => s.prepare("reply", { text: "Changed after prompting" }));
    assert.equal(conflict.status, 409, "changed intent under an existing key must conflict");
    evidence.geometry = await task(measureGeometry, [320, 390, 1280]);
    for (const measurement of evidence.geometry) assert.deepEqual(measurement.errors, [], JSON.stringify(measurement));
    const interaction = await task(checkInterference);
    assert.equal(interaction.bubbled, 0, "prompt keyboard and pointer events must not reach feed handlers");
    assert.equal(interaction.sameCard, true);
    assert.equal(interaction.sameFrame, true);
    assert.equal(interaction.promptOpen, true);
    await decide("[data-invocation-allow]");
    const approved = await result("reply");
    assert.equal(approved.error, null);
    assert.equal(approved.result.object.payload.text, literal);
    assert.equal(approved.result.object.author, mounted.actorId);
    assert.equal(approved.result.edge.target, mounted.targetId);
    assert.equal(approved.result.edge.source, approved.result.object.id);
    assert.equal(approved.result.edge.relation, "reply_to");
    effects = await inspect();
    assert.deepEqual(effects.objects, [approved.result.object.id]);
    assert.deepEqual(effects.edges, [approved.result.edge.id]);
    const completed = await task((s, id) => s.status(id), reply.invocation_id);
    assert.equal(completed.body.state.kind, "completed");
    assert.deepEqual(completed.body.result, approved.result);
    await send("reply-retry", "reply", literal, 30000, "reply");
    const retried = await until(value => value.results["reply-retry"] || value.prompt);
    assert.equal(retried.prompt, null, "committed exact retry must not prompt again");
    assert.deepEqual(retried.results["reply-retry"], approved);
    await noEffect(effects);
    evidence.operations.push({ method: "reply", invocationId: reply.invocation_id, objectId: approved.result.object.id,
      edgeId: approved.result.edge.id, exactRetry: true });

    for (const method of ["share", "follow", "unfollow"]) {
      const text = method === "share" ? `${marker} shared exact text` : undefined;
      await send(method, method, text);
      const view = await prompt(method, method, text);
      await noEffect(effects);
      await decide("[data-invocation-allow]");
      const output = await result(method);
      assert.equal(output.error, null);
      assert.equal(output.result.edge.target, mounted.targetId);
      assert.equal(output.result.edge.author, mounted.actorId);
      assert.deepEqual(output.result.edge.relation,
        method === "share" ? "quotes" : method === "follow" ? "follows" : { custom: "unfollows" });
      assert.equal(output.result.edge.source, output.result.object?.id ?? mounted.objectId);
      const next = await inspect();
      assert.deepEqual(next.edges, [...effects.edges, output.result.edge.id].sort(), `${method}: exactly one edge`);
      assert.deepEqual(next.objects, [...effects.objects, ...(output.result.object ? [output.result.object.id] : [])].sort());
      if (text !== undefined) assert.equal(output.result.object.payload.text, text);
      else assert.equal(output.result.object, undefined);
      await send(`${method}-retry`, method, text, 30000, method);
      const retry = await until(value => value.results[`${method}-retry`] || value.prompt);
      assert.equal(retry.prompt, null, `${method}: exact retry must not prompt`);
      assert.deepEqual(retry.results[`${method}-retry`], output);
      effects = next;
      await noEffect(effects);
      evidence.operations.push({ method, invocationId: view.invocation_id, edgeId: output.result.edge.id, exactRetry: true });
    }
    for (const [id, selector, kinds] of [
      ["denied", "[data-invocation-deny]", ["denied"]],
      ["cancelled", "[data-invocation-prompt] [aria-label=\"Cancel request\"]", ["cancelled"]],
    ]) {
      await send(id, "reply", `${marker} ${id}`);
      const view = await prompt(id, "reply", `${marker} ${id}`);
      await decide(selector);
      assert.ok((await result(id)).error, `${id}: SDK must reject`);
      assert.equal((await result(id)).result, null);
      const kind = await terminal(view, kinds);
      const late = await task((s, id) => s.mutateInvocation(id, "execute"), view.invocation_id);
      assert.ok(late.status >= 400, `${id}: execute must reject`);
      await send(`${id}-retry`, "reply", `${marker} ${id}`, 30000, id);
      const retry = await until(value => value.results[`${id}-retry`] || value.prompt);
      assert.equal(retry.prompt, null, `${id}: exact retry must not resurrect a terminal prompt`);
      assert.ok(retry.results[`${id}-retry`].error);
      await noEffect(effects);
      evidence.operations.push({ method: "reply", outcome: kind });
    }
    await send("expired", "reply", `${marker} expired`, 3000);
    const expiring = await prompt("expired", "reply", `${marker} expired`);
    await until(value => !value.prompt && value.results.expired);
    assert.ok((await result("expired")).error);
    await terminal(expiring, ["expired", "cancelled"]);
    await noEffect(effects);
    evidence.operations.push({ method: "reply", outcome: "deadline-no-write" });

    await send("closing", "reply", `${marker} closing`);
    const closing = await prompt("closing", "reply", `${marker} closing`);
    await evaluate("document.querySelector('[data-close-surface]').click(); true");
    await until(value => !value.frame && !value.prompt);
    await task(s => s.waitForEviction());
    const late = await task((s, id) => s.mutateInvocation(id, "execute"), closing.invocation_id);
    assert.ok(late.status >= 400, "evicted Surface must not execute a pending action");
    await noEffect(effects);
    assert.deepEqual(await task(s => s.grants()), [], "allow-once must never create a durable grant");
    evidence.operations.push({ method: "reply", outcome: "close-no-write" });
    evidence.effects = effects;
  } finally {
    if (installed) {
      const cleanup = await task(s => s.cleanup());
      await evaluate("delete window.__invocationAcceptance; true");
      assert.deepEqual(cleanup.errors, [], "owned fixture session, observer, account and port cleanup");
      assert.equal(cleanup.parentAccountPreserved, true);
    }
  }
  console.log("Invocation consent browser PASS", JSON.stringify(evidence));
  return evidence;
}

// Serialized into Aegis. Tokens/passwords stay in closures and never snapshots.
function install({ apiUrl, marker }) {
  if (window.__invocationAcceptance) throw new Error("invocation test already active");
  const api = new URL(apiUrl);
  if (new URL(document.documentElement.dataset.babelApi).origin !== api.origin) throw new Error("fixture API mismatch");
  const saved = sessionStorage.getItem(`babel.session.v1:${api.origin}`);
  const session = JSON.parse(saved ?? "null");
  if (!session?.token || !/^browser-author-/.test(session.identity?.handle ?? "")) throw new Error("isolated live-stack fixture account required");
  const originalSearch = document.querySelector('[data-search-input]').value;
  const nativeFetch = window.fetch.bind(window);
  let client, other, lastSession, documentId;
  const requests = new Map(), geometryFrames = new Set();
  const s = window.__invocationAcceptance = { ready: false, failure: null, taskDone: false, taskResult: null,
    results: {}, childReady: false, mutated: null, marker, actorId: session.identity.id, objectId: null, targetId: null, port: null };
  const fail = error => ({ name: error?.name ?? "Error", message: error?.message ?? "operation failed" });
  const request = async (path, { method = "GET", body, token = session.token, document = null } = {}) => {
    const headers = { "content-type": "application/json" };
    if (token) headers.authorization = `Bearer ${token}`;
    if (document) headers["x-babel-surface-document"] = document;
    const response = await nativeFetch(new URL(path, api), { method, headers, credentials: "omit", redirect: "error",
      ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal: AbortSignal.timeout(15000) });
    return { status: response.status, body: response.status === 204 ? null : await response.json() };
  };
  const get = async path => {
    const response = await request(path);
    if (response.status !== 200) throw new Error(`fixture read HTTP ${response.status}: ${path}`);
    return response.body;
  };
  const rpc = async (method, payload) => {
    const response = await request("/rpc", { method: "POST", body: {
      protocol: "babel.rpc.v1", id: crypto.randomUUID(), method, payload,
      binding: { object_id: null, surface_session_id: null, runtime_id: "invocation-acceptance", origin: location.origin, capability_grants: [] },
      idempotency_key: crypto.randomUUID(), deadline: { timeout_ms: 30000, client_started_at: new Date().toISOString() }, trace_id: null,
    } });
    if (response.status !== 200 || response.body.error) throw new Error(`fixture RPC ${method}: ${response.status}/${response.body.error?.code}`);
    return response.body.result;
  };
  s.task = (fn, args) => {
    s.taskDone = false; s.taskResult = null; s.failure = null;
    Promise.resolve().then(() => fn(s, ...args)).then(value => {
      s.taskResult = value ?? null; s.taskDone = true;
    }, error => { s.failure = fail(error); s.taskDone = true; });
  };
  s.snapshot = () => {
    const frame = document.querySelector('[data-surface-host] iframe');
    const dialog = document.querySelector('[data-invocation-prompt]');
    return { ready: s.ready, failure: s.failure, taskDone: s.taskDone, taskResult: s.taskResult,
      actorId: s.actorId, objectId: s.objectId, targetId: s.targetId, sessionId: lastSession, documentId,
      phase: document.querySelector('[data-surface-host]')?.dataset.state, frame: !!frame,
      src: frame?.src, sandbox: frame ? [...frame.sandbox].sort() : [], childReady: s.childReady,
      results: s.results, mutated: s.mutated, prompt: dialog ? {
        count: document.querySelectorAll('[data-invocation-prompt]').length, modal: dialog.matches(':modal'),
        hostOwned: dialog.ownerDocument === document && !!dialog.closest('[data-surface-panel]'),
        text: dialog.textContent, preview: dialog.querySelector('[aria-label="Exact text to publish"]')?.textContent,
        denyFocused: document.activeElement?.matches('[data-invocation-deny]'),
        injectedMarkup: !!dialog.querySelector('[data-invocation-injected]'),
      } : null };
  };
  s.publish = async (files, source) => {
    const { publishBundle } = await import('/src/app/bundle-publication.ts');
    const target = await client.publishText(s.actorId, `${marker} recipient`);
    s.targetId = target.id;
    const capabilities = ["follow", "unfollow", "share", "reply"].map(method => ({
      id: `babel.social.${method}`, version: 1, scope: { object_id: target.id },
    }));
    const assets = [
      { path: "index.html", content: '<!doctype html><html><head><meta charset="utf-8"><title>Consent acceptance</title></head><body><h1>Consent acceptance</h1><output id="status">Connecting</output><script type="module" src="./app.js"></script></body></html>' },
      { path: "app.js", content: `import { createSurfaceSDK } from './sdk/sdk.js';\nimport { connectSurfaceBridge } from './sdk/channel.js';\n(${source})(${JSON.stringify({ origin: location.origin, marker })}, createSurfaceSDK, connectSurfaceBridge);` }, ...files,
    ];
    const object = await publishBundle(client, s.actorId, marker, {
      files: assets.map(asset => {
        const file = new File([asset.content], asset.path.split('/').at(-1), { type: asset.path.endsWith('.html') ? 'text/html' : 'text/javascript' });
        Object.defineProperty(file, 'webkitRelativePath', { value: `consent-app/${asset.path}` });
        return file;
      }), entryPath: "index.html", capabilitiesText: JSON.stringify(capabilities),
    });
    s.objectId = object.id;
    return { objectId: object.id, signature: object.signature, bundle: object.surfaces[0].bundle, capabilities: object.capabilities };
  };
  s.grants = async () => (await rpc('babel.capabilities.inspect.v1', { object_id: s.objectId })).grants;
  s.connect = async binding => {
    // Read-only parent observation of registration is checked against the API;
    // this does not depend on sharing a class with the production bundle.
    if (lastSession || !binding || typeof binding.sessionId !== 'string' || typeof binding.documentId !== 'string') {
      throw new Error('one observed fixture registration required');
    }
    const { session: admitted } = await get(`/runtime/surfaces/sessions/${encodeURIComponent(binding.sessionId)}`);
    if (admitted.id !== binding.sessionId || admitted.plan.object_id !== s.objectId || admitted.lifecycle !== 'active') {
      throw new Error('wrong admitted fixture Surface');
    }
    lastSession = binding.sessionId; documentId = binding.documentId;
    const frame = document.querySelector('[data-surface-host] iframe');
    const channel = new MessageChannel();
    s.port = channel.port1;
    s.port.onmessage = ({ data }) => {
      if (data.type === 'ready') s.childReady = true;
      else if (data.type === 'result') s.results[data.id] = { result: data.result, error: data.error };
      else if (data.type === 'mutated') s.mutated = data.id;
      else if (data.type === 'failure') s.failure = { name: 'ChildFailure', message: data.message };
    };
    frame.contentWindow.postMessage({ type: 'invocation.acceptance', marker, plan: admitted.plan,
      sessionId: lastSession, actorId: s.actorId }, new URL(frame.src).origin, [channel.port2]);
    return admitted;
  };
  s.send = input => {
    const payload = { author_id: s.actorId, target_object_id: s.targetId,
      ...(['reply', 'share'].includes(input.method) ? { text: input.text } : {}) };
    requests.set(input.id, { ...input, payload });
    s.port.postMessage({ type: 'request', ...input, payload });
    return true;
  };
  s.prepare = (id, changes = {}) => {
    const input = requests.get(id);
    return request('/invocations/v1/prepare', { method: 'POST', document: documentId, body: {
      origin: { kind: 'surface', session_id: lastSession, document_id: documentId }, object_id: s.objectId,
      method: `babel.social.${input.method}.v2`, request_key: input.key,
      payload: { ...input.payload, ...changes }, timeout_ms: input.timeoutMs,
    } });
  };
  s.status = id => request(`/invocations/v1/${encodeURIComponent(id)}/status`, { document: documentId });
  s.mutateInvocation = (id, action) => request(`/invocations/v1/${encodeURIComponent(id)}/${action}`,
    { method: 'POST', document: documentId, body: {} });
  s.isolation = async id => {
    const path = `/invocations/v1/${encodeURIComponent(id)}/status`;
    return {
      anonymous: (await request(path, { token: null, document: documentId })).status,
      otherActor: (await request(path, { token: other.current.token, document: documentId })).status,
      wrongDocument: (await request(path, { document: crypto.randomUUID() })).status,
    };
  };
  s.effects = async () => {
    const { edges } = await get(`/graph/objects/${s.targetId}/incoming`);
    const search = await rpc('babel.search.objects.v1', { q: marker, author: s.actorId, kind: null, limit: 100 });
    return { objects: search.results.map(item => item.object).filter(object =>
      object.id !== s.targetId && object.id !== s.objectId && object.payload.text?.includes(marker)).map(object => object.id).sort(),
      edges: edges.map(edge => edge.id).sort() };
  };
  s.waitForEviction = async () => {
    if (!lastSession) return true;
    const deadline = Date.now() + 15000;
    while (Date.now() < deadline) {
      const { session: value } = await get(`/runtime/surfaces/sessions/${lastSession}`);
      if (value.lifecycle === 'evicted') return true;
      await new Promise(resolve => setTimeout(resolve, 50));
    }
    throw new Error('fixture Surface eviction was not acknowledged');
  };
  s.geometryFrames = geometryFrames;
  s.cleanup = async () => {
    const errors = [];
    document.querySelector('[data-invocation-prompt] [aria-label="Cancel request"]')?.click();
    document.querySelector('[data-close-surface]')?.click();
    s.port?.close();
    for (const frame of geometryFrames) frame.remove();
    try { await s.waitForEviction(); } catch { errors.push('surface-eviction'); }
    try { await other?.logout(); } catch { errors.push('temporary-account-logout'); }
    const search = document.querySelector('[data-search-input]');
    search.value = originalSearch; search.form.requestSubmit();
    return { errors, parentAccountPreserved: sessionStorage.getItem(`babel.session.v1:${api.origin}`) === saved };
  };
  (async () => {
    const [{ Accounts }, { BabelFrontendClient }] = await Promise.all([
      import('/src/app/accounts.ts'), import('/src/app/protocol.ts'),
    ]);
    const accounts = new Accounts(api.href, sessionStorage);
    client = new BabelFrontendClient(api.href, accounts.authenticatedFetch);
    other = new Accounts(api.href, null);
    await other.register(`consent-probe-${crypto.randomUUID()}`, crypto.randomUUID() + crypto.randomUUID());
    s.ready = true;
  })().catch(error => { s.failure = fail(error); });
}

function application({ origin, marker }, createSurfaceSDK, connectSurfaceBridge) {
  let sdk, probe;
  const inputs = new Map();
  const transport = connectSurfaceBridge({ parentOrigin: origin });
  window.addEventListener('message', async event => {
    if (event.source !== parent || event.origin !== origin || event.data?.type !== 'invocation.acceptance'
      || event.data.marker !== marker || event.ports.length !== 1 || probe) return;
    probe = event.ports[0];
    try {
      sdk = createSurfaceSDK({ transport: await transport, plan: event.data.plan,
        surfaceSessionId: event.data.sessionId, runtimeId: 'invocation-acceptance-child', origin: location.origin,
        currentIdentityId: event.data.actorId });
      probe.onmessage = ({ data }) => {
        if (data.type === 'mutate') {
          inputs.get(data.id).text = data.text;
          probe.postMessage({ type: 'mutated', id: data.id });
          return;
        }
        if (data.type !== 'request' || !['reply', 'share', 'follow', 'unfollow'].includes(data.method)) return;
        inputs.set(data.id, data.payload);
        sdk.social[data.method](data.payload, { id: data.id, idempotencyKey: data.key, timeoutMs: data.timeoutMs })
          .then(result => probe.postMessage({ type: 'result', id: data.id, result, error: null }),
            error => probe.postMessage({ type: 'result', id: data.id, result: null,
              error: { name: error.name, code: error.code ?? null } }));
      };
      document.querySelector('#status').textContent = 'Ready';
      probe.postMessage({ type: 'ready' });
    } catch (error) { probe.postMessage({ type: 'failure', message: error.message }); }
  });
  // Keep an early handshake rejection visible until the test port arrives.
  transport.catch(error => { document.querySelector('#status').textContent = error.message; });
  window.addEventListener('pagehide', () => { sdk?.close(); probe?.close(); }, { once: true });
}

async function checkInterference() {
  const dialog = document.querySelector('[data-invocation-prompt]');
  const card = document.querySelector('.post-card[data-offset="0"]');
  const frame = document.querySelector('[data-surface-host] iframe');
  let bubbled = 0;
  const types = ['keydown', 'keyup', 'pointerdown', 'pointermove', 'pointerup', 'wheel'];
  const count = () => { bubbled++; };
  types.forEach(type => document.addEventListener(type, count));
  try {
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }));
    dialog.dispatchEvent(new KeyboardEvent('keyup', { key: 'ArrowRight', bubbles: true, cancelable: true }));
    for (const [type, x] of [['pointerdown', 280], ['pointermove', 120], ['pointerup', 20]]) {
      dialog.dispatchEvent(new PointerEvent(type, { clientX: x, clientY: 120, pointerId: 71,
        pointerType: 'touch', isPrimary: true, bubbles: true, cancelable: true }));
    }
    dialog.dispatchEvent(new WheelEvent('wheel', { deltaX: 250, bubbles: true, cancelable: true }));
    await Promise.allSettled(document.getAnimations().filter(animation =>
      animation.effect?.getComputedTiming().iterations !== Infinity).map(animation => animation.finished));
    return { bubbled, sameCard: card === document.querySelector('.post-card[data-offset="0"]'),
      sameFrame: frame === document.querySelector('[data-surface-host] iframe'), promptOpen: dialog.open };
  } finally { types.forEach(type => document.removeEventListener(type, count)); }
}

async function measureGeometry(s, widths) {
  const original = document.querySelector('[data-invocation-prompt]');
  const css = [...document.styleSheets].flatMap(sheet => [...sheet.cssRules].map(rule => rule.cssText)).join('\n');
  const results = [];
  for (const width of widths) {
    const frame = document.createElement('iframe');
    frame.title = `Invocation prompt layout ${width}`;
    frame.style.cssText = `position:fixed;left:-10000px;top:0;width:${width}px;height:1000px;border:0`;
    s.geometryFrames.add(frame);
    try {
      const loaded = new Promise((resolve, reject) => {
        frame.onload = resolve; frame.onerror = () => reject(new Error('geometry frame failed'));
      });
      frame.srcdoc = '<!doctype html><html><head></head><body></body></html>';
      document.body.append(frame);
      await loaded;
      const doc = frame.contentDocument;
      const style = doc.createElement('style'); style.textContent = css; doc.head.append(style);
      const dialog = doc.importNode(original, true);
      dialog.removeAttribute('open'); doc.body.append(dialog); dialog.showModal();
      await doc.fonts.ready;
      const box = dialog.getBoundingClientRect();
      const errors = [];
      if (frame.contentWindow.innerWidth !== width) errors.push('wrong viewport');
      if (box.left < 0 || box.right > width || box.top < 0 || box.bottom > 1000) errors.push('prompt outside viewport');
      if (dialog.scrollWidth > dialog.clientWidth) errors.push('horizontal prompt overflow');
      for (const button of dialog.querySelectorAll('button')) {
        const rect = button.getBoundingClientRect();
        if (rect.width < 44 || rect.height < 44) errors.push('small decision target');
        if (rect.left < box.left || rect.right > box.right) errors.push('button outside prompt');
        if (button.scrollWidth > button.clientWidth) errors.push('button label overflow');
      }
      const deny = dialog.querySelector('[data-invocation-deny]').getBoundingClientRect();
      const allow = dialog.querySelector('[data-invocation-allow]').getBoundingClientRect();
      if (deny.right > allow.left && deny.bottom > allow.top && allow.bottom > deny.top) errors.push('decision buttons overlap');
      results.push({ viewport: width, width: box.width, height: box.height, errors });
    } finally { frame.remove(); s.geometryFrames.delete(frame); }
  }
  return results;
}
