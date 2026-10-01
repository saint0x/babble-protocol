import assert from "node:assert/strict";

export async function verifyAccountSecurity({ execute, waitFor, api, identityId, password }) {
  const changedPassword = "Browser security changed passphrase 930!";
  const evaluate = async code => {
    const response = await execute([{ type: "eval", code }]);
    assert.equal(response.results[0].ok, true, JSON.stringify(response.results[0]));
    return response.results[0].value;
  };
  const request = async (method, path, token, body) => {
    const response = await fetch(api + path, {
      method, signal: AbortSignal.timeout(15_000), redirect: "error",
      headers: { ...(token ? { authorization: `Bearer ${token}` } : {}),
        ...(body ? { "content-type": "application/json" } : {}) },
      ...(body ? { body: JSON.stringify(body) } : {}),
    });
    return { status: response.status, body: response.status === 204 ? null : await response.json() };
  };
  const otherLogin = async () => {
    const login = await request("POST", "/auth/login", null, { identity_id: identityId, password });
    assert.equal(login.status, 200);
    const list = await request("GET", "/auth/sessions", login.body.token);
    assert.equal(list.status, 200);
    return { token: login.body.token, id: list.body.sessions.find(session => session.current).id };
  };
  const click = selector => evaluate(`(() => {
    const button = document.querySelector(${JSON.stringify(selector)});
    if (!button || button.disabled || !button.getClientRects().length) throw Error('Security control unavailable');
    button.click(); return { clicked: true };
  })()`);
  const visibleRow = id => waitFor(`({ visible: !!document.querySelector('[data-account-session-id="${id}"]') })`, value => value.visible);
  const removedRow = id => waitFor(`({ removed: !document.querySelector('[data-account-session-id="${id}"]') })`, value => value.removed);
  const revoke = async id => {
    await click(`[data-account-session-id="${id}"] [data-security-revoke]`);
    await waitFor(`({ visible: !!document.querySelector('[data-security-confirm]')?.getClientRects().length })`, value => value.visible);
  };
  const first = await otherLogin();
  await click("[data-toggle-profile]");
  await click('[data-menu-action="account"]');
  await waitFor(`({ open: document.querySelector('[data-account-dialog]').open })`, value => value.open);
  await visibleRow(first.id);
  await revoke(first.id);
  await click("[data-security-cancel]");
  assert.equal((await request("GET", "/auth/session", first.token)).status, 200, "cancel does not revoke");
  await revoke(first.id);
  await click("[data-security-confirm]");
  await removedRow(first.id);
  assert.equal((await request("GET", "/auth/session", first.token)).status, 401);

  const second = await otherLogin();
  await click("[data-security-refresh]");
  await visibleRow(second.id);
  await click("[data-security-revoke-others]");
  await click("[data-security-confirm]");
  await removedRow(second.id);
  assert.equal((await request("GET", "/auth/session", second.token)).status, 401);
  const witness = await otherLogin();
  const submitPassword = current => evaluate(`(() => {
    document.querySelector('[data-security-current-password]').value = ${JSON.stringify(current)};
    document.querySelector('[data-security-new-password]').value = ${JSON.stringify(changedPassword)};
    document.querySelector('[data-security-confirm-password]').value = ${JSON.stringify(changedPassword)};
    document.querySelector('[data-security-password-form]').requestSubmit();
    return { submitted: true };
  })()`);
  await submitPassword("Incorrect current browser passphrase");
  await waitFor(`({ error: document.querySelector('[data-security-password-status]')?.dataset.state === 'error',
    signedIn: !document.querySelector('[data-account-details]').hidden })`, value => value.error && value.signedIn);
  assert.equal((await request("GET", "/auth/session", witness.token)).status, 200);
  await submitPassword(password);
  const changed = await waitFor(`(() => {
    const dialog = document.querySelector('[data-account-dialog]');
    return { signedOut: !dialog.querySelector('[data-account-form]').hidden,
      passwordsCleared: [...dialog.querySelectorAll('input[type="password"]')].every(input => input.value === ''),
      tokenRemoved: sessionStorage.getItem('babble.session.v1:' + new URL(document.documentElement.dataset.babbleApi).origin) === null };
  })()`, value => value.signedOut && value.tokenRemoved);
  assert.equal(changed.passwordsCleared, true);
  assert.equal((await request("GET", "/auth/session", witness.token)).status, 401);
  assert.equal((await request("POST", "/auth/login", null, { identity_id: identityId, password })).status, 401);

  await evaluate(`(() => {
    document.querySelector('[data-account-mode="login"]').click();
    document.querySelector('[data-account-login]').value = ${JSON.stringify(identityId)};
    document.querySelector('[data-account-password]').value = ${JSON.stringify(changedPassword)};
    document.querySelector('[data-account-form]').requestSubmit();
    return { submitted: true };
  })()`);
  await waitFor(`({ signedIn: document.querySelector('[data-account-status]').textContent === 'Signed in',
    rows: document.querySelectorAll('[data-account-session-id]').length })`, value => value.signedIn && value.rows === 1);

  await evaluate(`(() => {
    window.__securityLayout = null;
    (async () => {
      const results = [];
      for (const width of [390, 320]) {
        const frame = document.createElement('iframe');
        frame.title = 'Account layout verification';
        frame.style.cssText = 'position:fixed;inset:0;width:' + width + 'px;height:844px;border:0;z-index:9999';
        frame.src = location.href; document.body.append(frame);
        try {
          const deadline = Date.now() + 15000;
          const wait = async (stage, predicate) => {
            while (!predicate()) {
              if (Date.now() > deadline) {
                const doc = frame.contentDocument;
                throw Error('Account frame not ready: ' + JSON.stringify({ width, stage,
                  readyState: doc?.readyState, menuHidden: doc?.querySelector('[data-menu-action="account"]')?.hidden,
                  status: doc?.querySelector('[data-account-status]')?.textContent,
                  securityStatus: doc?.querySelector('[data-security-status]')?.textContent,
                  dialogOpen: doc?.querySelector('[data-account-dialog]')?.open,
                  rows: doc?.querySelectorAll('[data-account-session-id]').length,
                  overlay: !!doc?.querySelector('astro-error-overlay, vite-error-overlay') }));
              }
              await new Promise(resolve => setTimeout(resolve, 100));
            }
          };
          await wait('restore account', () => frame.contentDocument?.querySelector('[data-menu-action="account"]')?.hidden === false);
          const doc = frame.contentDocument;
          doc.querySelector('[data-toggle-profile]').click();
          doc.querySelector('[data-menu-action="account"]').click();
          await wait('open sessions', () => doc.querySelector('[data-account-dialog]')?.open && doc.querySelector('[data-account-session-id]'));
          const dialog = doc.querySelector('[data-account-dialog]');
          const bounds = dialog.getBoundingClientRect();
          const fields = [...dialog.querySelectorAll('input,button')].filter(node => node.getClientRects().length);
          results.push({ width, fits: bounds.left >= 0 && bounds.right <= width && bounds.height <= 844,
            overflow: doc.documentElement.scrollWidth > width || dialog.scrollWidth > dialog.clientWidth,
            contained: fields.every(node => { const r = node.getBoundingClientRect(); return r.left >= bounds.left && r.right <= bounds.right; }) });
        } finally { frame.remove(); }
      }
      window.__securityLayout = { results };
    })().catch(error => { window.__securityLayout = { error: String(error) }; });
    return { started: true };
  })()`);
  const layout = await waitFor("window.__securityLayout", value => value != null);
  assert.equal(layout.error, undefined, JSON.stringify(layout));
  for (const row of layout.results) {
    assert.equal(row.fits, true, JSON.stringify(row));
    assert.equal(row.overflow, false, JSON.stringify(row));
    assert.equal(row.contained, true, JSON.stringify(row));
  }
  const current = await evaluate(`(() => {
    const row = document.querySelector('[data-account-session-id]');
    delete window.__securityLayout;
    return { id: row.dataset.accountSessionId };
  })()`);
  await revoke(current.id);
  await click("[data-security-confirm]");
  await waitFor(`({ signedOut: !document.querySelector('[data-account-form]').hidden,
    tokenRemoved: sessionStorage.getItem('babble.session.v1:' + new URL(document.documentElement.dataset.babbleApi).origin) === null })`, value => value.signedOut && value.tokenRemoved);
  console.log("Account security UI PASS", JSON.stringify({ individualRevoke: true, cancelPreservesSession: true,
    revokeOthers: true, currentPasswordRequired: true, passwordChangeSignsOutAll: true,
    newPasswordLogin: true, currentRevoke: true, layouts: layout.results }));
}
