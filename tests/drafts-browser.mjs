import assert from "node:assert/strict";

// Aegis executes the page's existing event handlers; this helper never submits.
export async function verifyComposerDrafts(execute, waitFor, identityId) {
  assert.ok(identityId, "draft regression requires the authenticated account ID");
  await waitFor(`({
    online: document.querySelector('[data-status]')?.dataset.state === 'online',
    signedIn: Boolean(document.querySelector('[data-author-handle]')?.value),
    closed: document.querySelector('[data-composer-panel]')?.hidden === true,
    cards: document.querySelectorAll('[data-deck] .post-card[data-object-id]').length
  })`, (value) => value?.online && value.signedIn && value.closed && value.cards >= 2);

  const response = await execute([{ type: "eval", code: `(${exerciseDraftControls.toString()})(${JSON.stringify(identityId)})` }]);
  assert.equal(response.results.length, 1);
  const result = response.results[0];
  assert.equal(result.ok, true, JSON.stringify(result));
  assert.equal(result.value.checks.length, 16);
  for (const check of result.value.checks) {
    assert.deepEqual(check.actual, check.expected, check.name);
  }
  assert.equal(result.value.restored, true, "test must restore only the drafts it touched");
  assert.ok(result.value.navigationCount > 0, "draft targets must be reached through deck navigation");
  assert.equal(result.value.restoredActiveCard, true, "test must return to the original active Object");
  await waitFor("({ hidden: document.querySelector('[data-composer-panel]').hidden })", (value) => value?.hidden === true);
  console.log("Composer draft UI isolation PASS", JSON.stringify(result.value.targets));
}

