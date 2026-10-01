import assert from "node:assert/strict";

export async function verifySearchRecovery(execute, waitFor) {
  const selectLens = async lens => {
    await execute([{ type: "eval", code: `(() => {
      const buttons = [...document.querySelectorAll('[data-lens="${lens}"]')];
      let button = buttons.find(node => node.getClientRects().length > 0);
      if (!button) {
        document.querySelector('[data-toggle-profile]').click();
        document.querySelector('[data-menu-action="settings"]').click();
        button = buttons.find(node => node.getClientRects().length > 0);
      }
      if (!button) throw new Error('Lens control is not visible');
      button.click();
      const settings = document.querySelector('[data-settings-panel]');
      if (!settings.hidden) document.querySelector('[data-close-settings]').click();
      return { selected: true };
    })()` }]);
  };
  const submit = async query => execute([{ type: "eval", code: `(() => {
    const input = document.querySelector('[data-search-input]');
    input.value = ${JSON.stringify(query)};
    input.form.requestSubmit();
    return { submitted: true };
  })()` }]);
  const state = `(() => ({
    empty: !document.querySelector('[data-empty]').hidden,
    cards: document.querySelectorAll('.post-card:not([data-exiting])').length,
    source: document.querySelector('[data-source-label]').textContent,
    lens: document.querySelector('[data-lens-label]').textContent,
    local: document.querySelector('[data-local-label]').textContent,
    status: document.querySelector('[data-status-text]').textContent
  }))()`;
  await selectLens("balanced");
  await submit(`zyxqvabsentbrowser${Date.now()}`);
  const empty = await waitFor(state, value => value.empty && value.cards === 0 && value.status === "Online");
  assert.equal(empty.source, "discovery");
  assert.match(empty.local, /^Local/);
  await selectLens("weird");
  const relensed = await waitFor(state, value => value.lens === "Weird" && value.status === "Online");
  assert.equal(relensed.empty, true);
  assert.equal(relensed.cards, 0);
  assert.equal(relensed.source, "discovery");
  await submit("");
  await waitFor(state, value => !value.empty && value.cards > 0 && value.status === "Online");
  await selectLens("balanced");
  await waitFor(state, value => value.lens === "Balanced" && !value.empty && value.cards > 0 && value.status === "Online");
  console.log("Explicit search empty state, Lens isolation and clear-search recovery PASS");
}
