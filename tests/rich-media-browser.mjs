import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { verifyQuoteVisit } from "./quotes-browser.mjs";

export function audioFixture() {
  const samples = 32000, bytes = Buffer.alloc(44 + samples * 2);
  bytes.write("RIFF"); bytes.writeUInt32LE(bytes.length - 8, 4); bytes.write("WAVEfmt ", 8);
  bytes.writeUInt32LE(16, 16); bytes.writeUInt16LE(1, 20); bytes.writeUInt16LE(1, 22);
  bytes.writeUInt32LE(8000, 24); bytes.writeUInt32LE(16000, 28); bytes.writeUInt16LE(2, 32);
  bytes.writeUInt16LE(16, 34); bytes.write("data", 36); bytes.writeUInt32LE(samples * 2, 40);
  for (let i = 0; i < samples; i++) bytes.writeInt16LE(Math.round(Math.sin(i * 2 * Math.PI * 220 / 8000) * 1000), 44 + i * 2);
  return bytes;
}

export function videoFixture() {
  const directory = mkdtempSync(join(tmpdir(), "babble-media-fixture-"));
  try {
    const file = join(directory, "motion.webm");
    execFileSync("ffmpeg", ["-v", "error", "-f", "lavfi", "-i", "testsrc2=size=320x180:rate=12", "-t", "4", "-an",
      "-c:v", "libvpx", "-deadline", "realtime", "-pix_fmt", "yuv420p", file]);
    return readFileSync(file);
  } finally { rmSync(directory, { recursive: true, force: true }); }
}

