import { createElement, ThumbsUp, ThumbsDown, ChevronDown, RotateCw } from "lucide";
import type { Accounts, AccountSession } from "./accounts";
import { emptyReaction, ReactionClient, Reactions, type ReactionValue, type ReactionViewState } from "./reactions";
type ReactionDraft = { -readonly [Key in keyof ReactionValue]: ReactionValue[Key] };

export interface ReactionActions {
  publish(value: ReactionValue): void;
  confirm(): void;
  cancel(): void;
  retry(): void;
  refresh(): void;
  signIn(): void;
}

let sequence = 0;
const countFormat = new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 });
const node = <K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] => {
  const element = document.createElement(tag);
  element.className = className;
  element.textContent = text;
  return element;
};
const button = (text: string, action: () => void) => {
  const element = node("button", "reaction-button", text);
  element.type = "button";
  element.addEventListener("click", action);
  return element;
};
const options = (values: readonly (readonly [string, string])[]): HTMLSelectElement => {
  const select = node("select");
  for (const [value, label] of values) {
    const option = node("option", "", label);
    option.value = value;
    select.append(option);
  }
  return select;
};

/** Stable nodes preserve disclosure, slider and keyboard focus during readbacks. */
export class ReactionView {
  readonly element = node("section", "reactions");
  private readonly likeCount = node("span", "reaction-count");
  private readonly dislikeCount = node("span", "reaction-count");
  private readonly like: HTMLButtonElement;
  private readonly dislike: HTMLButtonElement;
  private readonly details = node("details", "reaction-details");
  private readonly totals = node("p", "reaction-totals");
  private readonly form = node("form", "reaction-form");
  private readonly fields = node("fieldset", "reaction-fields");
  private readonly engagement = options([["", "No choice"], ["engaging", "Engaging"], ["not_engaging", "Not engaging"]]);
  private readonly stance = options([["", "No position"], ["support", "Support"], ["oppose", "Oppose"], ["uncertain", "Uncertain"]]);
  private readonly confidence = node("input");
  private readonly confidenceToggle = node("input");
  private readonly output = node("output", "reaction-confidence-output");
  private readonly publishButton = node("button", "reaction-button reaction-publish", "Publish choices");
  private readonly clear: HTMLButtonElement;
  private readonly signIn: HTMLButtonElement;
  private readonly refresh: HTMLButtonElement;
  private readonly retry: HTMLButtonElement;
  private readonly status = node("p", "reaction-status");
  private readonly notice = node("div", "reaction-notice");
  private readonly noticeChoice = node("p", "reaction-choice");
  private readonly confirm: HTMLButtonElement;
  private readonly cancel: HTMLButtonElement;
  private view: ReactionViewState | null = null;
  private draft: ReactionDraft = emptyReaction();

