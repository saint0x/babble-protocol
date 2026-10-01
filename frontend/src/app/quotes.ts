import { ArrowUpRight, ChevronDown, createElement, RotateCw } from "lucide";
import type { FeedCard } from "./protocol";
import { captureReading, restoreReading } from "./reading";

export interface QuotePage {
  readonly objectId: string;
  readonly items: readonly { readonly targetId: string; readonly card: FeedCard | null }[];
  readonly nextCursor: string | null;
}

interface Panel {
  readonly id: string;
  readonly element: HTMLElement;
  readonly title: HTMLElement;
  readonly status: HTMLElement;
  readonly list: HTMLElement;
  readonly more: HTMLButtonElement;
  readonly pages: Map<string | null, QuotePage>;
  readonly rows: Map<string, { card: FeedCard | null; cursor: string | null; element: HTMLElement }>;
  generation: number;
  controller: AbortController | null;
  failed: boolean;
  dirty: boolean;
  requestCursor: string | null;
  nextCursor: string | null;
}

const CACHE_LIMIT = 32;

export class Quotes {
  private readonly panels = new Map<string, Panel>();
  private activeId: string | null = null;

  constructor(
    private readonly load: (objectId: string, cursor: string | null, signal: AbortSignal) => Promise<QuotePage>,
    private readonly open: (card: FeedCard, opener: HTMLElement) => void,
  ) {}

  view(objectId: string): HTMLElement {
    const existing = this.panels.get(objectId);
    if (existing) {
      this.panels.delete(objectId);
      this.panels.set(objectId, existing);
      return existing.element;
    }
    const element = node("section", "quotes");
    element.dataset.quotesRoot = objectId;
    element.setAttribute("aria-label", "Shared posts");
    const title = node("h3", "quotes-title", "Shared post");
    title.dataset.readingAnchor = `quotes:${objectId}`;
    const status = node("p", "quotes-status");
    status.setAttribute("role", "status");
    status.setAttribute("aria-live", "polite");
    const list = node("div", "quotes-list");
    const more = control("Load more", ChevronDown);
    more.className += " quotes-more";
    element.append(title, status, list, more);
    const panel: Panel = {
      id: objectId, element, title, status, list, more, pages: new Map(), rows: new Map(),
      generation: 0, controller: null, failed: false, dirty: false, requestCursor: null, nextCursor: null,
    };
    more.addEventListener("click", () => {
      if (panel.failed) this.request(panel, panel.requestCursor);
      else if (panel.nextCursor !== null) this.request(panel, panel.nextCursor);
    });
    this.panels.set(objectId, panel);
    this.render(panel);
    this.prune(objectId);
    return element;
  }

  activate(objectId: string): void {
    if (this.activeId !== objectId) {
      const previous = this.activeId === null ? undefined : this.panels.get(this.activeId);
      if (previous) {
        this.cancel(previous);
        this.render(previous);
      }
      this.activeId = objectId;
    }
    this.view(objectId);
    const panel = this.panels.get(objectId)!;
    if ((panel.pages.size === 0 || panel.dirty) && !panel.failed) this.request(panel, null);
  }

  clear(): void {
    this.activeId = null;
    for (const panel of this.panels.values()) this.retire(panel);
    this.panels.clear();
  }

  /** Inactive panels are refreshed only when they next become active. */
  refresh(objectId: string): void {
    const panel = this.panels.get(objectId);
    if (!panel) return;
    this.cancel(panel);
    panel.dirty = true;
    panel.failed = false;
    this.render(panel);
    if (this.activeId === objectId) this.request(panel, null);
  }

  private owns(panel: Panel): boolean {
    return this.activeId === panel.id && this.panels.get(panel.id) === panel;
  }

