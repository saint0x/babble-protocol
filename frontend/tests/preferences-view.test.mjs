import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";
import ts from "typescript";

const source = readFileSync(new URL("../src/app/preferences-view.ts", import.meta.url), "utf8");
const code = ts.transpileModule(source, {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS },
}).outputText;
const rankingKeys = ["noveltyTolerance", "explorationPreference", "evidencePreference", "contradictionTolerance"];
const authorA = `id_${"a".repeat(64)}`, authorB = `id_${"b".repeat(64)}`;
const defaults = () => ({ interests: [], expertise: [], hiddenTerms: [], mutedTerms: [], hiddenAuthors: [], creatorAffinity: {},
  noveltyTolerance: null, explorationPreference: null, evidencePreference: null, contradictionTolerance: null });
const plain = value => JSON.parse(JSON.stringify(value));

function harness(handlers = {}) {
  class Element {
    children = []; attributes = {}; listeners = {}; value = ""; checked = false;
    hidden = false; disabled = false; className = ""; parent = null; ownText = "";
    classList = { add: name => { this.className += ` ${name}`; } };
    constructor(tag) { this.tag = tag; }
    get textContent() { return this.ownText + this.children.map(child => child.textContent).join(""); }
    set textContent(value) { this.ownText = value; this.replaceChildren(); }
    set innerHTML(_) { throw new Error("Unsafe HTML assignment"); }
    append(...children) { for (const child of children) { child.parent = this; this.children.push(child); } }
    replaceChildren(...children) { for (const child of this.children) child.parent = null; this.children = []; this.append(...children); }
    remove() { if (this.parent) this.parent.children = this.parent.children.filter(child => child !== this); this.parent = null; }
    setAttribute(name, value) { this.attributes[name] = String(value); }
    getAttribute(name) { return this.attributes[name] ?? null; }
    addEventListener(name, callback) { (this.listeners[name] ??= []).push(callback); }
    querySelector(tag) { return walk(this).slice(1).find(node => node.tag === tag) ?? null; }
    contains(node) { return walk(this).includes(node); }
    focus() { document.activeElement = this; }
    emit(name, values = {}) {
      const event = { defaultPrevented: false, stopped: false,
        preventDefault() { this.defaultPrevented = true; }, stopPropagation() { this.stopped = true; }, ...values };
      if (this.disabled && name === "click") return event;
      for (const callback of this.listeners[name] ?? []) callback(event);
      return event;
    }
  }
  const walk = node => [node, ...node.children.flatMap(walk)];
  const document = { createElement: tag => new Element(tag), activeElement: null };
  const context = { document, Error, exports: {}, require(name) {
    assert.equal(name, "lucide", "Only Lucide may be imported at runtime");
    return { createElement: () => new Element("svg") };
  } };
  vm.runInNewContext(code, context);
  const root = new Element("div"); const saved = []; let resets = 0, clears = 0;
  const view = new context.exports.PreferencesView(root, {
    save: value => { saved.push(plain(value)); handlers.save?.(value); },
    reset: () => { resets++; handlers.reset?.(); },
    clearHistory: () => { clears++; handlers.clearHistory?.(); },
  });
  const all = (attribute, value) => walk(root).filter(node => attribute in node.attributes && (value === undefined || node.attributes[attribute] === value));
  const one = (attribute, value) => { const matches = all(attribute, value); assert.equal(matches.length, 1, `${attribute}=${value}`); return matches[0]; };
  const click = (attribute, value) => one(attribute, value).emit("click");
  const input = (key, value, type = "input") => { const node = one("data-preference-field", key); node.value = value; node.emit(type); return node; };
  const show = (preferences = defaults(), other = {}) => view.show({ scope: "This device - Guest", preferences, warning: null, seenCount: 4, ...other });
  return { root, view, saved, document, all, one, click, input, show, walk, get resets() { return resets; }, get clears() { return clears; } };
}

test("all fields map through the production module; multiline terms preserve punctuation", () => {
  const h = harness(); h.show();
  const expected = defaults();
  for (const key of ["interests", "expertise", "hiddenTerms", "mutedTerms"]) {
    h.input(key, `  ${key} first  \r\n\nsecond, with comma\n`);
    expected[key] = [`${key} first`, "second, with comma"];
  }
  h.input("hiddenAuthors", authorA); h.click("data-preferences-add-hidden-author");
  h.input("creatorAffinityAuthorId", authorB); h.click("data-preferences-add-affinity");
  h.input("creatorAffinity", "75");
  expected.hiddenAuthors = [authorA]; expected.creatorAffinity[authorB] = .75;
  rankingKeys.forEach((key, index) => {
    const check = h.one("data-preference-field", `${key}Default`); check.checked = false; check.emit("change");
    h.input(key, String(index * 25)); expected[key] = index * .25;
  });
  h.click("data-preferences-apply");
  assert.deepEqual(h.saved, [expected]);
  assert.equal(h.one("data-preferences-status").textContent, "Preferences applied.");
});

