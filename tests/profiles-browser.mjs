import assert from "node:assert/strict";

// Read-only real UI/API checks through the parent's existing Aegis connection.
// Requires a signed-in account that has published an Object, a loaded feed, and
// no open modal. Does not publish data, start a server or launch a browser.
export async function verifyPublicProfiles({ postAegisExecute, waitForAegisEval, identityId, responsive = true }) {
  assert.match(identityId, /^id_[a-f0-9]{64}$/);
  await waitForAegisEval(`({
    ready: document.querySelector('[data-status]')?.dataset.state === 'online',
    author: Boolean(document.querySelector('.post-card[data-offset="0"]:not([data-exiting]) [data-profile-author]')),
    modal: Boolean(document.querySelector('dialog[open]'))
  })`, (value) => value?.ready && value.author && !value.modal);
  const started = await postAegisExecute([{ type: "eval", code: `
    (() => {
      window.__babbleProfilesCheck = null;
      const exercise = ${exercisePublicProfile.toString()};
      (async () => {
        const results = [await exercise(document, ${JSON.stringify(identityId)}, true)];
        if (${JSON.stringify(responsive)}) {
          for (const width of [390, 320]) {
            const frame = document.createElement('iframe');
            frame.title = 'Temporary public profile responsive verification';
            frame.style.cssText = 'position:fixed;left:0;top:0;height:844px;border:0;z-index:9999;width:' + width + 'px';
            const url = new URL(location.href);
            url.searchParams.delete('surface');
            frame.src = url.href;
            document.body.append(frame);
            try {
              const deadline = Date.now() + 25000;
              while (!frame.contentDocument?.querySelector('.post-card[data-offset="0"]:not([data-exiting]) [data-profile-author]')) {
                if (Date.now() > deadline) throw new Error('Responsive profile feed did not load at ' + width);
                await new Promise((resolve) => setTimeout(resolve, 50));
              }
              results.push(await exercise(frame.contentDocument, ${JSON.stringify(identityId)}, false));
            } finally { frame.remove(); }
          }
        }
        window.__babbleProfilesCheck = { results };
      })().catch((error) => { window.__babbleProfilesCheck = { error: String(error) }; });
      return true;
    })()
  ` }]);
  assert.equal(started.results?.[0]?.ok, true, JSON.stringify(started));
  const result = await waitForAegisEval("window.__babbleProfilesCheck", (value) => value != null);
  await postAegisExecute([{ type: "eval", code: "delete window.__babbleProfilesCheck; true" }]);
  assert.equal(result.error, undefined, JSON.stringify(result));
  assert.equal(result.results.length, responsive ? 3 : 1);
  assert.equal(result.results[0].ownProfile, true);
  assert.equal(result.results[0].accountDistinct, true);
  for (const entry of result.results) {
    assert.equal(entry.restoredFeed, true, JSON.stringify(entry));
    assert.equal(entry.restoredFocus, true, JSON.stringify(entry));
    assert.equal(entry.profileFits, true, JSON.stringify(entry));
    assert.equal(entry.selectedObject, true, JSON.stringify(entry));
    assert.ok(entry.cardRadius >= 24, JSON.stringify(entry));
    assert.match(entry.cardBackground, /linear-gradient/, JSON.stringify(entry));
  }
  console.log("Public profile UI PASS", JSON.stringify(result.results));
  return result;
}

