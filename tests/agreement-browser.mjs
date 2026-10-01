import assert from "node:assert/strict";

export async function verifySourceAgreement(execute, waitFor, postRpc, apiUrl, authorId) {
  const selected = await waitFor(`({ id: document.querySelector('.post-card[data-offset="0"]')?.dataset.objectId,
    prior: [...document.querySelectorAll('[data-definition="babel.judgment.source_agreement.v1"]')].map(node => node.dataset.judgmentId) })`, value => Boolean(value.id));
  const sources = [];
  for (const [relation, text] of [
    ["supports", "According to the study, a measured increase was observed in the public dataset with reproducible methodology."],
    ["contradicts", "However, a replication study found no measured increase in the public dataset under the same methodology."],
  ]) {
    const source = await postRpc("babel.object.publish_text.v1", { author_id: authorId, text });
    await postRpc("babel.graph.edge.publish.v1", { author_id: authorId, source: source.object.id,
      target: selected.id, relation, origin: "HumanAssertion" });
    sources.push(source.object.id);
  }
  await execute([{ type: "eval", code: `(() => {
    const select = document.querySelector('[data-judgment-definition]');
    select.value = 'babel.judgment.source_agreement.v1';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    document.querySelector('[data-judgment-form]').requestSubmit();
    return { submitted: true };
  })()` }]);
  const evaluated = await waitFor(`(() => {
    const card = [...document.querySelectorAll('[data-definition="babel.judgment.source_agreement.v1"]')]
      .find(node => !${JSON.stringify(selected.prior)}.includes(node.dataset.judgmentId));
    return { ready: document.querySelector('[data-judgment-status]').dataset.state === 'ready',
      id: card?.dataset.judgmentId, text: card?.textContent,
      components: [...card?.querySelectorAll('dl .judgment-metric') ?? []].map(node => node.textContent),
      inputsOpen: card?.querySelector('.judgment-input').open };
  })()`, value => value.ready && Boolean(value.id));
  assert.equal(evaluated.inputsOpen, false);
  assert.equal(evaluated.components.length, 4);
  assert.match(evaluated.text, /Uncalibrated/);
  assert.match(evaluated.text, /not a truth assessment/);
  assert.match(evaluated.text, /Source prior/);

  const response = await fetch(`${apiUrl}/judgments/${evaluated.id}`);
  assert.equal(response.status, 200);
  const stored = await response.json();
  assert.equal(stored.input.object_id, selected.id);
  assert.equal(stored.input.judgment_id, evaluated.id);
  assert.equal(stored.judgment.output.validation_count, 2);
  assert.deepEqual([...stored.judgment.output.source_ids].sort(), sources.sort());
  assert.equal(stored.judgment.output.confidence_status, "uncalibrated");
  await execute([{ type: "eval", code: `(() => {
    document.querySelector('[data-judgment-id="${evaluated.id}"] .judgment-input summary').click();
    return { opened: true };
  })()` }]);
  const inspected = await waitFor(`(() => {
    const inputs = document.querySelector('[data-judgment-id="${evaluated.id}"] .judgment-input');
    return { open: inputs.open, busy: inputs.getAttribute('aria-busy'), text: inputs.querySelector('pre').textContent };
  })()`, value => value.open && value.busy === "false" && value.text.length > 0);
  assert.deepEqual(JSON.parse(inspected.text), stored.input.request, "visible inputs match the exact durable request");

  await execute([{ type: "eval", code: `document.querySelector('[data-judgment-form]').requestSubmit(); ({ submitted: true })` }]);
  await waitFor(`(() => ({ ready: document.querySelector('[data-judgment-status]').dataset.state === 'ready',
    count: document.querySelectorAll('[data-definition="babel.judgment.source_agreement.v1"]').length,
    retained: Boolean(document.querySelector('[data-judgment-id="${evaluated.id}"]')) }))()`,
  value => value.ready && value.count === selected.prior.length + 1 && value.retained);
  const repeated = await (await fetch(`${apiUrl}/judgments/${evaluated.id}`)).json();
  assert.deepEqual(repeated.judgment, stored.judgment, "repeat preserves evaluation identity and timestamp");
  console.log("Source agreement UI: signed evidence, Python result, exact input disclosure, and repeat identity PASS");
}
