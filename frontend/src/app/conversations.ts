import type { FeedCard } from "./protocol";
import { captureReading, restoreReading, type ReadingPosition } from "./reading";
import { createMediaPlayer } from "./media-player";
import { createMediaGallery } from "./media-gallery";
import { Flag, createElement } from "lucide";

export interface ConversationPage {
  readonly replies: readonly FeedCard[];
  readonly nextCursor: string | null;
}

interface Thread {
  readonly id: string;
  replies: Map<string, FeedCard>;
  readonly cursors: Set<string>;
  nextCursor: string | null;
  loaded: boolean;
  dirty: boolean;
  generation: number;
  pending: Promise<void> | null;
  requestCursor: string | null;
  failed: boolean;
}

interface Panel {
  readonly root: string;
  readonly element: HTMLElement;
  readonly title: HTMLElement;
  readonly header: HTMLElement;
  readonly back: HTMLButtonElement;
  readonly parent: HTMLElement;
  readonly parents: Map<string, FeedCard>;
  readonly status: HTMLElement;
  readonly list: HTMLElement;
  readonly more: HTMLButtonElement;
  readonly path: string[];
  readonly rows: Map<string, { signature: string; element: HTMLElement }>;
  readonly scrollPositions: Map<string, ReadingPosition>;
  threadId: string;
}

const ROOT_CACHE_LIMIT = 32;
const THREAD_CACHE_LIMIT = 128;
const HISTORY_LIMIT = 32;

export class Conversations {
  private readonly panels = new Map<string, Panel>();
  private readonly threads = new Map<string, Thread>();
  private activeRoot: string | null = null;

  constructor(private readonly load: (objectId: string, cursor: string | null) => Promise<ConversationPage>) {}

  /** Discard account-filtered history while preserving mounted panel elements. */
  clear(): void {
    this.activeRoot = null;
    for (const thread of this.threads.values()) {
      ++thread.generation;
      thread.pending = null;
    }
    this.threads.clear();
    for (const panel of this.panels.values()) {
      for (const media of panel.element.querySelectorAll<HTMLMediaElement>("audio, video")) media.pause();
      panel.path.splice(0, panel.path.length, panel.root);
      panel.threadId = panel.root;
      panel.header.dataset.readingAnchor = `thread:${panel.root}`;
      panel.parents.clear();
      panel.scrollPositions.clear();
      panel.rows.clear();
      panel.list.replaceChildren();
      panel.parent.replaceChildren();
      panel.parent.hidden = true;
      this.render(panel, this.thread(panel.root));
    }
  }

  view(objectId: string): HTMLElement {
    const existing = this.panels.get(objectId);
    if (existing) {
      this.panels.delete(objectId);
      this.panels.set(objectId, existing);
      this.prune(objectId);
      return existing.element;
    }

    const element = node("section", "conversation");
    element.dataset.conversationRoot = objectId;
    element.setAttribute("aria-label", "Replies");
    const header = node("header", "conversation-header");
    header.dataset.readingAnchor = `thread:${objectId}`;
    const title = node("h3", "conversation-title", "Replies");
    title.tabIndex = -1;
    const back = control("Back");
    back.hidden = true;
    header.append(back, title);
    const parent = node("div", "thread-context");
    parent.hidden = true;
    const status = node("p", "conversation-status");
    status.setAttribute("role", "status");
    status.setAttribute("aria-live", "polite");
    const list = node("div", "conversation-list");
    const pagination = node("div", "conversation-pagination");
    const more = control("Load more");
    more.hidden = true;
    pagination.append(more);
    element.append(header, parent, status, list, pagination);
    const panel: Panel = {
      root: objectId, element, title, header, back, parent, parents: new Map(), status, list, more,
      path: [objectId], rows: new Map(), scrollPositions: new Map(), threadId: objectId,
    };
    back.addEventListener("click", () => {
      if (this.activeRoot !== panel.root || panel.path.length < 2) return;
      panel.path.pop();
      this.show(panel, panel.path[panel.path.length - 1]!);
    });
    more.addEventListener("click", () => {
      if (this.activeRoot !== panel.root) return;
      const thread = this.thread(panel.threadId);
      if (thread.pending) return;
      if (thread.failed) void this.request(thread, thread.requestCursor);
      else if (thread.nextCursor !== null) void this.request(thread, thread.nextCursor);
    });
    this.panels.set(objectId, panel);
    this.render(panel, this.thread(objectId));
    this.prune(objectId);
    return element;
  }

