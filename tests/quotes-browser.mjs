import assert from "node:assert/strict";

export async function verifyQuoteVisit(evaluate, waitFor, sourceId, targetId, depth = 0) {
  const ready = await waitFor(`(() => {
    const card = document.querySelector('.post-card[data-offset="0"]');
    const section = card?.querySelector('[data-quotes-root]');
    const button = section?.querySelector('button[data-quote-target="${targetId}"]');
    return {source:card?.dataset.objectId, ready:!!button && !button.disabled && !section.hidden,
      outsidePrimary:!!section && !section.closest('.post-primary'),
      executable:!!section?.querySelector('iframe, audio, video')};
  })()`, value => value.source === sourceId && value.ready);
  assert.equal(ready.outsidePrimary, true, "quoted context is a sibling below the primary card");
  assert.equal(ready.executable, false, "quote previews must not mount executable or autoplaying content");
  const saved = await evaluate(`(() => {
    const card = document.querySelector('.post-card[data-offset="0"]');
    const button = card.querySelector('button[data-quote-target="${targetId}"]');
    button.scrollIntoView({block:'center',behavior:'instant'}); button.focus({preventScroll:true});
    const top = card.scrollTop;
    button.click();
    return {top, height:card.clientHeight, scrollHeight:card.scrollHeight};
  })()`);
  console.log("Quote visit reading", JSON.stringify({sourceId,targetId,...saved}));
  const opened = await waitFor(`(() => {
    const card = document.querySelector('.post-card[data-offset="0"]');
    return {id:card?.dataset.objectId, back:!document.querySelector('[data-back-object]')?.hidden,
      surface:!!card?.querySelector('.post-primary[data-surface-open="true"]'),
      playing:[...card?.querySelectorAll('audio,video')??[]].some(media=>!media.paused)};
  })()`, value => value.id === targetId && value.back);
  assert.equal(opened.surface, false); assert.equal(opened.playing, false);
  if (depth < 2) {
    const nested = await waitFor(`(() => {
      const card = document.querySelector('.post-card[data-offset="0"]:not([data-exiting])');
      const section = card?.querySelector('[data-quotes-root]');
      return {id:card?.dataset.objectId, ready:!!section && section.getAttribute('aria-busy')==='false',
        target:section?.querySelector('button[data-quote-target]')?.dataset.quoteTarget};
    })()`, value => value.id === targetId && value.ready);
    if (nested.target && nested.target !== sourceId && nested.target !== targetId)
      await verifyQuoteVisit(evaluate, waitFor, targetId, nested.target, depth + 1);
  }
  await evaluate("document.querySelector('[data-back-object]').click(); ({back:true})");
  const returned = await waitFor(`(() => {
    const card = document.querySelector('.post-card[data-offset="0"]');
    return {id:card?.dataset.objectId, top:card?.scrollTop, expectedTop:${saved.top},
      height:card?.clientHeight, scrollHeight:card?.scrollHeight,
      focused:document.activeElement?.dataset.quoteTarget,
      backHidden:document.querySelector('[data-back-object]')?.hidden};
  })()`, value => value.id === sourceId && Math.abs(value.top - saved.top) <= 1);
  assert.equal(returned.focused, targetId);
  assert.equal(returned.backHidden, depth === 0);
  console.log("Quote context navigation PASS", JSON.stringify({sourceId,targetId,scrollRestored:true}));
}