  private request(panel: Panel, cursor: string | null): void {
    if (!this.owns(panel) || panel.controller) return;
    if (panel.dirty) cursor = null;
    const controller = new AbortController();
    const generation = ++panel.generation;
    panel.controller = controller;
    panel.requestCursor = cursor;
    panel.failed = false;
    const resetHead = cursor === null && (panel.dirty || !panel.pages.has(null));
    const current = () => this.owns(panel) && panel.generation === generation && !controller.signal.aborted;
    this.render(panel);
    // Defer synchronous loaders too, and do not start one after navigation/clear.
    void Promise.resolve().then(() => current() ? this.load(panel.id, cursor, controller.signal) : null).then((page) => {
      if (!current()) return;
      if (page === null) throw new Error("Missing shared-post page");
      this.validate(panel, cursor, page, resetHead);
      if (resetHead) panel.pages.clear();
      panel.pages.set(cursor, page);
      panel.nextCursor = [...panel.pages.values()].at(-1)!.nextCursor;
      panel.dirty = false;
    }).catch(() => {
      if (current()) panel.failed = true;
    }).finally(() => {
      if (!current()) return;
      panel.controller = null;
      this.render(panel);
    });
  }

  private validate(panel: Panel, cursor: string | null, page: QuotePage, resetHead: boolean): void {
    if (page.objectId !== panel.id || !Array.isArray(page.items)
      || (page.nextCursor !== null && (typeof page.nextCursor !== "string" || !page.nextCursor))) {
      throw new Error("Invalid shared-post page");
    }
    for (const item of page.items) {
      if (!item || typeof item.targetId !== "string" || !item.targetId
        || (item.card !== null && (!item.card || item.card.id !== item.targetId))) {
        throw new Error("Shared-post target mismatch");
      }
    }
    // A retry may return the same successor, but must never loop to a prior page.
    const previous = panel.pages.get(cursor);
    if (!resetHead && page.nextCursor !== null && (page.nextCursor === cursor
      || (panel.pages.has(page.nextCursor) && previous?.nextCursor !== page.nextCursor))) {
      throw new Error("Shared-post pagination did not advance");
    }
    if (!resetHead && previous && previous.nextCursor !== page.nextCursor) {
      throw new Error("Shared-post pagination changed; refresh required");
    }
  }

  private render(panel: Panel): void {
    const column = panel.element.closest<HTMLElement>(".post-card");
    const reading = column ? captureReading(column) : null;
    const pending = panel.controller !== null;
    const items = new Map<string, { card: FeedCard | null; cursor: string | null }>();
    for (const [cursor, page] of panel.pages) {
      for (const item of page.items) {
        if (!items.has(item.targetId) || (items.get(item.targetId)!.card === null && item.card !== null)) {
          items.set(item.targetId, { card: item.card, cursor });
        }
      }
    }
    panel.element.hidden = items.size === 0 && !pending && !panel.failed && panel.nextCursor === null;
    panel.element.setAttribute("aria-busy", String(pending));
    panel.title.hidden = items.size === 0;
    panel.title.textContent = items.size > 1 ? "Shared posts" : "Shared post";
    panel.status.dataset.state = pending ? "loading" : panel.failed ? "error" : "ready";
    panel.status.textContent = pending ? "Loading shared posts..." : panel.failed ? "Could not load shared posts." : "";
    panel.status.hidden = !panel.status.textContent;
    panel.more.hidden = !panel.failed && panel.nextCursor === null;
    panel.more.disabled = pending;
    labelControl(panel.more, panel.failed ? "Retry" : "Load more", panel.failed ? RotateCw : ChevronDown);

    for (const [id, row] of panel.rows) {
      if (!items.has(id)) { row.element.remove(); panel.rows.delete(id); }
    }
    let position = panel.list.firstElementChild;
    for (const [id, item] of items) {
      let row = panel.rows.get(id);
      if (!row || row.card !== item.card || row.cursor !== item.cursor) {
        const element = this.preview(panel, id, item.card, item.cursor);
        if (row) {
          if (position === row.element) position = element;
          row.element.replaceWith(element);
        }
        row = { ...item, element };
        panel.rows.set(id, row);
      }
      for (const button of row.element.querySelectorAll<HTMLButtonElement>("[data-quote-retry]")) button.disabled = pending;
      if (row.element !== position) panel.list.insertBefore(row.element, position);
      position = row.element.nextElementSibling;
    }
    if (column && reading) restoreReading(column, reading);
  }

