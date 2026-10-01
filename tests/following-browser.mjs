import assert from "node:assert/strict";

// Run at the end of the parent's live stack through its existing Aegis runtime.
// Both identities already exist; the target has >=21 public posts matching search.
// IDs must be supplied in the node's newest-first order, including timestamp ties.
export async function verifyFollowing({ postAegisExecute, waitForAegisEval,
  identityId, targetIdentityId, targetHandle, search, targetPostIds }) {
  for (const id of [identityId, targetIdentityId]) assert.match(id, /^id_[a-f0-9]{64}$/);
  assert.notEqual(identityId, targetIdentityId);
  assert.ok(typeof targetHandle === "string" && targetHandle.length > 0);
  assert.ok(typeof search === "string" && search.length > 0);
  assert.ok(targetPostIds.length >= 21);
  for (const id of targetPostIds) assert.match(id, /^obj_[a-f0-9]{64}$/);
  const input = { identityId, targetIdentityId, targetHandle, search, targetPostIds };
  await waitForAegisEval("({ state: document.querySelector('[data-status]')?.dataset.state })", (value) => value?.state === "online");
  const started = await postAegisExecute([{ type: "eval", code: `(() => {
    window.__babelFollowingCheck = null;
    window.__babelFollowingFailure = null;
    window.__babelFollowingProgress = { step: 'starting', since: Date.now() };
    (${exerciseFollowing.toString()})(${JSON.stringify(input)})
      .then((result) => { window.__babelFollowingCheck = result; })
      .catch((error) => { window.__babelFollowingCheck = window.__babelFollowingFailure
        ?? { error: String(error), progress: window.__babelFollowingProgress }; });
    return true;
  })()` }]);
  assert.equal(started.results?.[0]?.ok, true, JSON.stringify(started));
  const snapshot = await waitForAegisEval("({ result: window.__babelFollowingCheck, progress: window.__babelFollowingProgress, failure: window.__babelFollowingFailure })", (value) => value?.result != null);
  const result = snapshot.result;
  await postAegisExecute([{ type: "eval", code: "delete window.__babelFollowingCheck; delete window.__babelFollowingProgress; delete window.__babelFollowingFailure; true" }]);
  assert.equal(result.error, undefined, JSON.stringify(result));
  assert.deepEqual(result.order, targetPostIds);
  for (const key of ["unfollowEmpty", "followPersisted", "privateListHandle", "selfHidden", "unfollowRemoved", "noFallback"]) {
    assert.equal(result[key], true, JSON.stringify(result));
  }
  console.log("Following UI PASS", JSON.stringify(result));
  return result;
}

