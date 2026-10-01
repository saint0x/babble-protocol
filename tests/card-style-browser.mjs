import assert from "node:assert/strict";

// Aegis evaluates the real app at desktop size and in narrow same-origin frames.
// Frame checks cover responsive layout, not physical-device gesture behavior.
export async function verifyCardPresentation(execute, waitFor) {
  await waitFor(`({ ready: Boolean(document.querySelector('.post-card[data-offset="0"] .post-primary')) })`, (value) => value.ready);
  await execute([{ type: "eval", code: `
    (() => {
      window.__babelCardStyle = null;
      const measure = (doc) => {
        const win = doc.defaultView;
        const card = doc.querySelector('.post-card[data-offset="0"]');
        const primary = card.querySelector('.post-primary');
        const rect = primary.getBoundingClientRect();
        const author = primary.querySelector('.post-author').getBoundingClientRect();
        const actions = primary.querySelector('.post-actions').getBoundingClientRect();
        const conversation = card.querySelector('.conversation').getBoundingClientRect();
        const next = doc.querySelector('.post-card[data-offset="1"] .post-primary')?.getBoundingClientRect();
        const header = doc.querySelector('.header-frame');
        const controls = [...header.children].filter((node) => win.getComputedStyle(node).display !== 'none');
        const bounds = controls.map((node) => node.getBoundingClientRect());
        const overlaps = bounds.some((a, i) => bounds.slice(i + 1).some((b) =>
          Math.min(a.right, b.right) - Math.max(a.left, b.left) > 1 &&
          Math.min(a.bottom, b.bottom) - Math.max(a.top, b.top) > 1));
        return {
          viewport: win.innerWidth, width: rect.width, height: rect.height,
          radius: parseFloat(win.getComputedStyle(primary).borderRadius),
          replyRadius: parseFloat(win.getComputedStyle(card).getPropertyValue('--reply-radius')),
          nestedRadius: parseFloat(win.getComputedStyle(card).getPropertyValue('--nested-reply-radius')),
          background: win.getComputedStyle(primary).backgroundImage,
          transition: win.getComputedStyle(card).transitionProperty,
          neighborFilters: [...doc.querySelectorAll('.post-card:not([data-offset="0"])')]
            .map((node) => win.getComputedStyle(node).filter),
          nearestOpacity: [...doc.querySelectorAll('.post-card[data-offset="1"], .post-card[data-offset="-1"]')]
            .map((node) => parseFloat(win.getComputedStyle(node).opacity)),
          mainShadow: win.getComputedStyle(primary).boxShadow,
          conversationFollowsPost: primary.nextElementSibling?.matches('[data-quotes-root]')
            && primary.nextElementSibling.nextElementSibling?.classList.contains('conversation'),
          inspectorAfterConversation: card.querySelector('.conversation').nextElementSibling?.classList.contains('post-manifest'),
          actionIcons: ['social', 'protocol'].every(kind => {
            const button = card.querySelector('.action-popover[data-kind="' + kind + '"] > button');
            return Boolean(button?.querySelector('svg') && button.title && button.getAttribute('aria-label'));
          }),
          overflow: card.scrollWidth > card.clientWidth,
          inViewport: rect.left >= 0 && rect.right <= win.innerWidth,
          authorInside: author.top >= rect.top && author.bottom <= rect.bottom,
          actionsInside: actions.top >= rect.top && actions.bottom <= rect.bottom,
          repliesNarrower: conversation.width < rect.width,
          replyWidthRatio: conversation.width / rect.width,
          nextCardGap: next ? next.left - rect.right : null,
          headerOverlap: overlaps,
          lensesVisible: win.getComputedStyle(doc.querySelector('.lens-switcher')).display !== 'none',
          documentOverflow: doc.documentElement.scrollWidth > win.innerWidth,
          contentPadding: parseFloat(win.getComputedStyle(primary.querySelector('.post-card-inner')).paddingLeft),
          // Control focus can scroll this independent conversation column.
          contentBelowHeader: rect.top + card.scrollTop >= header.getBoundingClientRect().bottom,
          readingOffset: card.scrollTop,
          neighborsVisible: [...doc.querySelectorAll('.post-card:not([data-offset="0"]) .post-primary')]
            .some((node) => { const r = node.getBoundingClientRect(); return r.right > 0 && r.left < win.innerWidth; })
        };
      };
      (async () => {
        const results = [measure(document)];
        for (const [width, height] of [[1440, 900], [1100, 800], [1024, 768], [860, 800], [390, 844], [320, 568]]) {
          const frame = document.createElement('iframe');
          frame.title = 'Temporary responsive card verification';
          frame.style.cssText = 'position:fixed;left:0;top:0;border:0;z-index:9999;width:' + width + 'px;height:' + height + 'px';
          frame.src = location.href;
          document.body.append(frame);
          try {
            const deadline = Date.now() + 12000;
            while (!frame.contentDocument?.querySelector('.post-card[data-offset="0"] .post-primary')) {
              if (Date.now() > deadline) throw new Error('Responsive feed failed to load: ' + JSON.stringify({
                width, url: frame.contentWindow?.location.href, readyState: frame.contentDocument?.readyState,
                status: frame.contentDocument?.querySelector('[data-status]')?.textContent,
                error: frame.contentDocument?.querySelector('[data-error]')?.textContent,
                text: frame.contentDocument?.body?.innerText.slice(-2000),
              }));
              await new Promise((resolve) => setTimeout(resolve, 100));
            }
            await new Promise((resolve) => setTimeout(resolve, 400));
            results.push(measure(frame.contentDocument));
          } finally { frame.remove(); }
        }
        window.__babelCardStyle = { results };
      })().catch((error) => { window.__babelCardStyle = { error: String(error) }; });
      return true;
    })()
  ` }]);
  const result = await waitFor("window.__babelCardStyle", (value) => value != null);
  await execute([{ type: "eval", code: "delete window.__babelCardStyle; true" }]);
  assert.equal(result.error, undefined, JSON.stringify(result));
  assert.equal(result.results.length, 7);
  for (const layout of result.results) {
    const context = JSON.stringify(layout);
    assert.equal(layout.radius, 32, context);
    assert.ok(layout.nestedRadius < layout.replyRadius && layout.replyRadius < layout.radius, context);
    assert.match(layout.background, /linear-gradient/, context);
    assert.match(layout.transition, /transform/, context);
    assert.ok(layout.neighborFilters.every(filter => filter === 'none'), context);
    assert.ok(layout.nearestOpacity.every(opacity => opacity >= 0.85 && opacity < 1), context);
    assert.notEqual(layout.mainShadow, 'none', context);
    assert.equal(layout.conversationFollowsPost, true, context);
    assert.equal(layout.inspectorAfterConversation, true, context);
    assert.equal(layout.actionIcons, true, context);
    assert.equal(layout.lensesVisible, layout.viewport >= 1100, context);
    assert.equal(layout.overflow, false, context);
    assert.equal(layout.inViewport, true, context);
    assert.equal(layout.authorInside, true, context);
    assert.equal(layout.actionsInside, true, context);
    assert.equal(layout.repliesNarrower, true, context);
    assert.ok(layout.replyWidthRatio <= 0.9, context);
    if (layout.viewport > 860) {
      assert.ok(layout.replyWidthRatio <= 0.8, context);
      if (layout.nextCardGap !== null) assert.ok(layout.nextCardGap >= 8, context);
    }
    assert.equal(layout.headerOverlap, false, context);
    assert.equal(layout.documentOverflow, false, context);
    assert.equal(layout.contentBelowHeader, true, context);
    // Image cards deliberately let media reach the edges; text keeps reading room.
    assert.ok(layout.contentPadding === 0 || layout.contentPadding >= 20, context);
    assert.equal(layout.neighborsVisible, true, context);
  }
  console.log("Rounded card geometry PASS", JSON.stringify(result.results));
  const before = await waitFor(`(() => {
    const card = document.querySelector('.post-card[data-offset="0"]');
    return { id: card.dataset.objectId, scroll: card.scrollTop,
      count: document.querySelectorAll('.post-card:not([data-exiting])').length };
  })()`, value => Boolean(value?.id));
  if (before.count > 1) {
    await execute([{ type: "eval", code: `document.querySelector('[data-next]').click(); ({ clicked: true })` }]);
    await waitFor(`(() => {
      const active = document.querySelector('.post-card[data-offset="0"]');
      return { id: active.dataset.objectId, accessible: !active.inert,
        inactive: [...document.querySelectorAll('.post-card:not([data-offset="0"])')].every(card => card.inert) };
    })()`, value => value.id !== before.id && value.accessible && value.inactive);
    await execute([{ type: "eval", code: `document.querySelector('[data-prev]').click(); ({ clicked: true })` }]);
    await waitFor(`(() => {
      const card = document.querySelector('.post-card[data-offset="0"]');
      return { id: card.dataset.objectId, scroll: card.scrollTop };
    })()`, value => value.id === before.id && Math.abs(value.scroll - before.scroll) <= 1);
    console.log("Horizontal card navigation and reading-position restoration PASS");
  }
}
