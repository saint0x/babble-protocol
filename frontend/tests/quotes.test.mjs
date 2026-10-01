import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const transpile = (name) => ts.transpileModule(readFileSync(new URL(`../src/app/${name}.ts`, import.meta.url), "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;
const code = transpile("quotes");
const reading = { exports: {} };
vm.runInNewContext(transpile("reading"), reading);

// DOM doubles retain parentage, focus, node identity and layout offsets while the
// real controller and reading helpers execute, without a browser or network.
class Element {
  constructor(tag) {
    Object.assign(this, { tag, children: [], parentElement: null, dataset: {}, attributes: new Map(),
      listeners: new Map(), className: "", hidden: false, disabled: false, text: "", scrollTop: 0,
      offsetTop: 0, offsetHeight: 0, clientHeight: 600 });
  }
  get offsetParent() { return this.parentElement; }
  get firstElementChild() { return this.children[0] ?? null; }
  get nextElementSibling() {
    const siblings = this.parentElement?.children ?? [];
    return siblings[siblings.indexOf(this) + 1] ?? null;
  }
  set textContent(value) { this.replaceChildren(); this.text = value; }
  get textContent() { return this.text + this.children.map((child) => child.textContent).join(""); }
  set innerHTML(_value) { throw new Error("HTML interpolation is forbidden"); }
  setAttribute(name, value) { this.attributes.set(name, value); }
  getAttribute(name) { return this.attributes.get(name) ?? null; }
  querySelectorAll(selector) {
    const key = { "[data-reading-anchor]": "readingAnchor", "[data-quote-retry]": "quoteRetry" }[selector];
    assert.ok(key, `Unexpected selector: ${selector}`);
    return all(this, (element) => element.dataset[key] !== undefined);
  }
  closest(selector) {
    for (let element = this; element; element = element.parentElement) {
      if (selector.startsWith(".") && element.className.split(" ").includes(selector.slice(1))) return element;
    }
    return null;
  }
  append(...children) { for (const child of children) this.insertBefore(child, null); }
  remove() {
    if (this.parentElement) this.parentElement.children.splice(this.parentElement.children.indexOf(this), 1);
    this.parentElement = null;
  }
  insertBefore(child, reference) {
    assert.ok(reference === null || reference.parentElement === this);
    if (child === reference) return child;
    child.remove();
    this.children.splice(reference === null ? this.children.length : this.children.indexOf(reference), 0, child);
    child.parentElement = this;
    return child;
  }
  replaceChildren(...children) { for (const child of [...this.children]) child.remove(); this.text = ""; this.append(...children); }
  replaceWith(child) { if (this.parentElement) { this.parentElement.insertBefore(child, this); this.remove(); } }
  addEventListener(name, callback) {
    const listeners = this.listeners.get(name) ?? [];
    listeners.push(callback);
    this.listeners.set(name, listeners);
  }
  dispatch(name) { for (const callback of this.listeners.get(name) ?? []) callback({ target: this }); }
  click() { if (!this.disabled) this.dispatch("click"); }
  focus() { this.focused = true; }
}

function all(element, predicate) {
  return element.children.flatMap((child) => [...(predicate(child) ? [child] : []), ...all(child, predicate)]);
}
const byClass = (element, name) => all(element, (child) => child.className.split(" ").includes(name));
const rows = (element) => byClass(element, "quote-preview");
const ids = (element) => rows(element).map((row) => row.dataset.quotedObject);
const status = (element) => byClass(element, "quotes-status")[0];
const more = (element) => byClass(element, "quotes-more")[0];
const button = (element, text) => {
  const found = all(element, (child) => child.tag === "button" && child.textContent === text && !child.hidden)[0];
  assert.ok(found, `Expected visible ${text} button`);
  return found;
};
const settle = () => new Promise((resolve) => setImmediate(resolve));
function card(id, overrides = {}) {
  return { id, author: "did:babble:1234567890abcdefghijklmnopqrstuvwxyz", title: `Title ${id}`, content: `Content ${id}`,
    media: null, mediaKind: null, mediaType: null, mediaItems: [], surfaces: [], ...overrides };
}
const item = (id, value = card(id)) => ({ targetId: id, card: value });

function harness(customLoad) {
  const document = { createElement: (tag) => new Element(tag) };
  const context = { exports: {}, document, AbortController, require(name) {
    if (name === "./reading") return reading.exports;
    assert.equal(name, "lucide");
    return { ArrowUpRight: "open", ChevronDown: "more", RotateCw: "retry", createElement(icon, attributes) {
      const element = new Element("svg");
      element.icon = icon;
      for (const [key, value] of Object.entries(attributes)) element.setAttribute(key, value);
      return element;
    } };
  } };
  vm.runInNewContext(code, context);
  const requests = [];
  const opened = [];
  const load = customLoad ?? ((id, cursor, signal) => new Promise((resolve, reject) => {
    requests.push({ id, cursor, signal, resolve: (items, nextCursor = null, objectId = id) => resolve({ objectId, items, nextCursor }), reject });
  }));
  const quotes = new context.exports.Quotes(load, (card, opener) => opened.push({ card, opener }));
  return { quotes, requests, opened, view: (id) => quotes.view(id) };
}

test("views stay stable, hidden and inert until activation; only the active card fetches", async () => {
  const h = harness();
  const root = h.view("root");
  for (let i = 0; i < 8; i++) h.view(`offscreen-${i}`);
  assert.equal(h.view("root"), root);
  assert.equal(root.dataset.quotesRoot, "root");
  assert.equal(root.hidden, true);
  await settle();
  assert.equal(h.requests.length, 0);
  h.quotes.activate("root");
  h.quotes.activate("root");
  assert.equal(root.hidden, false);
  assert.equal(status(root).textContent, "Loading shared posts...");
  assert.equal(status(root).getAttribute("role"), "status");
  assert.equal(root.getAttribute("aria-busy"), "true");
  assert.equal(byClass(root, "quotes-title")[0].hidden, true);
  await settle();
  assert.equal(h.requests.length, 1);
  assert.equal(h.requests[0].id, "root");
  assert.equal(h.requests[0].cursor, null);
  h.requests[0].resolve([item("original")]);
  await settle();
  const row = rows(root)[0];
  h.quotes.activate("other");
  h.quotes.activate("root");
  await settle();
  assert.equal(h.requests.length, 1, "navigation before the microtask does not start an offscreen load");
  assert.equal(rows(root)[0], row);
  assert.equal(root.getAttribute("aria-busy"), "false");
});

test("an authoritative empty page hides the whole section without an empty-state gap", async () => {
  const h = harness();
  const root = h.view("root");
  h.quotes.activate("root");
  await settle();
  h.requests[0].resolve([]);
  await settle();
  assert.equal(root.hidden, true);
  assert.equal(status(root).hidden, true);
  assert.equal(more(root).hidden, true);
  h.quotes.activate("root");
  await settle();
  assert.equal(h.requests.length, 1);
});

test("first-page failure has a real retry, suppresses error details and handles synchronous throws", async () => {
  let calls = 0;
  const h = harness(() => { calls++; if (calls === 1) throw new Error("secret internal detail"); return Promise.resolve({ objectId: "root", items: [], nextCursor: null }); });
  const root = h.view("root");
  h.quotes.activate("root");
  await settle();
  assert.equal(status(root).dataset.state, "error");
  assert.equal(status(root).textContent, "Could not load shared posts.");
  assert.equal(root.textContent.includes("secret"), false);
  h.quotes.activate("root");
  await settle();
  assert.equal(calls, 1, "failed pages are not retried by each render");
  const retry = button(root, "Retry");
  retry.click();
  retry.click();
  await settle();
  assert.equal(calls, 2);
  assert.equal(root.hidden, true);
});

test("pagination is explicit, deduplicates targets, preserves rows and retries the failed cursor", async () => {
  const h = harness();
  const root = h.view("root");
  h.quotes.activate("root");
  await settle();
  const first = item("one");
  h.requests[0].resolve([first, first], "second");
  await settle();
  assert.deepEqual(ids(root), ["one"]);
  assert.equal(h.requests.length, 1, "next cursor does not trigger a background fetch");
  const row = rows(root)[0];
  more(root).click();
  more(root).click();
  await settle();
  assert.equal(h.requests.length, 2);
  assert.equal(h.requests[1].cursor, "second");
  h.requests[1].reject(new Error("offline"));
  await settle();
  assert.equal(rows(root)[0], row);
  button(root, "Retry").click();
  await settle();
  assert.equal(h.requests[2].cursor, "second");
  h.requests[2].resolve([first, item("two"), item("two")]);
  await settle();
  assert.deepEqual(ids(root), ["one", "two"]);
  assert.equal(rows(root)[0], row);
  assert.equal(byClass(root, "quotes-title")[0].textContent, "Shared posts");
  assert.equal(more(root).hidden, true);
});

for (const nextCursor of ["second", "third"]) {
  test(`repeated/cyclic cursor ${nextCursor} rejects the entire page without partial state`, async () => {
    const h = harness();
    const root = h.view("root");
    h.quotes.activate("root");
    await settle();
    h.requests[0].resolve([item("one")], "second");
    await settle();
    more(root).click();
    await settle();
    h.requests[1].resolve([item("two")], "third");
    await settle();
    more(root).click();
    await settle();
    h.requests[2].resolve([item("untrusted")], nextCursor);
    await settle();
    assert.deepEqual(ids(root), ["one", "two"]);
    assert.equal(status(root).dataset.state, "error");
    button(root, "Retry").click();
    await settle();
    assert.equal(h.requests[3].cursor, "third");
    h.requests[3].resolve([item("three")]);
    await settle();
    assert.deepEqual(ids(root), ["one", "two", "three"]);
  });
}

test("empty intermediate pages keep an explicit next-page control", async () => {
  const h = harness();
  const root = h.view("root");
  h.quotes.activate("root");
  await settle();
  h.requests[0].resolve([], "second");
  await settle();
  assert.equal(more(root).hidden, false);
  assert.equal(root.hidden, false);
  more(root).click();
  await settle();
  h.requests[1].resolve([item("one")]);
  await settle();
  assert.deepEqual(ids(root), ["one"]);
});

for (const invalid of [
  { items: [item("one")], objectId: "other" },
  { items: [item("expected", card("wrong"))] },
  { items: [item("valid"), item("", null)] },
  { items: [item("valid")], nextCursor: "" },
  { items: [{ targetId: "bad" }] },
]) {
  test(`invalid target/page is rejected atomically: ${JSON.stringify(invalid)}`, async () => {
    const h = harness();
    const root = h.view("root");
    h.quotes.activate("root");
    await settle();
    h.requests[0].resolve(invalid.items, invalid.nextCursor ?? null, invalid.objectId ?? "root");
    await settle();
    assert.deepEqual(ids(root), []);
    assert.equal(status(root).dataset.state, "error");
    button(root, "Retry").click();
    await settle();
    h.requests[1].resolve([item("correct")]);
    await settle();
    assert.deepEqual(ids(root), ["correct"]);
  });
}

test("unavailable originals stay explicit and retry the owning page rather than a fabricated card", async () => {
  const h = harness();
  const root = h.view("root");
  h.quotes.activate("root");
  await settle();
  h.requests[0].resolve([item("one")], "second");
  await settle();
  more(root).click();
  await settle();
  h.requests[1].resolve([item("missing", null)], "third");
  await settle();
  more(root).click();
  await settle();
  h.requests[2].resolve([item("three")]);
  await settle();
  const missing = rows(root)[1];
  assert.match(missing.textContent, /Shared post unavailable/);
  assert.equal(all(missing, (node) => node.dataset.profileAuthor !== undefined).length, 0);
  assert.equal(all(missing, (node) => node.textContent === "Open post").length, 0);
  const retry = button(missing, "Retry");
  retry.click();
  assert.equal(retry.disabled, true);
  await settle();
  assert.equal(h.requests[3].cursor, "second");
  h.requests[3].resolve([item("missing")], "third");
  await settle();
  assert.deepEqual(ids(root), ["one", "missing", "three"]);
  assert.equal(more(root).hidden, true, "retrying a middle page preserves the tail cursor");
  button(rows(root)[1], "Open post").click();
  assert.equal(h.opened[0].card.id, "missing");
  retry.click();
  await settle();
  assert.equal(h.requests.length, 4, "retired retry controls cannot start requests");
});

test("resolved duplicates do not degrade to unavailable and unresolved duplicates can recover", async () => {
  const h = harness();
  const root = h.view("root");
  h.quotes.activate("root");
  await settle();
  h.requests[0].resolve([item("one"), item("two", null)], "second");
  await settle();
  more(root).click();
  await settle();
  h.requests[1].resolve([item("one", null), item("two")]);
  await settle();
  assert.deepEqual(ids(root), ["one", "two"]);
  assert.equal(root.textContent.includes("unavailable"), false);
});

test("retrying an unavailable head preserves loaded later pages and the tail cursor", async () => {
  const h = harness();
  const root = h.view("root");
  h.quotes.activate("root");
  await settle();
  h.requests[0].resolve([item("missing", null)], "second");
  await settle();
  more(root).click();
  await settle();
  h.requests[1].resolve([item("two")], "third");
  await settle();
  const tail = rows(root)[1];
  button(rows(root)[0], "Retry").click();
  await settle();
  assert.equal(h.requests[2].cursor, null);
  h.requests[2].resolve([item("missing")], "second");
  await settle();
  assert.deepEqual(ids(root), ["missing", "two"]);
  assert.equal(rows(root)[1], tail);
  more(root).click();
  await settle();
  assert.equal(h.requests[3].cursor, "third");
});

test("refresh failure retains readable context and every retry restarts the dirty head", async () => {
  const h = harness();
  const root = h.view("root");
  h.quotes.activate("root");
  await settle();
  h.requests[0].resolve([item("one")], "second");
  await settle();
  more(root).click();
  await settle();
  h.requests[1].resolve([item("missing", null)]);
  await settle();
  h.quotes.refresh("root");
  await settle();
  h.requests[2].reject(new Error("offline"));
  await settle();
  assert.deepEqual(ids(root), ["one", "missing"]);
  button(rows(root)[1], "Retry").click();
  await settle();
  assert.equal(h.requests[3].cursor, null, "an unavailable old row cannot bypass the head refresh");
  h.requests[3].resolve([item("new-head")]);
  await settle();
  assert.deepEqual(ids(root), ["new-head"]);
});

test("changed retry pagination is rejected until an explicit refresh rebuilds the pages", async () => {
  const h = harness();
  const root = h.view("root");
  h.quotes.activate("root");
  await settle();
  h.requests[0].resolve([item("missing", null)], "second");
  await settle();
  more(root).click();
  await settle();
  h.requests[1].resolve([item("two")]);
  await settle();
  button(rows(root)[0], "Retry").click();
  await settle();
  h.requests[2].resolve([item("missing")], "changed");
  await settle();
  assert.match(rows(root)[0].textContent, /unavailable/);
  assert.equal(status(root).dataset.state, "error");
  h.quotes.refresh("root");
  await settle();
  h.requests[3].resolve([item("missing")], "changed");
  await settle();
  assert.deepEqual(ids(root), ["missing"]);
  more(root).click();
  await settle();
  assert.equal(h.requests[4].cursor, "changed");
});

test("a runtime-null loader response is a visible failure, not false completion", async () => {
  const h = harness(() => Promise.resolve(null));
  const root = h.view("root");
  h.quotes.activate("root");
  await settle();
  assert.equal(root.hidden, false);
  assert.equal(status(root).dataset.state, "error");
  assert.equal(button(root, "Retry").disabled, false);
});

test("navigation aborts first-page work, ignores late completion and reactivation owns a fresh load", async () => {
  const h = harness();
  const a = h.view("a");
  h.quotes.activate("a");
  await settle();
  h.quotes.activate("b");
  assert.equal(h.requests[0].signal.aborted, true);
  assert.equal(a.getAttribute("aria-busy"), "false");
  await settle();
  h.quotes.activate("a");
  await settle();
  assert.equal(h.requests[1].signal.aborted, true);
  assert.equal(h.requests[2].id, "a");
  h.requests[0].resolve([item("stale")]);
  h.requests[1].reject(new Error("stale failure"));
  await settle();
  assert.equal(status(a).dataset.state, "loading");
  assert.deepEqual(ids(a), []);
  h.requests[2].resolve([item("fresh")]);
  await settle();
  assert.deepEqual(ids(a), ["fresh"]);
});

test("deactivated pagination is aborted; cached original and next cursor remain intact", async () => {
  const h = harness();
  const a = h.view("a");
  h.quotes.activate("a");
  await settle();
  h.requests[0].resolve([item("one")], "second");
  await settle();
  const open = button(a, "Open post");
  more(a).click();
  await settle();
  h.quotes.activate("b");
  await settle();
  assert.equal(h.requests[1].signal.aborted, true);
  open.click();
  more(a).click();
  assert.equal(h.opened.length, 0);
  assert.equal(h.requests.length, 3);
  h.requests[1].resolve([item("stale")]);
  await settle();
  assert.deepEqual(ids(a), ["one"]);
  h.quotes.activate("a");
  more(a).click();
  await settle();
  assert.equal(h.requests[3].cursor, "second");
});

test("clear drops account-owned views, aborts requests and rejects old callbacks for reused IDs", async () => {
  const h = harness();
  const old = h.view("root");
  h.quotes.activate("root");
  await settle();
  h.requests[0].resolve([item("old")], "second");
  await settle();
  const oldOpen = button(old, "Open post");
  const oldMore = more(old);
  oldMore.click();
  await settle();
  h.quotes.clear();
  assert.equal(h.requests[1].signal.aborted, true);
  assert.equal(old.hidden, true);
  assert.equal(old.children.length, 0);
  const fresh = h.view("root");
  assert.notEqual(fresh, old);
  h.quotes.activate("root");
  await settle();
  oldOpen.click();
  oldMore.click();
  assert.equal(h.opened.length, 0);
  h.requests[1].resolve([item("stale-account")]);
  await settle();
  assert.deepEqual(ids(fresh), []);
  assert.equal(fresh.getAttribute("aria-busy"), "true");
  h.requests[2].resolve([item("new-account")]);
  await settle();
  assert.deepEqual(ids(fresh), ["new-account"]);
});

test("refresh preserves the container, aborts obsolete work and atomically replaces the old head", async () => {
  const h = harness();
  const root = h.view("root");
  h.quotes.activate("root");
  await settle();
  h.requests[0].resolve([item("one")], "second");
  await settle();
  const oldOpen = button(root, "Open post");
  more(root).click();
  await settle();
  h.quotes.refresh("root");
  assert.equal(h.view("root"), root);
  assert.deepEqual(ids(root), ["one"]);
  assert.equal(h.requests[1].signal.aborted, true);
  await settle();
  h.requests[1].resolve([item("old-page")]);
  h.requests[2].resolve([item("new")]);
  await settle();
  assert.deepEqual(ids(root), ["new"]);
  oldOpen.click();
  assert.equal(h.opened.length, 0);
  h.quotes.refresh("root");
  await settle();
  h.requests[3].resolve([]);
  await settle();
  assert.equal(root.hidden, true);
  assert.deepEqual(ids(root), []);
});

test("inactive refresh only invalidates; missing views do not allocate or fetch", async () => {
  const h = harness();
  h.view("a");
  h.quotes.activate("a");
  await settle();
  h.requests[0].resolve([item("one")]);
  await settle();
  h.quotes.activate("b");
  await settle();
  h.quotes.refresh("a");
  h.quotes.refresh("unseen");
  await settle();
  assert.equal(h.requests.length, 2);
  h.quotes.activate("a");
  await settle();
  assert.equal(h.requests[2].id, "a");
  assert.equal(h.requests[2].cursor, null);
});

test("cache is bounded to 32, LRU views retire safely and the active panel is protected", async () => {
  const h = harness();
  const active = h.view("active");
  h.quotes.activate("active");
  await settle();
  const old = h.view("old");
  const retained = h.view("retained");
  for (let index = 0; index < 29; index++) h.view(`offscreen-${index}`);
  h.view("retained");
  h.view("new");
  assert.equal(old.children.length, 0, "the least recently used view was retired");
  assert.equal(h.view("retained"), retained);
  assert.equal(h.view("active"), active);
  assert.equal(h.requests[0].signal.aborted, false);
  assert.notEqual(h.view("old"), old);
  for (let index = 0; index < 100; index++) h.view(`more-${index}`);
  assert.equal(h.view("active"), active);
  assert.equal(h.quotes.panels.size, 32);
  await settle();
  assert.equal(h.requests.length, 1);
});

test("Open post exposes the exact card and opener, and never fetches quotes recursively", async () => {
  const h = harness();
  const root = h.view("root");
  h.quotes.activate("root");
  await settle();
  const original = card("original", { surfaces: [{ id: "game" }] });
  h.requests[0].resolve([item("original", original)]);
  await settle();
  const open = button(root, "Open post");
  assert.equal(open.dataset.quoteTarget, "original");
  assert.equal(all(root, (element) => element.dataset.quoteTarget === "original").length, 1);
  assert.equal(open.getAttribute("aria-label"), "Open post");
  assert.equal(open.children[0].getAttribute("aria-hidden"), "true");
  open.click();
  assert.equal(h.opened[0].card, original);
  assert.equal(h.opened[0].opener, open);
  await settle();
  assert.equal(h.requests.length, 1);
  assert.equal(all(root, (element) => ["iframe", "audio", "video"].includes(element.tag)).length, 0);
});

test("text, author and media title are literal; snippets are bounded and no HTML is interpreted", async () => {
  const h = harness();
  const root = h.view("root");
  h.quotes.activate("root");
  await settle();
  const attack = "<img src=x onerror=alert(1)>";
  const author = "<script>bad()</script>";
  h.requests[0].resolve([item("x", card("x", { author, title: attack.repeat(30), content: attack.repeat(50) }))]);
  await settle();
  const profile = byClass(root, "quote-author")[0];
  assert.equal(profile.dataset.profileAuthor, author);
  assert.equal(profile.textContent, author);
  assert.equal(profile.getAttribute("aria-label"), `View public profile for ${author}`);
  assert.equal(byClass(root, "quote-title")[0].textContent.length, 163);
  assert.equal(byClass(root, "quote-snippet")[0].textContent.length, 363);
  assert.equal(all(root, (element) => element.tag === "img" || element.tag === "script").length, 0);
});

for (const mediaKind of ["image", "audio", "video"]) {
  test(`${mediaKind} previews are inert, labeled and do not instantiate players`, async () => {
    const h = harness();
    const root = h.view("root");
    h.quotes.activate("root");
    await settle();
    const media = "https://example.test/objects/media";
    h.requests[0].resolve([item("media", card("media", { mediaKind, media, title: "A media title" }))]);
    await settle();
    assert.equal(rows(root)[0].dataset.kind, mediaKind);
    assert.match(root.textContent, /A media title/);
    assert.equal(all(root, (element) => ["iframe", "video", "audio", "script"].includes(element.tag)).length, 0);
    const images = all(root, (element) => element.tag === "img");
    assert.equal(images.length, mediaKind === "image" ? 1 : 0);
    if (mediaKind === "image") {
      assert.equal(images[0].src, media);
      assert.equal(images[0].loading, "lazy");
      assert.equal(images[0].width, images[0].height);
      assert.equal(images[0].referrerPolicy, "no-referrer");
      images[0].dispatch("error");
      assert.equal(images[0].hidden, true);
    }
  });
}

test("async status and quote changes preserve the nearest column's reading anchor", async () => {
  const h = harness();
  const column = new Element("article");
  column.className = "post-card";
  const root = h.view("root");
  const reply = new Element("article");
  reply.dataset.readingAnchor = "reply:later";
  reply.offsetHeight = 80;
  Object.defineProperty(reply, "offsetTop", { get: () => 500 + rows(root).length * 160 });
  column.append(root, reply);
  column.scrollTop = 510;
  h.quotes.activate("root");
  await settle();
  h.requests[0].resolve([item("one"), item("two")]);
  await settle();
  assert.equal(column.scrollTop, 830, "same reading offset after inserting quote rows above a reply");
  h.quotes.refresh("root");
  await settle();
  h.requests[1].resolve([]);
  await settle();
  assert.equal(column.scrollTop, 510, "removing quotes restores the same reply position");
});

test("pagination loading preserves an existing focused Open post node", async () => {
  const h = harness();
  const root = h.view("root");
  h.quotes.activate("root");
  await settle();
  h.requests[0].resolve([item("one")], "second");
  await settle();
  const open = button(root, "Open post");
  open.focus();
  more(root).click();
  await settle();
  h.requests[1].resolve([item("two")]);
  await settle();
  assert.equal(button(rows(root)[0], "Open post"), open);
  assert.equal(open.focused, true);
});

test("styles keep compact responsive previews, stable thumbnails and visible keyboard focus", () => {
  const css = readFileSync(new URL("../src/styles/quotes.css", import.meta.url), "utf8");
  assert.match(css, /\.quotes\[hidden\][\s\S]*?display: none/);
  assert.match(css, /\.quote-preview\s*\{[^}]*min-width: 0/);
  assert.match(css, /\.quote-preview\s*\{[^}]*border-radius: var\(--reply-radius/);
  assert.match(css, /\.quote-thumbnail\s*\{[^}]*aspect-ratio: 1/);
  assert.match(css, /@media \(max-width: 640px\)/);
  assert.match(css, /:focus-visible\s*\{[^}]*outline: 2px/);
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(css, /@media \(hover: hover\) and \(pointer: fine\)/);
  assert.doesNotMatch(css, /transition:\s*all|font-size:[^;]*vw/);
});
