import assert from "node:assert/strict";

export async function verifyInlineSurface(execute, waitFor) {
  const initial = await waitFor(`(() => {
    const panel = document.querySelector('[data-surface-panel]');
    const frame = panel.querySelector('iframe');
    if (!frame || frame.dataset.babelLifecycle !== 'active') return {};
    const primary = panel.closest('.post-primary');
    const column = panel.closest('.post-card');
    const session = [...panel.querySelectorAll('[data-surface-meta] span')].find(node => node.textContent.startsWith('Session: '))?.textContent.slice(9);
    window.__inlineSurface = { frame, session, object: column?.dataset.objectId };
    return { inline: !!primary, session, object: column?.dataset.objectId,
      radius: primary ? parseFloat(getComputedStyle(primary).borderRadius) : 0,
      frameWidth: frame.getBoundingClientRect().width,
      overflow: panel.scrollWidth > panel.clientWidth };
  })()`, (value) => value?.session && value.inline);
  assert.ok(initial.radius >= 24);
  assert.ok(initial.frameWidth > 200);
  assert.equal(initial.overflow, false);

  const expanded = await execute([{ type: "eval", code: `(() => {
    document.querySelector('[data-expand-surface]').click();
    const panel = document.querySelector('[data-surface-panel]');
    return { sameFrame: panel.querySelector('iframe') === window.__inlineSurface.frame,
      expanded: document.querySelector('[data-deck]').hasAttribute('data-surface-expanded'),
      pressed: document.querySelector('[data-expand-surface]').getAttribute('aria-pressed'),
      overflow: document.documentElement.scrollWidth > innerWidth };
  })()` }]);
  assert.equal(expanded.results[0].ok, true);
  assert.deepEqual(expanded.results[0].value, { sameFrame: true, expanded: true, pressed: "true", overflow: false });
  await execute([{ type: "eval", code: `(() => {
    document.querySelector('[data-expand-surface]').click();
    document.querySelector('[data-close-surface]').click();
    return { closed: true };
  })()` }]);
  const closed = await waitFor(`(() => ({
    hidden: document.querySelector('[data-surface-panel]').hidden,
    noFrame: !document.querySelector('[data-surface-host] iframe'),
    restored: !document.querySelector('.post-card[data-offset="0"] .post-primary').hasAttribute('data-surface-open'),
    focus: document.activeElement.dataset.action,
    expanded: document.querySelector('[data-deck]').hasAttribute('data-surface-expanded')
  }))()`, (value) => value?.hidden && value.noFrame);
  assert.deepEqual(closed, { hidden: true, noFrame: true, restored: true, focus: "surface", expanded: false });

  await execute([{ type: "eval", code: `(() => {
    window.__inlineEviction = null;
    import('/src/app/accounts.ts').then(async ({ Accounts }) => {
      const accounts = new Accounts(document.documentElement.dataset.babelApi, sessionStorage);
      for (let attempt = 0; attempt < 100; attempt++) {
        const response = await accounts.authenticatedFetch(new URL('/runtime/surfaces/sessions/' + window.__inlineSurface.session, accounts.origin));
        const result = await response.json();
        if (!response.ok || result.session?.lifecycle === 'evicted') {
          window.__inlineEviction = result;
          return;
        }
        await new Promise(resolve => setTimeout(resolve, 50));
      }
      throw new Error('Server did not evict the closed inline Surface');
    }).catch(error => { window.__inlineEviction = { error: String(error) }; });
    return { requested: true };
  })()` }]);
  const evicted = await waitFor("({ result: window.__inlineEviction })", (value) => value.result != null);
  assert.equal(evicted.result.error, undefined, JSON.stringify(evicted));
  assert.equal(evicted.result.session.lifecycle, "evicted");

  await execute([{ type: "eval", code: `(() => {
    document.querySelector('.post-card[data-offset="0"] [data-action="surface"]').click();
    return { opened: true };
  })()` }]);
  const reopened = await waitFor(`(() => {
    const panel = document.querySelector('[data-surface-panel]');
    return { active: panel.querySelector('iframe')?.dataset.babelLifecycle === 'active',
      sameObject: panel.closest('.post-card')?.dataset.objectId === window.__inlineSurface.object,
      newFrame: !!panel.querySelector('iframe') && panel.querySelector('iframe') !== window.__inlineSurface.frame,
      session: [...panel.querySelectorAll('[data-surface-meta] span')].find(node => node.textContent.startsWith('Session: '))?.textContent.slice(9) };
  })()`, (value) => value?.active && value.session);
  assert.equal(reopened.sameObject, true);
  assert.equal(reopened.newFrame, true);
  assert.notEqual(reopened.session, initial.session);
  await execute([{ type: "eval", code: "delete window.__inlineSurface; delete window.__inlineEviction; ({ cleared: true })" }]);
  console.log("Inline Surface lifecycle PASS", JSON.stringify({ inline: true, expandedWithoutRemount: true, closeEvicted: true, reopenedFreshSession: true }));
}
