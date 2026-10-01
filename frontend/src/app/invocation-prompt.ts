import { X, createElement } from "lucide";

export type InvocationMethod = `babel.social.${"follow" | "unfollow" | "share" | "reply"}.v2`;
export type InvocationDecision = "allow" | "deny" | "cancel";

export interface InvocationMedia {
  readonly id?: string;
  readonly title?: string;
  readonly mimeType?: string;
  readonly sizeBytes?: number;
  readonly digest?: string;
}

/** Supplied only after the host verifies the authoritative server summary. */
export interface InvocationSummary {
  readonly method: InvocationMethod;
  readonly actor: { readonly id: string; readonly label?: string };
  readonly requester: { readonly id: string; readonly title: string };
  readonly recipient: { readonly id: string; readonly title?: string };
  readonly text?: string;
  readonly media?: readonly InvocationMedia[];
  /** Absolute Unix milliseconds from the server; never refreshed by this view. */
  readonly deadlineEpochMs: number;
}

export interface InvocationPromptOptions {
  readonly target: HTMLElement;
  /** Must include the current account, Surface document and authorization context. */
  readonly authorized: () => boolean;
}

export interface InvocationPromptContext {
  readonly signal: AbortSignal;
}

const activeDocuments = new WeakSet<Document>();
const actions: Record<InvocationMethod, { heading: string; allow: string; effect: string }> = {
  "babel.social.follow.v2": { heading: "Follow Object?", allow: "Follow once", effect: "Add a follow from this identity to the recipient Object." },
  "babel.social.unfollow.v2": { heading: "Unfollow Object?", allow: "Unfollow once", effect: "Remove this identity's follow of the recipient Object." },
  "babel.social.share.v2": { heading: "Share Object?", allow: "Share once", effect: "Publish a share of the recipient Object as this identity." },
  "babel.social.reply.v2": { heading: "Reply to Object?", allow: "Reply once", effect: "Publish a reply to the recipient Object as this identity." },
};

/** A host-owned consent view. It neither authorizes nor sends an API decision. */
export class InvocationPrompt {
  readonly #target: HTMLElement;
  readonly #authorized: () => boolean;
  readonly #document: Document;
  #pending: (() => void) | null = null;
  #disposed = false;

  constructor(options: InvocationPromptOptions) {
    this.#target = options.target;
    this.#authorized = options.authorized;
    this.#document = options.target.ownerDocument;
  }

