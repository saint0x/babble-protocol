import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";
import { assertLiveStackIsolation } from "./live-stack-isolation.mjs";

const stages = ["fixtures", "baseline", "private", "lenses", "empty", "following"];

export function assertFeedDiversityIsolation(config) {
  return assertLiveStackIsolation(config, "BABBLE_FEED_DIVERSITY_SOURCE_FROZEN");
}

// These are actual HTTP bodies from the transparent acceptance proxy, never
// reconstructed client arguments. Keep parsing fail-closed when capture is lost.
export function decodeFeedDiscovery(snapshot, privateMarker) {
  const check = (value, message) => { if (!value) throw Error(message); };
  check(!snapshot.captureError, `HTTP observation failed: ${snapshot.captureError}`);
  check(Number.isSafeInteger(snapshot.cursor) && Array.isArray(snapshot.requests), 'Invalid HTTP observation');
  const calls = snapshot.requests.filter(request => request.method === 'POST' && new URL(request.url).pathname === '/rpc').map(request => {
    check(typeof request.requestBody === 'string', 'Missing observed RPC request body');
    return { request, envelope: JSON.parse(request.requestBody) };
  }).filter(({ envelope }) => envelope.method === 'babble.discovery.candidates.v1');
  check(calls.length === 1, `Expected one actual discovery HTTP request, got ${calls.length}`);
  const { request, envelope } = calls[0];
  check(request.method === 'POST' && request.status === 200 && !request.error, 'Discovery HTTP request failed or incomplete');
  check(!request.requestBody.toLowerCase().includes(privateMarker.toLowerCase()), 'Private preference leaked on discovery wire');
  check(typeof request.responseBody === 'string', 'Missing observed discovery response body');
  const response = JSON.parse(request.responseBody);
  check(!response.error && response.result?.discovery, 'Observed discovery RPC failed or omitted its result');
  return { payload: envelope.payload, response: response.result };
}

export async function verifyFeedDiversity({ execute, waitFor, readRequests, apiUrl, identityId, log = console.log }) {
  assert.equal(typeof readRequests, "function", "Real HTTP observation callback required");
  assert.equal(apiUrl, "http://127.0.0.1:18787");
  assert.match(identityId, /^id_[a-f0-9]{64}$/);
  const evaluate = async code => {
    const response = await execute([{ type: "eval", code }]);
    assert.equal(response.results?.[0]?.ok, true, "Aegis feed-diversity evaluation failed");
    const value = response.results[0].value;
    assert.ok(value && typeof value === "object", "Aegis probes must return objects");
    return value;
  };
  const run = async stage => {
    await evaluate(`(() => {
      window.__feedDiversityResult = null;
      Promise.resolve().then(() => window.__feedDiversityHarness.run(${JSON.stringify(stage)}))
        .then(result => { window.__feedDiversityResult = { result }; })
        .catch(error => { window.__feedDiversityResult = { error: String(error), step: window.__feedDiversityStep }; });
      return { started: true };
    })()`);
    while (true) {
      const snapshot = await waitFor("({ outcome: window.__feedDiversityResult, step: window.__feedDiversityStep, network: window.__feedDiversityNetworkRead })",
        value => value?.outcome != null || value?.network != null);
      if (snapshot.network && !snapshot.outcome) {
        let reply;
        try {
          const value = await readRequests({ since: snapshot.network.since });
          assert.ok(!value.captureError, `HTTP observation failed: ${value.captureError}`);
          assert.ok(Number.isSafeInteger(value.cursor) && value.cursor >= snapshot.network.since && Array.isArray(value.requests),
            "Invalid HTTP observation cursor or requests");
          reply = { value: snapshot.network.cursorOnly ? { cursor: value.cursor, requests: [] } : value };
        } catch (error) { reply = { error: String(error) }; }
        try {
          await evaluate(`window.__feedDiversityHarness.receiveNetwork(${JSON.stringify(snapshot.network.id)}, ${JSON.stringify(reply)}); ({ delivered: true })`);
        } catch (error) {
          // Settle the pending stage before the loop observes its error and starts
          // cleanup. A failed large delivery must not leave a live mailbox behind.
          const rejected = { error: `HTTP observation delivery failed: ${String(error).slice(0, 1000)}` };
          await evaluate(`window.__feedDiversityHarness.receiveNetwork(${JSON.stringify(snapshot.network.id)}, ${JSON.stringify(rejected)}); ({ delivered: true })`);
        }
        continue;
      }
      assert.equal(snapshot.outcome.error, undefined, JSON.stringify(snapshot));
      assert.equal(snapshot.outcome.result?.verified, true, `${stage} did not complete`);
      return snapshot.outcome.result;
    }
  };
  const coverage = {};
  let failure;
  await evaluate(`(() => {
    if (window.__feedDiversityHarness) throw Error('Feed diversity harness already installed');
    window.__feedDiversityHarness = (${feedDiversityHarness.toString()})(${JSON.stringify({
      apiUrl, identityId, marker: `FeedAcceptance${randomUUID().replaceAll("-", "")}`,
    })}, ${decodeFeedDiscovery.toString()});
    return { installed: true };
  })()`);
  try {
    for (const stage of stages) coverage[stage] = await run(stage);
  } catch (error) { failure = error; }
  try { await run("cleanup"); }
  catch (error) { failure = failure ? new AggregateError([failure, error], "Acceptance and cleanup failed") : error; }
  finally {
    await evaluate("delete window.__feedDiversityHarness; delete window.__feedDiversityResult; delete window.__feedDiversityStep; delete window.__feedDiversityNetworkRead; ({ cleaned: true })");
  }
  if (failure) throw failure;
  log("Feed diversity browser PASS", JSON.stringify(coverage));
  return coverage;
}

