import assert from "node:assert/strict";

export async function verifyReactions(execute, waitFor, actorId) {
  const ready = await waitFor(`(() => {
    const card = document.querySelector('.post-card[data-offset="0"]');
    const panel = card?.querySelector('.reactions');
    return { object: card?.dataset.objectId, ready: panel?.getAttribute('aria-busy') === 'false'
      && panel.querySelector('.reaction-appreciation')?.disabled === false };
  })()`, (value) => value?.ready);
  const objectId = ready.object;
  const read = async () => {
    await execute([{ type: "eval", code: `(() => {
      window.__reactionRead = null;
      fetch(document.documentElement.dataset.babelApi + '/objects/${objectId}/reactions/actors/${actorId}')
        .then(async r => { if (!r.ok) throw Error('read ' + r.status); return r.json(); })
        .then(record => { window.__reactionRead = { record }; })
        .catch(error => { window.__reactionRead = { error: String(error) }; });
      return { started: true };
    })()` }]);
    const result = await waitFor("window.__reactionRead", (value) => value != null);
    assert.equal(result.error, undefined);
    return result.record;
  };
  const click = async (selector) => execute([{ type: "eval", code: `(() => {
    const control = document.querySelector('.post-card[data-offset="0"] .reactions ${selector}');
    if (!control || control.disabled) throw Error('Reaction control unavailable');
    control.click(); return { clicked: true };
  })()` }]);
  const settled = () => waitFor(`(() => {
    const panel = document.querySelector('.post-card[data-offset="0"] .reactions');
    return { ready: panel?.getAttribute('aria-busy') === 'false', message: panel?.querySelector('.reaction-status').textContent };
  })()`, (value) => value?.ready);

  assert.equal((await read()).state.revision, 0);
  await click('[title="Like"]');
  await waitFor(`({ visible: document.querySelector('.reactions .reaction-notice')?.hidden === false })`, (value) => value.visible);
  assert.equal((await read()).state.revision, 0, "unconfirmed public choice never writes");
  await click('.reaction-notice button');
  await settled();
  assert.equal((await read()).state.value.appreciation, "like");
  await click('.reaction-disclosure');
  await execute([{ type: "eval", code: `(() => {
    const panel = document.querySelector('.post-card[data-offset="0"] .reactions');
    const selects = panel.querySelectorAll('select');
    selects[0].value = 'engaging'; selects[0].dispatchEvent(new Event('change', { bubbles: true }));
    selects[1].value = 'support'; selects[1].dispatchEvent(new Event('change', { bubbles: true }));
    panel.querySelector('input[type="checkbox"]').click();
    const range = panel.querySelector('input[type="range"]'); range.value = '0';
    range.dispatchEvent(new Event('input', { bubbles: true }));
    panel.querySelector('form').requestSubmit();
    return { submitted: true };
  })()` }]);
  await settled();
  const saved = await read();
  assert.equal(saved.state.revision, 2);
  assert.deepEqual(saved.state.value, { appreciation: "like", engagement: "engaging", stance: "support", certainty: 0 });

  // Another host tab commits while this card still holds the preceding revision.
  await execute([{ type: "eval", code: `(() => {
    window.__reactionConflict = null;
    const api = document.documentElement.dataset.babelApi;
    const session = JSON.parse(sessionStorage.getItem('babel.session.v1:' + new URL(api).origin));
    fetch(api + '/objects/${objectId}/reactions/mine', {
      method: 'PUT', headers: { authorization: 'Bearer ' + session.token, 'content-type': 'application/json' },
      body: JSON.stringify({ value: { appreciation: null, engagement: 'not_engaging', stance: 'uncertain', certainty: null }, expected_revision: 2, idempotency_key: 'reaction-other-tab' })
    }).then(async r => { window.__reactionConflict = { status: r.status, state: await r.json() }; })
      .catch(error => { window.__reactionConflict = { error: String(error) }; });
    return { started: true };
  })()` }]);
  const conflict = await waitFor("window.__reactionConflict", (value) => value != null);
  assert.equal(conflict.status, 200, JSON.stringify(conflict));
  await click('[title="Dislike"]');
  const conflictView = await settled();
  assert.match(conflictView.message, /changed elsewhere/);
  const fresh = await read();
  assert.equal(fresh.state.revision, 3);
  assert.equal(fresh.state.value.appreciation, null);
  assert.equal(fresh.state.value.stance, "uncertain");
  await execute([{ type: "eval", code: `(() => {
    const panel = document.querySelector('.post-card[data-offset="0"] .reactions');
    panel.querySelector('details').open = true;
    [...panel.querySelectorAll('button')].find(button => button.textContent === 'Withdraw all').click();
    return { withdrawn: true };
  })()` }]);
  await settled();
  const withdrawn = await read();
  assert.equal(withdrawn.state.revision, 4);
  assert.deepEqual(withdrawn.state.value, { appreciation: null, engagement: null, stance: null, certainty: null });
  assert.equal(withdrawn.action.payload.previous_id, fresh.action.id);

  await execute([{ type: "eval", code: `(() => {
    window.__reactionLayout = null;
    (async () => {
      const results = [];
      for (const width of [390, 320]) {
        const frame = document.createElement('iframe');
        frame.title = 'Reaction layout verification';
        frame.style.cssText = 'position:fixed;inset:0;width:' + width + 'px;height:844px;border:0;z-index:9999';
        frame.src = location.href; document.body.append(frame);
        try {
          const deadline = Date.now() + 15000;
          let panel;
          while (!(panel = frame.contentDocument?.querySelector('.reactions')) || panel.getAttribute('aria-busy') !== 'false') {
            if (Date.now() > deadline) throw Error('Narrow reactions did not load');
            await new Promise(resolve => setTimeout(resolve, 100));
          }
          panel.querySelector('details').open = true;
          const primary = panel.closest('.post-primary').getBoundingClientRect();
          const fields = [...panel.querySelectorAll('select, input, button')].filter(node => node.getClientRects().length);
          results.push({ width, overflow: frame.contentDocument.documentElement.scrollWidth > width,
            contained: fields.every(node => { const r = node.getBoundingClientRect(); return r.left >= primary.left && r.right <= primary.right; }),
            radius: parseFloat(frame.contentWindow.getComputedStyle(panel.closest('.post-primary')).borderRadius) });
        } finally { frame.remove(); }
      }
      window.__reactionLayout = { results };
    })().catch(error => { window.__reactionLayout = { error: String(error) }; });
    return { started: true };
  })()` }]);
  const layout = await waitFor("window.__reactionLayout", (value) => value != null);
  assert.equal(layout.error, undefined, JSON.stringify(layout));
  for (const result of layout.results) {
    assert.equal(result.overflow, false, JSON.stringify(result));
    assert.equal(result.contained, true, JSON.stringify(result));
    assert.equal(result.radius, 32);
  }
  await execute([{ type: "eval", code: `(() => {
    document.querySelector('.post-card[data-offset="0"] .reactions details').open = false;
    delete window.__reactionRead; delete window.__reactionConflict; delete window.__reactionLayout;
    return { cleaned: true };
  })()` }]);
  console.log("Public reaction UI PASS", JSON.stringify({ confirmedBeforeWrite: true, confidenceZero: true,
    conflictRefreshed: true, withdrawn: true, layouts: layout.results }));
}