  /** Busy, expired, unsupported and stale contexts resolve cancel without queueing. */
  prompt(input: InvocationSummary, context: InvocationPromptContext): Promise<InvocationDecision> {
    const doc = this.#document;
    const win = doc.defaultView;
    const signal = context.signal;
    if (!win || signal.aborted || !this.#live() || activeDocuments.has(doc) || doc.querySelector("dialog[open]")) {
      return Promise.resolve("cancel");
    }
    let summary: InvocationSummary;
    try {
      summary = snapshot(input);
    } catch {
      return Promise.resolve("cancel");
    }
    const remaining = summary.deadlineEpochMs - Date.now();
    if (remaining <= 0) return Promise.resolve("cancel");
    const startedAt = win.performance.now();
    const expired = () => Date.now() >= summary.deadlineEpochMs || win.performance.now() - startedAt >= remaining;
    const dialog = doc.createElement("dialog");
    dialog.className = "invocation-prompt";
    dialog.setAttribute("data-invocation-prompt", "");
    dialog.setAttribute("data-invocation-deadline", String(summary.deadlineEpochMs));
    const action = actions[summary.method];
    dialog.setAttribute("aria-label", action.heading);
    const previous = doc.activeElement;
    const close = doc.createElement("button");
    close.type = "button";
    close.className = "invocation-prompt-close";
    close.setAttribute("aria-label", "Cancel request");
    close.setAttribute("data-invocation-cancel", "");
    close.title = "Cancel request";
    close.append(createElement(X, { "aria-hidden": "true" }));
    const header = doc.createElement("header");
    header.append(text(doc, "p", "Babel permission", "invocation-prompt-eyebrow"), text(doc, "h2", action.heading),
      text(doc, "p", action.effect, "invocation-prompt-effect"));
    const body = doc.createElement("div");
    body.className = "invocation-prompt-body";
    body.append(identity(doc, "actor", "Acting as", summary.actor.id, summary.actor.label),
      identity(doc, "requester", "Requesting Object", summary.requester.id, summary.requester.title),
      identity(doc, "recipient", "Recipient Object", summary.recipient.id, summary.recipient.title));
    if (summary.text !== undefined) {
      const section = doc.createElement("section");
      section.append(text(doc, "h3", "Exact text"));
      const content = text(doc, "pre", summary.text, "invocation-prompt-text");
      content.tabIndex = 0;
      content.setAttribute("aria-label", "Exact text to publish");
      content.setAttribute("data-invocation-text", "");
      section.append(content);
      if (summary.text === "") section.append(text(doc, "p", "Empty text", "invocation-prompt-note"));
      body.append(section);
    }
    if (summary.media?.length) {
      const section = doc.createElement("section");
      section.append(text(doc, "h3", `Attachments (${summary.media.length})`));
      const list = doc.createElement("ul");
      list.className = "invocation-prompt-media";
      list.setAttribute("data-invocation-media", "");
      summary.media.forEach((media, index) => {
        const item = doc.createElement("li");
        const details = doc.createElement("details");
        details.append(text(doc, "summary", media.title ?? `Attachment ${index + 1}`));
        const metadata = doc.createElement("dl");
        for (const [label, value] of [["ID", media.id], ["MIME type", media.mimeType],
          ["Size", media.sizeBytes === undefined ? undefined : `${media.sizeBytes} bytes`], ["Digest", media.digest]] as const) {
          if (value !== undefined) metadata.append(text(doc, "dt", label), text(doc, "dd", value));
        }
        details.append(metadata);
        item.append(details);
        list.append(item);
      });
      section.append(list);
      body.append(section);
    }
    const footer = doc.createElement("footer");
    footer.append(text(doc, "p", "Permission applies to this request only.", "invocation-prompt-note"));
    const buttons = doc.createElement("div");
    buttons.className = "invocation-prompt-buttons";
    const deny = text(doc, "button", "Deny");
    deny.type = "button";
    deny.autofocus = true;
    deny.setAttribute("data-invocation-deny", "");
    const allow = text(doc, "button", action.allow, "invocation-prompt-allow");
    allow.type = "button";
    allow.setAttribute("data-invocation-allow", "");
    buttons.append(deny, allow);
    footer.append(buttons);
    dialog.append(close, header, body, footer);

    return new Promise(resolve => {
      let settled = false;
      let watch: ReturnType<typeof setInterval> | undefined;
      let deadline: ReturnType<typeof setTimeout> | undefined;
      const observer = new MutationObserver(() => check());
      const removals: (() => void)[] = [];
      const listen = (target: EventTarget, event: string, listener: EventListener) => {
        target.addEventListener(event, listener);
        removals.push(() => target.removeEventListener(event, listener));
      };
      const valid = () => !signal.aborted && !expired() && this.#live() && dialog.open &&
        dialog.parentElement === this.#target && visible(dialog) &&
        !Array.from(doc.querySelectorAll("dialog[open]")).some(other => other !== dialog);
      const finish = (decision: InvocationDecision, restore = false) => {
        if (settled) return;
        const mayRestore = restore && valid() && dialog.contains(doc.activeElement);
        settled = true;
        clearInterval(watch);
        clearTimeout(deadline);
        observer.disconnect();
        removals.forEach(remove => remove());
        // close() may restore obsolete focus itself. Remove from the top layer first.
        dialog.removeAttribute("open");
        dialog.remove();
        this.#pending = null;
        activeDocuments.delete(doc);
        if (mayRestore && previous instanceof HTMLElement && visible(previous) && !previous.matches(":disabled") && this.#live()) {
          previous.focus({ preventScroll: true });
        }
        resolve(decision);
      };
      const cancel = () => finish("cancel");
      const check = () => { if (!valid()) cancel(); };
      const decide = (decision: InvocationDecision) => (event: Event) => {
        event.preventDefault();
        event.stopPropagation();
        finish(valid() ? decision : "cancel", true);
      };
      this.#pending = cancel;
      activeDocuments.add(doc);
      listen(signal, "abort", cancel);
      listen(doc, "visibilitychange", check);
      listen(win, "pagehide", cancel);
      listen(win, "accountchange", cancel);
      listen(doc, "accountchange", cancel);
      listen(dialog, "cancel", decide("cancel"));
      listen(dialog, "close", cancel);
      listen(close, "click", decide("cancel"));
      listen(deny, "click", decide("deny"));
      listen(allow, "click", decide("allow"));
      for (const event of ["keydown", "keyup", "keypress", "pointerdown", "pointermove", "pointerup", "pointercancel",
        "touchstart", "touchmove", "touchend", "click", "wheel"]) {
        listen(dialog, event, event => event.stopPropagation());
      }
      try {
        this.#target.append(dialog);
        dialog.showModal();
        deny.focus({ preventScroll: true });
        observer.observe(doc.documentElement, { childList: true, subtree: true, attributes: true,
          attributeFilter: ["inert", "hidden", "open", "style", "class", "aria-hidden"] });
        // The host's authorization callback has no subscription; also recheck on every decision.
        watch = setInterval(check, 150);
        deadline = setTimeout(check, Math.min(remaining, 2_147_483_647));
        check();
      } catch {
        cancel();
      }
    });
  }

