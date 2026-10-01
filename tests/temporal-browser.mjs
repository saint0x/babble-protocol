import assert from "node:assert/strict";

export async function verifyTemporalPresentation(execute, waitFor, provider) {
  await execute([{ type: "eval", code: `
    (() => {
      window.__babbleTemporalLayout = null;
      const inspect = async (doc) => {
        const win = doc.defaultView;
        const card = doc.querySelector('.post-card[data-offset="0"]');
        const savedScroll = card.scrollTop;
        const wrap = card.querySelector('.action-popover[data-kind="analytics"]');
        const button = wrap.querySelector('button');
        const panel = wrap.querySelector('.popover-panel');
        const manifest = JSON.parse(card.querySelector('.post-manifest pre').textContent);
        if (!manifest.temporal) throw new Error('Discovery card omitted temporal provenance');
        button.scrollIntoView({block: 'center'});
        button.click();
        await new Promise(resolve => setTimeout(resolve, 300));
        try {
          const r = panel.getBoundingClientRect();
          const metrics = panel.querySelector('.temporal-metrics');
          const details = card.querySelector('.post-inspection .temporal-metrics');
          return {
            width: win.innerWidth,
            visible: !panel.hidden && button.getAttribute('aria-expanded') === 'true'
              && panel.getClientRects().length > 0 && r.width > 0 && r.height > 0,
            fits: r.left >= -1 && r.right <= win.innerWidth + 1 && r.top >= -1 && r.bottom <= win.innerHeight + 1,
            overflow: panel.scrollWidth > panel.clientWidth + 1 || doc.documentElement.scrollWidth > win.innerWidth,
            contained: [...metrics.querySelectorAll('strong')].every(node => {
              const n = node.getBoundingClientRect(); return n.left >= r.left - 1 && n.right <= r.right + 1;
            }),
            evaluated: details.querySelector('time').dateTime,
            provider: manifest.temporal.provider,
            score: manifest.temporal.score,
            fullComponents: details.textContent.includes('Decay rate') && details.textContent.includes('Time sensitivity'),
            actionsVisible: card.querySelector('.post-actions').getClientRects().length > 0,
            actions: card.querySelector('.post-actions').textContent,
            heuristicLabel: metrics.textContent.includes('Temporal heuristic'),
            radius: parseFloat(win.getComputedStyle(card.querySelector('.post-primary')).borderRadius),
          };
        } finally { button.click(); card.scrollTop = savedScroll; }
      };
      (async () => {
        const results = [];
        // An active Surface deliberately replaces the post controls. Inspect
        // normal feed cards in independent frames without closing that runtime.
        for (const [width, height] of [[1280, 800], [390, 844], [320, 568]]) {
          const frame = document.createElement('iframe');
          frame.title = 'Temporary temporal analytics verification';
          frame.style.cssText = 'position:fixed;left:0;top:0;border:0;z-index:9999;width:' + width + 'px;height:' + height + 'px';
          const url = new URL(location.href);
          url.searchParams.delete('surface');
          frame.src = url.href;
          document.body.append(frame);
          try {
            const deadline = Date.now() + 12000;
            while (!frame.contentDocument?.querySelector('.post-card[data-offset="0"] .temporal-metrics')) {
              if (Date.now() > deadline) throw new Error('Temporal discovery did not render in frame');
              await new Promise(resolve => setTimeout(resolve, 100));
            }
            await new Promise(resolve => setTimeout(resolve, 400));
            results.push(await inspect(frame.contentDocument));
          } finally { frame.remove(); }
        }
        window.__babbleTemporalLayout = { results };
      })().catch(error => { window.__babbleTemporalLayout = { error: String(error) }; });
      return { started: true };
    })()
  ` }]);
  const result = await waitFor("window.__babbleTemporalLayout", value => value != null);
  await execute([{ type: "eval", code: "delete window.__babbleTemporalLayout; ({cleared:true})" }]);
  assert.equal(result.error, undefined, JSON.stringify(result));
  assert.equal(result.results.length, 3);
  for (const layout of result.results) {
    const context = JSON.stringify(layout);
    assert.equal(layout.visible, true, context);
    assert.equal(layout.fits, true, context);
    assert.equal(layout.overflow, false, context);
    assert.equal(layout.contained, true, context);
    assert.equal(layout.fullComponents, true, context);
    assert.equal(layout.actionsVisible, true, context);
    assert.match(layout.actions, /Social/);
    assert.match(layout.actions, /Protocol/);
    assert.equal(layout.heuristicLabel, true, context);
    assert.equal(layout.radius, 32, context);
    assert.deepEqual(layout.provider, provider);
    assert.ok(Math.abs(Date.now() - Date.parse(layout.evaluated)) < 30000, context);
    assert.ok(layout.score.survival_score >= 0 && layout.score.survival_score <= 1, context);
  }
  console.log("Temporal analytics UI PASS", JSON.stringify(result.results));
}