test("every ranking slider distinguishes zero from Lens default and roundtrips fractional scores", () => {
  for (const value of [null, 0, .375, 1]) {
    const h = harness(); const preferences = { ...defaults(), ...Object.fromEntries(rankingKeys.map(key => [key, value])) };
    h.show(preferences);
    for (const key of rankingKeys) {
      const slider = h.one("data-preference-field", key), check = h.one("data-preference-field", `${key}Default`);
      assert.equal(slider.disabled, value === null); assert.equal(check.checked, value === null);
      assert.equal(slider.value, String(value === null ? 50 : value * 100));
      assert.equal(slider.min, "0"); assert.equal(slider.max, "100");
    }
    h.click("data-preferences-apply"); assert.deepEqual(h.saved[0], preferences);
  }
});

test("default toggles retain the draft numeric value and emit null only while checked", () => {
  const h = harness(); h.show({ ...defaults(), evidencePreference: 0 });
  const check = h.one("data-preference-field", "evidencePreferenceDefault");
  check.checked = true; check.emit("change"); h.click("data-preferences-apply");
  assert.equal(h.saved.at(-1).evidencePreference, null);
  check.checked = false; check.emit("change"); h.click("data-preferences-apply");
  assert.equal(h.saved.at(-1).evidencePreference, 0);
  assert.equal(h.one("data-preference-field", "evidencePreference").disabled, false);
});

test("creator affinities retain zero and endpoints, remove rows, and keep IDs unique after additions", () => {
  const h = harness(); h.show({ ...defaults(), creatorAffinity: { [authorA]: 0, [authorB]: 1 } });
  assert.deepEqual(h.all("data-preference-field", "creatorAffinity").map(node => node.value), ["0", "100"]);
  h.click("data-preferences-remove-affinity", authorA);
  h.input("creatorAffinityAuthorId", authorA); h.click("data-preferences-add-affinity");
  const ranges = h.all("data-preference-field", "creatorAffinity");
  assert.equal(new Set(ranges.map(node => node.id)).size, 2);
  h.click("data-preferences-apply"); assert.deepEqual(h.saved[0].creatorAffinity, { [authorB]: 1, [authorA]: .5 });
  h.click("data-preferences-remove-affinity", authorA); h.click("data-preferences-remove-affinity", authorB);
  h.click("data-preferences-apply"); assert.deepEqual(h.saved[1].creatorAffinity, {});
  assert.equal(h.document.activeElement, h.one("data-preference-field", "creatorAffinityAuthorId"));
});

test("hidden author add/remove is local to the draft and duplicate or empty IDs show errors", () => {
  const h = harness(); h.show();
  h.click("data-preferences-add-hidden-author"); assert.equal(h.one("data-preferences-status").attributes["data-state"], "error");
  const input = h.input("hiddenAuthors", ` ${authorA} `);
  const event = input.emit("keydown", { key: "Enter" }); assert.equal(event.defaultPrevented, true); assert.equal(event.stopped, true);
  h.input("hiddenAuthors", authorA); h.click("data-preferences-add-hidden-author");
  assert.equal(h.all("data-preferences-remove-hidden-author").length, 1);
  assert.equal(h.saved.length, 0); assert.equal(input.value, authorA);
  h.input("hiddenAuthors", ""); h.click("data-preferences-remove-hidden-author", authorA);
  assert.equal(h.document.activeElement, input); h.click("data-preferences-apply");
  assert.deepEqual(h.saved[0].hiddenAuthors, []);
});

test("pending author IDs cannot be silently lost on Apply; duplicate creators are retained for correction", () => {
  for (const key of ["hiddenAuthors", "creatorAffinityAuthorId"]) {
    const h = harness(); h.show(); h.input(key, authorA); h.click("data-preferences-apply");
    assert.equal(h.saved.length, 0); assert.equal(h.document.activeElement, h.one("data-preference-field", key));
    assert.match(h.one("data-preferences-status").textContent, /pending author ID/);
  }
  const h = harness(); h.show({ ...defaults(), creatorAffinity: { [authorA]: .8 } });
  h.input("creatorAffinityAuthorId", authorA); h.click("data-preferences-add-affinity");
  assert.equal(h.all("data-preference-field", "creatorAffinity").length, 1);
  assert.equal(h.one("data-preference-field", "creatorAffinityAuthorId").value, authorA);
});