export async function verifyRichMedia(execute, waitFor) {
  const evaluate = async code => {
    const result = await execute([{ type: "eval", code }]);
    assert.equal(result.results[0].ok, true, JSON.stringify(result));
    return result.results[0].value;
  };
  const fixtures = [
    { kind: "audio", type: "audio/wav", name: "sound.wav", bytes: audioFixture() },
    { kind: "video", type: "video/webm", name: "motion.webm", bytes: videoFixture() },
  ];
  for (const fixture of fixtures) {
    const { kind, type, name, bytes } = fixture;
    const caption = `Surface live ${kind} Object`;
    await evaluate(`(() => {
      document.querySelector('[data-toggle-composer]').click();
      const file = new File([Uint8Array.from(atob(${JSON.stringify(bytes.toString("base64"))}), c => c.charCodeAt(0))], ${JSON.stringify(name)}, { type: ${JSON.stringify(type)} });
      const transfer = new DataTransfer(); transfer.items.add(file);
      const input = document.querySelector('[data-compose-media]'); input.files = transfer.files;
      input.dispatchEvent(new Event('change', { bubbles: true }));
      const text = document.querySelector('[data-compose-text]'); text.value = ${JSON.stringify(caption)};
      text.dispatchEvent(new Event('input', { bubbles: true }));
      return { attached: true };
    })()`);
    const preview = await waitFor(`(() => {
      const p = document.querySelector('[data-compose-preview-${kind}]');
      return { ready: p.readyState >= 1, error: p.error?.message, duration: p.duration,
        controls: p.controls, autoplay: p.autoplay, hidden: p.hidden };
    })()`, value => value.ready || value.error);
    assert.equal(preview.error, undefined, JSON.stringify(preview));
    assert.equal(preview.ready, true);
    assert.ok(Number.isFinite(preview.duration) && preview.duration >= 3.9);
    assert.equal(preview.controls, true);
    assert.equal(preview.autoplay, false);
    assert.equal(preview.hidden, false);
    await evaluate(`(() => {
      window.__richMediaPlay = null;
      const p = document.querySelector('[data-compose-preview-${kind}]'); p.muted = true;
      p.play().then(() => { window.__richMediaPlay = { played: !p.paused }; }, e => { window.__richMediaPlay = { error: String(e), name: e.name }; });
      return { started: true };
    })()`);
    const played = await waitFor("window.__richMediaPlay", value => value != null);
    if (kind === "audio" && played.name === "NotAllowedError") {
      console.log("Audio preview activation limited: Aegis scripted events are not trusted user gestures");
    } else {
      assert.equal(played.error, undefined, JSON.stringify(played));
      assert.equal(played.played, true);
    }
    const closed = await evaluate(`(() => {
      document.querySelector('[data-close-composer]').click();
      const p = document.querySelector('[data-compose-preview-${kind}]');
      const result = { paused: p.paused, retained: !!document.querySelector('[data-compose-media]').files[0] };
      document.querySelector('[data-toggle-composer]').click();
      document.querySelector('[data-compose-form]').requestSubmit();
      return result;
    })()`);
    assert.equal(closed.paused, true); assert.equal(closed.retained, true);
    const published = await waitFor(`(() => {
      const card = document.querySelector('.post-card[data-offset="0"]');
      const p = card?.querySelector('.media-player ${kind}');
      return { ready: !!p && p.readyState >= 1, error: p?.error?.message,
        caption: card?.querySelector('.post-context .post-content')?.textContent,
        source: p?.currentSrc, paused: p?.paused, controls: p?.controls, autoplay: p?.autoplay,
        id: card?.dataset.objectId, closed: document.querySelector('[data-composer-panel]').hidden };
    })()`, value => value.closed && value.caption === caption && (value.ready || value.error));
    assert.equal(published.error, undefined, JSON.stringify(published));
    assert.equal(published.ready, true); assert.equal(published.controls, true);
    assert.equal(published.paused, true); assert.equal(published.autoplay, false);
    assert.match(published.source, /\/objects\/obj_[a-f0-9]+\/media\/[a-f0-9]+$/);
    const partial = await fetch(published.source, { headers: { range: "bytes=0-31" } });
    assert.equal(partial.status, 206);
    assert.equal(partial.headers.get("content-range"), `bytes 0-31/${bytes.length}`);
    assert.deepEqual(Buffer.from(await partial.arrayBuffer()), bytes.subarray(0, 32));
    await evaluate(`(() => {
      window.__richMediaPlay = null;
      const p = document.querySelector('.post-card[data-offset="0"] .media-player ${kind}');
      p.muted = true; p.currentTime = 1;
      p.play().then(() => { window.__richMediaPlay = { played: !p.paused }; }, e => { window.__richMediaPlay = { error: String(e), name: e.name }; });
      return { started: true };
    })()`);
    const playback = await waitFor("window.__richMediaPlay", value => value != null);
    const activationLimited = kind === "audio" && playback.name === "NotAllowedError";
    if (!activationLimited) {
      assert.equal(playback.error, undefined, JSON.stringify(playback)); assert.equal(playback.played, true);
    }
    const advanced = await waitFor(`(() => { const p = document.querySelector('.post-card[data-offset="0"] .media-player ${kind}'); return { time: p.currentTime, ready: p.readyState >= 2, seeking: p.seeking, width: p.videoWidth ?? null }; })()`, value => value.ready && !value.seeking && value.time >= (activationLimited ? 1 : 1.1));
    if (kind === "video") assert.equal(advanced.width, 320);
    const switched = await evaluate(`(() => {
      const card = document.querySelector('.post-card[data-offset="0"]');
      const p = card.querySelector('.media-player ${kind}');
      document.querySelector('[data-next]').click();
      return { paused: p.paused, inactive: card.inert, moved: card.dataset.offset !== '0' };
    })()`);
    assert.deepEqual(switched, { paused: true, inactive: true, moved: true });
    await evaluate("document.querySelector('[data-prev]').click(); ({ returned: true })");
    await waitFor(`({ current: document.querySelector('.post-card[data-offset="0"]')?.dataset.objectId })`, value => value.current === published.id);
    const returned = await evaluate(`({ paused: document.querySelector('.post-card[data-offset="0"] .media-player ${kind}').paused })`);
    assert.equal(returned.paused, true);
    console.log(`Rich ${kind} media PASS: publication, decode, byte range, seek, controls; trusted audio activation limited=${activationLimited}`);
  }
  await verifyMediaLayouts(evaluate, waitFor);
  await verifyMediaConversations(evaluate, waitFor, fixtures);
  await evaluate("delete window.__richMediaPlay; ({ cleaned: true })");
}

