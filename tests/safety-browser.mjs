import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";

// Called only by the parent's disposable, authenticated Aegis live stack.
// authorId is the parent RPC fixture author, not necessarily the browser account.
// No login replacement, clipboard access, external navigation, or frontend imports.
export async function verifySocialSafety({ execute, waitFor, rpc, authorId, apiUrl }) {
  for (const callback of [execute, waitFor, rpc]) assert.equal(typeof callback, "function");
  assert.match(authorId, /^id_[a-f0-9]{64}$/);
  const api = new URL(apiUrl);
  assert.ok(["localhost", "127.0.0.1", "[::1]"].includes(api.hostname), "Use an isolated local live-stack API");
  const marker = `safety-${randomUUID()}`;
  const evaluate = async code => {
    const response = await execute([{ type: "eval", code }]);
    assert.equal(response.results?.[0]?.ok, true, JSON.stringify(response));
    const value = response.results[0].value;
    assert.ok(value && typeof value === "object", "Aegis probes must return objects");
    return value;
  };
  const run = async stage => {
    await evaluate(`(() => {
      window.__socialSafetyResult = null;
      window.__socialSafetyStage = ${JSON.stringify(stage)};
      Promise.resolve().then(() => window.__socialSafetyHarness.run(${JSON.stringify(stage)}))
        .then(result => { window.__socialSafetyResult = { result }; })
        .catch(error => { window.__socialSafetyResult = { error: String(error), step: window.__socialSafetyStep }; });
      return { started: true };
    })()`);
    const snapshot = await waitFor("({ outcome: window.__socialSafetyResult, stage: window.__socialSafetyStage, step: window.__socialSafetyStep })",
      value => value?.outcome != null);
    assert.equal(snapshot.outcome.error, undefined, JSON.stringify(snapshot));
    return snapshot.outcome.result;
  };
  const coverage = {};
  let failure;
  try {
    await evaluate(`(() => {
      if (window.__socialSafetyHarness) throw Error('Another safety harness is still active');
      window.__socialSafetyHarness = (${socialSafetyHarness.toString()})(${JSON.stringify({ apiUrl: api.origin, marker })});
      return { installed: true };
    })()`);
    const fixture = await run("setup");
    assert.notEqual(fixture.ownerId, fixture.targetId);
    const publicRead = await rpc("babble.search.objects.v1", {
      q: marker, author: fixture.targetId, kind: null, limit: 20,
    });
    assert.ok(publicRead.results.some(item => item.object.id === fixture.objectId), "Real RPC must discover the signed fixture");
    for (const stage of ["cancel", "layout-320", "layout-390", "layout-1280", "mute", "muted-writes", "block", "blocked-writes", "restore", "receipts", "privacy"]) {
      coverage[stage] = await run(stage);
      assert.equal(coverage[stage].verified, true, `${stage} did not complete`);
    }
  } catch (cause) { failure = cause; }
  finally {
    try { coverage.cleanup = await run("cleanup"); }
    catch (cause) { failure = failure ? new AggregateError([failure, cause], "Safety acceptance and cleanup failed") : cause; }
    // Do not remove a running probe after a timeout: its cleanup must not race it.
    try {
      await evaluate("(() => { if (window.__socialSafetyHarness?.busy) throw Error('Safety probe still running'); delete window.__socialSafetyHarness; delete window.__socialSafetyResult; delete window.__socialSafetyStep; delete window.__socialSafetyStage; return { cleaned: true }; })()");
    } catch (cause) { failure = failure ? new AggregateError([failure, cause], "Safety probe teardown failed") : cause; }
  }
  if (failure) throw failure;
  console.log("Social safety real browser PASS", JSON.stringify(coverage));
  return coverage;
}

