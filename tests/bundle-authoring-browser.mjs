import assert from "node:assert/strict";
import { randomUUID } from "node:crypto";

// Uses the live stack's existing Aegis session and signed-in host. This helper
// starts no services and changes no production transports or Surface policies.
export async function verifyBundleAuthoring(execute, waitFor) {
  const marker = `Aegis bundle ${randomUUID()}`;
  const gatewayPort = process.env.BABBLE_LIVE_GATEWAY_PORT ?? "18788";
  const evaluate = async (code) => {
    const response = await execute([{ type: "eval", code }]);
    assert.equal(response.results.length, 1);
    assert.equal(response.results[0].ok, true, "bundle authoring Aegis evaluation failed");
    return response.results[0].value;
  };
  // Aegis eval need not await Promises. Keep credentials in the browser closure
  // and expose only each operation's public result to the polling helper.
  const task = async (operation, ...args) => {
    await evaluate(`(() => {
      const s = window.__bundleAuthoring;
      s.result = null;
      Promise.resolve().then(() => (${operation.toString()})(s, ...${JSON.stringify(args)}))
        .then(value => { s.result = { value }; }, error => { s.result = { error: String(error) }; });
      return { started: true };
    })()`);
    const { result } = await waitFor("({ result: window.__bundleAuthoring.result })", value => value?.result != null);
    assert.equal(result.error, undefined, result.error);
    return result.value;
  };
  const composer = () => waitFor(`(() => ({
    hidden: document.querySelector('[data-composer-panel]').hidden,
    disabled: document.querySelector('[data-compose-submit]').disabled,
    status: document.querySelector('[data-author-status]').textContent,
    state: document.querySelector('[data-author-status]').dataset.state
  }))()`, value => value?.hidden && value.status?.startsWith("Published "));

  await waitFor(`({
    signedIn: Boolean(document.querySelector('[data-author-handle]')?.value),
    closed: document.querySelector('[data-composer-panel]')?.hidden === true,
    online: document.querySelector('[data-status]')?.dataset.state === 'online'
  })`, value => value?.signedIn && value.closed && value.online);

  let initialized = false;
  try {
    initialized = true;
    const setup = await evaluate(`(${initialize.toString()})(${JSON.stringify(marker)})`);
    assert.equal(setup.directory, true, "composer must expose a directory file input");
    assert.equal(setup.statusRole, "status");
    const geometry = await task(measureComposerGeometry, [320, 390, 860]);
    assert.deepEqual(geometry.map(value => value.viewport), [320, 390, 860]);
    for (const measurement of geometry) {
      assert.deepEqual(measurement.errors, [], `composer geometry: ${JSON.stringify(measurement)}`);
    }
    const files = [
      { path: "index.html", type: "text/html", kind: "document", body: '<!doctype html><html><head><meta charset="utf-8"><title>Babble bundle counter</title><link rel="stylesheet" href="./theme.css"></head><body><main><h1>Babble bundle counter</h1><button id="increment" type="button">Increment</button><output id="count">0</output><p id="status">Connecting</p></main><script type="module" src="./main.js"></script></body></html>' },
      { path: "main.js", type: "text/javascript", kind: "script", body: `(${counterApplication.toString()})(${JSON.stringify({ marker, parentOrigin: setup.origin })});\n` },
      { path: "theme.css", type: "text/css", kind: "stylesheet", body: 'body { margin: 0; padding: 24px; color: rgb(17, 93, 121); background: white; font: 18px sans-serif; } main { display: grid; gap: 16px; } button { min-height: 44px; } output { display: block; font-size: 32px; }\n' },
    ];

    await task((s, files) => {
      s.select(files);
      s.type(s.marker + " invalid");
      return true;
    }, [{ path: "main.js", type: "text/javascript", body: "export const noDocument = true;" }]);
    const invalid = await waitFor(`(() => ({
      text: document.querySelector('[data-bundle-status]').textContent.trim(),
      state: document.querySelector('[data-bundle-status]').dataset.state,
      invalid: document.querySelector('[data-compose-bundle]').getAttribute('aria-invalid'),
      disabled: document.querySelector('[data-compose-submit]').disabled,
      entryCount: [...document.querySelector('[data-bundle-entry]').options].filter(o => o.value).length
    }))()`, value => value?.text?.length > 0 && (value.state === "error" || value.disabled));
    assert.equal(invalid.state, "error");
    assert.equal(invalid.invalid, "true");
    assert.equal(invalid.entryCount, 0, "a directory without HTML has no entry document");
    await evaluate("document.querySelector('[data-compose-form]').requestSubmit(); ({ attempted: true })");
    const blocked = await evaluate(`({
      open: !document.querySelector('[data-composer-panel]').hidden,
      state: document.querySelector('[data-author-status]').dataset.state
    })`);
    assert.deepEqual(blocked, { open: true, state: "error" }, "submission must reject the invalid picker state");
    assert.deepEqual(await task(async s => {
      await new Promise(resolve => setTimeout(resolve, 100));
      return s.search(s.marker + " invalid");
    }), [], "invalid directory must not fall back to publishing text");

    await evaluate("document.querySelector('[data-clear-bundle]').click(); ({ cleared: true })");
    const cleared = await evaluate(`({
      files: document.querySelector('[data-compose-bundle]').files.length,
      entries: [...document.querySelector('[data-bundle-entry]').options].filter(o => o.value).length
    })`);
    assert.deepEqual(cleared, { files: 0, entries: 0 });
    await task((s, files) => { s.select(files); s.type(s.marker); return true; }, files);
    const selected = await waitFor(`(() => {
      const entry = document.querySelector('[data-bundle-entry]');
      return { options: [...entry.options].filter(o => o.value).map(o => o.value), value: entry.value,
        summary: document.querySelector('[data-bundle-status]').textContent,
        disabled: document.querySelector('[data-compose-submit]').disabled };
    })()`, value => value?.options?.includes("index.html") && !value.disabled);
    assert.deepEqual(selected.options, ["index.html"], "only HTML documents are entry candidates");
    assert.match(selected.summary, /^3 files,/);
    assert.equal(selected.value, "index.html");
    await evaluate(`(() => {
      const entry = document.querySelector('[data-bundle-entry]');
      entry.value = 'index.html';
      entry.dispatchEvent(new Event('change', { bubbles: true }));
      document.querySelector('[data-close-composer]').click();
      return { closed: true };
    })()`);
    await waitFor("({ hidden: document.querySelector('[data-composer-panel]').hidden })", value => value?.hidden === true);
    await evaluate("document.querySelector('[data-toggle-composer]').click(); ({ reopened: true })");
    const retained = await evaluate(`(() => {
      return { summary: document.querySelector('[data-bundle-status]').textContent,
        text: document.querySelector('[data-compose-text]').value,
        entry: document.querySelector('[data-bundle-entry]').value };
    })()`);
    assert.deepEqual(retained, { summary: selected.summary, text: marker, entry: "index.html" });

    await evaluate(`(() => {
      const form = document.querySelector('[data-compose-form]');
      form.requestSubmit();
      form.requestSubmit();
      return { submitted: true };
    })()`);
    const published = await composer();
    assert.equal(published.state, "ready", published.status);
    const objects = await task(s => s.search(s.marker));
    assert.equal(objects.length, 1, "rapid duplicate submits must publish exactly one Object");
    const objectId = objects[0];
    const object = await task(async (s, objectId) => (await s.rpc("babble.object.get.v1", { object_id: objectId })).object, objectId);
    assert.equal(object.author, setup.identityId);
    assert.equal(object.signature?.algorithm, "Ed25519");
    assert.match(object.signature?.bytes ?? "", /^[0-9a-f]{128}$/);
    assert.equal(object.payload.text, marker);
    const surface = object.surfaces.find(surface => surface.role === "Feed");
    assert.equal(surface?.target, "Web");
    assert.equal(surface.bundle.version, 1);
    assert.equal(surface.bundle.entry_path, "index.html");
    assert.deepEqual(surface.bundle.files.map(file => file.path), files.map(file => file.path).sort());
    // Successful byte-for-byte publication after close/reopen proves the draft
    // retained the original Files even when the native picker was reset.
    for (const expected of files) {
      const file = surface.bundle.files.find(file => file.path === expected.path);
      assert.equal(file.media_type, expected.type);
      assert.equal(file.kind, expected.kind);
      assert.equal(file.size_bytes, Buffer.byteLength(expected.body));
      assert.match(file.integrity, /^[0-9a-f]{64}$/);
      assert.equal(file.source_uri, `babble://blobs/${file.integrity}`);
      const blob = await task((s, file) => s.rpc("babble.media.blob.get.v1", { hash: file.integrity, media_type: file.media_type }), file);
      assert.equal(blob.bytes_hex, Buffer.from(expected.body).toString("hex"), `${file.path}: stored bytes must match the selected File`);
    }
    const entry = surface.bundle.files.find(file => file.path === "index.html");
    assert.equal(surface.entry, entry.source_uri);
    assert.equal(surface.integrity, entry.integrity);

    const mounts = [];
    for (let attempt = 0; attempt < 2; attempt++) {
      await waitFor(`(() => {
        const card = document.querySelector('.post-card[data-offset="0"]');
        return { id: card?.dataset.objectId, surface: !!card?.querySelector('[data-action="surface"]') };
      })()`, value => value?.id === objectId && value.surface);
      await evaluate(`(() => {
        window.__bundleAuthoring.observation = null;
        document.querySelector('.post-card[data-offset="0"] [data-action="surface"]').click();
        return { opened: true };
      })()`);
      const mounted = await waitFor(`(() => {
        const panel = document.querySelector('[data-surface-panel]');
        const frame = panel?.querySelector('iframe');
        const session = [...panel?.querySelectorAll('[data-surface-meta] span') ?? []]
          .find(node => node.textContent.startsWith('Session: '))?.textContent.slice(9);
        return { active: frame?.dataset.babbleLifecycle === 'active',
          controller: document.querySelector('[data-surface-host]')?.dataset.state,
          src: frame?.src, sandbox: frame ? [...frame.sandbox].sort() : [], session,
          objectId: panel?.closest('.post-card')?.dataset.objectId };
      })()`, value => value?.active && value.controller === "active" && value.session);
      assert.equal(mounted.objectId, objectId);
      const url = new URL(mounted.src);
      assert.match(url.hostname, /^m-[0-9a-f]{48}\.localhost$/);
      assert.equal(url.protocol, "http:");
      assert.equal(url.port, gatewayPort);
      assert.equal(url.pathname, "/index.html");
      assert.notEqual(url.origin, setup.origin);
      assert.notEqual(url.origin, setup.apiOrigin);
      assert.deepEqual(mounted.sandbox, ["allow-same-origin", "allow-scripts"]);
      const session = await task(async (s, id) => (await s.get("/runtime/surfaces/sessions/" + encodeURIComponent(id))).session, mounted.session);
      assert.equal(session.id, mounted.session);
      assert.equal(session.lifecycle, "active");
      assert.equal(session.plan.object_id, objectId);
      assert.equal(session.plan.bundle_verification.policy_version, 1);
      assert.match(session.plan.bundle_verification.manifest_hash, /^[0-9a-f]{64}$/);
      // GET returns the native persisted plan. The gateway mount descriptor is
      // attached only to the start response; inspect its actual iframe above.
      assert.deepEqual(session.plan.surface.bundle, surface.bundle);

      // A separate test-only channel is offered TO the fixture, never through
      // the production bridge. Its handler clicks the actual child DOM button.
      await evaluate(`(() => {
        const s = window.__bundleAuthoring;
        const frame = document.querySelector('[data-surface-host] iframe');
        const channel = new MessageChannel();
        s.probe?.close();
        s.probe = channel.port1;
        s.probe.onmessage = event => { s.observation = event.data; };
        frame.contentWindow.postMessage({ type: 'bundle.authoring.probe', marker: s.marker },
          new URL(frame.src).origin, [channel.port2]);
        return { observing: true };
      })()`);
      const ready = await waitFor("window.__bundleAuthoring.observation", value => value?.ready === true);
      assert.equal(ready.origin, url.origin);
      assert.deepEqual(ready.controls, ["babble.surface.accept", "babble.surface.ready"]);
      assert.equal(ready.count, "0");
      assert.equal(ready.heading, "Babble bundle counter");
      assert.equal(ready.status, "Ready");
      assert.equal(ready.visible, true);
      assert.equal(ready.color, "rgb(17, 93, 121)");
      assert.equal(ready.parentDOMDenied, true);
      for (const path of ["/main.js", "/theme.css"]) assert.ok(ready.resources.includes(url.origin + path), `${path} must load through the verified gateway`);
      for (let count = 1; count <= 2; count++) {
        await evaluate("window.__bundleAuthoring.probe.postMessage({ type: 'increment' }); ({ clicked: true })");
        const changed = await waitFor("window.__bundleAuthoring.observation", value => value?.count === String(count));
        assert.equal(changed.status, "Ready");
        assert.equal(changed.visible, true);
      }
      mounts.push({ origin: url.origin, session: mounted.session });
      await evaluate("document.querySelector('[data-close-surface]').click(); ({ closed: true })");
      await waitFor(`({ hidden: document.querySelector('[data-surface-panel]').hidden,
        noFrame: !document.querySelector('[data-surface-host] iframe') })`, value => value?.hidden && value.noFrame);
      const evicted = await task(async (s, id) => {
        for (let attempt = 0; attempt < 100; attempt++) {
          const { session } = await s.get("/runtime/surfaces/sessions/" + encodeURIComponent(id));
          if (session.lifecycle === "evicted") return true;
          await new Promise(resolve => setTimeout(resolve, 50));
        }
        return false;
      }, mounted.session);
      assert.equal(evicted, true, "closing the Surface must evict its server session");
    }
    assert.notEqual(mounts[0].origin, mounts[1].origin, "reopening must allocate a fresh isolated origin");
    assert.notEqual(mounts[0].session, mounts[1].session);
    assert.deepEqual(await task(s => s.search(s.marker)), [objectId], "Surface interaction must not create duplicate Objects");
    assert.deepEqual(await task(s => s.search(s.marker + " invalid")), [], "invalid publication must remain absent");
  } finally {
    if (initialized) {
      await evaluate(`(${cleanup.toString()})()`);
      await waitFor(`({ signedIn: Boolean(document.querySelector('[data-author-handle]')?.value),
        closed: document.querySelector('[data-composer-panel]').hidden,
        noFrame: !document.querySelector('[data-surface-host] iframe') })`, value => value?.signedIn && value.closed && value.noFrame);
    }
  }
  console.log("Bundle authoring browser PASS", JSON.stringify({ invalidBlocked: true, filesRetained: true,
    signedInventory: true, duplicateSubmitSuppressed: true, gatewayExecutions: 2, counterClicks: 4,
    sessionsEvicted: true, composerGeometryWidths: [320, 390, 860] }));
}