async function exercisePublicProfile(doc, identityId, ownProfile) {
  const win = doc.defaultView;
  const required = (selector, root = doc) => {
    const element = root.querySelector(selector);
    if (!element) throw new Error(`Missing public profile control: ${selector}`);
    return element;
  };
  const active = () => required('.post-card[data-offset="0"]:not([data-exiting])');
  const pause = (ms = 40) => new Promise((resolve) => win.setTimeout(resolve, ms));
  const until = async (read, predicate, label) => {
    const deadline = Date.now() + 25000;
    while (Date.now() < deadline) {
      const value = read();
      if (predicate(value)) return value;
      await pause();
    }
    throw new Error(`Profile check timed out: ${label}`);
  };
  const check = (condition, message) => { if (!condition) throw new Error(message); };
  const dialog = required('[data-public-profile]');
  const close = required('[data-public-profile-close]');
  const originalId = active().dataset.objectId;
  const originalScroll = active().scrollTop;
  const originalFocus = doc.activeElement;
  const authorButton = required('[data-profile-author]', active());
  const authorId = authorButton.dataset.profileAuthor;
  const api = new URL(doc.documentElement.dataset.babbleApi);
  const response = await win.fetch(new URL('/identities/' + encodeURIComponent(authorId), api), { signal: win.AbortSignal.timeout(15000) });
  check(response.ok, 'Authoritative identity lookup failed');
  const authoritative = await response.json();
  const ready = () => until(() => required('[data-public-profile-status]').dataset.state, (state) => {
    if (state === 'error') throw new Error(required('[data-public-profile-status]').textContent);
    return state === 'ready';
  }, 'profile Objects');
  let result;
  try {
    // Close nonmodal tools through their actual controls before navigating.
    for (const selector of ['[data-close-composer]', '[data-close-settings]', '[data-close-help]', '[data-close-surface]', '[data-close-judgments]']) {
      doc.querySelector(selector)?.click();
    }
    authorButton.focus({ preventScroll: true });
    authorButton.click();
    check(dialog.open, 'Author click must open native profile modal');
    check(doc.activeElement === close, 'Profile initial focus must be on close');
    await ready();
    check(required('[data-public-profile-title]').textContent === authoritative.identity.handle, 'Handle must match authoritative identity');
    check(required('[data-public-profile-identity]').textContent === authorId, 'Profile must retain full canonical identity');
    check(!dialog.querySelector('[data-action="follow"]'), 'Profile must not offer an Object follow as an author follow');
    const rect = dialog.getBoundingClientRect();
    const profileFits = rect.left >= 0 && rect.right <= win.innerWidth + 1 && dialog.scrollWidth <= dialog.clientWidth + 1;
    const row = required('[data-profile-object-id]', dialog);
    const selectedId = row.dataset.profileObjectId;
    const modalScroll = dialog.scrollTop;
    row.click();
    await until(() => active().dataset.objectId, (value) => value === selectedId, 'selected Object in deck');
    check(!dialog.open, 'Selection must close the modal');
    check(doc.activeElement === active(), 'Selection must focus active Object');
    check(!active().inert && active().getAttribute('aria-hidden') !== 'true', 'Selected Object must be interactive');
    const primary = required('.post-primary', active());
    const style = win.getComputedStyle(primary);
    const cardRadius = parseFloat(style.borderRadius);
    const cardBackground = style.backgroundImage;
    check(!required('[data-back-profile]').hidden, 'Selected Object must retain Back to profile');
    required('[data-back-profile]').click();
    check(dialog.open, 'Back must restore the same profile');
    check(doc.activeElement?.dataset.profileObjectId === selectedId, 'Back must focus selected profile row');
    check(Math.abs(dialog.scrollTop - modalScroll) <= 1, 'Back must restore profile reading position');
    close.click();
    await pause(280);
    check(active().dataset.objectId === originalId, 'Closing profile must restore original feed Object');
    check(Math.abs(active().scrollTop - originalScroll) <= 1, 'Closing profile must restore feed reading position');
    const restoredFocus = doc.activeElement === active() || (doc.activeElement?.dataset.profileAuthor === authorId && active().contains(doc.activeElement));
    check(restoredFocus, 'Closing profile must restore focus to the active feed');
    result = { viewport: win.innerWidth, profileFits, selectedObject: true, cardRadius, cardBackground,
      restoredFeed: true, restoredFocus, ownProfile: false, accountDistinct: false };
    if (ownProfile) {
      required('[data-toggle-profile]').click();
      const own = required('[data-menu-action="profile"]');
      const account = required('[data-menu-action="account"]');
      check(!account.hidden && own !== account, 'Public profile and account settings must be separate controls');
      own.click();
      await ready();
      check(required('[data-public-profile-identity]').textContent === identityId, 'My profile must use the authenticated identity');
      check(!required('[data-account-dialog]').open, 'Public profile must not open account settings');
      close.click();
      await pause(250);
      required('[data-toggle-profile]').click();
      account.click();
      check(required('[data-account-dialog]').open && !dialog.open, 'Account settings must retain its own dialog');
      required('[data-account-close]').click();
      result.ownProfile = true;
      result.accountDistinct = true;
    }
  } finally {
    if (dialog.open) close.click();
    if (!required('[data-back-profile]').hidden) { required('[data-back-profile]').click(); close.click(); }
    if (doc.querySelector('[data-account-dialog]')?.open) required('[data-account-close]').click();
    if (originalFocus?.isConnected && !originalFocus.closest('[inert]')) originalFocus.focus({ preventScroll: true });
    if (active().dataset.objectId === originalId) active().scrollTop = originalScroll;
  }
  return result;
}
