import assert from "node:assert/strict";
import { audioFixture, videoFixture } from "./rich-media-browser.mjs";

const image = Buffer.from([
  137,80,78,71,13,10,26,10,0,0,0,13,73,72,68,82,0,0,0,1,0,0,0,1,8,6,0,0,0,31,21,196,
  137,0,0,0,13,73,68,65,84,120,156,99,248,15,4,0,9,251,3,253,167,89,231,219,0,0,0,0,73,69,78,68,174,66,96,130,
]);
const active = '.post-card[data-offset="0"]';

export async function verifyMediaAlbums(execute, waitFor) {
  const evaluate = async code => {
    const result = await execute([{ type: "eval", code }]);
    assert.equal(result.results[0].ok, true, JSON.stringify(result));
    return result.results[0].value;
  };
  const fixtures = [
    { name: "album-image.png", type: "image/png", kind: "image", bytes: image },
    { name: "album-sound.wav", type: "audio/wav", kind: "audio", bytes: audioFixture() },
    { name: "album-motion.webm", type: "video/webm", kind: "video", bytes: videoFixture() },
  ];
  const attach = async items => evaluate(`(() => {
    const input = document.querySelector('[data-compose-media]'), transfer = new DataTransfer();
    for (const item of ${JSON.stringify(items.map(({name,type,bytes}) => ({name,type,base64:bytes.toString("base64")})))}) {
      transfer.items.add(new File([Uint8Array.from(atob(item.base64), c => c.charCodeAt(0))], item.name, {type:item.type}));
    }
    input.files = transfer.files; input.dispatchEvent(new Event('change',{bubbles:true}));
    return {count:input.files.length, multiple:input.multiple};
  })()`);
  const click = selector => evaluate(`document.querySelector(${JSON.stringify(selector)}).click(); ({clicked:true})`);
  const caption = `Surface album ${process.pid}`;
  await click('[data-toggle-composer]');
  assert.deepEqual(await attach(fixtures.slice(0, 2)), {count:2, multiple:true});
  assert.equal((await attach(fixtures.slice(2))).count, 3);
  await click('[data-compose-media-index="2"]');
  await click('[data-compose-media-earlier]');
  await click('[data-compose-media-index="2"]');
  await click('[data-compose-media-remove]');
  assert.equal((await attach([fixtures[1]])).count, 3);
  await evaluate(`(() => {
    const text=document.querySelector('[data-compose-text]'); text.value=${JSON.stringify(caption)};
    text.dispatchEvent(new Event('input',{bubbles:true})); return {edited:true};
  })()`);
  // Different File identity with identical bytes must not publish a duplicate-resource Object.
  assert.equal((await attach([{...fixtures[0],name:"duplicate.png"}])).count, 4);
  await evaluate("document.querySelector('[data-compose-form]').requestSubmit(); ({submitted:true})");
  const failure = await waitFor(`({state:document.querySelector('[data-author-status]').dataset.state,
    text:document.querySelector('[data-author-status]').textContent,
    count:document.querySelector('[data-compose-media]').files.length,
    closed:document.querySelector('[data-composer-panel]').hidden})`, value => value.state === "error");
  assert.match(failure.text, /duplicate/i); assert.equal(failure.count, 4); assert.equal(failure.closed, false);
  await click('[data-close-composer]');
  await click('[data-toggle-composer]');
  await click('[data-compose-media-index="3"]');
  await click('[data-compose-media-remove]');
  const order = await evaluate("({names:[...document.querySelector('[data-compose-media]').files].map(file=>file.name)})");
  const ordered = [fixtures[0], fixtures[2], fixtures[1]];
  assert.deepEqual(order.names, ordered.map(file => file.name));
  await evaluate("document.querySelector('[data-compose-form]').requestSubmit(); ({submitted:true})");
  const published = await waitFor(`(() => {
    const card=document.querySelector('${active}');
    return {closed:document.querySelector('[data-composer-panel]').hidden,
      text:card?.querySelector('.post-context .post-content')?.textContent,
      id:card?.dataset.objectId, album:!!card?.querySelector('.post-primary [data-media-gallery]')};
  })()`, value => value.closed && value.text === caption && value.album);
  const {api} = await evaluate("({api:document.documentElement.dataset.babelApi})");
  await verifyObject(api, published.id, ordered);
  const mainGallery = `${active} .post-primary [data-media-gallery]`;
  await verifyGallery(mainGallery, ordered, published.id, evaluate, waitFor);
  const swiped=await evaluate(`(() => {
    const gallery=document.querySelector(${JSON.stringify(mainGallery)});
    gallery.querySelector('[data-media-index="0"]').click();
    const image=gallery.querySelector('.post-image-open');
    image.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,isPrimary:true,pointerId:9901,button:0,clientX:220,clientY:200}));
    window.dispatchEvent(new PointerEvent('pointerup',{bubbles:true,isPrimary:true,pointerId:9901,button:0,clientX:80,clientY:200}));
    return {id:document.querySelector('${active}').dataset.objectId,viewer:!!document.querySelector('.image-viewer[open]')};
  })()`);
  assert.notEqual(swiped.id,published.id);assert.equal(swiped.viewer,false);
  await click('[data-prev]');
  await waitFor(`({id:document.querySelector('${active}').dataset.objectId})`,value=>value.id===published.id);
  await verifyLayouts(evaluate, waitFor, published.id, api);

  for (const mode of ["reply", "share"]) {
    const text = `Surface album ${mode} ${process.pid}`;
    const target = await evaluate(`(() => {
      const card=document.querySelector('${active}');
      const menu=card.querySelector('.action-popover[data-kind="social"] > button');
      if(menu.getAttribute('aria-expanded')!=='true') menu.click();
      card.querySelector('[data-action="${mode}"]').click(); return {id:card.dataset.objectId};
    })()`);
    await attach(fixtures.slice(0, 2));
    await evaluate(`(() => {const text=document.querySelector('[data-compose-text]'); text.value=${JSON.stringify(text)};
      text.dispatchEvent(new Event('input',{bubbles:true})); document.querySelector('[data-compose-form]').requestSubmit(); return {submitted:true};})()`);
    const result = await waitFor(`(() => {
      const card=document.querySelector('${active}');
      const candidate=${mode === "reply"
        ? `[...card.querySelectorAll('.reply-row')].find(row=>row.querySelector('.reply-content')?.textContent===${JSON.stringify(text)})`
        : `card.querySelector('.post-context .post-content')?.textContent===${JSON.stringify(text)}?card:null`};
      return {closed:document.querySelector('[data-composer-panel]').hidden,id:candidate?.dataset.objectId,
        gallery:!!candidate?.querySelector('[data-media-gallery]'),active:card.dataset.objectId,
        error:document.querySelector('[data-author-status]').dataset.state==='error'?document.querySelector('[data-author-status]').textContent:null};
    })()`, value => value.error || value.closed && value.gallery);
    assert.equal(result.error, null, JSON.stringify(result));
    if (mode === "reply") assert.equal(result.active, target.id);
    await verifyObject(api, result.id, fixtures.slice(0, 2), {mode,target:target.id});
    const selector = mode === "reply"
      ? `${active} .reply-row[data-object-id="${result.id}"] [data-media-gallery]`
      : mainGallery;
    await verifyGallery(selector, fixtures.slice(0, 2), result.active, evaluate, waitFor);
    console.log(`Album ${mode} PASS: all attachments render, exact order/bytes, signed Object and graph link`);
  }
  console.log("Media albums PASS: append/reorder/remove, duplicate recovery, publish/reply/share, scoped navigation, playback cleanup, responsive layout");
}

