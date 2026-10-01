import { createElement, Plus, RotateCcw, Trash2, X } from "lucide";
import type { LocalPreferences } from "./local-preferences";

const terms = [
  ["interests", "Interests"], ["expertise", "Expertise"],
  ["hiddenTerms", "Hidden words"], ["mutedTerms", "Muted words"],
] as const;
const rankings = [
  ["noveltyTolerance", "Novelty tolerance"],
  ["explorationPreference", "Exploration preference"],
  ["evidencePreference", "Evidence preference"],
  ["contradictionTolerance", "Contradiction tolerance"],
] as const;
const tabs = [["interests", "Interests"], ["filters", "Filters"], ["ranking", "Ranking"], ["local-data", "Local data"]] as const;
type Term = typeof terms[number][0];
type Ranking = typeof rankings[number][0];
type Tab = typeof tabs[number][0];
type Action = "reset" | "clear-history";
type Handlers = { save: (value: LocalPreferences) => void; reset: () => void; clearHistory: () => void };
type Input = { scope: string; preferences: LocalPreferences; warning: string | null; seenCount: number; preserveTab?: boolean };
let instance = 0;

function element<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  node.textContent = text;
  return node;
}
function button(text: string, attribute: string, action: () => void): HTMLButtonElement {
  const node = element("button", "", text);
  node.type = "button";
  node.setAttribute(attribute, "");
  node.addEventListener("click", action);
  return node;
}
function iconButton(label: string, icon: typeof Plus, attribute: string, action: () => void): HTMLButtonElement {
  const node = button("", attribute, action);
  node.className = "preferences-icon";
  node.title = label;
  node.setAttribute("aria-label", label);
  node.append(createElement(icon, { "aria-hidden": "true", focusable: "false" }));
  return node;
}
function field(name: string, type: string): HTMLInputElement {
  const node = element("input");
  node.type = type;
  node.setAttribute("data-preference-field", name);
  if (type === "range") { node.min = "0"; node.max = "100"; node.step = "any"; }
  return node;
}
function copy(value: LocalPreferences): LocalPreferences {
  return { ...value, interests: [...value.interests], expertise: [...value.expertise],
    hiddenTerms: [...value.hiddenTerms], mutedTerms: [...value.mutedTerms],
    hiddenAuthors: [...value.hiddenAuthors], creatorAffinity: { ...value.creatorAffinity } };
}

/** The parent owns persistence and calls show only when replacing the current draft. */
export class PreferencesView {
  private readonly prefix = `preferences-${++instance}`;
  private readonly scope = element("p", "preferences-scope");
  private readonly warning = element("p", "preferences-warning");
  private readonly status = element("p", "preferences-status");
  private readonly privacy = element("p", "preferences-privacy", "Stored on this device. Not sent to the node.");
  private readonly history = element("p", "preferences-history");
  private readonly form = element("form", "preferences-form");
  private readonly termFields = new Map<Term, HTMLTextAreaElement>();
  private readonly rankingFields = new Map<Ranking, { slider: HTMLInputElement; defaults: HTMLInputElement; output: HTMLOutputElement }>();
  private readonly tabButtons = new Map<Tab, HTMLButtonElement>();
  private readonly panels = new Map<Tab, HTMLElement>();
  private readonly hiddenList = element("ul", "preferences-author-list");
  private readonly affinityList = element("ul", "preferences-author-list");
  private readonly hiddenAuthor = field("hiddenAuthors", "text");
  private readonly affinityAuthor = field("creatorAffinityAuthorId", "text");
  private readonly confirmation = element("div", "preferences-confirmation");
  private readonly confirmationText = element("p");
  private readonly confirm: HTMLButtonElement;
  private readonly cancel: HTMLButtonElement;
  private readonly apply: HTMLButtonElement;
  private readonly discard: HTMLButtonElement;
  private readonly reset: HTMLButtonElement;
  private readonly clear: HTMLButtonElement;
  private hiddenAuthors: string[] = [];
  private affinity = new Map<string, HTMLInputElement>();
  private baseline: LocalPreferences | null = null;
  private pending: Action | null = null;
  private revision = 0;
  private nextCreatorId = 0;