// Geometry only: use the actual page and CSS in separate same-origin viewports.
// No form events, login, file selection, or publication occur in these frames.
async function measureComposerGeometry(s, widths) {
  const measurements = [];
  s.geometryFrames = new Set();
  for (const width of widths) {
    const frame = document.createElement('iframe');
    frame.title = `Composer geometry ${width}`;
    frame.tabIndex = -1;
    frame.setAttribute('aria-hidden', 'true');
    frame.style.cssText = `position:fixed;left:-10000px;top:0;width:${width}px;height:1000px;border:0;`;
    const url = new URL(location.href);
    url.searchParams.delete('surface');
    url.hash = '';
    frame.src = url.href;
    s.geometryFrames.add(frame);
    let timer;
    try {
      await Promise.race([
        new Promise((resolve, reject) => {
          frame.onload = resolve;
          frame.onerror = () => reject(new Error(`Composer geometry ${width}: frame load failed`));
          document.body.append(frame);
        }).then(async () => {
          const view = frame.contentWindow;
          const doc = frame.contentDocument;
          await doc.fonts.ready;
          while (doc.querySelector('[data-status]')?.dataset.state !== 'online') {
            if (!frame.isConnected) throw new Error('Composer geometry frame was removed');
            await new Promise(resolve => setTimeout(resolve, 50));
          }
          const panel = doc.querySelector('[data-composer-panel]');
          const form = doc.querySelector('[data-compose-form]');
          const picker = doc.querySelector('[data-bundle-picker]');
          const entry = doc.querySelector('[data-bundle-entry]');
          if (!panel || !form || !picker || !entry) throw new Error('Composer geometry controls missing');
          panel.hidden = false;
          panel.dataset.state = 'open';
          picker.hidden = false;
          entry.closest('label').hidden = false;
          const option = doc.createElement('option');
          option.textContent = option.value = 'index.html';
          entry.replaceChildren(option);
          const permissions = doc.querySelector('[data-bundle-permissions]');
          permissions.hidden = false;
          permissions.open = true;
          doc.querySelector('.bundle-permission-advanced').open = true;
          doc.querySelector('[data-bundle-capabilities]').value = JSON.stringify([
            { id: 'babble.storage.local', version: 1, scope: { namespace: 'long-application-namespace' } },
            { id: 'babble.clipboard.write', version: 1, scope: {} },
          ], null, 2);
          // Layout is synchronous. A short turn lets the open-state transition
          // finish without relying on offscreen iframe animation-frame delivery.
          await new Promise(resolve => setTimeout(resolve, 250));
          if (!frame.isConnected) throw new Error('Composer geometry frame was removed');
          const errors = [];
          const check = (condition, message, evidence = {}) => { if (!condition) errors.push({ message, ...evidence }); };
          const rectangle = rect => ({ left: rect.left, right: rect.right, top: rect.top,
            bottom: rect.bottom, width: rect.width, height: rect.height });
          const bounds = element => element.getBoundingClientRect();
          const content = element => {
            const rect = bounds(element), style = view.getComputedStyle(element);
            return { left: rect.left + parseFloat(style.borderLeftWidth) + parseFloat(style.paddingLeft),
              right: rect.right - parseFloat(style.borderRightWidth) - parseFloat(style.paddingRight) };
          };
          const inside = (rect, container) => rect.width > 0 && rect.left >= container.left - 1 && rect.right <= container.right + 1;
          const visible = element => element.getClientRects().length > 0 && view.getComputedStyle(element).visibility === 'visible';
          const overlaps = (a, b) => Math.min(a.right, b.right) - Math.max(a.left, b.left) > 0.5
            && Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top) > 0.5;
          check(inside(bounds(panel), { left: 0, right: view.innerWidth }), 'panel extends outside viewport',
            { rect: rectangle(bounds(panel)), innerWidth: view.innerWidth });
          check(panel.scrollWidth <= panel.clientWidth, 'panel has horizontal overflow',
            { scrollWidth: panel.scrollWidth, clientWidth: panel.clientWidth });
          check(inside(bounds(form), content(panel)), 'form extends outside panel content',
            { rect: rectangle(bounds(form)), container: content(panel) });
          check(inside(bounds(picker), content(form)), 'application folder extends outside form content',
            { rect: rectangle(bounds(picker)), container: content(form) });
          const controls = [...panel.querySelectorAll('input, select, textarea, button')].filter(visible);
          for (const selector of ['[data-compose-bundle]', '[data-bundle-entry]', '[data-compose-text]', '[data-compose-submit]',
            '[data-bundle-capabilities]', '[data-bundle-clipboard]', '[data-bundle-fullscreen]']) {
            check(visible(doc.querySelector(selector)), `${selector} must be visible`);
          }
          for (const control of controls) {
            const name = control.getAttribute('aria-label') || control.name || control.tagName;
            const container = content(control.closest('label') ?? control.parentElement);
            check(inside(bounds(control), container), `${name} exceeds its container`,
              { rect: rectangle(bounds(control)), container });
            check(inside(bounds(control), content(panel)), `${name} exceeds panel content`,
              { rect: rectangle(bounds(control)), container: content(panel) });
          }
          // Decorative icons deliberately overlap transparent native file inputs;
          // screen-reader labels have clipped text ranges, not visible collisions.
          const labels = [...panel.querySelectorAll('label > span, .panel-heading h2, [data-compose-context], [data-author-status]')]
            .filter(element => visible(element) && !element.matches('.sr-only, [aria-hidden="true"]'));
          const lines = [];
          for (const label of labels) {
            const range = doc.createRange();
            range.selectNodeContents(label);
            for (const rect of range.getClientRects()) {
              check(inside(rect, content(panel)), `${label.textContent} extends outside panel content`,
                { rect: rectangle(rect), container: content(panel) });
              for (const control of controls) check(!overlaps(rect, bounds(control)), `${label.textContent} overlaps a control`,
                { rect: rectangle(rect), controlRect: rectangle(bounds(control)),
                  control: control.getAttribute('aria-label') || control.name || control.tagName });
              // Range can report overlapping fragments for one label. Compare
              // distinct label elements, even when their text happens to match.
              for (const previous of lines) {
                if (previous.label === label) continue;
                check(!overlaps(rect, previous.rect), `${label.textContent} overlaps ${previous.text}`,
                  { rect: rectangle(rect), otherRect: rectangle(previous.rect) });
              }
              lines.push({ label, rect, text: label.textContent });
            }
          }
          for (const selector of ['[data-compose-title]', '[data-compose-context]', '[data-author-status]']) {
            check(labels.includes(panel.querySelector(selector)), `${selector} was not measured`);
          }
          check(labels.some(label => label.textContent === 'Entry document'), 'application entry label was not measured');
          for (const selector of ['[data-bundle-clipboard]', '[data-bundle-fullscreen]']) {
            const label = panel.querySelector(selector).closest('label');
            check(bounds(label).height >= 44 && bounds(label).width >= 44, 'permission checkbox label hit target is too small');
          }
          for (const selector of ['[data-compose-media]', '[data-compose-bundle]']) {
            const input = panel.querySelector(selector);
            const tool = input.closest('.composer-attachment-tool');
            check(!!input.getAttribute('aria-label') && !!tool?.querySelector('svg'), 'attachment tool lacks label or icon');
            check(bounds(tool).width >= 44 && bounds(tool).height >= 44, 'attachment hit target is too small');
          }
          const style = view.getComputedStyle(panel);
          measurements.push({ viewport: view.innerWidth, requestedWidth: width,
            documentClientWidth: doc.documentElement.clientWidth,
            documentScrollWidth: doc.documentElement.scrollWidth,
            panelRect: rectangle(bounds(panel)),
            panelStyle: { position: style.position, left: style.left, width: style.width,
              maxWidth: style.maxWidth, transform: style.transform, boxSizing: style.boxSizing }, errors });
        }),
        new Promise((_, reject) => {
          timer = setTimeout(() => reject(new Error(`Composer geometry ${width}: readiness timed out`)), 15000);
        }),
      ]);
    } finally {
      clearTimeout(timer);
      frame.onload = frame.onerror = null;
      frame.remove();
      s.geometryFrames.delete(frame);
    }
  }
  return measurements;
}