  /** The parent calls this only for the explicitly active, visible column. */
  activate(objectId: string): void {
    this.activeRoot = objectId;
    this.view(objectId);
    const panel = this.panels.get(objectId)!;
    this.ensure(this.thread(panel.threadId));
    this.prune();
  }

  /** Inactive threads are invalidated now and fetched when next opened. */
  async refresh(objectId: string, published?: FeedCard): Promise<void> {
    const thread = published ? this.thread(objectId) : this.threads.get(objectId);
    if (!thread) return;
    if (published) {
      thread.replies.set(published.id, published);
      thread.replies = chronological(thread.replies);
    }
    thread.generation += 1;
    thread.pending = null;
    thread.dirty = true;
    thread.failed = false;
    this.renderThread(thread);
    const active = this.activeRoot === null ? undefined : this.panels.get(this.activeRoot);
    if (active?.threadId === objectId) await this.request(thread, null);
    this.prune();
  }

  private thread(id: string): Thread {
    const thread = this.threads.get(id) ?? {
      id, replies: new Map(), cursors: new Set(), nextCursor: null,
      loaded: false, dirty: false, generation: 0, pending: null,
      requestCursor: null, failed: false,
    };
    this.threads.delete(id);
    this.threads.set(id, thread);
    return thread;
  }

  private ensure(thread: Thread): void {
    if ((!thread.loaded || thread.dirty) && !thread.failed && !thread.pending) {
      void this.request(thread, null);
    }
  }

  private show(panel: Panel, id: string): void {
    const column = panel.element.closest<HTMLElement>(".post-card");
    const previousScroll = column ? captureReading(column) : { anchor: null, offset: 0, top: 0 };
    panel.scrollPositions.delete(panel.threadId);
    panel.scrollPositions.set(panel.threadId, previousScroll);
    panel.threadId = id;
    panel.header.dataset.readingAnchor = `thread:${id}`;
    panel.rows.clear();
    panel.list.replaceChildren();
    panel.parent.replaceChildren();
    const parent = panel.parents.get(id);
    panel.parent.hidden = id === panel.root || !parent;
    if (parent && id !== panel.root) panel.parent.append(parentContext(parent));
    const thread = this.thread(id);
    this.render(panel, thread);
    if (column) {
      const saved = panel.scrollPositions.get(id);
      if (saved) restoreReading(column, saved);
      else column.scrollTop = Math.max(0, panel.element.offsetTop - 16);
    }
    panel.title.focus({ preventScroll: true });
    for (const key of panel.scrollPositions.keys()) {
      if (panel.scrollPositions.size <= HISTORY_LIMIT) break;
      if (key !== panel.root && key !== id) panel.scrollPositions.delete(key);
    }
    this.ensure(thread);
    this.prune();
  }

  private open(panel: Panel, card: FeedCard): void {
    if (this.activeRoot !== panel.root) return;
    const id = card.id;
    panel.parents.set(id, card);
    const previous = panel.path.indexOf(id);
    if (previous >= 0) panel.path.splice(previous + 1);
    else {
      // Keep the root reachable even after a long descent through replies.
      if (panel.path.length >= HISTORY_LIMIT) panel.path.splice(1, 1);
      panel.path.push(id);
    }
    for (const key of panel.parents.keys()) {
      if (!panel.path.includes(key)) panel.parents.delete(key);
    }
    this.show(panel, id);
  }