  constructor(private readonly root: HTMLElement, private readonly handlers: Handlers) {
    root.classList.add("preferences-view");
    this.scope.setAttribute("data-preferences-scope", "");
    this.privacy.id = `${this.prefix}-privacy`;
    this.form.setAttribute("aria-label", "Feed preferences");
    this.form.setAttribute("aria-describedby", this.privacy.id);
    this.form.addEventListener("submit", event => { event.preventDefault(); this.save(); });
    this.warning.setAttribute("role", "status");
    this.warning.setAttribute("data-preferences-warning", "");
    this.status.setAttribute("role", "status");
    this.status.setAttribute("aria-live", "polite");
    this.status.setAttribute("aria-atomic", "true");
    this.status.setAttribute("data-preferences-status", "");
    const tablist = element("div", "preferences-tabs");
    tablist.setAttribute("role", "tablist");
    tablist.setAttribute("aria-label", "Feed preferences");
    for (const [key, label] of tabs) {
      const tab = button(label, "data-preference-tab", () => this.selectTab(key));
      tab.setAttribute("data-preference-tab", key);
      tab.id = `${this.prefix}-tab-${key}`;
      tab.setAttribute("role", "tab");
      tab.setAttribute("aria-controls", `${this.prefix}-panel-${key}`);
      tab.addEventListener("keydown", event => {
        const index = tabs.findIndex(([name]) => name === key);
        const next = event.key === "Home" ? 0 : event.key === "End" ? tabs.length - 1
          : event.key === "ArrowRight" ? (index + 1) % tabs.length
          : event.key === "ArrowLeft" ? (index + tabs.length - 1) % tabs.length : null;
        if (next === null) return;
        event.preventDefault(); event.stopPropagation();
        const target = tabs[next];
        if (target) this.selectTab(target[0], true);
      });
      const panel = element("section", "preferences-tab-panel");
      panel.id = `${this.prefix}-panel-${key}`;
      panel.setAttribute("role", "tabpanel");
      panel.setAttribute("aria-labelledby", tab.id);
      this.tabButtons.set(key, tab); this.panels.set(key, panel);
      tablist.append(tab);
    }
    for (const [key, title] of terms) {
      const label = element("label", "preferences-field", title);
      const hint = element("span", "preferences-hint", "One entry per line");
      hint.id = `${this.prefix}-${key}-hint`;
      const input = element("textarea"); input.rows = 3;
      input.setAttribute("data-preference-field", key);
      input.setAttribute("aria-describedby", hint.id);
      label.append(hint, input); this.termFields.set(key, input);
      this.panels.get(key === "interests" || key === "expertise" ? "interests" : "filters")?.append(label);
    }
    const hidden = element("div", "preferences-section");
    hidden.append(element("h3", "", "Hidden from feed"),
      element("p", "preferences-hint", "Hidden in ranked and Following feeds. Profiles, original visits, and conversations remain accessible."), this.hiddenList,
      this.authorEntry(this.hiddenAuthor, "Author ID to hide", "data-preferences-add-hidden-author", () => this.addHidden()));
    this.panels.get("filters")?.append(hidden);
    for (const [key, title] of rankings) this.addRanking(key, title);
    const creators = element("div", "preferences-section");
    creators.append(element("h3", "", "Creator affinity"), this.affinityList,
      this.authorEntry(this.affinityAuthor, "Creator author ID", "data-preferences-add-affinity", () => this.addAffinity()));
    this.panels.get("ranking")?.append(creators);
    this.reset = button("Reset defaults", "data-preferences-reset", () => this.request("reset"));
    this.reset.append(createElement(RotateCcw, { "aria-hidden": "true" }));
    this.clear = button("Clear history", "data-preferences-clear-history", () => this.request("clear-history"));
    this.clear.append(createElement(Trash2, { "aria-hidden": "true" }));
    const actions = element("div", "preferences-actions"); actions.append(this.reset, this.clear);
    this.confirm = button("", "data-preferences-confirm", () => this.confirmAction());
    this.cancel = button("Cancel", "data-preferences-cancel", () => this.closeConfirmation(true));
    const confirmationActions = element("div", "preferences-actions"); confirmationActions.append(this.cancel, this.confirm);
    this.confirmation.setAttribute("role", "group");
    this.confirmationText.id = `${this.prefix}-confirmation`;
    this.confirmation.setAttribute("aria-labelledby", this.confirmationText.id);
    this.confirmation.append(this.confirmationText, confirmationActions);
    this.history.setAttribute("data-preferences-seen-count", "");
    this.panels.get("local-data")?.append(element("h3", "", "Local data"), this.history, actions, this.confirmation);
    this.apply = button("Apply", "data-preferences-apply", () => this.save());
    this.apply.className = "preferences-primary";
    this.discard = button("Discard changes", "data-preferences-discard", () => {
      if (!this.baseline) return;
      this.populate(this.baseline); this.closeConfirmation(); this.message("Changes discarded.");
    });
    const footer = element("div", "preferences-footer"); footer.append(this.discard, this.apply);
    this.form.append(tablist, ...this.panels.values(), footer);
    root.append(this.scope, this.privacy, this.warning, this.form, this.status);
    this.apply.disabled = this.discard.disabled = this.reset.disabled = this.clear.disabled = true;
    this.confirmation.hidden = this.warning.hidden = true;
    this.selectTab("interests");
  }