// Serialized into Aegis. Credentials stay in this closure and never enter probe
// output. Every request below reaches the live API; there are no canned replies.
function socialSafetyHarness({ apiUrl, marker }) {
  const check = (condition, label) => { if (!condition) throw Error(label); };
  const canonical = value => Array.isArray(value) ? value.map(canonical)
    : value && typeof value === 'object' ? Object.fromEntries(Object.keys(value).sort().map(key => [key, canonical(value[key])])) : value;
  const equal = (actual, expected, label) => check(JSON.stringify(canonical(actual)) === JSON.stringify(canonical(expected)), label);
  const required = (selector, root = document) => {
    const element = root.querySelector(selector);
    check(element, `Missing safety acceptance control: ${selector}`);
    return element;
  };
  const visible = element => !!element && !element.closest('[hidden], [inert]')
    && element.getClientRects().length > 0 && getComputedStyle(element).visibility !== 'hidden';
  const until = async (label, predicate) => {
    window.__socialSafetyStep = label;
    const deadline = Date.now() + 25000;
    while (!await predicate()) {
      if (Date.now() >= deadline) throw Error(`Timed out: ${label}`);
      await new Promise(resolve => setTimeout(resolve, 40));
    }
  };
  const click = (selector, root = document) => {
    const button = required(selector, root);
    check(visible(button) && !button.disabled, `Unavailable control: ${selector}`);
    button.click();
    return button;
  };
  const sessionKey = `babble.session.v1:${apiUrl}`;
  const savedSession = sessionStorage.getItem(sessionKey);
  const owner = JSON.parse(savedSession ?? 'null');
  check(owner?.token && owner.identity?.id, 'Parent must supply an authenticated browser');
  check(new URL(document.documentElement.dataset.babbleApi).origin === apiUrl, 'Browser and fixture API differ');
  const ownerId = owner.identity.id;
  const savedSearch = required('[data-search-input]').value;
  const savedLens = document.querySelector('[data-lens][aria-pressed="true"]')?.dataset.lens;
  check(savedLens, 'Cannot capture the parent lens');
  let target, object, source, targetController, ownerController, reply;
  let initialSnapshot, followBefore, reactionBefore, stageActive = false;
  const documents = [];
  const key = label => `${marker}:${label}:${crypto.randomUUID()}`;
  const request = async (path, { method = 'GET', body, account = owner, headers = {} } = {}) => {
    const response = await fetch(new URL(path, apiUrl), {
      method, credentials: 'omit', redirect: 'error', signal: AbortSignal.timeout(20000),
      headers: { accept: 'application/json', ...(account ? { authorization: `Bearer ${account.token}` } : {}),
        ...(body === undefined ? {} : { 'content-type': 'application/json' }), ...headers },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
    const text = await response.text();
    if (path.startsWith('/social/safety')) check(/\bno-store\b/.test(response.headers.get('cache-control') ?? ''), 'Every safety response, including errors, must be no-store');
    let value;
    try { value = text ? JSON.parse(text) : null; } catch { throw Error(`${method} ${path}: non-JSON response (${response.status})`); }
    return { status: response.status, value, cache: response.headers.get('cache-control') };
  };
  const ok = async (path, options) => {
    const response = await request(path, options);
    check(response.status >= 200 && response.status < 300, `${options?.method ?? 'GET'} ${path}: ${response.status} ${JSON.stringify(response.value)}`);
    if (path.startsWith('/social/safety')) check(/\bno-store\b/.test(response.cache ?? ''), 'Private safety response must be no-store');
    return response.value;
  };
  const safetyPath = () => `/social/safety/${target.identity.id}`;
  const safety = () => ok(safetyPath());
  const flags = (state, blocked, muted) => check(state.author_id === ownerId && state.target_id === target.identity.id
    && state.blocked === blocked && state.muted === muted && Number.isSafeInteger(state.revision), 'Unexpected authoritative safety state');
  const setSafety = async (blocked, muted) => {
    const current = await safety();
    await ok(safetyPath(), { method: 'PUT', body: { blocked, muted, expected_revision: current.revision, idempotency_key: key('safety') } });
    const fresh = await safety(); flags(fresh, blocked, muted); return fresh;
  };
  const followPath = () => `/social/following/${target.identity.id}`;
  const setFollow = async following => {
    const current = await ok(followPath());
    await ok(followPath(), { method: 'PUT', body: { following, expected_revision: current.revision, idempotency_key: key('follow') } });
    check((await ok(followPath())).following === following, 'Follow readback mismatch');
  };
  const reactionPath = () => `/objects/${object.id}/reactions/mine`;
  const emptyReaction = { appreciation: null, engagement: null, stance: null, certainty: null };
  const reaction = { ...emptyReaction, appreciation: 'like' };
  const setReaction = async value => {
    const current = await ok(reactionPath());
    await ok(reactionPath(), { method: 'PUT', body: { value, expected_revision: current.revision, idempotency_key: key('react') } });
    equal((await ok(reactionPath())).value, value, 'Reaction readback mismatch');
  };
  const rpcRequest = async (method, payload, account = owner) => {
    const response = await request('/rpc', { method: 'POST', account, body: {
      protocol: 'babble.rpc.v1', id: `safety-${crypto.randomUUID()}`, method, payload, idempotency_key: key('rpc-receipt'),
      binding: { object_id: null, surface_session_id: null, runtime_id: 'safety-browser', origin: location.origin, capability_grants: [] },
      deadline: { timeout_ms: 30000, client_started_at: null }, trace_id: null,
    } });
    check(response.status === 200, `RPC HTTP ${response.status}`);
    return response.value;
  };
  const controller = async (account, targetId) => {
    const response = await ok('/objects', { method: 'POST', account, body: { author_id: account.identity.id, draft: {
      kind: 'babble.text', schema: 'babble.schema.text.v1', payload: { text: `Safety controller ${crypto.randomUUID()}`, metadata: {} },
      provenance: { parent: null, forked_from: null, remixed_from: [] }, resources: [], surfaces: [],
      capabilities: ['follow', 'reply', 'share'].map(action => ({ id: `babble.social.${action}`, version: 1, scope: { object_id: targetId } })),
    } } });
    return response.object;
  };
  const social = async (action, account, control, targetId, blocked) => {
    const documentId = crypto.randomUUID();
    await ok(`/invocations/v1/documents/${documentId}`, { method: 'PUT', account, body: { object_id: control.id } });
    documents.push({ documentId, account });
    const headers = { 'x-babble-host-document': documentId };
    let response = await request('/invocations/v1/prepare', { method: 'POST', account, headers, body: {
      origin: { kind: 'host_action', document_id: documentId }, object_id: control.id,
      method: `babble.social.${action}`, request_key: key(action), timeout_ms: 30000,
      payload: { author_id: account.identity.id, target_object_id: targetId, ...(action === 'follow' ? {} : { text: `Safety ${action} ${crypto.randomUUID()}` }) },
    } });
    for (const phase of ['decision', 'execute']) {
      if (response.status !== 200) break;
      check(response.value.invocation_id, 'Missing real invocation');
      response = await request(`/invocations/v1/${response.value.invocation_id}/${phase}`, {
        method: 'POST', account, headers, body: phase === 'decision' ? { decision: 'allow_once' } : {},
      });
    }
    if (blocked) {
      check(response.status === 409 && /interaction unavailable/.test(response.value?.message ?? ''), `Expected safety rejection for ${action}, got ${response.status}: ${JSON.stringify(response.value)}`);
    } else {
      check(response.status === 200 && response.value.state?.kind === 'completed', `${action} must succeed without a block: ${JSON.stringify(response.value)}`);
      check(response.value.result.edge.signature, 'Social edge must be signed');
      if (action !== 'follow') {
        const published = response.value.result.object;
        const readback = await ok(`/objects/${published.id}`, { account: null });
        equal(readback.object, published, 'Social publication must survive public readback');
      }
    }
    return response.value?.result;
  };
  const closeSafety = async () => {
    const dialog = document.querySelector('[data-safety-dialog]');
    if (dialog?.open) {
      click('[data-safety-action="close"]', dialog);
      await until('safety closed', () => !dialog.open);
    }
  };
  const closeProfile = () => { if (document.querySelector('[data-public-profile]')?.open) click('[data-public-profile-close]'); };
  const menu = async () => {
    const panel = required('[data-profile-dropdown]');
    if (panel.hidden || panel.dataset.state !== 'open') click('[data-toggle-profile]');
    await until('account menu visible', () => visible(panel) && panel.dataset.state === 'open');
  };
  const dialogReady = async () => {
    await until('safety dialog ready', () => {
      const dialog = document.querySelector('[data-safety-dialog]');
      return dialog?.open && visible(dialog) && dialog.getAttribute('aria-busy') === 'false';
    });
    return required('[data-safety-dialog]');
  };
  const manage = async () => {
    await closeSafety(); closeProfile(); await menu(); click('[data-menu-action="safety"]');
    return dialogReady();
  };
  const action = async name => {
    const dialog = await dialogReady(); click(`[data-safety-action="${name}"]`, dialog);
    await dialogReady();
  };
  const feedReady = async lens => until(`${lens} feed complete`, () => {
    const status = required(lens === 'following' ? '[data-following-status]' : '[data-status]');
    check(status.dataset.state !== 'error', `Feed failed: ${status.textContent}`);
    return status.dataset.state === (lens === 'following' ? 'ready' : 'online');
  });
  const feed = async (lens, search = marker) => {
    await closeSafety(); closeProfile();
    const back = document.querySelector('[data-back-profile]');
    if (visible(back)) { click('[data-back-profile]'); closeProfile(); }
    await menu(); click('[data-menu-action="settings"]');
    await until('settings visible', () => visible(document.querySelector('[data-settings-panel]')));
    click(`[data-lens="${lens}"]`); await feedReady(lens);
    click('[data-close-settings]');
    required('[data-search-input]').value = search;
    required('[data-search-form]').requestSubmit();
    await feedReady(lens);
  };
  const active = () => document.querySelector('.post-card[data-offset="0"]:not([data-exiting])');
  const findCard = async (objectId, lens = 'balanced', search = marker) => {
    await feed(lens, search);
    const count = Number(required('[data-count-label]').textContent);
    check(count > 0, 'Fixture not in discovery');
    for (let i = 0; i < count; i++) {
      if (active()?.dataset.objectId === objectId) return active();
      const previous = active()?.dataset.objectId;
      click('[data-next]');
      await until('next discovery card', () => active()?.dataset.objectId !== previous);
    }
    throw Error(`Fixture ${objectId} absent from ${count} loaded ${lens} Objects`);
  };
  const targetCard = () => findCard(object.id);
  const openCardSafety = async () => {
    const card = await targetCard();
    const button = required('[data-action="author-controls"]', card);
    if (!visible(button)) {
      const popover = button.closest('.action-popover');
      check(popover, 'Author controls have no reachable disclosure');
      click(':scope > button', popover);
      await until('card author controls visible', () => visible(button));
    }
    button.focus(); click('[data-action="author-controls"]', card);
    await dialogReady(); return button;
  };
  const profileReady = async () => {
    await until('explicit public profile', () => document.querySelector('[data-public-profile]')?.open
      && required('[data-public-profile-status]').dataset.state === 'ready');
    check(required('[data-public-profile-identity]').textContent === target.identity.id, 'Wrong explicit profile');
    check(visible(required(`[data-profile-object-id="${object.id}"]`)), 'Safety must not hide explicit profile Objects');
  };
  const explicitProfile = async () => {
    if (document.querySelector('[data-public-profile]')?.open
      && required('[data-public-profile-identity]').textContent === target.identity.id) return profileReady();
    // Safety list rows manage state; the retained Following list is the public
    // profile entry point while feed cards are filtered out.
    await feed('following');
    click('[data-following-people]');
    await until('following people list ready', () => required('[data-following-list-status]').dataset.state === 'ready'
      && document.querySelector('[data-following-dialog]')?.open);
    click(`[data-following-author="${target.identity.id}"]`);
    await profileReady();
  };
  const filtered = async () => {
    for (const lens of ['balanced', 'research', 'weird', 'following']) {
      await feed(lens);
      const count = Number(required('[data-count-label]').textContent);
      check(Number.isSafeInteger(count) && count >= 0, `${lens} returned an invalid feed count`);
      if (lens === 'following') check(count === 0, 'Following resurrected a hidden fixture');
      // Discovery search supplies ranking candidates, including exploration.
      // Assert exclusion across the entire deck, not absence of other authors.
      for (let index = 0; index < count; index++) {
        const card = active();
        check(card?.dataset.objectId !== object.id
          && required('.post-primary [data-profile-author]', card).dataset.profileAuthor !== target.identity.id,
        `${lens} rendered hidden author on Object ${card?.dataset.objectId}`);
        if (index + 1 < count) {
          const previous = card.dataset.objectId;
          click('[data-next]');
          await until(`${lens} filtered deck navigation`, () => active()?.dataset.objectId !== previous);
        }
      }
    }
  };
  const previews = async blocked => {
    await findCard(source.id, 'balanced', source.payload.text);
    check(active()?.dataset.objectId === source.id, 'Preview fixture source is not active');
    await until('real conversation and quote responses rendered', () => {
      const card = active();
      const thread = card?.querySelector('.conversation-status');
      const quotes = card?.querySelector('.quotes-status');
      check(thread?.dataset.state !== 'error' && quotes?.dataset.state !== 'error', 'Preview request failed');
      return thread?.dataset.state === 'ready' && quotes?.dataset.state === 'ready';
    });
    const replyRow = active().querySelector(`.reply-row[data-object-id="${reply.id}"]`);
    const quote = active().querySelector(`[data-quoted-object="${object.id}"]`);
    check(blocked ? !replyRow : visible(replyRow), 'Blocked reply filtering or mute-only reply visibility is wrong');
    check(blocked ? !quote : visible(quote), 'Blocked quote filtering or mute-only quote visibility is wrong');
    const publicReplies = await rpcRequest('babble.social.replies.list.v1', { object_id: source.id, limit: 20, cursor: null });
    check(publicReplies.error === null && publicReplies.result.replies.some(item => item.object.id === reply.id), 'Public reply readback lost signed history');
    // The public graph must retain both signed relationships even while host
    // presentation filters their target author.
    const incoming = (await ok(`/graph/objects/${source.id}/incoming`, { account: null })).edges;
    const outgoing = (await ok(`/graph/objects/${source.id}/outgoing`, { account: null })).edges;
    check(incoming.some(edge => edge.source === reply.id && edge.relation === 'reply_to'), 'Public reply history was deleted');
    check(outgoing.some(edge => edge.target === object.id && edge.relation === 'quotes'), 'Public quote history was deleted');
  };
  const readGraph = async () => (await ok(`/graph/objects/${object.id}/incoming`, { account: null })).edges;
  const edgePayload = (account = owner, from = source.id, to = object.id, relation = 'references') => ({
    author_id: account.identity.id, source: from, target: to, relation, origin: 'HumanAssertion',
  });
  const graphWrite = async (blocked, account = owner, from = source.id, to = object.id) => {
    for (const relation of ['references', 'reply_to', 'quotes', 'follows']) {
      const payload = edgePayload(account, from, to, relation);
      const rest = await request('/graph/edges', { method: 'POST', account, body: payload });
      const rpc = await rpcRequest('babble.graph.edge.publish.v1', payload, account);
      if (blocked) {
        check(rest.status === 409 && /interaction unavailable/.test(rest.value?.message ?? ''), `REST ${relation} bypassed block or failed for another reason`);
        check(rpc.result === null && rpc.error?.code === 'CONFLICT' && /interaction unavailable/.test(rpc.error.message), `RPC ${relation} did not reject for safety: ${JSON.stringify(rpc)}`);
      } else {
        check(rest.status === 200 && rest.value.edge?.signature && rpc.error === null && rpc.result?.edge?.signature, `${relation} control write failed`);
        const readback = await ok(`/graph/edges/${rpc.result.edge.id}`, { account: null });
        equal(readback.edge, rpc.result.edge, 'Signed edge readback mismatch');
      }
    }
  };
  // Same-origin frames load the production page and CSS. This measures responsive
  // dialog layout only; it is not a physical-device or touch-gesture test.
  const layout = async width => {
    const before = await safety();
    const height = width === 320 ? 568 : 844;
    const frame = document.createElement('iframe');
    frame.title = `Safety dialog layout verification at ${width}px`;
    frame.style.cssText = `position:fixed;left:0;top:0;width:${width}px;height:${height}px;border:0;z-index:9999`;
    const url = new URL(location.href);
    url.searchParams.delete('surface');
    url.searchParams.set('lens', 'balanced'); url.searchParams.set('q', marker);
    frame.src = url.href; document.body.append(frame);
    const diagnostics = () => {
      const doc = frame.contentDocument;
      return { width, url: doc?.URL, readyState: doc?.readyState,
        status: doc?.querySelector('[data-status]')?.dataset.state,
        authorStatus: doc?.querySelector('[data-author-status]')?.textContent,
        source: doc?.querySelector('[data-source-label]')?.textContent,
        count: doc?.querySelector('[data-count-label]')?.textContent,
        summary: doc?.querySelector('[data-feed-summary]')?.textContent,
        local: doc?.querySelector('[data-local-label]')?.textContent,
        expectedObjectId: object.id, expectedAuthorId: target.identity.id,
        cards: [...doc?.querySelectorAll('.post-card:not([data-exiting])') ?? []].map(card => ({
          id: card.dataset.objectId, offset: card.dataset.offset, inert: card.inert,
          author: card.querySelector('[data-profile-author]')?.dataset.profileAuthor,
        })), body: doc?.body?.innerText.slice(0, 6000) };
    };
    try {
      await until(`production frame ${width} ready`, () => frame.contentDocument?.querySelector('[data-status]')?.dataset.state === 'online'
        && frame.contentDocument?.querySelector('.post-card[data-offset="0"]:not([data-exiting]) .post-primary'));
      const doc = frame.contentDocument, win = frame.contentWindow;
      await doc.fonts.ready;
      check(win.innerWidth === width, 'Responsive frame width differs from requested viewport');
      const frameSession = JSON.parse(win.sessionStorage.getItem(sessionKey) ?? 'null');
      check(frameSession?.identity.id === ownerId && frameSession?.token === owner.token
        && required('[data-author-status]', doc).dataset.state === 'ready'
        && required('[data-author-status]', doc).textContent === `Signed in as ${owner.identity.handle}`,
      'Responsive frame must retain the authenticated parent account');
      const shown = node => !!node && !node.closest('[hidden], [inert]') && node.getClientRects().length > 0
        && win.getComputedStyle(node).visibility !== 'hidden';
      const press = (selector, root = doc) => {
        const node = required(selector, root);
        check(shown(node) && !node.disabled, `Frame ${width}: unavailable ${selector}`);
        node.click();
      };
      // Seen history is shared across same-origin frames and affects ranking.
      // Find the fixture through real deck navigation instead of fixing its rank.
      const frameActive = () => doc.querySelector('.post-card[data-offset="0"]:not([data-exiting])');
      const count = Number(required('[data-count-label]', doc).textContent);
      check(Number.isSafeInteger(count) && count > 0, 'Responsive frame must render a nonempty ranked feed');
      const visited = [];
      for (let index = 0; index < count; index++) {
        await until(`frame ${width} active card rendered`, () => shown(frameActive()?.querySelector('.post-primary')) && !frameActive()?.inert);
        const card = frameActive();
        visited.push(card.dataset.objectId);
        if (card.dataset.objectId === object.id) break;
        if (index + 1 < count) {
          const previous = card.dataset.objectId;
          press('[data-next]');
          await until(`frame ${width} next ranked card`, () => frameActive()?.dataset.objectId !== previous
            && shown(frameActive()?.querySelector('.post-primary')) && !frameActive()?.inert);
        }
      }
      check(frameActive()?.dataset.objectId === object.id, `Responsive fixture absent from ${count} loaded Objects; visited ${JSON.stringify(visited)}`);
      check(required('[data-profile-author]', frameActive()).dataset.profileAuthor === target.identity.id,
        'Responsive fixture must belong to the target account');
      check(required('[data-status]', doc).dataset.state === 'online', 'Responsive frame must remain online after navigation');
      const ready = () => until(`frame ${width} safety dialog ready`, () => {
        const node = doc.querySelector('[data-safety-dialog]');
        return node?.open && shown(node) && node.getAttribute('aria-busy') === 'false';
      });
      const measure = async state => {
        await ready();
        const dialog = required('[data-safety-dialog]', doc);
        await Promise.all(dialog.getAnimations({ subtree: true }).map(animation => animation.finished));
        await new Promise(resolve => win.requestAnimationFrame(() => win.requestAnimationFrame(resolve)));
        const box = dialog.getBoundingClientRect(), style = win.getComputedStyle(dialog);
        const context = `${width}px/${state}`;
        check(box.width > 0 && box.height > 0 && box.left >= -1 && box.right <= width + 1
          && box.top >= -1 && box.bottom <= height + 1, `Dialog escapes viewport: ${context}`);
        check(dialog.scrollWidth <= dialog.clientWidth + 1, `Dialog horizontal overflow: ${context}`);
        const padding = ['Top', 'Right', 'Bottom', 'Left'].map(side => parseFloat(style[`padding${side}`]));
        const radii = ['TopLeft', 'TopRight', 'BottomRight', 'BottomLeft'].map(corner => parseFloat(style[`border${corner}Radius`]));
        check(padding.every(value => value >= 16) && radii.every(value => value >= 16), `Dialog must retain rounded, padded production styling: ${context}`);
        const controls = [...dialog.querySelectorAll('button, a[href], input, select, textarea')].filter(shown);
        check(controls.length > 0, `No interactive controls measured: ${context}`);
        const sizes = controls.map(node => {
          const rect = node.getBoundingClientRect();
          check(rect.width >= 43.9 && rect.height >= 43.9, `Interactive target below 44px: ${context}/${node.getAttribute('aria-label')}`);
          check(rect.left >= box.left && rect.right <= box.right && node.scrollWidth <= node.clientWidth + 1
            && node.scrollHeight <= node.clientHeight + 1, `Control or its text overflows: ${context}`);
          return { width: rect.width, height: rect.height };
        });
        const walker = doc.createTreeWalker(dialog, 4);
        for (let text = walker.nextNode(); text; text = walker.nextNode()) {
          if (!text.textContent.trim() || !shown(text.parentElement)) continue;
          const range = doc.createRange(); range.selectNodeContents(text);
          const parent = text.parentElement.getBoundingClientRect();
          for (const line of range.getClientRects()) {
            check(line.left >= Math.max(box.left, parent.left) - 1 && line.right <= Math.min(box.right, parent.right) + 1,
              `Text escapes its container: ${context}/${text.textContent.slice(0, 60)}`);
          }
        }
        const title = required('.safety-header h2', dialog).getBoundingClientRect();
        const close = required('[data-safety-action="close"]', dialog).getBoundingClientRect();
        check(title.right <= close.left + 1, `Dialog heading overlaps close control: ${context}`);
        return { state, padding, radii, controls: sizes };
      };
      press('[data-toggle-profile]');
      await until(`frame ${width} account menu`, () => required('[data-profile-dropdown]', doc).dataset.state === 'open');
      press('[data-menu-action="safety"]');
      const measured = [await measure('management')];
      press('[data-safety-action="close"]');
      const card = required(`.post-card[data-offset="0"][data-object-id="${object.id}"]`, doc);
      const authorControls = required('[data-action="author-controls"]', card);
      if (!shown(authorControls)) {
        press(':scope > button', required('[data-action="author-controls"]', card).closest('.action-popover'));
        await until(`frame ${width} author launcher`, () => shown(authorControls));
      }
      press('[data-action="author-controls"]', card);
      measured.push(await measure('author'));
      press('[data-safety-action="block"]');
      check(shown(required('[data-safety-action="confirm-block"]', doc)), 'Frame must show real block confirmation');
      measured.push(await measure('confirmation'));
      press('[data-safety-action="cancel-block"]'); press('[data-safety-action="close"]');
      equal(await safety(), before, 'Layout inspection changed safety state');
      return { verified: true, width, height, measured, scope: 'Production iframe layout; not touch E2E' };
    } catch (cause) {
      throw Error(`${cause instanceof Error ? cause.message : String(cause)}; frame: ${JSON.stringify(diagnostics())}`);
    } finally { frame.remove(); }
  };
  const stages = {
    'layout-320': () => layout(320),
    'layout-390': () => layout(390),
    'layout-1280': () => layout(1280),
    async setup() {
      for (const selector of ['[data-account-close]', '[data-close-surface]', '[data-close-composer]', '[data-close-help]', '[data-close-settings]', '[data-close-judgments]']) {
        if (visible(document.querySelector(selector))) click(selector);
      }
      initialSnapshot = await ok('/social/safety');
      check(initialSnapshot.author_id === ownerId, 'Snapshot belongs to another account');
      target = await ok('/auth/register', { method: 'POST', account: null, body: {
        handle: marker, kind: 'Person', password: `${marker}-fixture-password!`,
      } });
      object = (await ok('/objects/text', { method: 'POST', account: target, body: { author_id: target.identity.id, text: marker } })).object;
      source = (await ok('/objects/text', { method: 'POST', body: { author_id: ownerId, text: `Safety source ${crypto.randomUUID()}` } })).object;
      check(object.author === target.identity.id && object.signature && source.signature, 'Fixtures must be real signed Objects');
      ownerController = await controller(owner, object.id);
      targetController = await controller(target, source.id);
      reply = (await social('reply', target, targetController, source.id, false)).object;
      await ok('/graph/edges', { method: 'POST', body: edgePayload(owner, source.id, object.id, 'quotes') });
      flags(await safety(), false, false);
      check((await safety()).revision === 0, 'Fixture must start untouched');
      followBefore = await ok(followPath()); reactionBefore = await ok(reactionPath());
      await setFollow(true);
      await feed('following');
      check(Number(required('[data-count-label]').textContent) === 1 && active()?.dataset.objectId === object.id, 'Following control must contain fixture before filtering');
      return { ownerId, targetId: target.identity.id, objectId: object.id };
    },
    async cancel() {
      const before = await safety();
      const snapshot = await ok('/social/safety');
      await manage(); await closeSafety();
      equal(await ok('/social/safety'), snapshot, 'Opening management changed private state');
      const opener = await openCardSafety();
      await action('block');
      check(visible(required('[data-safety-action="confirm-block"]')), 'Block requires explicit confirmation');
      check(/node|account/i.test(required('[data-safety-dialog]').textContent), 'Block confirmation must explain scope');
      equal(await safety(), before, 'Opening confirmation changed backend state');
      await action('cancel-block'); await closeSafety();
      equal(await safety(), before, 'Cancelling block changed backend state');
      equal(await ok('/social/safety'), snapshot, 'Cancel advanced snapshot revision');
      check(document.activeElement === opener, 'Safety close must restore focus');
      return { verified: true, management: true, cardLauncher: true, cancelNoWrite: true, focusRestored: true };
    },
    async mute() {
      await openCardSafety(); await action('mute');
      await until('mute persisted', async () => (await safety()).muted);
      flags(await safety(), false, true);
      const snapshot = await ok('/social/safety');
      const entry = snapshot.entries.find(entry => entry.identity.id === target.identity.id);
      check(entry?.identity.handle === target.identity.handle, 'Snapshot must contain authoritative identity');
      equal(entry.state, await safety(), 'Snapshot pair differs from current pair');
      await manage();
      const row = required(`[data-safety-list] [data-safety-author="${target.identity.id}"]`);
      check(row.textContent.includes(target.identity.handle) && visible(required('[data-safety-action="unmute"]', row)), 'Management must show the real muted account');
      await filtered();
      await explicitProfile();
      click('[data-author-controls]'); await dialogReady();
      check(visible(required('[data-safety-action="unmute"]')), 'Profile controls must reflect persisted mute');
      await closeSafety();
      click(`[data-profile-object-id="${object.id}"]`);
      await until('explicit muted Object', () => active()?.dataset.objectId === object.id);
      await filtered();
      check((await ok(followPath())).following, 'Mute must retain follow');
      return { verified: true, profileLauncher: true, filteredLenses: ['balanced', 'research', 'weird', 'following'], explicitProfileAndObject: true, cachedReturn: true };
    },
    async 'muted-writes'() {
      await setReaction(reaction);
      await graphWrite(false);
      for (const action of ['reply', 'share', 'follow']) await social(action, owner, ownerController, object.id, false);
      await previews(false);
      return { verified: true, mutedPreviewsVisible: true, realWritesAllowed: ['reaction', 'reply', 'share', 'follow', 'REST edges', 'RPC edges'] };
    },
    async block() {
      await explicitProfile(); click('[data-author-controls]'); await dialogReady();
      await action('block'); await action('confirm-block');
      await until('block persisted', async () => (await safety()).blocked);
      flags(await safety(), true, true);
      await filtered(); await previews(true); await explicitProfile();
      check((await ok(followPath())).following, 'Blocking must retain pre-existing follow');
      equal((await ok(reactionPath())).value, reaction, 'Blocking must preserve reaction history');
      return { verified: true, blockConfirmed: true, historyPreserved: true, explicitProfileVisible: true, blockedRepliesAndQuotesHidden: true };
    },
    async 'blocked-writes'() {
      const before = await readGraph();
      const objectsBefore = await ok(`/identities/${ownerId}/objects?limit=50`, { account: null });
      for (const [account, targetId, control] of [[owner, object.id, ownerController], [target, source.id, targetController]]) {
        for (const action of ['reply', 'share', 'follow']) await social(action, account, control, targetId, true);
        const path = `/objects/${targetId}/reactions/mine`;
        const current = await ok(path, { account });
        const denied = await request(path, { method: 'PUT', account, body: {
          value: { ...emptyReaction, appreciation: 'dislike' }, expected_revision: current.revision, idempotency_key: key('blocked-reaction'),
        } });
        check(denied.status === 409 && /interaction unavailable/.test(denied.value?.message ?? ''), 'Reaction must reject for safety');
        equal(await ok(path, { account }), current, 'Rejected reaction changed state');
        const followingPath = `/social/following/${account === owner ? target.identity.id : ownerId}`;
        const state = await ok(followingPath, { account });
        const result = await request(followingPath, { method: 'PUT', account, body: {
          following: true, expected_revision: state.revision, idempotency_key: key('blocked-follow'),
        } });
        check(result.status === 409 && /interaction unavailable/.test(result.value?.message ?? ''), 'Following must reject for safety');
        equal(await ok(followingPath, { account }), state, 'Rejected follow changed state');
      }
      await graphWrite(true); await graphWrite(true, target, object.id, source.id);
      equal(await readGraph(), before, 'Rejected writes added target graph edges');
      equal(await ok(`/identities/${ownerId}/objects?limit=50`, { account: null }), objectsBefore, 'Rejected writes published Objects');
      await setReaction(emptyReaction); await setFollow(false);
      return { verified: true, directions: ['owner-to-target', 'target-to-owner'], denied: ['reply', 'share', 'reaction', 'follow', 'REST edges', 'RPC edges'], removalAllowed: true };
    },
    async restore() {
      await closeSafety(); await explicitProfile();
      click('[data-author-controls]'); await dialogReady(); await action('unblock');
      await until('unblock persisted', async () => !(await safety()).blocked);
      flags(await safety(), false, true);
      await filtered();
      await setFollow(true); await setReaction(reaction);
      for (const action of ['reply', 'share', 'follow']) await social(action, owner, ownerController, object.id, false);
      await previews(false);
      await manage();
      click('[data-safety-action="unmute"]', required(`[data-safety-list] [data-safety-author="${target.identity.id}"]`));
      await dialogReady();
      await until('unmute persisted', async () => !(await safety()).muted);
      flags(await safety(), false, false);
      for (const lens of ['balanced', 'research', 'weird', 'following']) {
        await findCard(object.id, lens);
        if (lens === 'following') check(Number(required('[data-count-label]').textContent) === 1, 'Following did not restore exactly the matching fixture');
      }
      await manage();
      check(!document.querySelector(`[data-safety-author="${target.identity.id}"]`), 'Cleared target remains in management');
      await closeSafety();
      return { verified: true, independentUnblock: true, unmuteRestoresAllLenses: true, writesRestored: true, previewsRestored: true, manageUnmute: true };
    },
    async receipts() {
      const before = await safety();
      const snapshot = await ok('/social/safety');
      const intent = { blocked: false, muted: true, expected_revision: before.revision, idempotency_key: key('stable-retry') };
      const accepted = await ok(safetyPath(), { method: 'PUT', body: intent });
      flags(accepted, false, true);
      check(accepted.revision === before.revision + 1, 'Mutation must advance pair revision once');
      const mutated = await ok('/social/safety');
      check(mutated.revision === snapshot.revision + 1, 'Mutation must advance owner revision once');
      equal(await ok(safetyPath(), { method: 'PUT', body: intent }), accepted, 'Exact retry must return its original receipt');
      equal(await ok('/social/safety'), mutated, 'Exact retry changed snapshot');
      const changed = await request(safetyPath(), { method: 'PUT', body: { ...intent, muted: false } });
      check(changed.status === 409, 'Changed intent reused a receipt');
      const stale = await request(safetyPath(), { method: 'PUT', body: { ...intent, muted: false, idempotency_key: key('stale-cas') } });
      check(stale.status === 409, 'Stale CAS accepted');
      equal(await safety(), accepted, 'Conflict modified current state');
      const cleared = await setSafety(false, false);
      equal(await ok(safetyPath(), { method: 'PUT', body: intent }), accepted, 'Old retry lost original receipt');
      equal(await safety(), cleared, 'Old retry resurrected mute');
      const noChangeSnapshot = await ok('/social/safety');
      await setSafety(false, false);
      equal(await ok('/social/safety'), noChangeSnapshot, 'No-op advanced snapshot revision');
      return { verified: true, exactReplay: true, changedIntentRejected: true, staleCASRejected: true, oldReceiptCannotResurrect: true };
    },
    async privacy() {
      const paths = [`/objects/${object.id}`, `/identities/${target.identity.id}`, `/graph/objects/${object.id}/incoming`];
      const before = [];
      for (const path of paths) before.push(await ok(path, { account: null }));
      await setSafety(true, true);
      for (const [path, method, body] of [['/social/safety', 'GET'], [safetyPath(), 'GET'], [safetyPath(), 'PUT', {
        blocked: false, muted: false, expected_revision: (await safety()).revision, idempotency_key: key('guest'),
      }]]) {
        const response = await request(path, { method, body, account: null });
        check(response.status === 401, `${method} ${path} must require authentication`);
        check(!JSON.stringify(response.value).includes(ownerId), 'Guest response leaked owner');
      }
      const targetSnapshot = await ok('/social/safety', { account: target });
      check(targetSnapshot.author_id === target.identity.id && targetSnapshot.entries.length === 0 && targetSnapshot.revision === 0, 'Private state leaked to target account');
      const reverse = await ok(`/social/safety/${ownerId}`, { account: target });
      check(reverse.author_id === target.identity.id && !reverse.blocked && !reverse.muted && reverse.revision === 0, 'Reverse private state exposed owner block');
      for (const [index, path] of paths.entries()) {
        const readback = await ok(path, { account: null });
        equal(readback, before[index], 'Private mutations changed public graph/Object/identity');
        check(!/"(?:blocked|muted|safety_actions|safety_receipts|safety_states)"/.test(JSON.stringify(readback)), 'Private state leaked in public data');
      }
      await setSafety(false, false);
      return { verified: true, authenticatedRoutes: true, ownerIsolation: true, noStore: true, publicGraphUnchanged: true,
        exportExclusion: 'Not exercised: /events and /events/bundle require operator credentials; backend suite owns export/import coverage.' };
    },
    async cleanup() {
      await closeSafety(); closeProfile();
      for (const { documentId, account } of documents) await ok(`/invocations/v1/documents/${documentId}`, { method: 'DELETE', account });
      if (target) {
        await setSafety(false, false);
        if (followBefore) await setFollow(followBefore.following);
        if (reactionBefore) await setReaction(reactionBefore.value);
        const snapshot = await ok('/social/safety');
        equal(snapshot.entries, initialSnapshot.entries, 'Cleanup changed unrelated safety entries');
      }
      await feed(savedLens, savedSearch);
      check(sessionStorage.getItem(sessionKey) === savedSession, 'Parent browser login was changed');
      const session = await ok('/auth/session');
      check(session.identity.id === ownerId, 'Parent login is no longer valid');
      if (target) await ok('/auth/session', { method: 'DELETE', account: target });
      return { verified: true, parentSessionPreserved: true, originalFeedRestored: true };
    },
  };
  return {
    get busy() { return stageActive; },
    async run(stage) {
      check(!stageActive, 'Previous safety stage is still running; refusing concurrent cleanup');
      check(Object.hasOwn(stages, stage), `Unknown safety stage: ${stage}`);
      stageActive = true;
      try { return await stages[stage](); } finally { stageActive = false; }
    },
  };
}

// Browser-free negative orchestration checks. These exercise probe serialization,
// failure propagation and cleanup; they do NOT simulate safety acceptance.
if (process.argv[2] === '--contract-check') {
  const { Script, createContext, runInContext } = await import('node:vm');
  const id = `id_${'1'.repeat(64)}`, targetId = `id_${'2'.repeat(64)}`, objectId = `obj_${'3'.repeat(64)}`;
  for (const failedStage of ['setup', 'cancel', 'layout-320', 'layout-390', 'layout-1280', 'mute', 'muted-writes', 'block', 'blocked-writes', 'restore', 'receipts', 'privacy']) {
    const calls = [];
    const context = createContext({ window: {} });
    const stub = { busy: false, async run(stage) {
      calls.push(stage);
      if (stage === failedStage) throw Error(`injected:${stage}`);
      return stage === 'setup' ? { ownerId: id, targetId, objectId } : { verified: true };
    } };
    const execute = async commands => {
      assert.equal(commands.length, 1);
      const { code } = commands[0];
      new Script(code);
      if (code.includes('window.__socialSafetyHarness = (function')) {
        context.window.__socialSafetyHarness = stub;
        return { results: [{ ok: true, value: { installed: true } }] };
      }
      const value = runInContext(code, context);
      assert.equal(typeof value, 'object', 'Probe must return an object, never primitive eval output');
      return { results: [{ ok: true, value }] };
    };
    const waitFor = async (code, predicate) => {
      for (let i = 0; i < 20; i++) {
        await new Promise(resolve => setImmediate(resolve));
        const value = runInContext(code, context);
        assert.equal(typeof value, 'object');
        if (predicate(value)) return value;
      }
      throw Error('Orchestration probe did not settle');
    };
    await assert.rejects(verifySocialSafety({ execute, waitFor, authorId: id, apiUrl: 'http://127.0.0.1:18787',
      rpc: async () => ({ results: [{ object: { id: objectId } }] }),
    }), error => error.message.includes(`injected:${failedStage}`));
    assert.equal(calls.at(-1), 'cleanup');
    assert.equal(context.window.__socialSafetyHarness, undefined);
  }
  console.log('Safety harness negative orchestration PASS (no browser/API evidence)');
}
