import { ArrowLeft, ArrowUpRight, Flag, Inbox, LogIn, RotateCcw, ShieldCheck, X, createElement } from "lucide";
import { appealEligible, moderationId, moderationSignals, moderationText, reviewEligible } from "./moderation";
import { ModerationController } from "./moderation-controller";
import type { ModerationCase, ModerationOutcome, ModerationReason, ModerationScope, ModerationSource } from "./moderation";
export type { ModerationCase } from "./moderation";

export interface ModerationControlsOptions {
  readonly source: ModerationSource;
  readonly signIn: () => void;
  readonly openObject: (objectId: string, opener: HTMLElement | null) => void;
  readonly changed: (record: ModerationCase) => void;
}
const reasonNames: Record<ModerationReason, string> = { spam: "Spam", malware: "Malware", fraud: "Fraud", harassment: "Harassment", illegal_content: "Illegal content", other_integrity: "Other integrity concern" };
const statusNames = { pending: "Awaiting review", decided: "Decision made", appealed: "Appeal awaiting review", closed: "Final decision" };
const scopeNames: Record<ModerationScope, string> = { mine: "My reports", affected: "Affected posts", queue: "Reviewer queue" };
const restriction = "Restrictions apply on this node: the post is excluded from discovery, Following, and conversations; quoted post previews are hidden, and new or continued Surface execution is prevented. Signed history, quote relationships, and explicit public reads remain available. This is not deletion or a federation-wide ban. Other cases may keep a post restricted.";
type Draft = { reason: string; details: string; outcome: string; explanation: string; source_signals: string };
let instance = 0;
function element<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag); node.className = className; node.textContent = text; return node;
}

/** Native account-private dialog; the parent owns entrypoints and navigation. */
export class ModerationControls {
  private readonly controller: ModerationController;
  private readonly dialog = element("dialog", "moderation-dialog");
  private readonly title = element("h2");
  private readonly body = element("div", "moderation-body");
  private readonly status = element("p", "moderation-status");
  private readonly closeButton: HTMLButtonElement;
  private readonly id = `moderation-${++instance}`;
  private readonly drafts = new Map<string, Draft>();
  private opener: HTMLElement | null = null;
  private context = 0;
  private active = false;
  private disposed = false;
  private scope: ModerationScope = "mine";