  public show(input: Input): void {
    const restoreFocus = this.root.contains(document.activeElement);
    const selected = input.preserveTab
      ? tabs.find(([key]) => this.tabButtons.get(key)?.getAttribute("aria-selected") === "true")?.[0] ?? "interests"
      : "interests";
    this.revision++;
    this.baseline = copy(input.preferences);
    this.scope.textContent = input.scope;
    this.warning.textContent = input.warning ?? "";
    this.warning.hidden = !input.warning;
    this.setSeenCount(input.seenCount);
    this.populate(input.preferences);
    this.closeConfirmation(); this.selectTab(selected, restoreFocus); this.message("");
    this.apply.disabled = this.discard.disabled = this.reset.disabled = this.clear.disabled = false;
  }

  public message(text: string, error = false): void {
    this.status.textContent = text;
    this.status.setAttribute("data-state", error ? "error" : "ready");
    this.status.setAttribute("role", error ? "alert" : "status");
    this.status.setAttribute("aria-live", error ? "assertive" : "polite");
  }

  public setSeenCount(count: number): void {
    this.history.textContent = `${count} seen ${count === 1 ? "object" : "objects"}`;
  }

  private selectTab(key: Tab, focus = false): void {
    this.closeConfirmation();
    for (const [name, tab] of this.tabButtons) {
      tab.setAttribute("aria-selected", String(name === key));
      tab.tabIndex = name === key ? 0 : -1;
      const panel = this.panels.get(name); if (panel) panel.hidden = name !== key;
    }
    if (focus) this.tabButtons.get(key)?.focus();
  }

  private authorEntry(input: HTMLInputElement, title: string, attribute: string, add: () => void): HTMLElement {
    const entry = element("div", "preferences-author-entry");
    const label = element("label", "preferences-field", title);
    input.autocomplete = "off"; input.spellcheck = false;
    input.addEventListener("keydown", event => {
      if (event.key !== "Enter") return;
      event.preventDefault(); event.stopPropagation(); add();
    });
    label.append(input);
    entry.append(label, iconButton(`Add ${title.toLowerCase()}`, Plus, attribute, add));
    return entry;
  }

