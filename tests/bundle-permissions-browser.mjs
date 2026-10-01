import assert from "node:assert/strict";

export async function publishPermissionApp({ evaluate, waitFor, rpc, marker, files }) {
  await evaluate(`(() => {
    document.querySelector('[data-close-surface]').click();
    document.querySelector('[data-toggle-composer]').click();
    const text = document.querySelector('[data-compose-text]');
    text.value = ${JSON.stringify(marker)}; text.dispatchEvent(new Event('input', { bubbles: true }));
    const transfer = new DataTransfer();
    for (const spec of ${JSON.stringify(files)}) {
      const file = new File([spec.content], spec.path, { type: spec.media_type });
      Object.defineProperty(file, 'webkitRelativePath', { value: 'permission-app/' + spec.path });
      transfer.items.add(file);
    }
    const input = document.querySelector('[data-compose-bundle]'); input.files = transfer.files;
    input.dispatchEvent(new Event('change', { bubbles: true }));
    document.querySelector('[data-bundle-permissions]').open = true;
    document.querySelector('.bundle-permission-advanced').open = true;
    const editor = document.querySelector('[data-bundle-capabilities]');
    editor.value = '[invalid'; editor.dispatchEvent(new Event('input', { bubbles: true }));
    document.querySelector('[data-close-composer]').click();
    document.querySelector('[data-toggle-composer]').click();
    document.querySelector('[data-compose-form]').requestSubmit();
    return { submitted: true };
  })()`);
  const invalid = await evaluate(`({
    raw: document.querySelector('[data-bundle-capabilities]').value,
    invalid: document.querySelector('[data-bundle-capabilities]').getAttribute('aria-invalid'),
    state: document.querySelector('[data-author-status]').dataset.state,
    open: !document.querySelector('[data-composer-panel]').hidden
  })`);
  assert.deepEqual(invalid, { raw: "[invalid", invalid: "true", state: "error", open: true });
  const before = await rpc("babel.search.objects.v1", { q: marker, author: null, kind: null, limit: 20 });
  assert.equal(before.results.filter(item => item.object.payload.text === marker).length, 0,
    "invalid declarations must not publish an app with silently removed permissions");
  const declarations = [
    { id: "babel.storage.local", version: 1, scope: { namespace: "host-actions" } },
    { id: "babel.clipboard.write", version: 1, scope: {} },
    { id: "babel.fullscreen.enter", version: 1, scope: {} },
  ];
  await evaluate(`(() => {
    const editor = document.querySelector('[data-bundle-capabilities]');
    editor.value = ${JSON.stringify(JSON.stringify([declarations[0]], null, 2))};
    editor.dispatchEvent(new Event('input', { bubbles: true }));
    document.querySelector('[data-bundle-clipboard]').click();
    document.querySelector('[data-bundle-fullscreen]').click();
    document.querySelector('[data-close-composer]').click();
    document.querySelector('[data-toggle-composer]').click();
    return { edited: true };
  })()`);
  const edited = await evaluate(`({
    declarations: JSON.parse(document.querySelector('[data-bundle-capabilities]').value),
    count: document.querySelector('[data-bundle-permission-count]').textContent,
    clipboard: document.querySelector('[data-bundle-clipboard]').checked,
    fullscreen: document.querySelector('[data-bundle-fullscreen]').checked,
    invalid: document.querySelector('[data-bundle-capabilities]').getAttribute('aria-invalid')
  })`);
  assert.deepEqual(edited, { declarations, count: "3", clipboard: true, fullscreen: true, invalid: null });
  await evaluate("document.querySelector('[data-compose-form]').requestSubmit(); ({ submitted: true })");
  const completed = await waitFor(`({
    hidden: document.querySelector('[data-composer-panel]').hidden,
    text: document.querySelector('[data-author-status]').textContent,
    state: document.querySelector('[data-author-status]').dataset.state
  })`, value => value.hidden || value.state === "error");
  assert.equal(completed.state, "ready", completed.text);
  assert.match(completed.text, /^Published /);
  const published = await rpc("babel.search.objects.v1", { q: marker, author: null, kind: null, limit: 20 });
  const matches = published.results.filter(item => item.object.payload.text === marker);
  assert.equal(matches.length, 1);
  const object = matches[0].object;
  assert.deepEqual(object.capabilities, declarations, "signed Object must contain exactly the requested declarations");
  assert.equal(object.signature?.algorithm, "Ed25519");
  await waitFor(`({ id: document.querySelector('.post-card[data-offset="0"]')?.dataset.objectId })`, value => value.id === object.id);
  console.log("Capability authoring PASS: invalid draft retained and blocks publish, common toggles preserve scoped declarations, close/reopen, signed readback");
  return object;
}