  constructor(private readonly options: ModerationControlsOptions) {
    this.title.id = `${this.id}-title`; this.status.id = `${this.id}-status`;
    this.status.setAttribute("role", "status"); this.status.setAttribute("aria-live", "polite"); this.status.setAttribute("aria-atomic", "true");
    this.status.setAttribute("data-moderation-status", "");
    this.dialog.setAttribute("data-moderation-dialog", ""); this.dialog.setAttribute("aria-labelledby", this.title.id);
    this.closeButton = this.button("Close", "close", X, () => this.close(), true); this.closeButton.className = "moderation-close";
    const header = element("header", "moderation-header"); header.append(this.title, this.closeButton);
    this.dialog.append(header, this.body, this.status);
    this.dialog.addEventListener("cancel", event => { event.preventDefault(); this.close(); });
    this.dialog.addEventListener("close", () => { if (!this.dialog.open) this.dismiss(); });
    this.controller = new ModerationController(options.source, record => {
      this.drafts.delete(`report:${record.object_id}`); this.drafts.delete(`decision:${record.id}`); this.drafts.delete(`appeal:${record.id}`);
      options.changed(record);
    }, () => this.render());
    document.body.append(this.dialog);
  }
  account(owner: string | null): void {
    if (this.disposed || owner === this.controller.owner) return;
    this.close(); this.drafts.clear(); this.scope = "mine"; this.controller.account(owner);
  }
  openReport(objectId: string, opener: HTMLElement | null = null): void {
    if (this.disposed) return;
    this.begin(opener); void this.controller.show({ kind: "report", objectId });
  }
  openInbox(opener: HTMLElement | null = null): void {
    if (this.disposed) return;
    this.begin(opener); this.scope = "mine"; void this.controller.show({ kind: "inbox", scope: this.scope });
  }
  close(): void { if (this.dialog.open) this.dialog.close(); this.dismiss(); }
  dispose(): void {
    if (this.disposed) return;
    this.close(); this.drafts.clear(); this.controller.dispose(); this.dialog.remove(); this.disposed = true;
  }
  private begin(opener: HTMLElement | null): void {
    ++this.context;
    if (!this.dialog.open) this.opener = opener ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    this.active = true;
    if (!this.dialog.open) this.dialog.showModal(); this.closeButton.focus();
  }
  private dismiss(): void {
    if (!this.active) return;
    ++this.context; this.active = false; this.controller.hide();
    this.body.replaceChildren(); this.title.textContent = ""; this.status.textContent = "";
    const opener = this.opener; this.opener = null; if (opener?.isConnected) opener.focus();
  }
  private render(): void {
    if (!this.active) return;
    const active = document.activeElement;
    const focus = active instanceof HTMLElement && this.body.contains(active) ? active.getAttribute("data-moderation-focus") : null;
    this.body.replaceChildren();
    const view = this.controller.view, selection = view.selection;
    this.title.textContent = selection?.kind === "report" ? "Report post" : selection?.kind === "detail" ? "Report details" : "Reports and moderation";
    this.dialog.setAttribute("aria-busy", String(view.loading || this.controller.pending));
    this.message(view.message, view.error);
    if (!this.controller.owner) {
      this.body.append(element("p", "", "Sign in to report a post or view your private moderation inbox."),
        this.button("Sign in", "sign-in", LogIn, () => { this.close(); this.options.signIn(); }));
    } else {
      if (this.controller.uncertain) {
        this.body.append(element("p", "", "The previous submission has not been confirmed. Retry the same submission before sending another."),
          this.button("Retry previous submission", "retry", RotateCcw, () => { void this.controller.retry(); }));
      }
      if (selection?.kind === "inbox") this.inbox();
      else if (selection?.kind === "report") this.report(selection.objectId);
      else if (selection?.kind === "detail") this.detail();
      if ((view.error || selection?.kind !== "report") && !view.loading && !this.controller.pending && !this.controller.uncertain)
        this.body.append(this.button("Refresh", "refresh", RotateCcw, () => { void this.controller.refresh(); }, true));
    }
    if (focus && this.dialog.open) {
      const next = [...this.body.querySelectorAll<HTMLElement>("[data-moderation-focus]")].find(node => node.getAttribute("data-moderation-focus") === focus && !("disabled" in node && node.disabled));
      (next ?? this.closeButton).focus();
    }
  }
  private inbox(): void {
    const view = this.controller.view;
    const tabs = element("div", "moderation-tabs"); tabs.setAttribute("role", "tablist"); tabs.setAttribute("aria-label", "Moderation inbox");
    const scopes: ModerationScope[] = view.access?.can_review ? ["mine", "affected", "queue"] : ["mine", "affected"];
    const selected = view.selection?.kind === "inbox" ? view.selection.scope : this.scope;
    for (const scope of scopes) {
      const tab = this.button(scopeNames[scope], scope, scope === "queue" ? ShieldCheck : Inbox, () => this.selectScope(scope));
      tab.id = `${this.id}-tab-${scope}`; tab.setAttribute("role", "tab"); tab.setAttribute("aria-selected", String(selected === scope));
      tab.setAttribute("aria-controls", `${this.id}-panel`); tab.tabIndex = selected === scope ? 0 : -1;
      const context = this.context;
      tab.addEventListener("keydown", event => {
        if (context !== this.context || !tab.isConnected || !this.active) return;
        const index = scopes.indexOf(scope);
        const next = event.key === "Home" ? scopes[0] : event.key === "End" ? scopes.at(-1)
          : event.key === "ArrowRight" ? scopes[(index + 1) % scopes.length] : event.key === "ArrowLeft" ? scopes[(index + scopes.length - 1) % scopes.length] : null;
        if (next) { event.preventDefault(); this.selectScope(next); this.body.querySelector<HTMLButtonElement>(`[data-moderation-action="${next}"]`)?.focus(); }
      });
      tabs.append(tab);
    }
    const panel = element("section", "moderation-body"); panel.id = `${this.id}-panel`;
    panel.setAttribute("role", "tabpanel"); panel.setAttribute("aria-labelledby", `${this.id}-tab-${selected}`);
    panel.setAttribute("data-moderation-scope", selected);
    this.body.append(tabs, panel);
    const list = element("ul", "moderation-list"); list.setAttribute("data-moderation-list", ""); panel.append(list);
    if (!view.items.length && !view.loading && !view.error) list.append(element("li", "", selected === "mine" ? "No reports submitted." : selected === "affected" ? "No decisions on your posts." : "No reports in the reviewer queue."));
    for (const item of view.items) {
      const row = element("li", "moderation-row"); row.setAttribute("data-moderation-case", item.id);
      const badge = element("span", "moderation-badge", statusNames[item.status]); badge.setAttribute("data-outcome", item.decisions.at(-1)?.outcome ?? "pending");
      row.setAttribute("data-moderation-object", item.object_id);
      row.append(badge, element("h3", "", item.reason ? reasonNames[item.reason] : "Decision on your post"), this.identifier(item.object_id, "Post"),
        element("p", "moderation-meta", this.date(item.updated_at)), this.button("View report", "detail", ArrowUpRight, () => {
          ++this.context; void this.controller.show({ kind: "detail", id: item.id }); this.closeButton.focus();
        }, false, item.id));
      list.append(row);
    }
    if (view.nextBefore !== null) {
      const more = this.button("Load more", "load-more", Inbox, () => { void this.controller.refresh(true); }); more.disabled = view.loading;
      panel.append(more);
    }
  }
  private selectScope(scope: ModerationScope): void { ++this.context; this.scope = scope; void this.controller.show({ kind: "inbox", scope }); }
  private report(objectId: string): void {
    if (!moderationId(objectId, "obj")) { this.message("This Object ID is not valid.", true); return; }
    this.body.append(this.identifier(objectId, "Post"), element("p", "", "Your report is private to you and authorized node reviewers. Submitting a report does not restrict the post."));
    if (!this.controller.view.access) return;
    const draft = this.draft(`report:${objectId}`), form = this.form("report", () => {
      void this.controller.report(draft.reason as ModerationReason, draft.details);
    });
    this.reasonField(form, draft);
    this.textField(form, "Explanation", "details", draft, "details");
    this.submit(form, "Submit report", "submit-report", Flag, () => this.validReason(draft) && moderationText(draft.details.trim()));
    this.body.append(form);
  }
  private detail(): void {
    const item = this.controller.view.detail;
    this.body.append(this.button("Back to inbox", "back", ArrowLeft, () => this.selectScope(this.scope)));
    if (!item) return;
    const header = element("section", "moderation-body"); header.setAttribute("data-moderation-case", item.id); header.setAttribute("data-moderation-object", item.object_id);
    const badge = element("span", "moderation-badge", statusNames[item.status]); badge.setAttribute("data-outcome", item.decisions.at(-1)?.outcome ?? "pending");
    header.append(badge, this.identifier(item.object_id, "Post"), this.button("Open post", "open-object", ArrowUpRight, () => {
      const opener = this.opener; this.close(); this.options.openObject(item.object_id, opener);
    }), element("p", "moderation-meta", `Reported ${this.date(item.created_at)}. Revision ${item.revision}.`));
    this.body.append(header);
    const evidence = element("section", "moderation-section"); evidence.setAttribute("data-moderation-evidence", item.details === null ? "redacted" : "visible");
    evidence.append(element("h3", "", "Report evidence"));
    if (item.details === null) evidence.append(element("p", "", "Reporter identity and report evidence are private. Decisions on your post are shown below."));
    else evidence.append(element("p", "", reasonNames[item.reason!]), element("p", "moderation-evidence", item.details), this.identifier(item.reporter_id!, "Reporter"));
    this.body.append(evidence);
    for (const [index, decision] of item.decisions.entries()) {
      const section = element("section", "moderation-section"); section.setAttribute("data-moderation-decision", String(index + 1));
      const outcome = decision.outcome === "restrict" ? "Restrict on this node" : "No action for this case";
      section.append(element("h3", "", `${index ? "Final appeal decision" : "Decision"}: ${outcome}`), element("p", "", reasonNames[decision.reason]),
        element("p", "moderation-evidence", decision.explanation), this.identifier(decision.reviewer_id, "Reviewer"),
        element("p", "moderation-meta", `${decision.policy_version} · ${this.date(decision.created_at)}`));
      if (decision.source_signals.length) {
        section.append(element("h3", "", "Source Judgments"));
        const signals = element("ul"); for (const id of decision.source_signals) { const row = element("li"); row.append(this.identifier(id, "Judgment")); signals.append(row); } section.append(signals);
      }
      this.body.append(section);
    }
    if (item.appeal) {
      const appeal = element("section", "moderation-section"); appeal.setAttribute("data-moderation-appeal", "");
      appeal.append(element("h3", "", "Appeal"), element("p", "moderation-evidence", item.appeal.details ?? "Appeal evidence is private to the appellant and authorized reviewers."),
        element("p", "moderation-meta", this.date(item.appeal.created_at)));
      this.body.append(appeal);
    }
    const scope = element("details", "moderation-scope"); scope.append(element("summary", "", "Local restriction scope"), element("p", "moderation-meta", restriction));
    this.body.append(element("p", "", item.decisions.at(-1)?.outcome === "restrict" ? "This case restricts the post on this node." : "This case does not currently restrict the post."), scope);
    if (item.status === "appealed") this.body.append(element("p", "", "Any existing restriction remains while the appeal is reviewed."));
    if (reviewEligible(this.controller.view.access, item)) this.decisionForm(item);
    else if (this.controller.view.access?.can_review && (item.status === "pending" || item.status === "appealed")) this.body.append(element("p", "", "This case requires an independent reviewer. You cannot review your own report, your own post, or an appeal of your decision."));
    if (appealEligible(this.controller.owner, item)) this.appealForm(item);
  }
  private decisionForm(item: ModerationCase): void {
    const draft = this.draft(`decision:${item.id}`), form = this.form("decision", () => {
      void this.controller.decide({ outcome: draft.outcome as ModerationOutcome, reason: draft.reason as ModerationReason, explanation: draft.explanation, source_signals: this.signals(draft) });
    });
    form.classList.add("moderation-section"); form.append(element("h3", "", item.status === "appealed" ? "Resolve appeal" : "Review decision"),
      element("p", "", "Restrict removes this post from this node's discovery, Following, and conversations, hides quoted post previews, and stops Surface execution here. Direct public reads and signed history remain. No action clears only this case's restriction; other cases may still apply."));
    this.selectField(form, "Decision", "outcome", draft, [["", "Choose a decision"], ["no_action", "No action for this case"], ["restrict", "Restrict on this node"]]);
    this.reasonField(form, draft);
    this.textField(form, "Decision explanation", "explanation", draft, "explanation");
    this.textField(form, "Source Judgment IDs (optional)", "source_signals", draft, "source_signals", false);
    const policy = element("p", "moderation-meta", `Policy: ${this.controller.view.access!.policy_version}`); policy.setAttribute("data-moderation-field", "policy_version"); form.append(policy);
    this.submit(form, item.status === "appealed" ? "Record final decision" : "Record decision", "submit-decision", ShieldCheck,
      () => this.validReason(draft) && ["no_action", "restrict"].includes(draft.outcome) && moderationText(draft.explanation.trim()) && moderationSignals(this.signals(draft)));
    this.body.append(form);
  }
  private appealForm(item: ModerationCase): void {
    const draft = this.draft(`appeal:${item.id}`), form = this.form("appeal", () => { void this.controller.appeal(draft.details); });
    form.classList.add("moderation-section"); form.append(element("h3", "", "Appeal this decision"), element("p", "", "One appeal is available per case. A different reviewer will review it; an existing restriction stays in effect until the decision changes."));
    this.textField(form, "Appeal explanation", "details", draft, "details");
    this.submit(form, "Submit appeal", "submit-appeal", Flag, () => moderationText(draft.details.trim())); this.body.append(form);
  }
  private draft(key: string): Draft {
    let draft = this.drafts.get(key);
    if (!draft) { draft = { reason: "", details: "", outcome: "", explanation: "", source_signals: "" }; this.drafts.set(key, draft); }
    return draft;
  }
  private form(kind: string, handler: () => void): HTMLFormElement {
    const form = element("form", "moderation-form"); form.setAttribute("data-moderation-form", kind);
    const context = this.context;
    form.addEventListener("submit", event => {
      event.preventDefault(); if (context !== this.context || !this.active || !form.isConnected || this.controller.pending || this.controller.uncertain || this.controller.view.loading) return;
      if (form.querySelector<HTMLButtonElement>('button[type="submit"]')?.disabled) return;
      handler();
    });
    return form;
  }
  private reasonField(form: HTMLFormElement, draft: Draft): void {
    this.selectField(form, "Reason", "reason", draft, [["", "Choose a reason"], ...this.controller.view.access!.reasons.map(reason => [reason, reasonNames[reason]] as [string, string])]);
  }
  private selectField(form: HTMLFormElement, label: string, name: "reason" | "outcome", draft: Draft, choices: [string, string][]): void {
    const field = element("label", "moderation-field", label), input = element("select"); input.name = name; input.required = true;
    for (const [value, title] of choices) { const option = element("option", "", title); option.value = value; input.append(option); }
    input.value = draft[name]; this.input(input, name);
    input.addEventListener("change", () => { if (input.isConnected && this.active) draft[name] = input.value; });
    field.append(input); form.append(field);
  }
  private textField(form: HTMLFormElement, label: string, name: string, draft: Draft, key: "details" | "explanation" | "source_signals", prose = true): HTMLTextAreaElement {
    const field = element("label", "moderation-field", label), input = element("textarea"); input.name = name; input.value = draft[key]; input.rows = prose ? 5 : 3;
    input.maxLength = prose ? 8000 : 1400; input.required = prose; this.input(input, name);
    const count = element("span", "moderation-counter"); count.id = `${this.id}-${form.getAttribute("data-moderation-form")}-${name}-hint`; input.setAttribute("aria-describedby", count.id);
    const update = () => {
      if (input.isConnected && this.active) draft[key] = input.value;
      count.textContent = prose ? `${[...input.value.trim()].length} / 4,000 characters (minimum 20)` : "Up to 20 existing Judgment IDs for this post, separated by spaces or commas.";
      const invalid = input.value.trim().length > 0 && (prose ? !moderationText(input.value.trim()) : !moderationSignals(this.signals({ ...draft, source_signals: input.value })));
      if (!prose && invalid) count.textContent = "Enter at most 20 unique Judgment IDs beginning with jud_ followed by 64 lowercase hexadecimal characters.";
      count.setAttribute("data-error", String(invalid)); input.setAttribute("aria-invalid", String(invalid));
    };
    input.addEventListener("input", update); update(); field.append(input, count); form.append(field); return input;
  }
  private input(input: HTMLSelectElement | HTMLTextAreaElement, name: string): void {
    input.setAttribute("data-moderation-field", name); input.setAttribute("data-moderation-focus", `field:${name}`);
    input.disabled = this.controller.pending || this.controller.uncertain || this.controller.view.loading;
  }
  private submit(form: HTMLFormElement, label: string, action: string, icon: typeof X, valid: () => boolean): void {
    const button = this.button(label, action, icon, () => undefined); button.type = "submit"; button.setAttribute("type", "submit"); button.className = "moderation-primary";
    const update = () => { button.disabled = !valid() || this.controller.pending || this.controller.uncertain || this.controller.view.loading; };
    form.addEventListener("input", update); form.addEventListener("change", update); update(); form.append(button);
  }
  private validReason(draft: Draft): boolean { return !!this.controller.view.access?.reasons.includes(draft.reason as ModerationReason); }
  private signals(draft: Draft): string[] { return draft.source_signals.trim() ? draft.source_signals.trim().split(/[\s,]+/) : []; }
  private identifier(value: string, label: string): HTMLElement {
    const node = element("p", "moderation-meta", `${label}: ${value.slice(0, 12)}...${value.slice(-6)}`);
    node.title = value; node.setAttribute("aria-label", `${label}: ${value}`);
    if (moderationId(value, "obj")) node.setAttribute("data-moderation-object", value);
    return node;
  }
  private date(value: string): string { return new Date(value).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" }); }
  private button(label: string, action: string, icon: typeof X, handler: () => void, iconOnly = false, target = ""): HTMLButtonElement {
    const node = element("button"); node.type = "button"; node.title = label; node.setAttribute("aria-label", label);
    node.setAttribute("data-moderation-action", action); node.setAttribute("data-moderation-focus", `${target}:${action}`);
    if (target) node.setAttribute("data-moderation-case", target);
    node.append(createElement(icon, { "aria-hidden": "true", focusable: "false" })); if (!iconOnly) node.append(element("span", "", label));
    const context = this.context;
    node.addEventListener("click", () => { if (!this.disposed && this.dialog.open && node.isConnected && !node.disabled && (action === "close" || context === this.context)) handler(); });
    return node;
  }
  private message(text: string, error = false): void { this.status.textContent = text; this.status.setAttribute("data-error", String(error)); this.status.setAttribute("role", error ? "alert" : "status"); }
}
