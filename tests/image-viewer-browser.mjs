import assert from "node:assert/strict";

export async function verifyImageViewer(execute, waitFor, expectedObjectId) {
  assert.match(expectedObjectId, /^obj_[a-f0-9]{64}$/, "published image Object ID required");
  const evaluate = async code => {
    const result = await execute([{ type: "eval", code }]);
    assert.equal(result.results[0].ok, true, JSON.stringify(result));
    return result.results[0].value;
  };
  // Retiring cards keep their old offset until the exit transition finishes.
  // Select the actual current publication without depending on that duration.
  await waitFor(`(() => {
    const card = document.querySelector('.post-card[data-offset="0"]:not([data-exiting])');
    const image = card?.querySelector('.post-image-open img');
    return { id: card?.dataset.objectId, active: !!card && !card.inert,
      loaded: !!image?.complete && image.naturalWidth > 0 };
  })()`, value => value.id === expectedObjectId && value.active && value.loaded);
  const opened = await evaluate(`(() => {
    const card = document.querySelector('.post-card[data-offset="0"]:not([data-exiting])');
    if (card?.dataset.objectId !== ${JSON.stringify(expectedObjectId)} || card.inert) {
      return { error: 'published image is not the active Object' };
    }
    const trigger = card.querySelector('.post-image-open');
    if (!trigger) return { error: 'active published image is not openable' };
    window.__imageReading = { id: card.dataset.objectId, top: card.scrollTop };
    trigger.focus(); trigger.click();
    return { open: !!document.querySelector('.image-viewer[open]') };
  })()`);
  assert.equal(opened.open, true, JSON.stringify(opened));
  await waitFor(`({ ready: !!document.querySelector('.image-viewer img')?.naturalWidth
    && !document.querySelector('.image-viewer img')?.hidden })`, value => value.ready);
  const viewed = await evaluate(`(() => {
    const dialog = document.querySelector('.image-viewer');
    const image = dialog.querySelector('img');
    const before = image.width;
    dialog.querySelector('[aria-label="Zoom in"]').click();
    const doubled = image.width;
    dialog.querySelector('[aria-label="Fit image"]').click();
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true }));
    const result = { before, doubled, reset: image.width,
      zoom: dialog.querySelector('output').value,
      unchanged: document.querySelector('.post-card[data-offset="0"]:not([data-exiting])').dataset.objectId === window.__imageReading.id,
      caption: dialog.querySelector('.image-viewer-caption').textContent,
      controls: [...dialog.querySelectorAll('button')].every(button => !!button.title && !!button.getAttribute('aria-label')) };
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    return result;
  })()`);
  assert.equal(viewed.doubled, viewed.before * 2);
  assert.equal(viewed.reset, viewed.before);
  assert.equal(viewed.zoom, "Fit");
  assert.equal(viewed.unchanged, true);
  assert.equal(viewed.controls, true);
  assert.equal(viewed.caption, "Surface live image Object");
  const restored = await waitFor(`(() => ({
    closed: !document.querySelector('.image-viewer'),
    focused: document.activeElement?.classList.contains('post-image-open'),
    objectId: document.querySelector('.post-card[data-offset="0"]:not([data-exiting])')?.dataset.objectId,
    reading: document.querySelector('.post-card[data-offset="0"]:not([data-exiting])').scrollTop,
    previous: window.__imageReading.top
  }))()`, value => value.closed);
  assert.equal(restored.focused, true);
  assert.equal(restored.objectId, expectedObjectId);
  assert.equal(restored.reading, restored.previous);
  await evaluate(`delete window.__imageReading; ({ cleaned: true })`);

  // Same-origin frames provide real narrow layout, not physical touch evidence.
  await evaluate(`(() => {
    window.__imageLayouts = null;
    (async () => {
      const results = [];
      for (const [width, height] of [[1280, 800], [390, 844], [320, 568]]) {
        const frame = document.createElement('iframe');
        frame.title = 'Temporary image viewer layout verification';
        frame.style.cssText = 'position:fixed;left:0;top:0;border:0;z-index:9999;width:' + width + 'px;height:' + height + 'px';
        frame.src = location.href;
        document.body.append(frame);
        try {
          await new Promise((resolve, reject) => {
            const timer = setTimeout(() => reject(new Error('viewer frame failed to load')), 12000);
            frame.onload = () => { clearTimeout(timer); resolve(); };
          });
          const composerDoc = frame.contentDocument;
          composerDoc.querySelector('[data-toggle-composer]').click();
          await new Promise(resolve => setTimeout(resolve, 300));
          const composer = composerDoc.querySelector('[data-composer-panel]');
          const composerRect = composer.getBoundingClientRect();
          const textarea = composer.querySelector('[data-compose-text]').getBoundingClientRect();
          const tools = [...composer.querySelectorAll('.composer-attachment-tool')].map(tool => tool.getBoundingClientRect());
          const composerLayout = {
            open: !composer.hidden,
            inside: composerRect.left >= 0 && composerRect.right <= width && composerRect.bottom <= height,
            overflow: composer.scrollWidth > composer.clientWidth,
            textPadding: textarea.left - composerRect.left,
            toolsInside: tools.every(tool => tool.left >= composerRect.left && tool.right <= composerRect.right),
          };
          composer.querySelector('[data-close-composer]').click();
          await frame.contentWindow.eval(
            '(async () => { const {openImageViewer} = await import("/src/app/image-viewer.ts");' +
            'const canvas = document.createElement("canvas"); canvas.width=1600; canvas.height=1000;' +
            'const context=canvas.getContext("2d"); context.fillStyle="#4b9b7c"; context.fillRect(0,0,1600,1000);' +
            'const trigger=document.createElement("button");document.body.append(trigger);' +
            'openImageViewer({src:canvas.toDataURL(),alt:"Full resolution layout test",caption:"long-caption-".repeat(90)},trigger); })()'
          );
          const doc = frame.contentDocument;
          const deadline = Date.now() + 10000;
          while (doc.querySelector('.image-viewer img').hidden) {
            if (Date.now() > deadline) throw new Error('viewer image did not load');
            await new Promise(resolve => setTimeout(resolve, 30));
          }
          await new Promise(resolve => setTimeout(resolve, 150));
          const dialog = doc.querySelector('.image-viewer');
          const stage = dialog.querySelector('.image-viewer-stage');
          const img = stage.querySelector('img');
          const rect = dialog.getBoundingClientRect();
          const caption = dialog.querySelector('.image-viewer-caption').getBoundingClientRect();
          const stageRect = stage.getBoundingClientRect();
          const buttons = [...dialog.querySelectorAll('.image-viewer-controls button')].map(button => button.getBoundingClientRect());
          const fit = img.width <= stage.clientWidth && img.height <= stage.clientHeight;
          dialog.querySelector('[aria-label="Zoom in"]').click();
          const scrollable = stage.scrollWidth > stage.clientWidth || stage.scrollHeight > stage.clientHeight;
          results.push({ width, fit, scrollable, composer: composerLayout, inViewport: rect.left >= 0 && rect.right <= width && rect.top >= 0 && rect.bottom <= height,
            controlsInside: buttons.every(button => button.left >= rect.left && button.right <= rect.right),
            separateCaption: caption.top >= stageRect.bottom,
            dialogOverflow: dialog.scrollWidth > dialog.clientWidth });
          dialog.close();
        } finally { frame.remove(); }
      }
      window.__imageLayouts = { results };
    })().catch(error => { window.__imageLayouts = { error: String(error) }; });
    return { started: true };
  })()`);
  const layouts = await waitFor("window.__imageLayouts", value => value != null);
  await evaluate("delete window.__imageLayouts; ({ cleaned: true })");
  assert.equal(layouts.error, undefined, JSON.stringify(layouts));
  for (const result of layouts.results) {
    const detail = JSON.stringify(result);
    assert.equal(result.fit, true, detail);
    assert.equal(result.scrollable, true, detail);
    assert.equal(result.inViewport, true, detail);
    assert.equal(result.controlsInside, true, detail);
    assert.equal(result.separateCaption, true, detail);
    assert.equal(result.dialogOverflow, false, detail);
    assert.equal(result.composer.open, true, detail);
    assert.equal(result.composer.inside, true, detail);
    assert.equal(result.composer.overflow, false, detail);
    assert.equal(result.composer.toolsInside, true, detail);
    assert.ok(result.composer.textPadding >= 16, detail);
  }
  console.log("Image viewer PASS: published image opens, zooms and restores deck/focus; responsive full-image layout verified");
}