test("save failures show the real error, preserve all draft values, and allow retry", () => {
  let fail = true;
  const h = harness({ save() { if (fail) throw new Error("Storage quota exceeded"); } });
  h.show(); h.input("interests", "uncommitted\nprivate");
  h.input("hiddenAuthors", authorA); h.click("data-preferences-add-hidden-author");
  h.click("data-preferences-apply");
  const status = h.one("data-preferences-status");
  assert.equal(status.textContent, "Storage quota exceeded"); assert.equal(status.attributes.role, "alert");
  assert.equal(h.one("data-preference-field", "interests").value, "uncommitted\nprivate");
  assert.equal(h.all("data-preferences-remove-hidden-author").length, 1);
  fail = false; h.click("data-preferences-apply");
  assert.deepEqual(h.saved.at(-1).interests, ["uncommitted", "private"]);
  assert.equal(status.attributes["data-state"], "ready");
});

test("non-Error failures and invalid numeric values never report success", () => {
  const h = harness({ save() { throw "failed"; } }); h.show(); h.click("data-preferences-apply");
  assert.match(h.one("data-preferences-status").textContent, /Could not update/);
  const numeric = harness(); numeric.show({ ...defaults(), noveltyTolerance: .5 });
  for (const bad of ["101", "-1", "NaN", "Infinity", ""]) {
    numeric.input("noveltyTolerance", bad); numeric.click("data-preferences-apply");
    assert.equal(numeric.saved.length, 0); assert.equal(numeric.one("data-preferences-status").attributes["data-state"], "error");
  }
});

test("reset requires inline confirmation, Cancel restores focus, and a successful reset uses Lens defaults", () => {
  const h = harness(); h.show({ ...defaults(), interests: ["private"], noveltyTolerance: 0 });
  h.click("data-preference-tab", "local-data"); h.click("data-preferences-reset");
  assert.equal(h.resets, 0); assert.equal(h.document.activeElement, h.one("data-preferences-cancel"));
  h.click("data-preferences-cancel"); assert.equal(h.resets, 0); assert.equal(h.document.activeElement, h.one("data-preferences-reset"));
  h.click("data-preferences-confirm"); assert.equal(h.resets, 0);
  h.click("data-preferences-reset"); h.click("data-preferences-confirm", "reset"); assert.equal(h.resets, 1);
  assert.equal(h.one("data-preference-field", "interests").value, "");
  assert.equal(h.one("data-preference-field", "noveltyToleranceDefault").checked, true);
  assert.equal(h.one("data-preferences-seen-count").textContent, "4 seen objects");
  h.click("data-preferences-apply"); assert.deepEqual(h.saved[0], defaults());
});

test("clear history confirms separately and preserves unsaved preferences and pending author IDs", () => {
  const h = harness(); h.show(); h.input("interests", "draft"); h.input("hiddenAuthors", authorA);
  h.click("data-preference-tab", "local-data"); h.click("data-preferences-clear-history");
  assert.equal(h.clears, 0); h.click("data-preferences-cancel"); assert.equal(h.clears, 0);
  h.click("data-preferences-clear-history"); h.click("data-preferences-confirm", "clear-history");
  assert.equal(h.clears, 1); assert.equal(h.resets, 0);
  assert.equal(h.one("data-preference-field", "interests").value, "draft");
  assert.equal(h.one("data-preference-field", "hiddenAuthors").value, authorA);
  assert.equal(h.one("data-preferences-seen-count").textContent, "0 seen objects");
});

test("reset and clear-history failures retain drafts, history, and the retryable confirmation", () => {
  for (const action of ["reset", "clear-history"]) {
    const h = harness({ reset() { throw new Error("Reset failed"); }, clearHistory() { throw new Error("Clear failed"); } });
    h.show(); h.input("expertise", "private draft"); h.click("data-preference-tab", "local-data");
    h.click(`data-preferences-${action}`); h.click("data-preferences-confirm", action);
    assert.equal(h.one("data-preference-field", "expertise").value, "private draft");
    assert.equal(h.one("data-preferences-seen-count").textContent, "4 seen objects");
    assert.equal(h.one("data-preferences-status").attributes["data-state"], "error");
    assert.equal(h.one("data-preferences-confirm").parent.parent.hidden, false);
    h.click("data-preferences-cancel");
  }
});