  constructor(private readonly actions: ReactionActions) {
    const prefix = `reaction-${++sequence}`;
    this.element.setAttribute("aria-label", "Public reactions");
    // Local controls must not become deck swipe or arrow-key navigation events.
    this.element.addEventListener("pointerdown", (event) => event.stopPropagation());
    this.element.addEventListener("keydown", (event) => event.stopPropagation());
    const bar = node("div", "reaction-bar");
    this.like = this.appreciation("like", this.likeCount);
    this.dislike = this.appreciation("dislike", this.dislikeCount);
    const summary = node("summary", "reaction-disclosure", "More reactions");
    summary.append(createElement(ChevronDown, { "aria-hidden": "true" }));
    this.details.append(summary, this.totals, this.form);
    bar.append(this.like, this.dislike);
    this.signIn = button("Sign in to react", actions.signIn);
    bar.append(this.signIn);
    this.element.append(bar, this.details);

    const legend = node("legend", "reaction-sr-only", "Your public choices");
    this.fields.append(legend, this.label("Engagement", this.engagement, `${prefix}-engagement`),
      this.label("Position", this.stance, `${prefix}-stance`));
    this.confidenceToggle.type = "checkbox";
    this.confidenceToggle.id = `${prefix}-include-confidence`;
    const include = node("label", "reaction-include-confidence");
    include.htmlFor = this.confidenceToggle.id;
    include.append(this.confidenceToggle, document.createTextNode("Include my confidence"));
    this.confidence.type = "range";
    this.confidence.min = "0";
    this.confidence.max = "100";
    this.confidence.step = "1";
    const confidenceField = this.label("Confidence in my position", this.confidence, `${prefix}-confidence`);
    this.output.htmlFor = this.confidence.id;
    confidenceField.append(this.output);
    this.fields.append(include, confidenceField);
    this.publishButton.type = "submit";
    this.clear = button("Withdraw all", () => actions.publish(emptyReaction()));
    const commands = node("div", "reaction-commands");
    commands.append(this.publishButton, this.clear);
    this.fields.append(commands);
    this.form.append(this.fields);
    this.form.addEventListener("submit", (event) => { event.preventDefault(); this.actions.publish({ ...this.draft }); });
    this.engagement.addEventListener("change", () => {
      this.draft.engagement = this.engagement.value === "engaging" ? "engaging" : this.engagement.value === "not_engaging" ? "not_engaging" : null;
    });
    this.stance.addEventListener("change", () => {
      this.draft.stance = this.stance.value === "support" ? "support" : this.stance.value === "oppose" ? "oppose" : this.stance.value === "uncertain" ? "uncertain" : null;
      if (this.draft.stance === null) this.draft.certainty = null;
      this.syncConfidence();
    });
    this.confidenceToggle.addEventListener("change", () => {
      // An explicit opt-in is required; the range's visual midpoint is not data.
      this.draft.certainty = this.confidenceToggle.checked && this.draft.stance !== null ? Number(this.confidence.value) : null;
      this.syncConfidence();
    });
    this.confidence.addEventListener("input", () => {
      if (this.draft.stance !== null && this.confidenceToggle.checked) this.draft.certainty = Number(this.confidence.value);
      this.syncConfidence();
    });

    this.status.setAttribute("role", "status");
    this.status.setAttribute("aria-live", "polite");
    this.status.setAttribute("aria-atomic", "true");
    this.refresh = button("Refresh", actions.refresh);
    this.refresh.prepend(createElement(RotateCw, { "aria-hidden": "true" }));
    this.retry = button("Retry same change", actions.retry);
    this.notice.id = `${prefix}-privacy`;
    this.notice.setAttribute("role", "group");
    this.notice.setAttribute("aria-label", "Confirm public reaction");
    const privacy = node("p", "", "Your reactions and confidence are signed, public, and linked to your identity. Withdrawing removes your current choices but does not erase signed history.");
    this.confirm = button("Publish publicly", actions.confirm);
    this.cancel = button("Cancel", actions.cancel);
    this.notice.append(privacy, this.noticeChoice, this.confirm, this.cancel);
    this.element.append(this.notice, this.status, this.retry, this.refresh);
  }

  render(view: ReactionViewState): void {
    const previous = this.view;
    const changed = previous?.generation !== view.generation;
    const newState = previous?.state?.revision !== view.state?.revision
      || JSON.stringify(previous?.state?.value) !== JSON.stringify(view.state?.value);
    this.view = view;
    this.element.hidden = !view.object;
    this.element.setAttribute("aria-busy", String(view.busy));
    if (changed) this.details.open = false;
    if (changed || newState) {
      this.draft = { ...(view.state?.value ?? emptyReaction()) };
      this.engagement.value = this.draft.engagement ?? "";
      this.stance.value = this.draft.stance ?? "";
      this.syncConfidence();
    }
    const locked = view.busy || !!view.confirmation || view.retry || !view.state;
    this.fields.disabled = locked;
    for (const [control, kind, count, total] of [
      [this.like, "like", this.likeCount, view.summary?.likes],
      [this.dislike, "dislike", this.dislikeCount, view.summary?.dislikes],
    ] as const) {
      control.disabled = view.actor !== null && locked;
      const label = kind === "like" ? "Like" : "Dislike";
      control.setAttribute("aria-pressed", String(view.state?.value.appreciation === kind));
      control.setAttribute("aria-label", `${label}${total === undefined ? ": count unavailable" : `: ${total}`}`);
      count.textContent = total === undefined ? "" : countFormat.format(total);
    }
    this.form.hidden = !view.actor;
    this.signIn.hidden = !!view.actor;
    this.clear.disabled = !view.state || Object.values(view.state.value).every((value) => value === null);
    this.refresh.hidden = !view.needsRefresh;
    this.refresh.disabled = view.busy;
    this.retry.hidden = !view.retry;
    this.retry.disabled = view.busy;
    this.notice.hidden = !view.confirmation;
    this.confirm.disabled = view.busy;
    this.cancel.disabled = view.busy;
    this.status.textContent = view.message;
    const aggregate = view.summary;
    this.totals.textContent = aggregate
      ? `${aggregate.participants} participants · ${aggregate.engaging} engaging · ${aggregate.not_engaging} not engaging · ${aggregate.support} support · ${aggregate.oppose} oppose · ${aggregate.uncertain} uncertain · ${aggregate.certainty_responses} shared confidence`
      : "Reaction totals unavailable.";
    this.noticeChoice.textContent = view.confirmation ? describeReaction(view.confirmation) : "";
    if (view.confirmation && !previous?.confirmation) this.confirm.focus();
    else if (!view.confirmation && previous?.confirmation) this.like.focus();
  }

