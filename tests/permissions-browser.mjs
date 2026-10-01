import assert from "node:assert/strict";

export async function verifyPermissions({ execute, waitFor, rpc, authorId }) {
  const evaluate = async code => {
    const response = await execute([{ type: "eval", code }]);
    assert.equal(response.results?.[0]?.ok, true, JSON.stringify(response));
    return response.results[0].value;
  };
  // A separate authenticated request uses the browser account without involving its UI controller.
  const accountRpc = async (method, payload, { sessionId = null, status = 200, idempotencyKey = null } = {}) => {
    await evaluate(`(() => {
      window.__permissionRpc = null;
      import('/src/app/accounts.ts').then(async ({ Accounts }) => {
        const accounts = new Accounts(document.documentElement.dataset.babbleApi, sessionStorage);
        const actorId = accounts.current?.identity.id;
        if (!actorId) throw new Error('Permission request requires the mounted Surface account');
        try {
          const response = await accounts.authenticatedFetch(new URL('/rpc', accounts.origin), {
            method: 'POST', headers: { 'content-type': 'application/json' },
            body: JSON.stringify({ protocol: 'babble.rpc.v1', id: crypto.randomUUID(),
              method: ${JSON.stringify(method)}, payload: ${JSON.stringify(sessionId ? { ...payload, session_id: sessionId } : payload)},
              binding: { object_id: null, surface_session_id: ${JSON.stringify(sessionId)},
                runtime_id: 'babble-permission-browser', origin: location.origin, capability_grants: [] },
              idempotency_key: ${JSON.stringify(idempotencyKey)} ?? crypto.randomUUID(),
              deadline: { timeout_ms: 30000, client_started_at: new Date().toISOString() }, trace_id: null }),
          });
          window.__permissionRpc = { actorId, status: response.status, body: await response.json() };
        } catch (error) {
          window.__permissionRpc = { actorId, status: error.status, error: String(error) };
        }
      }).catch(error => { window.__permissionRpc = { error: String(error) }; });
      return { requested: true };
    })()`);
    let response;
    try {
      response = await waitFor('window.__permissionRpc', value => value != null);
    } finally {
      await evaluate('delete window.__permissionRpc; ({ cleaned: true })');
    }
    assert.equal(response.status, status, `${method}: ${JSON.stringify(response)}`);
    if (status === 200) {
      assert.equal(response.error, undefined, JSON.stringify(response));
      assert.equal(response.body.error, null, JSON.stringify(response));
      assert.ok(response.body.result, `${method} should return a result`);
    }
    return response;
  };
  const current = await evaluate(`({ id: document.querySelector('.post-card[data-offset="0"]').dataset.objectId })`);
  const { object } = await rpc("babble.object.get.v1", { object_id: current.id });
  assert.ok(object.surfaces[0].bundle, "permission fixture reuses the just-published verified application");
  const marker = `Permission application ${Date.now()}`;
  const request = { id: "babble.storage.local", version: 1, scope: { namespace: "permission-counter" } };
  const { object: published } = await rpc("babble.object.publish.v1", { author_id: authorId, draft: {
    kind: object.kind, schema: object.schema, payload: { text: marker, metadata: {} },
    surfaces: object.surfaces, resources: object.resources, capabilities: [request],
    provenance: { parent: null, forked_from: null, remixed_from: [] },
  } });
  await evaluate(`(() => {
    const input = document.querySelector('[data-search-input]');
    input.value = ${JSON.stringify(marker)}; input.form.requestSubmit();
    return { searched: true };
  })()`);
  await waitFor(`({ id: document.querySelector('.post-card[data-offset="0"]')?.dataset.objectId })`, value => value?.id === published.id);
  const state = `(() => {
    const dialog = document.querySelector('[data-permission-dialog]');
    const row = dialog.querySelector('[data-permission-id="babble.storage.local"]');
    return { open: dialog.open, busy: dialog.getAttribute('aria-busy') === 'true',
      text: row?.textContent, action: row?.querySelector('[data-permission-action]')?.dataset.permissionAction,
      launchDisabled: dialog.querySelector('[data-permission-open]').disabled,
      error: dialog.querySelector('[data-permission-status]').textContent };
  })()`;
  await evaluate(`(() => {
    const card = document.querySelector('.post-card[data-offset="0"]');
    const menu = card.querySelector('.action-popover[data-kind="protocol"] > button');
    if (menu.getAttribute('aria-expanded') !== 'true') menu.click();
    card.querySelector('[data-action="permissions"]').click(); return { opened: true };
  })()`);
  const initial = await waitFor(state, value => value?.open && !value.busy && value.action === "approve");
  assert.match(initial.text, /permission-counter/);
  assert.equal(initial.launchDisabled, true);
  assert.equal(initial.error, "");
  await evaluate(`document.querySelector('[data-permission-action="approve"]').click(); ({ approved: true })`);
  const allowed = await waitFor(state, value => value?.open && !value.busy && value.action === "revoke");
  assert.equal(allowed.launchDisabled, false);
  assert.equal(allowed.error, "");
  await evaluate(`document.querySelector('[data-permission-open]').click(); ({ opened: true })`);
  const surfaceState = `(() => {
    const panel = document.querySelector('[data-surface-panel]');
    const host = document.querySelector('[data-surface-host]');
    return { phase: host.dataset.state, noFrame: !host.querySelector('iframe'),
      message: host.textContent, retry: !document.querySelector('[data-retry-surface]').hidden,
      reviewOpen: document.querySelector('[data-permission-dialog]').open,
      session: [...panel.querySelectorAll('[data-surface-meta] span')].find(span => span.textContent.startsWith('Session: '))?.textContent.slice(9),
      active: panel.querySelector('iframe')?.dataset.babbleLifecycle === 'active',
      permission: [...panel.querySelectorAll('[data-surface-meta] span')].some(span => span.textContent.includes('babble.storage.local') && span.textContent.includes('granted')) };
  })()`;
  const mounted = await waitFor(surfaceState, value => value?.phase === "active" && value.active && value.session);
  assert.equal(mounted.permission, true);
  assert.equal(mounted.reviewOpen, false);
  const admitted = await accountRpc("babble.runtime.surface.session.get.v1", {}, { sessionId: mounted.session });
  const admittedSession = admitted.body.result.session;
  assert.equal(admittedSession.id, mounted.session);
  assert.equal(admittedSession.lifecycle, "active");
  assert.equal(admittedSession.plan.object_id, published.id);
  assert.notEqual(admitted.actorId, authorId, "consent belongs to the viewer of the foreign-authored Object");
  const selected = admittedSession.plan.capability_decisions.find(decision => decision.request.id === request.id);
  assert.equal(selected?.status, "granted");
  assert.deepEqual(selected.request, request);
  assert.ok(selected.grant?.id, "admission must select the explicit grant being revoked");
  assert.deepEqual(selected.grant.scope, request.scope);
  const revokedGrantId = selected.grant.id;
  const revokeKey = `permission-revoke-${revokedGrantId}`;
  const revokePayload = { author_id: admitted.actorId, object_id: published.id, grant_id: revokedGrantId };
  let revocationEvent;
  await evaluate(`(() => {
    const host = document.querySelector('[data-surface-host]');
    const probe = window.__permissionRevocation = { frame: host.querySelector('iframe'), addedFrames: 0 };
    probe.observer = new MutationObserver(records => {
      for (const record of records) for (const node of record.addedNodes) {
        if (node instanceof Element) probe.addedFrames += Number(node.matches('iframe')) + node.querySelectorAll('iframe').length;
      }
    });
    probe.observer.observe(host, { childList: true, subtree: true });
    return { watching: true };
  })()`);
  try {
    const remoteRevocation = await accountRpc("babble.capabilities.revoke.v1", revokePayload, { idempotencyKey: revokeKey });
    assert.equal(remoteRevocation.actorId, admitted.actorId);
    assert.equal(remoteRevocation.body.result.event.kind, "capability_revoked");
    revocationEvent = remoteRevocation.body.result.event;
    const revokeRetry = await accountRpc("babble.capabilities.revoke.v1", revokePayload, { idempotencyKey: revokeKey });
    assert.deepEqual(revokeRetry.body.result, remoteRevocation.body.result);
    const retired = await accountRpc("babble.runtime.surface.session.get.v1", {}, { sessionId: mounted.session });
    assert.equal(retired.body.result.session.id, mounted.session);
    assert.equal(retired.body.result.session.lifecycle, "evicted", "successful remote revocation must retire the server session immediately");

    // No UI action or manual heartbeat intervenes: the normal <=15s renewal must stop the iframe.
    const stopped = await waitFor(surfaceState, value => value?.phase === "error" && value.noFrame);
    assert.equal(stopped.reviewOpen, false);
    assert.equal(stopped.retry, true);
    assert.match(stopped.message, /Open the Surface again to retry/i);
    assert.doesNotMatch(stopped.message, /heartbeat timed out|lease expired/i, "revocation must reject renewal before ordinary lease expiry");
    await accountRpc("babble.runtime.surface.session.heartbeat.v1", {}, { sessionId: mounted.session, status: 403 });
    await accountRpc("babble.runtime.surface.session.transition.v1", {
      lifecycle: "active", reason: "Permission regression must not reactivate a revoked session",
    }, { sessionId: mounted.session, status: 403 });

    // Observe another complete renewal interval to catch an automatic remount after lease failure.
    await evaluate('window.__permissionRevocation.stoppedAt = Date.now(); ({ observing: true })');
    const quiet = await waitFor(`({ ...${surfaceState},
      addedFrames: window.__permissionRevocation.addedFrames,
      originalRemoved: !window.__permissionRevocation.frame.isConnected,
      observedMs: Date.now() - window.__permissionRevocation.stoppedAt })`, value => value.observedMs >= 16_000);
    assert.equal(quiet.phase, "error");
    assert.equal(quiet.noFrame, true);
    assert.equal(quiet.originalRemoved, true);
    assert.equal(quiet.addedFrames, 0, "revocation must not silently mount a replacement iframe");
    await evaluate(`document.querySelector('[data-retry-surface]').click(); ({ retry: true })`);
    const blocked = await waitFor(surfaceState, value => value?.phase === "blocked" && value.noFrame);
    assert.match(blocked.message, /permission|revoked|grant|access/i);
    assert.equal(blocked.session, undefined, "retry without a new grant must not admit a new session");
    await evaluate(`document.querySelector('[data-surface-permissions]').click(); ({ reviewing: true })`);
    const denied = await waitFor(state, value => value?.open && !value.busy && value.action === "approve");
    assert.equal(denied.launchDisabled, true);
    assert.equal(denied.error, "");
    assert.equal((await evaluate('({ addedFrames: window.__permissionRevocation.addedFrames })')).addedFrames, 0);
  } finally {
    await evaluate(`window.__permissionRevocation.observer.disconnect(); delete window.__permissionRevocation; ({ cleaned: true })`);
  }
  await evaluate(`document.querySelector('[data-permission-action="approve"]').click(); ({ approved: true })`);
  const reapproved = await waitFor(state, value => value?.open && !value.busy && value.action === "revoke");
  assert.equal(reapproved.launchDisabled, false);
  assert.equal(reapproved.error, "");
  assert.equal((await evaluate(surfaceState)).noFrame, true, "a new grant alone must not silently reopen the Surface");
  await evaluate(`document.querySelector('[data-permission-open]').click(); ({ opened: true })`);
  const remounted = await waitFor(surfaceState, value => value?.phase === "active" && value.active && value.session);
  assert.notEqual(remounted.session, mounted.session, "explicit reopening must use a fresh session");
  assert.equal(remounted.permission, true);
  const readmitted = await accountRpc("babble.runtime.surface.session.get.v1", {}, { sessionId: remounted.session });
  const freshGrant = readmitted.body.result.session.plan.capability_decisions.find(decision => decision.request.id === request.id);
  assert.equal(readmitted.actorId, admitted.actorId);
  assert.equal(freshGrant?.status, "granted");
  assert.ok(freshGrant.grant?.id);
  assert.notEqual(freshGrant.grant.id, revokedGrantId, "the revoked grant cannot be reused for admission");
  assert.deepEqual(freshGrant.grant.scope, request.scope);
  const laterRevokeRetry = await accountRpc("babble.capabilities.revoke.v1", revokePayload, { idempotencyKey: revokeKey });
  assert.deepEqual(laterRevokeRetry.body.result.event, revocationEvent);
  assert.equal(laterRevokeRetry.body.result.grants.find(grant => grant.id === freshGrant.grant.id)?.revoked_at, null,
    "retrying an earlier revocation must leave the later approval intact");
  const stillRetired = await accountRpc("babble.runtime.surface.session.get.v1", {}, { sessionId: mounted.session });
  assert.equal(stillRetired.body.result.session.lifecycle, "evicted", "regrant must not resurrect the old session");
  await accountRpc("babble.runtime.surface.session.heartbeat.v1", {}, { sessionId: mounted.session, status: 403 });
  await evaluate(`document.querySelector('[data-surface-permissions]').click(); ({ reviewing: true })`);
  await waitFor(state, value => value?.open && !value.busy && value.action === "revoke");
  assert.deepEqual(await evaluate(`({ noFrame: !document.querySelector('[data-surface-host] iframe') })`), { noFrame: true });
  await evaluate(`document.querySelector('[data-permission-action="revoke"]').click(); ({ revoked: true })`);
  const revoked = await waitFor(state, value => value?.open && !value.busy && value.action === "approve");
  assert.equal(revoked.launchDisabled, true);
  assert.equal(revoked.error, "");
  await evaluate(`document.querySelector('[aria-label="Close permissions"]').click(); ({ closed: true })`);
  // Reopening checks persisted access, not the controller's previous snapshot.
  await evaluate(`(() => {
    const card = document.querySelector('.post-card[data-offset="0"]');
    const menu = card.querySelector('.action-popover[data-kind="protocol"] > button');
    if (menu.getAttribute('aria-expanded') !== 'true') menu.click();
    card.querySelector('[data-action="permissions"]').click(); return { opened: true };
  })()`);
  await waitFor(state, value => value?.open && !value.busy && value.action === "approve");
  await evaluate(`document.querySelector('[aria-label="Close permissions"]').click(); ({ closed: true })`);
  const grantPayload = { author_id: admitted.actorId, object_id: published.id, capability: request, decision: "approved" };
  const grantKey = `permission-grant-${published.id}`;
  const grantResult = (await accountRpc("babble.capabilities.grant.v1", grantPayload, { idempotencyKey: grantKey })).body.result;
  const grantRetry = (await accountRpc("babble.capabilities.grant.v1", grantPayload, { idempotencyKey: grantKey })).body.result;
  assert.deepEqual(grantRetry, grantResult, "identical browser retries must not issue a second grant");
  const retriedGrantId = grantResult.event.payload.grant.id;
  await accountRpc("babble.capabilities.revoke.v1", {
    author_id: admitted.actorId, object_id: published.id, grant_id: retriedGrantId,
  });
  const revokedGrantRetry = (await accountRpc("babble.capabilities.grant.v1", grantPayload, { idempotencyKey: grantKey })).body.result;
  assert.deepEqual(revokedGrantRetry.event, grantResult.event);
  assert.ok(revokedGrantRetry.grants.find(grant => grant.id === retriedGrantId)?.revoked_at,
    "a historical approval replay must report the current revoked grant");
  const reviewedRetry = (await accountRpc("babble.capabilities.inspect.v1", { object_id: published.id })).body.result;
  assert.ok(reviewedRetry.decisions.every(decision => decision.status !== "granted"),
    "replaying old approval must not restore launch authority");
  await evaluate(`(() => {
    window.__permissionGeometry = null;
    (async () => {
      const results = [];
      for (const width of [320, 390, 860]) {
        const frame = document.createElement('iframe');
        frame.title = 'Permission review responsive verification';
        frame.style.cssText = 'position:fixed;inset:0;border:0;z-index:9999;width:' + width + 'px;height:844px';
        frame.src = location.href;
        document.body.append(frame);
        try {
          const deadline = Date.now() + 15000;
          const until = async predicate => {
            while (!predicate()) {
              if (Date.now() > deadline) throw new Error('Permission frame timed out');
              await new Promise(resolve => setTimeout(resolve, 50));
            }
          };
          await until(() => frame.contentDocument?.querySelector('.post-card[data-offset="0"] [data-action="permissions"]'));
          const doc = frame.contentDocument;
          const card = doc.querySelector('.post-card[data-offset="0"]');
          card.querySelector('.action-popover[data-kind="protocol"] > button').click();
          card.querySelector('[data-action="permissions"]').click();
          const dialog = doc.querySelector('[data-permission-dialog]');
          await until(() => dialog.open && dialog.querySelector('[data-permission-action="approve"]') && dialog.getAttribute('aria-busy') === 'false');
          const rect = dialog.getBoundingClientRect();
          const controls = [...dialog.querySelectorAll('button')].map(node => node.getBoundingClientRect());
          results.push({ width, inViewport: rect.left >= 0 && rect.right <= width,
            overflow: dialog.scrollWidth > dialog.clientWidth,
            contained: controls.every(r => r.left >= rect.left && r.right <= rect.right),
            touchTargets: controls.every(r => r.width >= 44 && r.height >= 44),
            scope: dialog.querySelector('dd').textContent });
          dialog.querySelector('[aria-label="Close permissions"]').click();
        } finally { frame.remove(); }
      }
      window.__permissionGeometry = { results };
    })().catch(error => { window.__permissionGeometry = { error: String(error) }; });
    return { started: true };
  })()`);
  const geometry = await waitFor("window.__permissionGeometry", value => value != null);
  await evaluate("delete window.__permissionGeometry; ({ cleaned: true })");
  assert.equal(geometry.error, undefined, JSON.stringify(geometry));
  for (const item of geometry.results) {
    assert.equal(item.inViewport, true, JSON.stringify(item));
    assert.equal(item.overflow, false, JSON.stringify(item));
    assert.equal(item.contained, true, JSON.stringify(item));
    assert.equal(item.touchTargets, true, JSON.stringify(item));
    assert.equal(item.scope, "permission-counter");
  }
  console.log("Surface permission consent PASS", JSON.stringify({ declaredScope: true, explicitApproval: true,
    foreignAuthoredObject: true, admittedExecution: true, remoteRevocationEvictsSession: true,
    heartbeatRemovesMountedFrame: true, noSilentReopen: true, revokedRetryBlocked: true,
    explicitRegrantStartsFreshSession: true, oldSessionRemainsEvicted: true,
    stoppedBeforeReview: true, revoked: true, persistedReadback: true }));
}