test("tabs have linked ARIA, roving focus, wraparound arrows, Home/End, and retain inputs", () => {
  const h = harness(); h.show(); h.input("interests", "draft");
  const names = ["interests", "filters", "ranking", "local-data"];
  const tab = key => h.one("data-preference-tab", key);
  for (const name of names) {
    const panel = h.all("role", "tabpanel").find(node => node.id === tab(name).attributes["aria-controls"]);
    assert.equal(panel.attributes["aria-labelledby"], tab(name).id);
  }
  for (const [from, key, to] of [["interests", "ArrowLeft", "local-data"], ["local-data", "ArrowRight", "interests"],
    ["interests", "End", "local-data"], ["local-data", "Home", "interests"], ["interests", "ArrowRight", "filters"]]) {
    const event = tab(from).emit("keydown", { key }); assert.equal(event.defaultPrevented, true); assert.equal(event.stopped, true);
    assert.equal(h.document.activeElement, tab(to));
    assert.deepEqual(names.map(name => tab(name).tabIndex), names.map(name => name === to ? 0 : -1));
    assert.equal(h.all("role", "tabpanel").filter(node => !node.hidden).length, 1);
  }
  assert.equal(tab("filters").emit("keydown", { key: "Tab" }).defaultPrevented, false);
  h.click("data-preference-tab", "ranking"); assert.equal(tab("ranking").attributes["aria-selected"], "true");
  assert.equal(h.one("data-preference-field", "interests").value, "draft");
});

test("tab changes dismiss destructive confirmations without executing them", () => {
  const h = harness(); h.show(); h.click("data-preference-tab", "local-data"); h.click("data-preferences-reset");
  h.click("data-preference-tab", "filters"); h.click("data-preferences-confirm"); assert.equal(h.resets, 0);
});

test("a persisted Apply can retain its active tab and focus while ordinary account show resets it", () => {
  let h;
  h = harness({ save(value) { h.show(value, { preserveTab: true }); } });
  h.show(); h.click("data-preference-tab", "ranking");
  h.one("data-preference-field", "evidencePreference").focus();
  h.click("data-preferences-apply");
  assert.equal(h.one("data-preference-tab", "ranking").attributes["aria-selected"], "true");
  assert.equal(h.document.activeElement, h.one("data-preference-tab", "ranking"));
  h.show(); assert.equal(h.one("data-preference-tab", "interests").attributes["aria-selected"], "true");
});

test("show replaces all prior account data, drafts, warnings, status, and confirmation", () => {
  const h = harness(); const privatePreferences = { ...defaults(), interests: ["secret-interest"], expertise: ["secret-expertise"],
    hiddenTerms: ["secret-hidden"], mutedTerms: ["secret-muted"], hiddenAuthors: ["secret-author"], creatorAffinity: { "secret-creator": .8 }, evidencePreference: .7 };
  h.show(privatePreferences, { scope: "This device - @private", warning: "private-warning", seenCount: 999 });
  h.input("hiddenAuthors", "secret-pending"); h.input("creatorAffinityAuthorId", "secret-pending-creator");
  h.view.message("private error", true); h.click("data-preference-tab", "local-data"); h.click("data-preferences-reset");
  h.show();
  assert.equal(h.root.textContent.includes("secret"), false); assert.equal(h.root.textContent.includes("private"), false);
  for (const node of h.all("data-preference-field")) assert.equal(node.value.includes("secret"), false);
  assert.equal(h.one("data-preferences-scope").textContent, "This device - Guest");
  assert.equal(h.one("data-preferences-warning").hidden, true); assert.equal(h.one("data-preferences-status").textContent, "");
  assert.equal(h.one("data-preference-tab", "interests").attributes["aria-selected"], "true");
  h.click("data-preferences-confirm"); assert.equal(h.resets, 0);
  h.click("data-preferences-apply"); assert.deepEqual(h.saved[0], defaults());
});

test("setSeenCount updates only history; discard restores the latest successful snapshot", () => {
  const h = harness(); h.show({ ...defaults(), interests: ["saved"] }); h.input("interests", "draft");
  h.view.setSeenCount(1); assert.equal(h.one("data-preferences-seen-count").textContent, "1 seen object");
  assert.equal(h.one("data-preference-field", "interests").value, "draft");
  h.click("data-preferences-discard"); assert.equal(h.one("data-preference-field", "interests").value, "saved");
  h.input("interests", "next save"); h.click("data-preferences-apply"); h.input("interests", "another draft");
  h.click("data-preferences-discard"); assert.equal(h.one("data-preference-field", "interests").value, "next save");
});