async function exerciseFollowing(input) {
  const { identityId, targetIdentityId, targetHandle, search, targetPostIds } = input;
  const required = (selector, root = document) => {
    const element = root.querySelector(selector);
    if (!element) throw new Error(`Missing Following control: ${selector}`);
    return element;
  };
  const pause = () => new Promise((resolve) => setTimeout(resolve, 35));
  const mark = (step) => { window.__babelFollowingProgress = { step, since: Date.now() }; };
  const until = async (read, predicate, label) => {
    mark(label);
    const deadline = Date.now() + 25000;
    while (Date.now() < deadline) {
      const value = read();
      if (predicate(value)) return value;
      await pause();
    }
    throw new Error(`Following check timed out: ${label}; ${required('[data-following-status]').textContent}`);
  };
  const check = (condition, message) => { if (!condition) throw new Error(message); };
  const visible = (element) => {
    if (!element || element.closest('[hidden], [inert]')) return false;
    const rect = element.getBoundingClientRect();
    if (rect.width <= 0 || rect.height <= 0) return false;
    for (let node = element; node instanceof Element; node = node.parentElement) {
      const style = getComputedStyle(node);
      if (style.display === 'none' || style.visibility === 'hidden' || style.visibility === 'collapse' || Number(style.opacity) === 0) return false;
    }
    return true;
  };
  const click = (selector, root = document) => {
    mark(`click ${selector}`);
    const control = required(selector, root);
    check(visible(control) && !control.disabled, `Control must be visible and enabled: ${selector}`);
    control.click();
  };
  const openMenu = async () => {
    const menu = required('[data-profile-dropdown]');
    if (menu.hidden || menu.dataset.state !== 'open') click('[data-toggle-profile]');
    await until(() => menu.dataset.state === 'open' && visible(menu), Boolean, 'visible account menu');
  };
  const selectLens = async (lens) => {
    mark(`select ${lens} lens`);
    const settings = required('[data-settings-panel]');
    if (settings.hidden || settings.dataset.state !== 'open') {
      await openMenu();
      click('[data-menu-action="settings"]');
    }
    await until(() => settings.dataset.state === 'open' && visible(settings), Boolean, 'visible settings panel');
    click(`[data-lens="${lens}"]`, settings);
    check(required(`[data-lens="${lens}"]`, settings).getAttribute('aria-pressed') === 'true', 'Selected lens must be active');
    click('[data-close-settings]', settings);
    await until(() => settings.hidden, Boolean, 'settings closed after lens selection');
  };
  const active = () => required('.post-card[data-offset="0"]:not([data-exiting])');
  const dialog = required('[data-public-profile]');
  const follow = required('[data-author-follow]');
  const visibilityDetails = (element) => {
    const ancestors = [];
    for (let node = element; node instanceof Element; node = node.parentElement) {
      const style = getComputedStyle(node);
      const rect = node.getBoundingClientRect();
      ancestors.push({ tag: node.tagName, className: node.className, hidden: node.hidden,
        inert: node.inert, disabled: node.disabled, open: node.open, dataset: { ...node.dataset },
        display: style.display, visibility: style.visibility, opacity: style.opacity,
        rect: { x: rect.x, y: rect.y, width: rect.width, height: rect.height } });
    }
    return { visible: visible(element), ancestors };
  };
  const captureFailure = (cause) => {
    window.__babelFollowingFailure ??= {
      error: String(cause), progress: { ...window.__babelFollowingProgress },
      dialog: visibilityDetails(dialog), follow: visibilityDetails(follow),
      followStatus: required('[data-author-follow-status]').textContent,
      profileStatus: required('[data-public-profile-status]').textContent,
      feedStatus: required('[data-following-status]').textContent,
    };
  };
  const closeProfile = () => { if (dialog.open) click('[data-public-profile-close]'); };
  const setSearch = () => {
    mark('search target posts');
    check(visible(required('[data-search-input]')), 'Search input must be visible');
    required('[data-search-input]').value = search;
    click('button[type="submit"]', required('[data-search-form]'));
  };
  const feedReady = () => until(() => required('[data-following-status]').dataset.state, (state) => {
    if (state === 'error') throw new Error(required('[data-following-status]').textContent);
    return state === 'ready';
  }, 'Following page');
  const followReady = () => until(() => dialog.open && visible(dialog) && visible(follow)
    && !follow.disabled && ['true', 'false'].includes(follow.dataset.following), Boolean, 'visible profile and current follow state');
  const openTarget = async () => {
    closeProfile();
    await selectLens('balanced');
    setSearch();
    await until(() => required('[data-status]').dataset.state, (state) => {
      if (state === 'error') throw new Error(`Balanced search failed: ${required('[data-error]').textContent}`);
      return state === 'online';
    }, 'Balanced search completed');
    // Search constrains public discovery to matching Objects; the selected Lens
    // still ranks those matches. Enter the target profile through a real card.
    const count = Number(required('[data-count-label]').textContent);
    check(Number.isSafeInteger(count) && count > 0, 'Balanced search returned no profile entry points');
    const visited = [];
    let found = false;
    for (let index = 0; index < count; index++) {
      const card = active();
      const author = required('[data-profile-author]', card);
      visited.push({ objectId: card.dataset.objectId, authorId: author.dataset.profileAuthor });
      if (author.dataset.profileAuthor === targetIdentityId) {
        await until(() => visible(author), Boolean, 'visible target author in active card');
        found = true;
        break;
      }
      if (index + 1 < count) {
        const previous = card.dataset.objectId;
        click('[data-next]');
        await until(() => active().dataset.objectId !== previous
          && visible(required('[data-profile-author]', active())), Boolean, `Balanced card ${index + 2} of ${count}`);
      }
    }
    check(found, `Target author ${targetIdentityId} absent from ${count} loaded Balanced cards: ${JSON.stringify(visited)}`);
    click('[data-profile-author]', active());
    await followReady();
    await until(() => required('[data-public-profile-title]').textContent, (value) => value === targetHandle, 'authoritative profile handle');
    check(required('[data-public-profile-identity]').textContent === targetIdentityId, 'Wrong target profile');
    check(!follow.hidden, 'Other author must expose Follow');
  };
  const setFollow = async (desired) => {
    await followReady();
    if (follow.dataset.following !== String(desired)) {
      click('[data-author-follow]');
      check(follow.disabled, 'Pending mutation must disable duplicate clicks');
      await followReady();
    }
    check(follow.dataset.following === String(desired), 'Follow mutation was not confirmed by current server state');
    check(!required('[data-author-follow-status]').textContent, 'Follow mutation should finish without an error');
  };
  const followingFeed = async () => {
    closeProfile();
    await selectLens('following');
    setSearch();
    await feedReady();
  };
  let initial = false;
  let captured = false;
  try {
    for (const selector of ['[data-close-composer]', '[data-close-settings]', '[data-close-help]', '[data-close-surface]', '[data-close-judgments]']) {
      if (visible(document.querySelector(selector))) click(selector);
    }
    if (required('[data-account-dialog]').open) click('[data-account-close]');
    closeProfile();
    if (!required('[data-back-profile]').hidden) {
      click('[data-back-profile]');
      await until(() => visible(dialog), Boolean, 'visible resumed public profile');
      closeProfile();
    }
    await openTarget();
    initial = follow.dataset.following === 'true'; captured = true;
    await setFollow(false);
    await followingFeed();
    check(required('[data-count-label]').textContent === '0', 'Unfollowed author leaked into Following');
    check(!required('[data-empty]').hidden, 'Empty Following should be visible');
    check(required('[data-following-more]').hidden, 'Finished empty feed must not offer pagination');
    await openTarget(); await setFollow(true); closeProfile();
    await openTarget(); check(follow.dataset.following === 'true', 'Reopening must fetch persisted follow state');
    await followingFeed();
    const firstCount = Number(required('[data-count-label]').textContent);
    check(firstCount > 0 && firstCount <= 20, 'First Following page must respect limit 20');
    check(!required('[data-following-more]').hidden, '21 matching posts require Load more');
    for (let page = 0; page < 50 && !required('[data-following-more]').hidden; page++) {
      required('[data-following-more]').focus();
      click('[data-following-more]');
      await feedReady();
    }
    check(required('[data-following-more]').hidden, 'Pagination did not reach the end');
    check(Number(required('[data-count-label]').textContent) === targetPostIds.length, 'Following must include all and only seeded matching posts');
    const order = [];
    for (let index = 0; index < targetPostIds.length; index++) {
      const card = active();
      order.push(card.dataset.objectId);
      check(required('[data-profile-author]', card).dataset.profileAuthor === targetIdentityId, 'Following leaked another author');
      click('[data-next]');
    }
    check(JSON.stringify(order) === JSON.stringify(targetPostIds), 'Following must retain chronological order through pagination');
    click('[data-following-people]');
    await until(() => required('[data-following-list-status]').dataset.state === 'ready'
      && visible(required('[data-following-dialog]')), Boolean, 'visible private following list');
    const row = required(`[data-following-author="${targetIdentityId}"]`);
    check(required('strong', row).textContent === targetHandle, 'Private list must display the authoritative handle');
    click(`[data-following-author="${targetIdentityId}"]`); await followReady(); await setFollow(false); closeProfile();
    await feedReady();
    check(required('[data-count-label]').textContent === '0', 'Unfollow must invalidate the Following feed');
    check(!required('[data-empty]').hidden, 'Unfollow must restore the empty state');
    click('[data-following-people]');
    await until(() => required('[data-following-list-status]').dataset.state === 'ready'
      && visible(required('[data-following-dialog]')), Boolean, 'visible updated private list');
    check(!document.querySelector(`[data-following-author="${targetIdentityId}"]`), 'Unfollow must remove private list row');
    click('[data-following-list-close]');
    await openMenu(); click('[data-menu-action="profile"]');
    await until(() => visible(dialog), Boolean, 'visible own public profile');
    check(required('[data-public-profile-identity]').textContent === identityId, 'Own public profile must use signed-in author');
    check(follow.hidden, 'Self-follow control must be hidden');
    closeProfile();
    return { firstCount, order, unfollowEmpty: true, followPersisted: true, privateListHandle: true,
      selfHidden: true, unfollowRemoved: true, noFallback: true };
  } catch (cause) {
    captureFailure(cause);
    throw cause;
  } finally {
    try {
      if (dialog.open) await until(() => visible(dialog), Boolean, 'visible profile before cleanup');
      if (required('[data-following-dialog]').open) {
        await until(() => visible(required('[data-following-dialog]')), Boolean, 'visible following list before cleanup');
        click('[data-following-list-close]');
      }
      closeProfile();
      if (captured && initial) { await openTarget(); await setFollow(true); closeProfile(); await followingFeed(); }
    } catch (cause) {
      if (window.__babelFollowingFailure) window.__babelFollowingFailure.cleanupError = String(cause);
      else { captureFailure(cause); throw cause; }
    }
  }
}
