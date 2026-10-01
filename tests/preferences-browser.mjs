import assert from "node:assert/strict";

export async function verifyFeedPreferences({ execute, waitFor, navigate, identityId, targetIdentityId, password, search }) {
  const evaluate = async code => {
    const response = await execute([{ type: "eval", code }]);
    assert.equal(response.results[0].ok, true, JSON.stringify(response.results[0]));
    return response.results[0].value;
  };
  const input = { identityId, targetIdentityId, password, search };
  const run = async stage => {
    await evaluate(`(() => {
      window.__preferencesProbe = null;
      (${exercisePreferences.toString()})(${JSON.stringify(input)}, ${JSON.stringify(stage)})
        .then(value => { window.__preferencesProbe = value; })
        .catch(error => { window.__preferencesProbe = {error:String(error), step:window.__preferencesStep}; });
      return true;
    })()`);
    const result = await waitFor("window.__preferencesProbe", value => value != null);
    assert.equal(result.error, undefined, JSON.stringify(result));
    return result;
  };
  const configured = await run("configure");
  assert.equal(configured.immediateHide, true);
  assert.equal(configured.undo, true);
  assert.deepEqual(configured.filteredLenses, ["following", "balanced", "research", "weird"]);
  assert.equal(configured.historyCleared, true);
  const page = await evaluate("({url:location.href})");
  const reload = new URL(page.url);
  reload.searchParams.set("preferences_reload", String(Date.now()));
  await navigate(reload.href);
  await waitFor("({newDocument:window.__preferencesProbe === undefined,ready:document.querySelector('[data-status]')?.dataset.state})",
    value => value.newDocument && value.ready === "online");
  const restored = await run("restore");
  assert.equal(restored.reload, true);
  assert.equal(restored.accountIsolation, true);
  const layout = await run("layout");
  assert.deepEqual(layout.widths, [1280, 390, 320]);
  await run("cleanup");
  console.log("Feed preferences UI PASS", JSON.stringify({ ...configured, ...restored, ...layout }));
}