test("parent show and message inside save or reset remain authoritative", () => {
  for (const action of ["save", "reset"]) {
    let h;
    h = harness({ [action]() { h.show({ ...defaults(), interests: ["normalized"] }); h.view.message("Parent persisted snapshot."); } });
    h.show(); h.input("interests", "draft");
    if (action === "save") h.click("data-preferences-apply");
    else { h.click("data-preference-tab", "local-data"); h.click("data-preferences-reset"); h.click("data-preferences-confirm"); }
    assert.equal(h.one("data-preference-field", "interests").value, "normalized");
    assert.equal(h.one("data-preferences-status").textContent, "Parent persisted snapshot.");
  }
});

test("show restores focus from a hidden tab and late callback failure overrides any success", () => {
  let h;
  h = harness({ save() { h.show(); h.view.message("Saved"); throw new Error("Refresh failed"); } });
  h.show(); h.one("data-preference-tab", "ranking").focus();
  h.show(); assert.equal(h.document.activeElement, h.one("data-preference-tab", "interests"));
  h.click("data-preferences-apply");
  assert.equal(h.one("data-preferences-status").textContent, "Refresh failed");
  assert.equal(h.one("data-preferences-status").attributes["data-state"], "error");
});

test("input snapshots and callback arguments cannot mutate the draft or discard baseline", () => {
  const h = harness({ save(value) { value.interests.push("callback mutation"); value.creatorAffinity[authorA] = 1; } });
  const preferences = { ...defaults(), interests: ["original"], creatorAffinity: { [authorA]: .25 } };
  h.show(preferences); preferences.interests.push("external mutation"); preferences.creatorAffinity[authorA] = 0;
  h.click("data-preferences-apply"); h.input("interests", "draft"); h.click("data-preferences-discard");
  assert.equal(h.one("data-preference-field", "interests").value, "original");
  assert.equal(h.one("data-preference-field", "creatorAffinity").value, "25");
});

test("parent clearHistory can update the seen count without resetting a pending draft", () => {
  let h;
  h = harness({ clearHistory() { h.view.setSeenCount(0); h.view.message("History cleared by parent."); } });
  h.show(); h.input("expertise", "unsaved expertise");
  h.click("data-preference-tab", "local-data"); h.click("data-preferences-clear-history"); h.click("data-preferences-confirm");
  assert.equal(h.one("data-preferences-seen-count").textContent, "0 seen objects");
  assert.equal(h.one("data-preference-field", "expertise").value, "unsaved expertise");
});

test("untrusted content is literal text; privacy and feed-only hiding scope are explicit", () => {
  const attack = '<img src=x onerror="alert(1)">';
  const h = harness(); h.show({ ...defaults(), interests: [attack], hiddenAuthors: [attack], creatorAffinity: { [attack]: .5 } }, { scope: attack, warning: attack });
  h.view.message(attack, true);
  assert.equal(h.one("data-preferences-scope").textContent, attack);
  assert.equal(h.one("data-preference-field", "interests").value, attack);
  assert.equal(h.walk(h.root).some(node => node.tag === "img"), false);
  assert.match(h.root.textContent, /Stored on this device\. Not sent to the node\./);
  assert.match(h.root.textContent, /Hidden from feed/); assert.match(h.root.textContent, /Profiles, original visits, and conversations remain accessible/);
  assert.match(source, /import type \{ LocalPreferences \}/);
  assert.doesNotMatch(code, /require\(["']\.\/local-preferences/);
  assert.doesNotMatch(source, /\b(?:fetch|localStorage|sessionStorage|confirm)\s*\(/);
});

test("CSS constrains small layouts, touch targets, text, hover and reduced motion without nested cards", () => {
  const css = readFileSync(new URL("../src/styles/preferences.css", import.meta.url), "utf8");
  assert.match(css, /letter-spacing: 0/); assert.match(css, /min-height: 44px/);
  assert.match(css, /repeat\(4, minmax\(0, 1fr\)\)/); assert.match(css, /grid-template-columns: minmax\(0, 1fr\) 44px/);
  assert.match(css, /overflow-wrap: anywhere/); assert.match(css, /font-size: 16px/);
  assert.match(css, /@media \(hover: hover\) and \(pointer: fine\)/);
  assert.match(css, /@media \(prefers-reduced-motion: reduce\)/);
  assert.match(css, /\[hidden\] \{ display: none !important/);
  assert.doesNotMatch(css, /box-shadow|transition: all|font-size:[^;]*vw/);
  assert.doesNotMatch(source, /className = ["'][^"']*card/);
  assert.match(css, /width: min\(680px, calc\(100vw - 32px\)\)/);
  assert.match(css, /\.settings-advanced[^\n]*border-radius: 0; background: transparent/);
});
