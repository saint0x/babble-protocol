import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";

// Uses the existing Aegis runtime and Astro preview; never creates accounts or
// writes backend data. Actual production DOM/view/controller run with test IO.
const runtime = process.env.AEGIS_REACTIONS_URL ?? "http://127.0.0.1:7879";
async function execute(code) {
  const response = await fetch(`${runtime}/execute`, { method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ commands: [{ type: "eval", code }] }), signal: AbortSignal.timeout(15_000) });
  assert.equal(response.ok, true);
  const body = await response.json();
  assert.equal(body.results?.[0]?.ok, true, JSON.stringify(body));
  return body.results[0].value;
}

const code = `(() => {
  window.__reactionDOMResult = { pending: true };
  (async () => {
    const { ReactionView, ReactionPanel } = await import('/src/app/reaction-view.ts');
    await import('/src/styles/reactions.css');
    const check = (condition, message) => { if (!condition) throw new Error(message); };
    const empty = () => ({ appreciation: null, engagement: null, stance: null, certainty: null });
    const object = 'obj_' + 'c'.repeat(64), actor = 'id_' + 'a'.repeat(64);
    const summary = { object_id: object, participants: 2, likes: 1, dislikes: 1, engaging: 1, not_engaging: 1, support: 1, oppose: 0, uncertain: 1, certainty_responses: 1 };
    const baseline = { object, actor, generation: 1, summary, state: { author_id: actor, object_id: object, revision: 1, value: empty() }, busy: false, retry: false, confirmation: null, message: '', needsRefresh: false };
    const writes = [], actions = [];
    const priorFocus = document.activeElement;
    const mount = document.createElement('div');
    mount.style.cssText = 'position:fixed;left:0;top:0;width:500px;padding:20px;background:#eff3f0;z-index:2147483647;';
    document.body.append(mount);
    let panel;
    try {
      const view = new ReactionView({ publish: value => writes.push(value), confirm: () => actions.push('confirm'), cancel: () => actions.push('cancel'), retry: () => actions.push('retry'), refresh: () => actions.push('refresh'), signIn: () => actions.push('signin') });
      mount.append(view.element); view.render(baseline);
      const thumbs = [...view.element.querySelectorAll('.reaction-appreciation')];
      check(thumbs.length === 2 && thumbs.every(button => button.querySelector('svg')), 'Real Lucide thumb icons');
      check(thumbs[0].getAttribute('aria-pressed') === 'false', 'Initial unselected thumb');
      const details = view.element.querySelector('details'), disclosure = details.querySelector('summary');
      const summaryRect = disclosure.getBoundingClientRect(), thumbRect = thumbs[0].getBoundingClientRect();
      check(Math.abs(summaryRect.top - thumbRect.top) < 4, 'Closed bar and disclosure align horizontally');
      check(getComputedStyle(view.element.querySelector('form')).display === 'none', 'Native disclosure hides form');
      disclosure.click(); check(details.open, 'Native disclosure opens');
      const selects = view.element.querySelectorAll('select'), slider = view.element.querySelector('input[type=range]'), include = view.element.querySelector('input[type=checkbox]');
      for (const input of [...selects, slider, include]) check(view.element.querySelector('label[for="' + input.id + '"]'), 'Every field labelled');
      check(slider.disabled && !include.checked, 'No default confidence submission');
      selects[1].value = 'uncertain'; selects[1].dispatchEvent(new Event('change'));
      view.element.querySelector('form').requestSubmit();
      check(writes.at(-1).stance === 'uncertain' && writes.at(-1).certainty === null, 'Uncertain is a position with no implicit confidence');
      include.click(); slider.value = '0'; slider.dispatchEvent(new Event('input'));
      view.element.querySelector('form').requestSubmit();
      check(writes.at(-1).certainty === 0, 'Explicit confidence zero retained');
      check(view.element.querySelector('output').value === '0%', 'Output tracks slider');
      slider.focus(); view.render({ ...baseline, message: 'Loading reactions...', busy: true });
      check(details.open && slider.isConnected, 'Busy render preserves DOM and disclosure');
      view.render({ ...baseline, state: { ...baseline.state } });
      check(selects[1].value === 'uncertain' && slider.value === '0', 'Same revision readback preserves unsaved draft');
      view.render({ ...baseline, confirmation: { ...empty(), appreciation: 'like' } });
      const notice = view.element.querySelector('.reaction-notice');
      check(!notice.hidden && /signed, public/.test(notice.textContent) && /does not erase signed history/.test(notice.textContent), 'Explicit signed-public retained-history notice');
      check(document.activeElement === notice.querySelector('button'), 'Confirmation receives keyboard focus');
      notice.querySelector('button').click(); check(actions.at(-1) === 'confirm', 'Confirmation wired');
      view.render({ ...baseline, state: { ...baseline.state, revision: 2, value: { ...empty(), appreciation: 'like' } } });
      check(thumbs[0].getAttribute('aria-pressed') === 'true', 'Committed pressed state');
      thumbs[0].click(); check(writes.at(-1).appreciation === null, 'Pressed thumb clears real value');
      [...view.element.querySelectorAll('button')].find(button => button.textContent === 'Withdraw all').click();
      check(Object.values(writes.at(-1)).every(value => value === null), 'Withdraw clears all dimensions');
      view.render({ ...baseline, actor: null, state: null });
      thumbs[0].click(); check(actions.at(-1) === 'signin', 'Guest thumb requests login');
      check(thumbs[0].textContent === '1', 'Guest sees authoritative count');
      view.render({ ...baseline, state: null, summary: null, retry: true, needsRefresh: true, message: 'Unconfirmed' });
      check(thumbs[0].disabled && thumbs[0].textContent === '', 'No fake zero for missing totals');
      [...view.element.querySelectorAll('button')].find(button => button.textContent === 'Retry same change').click();
      check(actions.at(-1) === 'retry', 'Exact retry action wired');
      [...view.element.querySelectorAll('button')].find(button => button.textContent === 'Refresh').click();
      check(actions.at(-1) === 'refresh', 'Refresh action wired');
      view.render(baseline); details.open = true;
      for (const width of [320, 390, 768]) {
        mount.style.width = width + 'px';
        check(view.element.scrollWidth <= view.element.clientWidth + 1, 'No reaction overflow at width ' + width + ': ' + JSON.stringify({ client: view.element.clientWidth, scroll: view.element.scrollWidth, tracks: getComputedStyle(view.element).gridTemplateColumns, bar: view.element.querySelector('.reaction-bar').getBoundingClientRect().width, disclosure: { width: disclosure.getBoundingClientRect().width, css: getComputedStyle(disclosure).cssText }, offenders: [...view.element.querySelectorAll('*')].filter(el => el.getBoundingClientRect().right > mount.getBoundingClientRect().right).map(el => [el.tagName,el.className,el.getBoundingClientRect().width]) }));
        for (const input of selects) check(input.getBoundingClientRect().right <= mount.getBoundingClientRect().right, 'Select remains in container');
        view.render({ ...baseline, actor: null, state: null, summary: { ...summary, likes: Number.MAX_SAFE_INTEGER, dislikes: Number.MAX_SAFE_INTEGER } });
        check(view.element.scrollWidth <= view.element.clientWidth + 1, 'Guest controls and large counts fit width ' + width);
        check(thumbs[0].getAttribute('aria-label').includes(String(Number.MAX_SAFE_INTEGER)), 'Accessible label retains exact count');
        view.render(baseline);
      }
      view.element.remove();
      const accounts = new EventTarget();
      accounts.current = { identity: { id: actor, handle: 'test' }, token: 'a'.repeat(64), expires_at: '2099-01-01T00:00:00Z' };
      accounts.origin = new URL(location.origin);
      let reads = 0;
      const io = async (url) => { reads++; return Response.json(url.pathname.endsWith('/mine') ? baseline.state : summary); };
      accounts.authenticatedFetch = io;
      panel = new ReactionPanel(accounts, { signIn() {}, publicFetch: io });
      await panel.select(object, mount, accounts.current);
      const input = panel.view.element.querySelector('select'); panel.view.element.querySelector('details').open = true;
      input.value = 'engaging'; input.dispatchEvent(new Event('change')); input.focus();
      await panel.select(object, mount, accounts.current);
      check(reads === 2 && document.activeElement === input && input.value === 'engaging', 'Same card repeated select preserves focus, draft and network');
      const replacement = document.createElement('div'); mount.append(replacement);
      await panel.select(object, replacement, accounts.current);
      check(reads === 2 && panel.view.element.parentElement === replacement && document.activeElement === input, 'New container moves existing panel without rereading');
      await panel.select(null); check(!panel.view.element.isConnected, 'No active card detaches');
      const column = document.createElement('article');
      column.className = 'post-card';
      column.style.cssText = 'position:relative;inset:auto;height:260px;width:400px;margin:0;padding:20px;transform:none';
      const content = document.createElement('div'); content.style.height = '400px';
      const context = document.createElement('div'); context.className = 'post-context'; context.style.minHeight = '1px';
      column.append(content, context); mount.append(column);
      await panel.select(object, context, accounts.current);
      column.scrollTop = column.scrollHeight;
      const readingTop = column.scrollTop, contextHeight = context.offsetHeight;
      check(readingTop > 0 && contextHeight > 40, 'Scrolled card includes actual reaction controls');
      await panel.select(object, replacement, accounts.current);
      check(context.offsetHeight >= contextHeight && Math.abs(column.scrollTop - readingTop) <= 1,
        'Moving reactions away does not collapse the card or clamp its reading position');
      await panel.select(object, context, accounts.current);
      check(context.style.minHeight === '1px' && Math.abs(column.scrollTop - readingTop) <= 1,
        'Returning reactions releases the reservation and keeps the same reading position');
      return { passed: true, layouts: [320,390,768], realDOM: true, productionModules: true, swipeReadingPreserved: true };
    } finally { panel?.dispose(); mount.remove(); if (priorFocus instanceof HTMLElement) priorFocus.focus({ preventScroll: true }); }
  })().then(result => window.__reactionDOMResult = result, error => window.__reactionDOMResult = { error: String(error.stack || error) });
  return { started: true };
})()`;
await execute(code);
let result;
for (let attempt = 0; attempt < 100; attempt++) {
  result = await execute("({...window.__reactionDOMResult})");
  if (!result?.pending) break;
  await delay(100);
}
assert.equal(result?.passed, true, JSON.stringify(result));
console.log("Reaction DOM PASS", JSON.stringify(result));
await execute("delete window.__reactionDOMResult; ({ cleaned: true })");
