import assert from "node:assert/strict";
import { setTimeout as delay } from "node:timers/promises";

export async function verifySurfaceLease(execute, waitFor, probe) {
  const initial = await waitFor(`(() => {
    const panel = document.querySelector('[data-surface-panel]');
    const frame = panel.querySelector('iframe');
    const session = [...panel.querySelectorAll('[data-surface-meta] span')].find(node => node.textContent.startsWith('Session: '))?.textContent.slice(9);
    if (frame?.dataset.babelLifecycle !== 'active' || !session) return {};
    window.__leaseFrame = frame;
    return { session };
  })()`, value => !!value?.session);
  const before = probe.read(initial.session);
  assert.equal(before.retired, 0);
  assert.ok(before.lease_expires_at > Date.now());
  const deadline = Date.now() + 25_000;
  while (probe.read(initial.session).lease_expires_at <= before.lease_expires_at) {
    assert.ok(Date.now() < deadline, "Visible Surface did not renew its real server lease");
    await delay(100);
  }
  const renewed = await execute([{ type: "eval", code: `({
    sameFrame: document.querySelector('[data-surface-host] iframe') === window.__leaseFrame,
    active: window.__leaseFrame.dataset.babelLifecycle === 'active'
  })` }]);
  assert.deepEqual(renewed.results[0].value, { sameFrame: true, active: true });

  // Only the disposable live-stack store is changed, simulating server-side expiry.
  probe.expire(initial.session);
  const stopped = await waitFor(`(() => ({
    phase: document.querySelector('[data-surface-host]').dataset.state,
    noFrame: !document.querySelector('[data-surface-host] iframe'),
    retry: !document.querySelector('[data-retry-surface]').hidden,
    message: document.querySelector('[data-surface-host]').textContent
  }))()`, value => value?.phase === 'error' && value.noFrame);
  assert.equal(stopped.retry, true);
  assert.ok(stopped.message.length > 0);
  assert.equal(probe.read(initial.session).retired, 1);
  await execute([{ type: "eval", code: "document.querySelector('[data-retry-surface]').click(); ({ retry: true })" }]);
  const reopened = await waitFor(`(() => {
    const panel = document.querySelector('[data-surface-panel]');
    return { active: panel.querySelector('iframe')?.dataset.babelLifecycle === 'active',
      session: [...panel.querySelectorAll('[data-surface-meta] span')].find(node => node.textContent.startsWith('Session: '))?.textContent.slice(9) };
  })()`, value => value?.active && !!value.session);
  assert.notEqual(reopened.session, initial.session);
  await execute([{ type: "eval", code: "delete window.__leaseFrame; ({ cleared: true })" }]);
  console.log("Surface lease PASS", JSON.stringify({ renewedWithoutRemount: true, expiredExecutionStopped: true, explicitFreshRetry: true }));
}