  private request(thread: Thread, cursor: string | null): Promise<void> {
    if (thread.pending) return thread.pending;
    const generation = ++thread.generation;
    thread.requestCursor = cursor;
    thread.failed = false;
    // Defer the loader so even synchronous throws follow the same error path.
    const pending = Promise.resolve().then(() => this.load(thread.id, cursor)).then((page) => {
      if (thread.generation !== generation) return;
      if (cursor !== null && page.nextCursor !== null
        && (page.nextCursor === cursor || thread.cursors.has(page.nextCursor))) {
        throw new Error("Reply pagination did not advance");
      }
      if (cursor === null) {
        // Keep already-read pages and their DOM anchors while refreshing the head.
        const replies = new Map(page.replies.map((reply) => [reply.id, reply]));
        for (const [id, reply] of thread.replies) if (!replies.has(id)) replies.set(id, reply);
        thread.replies = replies;
        thread.cursors.clear();
      } else {
        for (const reply of page.replies) thread.replies.set(reply.id, reply);
        thread.cursors.add(cursor);
      }
      thread.replies = chronological(thread.replies);
      thread.nextCursor = page.nextCursor;
      thread.loaded = true;
      thread.dirty = false;
    }).catch(() => {
      if (thread.generation === generation) thread.failed = true;
    }).finally(() => {
      if (thread.generation !== generation) return;
      thread.pending = null;
      this.renderThread(thread);
      this.prune();
    });
    thread.pending = pending;
    this.renderThread(thread);
    return pending;
  }

  private renderThread(thread: Thread): void {
    for (const panel of this.panels.values()) {
      if (panel.threadId === thread.id) this.render(panel, thread);
    }
  }

  private render(panel: Panel, thread: Thread): void {
    const column = panel.element.closest<HTMLElement>(".post-card");
    const reading = column ? captureReading(column) : null;
    panel.element.dataset.conversationThread = thread.id;
    panel.element.dataset.nested = String(panel.path.length > 1);
    panel.element.setAttribute("aria-busy", String(thread.pending !== null));
    panel.back.hidden = panel.path.length < 2;
    panel.title.textContent = thread.id === panel.root ? "Replies" : "Thread";
    panel.status.dataset.state = thread.pending ? "loading" : thread.failed ? "error" : "ready";
    panel.status.textContent = thread.pending
      ? (thread.requestCursor !== null ? "Loading more replies..." : thread.loaded ? "Refreshing replies..." : "Loading replies...")
      : thread.failed
        ? (thread.requestCursor === null ? "Could not load replies." : "Could not load more replies.")
        : thread.loaded && !thread.dirty
          ? (thread.replies.size === 0 ? "No replies yet." : thread.nextCursor === null ? "All replies loaded." : "")
          : "";
    panel.more.textContent = thread.failed ? "Retry" : "Load more";
    panel.more.hidden = !thread.failed && thread.nextCursor === null;
    panel.more.disabled = thread.pending !== null;

    // Reconcile in place: loading, errors and pagination never detach existing rows.
    let position = panel.list.firstElementChild;
    for (const card of thread.replies.values()) {
      const signature = JSON.stringify([card.author, card.createdAt, card.title, card.content, card.media, card.mediaKind, card.mediaItems, card.surfaces.length > 0]);
      let row = panel.rows.get(card.id);
      if (!row || row.signature !== signature) {
        const element = replyRow(card, () => this.open(panel, card));
        if (row) {
          if (position === row.element) position = element;
          row.element.replaceWith(element);
        }
        row = { signature, element };
        panel.rows.set(card.id, row);
      }
      if (row.element !== position) panel.list.insertBefore(row.element, position);
      position = row.element.nextElementSibling;
    }
    if (column && reading) restoreReading(column, reading);
  }

