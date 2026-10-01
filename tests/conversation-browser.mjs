import assert from "node:assert/strict";

// Real DOM regression using the production controller with controlled reply data.
// The surrounding live-stack suite separately verifies signed API publication.
export async function verifyConversationReading(execute, waitFor) {
  for (const columnWidth of [320, 360, 760]) {
    await verifyConversationWidth(execute, waitFor, columnWidth);
  }
}

async function verifyConversationWidth(execute, waitFor, columnWidth) {
  await execute([{ type: "eval", code: `
    (() => {
      window.__babbleReadingCheck = null;
      import('/src/app/conversations.ts').then(async ({ Conversations }) => {
        const host = document.createElement('div');
        host.style.cssText = 'position:fixed;inset:0;z-index:9999;background:white';
        const column = document.createElement('article');
        column.className = 'post-card';
        column.style.cssText = 'height:360px;width:${columnWidth}px;transform:none';
        column.dataset.offset = '0';
        const content = document.createElement('div');
        content.style.height = '400px';
        content.dataset.readingAnchor = 'content';
        const card = (id, createdAt) => ({
          id, author: 'reading-test-author', title: id, media: null, surfaces: [],
          content: ('Readable reply ' + id + '. ').repeat(12), createdAt
        });
        let replies = Array.from({ length: 6 }, (_, index) => card(
          'reading-' + index, '2026-09-29T12:00:00Z'
        ));
        const controller = new Conversations(async (id) => ({
          replies: id === 'reading-root' ? replies : [card('nested-reply', '2026-09-29T13:00:00Z')], nextCursor: null
        }));
        const panel = controller.view('reading-root');
        column.append(content, panel);
        host.append(column);
        document.body.append(host);
        const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
        try {
          controller.activate('reading-root');
          await frame(); await frame();
          const target = panel.querySelector('[data-object-id="reading-2"]');
          column.scrollTop = target.offsetTop + 24;
          const offset = () => panel.querySelector('[data-object-id="reading-2"]').getBoundingClientRect().top
            - column.getBoundingClientRect().top;
          const before = offset();
          replies = [card('earlier', '2026-09-29T11:00:00Z'), ...replies];
          await controller.refresh('reading-root');
          await frame();
          const after = offset();
          const rootWidth = target.getBoundingClientRect().width;
          [...target.querySelectorAll('button')].find((button) => button.textContent === 'View replies').click();
          await frame();
          const nested = panel.querySelector('.reply-row');
          const nestedRadius = parseFloat(getComputedStyle(nested).borderRadius);
          const nestedPadding = parseFloat(getComputedStyle(nested).paddingTop);
          const nestedWidth = nested.getBoundingClientRect().width;
          const nestedOverflow = column.scrollWidth > column.clientWidth;
          const parent = panel.querySelector('.thread-parent');
          const parentText = parent?.querySelector('.thread-parent-preview')?.textContent;
          const headingFocused = document.activeElement === panel.querySelector('h3');
          parent.open = true;
          await frame();
          const completeText = parent.querySelector('.reply-content').textContent;
          [...panel.querySelectorAll('.conversation-header button')].find((button) => button.textContent === 'Back').click();
          await frame();
          const returnedReply = panel.querySelector('[data-object-id="reading-2"]');
          window.__babbleReadingCheck = {
            before, after, returned: offset(), parentText, completeText, headingFocused,
            rootWidth, nestedWidth, nestedRadius, nestedPadding, nestedOverflow,
            count: panel.querySelectorAll('.reply-row').length,
            replyRadius: parseFloat(getComputedStyle(returnedReply).borderRadius),
            replyPadding: parseFloat(getComputedStyle(returnedReply).paddingTop),
            replyBackground: getComputedStyle(returnedReply).backgroundImage,
            replyNarrower: returnedReply.getBoundingClientRect().width < column.getBoundingClientRect().width,
            replyLayout: getComputedStyle(returnedReply).display,
            overflow: column.scrollWidth > column.clientWidth
          };
        } finally {
          host.remove();
        }
      }).catch((error) => { window.__babbleReadingCheck = { error: String(error) }; });
      return true;
    })()
  ` }]);
  const result = await waitFor("window.__babbleReadingCheck", (value) => value != null);
  await execute([{ type: "eval", code: "delete window.__babbleReadingCheck; true" }]);
  assert.equal(result.error, undefined, JSON.stringify(result));
  assert.equal(result.count, 7);
  assert.ok(Math.abs(result.before - result.after) <= 1, "insertion must preserve the visible reply offset");
  assert.ok(Math.abs(result.after - result.returned) <= 1, "nested Back must restore the same reply offset");
  assert.match(result.parentText, /Readable reply reading-2/);
  assert.equal(result.completeText, result.parentText);
  assert.equal(result.headingFocused, true);
  assert.equal(result.replyRadius, 18);
  assert.equal(result.nestedRadius, 14);
  assert.ok(result.nestedPadding < result.replyPadding, 'Nested replies have a more compact inset');
  assert.ok(result.nestedWidth < result.rootWidth, 'Nested replies are smaller than first-level replies');
  assert.equal(result.nestedOverflow, false);
  assert.match(result.replyBackground, /linear-gradient/);
  assert.equal(result.replyNarrower, true);
  assert.equal(result.replyLayout, columnWidth === 760 ? "grid" : "block");
  assert.equal(result.overflow, false);
}
