// Serialized into the disposable live-stack page. API credentials stay inside
// the fixture closure; native browser APIs and production responses stay intact.
export function installBrowserInvocationFixture({ apiUrl, marker, password }) {
  if (window.__browserInvocationTest) throw new Error('browser invocation fixture already installed');
  const api = new URL(apiUrl);
  if (new URL(document.documentElement.dataset.babbleApi).origin !== api.origin) throw new Error('fixture API mismatch');
  const saved = sessionStorage.getItem(`babble.session.v1:${api.origin}`);
  const session = JSON.parse(saved ?? 'null');
  if (!session?.token || !/^browser-author-/.test(session.identity?.handle ?? '')) throw new Error('isolated live-stack account required');
  const search = document.querySelector('[data-search-input]');
  const originalSearch = search.value;
  const nativeFetch = window.fetch.bind(window);
  let client, other, secondLogin, sessionId, documentId, connectedSession;
  const requests = new Map();
  const s = window.__browserInvocationTest = { ready: false, failure: null, taskDone: false, taskResult: null,
    results: {}, childReady: false, mutated: null, actorId: session.identity.id, objectId: null, port: null, clicks: [] };
  const request = async (path, { method = 'GET', body, token = session.token, document: source = null } = {}) => {
    const headers = { 'content-type': 'application/json' };
    if (token) headers.authorization = `Bearer ${token}`;
    if (source) headers['x-babble-surface-document'] = source;
    const response = await nativeFetch(new URL(path, api), { method, headers, credentials: 'omit', redirect: 'error',
      ...(body === undefined ? {} : { body: JSON.stringify(body) }), signal: AbortSignal.timeout(15000) });
    return { status: response.status, body: response.status === 204 ? null : await response.json() };
  };
  const get = async path => {
    const response = await request(path);
    if (response.status !== 200) throw new Error(`fixture read HTTP ${response.status}: ${path}`);
    return response.body;
  };
  const rpc = async (method, payload) => {
    const response = await request('/rpc', { method: 'POST', body: {
      protocol: 'babble.rpc.v1', id: crypto.randomUUID(), method, payload,
      binding: { object_id: null, surface_session_id: null, runtime_id: 'browser-invocation-acceptance', origin: location.origin, capability_grants: [] },
      idempotency_key: crypto.randomUUID(), deadline: { timeout_ms: 30000, client_started_at: new Date().toISOString() }, trace_id: null,
    } });
    if (response.status !== 200 || response.body.error) throw new Error(`fixture RPC ${method}: ${response.status}/${response.body.error?.code}`);
    return response.body.result;
  };
  const failure = error => ({ name: error?.name ?? 'Error', message: error?.message ?? 'operation failed' });
  s.task = (fn, args) => {
    s.taskDone = false; s.taskResult = null; s.failure = null;
    Promise.resolve().then(() => fn(s, ...args)).then(value => { s.taskResult = value ?? null; s.taskDone = true; },
      error => { s.failure = failure(error); s.taskDone = true; });
  };
  const click = event => {
    const button = event.target.closest?.('[data-host-action-allow]');
    if (button) s.clicks.push({ trusted: event.isTrusted, stage: button.closest('[data-host-action-stage]')?.dataset.hostActionStage });
  };
  document.addEventListener('click', click, true);
  s.snapshot = () => {
    const dialog = document.querySelector('[data-host-action-dialog]');
    const social = document.querySelector('[data-invocation-prompt]');
    const frame = document.querySelector('[data-surface-host] iframe');
    return { ready: s.ready, failure: s.failure, taskDone: s.taskDone, taskResult: s.taskResult,
      actorId: s.actorId, actorHandle: session.identity.handle, objectId: s.objectId, sessionId, documentId, results: s.results,
      childReady: s.childReady, mutated: s.mutated, clicks: s.clicks, frame: !!frame,
      phase: document.querySelector('[data-surface-host]')?.dataset.state,
      ownedFullscreen: document.fullscreenElement === document.querySelector('[data-surface-panel]'),
      fullscreen: !!document.fullscreenElement,
      fullscreenAvailable: typeof document.querySelector('[data-surface-panel]')?.requestFullscreen === 'function' && document.fullscreenEnabled,
      social: social ? { text: social.textContent, open: social.open } : null,
      prompt: dialog ? { open: dialog.open, stage: dialog.dataset.hostActionStage, text: dialog.textContent,
        preview: dialog.querySelector('[aria-label="Text to copy"]')?.textContent,
        hostOwned: !!dialog.closest('[data-surface-panel]'), modal: dialog.matches(':modal'),
        cancelFocused: document.activeElement?.matches('[data-host-action-cancel]'),
        injectedMarkup: !!dialog.querySelector('[data-browser-injected]') } : null };
  };
  s.publish = async (files, source) => {
    const { publishBundle } = await import('/src/app/bundle-publication.ts');
    const capabilities = [
      { id: 'babble.storage.local', version: 1, scope: { namespace: 'host-actions' } },
      { id: 'babble.clipboard.write', version: 1, scope: {} },
      { id: 'babble.fullscreen.enter', version: 1, scope: {} },
      { id: 'babble.social.reply', version: 1, scope: {} },
    ];
    const assets = [
      { path: 'index.html', content: '<!doctype html><html><head><meta charset="utf-8"><title>Browser invocation acceptance</title></head><body><h1>Browser invocation acceptance</h1><output id="status">Connecting</output><script type="module" src="./app.js"></script></body></html>' },
      { path: 'app.js', content: `import { createSurfaceSDK } from './sdk/sdk.js';\nimport { connectSurfaceBridge } from './sdk/channel.js';\n(${source})(${JSON.stringify({ origin: location.origin, marker })}, createSurfaceSDK, connectSurfaceBridge);` }, ...files,
    ];
    const object = await publishBundle(client, s.actorId, marker, {
      files: assets.map(asset => {
        const file = new File([asset.content], asset.path.split('/').at(-1), { type: asset.path.endsWith('.html') ? 'text/html' : 'text/javascript' });
        Object.defineProperty(file, 'webkitRelativePath', { value: `browser-app/${asset.path}` });
        return file;
      }), entryPath: 'index.html', capabilitiesText: JSON.stringify(capabilities),
    });
    s.objectId = object.id;
    search.value = marker; search.form.requestSubmit();
    return { objectId: object.id, signature: object.signature, capabilities: object.capabilities };
  };
  s.connect = async binding => {
    // The parent observes the real registration in the disposable API's store.
    // Revalidate its session through the authenticated API before using the IDs.
    if (sessionId || !binding || typeof binding.sessionId !== 'string' || typeof binding.documentId !== 'string') {
      throw new Error('one observed fixture registration required');
    }
    const { session: admitted } = await get(`/runtime/surfaces/sessions/${encodeURIComponent(binding.sessionId)}`);
    if (admitted.id !== binding.sessionId || admitted.plan.object_id !== s.objectId || admitted.lifecycle !== 'active') {
      throw new Error('wrong fixture Surface');
    }
    sessionId = binding.sessionId; documentId = binding.documentId; connectedSession = sessionId;
    const documentReadProbe = await request(`/runtime/surfaces/sessions/${sessionId}`, { document: documentId });
    const frame = document.querySelector('[data-surface-host] iframe');
    const channel = new MessageChannel(); s.port = channel.port1;
    s.port.onmessage = ({ data }) => {
      if (data.type === 'ready') s.childReady = true;
      else if (data.type === 'result') s.results[data.id] = { result: data.result, error: data.error };
      else if (data.type === 'mutated') s.mutated = data.id;
      else if (data.type === 'failure') s.failure = { name: 'ChildFailure', message: data.message };
    };
    frame.contentWindow.postMessage({ type: 'browser.acceptance', marker, plan: admitted.plan, sessionId, actorId: s.actorId },
      new URL(frame.src).origin, [channel.port2]);
    return { origin: new URL(frame.src).origin, sandbox: [...frame.sandbox].sort(), plan: admitted.plan, documentReadProbe };
  };
  s.send = input => {
    requests.set(input.id, input);
    s.port.postMessage({ type: 'request', ...input });
    return true;
  };
  s.cancel = id => { s.port.postMessage({ type: 'cancel', id }); return true; };
  s.prepare = (input, changes = {}) => {
    const value = typeof input === 'string' ? requests.get(input) : input;
    return request(`/invocations/v1/${value.method.startsWith('babble.social.') ? '' : 'browser/'}prepare`, {
      method: 'POST', document: documentId, body: { origin: { kind: 'surface', session_id: sessionId, document_id: documentId }, object_id: s.objectId,
        method: value.method, request_key: value.key, payload: { ...value.payload, ...changes }, timeout_ms: value.timeoutMs },
    });
  };
  s.invocation = (id, action = 'status', body, overrides = {}) => request(`/invocations/v1/browser/${encodeURIComponent(id)}/${action}`, {
    document: documentId, ...(action === 'status' ? {} : { method: 'POST', body: body ?? {} }), ...overrides,
  });
  s.isolation = async id => {
    const paths = ['status', 'decision', 'dispatch', 'ack', 'cancel'];
    const results = {};
    for (const action of paths) {
      const body = action === 'decision' ? { decision: 'allow_once' }
        : action === 'ack' ? { dispatch_id: 'a'.repeat(64), result: { kind: 'failed', code: 'context_lost' } } : {};
      results[action] = {};
      for (const [name, overrides] of Object.entries({ anonymous: { token: null }, otherActor: { token: other.current.token },
        otherLogin: { token: secondLogin.current.token }, wrongDocument: { document: crypto.randomUUID() } })) {
        results[action][name] = (await s.invocation(id, action, body, overrides)).status;
      }
    }
    return results;
  };
  s.grants = async () => (await rpc('babble.capabilities.inspect.v1', { object_id: s.objectId })).grants;
  s.effects = async () => {
    const { edges } = await get(`/graph/objects/${s.objectId}/incoming`);
    const { results } = await rpc('babble.search.objects.v1', { q: marker, author: s.actorId, kind: null, limit: 100 });
    return { edges: edges.map(edge => edge.id).sort(), objects: results.map(item => item.object.id).sort() };
  };
  s.cleanup = async () => {
    const errors = [];
    document.querySelector('[data-host-action-cancel]')?.click();
    document.querySelector('[data-invocation-prompt] [aria-label="Cancel request"]')?.click();
    document.querySelector('[data-close-surface]')?.click();
    s.port?.close(); document.removeEventListener('click', click, true);
    if (connectedSession) {
      try {
        const deadline = Date.now() + 10000;
        while ((await get(`/runtime/surfaces/sessions/${connectedSession}`)).session.lifecycle !== 'evicted') {
          if (Date.now() >= deadline) throw new Error('Surface eviction not acknowledged');
          await new Promise(resolve => setTimeout(resolve, 50));
        }
      } catch { errors.push('surface-eviction'); }
    }
    for (const account of [other, secondLogin]) {
      try { await account?.logout(); } catch { errors.push('temporary-login-logout'); }
    }
    search.value = originalSearch; search.form.requestSubmit();
    return { errors, parentAccountPreserved: sessionStorage.getItem(`babble.session.v1:${api.origin}`) === saved };
  };
  (async () => {
    const [{ Accounts }, { BabbleFrontendClient }] = await Promise.all([import('/src/app/accounts.ts'), import('/src/app/protocol.ts')]);
    const accounts = new Accounts(api.href, sessionStorage);
    client = new BabbleFrontendClient(api.href, accounts.authenticatedFetch);
    other = new Accounts(api.href, null);
    secondLogin = new Accounts(api.href, null);
    await other.register(`browser-probe-${crypto.randomUUID()}`, crypto.randomUUID() + crypto.randomUUID());
    await secondLogin.login(s.actorId, password);
    s.ready = true;
  })().catch(error => { s.failure = failure(error); });
}