async function verifyObject(api, id, fixtures, relation) {
  const response = await fetch(`${api}/objects/${id}`);
  assert.equal(response.status, 200);
  const {object} = await response.json();
  assert.equal(object.kind, "babel.media");
  assert.deepEqual(object.payload.resources.map(resource=>resource.media_type), fixtures.map(file=>file.type));
  assert.deepEqual(object.payload.primary_resource, object.payload.resources[0]);
  assert.deepEqual(object.resources.map(resource=>resource.integrity), object.payload.resources.map(resource=>resource.integrity));
  for (const [index, resource] of object.resources.entries()) {
    const media = await fetch(`${api}/objects/${id}/media/${resource.integrity}`);
    assert.equal(media.status, 200);
    assert.deepEqual(Buffer.from(await media.arrayBuffer()), fixtures[index].bytes);
  }
  if (relation) {
    const response = await fetch(`${api}/graph/objects/${id}/outgoing`);
    assert.equal(response.status, 200);
    const {edges} = await response.json();
    const links = edges.filter(edge=>edge.relation === (relation.mode === "reply" ? "reply_to" : "quotes"));
    assert.equal(links.length, 1); assert.equal(links[0].source, id); assert.equal(links[0].target, relation.target);
  }
}

async function verifyGallery(selector, fixtures, activeId, evaluate, waitFor) {
  const query = `document.querySelector(${JSON.stringify(selector)})`;
  for (const [index, fixture] of fixtures.entries()) {
    await evaluate(`(() => {${query}.querySelector('[data-media-index="${index}"]').click(); return {selected:true};})()`);
    const state = await waitFor(`(() => {
      const gallery=${query}, media=gallery.querySelector('.media-gallery-stage ${fixture.kind === "image" ? "img" : fixture.kind}');
      return {id:document.querySelector('${active}').dataset.objectId, index:gallery.dataset.mediaSelected,
        ready:${fixture.kind === "image" ? "media?.complete && media.naturalWidth>0" : "media?.readyState>=1"},
        src:media?.currentSrc, paused:media?.paused??true,error:media?.error?.message};
    })()`, value=>value.ready||value.error);
    assert.equal(state.error, undefined, JSON.stringify(state)); assert.equal(state.ready,true);
    assert.equal(state.id,activeId); assert.equal(state.index,String(index)); assert.equal(state.paused,true);
    const response=await fetch(state.src); assert.equal(response.status,200);
    assert.deepEqual(Buffer.from(await response.arrayBuffer()),fixture.bytes);
    if(fixture.kind === "image") {
      await evaluate(`(() => {${query}.querySelector('.media-gallery-stage .post-image-open').click(); return {opened:true};})()`);
      const viewer=await waitFor("(() => {const dialog=document.querySelector('.image-viewer[open]'),img=dialog?.querySelector('img'); return {open:!!dialog,ready:img?.complete&&img.naturalWidth>0,src:img?.currentSrc};})()",value=>value.open&&value.ready);
      assert.equal(viewer.src,state.src);
      await evaluate("document.querySelector('.image-viewer [aria-label=\"Close image\"]').click(); ({closed:true})");
    }
    if(fixture.kind !== "image") {
      if(fixture.kind === "video") {
        await evaluate(`(() => {
          window.__albumPlay=null;const player=${query}.querySelector('video');player.muted=true;
          player.play().then(()=>{window.__albumPlay={playing:!player.paused};},error=>{window.__albumPlay={error:String(error)};});
          return {started:true};
        })()`);
        const playback=await waitFor("window.__albumPlay",value=>value!=null);
        assert.equal(playback.error,undefined,JSON.stringify(playback));assert.equal(playback.playing,true);
      }
      const cleanup=await evaluate(`(() => {
        const gallery=${query}, player=gallery.querySelector('${fixture.kind}');
        gallery.querySelector('[data-media-next]').click();
        return {paused:player.paused,released:!player.hasAttribute('src'),connected:player.isConnected,
          id:document.querySelector('${active}').dataset.objectId};
      })()`);
      assert.deepEqual(cleanup,{paused:true,released:true,connected:false,id:activeId});
      await evaluate("delete window.__albumPlay; ({cleaned:true})");
    }
  }
  const keyboard=await evaluate(`(() => {
    const gallery=${query}, button=gallery.querySelector('[data-media-index="0"]'); button.click();button.focus();
    button.dispatchEvent(new KeyboardEvent('keydown',{key:'End',bubbles:true,cancelable:true}));
    return {selected:gallery.dataset.mediaSelected,id:document.querySelector('${active}').dataset.objectId,
      focus:document.activeElement.dataset.mediaIndex};
  })()`);
  assert.deepEqual(keyboard,{selected:String(fixtures.length-1),id:activeId,focus:String(fixtures.length-1)});
}