async function verifyMediaConversations(evaluate, waitFor, playbackFixtures) {
  const fixtures = [{ kind: "image", type: "image/png", name: "image.png", bytes: Buffer.from([
    137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82,0,0,0,1,0,0,0,1,8,6,0,0,0,31,21,196,
    137,0,0,0,13,73,68,65,84,120,156,99,248,15,4,0,9,251,3,253,167,89,231,219,0,0,0,0,73,69,78,68,174,66,96,130,
  ]) }, ...playbackFixtures];
  const { api } = await evaluate("({api:document.documentElement.dataset.babbleApi})");
  for (const fixture of fixtures) {
    for (const mode of ["reply", "share"]) {
      const caption = `Surface live ${mode} ${fixture.kind} Object`;
      const { target, enabled, bundlesHidden } = await evaluate(`(() => {
        const card = document.querySelector('.post-card[data-offset="0"]');
        const menu = card.querySelector('.action-popover[data-kind="social"] > button');
        if (menu.getAttribute('aria-expanded') !== 'true') menu.click();
        card.querySelector('[data-action="${mode}"]').click();
        const input = document.querySelector('[data-compose-media]');
        const file = new File([Uint8Array.from(atob(${JSON.stringify(fixture.bytes.toString("base64"))}), c => c.charCodeAt(0))],
          ${JSON.stringify(fixture.name)}, { type: ${JSON.stringify(fixture.type)} });
        const transfer = new DataTransfer(); transfer.items.add(file); input.files = transfer.files;
        input.dispatchEvent(new Event('change', {bubbles:true}));
        const text = document.querySelector('[data-compose-text]'); text.value = ${JSON.stringify(caption)};
        text.dispatchEvent(new Event('input', {bubbles:true}));
        return {target:card.dataset.objectId, enabled:!input.disabled,
          bundlesHidden:document.querySelector('[data-bundle-picker]').hidden};
      })()`);
      assert.equal(enabled, true); assert.equal(bundlesHidden, true);
      await evaluate("document.querySelector('[data-compose-form]').requestSubmit(); ({submitted:true})");
      const publication = await waitFor(`(() => {
        const status = document.querySelector('[data-author-status]');
        const error = status.dataset.state === 'error' ? status.textContent : null;
        const closed = document.querySelector('[data-composer-panel]').hidden;
        const card = document.querySelector('.post-card[data-offset="0"]');
        const candidate = ${mode === "reply"
          ? `[...card.querySelectorAll('.reply-row')].find(row=>row.querySelector('.reply-content')?.textContent===${JSON.stringify(caption)})`
          : `card.querySelector('.post-context .post-content')?.textContent===${JSON.stringify(caption)} ? card : null`};
        const media = candidate?.querySelector('${fixture.kind === "image" ? "img.reply-media, .post-image img" : fixture.kind}');
        const ready = ${fixture.kind === "image" ? "media?.complete && media?.naturalWidth>0" : "media?.readyState>=1"};
        return {error, closed, ready, id:candidate?.dataset.objectId, src:media?.currentSrc,
          active:card.dataset.objectId, paused:media?.paused ?? true};
      })()`, value => value.error || value.closed && value.ready);
      assert.equal(publication.error, null, JSON.stringify(publication));
      assert.equal(publication.closed, true); assert.equal(publication.ready, true);
      assert.equal(publication.paused, true);
      if (mode === "reply") assert.equal(publication.active, target, "reply must retain the parent card");
      const objectResponse = await fetch(`${api}/objects/${publication.id}`);
      assert.equal(objectResponse.status, 200);
      const { object } = await objectResponse.json();
      assert.equal(object.kind, "babble.media");
      assert.equal(object.payload.description, caption);
      assert.equal(object.payload.primary_resource.media_type, fixture.type);
      const graphResponse = await fetch(`${api}/graph/objects/${publication.id}/outgoing`);
      assert.equal(graphResponse.status, 200);
      const { edges } = await graphResponse.json();
      const links = edges.filter(edge => edge.relation === (mode === "reply" ? "reply_to" : "quotes"));
      assert.equal(links.length, 1);
      assert.equal(links[0].source, object.id); assert.equal(links[0].target, target);
      const delivered = await fetch(publication.src);
      assert.equal(delivered.status, 200);
      assert.deepEqual(Buffer.from(await delivered.arrayBuffer()), fixture.bytes);
      if (mode === "share") await verifyQuoteVisit(evaluate, waitFor, object.id, target);
      console.log(`Media ${mode} ${fixture.kind} PASS: composer, rendered content, persisted Object, graph link, exact bytes`);
    }
  }
}