function initialize(marker) {
  if (window.__bundleAuthoring) throw new Error("Bundle authoring test is already active");
  const required = selector => {
    const element = document.querySelector(selector);
    if (!element) throw new Error("Missing bundle authoring control: " + selector);
    return element;
  };
  const bundle = required('[data-compose-bundle]');
  const entry = required('[data-bundle-entry]');
  const status = required('[data-bundle-status]');
  required('[data-clear-bundle]');
  const api = new URL(document.documentElement.dataset.babbleApi).origin;
  const session = JSON.parse(sessionStorage.getItem('babble.session.v1:' + api));
  if (!session?.token || !session.identity?.id) throw new Error("Bundle authoring requires a stored authenticated account");
  const draftKey = `babble.draft.v1:${JSON.stringify([api, session.identity.id, 'publish', null])}`;
  const stored = localStorage.getItem(draftKey);
  required('[data-toggle-composer]').click();
  if ([...entry.options].some(option => option.value)) {
    required('[data-close-composer]').click();
    throw new Error("Bundle authoring test requires no existing application attachment");
  }
  const original = { text: required('[data-compose-text]').value,
    media: [...required('[data-compose-media]').files], files: [...bundle.files], entry: entry.value };
  const s = window.__bundleAuthoring = { marker, original, draftKey, stored, result: null, observation: null };
  s.type = text => {
    required('[data-compose-text]').value = text;
    required('[data-compose-text]').dispatchEvent(new Event('input', { bubbles: true }));
  };
  s.assign = (input, files) => {
    const transfer = new DataTransfer();
    for (const file of files) transfer.items.add(file);
    input.files = transfer.files;
    input.dispatchEvent(new Event('change', { bubbles: true }));
  };
  s.select = files => s.assign(bundle, files.map(file => {
    const value = new File([file.body], file.path.split('/').at(-1), { type: file.type });
    Object.defineProperty(value, 'webkitRelativePath', { value: 'package/' + file.path });
    return value;
  }));
  const request = async (path, body) => {
    const response = await fetch(new URL(path, api), {
      method: body ? 'POST' : 'GET', credentials: 'omit', redirect: 'error',
      headers: { authorization: 'Bearer ' + session.token, 'content-type': 'application/json' },
      ...(body ? { body: JSON.stringify(body) } : {}), signal: AbortSignal.timeout(10000),
    });
    if (!response.ok) throw new Error(`Bundle test host request ${path}: HTTP ${response.status}`);
    return response.json();
  };
  s.get = path => request(path);
  s.rpc = async (method, payload) => {
    const response = await request('/rpc', {
      protocol: 'babble.rpc.v1', id: crypto.randomUUID(), method,
      binding: { object_id: null, surface_session_id: null, runtime_id: 'bundle-authoring-test',
        origin: location.origin, capability_grants: [] },
      payload, idempotency_key: null, deadline: { timeout_ms: 10000, client_started_at: new Date().toISOString() }, trace_id: null,
    });
    if (response.error) throw new Error(`Bundle test RPC failed: ${method}`);
    return response.result;
  };
  s.search = async q => {
    const result = await s.rpc('babble.search.objects.v1', { q, author: session.identity.id, kind: null, limit: 100 });
    return result.results.filter(item => item.object.payload.text === q).map(item => item.object.id);
  };
  s.assign(required('[data-compose-media]'), []);
  required('[data-clear-bundle]').click();
  return { origin: location.origin, apiOrigin: api, identityId: session.identity.id,
    directory: bundle.matches('input[type="file"][webkitdirectory]'), statusRole: status.getAttribute('role') };
}