async function verifyLayouts(evaluate, waitFor, id, api) {
  const setup = `(async()=>{
    const {BabelFrontendClient}=await import('/src/app/protocol.ts');
    const {createMediaGallery}=await import('/src/app/media-gallery.ts');
    const client=new BabelFrontendClient(${JSON.stringify(api)});
    const response=await fetch(${JSON.stringify(`${api}/objects/${id}`)});
    const {object}=await response.json(),resolved=await client.describeObject(object);
    const card={...resolved,title:'A long album title with an-unbroken-word-'.repeat(12)};
    const inner=document.querySelector('${active} .post-card-inner');inner.dataset.contentKind='image';
    inner.replaceChildren(createMediaGallery(card));
    const reply=document.createElement('article');reply.className='reply-row';
    reply.dataset.albumLayout='';reply.append(createMediaGallery(card,true));
    document.querySelector('${active} [data-conversation-root]').append(reply);
  })()`;
  await evaluate(`(() => {
    window.__albumLayouts=null;
    (async()=>{
      const results=[];
      for(const [width,height] of [[1280,800],[390,844],[320,568]]) {
        const frame=document.createElement('iframe');frame.title='Temporary album layout verification';
        frame.style.cssText='position:fixed;left:0;top:0;border:0;z-index:9999;width:'+width+'px;height:'+height+'px';
        frame.src=location.href;document.body.append(frame);
        try {
          await new Promise((resolve,reject)=>{const timeout=setTimeout(()=>reject(new Error('album frame timeout')),12000);
            frame.onload=()=>{clearTimeout(timeout);resolve();};});
          const doc=frame.contentDocument,win=frame.contentWindow;
          const deadline=Date.now()+12000;
          while(!doc.querySelector('${active}')) {if(Date.now()>deadline) throw new Error('album feed timeout');await new Promise(r=>setTimeout(r,30));}
          await win.eval(${JSON.stringify(setup)});
          for(const gallery of [doc.querySelector('${active} .post-primary [data-media-gallery]'),
            doc.querySelector('[data-album-layout] [data-media-gallery]')]) {
            for(const item of gallery.querySelectorAll('[data-media-index]')) {
              item.click();await new Promise(r=>setTimeout(r,100));
              const bounds=gallery.getBoundingClientRect(),stage=gallery.querySelector('.media-gallery-stage').getBoundingClientRect();
              const player=gallery.querySelector('.media-gallery-stage audio,.media-gallery-stage video')?.getBoundingClientRect();
              results.push({width,index:item.dataset.mediaIndex,compact:gallery.dataset.mediaGallery==='compact',
                overflow:gallery.scrollWidth>gallery.clientWidth+1,
                playerHeight:player?.height??null,stageHeight:stage.height,
                playerFits:!player||(player.height>=44&&player.left>=stage.left-1&&player.right<=stage.right+1&&player.top>=stage.top-1&&player.bottom<=stage.bottom+1),
                fits:bounds.left>=0&&bounds.right<=width+1,
                targets:[...gallery.querySelectorAll('[data-media-next],[data-media-previous],[data-media-index]')]
                  .every(button=>button.offsetWidth>=44&&button.offsetHeight>=44)});
            }
          }
          doc.querySelector('[data-toggle-composer]').click();
          const input=doc.querySelector('[data-compose-media]'),transfer=new win.DataTransfer();
          for(let index=0;index<12;index++) transfer.items.add(new win.File([
            Uint8Array.from(atob(${JSON.stringify(image.toString("base64"))}),c=>c.charCodeAt(0))],
            'Long-filename-for-responsive-album-layout-'+index+'-'.repeat(80)+'.png',{type:'image/png'}));
          input.files=transfer.files;input.dispatchEvent(new win.Event('change',{bubbles:true}));
          await new Promise(r=>setTimeout(r,250));
          const composer=doc.querySelector('[data-composer-panel]'),bounds=composer.getBoundingClientRect();
          const thumbnail=doc.querySelector('[data-compose-media-index="11"]');thumbnail.click();
          results.push({width,composer:true,overflow:composer.scrollWidth>composer.clientWidth+1,
            fits:bounds.left>=0&&bounds.right<=width+1,
            targets:[...composer.querySelectorAll('[data-compose-media-index],[data-compose-media-earlier],[data-compose-media-later],[data-compose-media-remove]')]
              .every(button=>button.offsetWidth>=44&&button.offsetHeight>=44)});
        } finally {frame.remove();}
      }
      window.__albumLayouts={results};
    })().catch(error=>{window.__albumLayouts={error:String(error)};});return {started:true};
  })()`);
  const outcome=await waitFor("window.__albumLayouts",value=>value!=null);
  assert.equal(outcome.error,undefined,JSON.stringify(outcome));assert.equal(outcome.results.length,21);
  for(const item of outcome.results) {
    assert.equal(item.overflow,false,JSON.stringify(item));assert.equal(item.fits,true,JSON.stringify(item));
    assert.equal(item.targets,true,JSON.stringify(item));if(!item.composer)assert.equal(item.playerFits,true,JSON.stringify(item));
  }
  await evaluate("delete window.__albumLayouts; ({cleaned:true})");
}