// This closure observes the compiled app through its UI, persisted preferences,
// and actual HTTP traffic. Auxiliary modules provide the canonical SDK oracle;
// they never replace or patch the application's bundled classes.
function feedDiversityHarness({ apiUrl, identityId, marker }, decodeDiscovery) {
  const check = (value, message) => { if (!value) throw Error(message); };
  const equal = (actual, expected, message) => check(JSON.stringify(actual) === JSON.stringify(expected),
    `${message}: ${JSON.stringify({ actual, expected })}`);
  const required = (selector, root = document) => {
    const node = root.querySelector(selector);
    check(node, `Missing control: ${selector}`); return node;
  };
  const click = (selector, root = document) => {
    const node = required(selector, root);
    check(!node.disabled && !node.closest('[hidden], [inert]') && node.getClientRects().length > 0,
      `Unavailable control: ${selector}`);
    node.click();
  };
  const until = async (label, predicate) => {
    window.__feedDiversityStep = label;
    const deadline = Date.now() + 45000;
    while (!predicate()) {
      check(Date.now() < deadline, `Timed out: ${label}; ${document.querySelector('[data-error]')?.textContent ?? ''}`);
      await new Promise(resolve => setTimeout(resolve, 35));
    }
  };
  const origin = new URL(apiUrl).origin;
  const sessionKey = `babble.session.v1:${origin}`;
  const session = JSON.parse(sessionStorage.getItem(sessionKey) ?? "null");
  check(session?.identity.id === identityId, "Expected live-stack's ordinary browser account");
  const accounts = [], objects = [], edges = [];
  const privateMarker = `PrivateOnly${marker}`;
  let sdk, preferencesModule, dominant, baselineIds, oldFollow, initialized = false;
  let networkSerial = 0, networkCursor = 0, pendingNetwork = null;
  const network = since => new Promise((resolve, reject) => {
    check(!pendingNetwork, "Overlapping HTTP observation requests");
    const id = ++networkSerial;
    pendingNetwork = { id, resolve, reject };
    window.__feedDiversityNetworkRead = { id, since: since ?? networkCursor, cursorOnly: since === undefined };
  });
  const originalQuery = required('[data-search-input]').value;
  const originalLens = new URL(location.href).searchParams.get('lens') ?? 'balanced';
  const api = async (path, { account = session, method = 'GET', body } = {}) => {
    const response = await fetch(new URL(path, apiUrl), {
      method, credentials: 'omit', signal: AbortSignal.timeout(30000),
      headers: { ...(account ? { authorization: `Bearer ${account.token}` } : {}),
        ...(body === undefined ? {} : { 'content-type': 'application/json' }) },
      ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    });
    check(response.ok, `${method} ${path}: ${response.status}`);
    return response.json();
  };
  const field = name => required(`[data-preference-field="${name}"]`);
  const tab = name => click(`[data-preference-tab="${name}"]`);
  const open = async () => {
    if (required('[data-settings-panel]').dataset.state === 'open') return;
    click('[data-toggle-profile]');
    await until('account menu', () => required('[data-profile-dropdown]').dataset.state === 'open');
    click('[data-menu-action="settings"]');
    await until('settings panel', () => required('[data-settings-panel]').dataset.state === 'open');
  };
  const close = async () => {
    click('[data-close-settings]');
    await until('settings closed', () => required('[data-settings-panel]').hidden);
  };
  const modelSnapshot = () => {
    const key = kind => `babble.local:${JSON.stringify([origin, identityId, kind])}`;
    const parsed = preferencesModule.readPreferences(localStorage, key('preferences'));
    check(!parsed.warning, `Unreadable saved preferences: ${parsed.warning}`);
    const p = parsed.preferences, query = required('[data-search-input]').value.trim();
    const mode = new URL(location.href).searchParams.get('lens') ?? 'balanced';
    const defaults = mode === 'research' ? [.45, .35, .85, .65]
      : mode === 'weird' ? [.9, .92, .35, .85] : [.55, .45, .6, .55];
    return {
      model_revision: 'browser-local-v1', interests: [...p.interests, ...(query ? [query] : [])],
      expertise: p.expertise, muted_terms: p.mutedTerms, hidden_terms: p.hiddenTerms,
      hidden_authors: p.hiddenAuthors, creator_affinity: p.creatorAffinity,
      seen_objects: JSON.parse(localStorage.getItem(key('seen')) ?? '{}'),
      novelty_tolerance: p.noveltyTolerance ?? defaults[0], exploration_preference: p.explorationPreference ?? defaults[1],
      evidence_preference: p.evidencePreference ?? defaults[2], contradiction_tolerance: p.contradictionTolerance ?? defaults[3],
    };
  };
  const refresh = async action => {
    const { cursor } = await network();
    action();
    // Apply saves synchronously; loadFeed awaits safety before it reads the model.
    // Snapshot before asynchronous discovery renders and records its selected card.
    const model = modelSnapshot();
    await until('compiled feed render completed', () => required('[data-status]').dataset.state === 'online');
    const observation = await network(cursor);
    const discovery = decodeDiscovery(observation, privateMarker);
    return { model, discovery };
  };
  const search = query => refresh(() => {
    required('[data-search-input]').value = query;
    required('[data-search-form]').requestSubmit();
  });
  const lens = async name => {
    await open();
    if (required(`[data-lens="${name}"]`).getAttribute('aria-pressed') === 'true') return search(required('[data-search-input]').value);
    return refresh(() => click(`[data-lens="${name}"]`));
  };
  const apply = () => refresh(() => click('[data-preferences-apply]'));
  const reset = async () => {
    await open(); tab('local-data'); click('[data-preferences-reset]');
    return refresh(() => click('[data-preferences-confirm="reset"]'));
  };
  const active = () => document.querySelector('.post-card[data-offset="0"]:not([data-exiting])');
  const signedPercent = value => { const n = Math.round(value * 100); return `${n > 0 ? '+' : ''}${n}%`; };
  const inspect = async count => {
    await close();
    const position = () => required('[data-position-label]').textContent;
    // Apply can preserve a surviving active card. Observe rank one independently
    // of the SDK expectation, then traverse the actual carousel's full page.
    let remaining = count;
    while (position() !== `1/${count}`) {
      check(remaining-- > 0, 'Carousel could not return to its first position');
      const previous = position(); click('[data-prev]');
      await until('return to first final position', () => position() !== previous);
    }
    const observed = [];
    for (let index = 0; index < count; index++) {
      await until(`card position ${index + 1}`, () => position() === `${index + 1}/${count}` && !!active());
      const card = active(), manifest = JSON.parse(required('.post-manifest pre', card).textContent);
      equal(manifest.id, card.dataset.objectId, 'Inspector object must match active card');
      const score = [...card.querySelectorAll('span')].find(node => node.textContent === 'Score')?.nextElementSibling?.textContent;
      check(score !== undefined, 'Rendered analytics score is missing');
      observed.push({ ...manifest, score });
      if (index < count - 1) click('[data-next]');
    }
    await open();
    return observed;
  };
  const verify = async (load, { hidden = false, empty = false } = {}) => {
    const { payload, response } = load.discovery;
    equal(Object.keys(payload).sort(), ['anchors', 'exploration_slots', 'followed_objects', 'lens', 'limit', 'search'], 'Public request allowlist');
    equal(payload.anchors, [], 'UI public anchors'); equal(payload.followed_objects, [], 'UI public followed objects');
    equal(payload.limit, 200, 'Private feeds retrieve bounded candidate pool');
    const d = response.discovery;
    check(d.ranked.length <= 200, 'Public pool exceeds bound');
    equal(d.objects.map(o => o.id), d.ranked.map(e => e.candidate.object_id), 'Object and ranked pool alignment');
    const expected = sdk.personalizeFeed(load.model, d.ranked, d.objects.map(sdk.summarizeDiscoveryObject), d.diversity_trace.policy, 9);
    const final = expected.diversity_trace;
    const count = Number(required('[data-count-label]').textContent);
    equal(count, empty ? 0 : 9, 'Final rendered page size');
    if (!empty) check(d.ranked.length > 9, 'Regression requires more than nine public candidates');
    equal(required('[data-local-label]').textContent, expected.filtered.length > 0 ? `Local ${expected.filtered.length} filtered` : 'Local',
      'Rendered private filter count');
    const label = final.policy.max_source_share === null ? 'Off'
      : final.filtered.length > 0 ? `${final.policy.source_floors.length} floors`
        : final.candidates.some(c => c.reasons.length) ? 'Active' : `${final.policy.source_floors.length} floors`;
    equal(required('[data-diversity-label]').textContent, label, 'Rendered final diversity metadata');
    if (empty) check(!required('[data-empty]').hidden && !active(), 'Empty search must render no stale cards');
    load.cards = empty ? [] : await inspect(count);
    const ids = load.cards.map(card => card.id);
    equal(ids, final.candidates.map(entry => entry.object_id), 'Compiled app final order must match canonical SDK for pre-render persisted model');
    equal(new Set(ids).size, ids.length, 'Final IDs unique');
    for (const [index, card] of load.cards.entries()) {
      const trace = final.candidates[index];
      equal(trace.rank, index + 1, 'Final trace rank');
      equal(trace.source, d.ranked.find(e => e.candidate.object_id === card.id).candidate.source, 'Final source provenance');
      equal(card.source, 'local', 'Inspector must show private final card');
      equal(card.ranking_provider, d.ranking_provider, 'Inspector public ranking provenance');
      equal(card.score, String(Math.round(trace.diversified_score * 100) / 100), 'Rendered final analytics score');
      const reasons = trace.reasons.filter(r => r.contribution !== 0).slice(0, 2)
        .map(r => `local.diversity.${r.signal} ${signedPercent(r.contribution)}`);
      equal(card.reasons.filter(r => r.startsWith('local.diversity.')), reasons, 'Inspector final diversity reasons');
      const personalized = expected.ranked[index];
      const privateReasons = personalized.reasons
        .filter(r => r.contribution !== 0 && !r.signal.startsWith('private.diversity.'))
        .sort((left, right) => Math.abs(right.contribution) - Math.abs(left.contribution)).slice(0, 4)
        .map(r => `${r.signal} ${signedPercent(r.contribution)}`);
      equal(card.reasons, [...reasons, ...(privateReasons.length ? privateReasons
        : personalized.ranked.reasons.map(r => `${r.signal} ${Math.round(r.contribution * 100)}%`))],
      'Inspector reasons must explain the final personalized result');
      check(!card.reasons.some(r => /^diversity[.:]|^private\.diversity\./.test(r)), 'Stale public diversity reasons in final card');
      if (hidden) check(card.author !== dominant.identity.id, 'Hidden dominant author survived');
    }
    return { verified: true, publicCandidates: d.ranked.length, selected: ids,
      sources: final.candidates.map(e => e.source), filtered: expected.filtered.length,
      publicSourceMemberships: [...new Set(d.ranked.flatMap(e => e.candidate.sources.map(s => s.source)))],
      modelEvidence: 'persisted preferences and history captured before render; fixture has no safety exclusions',
      renderedCount: load.cards.length, conversionBoundEvidence: 'frontend/tests/feed-diversity.test.mjs (unit scope)',
      finalTrace: final.candidates, publicSearch: payload.search };
  };
  const actions = {
    async fixtures() {
      check((await api('/moderation/access')).can_review === false, 'Default account must not gain reviewer role');
      const safety = await api('/social/safety');
      check(safety.author_id === identityId && safety.entries.length === 0, 'Fixture requires an ordinary account without safety exclusions');
      sdk = await import('/__test_modules/sdk-personalization.js');
      preferencesModule = await import('/__test_modules/local-preferences.js');
      initialized = true;
      for (let index = 0; index < 4; index++) accounts.push(await api('/auth/register', { account: null, method: 'POST', body: {
        kind: 'Person', handle: `${marker.toLowerCase()}-${index}`, password: `Acceptance ${crypto.randomUUID()}!`,
      } }));
      dominant = accounts[0];
      // Thirty dominant-author posts and twelve alternatives expose truncation
      // before private filtering. Everything is signed by ordinary accounts.
      for (let index = 0; index < 42; index++) {
        const account = index < 12 ? accounts[1 + index % 3] : dominant;
        const text = `${marker} public study ${index}. ${index % 3 === 0
          ? 'Evidence methods data measured analysis source https://example.org/study'
          : index % 3 === 1 ? 'An unusual speculative creative hypothesis and open question'
            : 'Community discussion with practical observations and a reproducible result'}`;
        const { object } = await api('/objects/text', { account, method: 'POST', body: { author_id: account.identity.id, text } });
        check(object.author === account.identity.id && object.signature?.algorithm === 'Ed25519', 'Real signed publication required');
        objects.push(object);
      }
      for (const [index, relation] of ['evidence_for', 'evidence_against', 'references', 'follows'].entries()) {
        const source = objects[index];
        const account = accounts.find(a => a.identity.id === source.author);
        const { edge } = await api('/graph/edges', { account, method: 'POST', body: {
          author_id: account.identity.id, source: source.id, target: objects[41].id, relation, origin: 'HumanAssertion',
        } });
        check(edge.signature?.algorithm === 'Ed25519', 'Real signed relationship required'); edges.push(edge);
      }
      const { discovery } = await api('/discovery/candidates', { account: null, method: 'POST', body: {
        anchors: [objects[41].id], followed_objects: [], search: marker, limit: 200, exploration_slots: 2, lens: null,
      } });
      const sources = [...new Set(discovery.ranked.flatMap(e => e.candidate.sources.map(s => s.source)))];
      const graphFixtures = [];
      for (const [index, expected] of ['Evidence', 'Contradiction', 'SemanticNeighborhood', 'SocialGraph'].entries()) {
        const candidate = discovery.ranked.find(e => e.candidate.object_id === objects[index].id)?.candidate;
        check(candidate?.sources.some(s => s.source === expected), `Signed graph fixture ${objects[index].id} missing ${expected} membership`);
        graphFixtures.push({ objectId: candidate.object_id, edgeId: edges[index].id,
          expectedMembership: expected, primarySource: candidate.source, memberships: candidate.sources.map(s => s.source) });
      }
      await open(); await lens('balanced'); await reset();
      return { verified: true, signedPosts: objects.length, signedEdges: edges.length, anchoredApiSources: sources,
        graphFixtures,
        semanticCoverage: 'signed graph neighbors only; no embedding provider', reviewer: false };
    },
    async baseline() {
      await search(marker);
      await open(); tab('ranking'); field('creatorAffinityAuthorId').value = dominant.identity.id;
      click('[data-preferences-add-affinity]'); field('creatorAffinity').value = '100';
      const load = await apply();
      const result = await verify(load);
      const dominantCount = load.cards.filter(c => c.author === dominant.identity.id).length;
      check(dominantCount >= 5, `Fixture must establish dominant initial author, got ${dominantCount}/9`);
      baselineIds = load.cards.map(c => c.id);
      return { ...result, dominantCount, dominantAuthor: dominant.identity.id };
    },
    async private() {
      await open(); tab('interests'); field('interests').value = privateMarker; field('expertise').value = `${privateMarker}Expertise`;
      tab('filters'); field('mutedTerms').value = `${privateMarker}Mute`;
      field('hiddenAuthors').value = dominant.identity.id; click('[data-preferences-add-hidden-author]');
      const load = await apply();
      check(load.model.hidden_authors.includes(dominant.identity.id), 'Settings Apply must persist the hidden author before discovery');
      const result = await verify(load, { hidden: true });
      const publicNine = load.discovery.response.discovery.ranked.slice(0, 9).map(e => e.candidate.object_id);
      check(load.cards.some(c => !publicNine.includes(c.id)), 'Must refill from beyond the public first nine');
      check(load.cards.some(c => !baselineIds.includes(c.id)), 'Must replace filtered initial cards');
      const publicTrace = load.discovery.response.discovery.diversity_trace;
      const changedReasons = result.finalTrace.filter(entry => JSON.stringify(entry.reasons)
        !== JSON.stringify(publicTrace.candidates.find(c => c.object_id === entry.object_id)?.reasons)).length;
      check(changedReasons > 0, 'Fixture must distinguish final reasons from stale public reasons');
      return { ...result, refilledBeyondPublicNine: true, replacedInitialCards: true, changedPublicReasons: changedReasons };
    },
    async lenses() {
      const output = {};
      for (const name of ['research', 'weird', 'balanced']) {
        const load = await lens(name);
        output[name] = await verify(load, { hidden: true });
        equal(load.discovery.payload.lens.id, `babble.lens.stack.${name}.v1`, 'Distinct lens reaches backend');
      }
      return { verified: true, lenses: output };
    },
    async empty() {
      const empty = await verify(await search(`${marker}NoSuchPublicObject`), { empty: true, hidden: true });
      const unqueried = await verify(await search(''), { hidden: true });
      equal(unqueried.publicSearch, null, 'Empty input means public discovery, not explicit no-match search');
      await search(marker);
      return { verified: true, explicitNoMatch: empty, emptyQuery: unqueried };
    },
    async following() {
      const target = accounts[1];
      oldFollow = await api(`/social/following/${target.identity.id}`);
      await api(`/social/following/${target.identity.id}`, { method: 'PUT', body: {
        following: true, expected_revision: oldFollow.revision, idempotency_key: crypto.randomUUID(),
      } });
      await open();
      const { cursor } = await network();
      click('[data-lens="following"]'); await close();
      await until('Following ready', () => required('[data-following-status]').dataset.state === 'ready' && !!active());
      const expected = objects.filter(o => o.author === target.identity.id).sort((a, b) => {
        // Preserve server nanosecond order without converting to millisecond Date.
        const timestamp = value => value.replace(/\.(\d+)Z$/, (_, fraction) => `.${fraction.padEnd(9, '0')}Z`);
        return timestamp(b.created_at).localeCompare(timestamp(a.created_at)) || b.id.localeCompare(a.id);
      }).map(o => o.id);
      const observed = [];
      for (const [index, id] of expected.entries()) {
        await until('Following chronological position', () => active()?.dataset.objectId === id);
        const manifest = JSON.parse(required('.post-manifest pre', active()).textContent);
        equal(manifest.source, 'following', 'Following source unchanged'); equal(manifest.reasons, [], 'Following must not be reranked');
        observed.push(id); if (index < expected.length - 1) click('[data-next]');
      }
      equal(observed, expected, 'Following newest-first order');
      equal(required('[data-count-label]').textContent, String(expected.length), 'Following count unaffected');
      equal(required('[data-diversity-label]').textContent, 'Chronological', 'Following bypasses ranked diversity');
      const observedRequests = await network(cursor);
      const discoveryCalls = observedRequests.requests.filter(r => r.method === 'POST' && new URL(r.url).pathname === '/rpc')
        .filter(r => JSON.parse(r.requestBody).method === 'babble.discovery.candidates.v1');
      equal(discoveryCalls.length, 0, 'Following must not call ranked discovery');
      return { verified: true, observed, chronological: true, rankedDiscoveryCalls: 0 };
    },
    async cleanup() {
      if (oldFollow) {
        const target = accounts[1], state = await api(`/social/following/${target.identity.id}`);
        if (state.following !== oldFollow.following) await api(`/social/following/${target.identity.id}`, { method: 'PUT', body: {
          following: oldFollow.following, expected_revision: state.revision, idempotency_key: crypto.randomUUID(),
        } });
      }
      if (initialized) {
        await open(); await lens('balanced'); await reset();
        await search(originalQuery);
        if (originalLens !== 'following') await lens(originalLens);
        await close();
      }
      check(JSON.parse(sessionStorage.getItem(sessionKey)).identity.id === identityId, 'Main browser account changed');
      return { verified: true };
    },
  };
  return {
    async run(stage) { check(actions[stage], `Unknown stage: ${stage}`); return actions[stage](); },
    receiveNetwork(id, reply) {
      check(pendingNetwork?.id === id, 'Unexpected HTTP observation reply');
      const pending = pendingNetwork; pendingNetwork = null;
      delete window.__feedDiversityNetworkRead;
      if (reply.error) pending.reject(Error(reply.error));
      else { networkCursor = reply.value.cursor; pending.resolve(reply.value); }
    },
  };
}