  private prune(keepRoot?: string): void {
    // Limits are soft for attached panels and the active root: never evict live UI.
    for (const [id, panel] of this.panels) {
      if (this.panels.size <= ROOT_CACHE_LIMIT) break;
      if (id !== this.activeRoot && id !== keepRoot && !panel.element.isConnected) this.panels.delete(id);
    }
    const retained = new Set([...this.panels.values()].flatMap((panel) => panel.path));
    for (const [id, thread] of this.threads) {
      if (this.threads.size <= THREAD_CACHE_LIMIT) break;
      if (!retained.has(id)) {
        thread.generation += 1;
        this.threads.delete(id);
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

function control(label: string): HTMLButtonElement {
  const button = node("button", "conversation-control", label);
  button.type = "button";
  return button;
}

function compactId(id: string): string {
  return id.length > 24 ? `${id.slice(0, 12)}...${id.slice(-8)}` : id;
}

function chronological(replies: Map<string, FeedCard>): Map<string, FeedCard> {
  return new Map([...replies].sort(([leftId, left], [rightId, right]) => {
    const time = new Date(left.createdAt).getTime() - new Date(right.createdAt).getTime();
    if (Number.isFinite(time) && time !== 0) return time;
    // Backend RFC3339 timestamps may have finer precision than JavaScript Date.
    if (time === 0) {
      const fraction = fractionalRemainder(left.createdAt) - fractionalRemainder(right.createdAt);
      if (fraction !== 0) return fraction;
    }
    return leftId < rightId ? -1 : leftId > rightId ? 1 : 0;
  }));
}

function fractionalRemainder(timestamp: string): number {
  const fraction = timestamp.match(/\.(\d+)(?:Z|[+-]\d{2}:\d{2})$/i)?.[1] ?? "";
  return Number(fraction.padEnd(9, "0").slice(3, 9));
}

function replyRow(card: FeedCard, open: () => void): HTMLElement {
  const article = node("article", "reply-row");
  article.dataset.objectId = card.id;
  article.dataset.readingAnchor = `reply:${card.id}`;
  const author = node("header", "reply-author");
  const identity = authorProfile(card.author);
  const timestamp = node("time", "");
  const date = new Date(card.createdAt);
  if (Number.isFinite(date.getTime())) {
    timestamp.dateTime = date.toISOString();
    timestamp.textContent = date.toLocaleString();
  } else {
    timestamp.textContent = "Unknown time";
  }
  author.append(identity, timestamp);
  article.append(author, node("p", "reply-content", card.content || card.title));
  if (card.media) {
    if (card.mediaItems.length > 1) article.append(createMediaGallery(card, true));
    else if (card.mediaKind === "audio" || card.mediaKind === "video") article.append(createMediaPlayer(card));
    else {
      const image = node("img", "reply-media");
      image.src = card.media;
      image.alt = card.title;
      image.loading = "lazy";
      image.decoding = "async";
      article.append(image);
    }
  }
  const actions = node("footer", "reply-actions");
  const reply = control("Reply");
  reply.dataset.action = "reply";
  const replies = control("View replies");
  replies.addEventListener("click", open);
  actions.append(reply, replies);
  if (card.surfaces.length > 0) {
    const surface = control("Surface");
    surface.dataset.action = "surface";
    actions.append(surface);
  }
  const report = control("");
  report.dataset.action = "report";
  report.className = "conversation-control reply-report";
  report.title = "Report reply";
  report.setAttribute("aria-label", "Report reply");
  report.append(createElement(Flag, { "aria-hidden": "true" }));
  actions.append(report);
  article.append(actions);
  return article;
}

function parentContext(card: FeedCard): HTMLElement {
  const details = node("details", "thread-parent");
  details.dataset.objectId = card.id;
  details.dataset.readingAnchor = `parent:${card.id}`;
  const summary = node("summary", "thread-parent-summary");
  summary.append(node("span", "thread-parent-author", `Reply by ${compactId(card.author)}`));
  summary.append(node("span", "thread-parent-preview", card.content || card.title));
  const content = node("div", "thread-parent-content");
  content.append(authorProfile(card.author));
  content.append(node("p", "reply-content", card.content || card.title));
  if (card.media) {
    if (card.mediaItems.length > 1) content.append(createMediaGallery(card, true));
    else if (card.mediaKind === "audio" || card.mediaKind === "video") content.append(createMediaPlayer(card));
    else {
      const image = node("img", "reply-media");
      image.src = card.media;
      image.alt = card.title;
      image.loading = "lazy";
      content.append(image);
    }
  }
  details.append(summary, content);
  return details;
}

function authorProfile(author: string): HTMLButtonElement {
  const identity = node("button", "author-profile-link", compactId(author));
  identity.type = "button";
  identity.dataset.profileAuthor = author;
  identity.title = author;
  identity.setAttribute("aria-label", `View public profile for ${author}`);
  return identity;
}