function cleanup() {
  const s = window.__bundleAuthoring;
  if (!s) return { cleaned: true };
  for (const frame of s.geometryFrames ?? []) frame.remove();
  s.probe?.close();
  if (document.querySelector('[data-surface-host] iframe')) document.querySelector('[data-close-surface]').click();
  if (document.querySelector('[data-composer-panel]').hidden) document.querySelector('[data-toggle-composer]').click();
  try {
    document.querySelector('[data-clear-bundle]').click();
    s.type(s.original.text);
    s.assign(document.querySelector('[data-compose-media]'), s.original.media);
    if (s.original.files.length) {
      s.assign(document.querySelector('[data-compose-bundle]'), s.original.files);
      const entry = document.querySelector('[data-bundle-entry]');
      entry.value = s.original.entry;
      entry.dispatchEvent(new Event('change', { bubbles: true }));
    }
    document.querySelector('[data-close-composer]').click();
  } finally {
    if (s.stored === null) localStorage.removeItem(s.draftKey);
    else localStorage.setItem(s.draftKey, s.stored);
    delete window.__bundleAuthoring;
  }
  return { cleaned: true };
}

function counterApplication(config) {
  const button = document.querySelector('#increment');
  const count = document.querySelector('#count');
  const status = document.querySelector('#status');
  let ready = false;
  let accepted = false;
  let probe;
  const controls = [];
  const report = () => {
    let parentDOMDenied = false;
    try { void parent.document.body; } catch (error) { parentDOMDenied = error.name === 'SecurityError'; }
    const bounds = button.getBoundingClientRect();
    probe?.postMessage({ ready, controls, count: count.textContent, status: status.textContent,
      heading: document.querySelector('h1').textContent, origin: location.origin, parentDOMDenied,
      color: getComputedStyle(document.body).color,
      visible: bounds.width > 0 && bounds.height >= 44 && getComputedStyle(button).visibility === 'visible',
      resources: performance.getEntriesByType('resource').map(resource => resource.name) });
  };
  button.addEventListener('click', () => { count.textContent = String(Number(count.textContent) + 1); report(); });
  window.addEventListener('message', event => {
    if (event.source !== parent || event.origin !== config.parentOrigin
      || event.data?.type !== 'bundle.authoring.probe' || event.data.marker !== config.marker || event.ports.length !== 1) return;
    probe?.close();
    probe = event.ports[0];
    probe.onmessage = message => { if (ready && message.data?.type === 'increment') button.click(); };
    report();
  });
  const channel = new MessageChannel();
  const control = type => ({ type: 'babble.surface.' + type, protocol: 'babble.rpc.v1', version: 1 });
  channel.port1.onmessage = event => {
    const data = event.data;
    if (!data || data.protocol !== 'babble.rpc.v1' || data.version !== 1 || Object.keys(data).length !== 3) return;
    if (!accepted && data.type === 'babble.surface.accept') {
      accepted = true;
      controls.push(data.type);
      channel.port1.postMessage(control('confirm'));
    } else if (accepted && !ready && data.type === 'babble.surface.ready') {
      controls.push(data.type);
      ready = true;
      status.textContent = 'Ready';
      report();
    } else if (data.type === 'babble.surface.close') {
      ready = false;
      channel.port1.close();
      probe?.close();
    }
  };
  const connect = () => parent.postMessage(control('connect'), config.parentOrigin, [channel.port2]);
  if (document.readyState === 'complete') connect();
  else window.addEventListener('load', connect, { once: true });
  window.addEventListener('pagehide', () => { channel.port1.close(); probe?.close(); }, { once: true });
}