// Failure injection validates the harness, not the browser feature. No API,
// Astro, Aegis, SDK build, or production model is run by this contract check.
if (process.argv[2] === "--contract-check") {
  const { Script, createContext, runInContext } = await import("node:vm");
  const originalFreeze = process.env.BABBLE_FEED_DIVERSITY_SOURCE_FROZEN;
  const ports = { apiPort: 18787, gatewayPort: 18788, frontendPort: 14329, aegisAddr: "127.0.0.1:17878" };
  try {
    delete process.env.BABBLE_FEED_DIVERSITY_SOURCE_FROZEN;
    await assert.rejects(assertFeedDiversityIsolation(ports), /Parent source freeze required/);
    process.env.BABBLE_FEED_DIVERSITY_SOURCE_FROZEN = "1";
    await assert.rejects(assertFeedDiversityIsolation({ ...ports, apiPort: 8787 }), /Disposable test ports/);
    await assert.rejects(assertFeedDiversityIsolation({ ...ports, gatewayPort: 8788 }), /Disposable test ports/);
    await assert.rejects(assertFeedDiversityIsolation({ ...ports, frontendPort: 4321 }), /Disposable test ports/);
    await assert.rejects(assertFeedDiversityIsolation({ ...ports, aegisAddr: "127.0.0.1:7878" }), /Acceptance Aegis/);
  } finally {
    if (originalFreeze === undefined) delete process.env.BABBLE_FEED_DIVERSITY_SOURCE_FROZEN;
    else process.env.BABBLE_FEED_DIVERSITY_SOURCE_FROZEN = originalFreeze;
  }
  for (const failed of [...stages, "cleanup"]) {
    const calls = [], context = createContext({ window: {} });
    const harness = { async run(stage) { calls.push(stage); if (stage === failed) throw Error(`injected:${stage}`); return { verified: true }; } };
    const execute = async commands => {
      const [{ code }] = commands; new Script(code);
      if (code.includes('window.__feedDiversityHarness = (function')) {
        context.window.__feedDiversityHarness = harness;
        return { results: [{ ok: true, value: { installed: true } }] };
      }
      const value = runInContext(code, context); assert.equal(typeof value, 'object');
      return { results: [{ ok: true, value }] };
    };
    const waitFor = async (code, predicate) => {
      for (let i = 0; i < 20; i++) {
        await new Promise(resolve => setImmediate(resolve));
        const value = runInContext(code, context); if (predicate(value)) return value;
      }
      throw Error('Probe did not settle');
    };
    await assert.rejects(verifyFeedDiversity({ execute, waitFor, apiUrl: 'http://127.0.0.1:18787',
      identityId: `id_${'a'.repeat(64)}`, readRequests: () => ({ cursor: 0, requests: [] }), log: () => {} }), error => error.message.includes(`injected:${failed}`));
    assert.equal(calls.at(-1), 'cleanup'); assert.equal(context.window.__feedDiversityHarness, undefined);
  }
  const request = {
    sequence: 1, url: 'http://127.0.0.1:18787/rpc', method: 'POST', status: 200,
    requestBody: JSON.stringify({ method: 'babble.discovery.candidates.v1', payload: { search: 'public' } }),
    responseBody: JSON.stringify({ result: { discovery: { ranked: [] } }, error: null }),
  };
  const observation = patch => ({ cursor: 1, requests: [{ ...request, ...patch }] });
  assert.deepEqual(decodeFeedDiscovery(observation(), 'PrivateOnly').payload, { search: 'public' });
  const preflight = { sequence: 1, url: request.url, method: 'OPTIONS', status: 204, requestBody: '', responseBody: '' };
  assert.deepEqual(decodeFeedDiscovery({ cursor: 2, requests: [preflight, { ...request, sequence: 2 }] }, 'PrivateOnly').payload,
    { search: 'public' }, 'CORS preflight is transport metadata, not an RPC envelope');
  assert.throws(() => decodeFeedDiscovery({ cursor: 1, requests: [preflight] }, 'PrivateOnly'), /got 0/);
  assert.throws(() => decodeFeedDiscovery(observation({ requestBody: '' }), 'private'), SyntaxError,
    'An empty POST body must still fail closed');
  for (const body of ['PrivateOnly', 'privateonly', 'PRIVATEONLY']) {
    assert.throws(() => decodeFeedDiscovery(observation({ requestBody: JSON.stringify({
      method: 'babble.discovery.candidates.v1', payload: { search: 'public', interests: [body] },
    }) }), 'PrivateOnly'), /Private preference leaked/);
  }
  assert.throws(() => decodeFeedDiscovery({ cursor: 1, requests: [] }, 'private'), /Expected one actual discovery/);
  assert.throws(() => decodeFeedDiscovery({ cursor: 2, requests: [request, { ...request, sequence: 2 }] }, 'private'), /got 2/);
  assert.throws(() => decodeFeedDiscovery({ ...observation(), captureError: 'truncated' }, 'private'), /HTTP observation failed/);
  for (const patch of [{ requestBody: null }, { requestBody: '{' }, { status: null }, { status: 500 },
    { error: 'disconnected' }, { responseBody: null }, { responseBody: '{' },
    { responseBody: '{"error":{"message":"rejected"},"result":null}' }, { responseBody: '{"result":{}}' }]) {
    assert.throws(() => decodeFeedDiscovery(observation(patch), 'private'));
  }
  // Exercise the actual Node-to-browser mailbox service, including capture loss.
  // The VM has no browser/API; these checks prove orchestration failure handling.
  for (const kind of ['success', 'cursor-only', 'delivery-error', 'capture-error', 'callback-error', 'invalid-cursor']) {
    const calls = [], context = createContext({ window: {} });
    let pending;
    const harness = {
      async run(stage) {
        calls.push(stage);
        if (stage !== 'fixtures') return { verified: true };
        context.window.__feedDiversityNetworkRead = { id: 1, since: 3, cursorOnly: kind === 'cursor-only' };
        return new Promise((resolve, reject) => { pending = { resolve, reject }; });
      },
      receiveNetwork(id, reply) {
        assert.equal(id, 1); delete context.window.__feedDiversityNetworkRead;
        if (reply.error) pending.reject(Error(reply.error));
        else {
          assert.equal(reply.value.cursor, 4);
          assert.equal(reply.value.requests.length, kind === 'cursor-only' ? 0 : 1);
          pending.resolve({ verified: true });
        }
      },
    };
    const execute = async ([{ code }]) => {
      new Script(code);
      if (code.includes('window.__feedDiversityHarness = (function')) {
        context.window.__feedDiversityHarness = harness;
        return { results: [{ ok: true, value: { installed: true } }] };
      }
      if (kind === 'delivery-error' && code.includes('receiveNetwork(1, {\"value\"')) throw Error('Aegis execute returned 400');
      return { results: [{ ok: true, value: runInContext(code, context) }] };
    };
    const waitFor = async (code, predicate) => {
      for (let i = 0; i < 20; i++) {
        await new Promise(resolve => setImmediate(resolve));
        const value = runInContext(code, context); if (predicate(value)) return value;
      }
      throw Error('Mailbox did not settle');
    };
    const readRequests = ({ since }) => {
      assert.equal(since, 3);
      if (kind === 'callback-error') throw Error('observer disconnected');
      return { cursor: kind === 'invalid-cursor' ? 2 : 4, requests: [{ ...request, responseBody: 'x'.repeat(4096) }],
        ...(kind === 'capture-error' ? { captureError: 'response exceeded observation limit' } : {}) };
    };
    const task = verifyFeedDiversity({ execute, waitFor, readRequests, apiUrl: 'http://127.0.0.1:18787',
      identityId: `id_${'a'.repeat(64)}`, log: () => {} });
    if (kind === 'success' || kind === 'cursor-only') assert.deepEqual(Object.keys(await task), stages);
    else await assert.rejects(task, /HTTP observation failed|HTTP observation delivery failed|observer disconnected|Invalid HTTP observation/);
    assert.equal(calls.at(-1), 'cleanup');
    assert.equal(context.window.__feedDiversityHarness, undefined);
    assert.equal(context.window.__feedDiversityNetworkRead, undefined);
  }
  console.log('Feed diversity harness observation and negative orchestration PASS (no browser/API evidence)');
}