  private addRanking(key: Ranking, title: string): void {
    const row = element("div", "preferences-ranking");
    const label = element("label", "preferences-range-label", title);
    const slider = field(key, "range"); slider.id = `${this.prefix}-${key}`; label.htmlFor = slider.id;
    const output = element("output"); output.setAttribute("for", slider.id);
    const defaults = field(`${key}Default`, "checkbox");
    defaults.setAttribute("aria-label", `${title}: Use Lens default`);
    const check = element("label", "preferences-default"); check.append(defaults, element("span", "", "Use Lens default"));
    const update = () => {
      slider.disabled = defaults.checked;
      output.textContent = defaults.checked ? "Lens default" : slider.value;
    };
    slider.addEventListener("input", update); defaults.addEventListener("change", update);
    label.append(output); row.append(label, slider, check);
    this.rankingFields.set(key, { slider, defaults, output }); this.panels.get("ranking")?.append(row);
  }

  private populate(value: LocalPreferences): void {
    for (const [key, input] of this.termFields) input.value = value[key].join("\n");
    for (const [key, { slider, defaults, output }] of this.rankingFields) {
      const score = value[key]; defaults.checked = score === null;
      slider.value = String(score === null ? 50 : score * 100); slider.disabled = defaults.checked;
      output.textContent = score === null ? "Lens default" : slider.value;
    }
    this.hiddenAuthor.value = this.affinityAuthor.value = "";
    this.hiddenAuthors = [...value.hiddenAuthors]; this.renderHidden();
    this.affinity.clear(); this.affinityList.replaceChildren();
    for (const [author, score] of Object.entries(value.creatorAffinity)) this.affinityRow(author, score * 100);
    this.affinityList.hidden = this.affinity.size === 0;
  }

  private renderHidden(): void {
    this.hiddenList.replaceChildren(...this.hiddenAuthors.map(author => {
      const row = element("li"); const name = element("span", "preferences-author-id", author);
      const remove = iconButton(`Remove hidden author ${author}`, X, "data-preferences-remove-hidden-author", () => {
        const index = this.hiddenAuthors.indexOf(author);
        this.hiddenAuthors.splice(index, 1); this.renderHidden();
        const next = this.hiddenList.children[Math.min(index, this.hiddenAuthors.length - 1)];
        (next?.querySelector<HTMLButtonElement>("button") ?? this.hiddenAuthor).focus();
      });
      remove.setAttribute("data-preferences-remove-hidden-author", author); row.append(name, remove); return row;
    }));
    this.hiddenList.hidden = this.hiddenAuthors.length === 0;
  }

  private addHidden(): void {
    const author = this.hiddenAuthor.value.trim();
    if (!author || this.hiddenAuthors.includes(author)) {
      this.message(author ? "That author is already hidden." : "Enter an author ID.", true); this.hiddenAuthor.focus(); return;
    }
    this.hiddenAuthors.push(author); this.renderHidden(); this.hiddenAuthor.value = ""; this.hiddenAuthor.focus(); this.message("");
  }

  private addAffinity(): void {
    const author = this.affinityAuthor.value.trim();
    if (!author || this.affinity.has(author)) {
      this.message(author ? "That creator already has an affinity value." : "Enter a creator author ID.", true); this.affinityAuthor.focus(); return;
    }
    this.affinityRow(author, 50); this.affinityList.hidden = false; this.affinityAuthor.value = "";
    this.affinity.get(author)?.focus(); this.message("");
  }