  private appreciation(kind: "like" | "dislike", count: HTMLElement): HTMLButtonElement {
    const control = button("", () => {
      if (!this.view?.actor) { this.actions.signIn(); return; }
      if (!this.view.state) return;
      this.actions.publish({ ...this.view.state.value, appreciation: this.view.state.value.appreciation === kind ? null : kind });
    });
    control.title = kind === "like" ? "Like" : "Dislike";
    control.classList.add("reaction-appreciation");
    control.append(createElement(kind === "like" ? ThumbsUp : ThumbsDown, { "aria-hidden": "true" }), count);
    return control;
  }

  private label(text: string, input: HTMLInputElement | HTMLSelectElement, id: string): HTMLLabelElement {
    const label = node("label", "reaction-field");
    input.id = id;
    label.htmlFor = id;
    label.append(node("span", "", text), input);
    return label;
  }

  private syncConfidence(): void {
    const included = this.draft.certainty !== null;
    this.confidenceToggle.checked = included;
    this.confidenceToggle.disabled = this.draft.stance === null;
    this.confidence.disabled = !included || this.draft.stance === null;
    this.confidence.value = String(this.draft.certainty ?? 50);
    this.output.value = included ? `${this.draft.certainty}%` : "Not shared";
  }
}

function describeReaction(value: ReactionValue): string {
  const choices = [value.appreciation, value.engagement?.replaceAll("_", " "), value.stance,
    value.certainty === null ? null : `${value.certainty}% confidence`].filter(Boolean);
  return choices.length ? `Publish: ${choices.join(", ")}.` : "Withdraw all current choices.";
}

/** Move this single panel to the active card; inactive cards do not fetch. */
export class ReactionPanel {
  readonly view: ReactionView;
  readonly controller: Reactions;
  private readonly reservedHeights = new WeakMap<HTMLElement, string>();
  constructor(private readonly accounts: Accounts, options: { signIn: () => void; publicFetch?: typeof fetch; timeoutMs?: number }) {
    this.view = new ReactionView({
      publish: (value) => { void this.controller.publish(value); },
      confirm: () => { void this.controller.confirm(); },
      cancel: () => this.controller.cancelConfirmation(),
      retry: () => { void this.controller.retry(); },
      refresh: () => { void this.controller.refresh(); },
      signIn: options.signIn,
    });
    this.controller = new Reactions(accounts, (state) => this.view.render(state), new ReactionClient(accounts, options.publicFetch, options.timeoutMs));
    this.view.render(this.controller.view);
  }

  select(object: string | null, container?: HTMLElement, session: AccountSession | null = this.accounts.current): Promise<void> {
    if (object && !container) throw new Error("An active reaction panel requires its card container.");
    if (container && object && this.view.element.parentElement !== container) {
      const active = document.activeElement;
      const focused = active instanceof HTMLElement && this.view.element.contains(active) ? active : null;
      const previous = this.view.element.parentElement;
      if (previous) {
        // Keep an inactive card's geometry so removing the shared controls cannot
        // clamp its scroll position before the reader swipes back.
        this.reservedHeights.set(previous, previous.style.minHeight);
        previous.style.minHeight = `${previous.offsetHeight}px`;
      }
      container.append(this.view.element);
      const reserved = this.reservedHeights.get(container);
      if (reserved !== undefined) {
        container.style.minHeight = reserved;
        this.reservedHeights.delete(container);
      }
      focused?.focus({ preventScroll: true });
    }
    else if (!object) this.view.element.remove();
    return this.controller.select(object, session);
  }

  dispose(): void { this.controller.dispose(); this.view.element.remove(); }
}
