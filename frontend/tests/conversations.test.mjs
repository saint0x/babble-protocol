import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const code = ts.transpileModule(readFileSync(new URL("../src/app/conversations.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;
const galleryCode = ts.transpileModule(readFileSync(new URL("../src/app/media-gallery.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;
const reading = { exports: {} };
vm.runInNewContext(ts.transpileModule(readFileSync(new URL("../src/app/reading.ts", import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText, reading);

// A small DOM double exercises the entire production controller, including node
// identity, connection, insertion order and bubbling, without browser automation.
class Element {
  constructor(tag) {
    Object.assign(this, { tag, children: [], parentElement: null, dataset: {}, attributes: new Map(),
      listeners: new Map(), hidden: false, disabled: false, className: "", text: "", scrollTop: 0 });
    this.offsetTop = 0;
    this.offsetHeight = 0;
    this.clientHeight = 600;
  }
  get isConnected() { return this.tag === "document" || (this.parentElement?.isConnected ?? false); }
  get offsetParent() { return this.parentElement; }
  querySelectorAll(selector) {
    if (selector === "audio, video") return all(this, child => child.tag === "audio" || child.tag === "video");
    if (selector === "img") return all(this, child => child.tag === "img");
    assert.equal(selector, "[data-reading-anchor]");
    return all(this, (child) => child.dataset.readingAnchor !== undefined);
  }
  focus(options) { this.focused = true; this.focusOptions = options; }
  get firstElementChild() { return this.children[0] ?? null; }
  get nextElementSibling() {
    const siblings = this.parentElement?.children ?? [];
    return siblings[siblings.indexOf(this) + 1] ?? null;
  }
  set textContent(value) { this.replaceChildren(); this.text = value; }
  get textContent() { return this.text + this.children.map((child) => child.textContent).join(""); }
  set innerHTML(_value) { throw new Error("Unsafe HTML insertion"); }
  setAttribute(name, value) { this.attributes.set(name, value); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  hasAttribute(name) { return this.attributes.has(name); }
  removeAttribute(name) { this.attributes.delete(name); }
  closest(selector) {
    for (let element = this; element; element = element.parentElement) {
      if (selector === `.${element.className}`) return element;
    }
    return null;
  }
  append(...children) { for (const child of children) this.insertBefore(child, null); }
  remove() {
    if (this.parentElement) this.parentElement.children.splice(this.parentElement.children.indexOf(this), 1);
    this.parentElement = null;
  }
  insertBefore(child, reference) {
    assert.ok(reference === null || reference.parentElement === this, "reference must be attached");
    if (child === reference) return child;
    child.remove();
    this.children.splice(reference === null ? this.children.length : this.children.indexOf(reference), 0, child);
    child.parentElement = this;
    return child;
  }
  replaceChildren(...children) { for (const child of [...this.children]) child.remove(); this.text = ""; this.append(...children); }
  replaceWith(child) { const parent = this.parentElement; if (parent) { parent.insertBefore(child, this); this.remove(); } }
  addEventListener(type, listener) {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }
  click() {
    if (this.disabled) return;
    const event = { type: "click", target: this, stopped: false, stopPropagation() { this.stopped = true; } };
    for (let node = this; node; node = node.parentElement) {
      for (const listener of node.listeners.get("click") ?? []) listener(event);
      if (event.stopped) break;
    }
  }
}

function all(element, predicate) {
  return element.children.flatMap((child) => [...(predicate(child) ? [child] : []), ...all(child, predicate)]);
}
const byClass = (element, name) => all(element, (child) => child.className === name);
const rows = (element) => byClass(element, "reply-row");
const ids = (element) => rows(element).map((row) => row.dataset.objectId);
const status = (element) => byClass(element, "conversation-status")[0];
const button = (element, text) => {
  const found = all(element, (child) => child.tag === "button" && child.textContent === text && !child.hidden)[0];
  assert.ok(found, `Expected visible ${text} button`);
  return found;
};
const settle = () => new Promise((resolve) => setImmediate(resolve));
function card(id, overrides = {}) {
  return {
    id, author: "did:babel:1234567890abcdefghijklmnopqrstuvwxyz", title: `Title ${id}`,
    content: `Reply ${id}`, createdAt: "2026-09-29T12:00:00Z", media: null, mediaItems: [], surfaces: [], ...overrides,
  };
}
function harness() {
  const document = new Element("document");
  document.createElement = (tag) => new Element(tag);
  const context = { document, exports: {}, requestAnimationFrame: () => {}, require: (name) => {
    if (name === "./media-gallery") return gallery.exports;
    if (name === "lucide") return { createElement: () => new Element("svg") };
    if (name === "./image-viewer") return { openImageViewer: () => {} };
    if (name === "./media-player") return { createMediaPlayer: card => {
      const player = new Element(card.mediaKind);
      player.src = card.media;
      player.pause = () => { player.paused = true; };
      player.load = () => {};
      return player;
    } };
    assert.equal(name, "./reading");
    return reading.exports;
  } };
  const gallery = { ...context, exports: {} };
  vm.runInNewContext(galleryCode, gallery);
  vm.runInNewContext(code, context);
  const requests = [];
  const conversations = new context.exports.Conversations((id, cursor) => new Promise((resolve, reject) => {
    requests.push({ id, cursor, resolve: (replies, nextCursor = null) => resolve({ replies, nextCursor }), reject });
  }));
  const view = (id) => { const panel = conversations.view(id); document.append(panel); return panel; };
  return { document, requests, conversations, view, Conversations: context.exports.Conversations };
}

test("view is stable and inert until explicit activation; reentry reuses loaded data", async () => {
  const h = harness();
  const a = h.view("a");
  h.view("b");
  assert.equal(h.conversations.view("a"), a);
  assert.equal(a.dataset.conversationRoot, "a");
  assert.equal(status(a).getAttribute("role"), "status");
  assert.equal(byClass(a, "conversation-title")[0].tabIndex, -1);
  await settle();
  assert.equal(h.requests.length, 0);
  h.conversations.activate("a");
  h.conversations.activate("a");
  assert.equal(status(a).textContent, "Loading replies...");
  assert.equal(status(a).dataset.state, "loading");
  await settle();
  assert.deepEqual(h.requests.map(({ id, cursor }) => [id, cursor]), [["a", null]]);
  h.requests[0].resolve([card("one")]);
  await settle();
  const row = rows(a)[0];
  h.conversations.activate("b");
  h.conversations.activate("a");
  await settle();
  assert.equal(h.requests.length, 2);
  assert.equal(rows(a)[0], row);
  assert.equal(status(a).textContent, "All replies loaded.");
  assert.equal(status(a).dataset.state, "ready");
});

test("first-page failure exposes retry; repeated clicks share one request; empty is real", async () => {
  const h = harness();
  const panel = h.view("root");
  h.conversations.activate("root");
  await settle();
  h.requests[0].reject(new Error("network detail"));
  await settle();
  assert.equal(status(panel).textContent, "Could not load replies.");
  assert.equal(status(panel).dataset.state, "error");
  assert.equal(panel.textContent.includes("network detail"), false);
  const retry = button(panel, "Retry");
  retry.click();
  retry.click();
  await settle();
  assert.equal(h.requests.length, 2);
  h.requests[1].resolve([]);
  await settle();
  assert.equal(status(panel).textContent, "No replies yet.");
  assert.equal(panel.getAttribute("aria-busy"), "false");
  assert.equal(byClass(panel, "conversation-pagination")[0].children[0].hidden, true);
});

test("pagination deduplicates IDs and preserves attached rows on failure and retry", async () => {
  const h = harness();
  const panel = h.view("root");
  h.conversations.activate("root");
  await settle();
  const first = card("one");
  h.requests[0].resolve([first, first], "page-two");
  await settle();
  const row = rows(panel)[0];
  button(panel, "Load more").click();
  await settle();
  assert.equal(h.requests[1].cursor, "page-two");
  h.requests[1].reject(new Error("offline"));
  await settle();
  assert.equal(status(panel).textContent, "Could not load more replies.");
  assert.equal(rows(panel)[0], row);
  assert.equal(row.isConnected, true);
  button(panel, "Retry").click();
  await settle();
  assert.equal(h.requests[2].cursor, "page-two");
  h.requests[2].resolve([first, card("two"), card("two")]);
  await settle();
  assert.deepEqual(ids(panel), ["one", "two"]);
  assert.equal(rows(panel)[0], row);
  assert.equal(status(panel).textContent, "All replies loaded.");
});

test("cursor cycles fail visibly without appending an untrustworthy page", async () => {
  const h = harness();
  const panel = h.view("root");
  h.conversations.activate("root");
  await settle();
  h.requests[0].resolve([card("one")], "a");
  await settle();
  button(panel, "Load more").click();
  await settle();
  h.requests[1].resolve([card("two")], "b");
  await settle();
  button(panel, "Load more").click();
  await settle();
  h.requests[2].resolve([card("three")], "a");
  await settle();
  assert.deepEqual(ids(panel), ["one", "two"]);
  assert.equal(status(panel).textContent, "Could not load more replies.");
  button(panel, "Retry");
});

test("nested replies stay in the root panel, keep separate data, and support Back", async () => {
  const h = harness();
  const panel = h.view("root");
  h.conversations.activate("root");
  await settle();
  h.requests[0].resolve([card("child")], "root-more");
  await settle();
  button(rows(panel)[0], "View replies").click();
  assert.equal(panel.dataset.conversationRoot, "root");
  assert.equal(panel.dataset.conversationThread, "child");
  await settle();
  assert.equal(h.requests[1].id, "child");
  button(panel, "Back").click();
  h.requests[1].resolve([card("grandchild")]);
  await settle();
  assert.deepEqual(ids(panel), ["child"]);
  button(panel, "Load more");
  button(rows(panel)[0], "View replies").click();
  assert.deepEqual(ids(panel), ["grandchild"]);
  h.view("other");
  h.conversations.activate("other");
  h.conversations.activate("root");
  await settle();
  assert.equal(panel.dataset.conversationThread, "child");
  assert.equal(h.requests.filter((request) => request.id === "child").length, 1);
  button(panel, "Back").click();
  assert.deepEqual(ids(panel), ["child"]);
});

test("refresh wins over stale first-page success and failure", async () => {
  for (const outcome of ["resolve", "reject"]) {
    const h = harness();
    const panel = h.view("root");
    h.conversations.activate("root");
    await settle();
    const refresh = h.conversations.refresh("root");
    await settle();
    h.requests[1].resolve([card("fresh")]);
    await refresh;
    if (outcome === "resolve") h.requests[0].resolve([card("stale")], "stale-cursor");
    else h.requests[0].reject(new Error("late failure"));
    await settle();
    assert.deepEqual(ids(panel), ["fresh"]);
    assert.equal(status(panel).textContent, "All replies loaded.");
  }
});

test("account safety reset removes nested history and ignores outstanding previous-account replies", async () => {
  const h = harness(), panel = h.view("root");
  h.conversations.activate("root"); await settle();
  h.requests[0].resolve([card("child")]); await settle();
  button(rows(panel)[0], "View replies").click(); await settle();
  assert.equal(panel.dataset.conversationThread, "child");
  h.conversations.clear();
  assert.equal(h.conversations.view("root"), panel, "mounted panel identity is retained");
  assert.deepEqual(ids(panel), []);
  assert.equal(panel.dataset.conversationThread, "root");
  assert.equal(panel.dataset.nested, "false");
  h.requests[1].resolve([card("late-private-filter-result")]); await settle();
  assert.deepEqual(ids(panel), []);
  h.conversations.activate("root"); await settle();
  h.requests[2].resolve([card("fresh")]); await settle();
  assert.deepEqual(ids(panel), ["fresh"]);
});

test("refresh retains loaded pages, DOM anchors and scroll; stale pagination cannot append", async () => {
  const h = harness();
  const panel = h.view("root");
  h.conversations.activate("root");
  await settle();
  h.requests[0].resolve([card("one")], "two");
  await settle();
  button(panel, "Load more").click();
  await settle();
  h.requests[1].resolve([card("two")], "three");
  await settle();
  const oldRows = rows(panel);
  panel.scrollTop = 271;
  button(panel, "Load more").click();
  await settle();
  const refresh = h.conversations.refresh("root");
  await settle();
  assert.deepEqual(rows(panel), oldRows);
  assert.equal(status(panel).textContent, "Refreshing replies...");
  h.requests[3].resolve([card("new"), card("one")], "new-cursor");
  await refresh;
  h.requests[2].resolve([card("stale")]);
  await settle();
  assert.deepEqual(ids(panel), ["new", "one", "two"]);
  assert.equal(rows(panel)[1], oldRows[0]);
  assert.equal(rows(panel)[2], oldRows[1]);
  assert.equal(panel.scrollTop, 271);
  button(panel, "Load more").click();
  await settle();
  assert.equal(h.requests[4].cursor, "new-cursor");
});

test("failed refresh retains data and retries the head rather than the old next page", async () => {
  const h = harness();
  const panel = h.view("root");
  h.conversations.activate("root");
  await settle();
  h.requests[0].resolve([card("one")], "two");
  await settle();
  const refresh = h.conversations.refresh("root");
  await settle();
  h.requests[1].reject(new Error("offline"));
  await refresh;
  assert.deepEqual(ids(panel), ["one"]);
  button(panel, "Retry").click();
  await settle();
  assert.equal(h.requests[2].cursor, null);
  h.requests[2].resolve([card("new"), card("one")]);
  await settle();
  assert.deepEqual(ids(panel), ["new", "one"]);
});

test("inactive refresh is lazy, invalidates pending data, and never loads unknown targets", async () => {
  const h = harness();
  const a = h.view("a");
  h.view("b");
  h.conversations.activate("a");
  await settle();
  h.conversations.activate("b");
  await settle();
  await h.conversations.refresh("a");
  await h.conversations.refresh("unknown");
  h.requests[0].resolve([card("stale")]);
  await settle();
  assert.equal(h.requests.length, 2);
  assert.deepEqual(ids(a), []);
  h.conversations.activate("a");
  await settle();
  assert.equal(h.requests[2].id, "a");
  h.requests[2].resolve([card("fresh")]);
  await settle();
  assert.deepEqual(ids(a), ["fresh"]);
});

test("refresh of a shared nested thread updates every matching cached panel", async () => {
  const h = harness();
  const a = h.view("a");
  const b = h.view("b");
  h.conversations.activate("a");
  await settle();
  h.requests[0].resolve([card("shared")]);
  await settle();
  button(a, "View replies").click();
  await settle();
  h.requests[1].resolve([card("old")]);
  await settle();
  h.conversations.activate("b");
  await settle();
  h.requests[2].resolve([card("shared")]);
  await settle();
  button(b, "View replies").click();
  await settle();
  assert.equal(h.requests.length, 3);
  const refresh = h.conversations.refresh("shared");
  await settle();
  h.requests[3].resolve([card("new"), card("old")]);
  await refresh;
  assert.deepEqual(ids(a), ["new", "old"]);
  assert.deepEqual(ids(b), ["new", "old"]);
});

test("reply media and nested parent context select audio/video players instead of images", async () => {
  for (const mediaKind of ["audio", "video"]) {
    const h = harness(), panel = h.view("root");
    h.conversations.activate("root");
    await settle();
    const reply = card("clip", { media: "https://media.test/clip", mediaKind, mediaType: `${mediaKind}/mp4` });
    h.requests[0].resolve([reply]); await settle();
    assert.equal(all(panel, node => node.tag === mediaKind).length, 1);
    assert.equal(all(panel, node => node.tag === "img").length, 0);
    button(panel, "View replies").click();
    const player = all(panel, node => node.tag === mediaKind)[0];
    assert.ok(player);
    assert.equal(player.src, reply.media);
    assert.equal(all(panel, node => node.tag === "img").length, 0);
  }
});

test("reply albums and nested parent context expose every attachment with independent selection", async () => {
  const h = harness(), panel = h.view("root");
  h.conversations.activate("root");
  await settle();
  const mediaItems = [
    { media: "https://media.test/one.png", mediaKind: "image", mediaType: "image/png", integrity: "a" },
    { media: "https://media.test/two.mp3", mediaKind: "audio", mediaType: "audio/mpeg", integrity: "b" },
    { media: "https://media.test/three.mp4", mediaKind: "video", mediaType: "video/mp4", integrity: "c" },
  ];
  h.requests[0].resolve([card("album", { ...mediaItems[1], mediaItems })]);
  await settle();
  const gallery = byClass(rows(panel)[0], "media-gallery")[0];
  assert.equal(gallery.dataset.mediaGallery, "compact");
  assert.equal(gallery.dataset.mediaSelected, "1");
  const selectors = gallery.children[2].children;
  for (let index = 0; index < mediaItems.length; index++) {
    selectors[index].click();
    assert.equal(gallery.dataset.mediaSelected, String(index));
    assert.equal(gallery.children[0].dataset.kind, mediaItems[index].mediaKind);
  }
  button(panel, "View replies").click();
  const parent = byClass(panel, "thread-parent")[0];
  const contextGallery = byClass(parent, "media-gallery")[0];
  assert.equal(contextGallery.dataset.mediaGallery, "compact");
  assert.equal(contextGallery.dataset.mediaSelected, "1");
  contextGallery.children[2].children[0].click();
  assert.equal(contextGallery.dataset.mediaSelected, "0");
  assert.equal(gallery.dataset.mediaSelected, "2");
  button(panel, "Back").click();
  const returned = byClass(rows(panel)[0], "media-gallery")[0];
  assert.equal(returned.dataset.mediaSelected, "1");
  assert.equal(returned.children[2].children.length, 3);
});

test("reply DOM preserves full text, semantic dates, actual media and delegated actions", async () => {
  const h = harness();
  const panel = h.view("root");
  h.conversations.activate("root");
  await settle();
  const content = '<script>alert("no")</script>\n' + "A long reply. ".repeat(1000);
  const author = "did:babel:1234567890abcdefghijklmnopqrstuvwxyz";
  h.requests[0].resolve([card("text", { content, author }), card("media", {
    media: "https://example.test/reply.png", surfaces: [{ target: "app" }],
  })]);
  await settle();
  const text = rows(panel).find((row) => row.dataset.objectId === "text");
  const media = rows(panel).find((row) => row.dataset.objectId === "media");
  assert.equal(byClass(text, "reply-content")[0].textContent, content);
  assert.equal(byClass(text, "reply-author")[0].children[0].title, author);
  assert.equal(byClass(text, "reply-author")[0].children[0].textContent, "did:babel:12...stuvwxyz");
  const profile = byClass(text, "reply-author")[0].children[0];
  assert.equal(profile.tag, "button");
  assert.equal(profile.type, "button");
  assert.equal(profile.dataset.profileAuthor, author);
  assert.equal(profile.getAttribute("aria-label"), `View public profile for ${author}`);
  const time = all(text, (element) => element.tag === "time")[0];
  assert.equal(time.dateTime, "2026-09-29T12:00:00.000Z");
  assert.equal(byClass(text, "reply-media").length, 0);
  assert.equal(byClass(media, "reply-media")[0].src, "https://example.test/reply.png");
  assert.equal(button(text, "Reply").dataset.action, "reply");
  assert.equal(button(media, "Surface").dataset.action, "surface");
  const actions = [];
  panel.addEventListener("click", (event) => actions.push(event.target.dataset.action));
  button(text, "Reply").click();
  button(media, "Surface").click();
  const report = all(text, element => element.dataset.action === "report")[0];
  assert.ok(report, "every reply exposes its own report control");
  assert.equal(report.getAttribute("aria-label"), "Report reply");
  assert.equal(report.title, "Report reply");
  report.click();
  assert.deepEqual(actions, ["reply", "surface", "report"]);
  assert.equal(all(panel, (element) => element.tag === "script").length, 0);
});

test("offscreen controls cannot initiate pagination or nested loads", async () => {
  const h = harness();
  const a = h.view("a");
  h.conversations.activate("a");
  await settle();
  h.requests[0].resolve([card("child")], "more");
  await settle();
  h.view("b");
  h.conversations.activate("b");
  await settle();
  button(a, "Load more").click();
  button(a, "View replies").click();
  await settle();
  assert.equal(h.requests.length, 2);
  assert.equal(a.dataset.conversationThread, "a");
});

test("cache evicts old detached roots, protects active roots and never evicts attached nodes", async () => {
  const h = harness();
  const active = h.view("active");
  h.conversations.activate("active");
  await settle();
  h.requests[0].resolve([card("one")]);
  await settle();
  active.remove();
  const old = h.conversations.view("old");
  for (let index = 0; index < 160; index++) h.conversations.view(`detached-${index}`);
  assert.equal(h.conversations.view("active"), active);
  assert.notEqual(h.conversations.view("old"), old);
  const attached = [];
  for (let index = 0; index < 40; index++) attached.push(h.view(`attached-${index}`));
  for (let index = 0; index < attached.length; index++) {
    assert.equal(h.conversations.view(`attached-${index}`), attached[index]);
    assert.equal(attached[index].isConnected, true);
  }
  h.conversations.activate("new-active");
  await settle();
  assert.equal(h.requests.at(-1).id, "new-active");
});

test("synchronously throwing loaders render a recoverable error", async () => {
  const h = harness();
  const conversations = new h.Conversations(() => { throw new Error("sync failure"); });
  const panel = conversations.view("root");
  conversations.activate("root");
  await settle();
  assert.equal(status(panel).textContent, "Could not load replies.");
  button(panel, "Retry");
});

test("published tail is shown beyond the first page and survives refresh deduplication", async () => {
  const h = harness();
  const panel = h.view("root");
  h.conversations.activate("root");
  await settle();
  const firstPage = Array.from({ length: 20 }, (_, index) => card(`reply-${String(index).padStart(2, "0")}`));
  h.requests[0].resolve(firstPage, "page-two");
  await settle();
  const existingRows = rows(panel);
  const published = card("published-tail", { createdAt: "2026-09-29T13:00:00Z" });
  const refresh = h.conversations.refresh("root", published);
  assert.equal(ids(panel).at(-1), published.id);
  await settle();
  h.requests[1].resolve(firstPage, "page-two");
  await refresh;
  assert.equal(rows(panel).length, 21);
  assert.equal(ids(panel).at(-1), published.id);
  assert.deepEqual(rows(panel).slice(0, 20), existingRows);
  button(panel, "Load more").click();
  await settle();
  h.requests[2].resolve([card("reply-20", { createdAt: "2026-09-29T12:30:00Z" }), published]);
  await settle();
  assert.equal(rows(panel).length, 22);
  assert.deepEqual(ids(panel).slice(-2), ["reply-20", "published-tail"]);
});

test("published replies for unseen threads are cached lazily and ordered by timestamp then ID", async () => {
  const h = harness();
  const published = card("published", { createdAt: "2026-09-29T13:00:00Z" });
  await h.conversations.refresh("unseen", published);
  assert.equal(h.requests.length, 0);
  const panel = h.view("unseen");
  assert.deepEqual(ids(panel), [published.id]);
  h.conversations.activate("unseen");
  await settle();
  h.requests[0].resolve([published, card("b"), card("a")]);
  await settle();
  assert.deepEqual(ids(panel), ["a", "b", "published"]);
});

test("nested Back and reentry restore the column scroll; async data never forces a reset", async () => {
  const h = harness();
  const panel = h.view("root");
  const column = new Element("article");
  column.className = "post-card";
  h.document.append(column);
  column.append(panel);
  h.conversations.activate("root");
  await settle();
  h.requests[0].resolve([card("child")]);
  await settle();
  column.scrollTop = 700;
  button(panel, "View replies").click();
  column.scrollTop = 350;
  await settle();
  h.requests[1].resolve([card("grandchild")]);
  await settle();
  assert.equal(column.scrollTop, 350);
  button(panel, "Back").click();
  assert.equal(column.scrollTop, 700);
  button(panel, "View replies").click();
  assert.equal(column.scrollTop, 350);
  const refresh = h.conversations.refresh("child", card("published"));
  await settle();
  h.requests[2].resolve([card("grandchild")]);
  await refresh;
  assert.equal(column.scrollTop, 350);
});

test("chronological order preserves submillisecond timestamp precision before the ID tie-break", async () => {
  const h = harness();
  const panel = h.view("root");
  h.conversations.activate("root");
  await settle();
  h.requests[0].resolve([
    card("a", { createdAt: "2026-09-29T12:00:00.123999Z" }),
    card("z", { createdAt: "2026-09-29T12:00:00.123001Z" }),
    card("b", { createdAt: "2026-09-29T12:00:00.123001Z" }),
  ]);
  await settle();
  assert.deepEqual(ids(panel), ["b", "z", "a"]);
});

test("nested threads retain readable parent context and focus the thread heading", async () => {
  const h = harness();
  const panel = h.view("root");
  h.conversations.activate("root");
  await settle();
  const text = "A recognizable reply, with the original argument intact. ".repeat(20);
  h.requests[0].resolve([card("parent", { content: text, media: "/parent.png" })]);
  await settle();
  button(panel, "View replies").click();
  const context = byClass(panel, "thread-context")[0];
  const parent = byClass(panel, "thread-parent")[0];
  assert.equal(context.hidden, false);
  assert.equal(parent.tag, "details");
  assert.equal(parent.dataset.objectId, "parent");
  assert.equal(byClass(parent, "thread-parent-preview")[0].textContent, text);
  assert.equal(byClass(parent, "reply-content")[0].textContent, text);
  assert.equal(byClass(parent, "reply-media")[0].src, "/parent.png");
  assert.equal(byClass(panel, "conversation-title")[0].focused, true);
  assert.equal(byClass(panel, "conversation-title")[0].focusOptions.preventScroll, true);
  button(panel, "Back").click();
  assert.equal(context.hidden, true);
  assert.equal(byClass(panel, "thread-parent").length, 0);
});

test("inserting an earlier reply keeps the currently read reply at its viewport offset", async () => {
  const h = harness();
  const panel = h.view("root");
  const column = new Element("article");
  column.className = "post-card";
  h.document.append(column);
  column.append(panel);
  panel.offsetTop = 400;
  byClass(panel, "conversation-list")[0].offsetTop = 60;
  h.conversations.activate("root");
  await settle();
  h.requests[0].resolve([card("a"), card("b"), card("c")]);
  await settle();
  for (const row of rows(panel)) {
    row.offsetHeight = 100;
    Object.defineProperty(row, "offsetTop", { get: () => rows(panel).indexOf(row) * 100 });
  }
  column.scrollTop = 590;
  const refresh = h.conversations.refresh("root", card("earlier", { createdAt: "2026-09-29T11:00:00Z" }));
  assert.equal(column.scrollTop, 690);
  await settle();
  h.requests[1].resolve([card("earlier", { createdAt: "2026-09-29T11:00:00Z" }), card("a"), card("b"), card("c")]);
  await refresh;
  assert.equal(column.scrollTop, 690);
  assert.equal(rows(panel)[2].dataset.objectId, "b");
});