function exerciseDraftControls(identityId) {
  const required = (selector, parent = document) => {
    const element = parent.querySelector(selector);
    if (!element) throw new Error(`Missing draft test control: ${selector}`);
    return element;
  };
  const cards = [...document.querySelectorAll('[data-deck] .post-card[data-object-id]')];
  const cardA = cards.find((card) => card.dataset.offset === "0");
  const cardB = cards.find((card) => card.dataset.offset === "1")
    ?? cards.find((card) => card.dataset.offset === "-1");
  if (!cardA || !cardB) throw new Error("Draft regression requires two distinct feed Objects");
  const directionToB = Number(cardB.dataset.offset);
  const textarea = required("[data-compose-text]");
  const media = required("[data-compose-media]");
  const submit = required("[data-compose-submit]");
  const panel = required("[data-composer-panel]");
  const close = required("[data-close-composer]");
  const status = required("[data-author-status]");
  const previousStatus = { text: status.textContent, state: status.dataset.state };
  const previousFocus = document.activeElement;
  const scroll = cards.map((card) => [card, card.scrollTop]);
  const popovers = [cardA, cardB].map((card) => {
    const button = required('.action-popover[data-kind="social"] > button', card);
    return { button, expanded: button.getAttribute("aria-expanded") === "true" };
  });
  const origin = new URL(document.documentElement.dataset.babelApi).origin;
  const targets = [
    { mode: "reply", parent: cardA.dataset.objectId, card: cardA, label: "Reply" },
    { mode: "reply", parent: cardB.dataset.objectId, card: cardB, label: "Reply" },
    { mode: "share", parent: cardA.dataset.objectId, card: cardA, label: "Share" },
    { mode: "publish", parent: null, card: null, label: "Post" },
  ].map((target) => {
    const key = `babel.draft.v1:${JSON.stringify([origin, identityId, target.mode, target.parent])}`;
    return { ...target, key, stored: localStorage.getItem(key), original: null };
  });
  let navigationCount = 0;
  const activate = (objectId) => {
    let active = required('[data-deck] .post-card[data-offset="0"]');
    if (active.dataset.objectId !== objectId) {
      close.click();
      const direction = objectId === cardB.dataset.objectId ? directionToB : -directionToB;
      const navigation = required(direction > 0 ? "[data-next]" : "[data-prev]");
      if (navigation.disabled) throw new Error("Draft target navigation is disabled");
      navigation.click();
      navigationCount += 1;
      active = required('[data-deck] .post-card[data-offset="0"]');
    }
    if (active.dataset.objectId !== objectId || active.inert || active.getAttribute("aria-hidden") === "true") {
      throw new Error(`Draft target ${objectId} did not become the interactive active card`);
    }
    return active;
  };
  const open = (target) => {
    if (target.mode === "publish") required("[data-toggle-composer]").click();
    else {
      const active = activate(target.parent);
      const social = required('.action-popover[data-kind="social"] > button', active);
      if (social.getAttribute("aria-expanded") !== "true") social.click();
      required(`[data-action="${target.mode}"]`, active).click();
    }
  };
  const type = (text) => {
    textarea.value = text;
    textarea.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const checks = [];
  const check = (name, actual, expected) => checks.push({ name, actual, expected });
  const compact = (id) => id.length <= 18 ? id : `${id.slice(0, 12)}...${id.slice(-6)}`;
  let restored = false;
  try {
    // Snapshot every target before adding markers, including memory-only Files.
    for (const target of targets) {
      open(target);
      target.original = { text: textarea.value, files: [...media.files] };
    }
    const marker = `Aegis draft ${crypto.randomUUID()}`;
    for (const [index, target] of targets.entries()) {
      open(target);
      check(`${target.mode} ${index}: another target's text must not leak`, textarea.value, target.original.text);
      check(`${target.mode} ${index}: visible composer controls`, {
        open: !panel.hidden,
        label: submit.textContent.trim(),
        enabled: !submit.disabled,
        mediaDisabled: media.disabled,
        targetStatus: target.parent ? required('[data-compose-context]').textContent.includes(compact(target.parent))
          : required('[data-compose-context]').textContent === "Public post",
      }, { open: true, label: target.label, enabled: true, mediaDisabled: false, targetStatus: true });
      target.marker = `${marker}: ${target.mode} ${index}`;
      type(target.marker);
      check(`${target.mode} ${index}: input event persists its own draft`,
        JSON.parse(localStorage.getItem(target.key) ?? "null")?.text ?? null, target.marker);
      close.click();
    }
    for (const target of targets) {
      open(target);
      check(`${target.mode} ${target.parent ?? "new"}: reopening restores unsent text`, textarea.value, target.marker);
      close.click();
    }
  } finally {
    // Restore through the controls first, so removing test storage cannot leave
    // test text in the production controller's in-memory draft cache.
    try {
      for (const target of targets) {
        if (!target.original) continue;
        open(target);
        type(target.original.text);
        const transfer = new DataTransfer();
        target.original.files.forEach((file) => transfer.items.add(file));
        media.files = transfer.files;
        media.dispatchEvent(new Event("change", { bubbles: true }));
      }
      close.click();
    } finally {
      for (const target of targets) {
        if (target.stored === null) localStorage.removeItem(target.key);
        else localStorage.setItem(target.key, target.stored);
      }
      activate(cardA.dataset.objectId);
      for (const { button, expanded } of popovers) {
        if ((button.getAttribute("aria-expanded") === "true") !== expanded) button.click();
      }
      status.textContent = previousStatus.text;
      if (previousStatus.state === undefined) delete status.dataset.state;
      else status.dataset.state = previousStatus.state;
      if (previousFocus instanceof HTMLElement && previousFocus.isConnected) previousFocus.focus({ preventScroll: true });
      scroll.forEach(([card, top]) => { card.scrollTop = top; });
      restored = targets.every((target) => localStorage.getItem(target.key) === target.stored);
    }
  }
  return { checks, restored, navigationCount,
    restoredActiveCard: required('[data-deck] .post-card[data-offset="0"]').dataset.objectId === cardA.dataset.objectId,
    targets: [cardA.dataset.objectId, cardB.dataset.objectId] };
}