async function exercisePreferences({ identityId, targetIdentityId, password, search }, stage) {
  const required = (selector, root = document) => {
    const node = root.querySelector(selector);
    if (!node) throw Error(`Missing preferences control: ${selector}`);
    return node;
  };
  const check = (value, label) => { if (!value) throw Error(label); };
  const pause = () => new Promise(resolve => setTimeout(resolve, 40));
  const until = async (label, predicate) => {
    window.__preferencesStep = label;
    const deadline = Date.now() + 20000;
    while (!predicate()) {
      if (Date.now() > deadline) throw Error(`Timed out: ${label}; ${document.querySelector('[data-preferences-status]')?.textContent}`);
      await pause();
    }
  };
  const click = (selector, root = document) => {
    const node = required(selector, root);
    check(!node.disabled && !node.closest('[hidden], [inert]') && node.getClientRects().length > 0,
      `Preferences control unavailable: ${selector}`);
    node.click();
  };
  const field = name => required(`[data-preference-field="${name}"]`);
  const tab = name => click(`[data-preference-tab="${name}"]`);
  const panel = required('[data-settings-panel]');
  const origin = new URL(document.documentElement.dataset.babbleApi).origin;
  const key = kind => 'babble.local:' + JSON.stringify([origin, identityId, kind]);
  const saved = () => JSON.parse(localStorage.getItem(key('preferences')) ?? 'null');
  const ready = () => until('feed ready', () => required('[data-status]').dataset.state === 'online');
  const open = async () => {
    if (!panel.hidden && panel.dataset.state === 'open') return;
    click('[data-toggle-profile]');
    await until('account menu', () => required('[data-profile-dropdown]').dataset.state === 'open');
    click('[data-menu-action="settings"]');
    await until('settings open', () => panel.dataset.state === 'open');
  };
  const close = async () => {
    click('[data-close-settings]');
    await until('settings closed', () => panel.hidden);
  };
  const apply = async () => {
    click('[data-preferences-apply]');
    check(required('[data-preferences-status]').dataset.state !== 'error', required('[data-preferences-status]').textContent);
    await ready();
  };
  const lens = async name => { click(`[data-lens="${name}"]`, panel); await ready(); };
  const active = () => document.querySelector('.post-card[data-offset="0"]:not([data-exiting])');
  const setFollow = async desired => {
    click('[data-profile-author]', active());
    await until('author follow state', () => required('[data-public-profile]').open
      && !required('[data-author-follow]').disabled && ['true', 'false'].includes(required('[data-author-follow]').dataset.following));
    if (required('[data-author-follow]').dataset.following !== String(desired)) click('[data-author-follow]');
    await until('author follow confirmed', () => !required('[data-author-follow]').disabled
      && required('[data-author-follow]').dataset.following === String(desired));
    click('[data-public-profile-close]');
  };
  if (stage === 'configure') {
    for (const selector of ['[data-composer-panel]', '[data-help-panel]', '[data-judgment-panel]']) {
      const section = document.querySelector(selector);
      if (section && !section.hidden) section.querySelector('button[aria-label^="Close"]')?.click();
    }
    await open(); await lens('balanced'); await close();
    required('[data-search-input]').value = search; required('[data-search-form]').requestSubmit();
    await ready();
    await setFollow(true);
    await open(); await lens('following'); await close();
    await ready();
    const post = active();
    check(required('[data-profile-author]', post).dataset.profileAuthor === targetIdentityId, 'Expected followed author');
    click('.action-popover[data-kind="protocol"] > button', post);
    await until('Object actions', () => required('.popover-panel[data-kind="protocol"]', post).dataset.state === 'open');
    click('[data-action="hide-author"]', post);
    const immediateHide = !active();
    check(immediateHide, 'Hidden author remained active while refreshing');
    await ready();
    check(saved().hiddenAuthors.includes(targetIdentityId), 'Hide was not persisted');
    click('[data-undo-hide-author]');
    await ready();
    check(!saved().hiddenAuthors.includes(targetIdentityId) && !!active(), 'Undo did not restore author');
    await open();
    field('interests').value = 'privateinterestmarker';
    field('expertise').value = 'privateexpertisemarker';
    tab('ranking');
    for (const [name, value] of [['noveltyTolerance', 0], ['explorationPreference', 100], ['evidencePreference', 25], ['contradictionTolerance', 75]]) {
      field(name + 'Default').click(); field(name).value = String(value);
      field(name).dispatchEvent(new Event('input', {bubbles:true}));
    }
    field('creatorAffinityAuthorId').value = targetIdentityId;
    click('[data-preferences-add-affinity]');
    field('creatorAffinity').value = '65';
    tab('filters'); field('hiddenTerms').value = 'post'; field('mutedTerms').value = 'privatemutedmarker';
    await apply();
    const value = saved();
    check(value.noveltyTolerance === 0 && value.explorationPreference === 1 && value.evidencePreference === .25
      && value.contradictionTolerance === .75 && value.creatorAffinity[targetIdentityId] === .65, 'Ranking preferences lost values');
    const filteredLenses = [];
    for (const name of ['following', 'balanced', 'research', 'weird']) {
      await lens(name);
      if (name === 'following') {
        check(required('[data-count-label]').textContent === '0', 'Following word filter failed');
        check(!required('[data-empty]').hidden && !required('[data-open-feed-preferences]').hidden, 'Missing filter recovery');
      } else {
        check(/Local [1-9][0-9]* filtered/.test(required('[data-local-label]').textContent), `No filtered candidates in ${name}`);
        for (const card of document.querySelectorAll('.post-card:not([data-exiting]) .post-primary')) {
          check(!/\bpost\b/i.test(card.innerText), `Excluded word remained in ${name}`);
        }
      }
      filteredLenses.push(name);
    }
    tab('filters'); field('hiddenTerms').value = ''; await apply();
    check(!!active(), 'Clearing word filter did not restore posts');
    const before = JSON.stringify(saved());
    const sessionBefore = sessionStorage.getItem('babble.session.v1:' + origin);
    tab('local-data'); click('[data-preferences-clear-history]'); click('[data-preferences-cancel]');
    check(localStorage.getItem(key('seen')) !== null, 'Cancel cleared history');
    click('[data-preferences-clear-history]'); click('[data-preferences-confirm="clear-history"]');
    await ready();
    check(localStorage.getItem(key('seen')) === null, 'History returned without a new card visit');
    check(JSON.stringify(saved()) === before && sessionStorage.getItem('babble.session.v1:' + origin) === sessionBefore,
      'Clearing history changed preferences or account');
    await close();
    return {immediateHide, undo:true, filteredLenses, historyCleared:true};
  }
  if (stage === 'restore') {
    await open();
    check(field('interests').value === 'privateinterestmarker', 'Preference missing after reload');
    const before = JSON.stringify(saved());
    await close();
    click('[data-toggle-profile]');
    await until('logout menu', () => required('[data-profile-dropdown]').dataset.state === 'open');
    click('[data-menu-action="sign-out"]');
    await until('sign-out confirmation', () => required('[data-account-dialog]').open
      && !required('[data-security-confirm]').disabled && required('[data-security-confirm]').getClientRects().length > 0);
    click('[data-security-confirm]');
    await until('signed out', () => sessionStorage.getItem('babble.session.v1:' + origin) === null);
    click('[data-account-close]');
    await ready(); await open();
    check(field('interests').value === '' && required('[data-preferences-scope]').textContent.endsWith('Guest'), 'Account preferences leaked to guest');
    check(JSON.stringify(saved()) === before, 'Logout deleted private preferences');
    await close(); click('[data-toggle-profile]');
    await until('login menu', () => required('[data-profile-dropdown]').dataset.state === 'open');
    click('[data-menu-action="profile"]');
    await until('account dialog', () => required('[data-account-dialog]').open);
    click('[data-account-mode="login"]');
    required('[data-account-login]').value = identityId;
    required('[data-account-password]').value = password;
    required('[data-account-form]').requestSubmit();
    await until('signed in', () => required('[data-account-status]').textContent === 'Signed in');
    click('[data-account-close]'); await ready(); await open();
    check(field('interests').value === 'privateinterestmarker', 'Sign in did not restore account preferences');
    await close(); return {reload:true, accountIsolation:true};
  }
  if (stage === 'layout') {
    const widths = [];
    for (const width of [1280, 390, 320]) {
      const frame = document.createElement('iframe');
      frame.title = 'Preferences layout verification';
      frame.style.cssText = `position:fixed;inset:0;width:${width}px;height:844px;border:0;z-index:9999`;
      frame.src = location.href; document.body.append(frame);
      try {
        await until(`frame ${width}`, () => frame.contentDocument?.querySelector('[data-status]')?.dataset.state === 'online');
        const doc = frame.contentDocument;
        const win = frame.contentWindow;
        click('[data-toggle-profile]', doc);
        await until('frame menu', () => required('[data-profile-dropdown]', doc).dataset.state === 'open');
        click('[data-menu-action="settings"]', doc);
        const settings = required('[data-settings-panel]', doc);
        await until('frame settings', () => settings.dataset.state === 'open');
        await Promise.all(settings.getAnimations().map(animation => animation.finished));
        for (const name of ['interests', 'filters', 'ranking', 'local-data']) {
          click(`[data-preference-tab="${name}"]`, doc);
          const box = settings.getBoundingClientRect();
          check(box.left >= -1 && box.right <= width + 1 && settings.scrollWidth <= settings.clientWidth + 1,
            `Preferences overflow ${width}/${name}`);
          for (const node of settings.querySelectorAll('button, textarea, input, select')) {
            if (node.closest('[hidden]') || !node.getClientRects().length || win.getComputedStyle(node).visibility === 'hidden') continue;
            const bounds = node.getBoundingClientRect();
            if (node.type === 'checkbox') {
              const label = node.closest('label'), height = label.getBoundingClientRect().height;
              check(label.offsetHeight >= 44 && height >= 43.9, `Small checkbox label: ${width}/${name}, ${height}px`);
            }
            else check(bounds.height >= 43.9, `Small target: ${node.outerHTML.slice(0,120)}`);
            check(bounds.left >= box.left && bounds.right <= box.right, 'Control exceeds panel width');
          }
        }
        widths.push(width);
      } finally { frame.remove(); }
    }
    return {widths};
  }
  await open(); tab('local-data');
  click('[data-preferences-reset]'); click('[data-preferences-confirm="reset"]'); await ready();
  check(saved().interests.length === 0 && saved().hiddenAuthors.length === 0 && saved().noveltyTolerance === null,
    'Reset did not restore defaults');
  await lens('following'); await close();
  await setFollow(false); await ready();
  delete window.__preferencesStep;
  return {reset:true};
}