  private preview(panel: Panel, targetId: string, card: FeedCard | null, cursor: string | null): HTMLElement {
    const article = node("article", "quote-preview");
    article.dataset.quotedObject = targetId;
    article.dataset.readingAnchor = `quote:${panel.id}:${targetId}`;
    if (!card) {
      article.dataset.kind = "unavailable";
      article.append(node("p", "quote-title", "Shared post unavailable"));
      article.append(node("p", "quote-snippet", "This post could not be retrieved."));
      const retry = control("Retry", RotateCw);
      retry.dataset.quoteRetry = "";
      retry.addEventListener("click", () => {
        if (panel.rows.get(targetId)?.element === article) this.request(panel, cursor);
      });
      article.append(retry);
      return article;
    }
    const kind = card.mediaKind ?? (card.surfaces.length > 0 ? "surface" : "text");
    article.dataset.kind = kind;
    const header = node("header", "quote-meta");
    const author = node("button", "author-profile-link quote-author", compactId(card.author));
    author.type = "button";
    author.dataset.profileAuthor = card.author;
    author.title = card.author;
    author.setAttribute("aria-label", `View public profile for ${card.author}`);
    header.append(author, node("span", "quote-kind", kind === "surface" ? "Interactive post" : `${kind[0]!.toUpperCase()}${kind.slice(1)} post`));
    article.append(header);
    const body = node("div", "quote-body");
    if (kind === "image" && card.media) {
      const image = node("img", "quote-thumbnail");
      image.src = card.media;
      image.alt = "";
      image.loading = "lazy";
      image.decoding = "async";
      image.referrerPolicy = "no-referrer";
      image.width = 88;
      image.height = 88;
      image.addEventListener("error", () => {
        const column = article.closest<HTMLElement>(".post-card");
        const reading = column ? captureReading(column) : null;
        image.hidden = true;
        if (column && reading) restoreReading(column, reading);
      });
      body.append(image);
    }
    const copy = node("div", "quote-copy");
    if (card.title) copy.append(node("p", "quote-title", snippet(card.title, 160)));
    if (card.content && card.content !== card.title) copy.append(node("p", "quote-snippet", snippet(card.content, 360)));
    body.append(copy);
    article.append(body);
    const open = control("Open post", ArrowUpRight);
    open.dataset.quoteTarget = targetId;
    open.setAttribute("aria-label", "Open post");
    open.addEventListener("click", () => {
      if (this.owns(panel) && panel.rows.get(targetId)?.element === article) this.open(card, open);
    });
    article.append(open);
    return article;
  }

  private cancel(panel: Panel): void {
    panel.generation += 1;
    panel.controller?.abort();
    panel.controller = null;
  }

  private retire(panel: Panel): void {
    this.cancel(panel);
    panel.pages.clear();
    panel.rows.clear();
    panel.element.replaceChildren();
    panel.element.hidden = true;
    panel.element.setAttribute("aria-busy", "false");
  }

  private prune(keepId: string): void {
    for (const [id, panel] of this.panels) {
      if (this.panels.size <= CACHE_LIMIT) break;
      if (id !== this.activeId && id !== keepId) {
        this.retire(panel);
        this.panels.delete(id);
      }
    }
  }
}

function node<K extends keyof HTMLElementTagNameMap>(tag: K, className: string, text?: string): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  element.className = className;
  if (text !== undefined) element.textContent = text;
  return element;
}

function control(label: string, icon: typeof ArrowUpRight): HTMLButtonElement {
  const button = node("button", "quote-control");
  button.type = "button";
  labelControl(button, label, icon);
  return button;
}

function labelControl(button: HTMLButtonElement, label: string, icon: typeof ArrowUpRight): void {
  button.replaceChildren(createElement(icon, { "aria-hidden": "true" }), node("span", "", label));
}

function compactId(id: string): string {
  return id.length > 24 ? `${id.slice(0, 12)}...${id.slice(-8)}` : id;
}

function snippet(text: string, limit: number): string {
  return text.length > limit ? `${text.slice(0, limit).replace(/[\uD800-\uDBFF]$/, "")}...` : text;
}
