import assert from "node:assert/strict";

/**
 * Run inside the isolated live-stack browser; creates a fixture account with memory-only credentials.
 * objectId must identify the signed content-addressed seed Surface that sends surface-auto-1.
 * execute/waitFor are the parent's Aegis /execute adapters; this module owns no server.
 */
export async function verifyDocumentRegistration(execute, waitFor, { apiUrl, objectId }) {
  const api = new URL(apiUrl);
  assert.ok(["127.0.0.1", "localhost", "[::1]"].includes(api.hostname), "requires an isolated loopback API");
  assert.ok(typeof objectId === "string" && objectId.startsWith("obj_"), "requires the executable seed Object");
  const evaluate = async code => {
    const response = await execute([{ type: "eval", code }]);
    // Never stringify arbitrary evaluator failures: they can contain authentication inputs.
    assert.equal(response.results?.[0]?.ok, true, "document registration browser evaluation failed");
    return response.results[0].value;
  };
  const snapshot = "window.__documentRegistrationTest?.snapshot()";
  const until = async predicate => {
    const state = await waitFor(snapshot, value => value && (value.failure || predicate(value)));
    assert.equal(state.failure, null, `document registration browser failure: ${JSON.stringify(state.failure)}`);
    return state;
  };
  let evidence, installed = false;
  try {
    await evaluate(`(${install.toString()})(${JSON.stringify({ apiUrl: api.href, objectId })})`);
    installed = true;
    const held = await until(state => state.registration !== null);
    assert.equal(held.registration.status, 200, "real authenticated registration must succeed");
    assert.equal(held.registration.authenticated, true);
    assert.equal(held.registration.method, "PUT");
    assert.deepEqual(held.registration.bodyKeys, ["document_id"]);
    assert.equal(held.registration.responseSessionId, held.sessionId);
    assert.equal(held.registration.responseDocumentId, held.registration.documentId);
    assert.match(held.registration.documentId, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
    assert.equal(held.phase, "mounting");
    const seed = held.seedPlan;
    assert.ok(seed, "the real session must expose its admitted seed Surface plan");
    assert.equal(seed.objectId, objectId);
    assert.equal(seed.role, "Feed");
    assert.equal(seed.target, "Web");
    assert.equal(seed.signatureAlgorithm, "Ed25519");
    assert.equal(seed.signaturePresent, true);
    assert.equal(seed.signedSurfaceMatches, true, "admitted Surface must match the signed Object's Surface");
    assert.equal(seed.bundle, null, "this fixture exercises the content-addressed seed, not the verified bundle gateway");
    assert.match(seed.integrity, /^[0-9a-f]{64}$/);
    assert.equal(seed.resourceIntegrity, seed.integrity);
    assert.equal(seed.resourceUri, `babel://blobs/${seed.integrity}`);
    assert.equal(seed.resourceMediaType, "text/html");
    const entry = new URL(seed.entry);
    assert.equal(entry.origin, api.origin);
    assert.equal(entry.pathname, `/runtime/surfaces/blobs/${seed.integrity}`);
    assert.equal(entry.searchParams.get("media_type"), "text/html");
    assert.equal(held.dispatches.length, 0, "dispatch cannot run before registration is acknowledged");
    assert.deepEqual(held.controls, ["babel.surface.accept"]);
    assert.ok(held.events.indexOf("confirm") >= 0);
    assert.ok(held.events.indexOf("confirm") < held.events.indexOf("registration-request"));

    await evaluate("window.__documentRegistrationTest.release(); ({ released: true })");
    const active = await until(state => state.phase === "active" && state.bridgeResponse !== null);
    assert.equal(active.offerOrigin, "null", "the seed Surface retains an opaque sandbox origin");
    assert.deepEqual(active.controls, ["babel.surface.accept", "babel.surface.ready"]);
    assert.equal(active.controlShapeValid, true, "registration must not change the child protocol");
    assert.equal(active.bridgeResponse.id, "surface-auto-1");
    assert.equal(active.bridgeResponse.errorCode, null);
    assert.equal(active.bridgeResponse.resultCount, 1, "bridge must deliver a real backend read result");
    assert.equal(active.dispatches.length, 1);
    assert.equal(active.dispatches[0].documentId, active.registration.documentId);
    assert.equal(active.dispatches[0].objectId, objectId);
    assert.equal(active.dispatches[0].sessionId, active.sessionId);
    assert.equal(active.dispatches[0].signal, true);
    const boundRead = active.requests.find(request => request.id === "surface-auto-1");
    assert.ok(boundRead, "the bridge read must reach the actual HTTP transport");
    assert.equal(boundRead.documentId, active.registration.documentId);
    assert.equal(boundRead.method, "babel.search.objects.v1");
    assert.equal(boundRead.objectId, objectId);
    assert.equal(boundRead.sessionId, active.sessionId);
    assert.equal(boundRead.authenticated, true);
    assert.equal(boundRead.status, 200);
    const order = ["confirm", "registration-request", "registration-response", "registration-released", "ready", "dispatch", "rpc-request", "rpc-response"];
    for (let index = 1; index < order.length; index++) {
      assert.ok(active.events.indexOf(order[index - 1]) < active.events.indexOf(order[index]), `expected ${order[index - 1]} before ${order[index]}`);
    }
    const management = active.requests.filter(request => request.sessionId && !request.objectId);
    assert.ok(management.some(request => request.method === "babel.runtime.surface.session.heartbeat.v1"));
    assert.ok(management.some(request => request.method === "babel.runtime.surface.session.transition.v1"));
    for (const request of management) {
      assert.equal(request.documentId, null, "host session management must not carry a document header");
      assert.equal(request.status, 200);
    }

    await evaluate("window.__documentRegistrationTest.exercise(); ({ started: true })");
    evidence = await until(state => state.probes !== null);
    assert.equal(evidence.sameIdentityDifferentLogin, true);
    assert.deepEqual(evidence.probes, {
      identicalRetry: 200,
      retryMatches: true,
      replacementDocument: 409,
      otherLoginRegistration: 403,
      anonymousRegistration: 401,
      malformedDocument: 400,
      currentDocumentRead: 200,
      currentDocumentReadSucceeded: true,
      missingDocument: 403,
      mismatchedDocument: 403,
      otherLoginRead: 403,
      originalDocumentStillWorks: true,
      unregisteredDocument: 403,
      closedDocumentRead: 403,
    });
    assert.deepEqual(evidence.cleanupErrors, []);
  } finally {
    if (installed) {
      await evaluate("window.__documentRegistrationTest.cleanup(); ({ cleaning: true })");
      const cleaned = await waitFor(snapshot, value => value?.cleaned === true);
      await evaluate("delete window.__documentRegistrationTest; ({ cleaned: true })");
      assert.deepEqual(cleaned.cleanupErrors, [], "document registration cleanup must evict sessions and revoke both fixture logins");
    }
  }
  console.log("Document registration PASS: signed content-addressed seed Surface, authenticated registration before ready, trusted HTTP header, backend read, immutable retry, login isolation, host management and eviction");
  return { sessionId: evidence.sessionId, documentId: evidence.registration.documentId, seedPlan: evidence.seedPlan, probes: evidence.probes };
}

// Serialized into Aegis. Credentials remain in the browser and are never in snapshots.
function install({ apiUrl, objectId }) {
  if (window.__documentRegistrationTest) throw new Error("document registration test already installed");
  const s = window.__documentRegistrationTest = {
    phase: "setup", failure: null, registration: null, sessionId: null, seedPlan: null,
    events: [], controls: [], controlShapeValid: true, dispatches: [], requests: [],
    bridgeResponse: null, offerOrigin: null, probes: null, sameIdentityDifferentLogin: false,
    cleanupErrors: [], cleaned: false,
  };
  let accounts, otherLogin, client, surfaces, container, boundRequest, freshSession;
  let observer, releaseRegistration, opening, exercising, cleaning;
  const restorePorts = [];
  const nativeFetch = window.fetch.bind(window);
  const registrationGate = new Promise(resolve => { releaseRegistration = resolve; });
  s.release = () => { s.events.push("registration-released"); releaseRegistration(); };
  s.snapshot = () => ({
    phase: s.phase, failure: s.failure, registration: s.registration, sessionId: s.sessionId, seedPlan: s.seedPlan,
    events: s.events, controls: s.controls, controlShapeValid: s.controlShapeValid,
    dispatches: s.dispatches, requests: s.requests, bridgeResponse: s.bridgeResponse,
    offerOrigin: s.offerOrigin, probes: s.probes, sameIdentityDifferentLogin: s.sameIdentityDifferentLogin,
    cleanupErrors: s.cleanupErrors, cleaned: s.cleaned,
  });
  const fail = (stage, error) => { s.failure = { stage, name: error?.name ?? "Error", status: error?.status ?? null }; };
  const require = (condition, label) => { if (!condition) { const error = new Error(label); error.name = label; throw error; } };

  // Observe the final authenticated RequestInit; forward every call to the real API.
  const observedFetch = async (input, init = {}) => {
    const url = new URL(input instanceof Request ? input.url : String(input));
    const headers = new Headers(input instanceof Request ? input.headers : undefined);
    new Headers(init.headers).forEach((value, key) => headers.set(key, value));
    const registration = url.origin === new URL(apiUrl).origin && /^\/runtime\/surfaces\/sessions\/[^/]+\/document$/.test(url.pathname);
    const rpc = url.origin === new URL(apiUrl).origin && url.pathname === "/rpc";
    const body = (registration || rpc) && typeof init.body === "string" ? JSON.parse(init.body) : null;
    let record;
    if (rpc) {
      record = { id: body.id, method: body.method, objectId: body.binding.object_id,
        sessionId: body.binding.surface_session_id, documentId: headers.get("x-babel-surface-document"),
        authenticated: headers.has("authorization"), status: null };
      s.requests.push(record);
      if (record.objectId && record.sessionId) s.events.push("rpc-request");
    }
    if (registration) s.events.push("registration-request");
    const response = await nativeFetch(input, init);
    if (record) {
      record.status = response.status;
      if (record.objectId && record.sessionId) s.events.push("rpc-response");
    }
    if (registration) {
      const result = await response.clone().json();
      s.events.push("registration-response");
      s.registration = { status: response.status, method: init.method, authenticated: headers.has("authorization"),
        bodyKeys: Object.keys(body).sort(), documentId: body.document_id,
        responseSessionId: result.session_id ?? null, responseDocumentId: result.document_id ?? null };
      // Hold the actual response, never substitute a successful endpoint or callback.
      await registrationGate;
    }
    return response;
  };

  opening = (async () => {
    const [{ Accounts }, { BabelFrontendClient }, { Surfaces }] = await Promise.all([
      import("/src/app/accounts.ts"), import("/src/app/protocol.ts"), import("/src/app/surfaces.ts"),
    ]);
    require(new URL(document.documentElement.dataset.babelApi).origin === new URL(apiUrl).origin, "FixtureApiMismatch");
    accounts = new Accounts(apiUrl, null, observedFetch);
    require(accounts.current === null, "FixtureMustNotRestoreParentLogin");
    let password = crypto.randomUUID() + crypto.randomUUID();
    await accounts.register(`document-registration-${crypto.randomUUID()}`, password);
    otherLogin = new Accounts(apiUrl, null);
    require(otherLogin.current === null, "SecondLoginMustStartUnauthenticated");
    await otherLogin.login(accounts.current.identity.id, password);
    password = null;
    s.sameIdentityDifferentLogin = otherLogin.current.identity.id === accounts.current.identity.id
      && otherLogin.current.token !== accounts.current.token;
    client = new BabelFrontendClient(apiUrl, accounts.authenticatedFetch);
    const { object: seedObject } = await client.publicObject(objectId);
    container = document.createElement("div");
    container.dataset.documentRegistrationTest = "";
    container.hidden = true;
    document.body.append(container);
    observer = event => {
      if (event.source !== container.querySelector("iframe")?.contentWindow
        || event.data?.type !== "babel.surface.connect" || event.ports.length !== 1) return;
      s.offerOrigin = event.origin;
      const port = event.ports[0], send = port.postMessage;
      const receive = message => {
        if (message.data?.type === "babel.surface.confirm") s.events.push("confirm");
      };
      port.addEventListener("message", receive, { capture: true });
      port.postMessage = function(message, ...args) {
        if (message?.type?.startsWith("babel.surface.")) {
          s.controls.push(message.type);
          s.controlShapeValid &&= Object.keys(message).sort().join(",") === "protocol,type,version"
            && message.protocol === "babel.rpc.v1" && message.version === 1;
          if (message.type === "babel.surface.ready") s.events.push("ready");
        }
        if (message?.type === "babel.rpc.response" && message.response?.id === "surface-auto-1") {
          s.bridgeResponse = { id: message.response.id, errorCode: message.response.error?.code ?? null,
            resultCount: message.response.result?.results?.length ?? null };
        }
        return send.call(this, message, ...args);
      };
      restorePorts.push(() => { port.postMessage = send; port.removeEventListener("message", receive, { capture: true }); });
    };
    window.addEventListener("message", observer, { capture: true });
    surfaces = new Surfaces({
      onState: state => {
        s.phase = state.phase;
        if (state.session) s.sessionId = state.session.id;
        if (state.phase === "mounting") {
          const surface = state.plan.surface;
          const resource = seedObject.resources.find(resource => resource.integrity === surface.integrity);
          s.seedPlan = {
            objectId: state.plan.object_id, role: surface.role, target: surface.target,
            entry: surface.entry, integrity: surface.integrity ?? null, bundle: surface.bundle ?? null,
            signatureAlgorithm: seedObject.signature?.algorithm ?? null,
            signaturePresent: typeof seedObject.signature?.bytes === "string" && seedObject.signature.bytes.length > 0,
            signedSurfaceMatches: seedObject.id === state.plan.object_id && seedObject.surfaces.some(candidate =>
              candidate.role === surface.role && candidate.target === surface.target
              && candidate.entry === surface.entry && candidate.integrity === surface.integrity
              && (candidate.bundle ?? null) === null && (surface.bundle ?? null) === null),
            resourceIntegrity: resource?.integrity ?? null, resourceUri: resource?.uri ?? null,
            resourceMediaType: resource?.media_type ?? null,
          };
        }
        if (state.phase === "error" || state.phase === "blocked") fail("surface-open", { name: state.phase });
      },
      onCleanupError: () => s.cleanupErrors.push("surface-cleanup"),
    });
    const dispatch = client.bridgeDispatch();
    await surfaces.open({ objectId, container, currentIdentityId: accounts.current.identity.id,
      source: client, authorized: () => accounts.current !== null,
      dispatch: (request, context) => {
        s.events.push("dispatch");
        boundRequest = structuredClone(request);
        s.dispatches.push({ documentId: context.surfaceDocumentId ?? null,
          objectId: request.binding.object_id, sessionId: request.binding.surface_session_id,
          signal: context.signal instanceof AbortSignal });
        return dispatch(request, context);
      },
    });
  })().catch(error => fail("setup-or-mount", error));

  // Use fresh IDs for every backend read so a replay cannot stand in for authorization.
  const read = async (owner, documentId, sessionId = s.sessionId) => {
    const request = structuredClone(boundRequest);
    request.id = crypto.randomUUID();
    request.idempotency_key = null;
    request.binding.surface_session_id = sessionId;
    request.deadline.client_started_at = new Date().toISOString();
    const headers = { "content-type": "application/json", authorization: `Bearer ${owner.current.token}` };
    if (documentId !== null) headers["x-babel-surface-document"] = documentId;
    const response = await nativeFetch(new URL("/rpc", apiUrl), {
      method: "POST", headers, body: JSON.stringify(request), credentials: "omit", redirect: "error",
      signal: AbortSignal.timeout(15000),
    });
    const body = await response.json();
    return { status: response.status, succeeded: response.status === 200 && body.error === null && body.result?.results?.length === 1 };
  };
  const register = async (owner, documentId) => {
    const headers = { "content-type": "application/json" };
    if (owner) headers.authorization = `Bearer ${owner.current.token}`;
    const response = await nativeFetch(new URL(`/runtime/surfaces/sessions/${encodeURIComponent(s.sessionId)}/document`, apiUrl), {
      method: "PUT", headers, body: JSON.stringify({ document_id: documentId }), credentials: "omit", redirect: "error",
      signal: AbortSignal.timeout(15000),
    });
    const body = await response.json();
    return { status: response.status, matches: body.session_id === s.sessionId && body.document_id === documentId };
  };
  s.exercise = () => {
    if (exercising) return;
    exercising = (async () => {
      const documentId = s.registration.documentId;
      const retry = await register(accounts, documentId);
      const probes = { identicalRetry: retry.status, retryMatches: retry.matches };
      probes.replacementDocument = (await register(accounts, crypto.randomUUID())).status;
      probes.otherLoginRegistration = (await register(otherLogin, documentId)).status;
      probes.anonymousRegistration = (await register(null, documentId)).status;
      probes.malformedDocument = (await register(accounts, "not-a-uuid")).status;
      const current = await read(accounts, documentId);
      probes.currentDocumentRead = current.status;
      probes.currentDocumentReadSucceeded = current.succeeded;
      probes.missingDocument = (await read(accounts, null)).status;
      probes.mismatchedDocument = (await read(accounts, crypto.randomUUID())).status;
      probes.otherLoginRead = (await read(otherLogin, documentId)).status;
      probes.originalDocumentStillWorks = (await read(accounts, documentId)).succeeded;
      freshSession = await client.startSurfaceSession(objectId);
      probes.unregisteredDocument = (await read(accounts, documentId, freshSession.id)).status;
      await client.transitionSurfaceSession(freshSession.id, "evicted", "Document registration test finished");
      freshSession = null;
      await surfaces.close("Document registration test finished");
      probes.closedDocumentRead = (await read(accounts, documentId)).status;
      s.probes = probes;
    })().catch(error => fail("authorization-probes", error));
  };
  s.cleanup = () => {
    if (cleaning) return;
    cleaning = (async () => {
      releaseRegistration();
      await opening;
      await exercising;
      try { await surfaces?.close("Document registration test cleanup"); }
      catch { s.cleanupErrors.push("close-mounted-session"); }
      if (freshSession) {
        try { await client.transitionSurfaceSession(freshSession.id, "evicted", "Document registration test cleanup"); }
        catch { s.cleanupErrors.push("close-unregistered-session"); }
      }
      try { await otherLogin?.logout(); }
      catch { s.cleanupErrors.push("revoke-extra-login"); }
      try { await accounts?.logout(); }
      catch { s.cleanupErrors.push("revoke-fixture-login"); }
      if (observer) window.removeEventListener("message", observer, { capture: true });
      for (const restore of restorePorts) restore();
      container?.remove();
      s.cleaned = true;
    })();
  };
  return { installed: true };
}