export function browserInvocationApplication({ origin, marker }, createSurfaceSDK, connectSurfaceBridge) {
  let sdk, probe;
  const inputs = new Map(), controllers = new Map();
  const transport = connectSurfaceBridge({ parentOrigin: origin });
  window.addEventListener('message', async event => {
    if (event.source !== parent || event.origin !== origin || event.data?.type !== 'browser.acceptance'
      || event.data.marker !== marker || event.ports.length !== 1 || probe) return;
    probe = event.ports[0];
    try {
      sdk = createSurfaceSDK({ transport: await transport, plan: event.data.plan,
        surfaceSessionId: event.data.sessionId, runtimeId: 'browser-invocation-child', origin: location.origin,
        currentIdentityId: event.data.actorId });
      probe.onmessage = ({ data }) => {
        if (data.type === 'cancel') { controllers.get(data.id)?.abort(); return; }
        if (data.type === 'mutate') {
          inputs.get(data.id).text = data.text;
          probe.postMessage({ type: 'mutated', id: data.id }); return;
        }
        if (data.type !== 'request') return;
        inputs.set(data.id, data.payload);
        const controller = new AbortController(); controllers.set(data.id, controller);
        const options = { id: data.id, idempotencyKey: data.key, timeoutMs: data.timeoutMs, signal: controller.signal };
        const operation = data.method === 'babble.clipboard.write' ? sdk.clipboard.write(data.payload, options)
          : data.method === 'babble.fullscreen.enter' ? sdk.fullscreen.enter(data.payload, options)
          : data.method === 'babble.social.reply' ? sdk.social.reply(data.payload, options)
          : sdk.rpc.call(data.method, data.payload, options);
        operation.then(result => probe.postMessage({ type: 'result', id: data.id, result, error: null }),
          error => probe.postMessage({ type: 'result', id: data.id, result: null, error: { name: error.name, code: error.code ?? null } }));
      };
      document.querySelector('#status').textContent = 'Ready';
      probe.postMessage({ type: 'ready' });
    } catch (error) { probe.postMessage({ type: 'failure', message: error.message }); }
  });
  transport.catch(error => { document.querySelector('#status').textContent = error.message; });
  window.addEventListener('pagehide', () => { sdk?.close(); probe?.close(); }, { once: true });
}