  dispose(): void {
    this.#disposed = true;
    this.#pending?.();
  }

  #live(): boolean {
    try {
      return !this.#disposed && this.#document.visibilityState === "visible" && visible(this.#target) && this.#authorized();
    } catch {
      return false;
    }
  }
}

function visible(element: Element): boolean {
  if (!element.isConnected || element.closest('[inert], [hidden], [aria-hidden="true"]')) return false;
  const win = element.ownerDocument.defaultView;
  if (!win) return false;
  for (let current: Element | null = element; current; current = current.parentElement) {
    const style = win.getComputedStyle(current);
    if (style.display === "none" || style.visibility === "hidden" || style.visibility === "collapse" ||
      style.contentVisibility === "hidden" || style.opacity === "0") return false;
  }
  return true;
}

function text<K extends keyof HTMLElementTagNameMap>(doc: Document, tag: K, value: string, className?: string): HTMLElementTagNameMap[K] {
  const element = doc.createElement(tag);
  element.textContent = value;
  if (className) element.className = className;
  return element;
}

function identity(doc: Document, kind: "actor" | "requester" | "recipient", heading: string, id: string, label?: string): HTMLElement {
  const section = doc.createElement("section");
  section.setAttribute(`data-invocation-${kind}`, "");
  section.append(text(doc, "h3", heading));
  if (label !== undefined) section.append(text(doc, "p", label, "invocation-prompt-label"));
  const value = text(doc, "code", id, "invocation-prompt-id");
  if (id.length > 48) {
    const details = doc.createElement("details");
    details.append(text(doc, "summary", "Full ID"), value);
    section.append(details);
  } else section.append(value);
  return section;
}

function snapshot(input: InvocationSummary): InvocationSummary {
  const result = Object.freeze({ ...input, actor: Object.freeze({ ...input.actor }),
    requester: Object.freeze({ ...input.requester }), recipient: Object.freeze({ ...input.recipient }),
    ...(input.media === undefined ? {} : { media: Object.freeze(input.media.map(media => Object.freeze({ ...media }))) }) });
  const optionalString = (value: unknown) => value === undefined || typeof value === "string";
  if (!Object.hasOwn(actions, result.method) || !Number.isFinite(result.deadlineEpochMs) ||
    ![result.actor.id, result.requester.id, result.recipient.id].every(id => typeof id === "string" && id.length > 0) ||
    typeof result.requester.title !== "string" || !optionalString(result.actor.label) || !optionalString(result.recipient.title) ||
    !optionalString(result.text) || result.media?.some(media =>
      ![media.id, media.title, media.mimeType, media.digest].every(optionalString) ||
      (media.sizeBytes !== undefined && (!Number.isSafeInteger(media.sizeBytes) || media.sizeBytes < 0)))) {
    throw new TypeError("Invalid invocation summary");
  }
  return result;
}