async function verifyMediaLayouts(evaluate, waitFor) {
  const setup = `(async () => {
    const { createMediaPlayer } = await import('/src/app/media-player.ts');
    const article = document.querySelector('.post-card[data-offset="0"]');
    const kind = window.__layoutKind;
    article.dataset.contentKind = kind;
    const inner = article.querySelector('.post-card-inner');
    inner.dataset.contentKind = kind;
    inner.replaceChildren(createMediaPlayer({ media: window.__layoutSources[kind], mediaKind: kind,
      mediaType: kind === 'audio' ? 'audio/wav' : 'video/webm', title: 'A recording with a-long-unbroken-word-'.repeat(8) }));
  })()`;
  const audio = `data:audio/wav;base64,${audioFixture().toString("base64")}`;
  await evaluate(`(() => {
    window.__richMediaLayouts = null;
    const video = document.querySelector('.post-card[data-offset="0"] video').currentSrc;
    (async () => {
      const results = [];
      for (const [width, height] of [[1280, 800], [390, 844], [320, 568]]) {
        const frame = document.createElement('iframe');
        frame.title = 'Temporary rich-media layout verification';
        frame.style.cssText = 'position:fixed;left:0;top:0;border:0;z-index:9999;width:' + width + 'px;height:' + height + 'px';
        frame.src = location.href; document.body.append(frame);
        try {
          await new Promise((resolve, reject) => { const timer = setTimeout(() => reject(new Error('media frame timed out')), 12000);
            frame.onload = () => { clearTimeout(timer); resolve(); }; });
          const doc = frame.contentDocument;
          const loaded=Date.now()+10000;
          while(!doc.querySelector('.post-card[data-offset="0"]')) { if(Date.now()>loaded) throw new Error('media feed not ready'); await new Promise(r=>setTimeout(r,30)); }
          frame.contentWindow.__layoutSources = { audio: ${JSON.stringify(audio)}, video };
          for (const kind of ['audio', 'video']) {
            frame.contentWindow.__layoutKind = kind;
            await frame.contentWindow.eval(${JSON.stringify(setup)});
            const inner=doc.querySelector('.post-card[data-offset="0"] .post-card-inner');
            const player=inner.querySelector(kind), deadline=Date.now()+10000;
            while(player.readyState<1) { if(player.error || Date.now()>deadline) throw new Error(kind+' layout media failed'); await new Promise(r=>setTimeout(r,30)); }
            await new Promise(r=>setTimeout(r,350));
            const a=inner.getBoundingClientRect(), b=player.getBoundingClientRect();
            const primary = inner.closest('.post-primary');
            const readyHeight = primary.offsetHeight;
            player.dispatchEvent(new frame.contentWindow.Event('waiting'));
            const loadingHeight = primary.offsetHeight;
            player.dispatchEvent(new frame.contentWindow.Event('canplay'));
            results.push({width, kind, inside:b.left>=a.left && b.right<=a.right+.5, overflow:inner.scrollWidth>inner.clientWidth, height:b.height,
              stableLoading:readyHeight===loadingHeight && readyHeight===primary.offsetHeight,
              vertical: kind === 'audio' || b.top>=a.top && b.bottom<=a.bottom+.5 });
          }
        } finally { frame.remove(); }
      }
      window.__richMediaLayouts={results};
    })().catch(error=>{window.__richMediaLayouts={error:String(error)};});
    return {started:true};
  })()`);
  const layouts = await waitFor("window.__richMediaLayouts", value => value != null);
  assert.equal(layouts.error, undefined, JSON.stringify(layouts));
  for (const item of layouts.results) {
    assert.equal(item.inside, true, JSON.stringify(item));
    assert.equal(item.overflow, false, JSON.stringify(item));
    assert.equal(item.vertical, true, JSON.stringify(item));
    assert.equal(item.stableLoading, true, JSON.stringify(item));
    assert.ok(item.height >= 44, JSON.stringify(item));
  }
  await evaluate("delete window.__richMediaLayouts; ({ cleaned:true })");
}