  private affinityRow(author: string, score: number): void {
    const row = element("li", "preferences-affinity-row");
    const label = element("label", "preferences-field preferences-author-id", author);
    const slider = field("creatorAffinity", "range"); slider.value = String(score);
    slider.setAttribute("data-preference-author-id", author); slider.setAttribute("aria-label", `Affinity for ${author}`);
    const output = element("output", "", String(score));
    slider.id = `${this.prefix}-creator-${this.nextCreatorId++}`; output.setAttribute("for", slider.id);
    slider.addEventListener("input", () => { output.textContent = slider.value; });
    label.append(slider);
    const remove = iconButton(`Remove creator ${author}`, X, "data-preferences-remove-affinity", () => {
      this.affinity.delete(author); row.remove(); this.affinityList.hidden = this.affinity.size === 0; this.affinityAuthor.focus();
    });
    remove.setAttribute("data-preferences-remove-affinity", author);
    row.append(label, output, remove); this.affinity.set(author, slider); this.affinityList.append(row);
  }

  private read(): LocalPreferences {
    const value = copy(this.baseline!);
    const text = Object.fromEntries([...this.termFields].map(([key, input]) => [key, input.value.split(/\r?\n/).map(line => line.trim()).filter(Boolean)])) as Record<Term, string[]>;
    const numeric = Object.fromEntries([...this.rankingFields].map(([key, { slider, defaults }]) => [key, defaults.checked ? null : this.score(slider)])) as Record<Ranking, number | null>;
    return { ...value, ...text, ...numeric, hiddenAuthors: [...this.hiddenAuthors],
      creatorAffinity: Object.fromEntries([...this.affinity].map(([author, slider]) => [author, this.score(slider)])) };
  }

  private score(input: HTMLInputElement): number {
    const score = Number(input.value);
    if (!input.value.trim() || !Number.isFinite(score) || score < 0 || score > 100) throw new Error("Choose a value from 0 to 100.");
    return score / 100;
  }

  private save(): void {
    if (!this.baseline) return;
    if (this.hiddenAuthor.value.trim() || this.affinityAuthor.value.trim()) {
      const pending = this.hiddenAuthor.value.trim() ? this.hiddenAuthor : this.affinityAuthor;
      this.selectTab(pending === this.hiddenAuthor ? "filters" : "ranking");
      this.message("Add or clear the pending author ID before applying.", true); pending.focus(); return;
    }
    const revision = this.revision;
    try {
      const value = this.read(); this.handlers.save(copy(value));
      if (revision !== this.revision) return;
      this.baseline = copy(value); this.closeConfirmation(); this.message("Preferences applied.");
    } catch (error) { this.failure(error); }
  }

  private request(action: Action): void {
    if (!this.baseline) return;
    this.pending = action;
    this.confirmationText.textContent = action === "reset"
      ? "Reset preferences to Lens defaults? Custom preferences and unsaved changes will be removed."
      : "Clear seen history on this device? Previously seen objects may appear again.";
    this.confirm.textContent = action === "reset" ? "Reset defaults" : "Clear history";
    this.confirm.setAttribute("data-preferences-confirm", action);
    this.confirmation.hidden = false; this.cancel.focus();
  }

  private closeConfirmation(focus = false): void {
    const action = this.pending; this.pending = null; this.confirmation.hidden = true;
    this.confirmationText.textContent = "";
    if (focus && action) (action === "reset" ? this.reset : this.clear).focus();
  }

  private confirmAction(): void {
    const action = this.pending; if (!action) return;
    const revision = this.revision;
    try {
      if (action === "reset") this.handlers.reset(); else this.handlers.clearHistory();
      if (revision !== this.revision) return;
      if (action === "reset") {
        this.baseline = { interests: [], expertise: [], hiddenTerms: [], mutedTerms: [], hiddenAuthors: [], creatorAffinity: {},
          noveltyTolerance: null, explorationPreference: null, evidencePreference: null, contradictionTolerance: null };
        this.populate(this.baseline);
      } else this.setSeenCount(0);
      this.closeConfirmation(true);
      this.message(action === "reset" ? "Preferences reset to Lens defaults." : "Seen history cleared.");
    } catch (error) { this.failure(error); }
  }

  private failure(error: unknown): void {
    this.message(error instanceof Error ? error.message : "Could not update local preferences. Try again.", true);
  }
}
