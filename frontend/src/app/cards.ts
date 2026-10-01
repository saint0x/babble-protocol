import type { FeedCard } from "./protocol";
import type { Conversations } from "./conversations";
import type { Quotes } from "./quotes";
import { captureReading, restoreReading, type ReadingPosition } from "./reading";
import { openImageViewer } from "./image-viewer";
import { createMediaPlayer, pauseMedia } from "./media-player";
import { createMediaGallery } from "./media-gallery";
import { ChartNoAxesColumn, createElement, Ellipsis, MessageCircle, Share2 } from "lucide";

const EXIT_ANIMATION_MS = 240;
const offsets = [-2, -1, 0, 1, 2] as const;
const scrollPositions = new Map<string, ReadingPosition>();
const renderedCards = new WeakMap<HTMLElement, FeedCard>();

document.addEventListener("pointerdown", (event) => {
  for (const wrap of document.querySelectorAll<HTMLElement>(".action-popover")) {
    if (event.target instanceof Node && !wrap.contains(event.target)) {
      togglePopover(wrap, false);
    }
  }
});

document.addEventListener("keydown", (event) => {
  if (event.key !== "Escape") return;
  for (const wrap of document.querySelectorAll<HTMLElement>(".action-popover")) {
    const panel = wrap.querySelector<HTMLElement>(".popover-panel");
    if (panel && !panel.hidden) {
      togglePopover(wrap, false);
      if (wrap.contains(document.activeElement)) {
        wrap.querySelector<HTMLButtonElement>("button")?.focus();
      }
    }
  }
});

export function renderDeck(deck: HTMLElement, cards: readonly FeedCard[], currentIndex: number, conversations: Conversations, quotes: Quotes): void {
  const visible = visibleCards(cards, currentIndex);
  const nextIds = new Set(visible.map((item) => item.card.id));
  const existing = new Map(
    [...deck.querySelectorAll<HTMLElement>(".post-card")].map((card) => [card.dataset.objectId ?? "", card]),
  );

  for (const card of existing.values()) {
    const id = card.dataset.objectId;
    if (id && nextIds.has(id)) {
      continue;
    }
    retireCard(card);
  }

  for (const item of visible) {
    const current = existing.get(item.card.id);
    const element = current ?? renderCard(item.card, item.offset, conversations);
    const quoteView = quotes.view(item.card.id);
    if (quoteView.parentElement !== element) {
      element.querySelector("[data-quotes-root]")?.remove();
      element.insertBefore(quoteView, element.children[1] ?? null);
    }
    if (current && renderedCards.get(current) !== item.card) {
      refreshAnalytics(current, item.card);
    }
    renderedCards.set(element, item.card);
    delete element.dataset.exiting;
    applyCardPosition(element, item.offset);
    if (!current) {
      element.dataset.entering = "true";
      deck.append(element);
      const saved = scrollPositions.get(item.card.id);
      if (saved) restoreReading(element, saved);
      requestAnimationFrame(() => {
        delete element.dataset.entering;
      });
    }
  }
  const feedIds = new Set(cards.map((card) => card.id));
  for (const id of scrollPositions.keys()) {
    if (!feedIds.has(id)) scrollPositions.delete(id);
  }
}

export function visibleCards(
  cards: readonly FeedCard[],
  currentIndex: number,
): readonly { readonly card: FeedCard; readonly offset: number }[] {
  if (cards.length === 0) {
    return [];
  }
  const maxVisible = Math.min(cards.length, 5);
  const start = 2 - Math.floor(maxVisible / 2);
  return offsets.slice(start, start + maxVisible).map((offset) => ({
    card: cards[(currentIndex + offset + cards.length) % cards.length] as FeedCard,
    offset,
  }));
}

function renderCard(card: FeedCard, offset: number, conversations: Conversations): HTMLElement {
  const article = document.createElement("article");
  article.className = "post-card";
  article.dataset.objectId = card.id;
  article.tabIndex = 0;
  article.ariaLabel = `Post by ${compact(card.author)}`;
  article.addEventListener("scroll", () => scrollPositions.set(card.id, captureReading(article)), { passive: true });
  applyCardPosition(article, offset);

  const primary = document.createElement("div");
  primary.className = "post-primary";
  primary.dataset.contentKind = card.media ? card.mediaKind ?? "image" : card.surfaces.length ? "surface" : "text";
  article.dataset.contentKind = primary.dataset.contentKind;
  primary.dataset.readingAnchor = "content";
  const inner = document.createElement("div");
  inner.className = "post-card-inner";
  inner.dataset.contentKind = primary.dataset.contentKind;
  const reading = document.createElement("div");
  reading.className = "post-reading-area";
  if (card.media) {
    inner.append(media(card));
  } else {
    reading.append(content(card));
    inner.append(authorRow(card, offset === 0), reading);
  }
  if (card.surfaces.length > 0) {
    const open = protocolButton("Open Surface", "surface");
    open.className = "surface-open";
    inner.append(open);
  }
  const context = document.createElement("div");
  context.className = "post-context";
  context.dataset.readingAnchor = "context";
  if (card.media) context.append(authorRow(card, offset === 0));
  if (card.media && card.content) context.append(content(card));
  context.append(actionRow(card));
  primary.append(inner, context);
  const inspection = manifest(card);
  inspection.dataset.readingAnchor = "inspection";
  article.append(primary, conversations.view(card.id), inspection);
  return article;
}

function applyCardPosition(article: HTMLElement, offset: number): void {
  article.dataset.offset = String(offset);
  article.style.transform = cardTransform(offset);
  article.style.zIndex = String(20 - Math.abs(offset));
  article.style.opacity = offset === 0 ? "1" : String(0.96 - Math.abs(offset) * 0.08);
  article.setAttribute("aria-hidden", String(offset !== 0));
  article.inert = offset !== 0;
  article.tabIndex = offset === 0 ? 0 : -1;
  if (offset !== 0) {
    pauseMedia(article);
    for (const wrap of article.querySelectorAll<HTMLElement>(".action-popover")) {
      const panel = wrap.querySelector<HTMLElement>(".popover-panel");
      if (panel) {
        panel.hidden = true;
        delete panel.dataset.state;
      }
      wrap.querySelector("button")?.setAttribute("aria-expanded", "false");
    }
  }
}

function retireCard(article: HTMLElement): void {
  if (article.dataset.exiting === "true") {
    return;
  }
  const id = article.dataset.objectId;
  if (id) scrollPositions.set(id, captureReading(article));
  article.dataset.exiting = "true";
  pauseMedia(article);
  article.inert = true;
  article.tabIndex = -1;
  article.setAttribute("aria-hidden", "true");
  article.style.opacity = "0";
  window.setTimeout(() => {
    if (article.dataset.exiting === "true") {
      article.remove();
    }
  }, EXIT_ANIMATION_MS);
}

function authorRow(card: FeedCard, isCurrent: boolean): HTMLElement {
  const row = document.createElement("header");
  row.className = "post-author";

  const avatar = document.createElement("div");
  avatar.className = "post-avatar";
  avatar.textContent = authorInitial(card);

  const meta = document.createElement("div");
  meta.className = "post-author-meta";

  const line = document.createElement("div");
  const username = document.createElement("h2");
  const profile = document.createElement("button");
  profile.type = "button";
  profile.className = "author-profile-link";
  profile.dataset.profileAuthor = card.author;
  profile.textContent = compact(card.author);
  profile.title = `View public profile: ${card.author}`;
  profile.setAttribute("aria-label", `View public profile for ${card.author}`);
  username.append(profile);
  if (!isCurrent) {
    username.className = "dim";
  }
  const timestamp = document.createElement("time");
  timestamp.dateTime = card.createdAt;
  timestamp.textContent = relativeTime(card.createdAt);
  line.append(username, timestamp);

  meta.append(line);
  row.append(avatar, meta);
  return row;
}

function content(card: FeedCard): HTMLElement {
  const body = document.createElement("section");
  body.className = "post-content";

  const text = document.createElement("p");
  text.textContent = card.content || card.title;
  body.append(text);
  return body;
}

function media(card: FeedCard): HTMLElement {
  if (card.mediaItems.length > 1) return createMediaGallery(card);
  if (card.mediaKind === "audio" || card.mediaKind === "video") return createMediaPlayer(card);
  const frame = document.createElement("div");
  frame.className = "post-image";
  if (card.media) {
    const open = document.createElement("button");
    open.type = "button";
    open.className = "post-image-open";
    open.title = "View full image";
    open.setAttribute("aria-label", `View full image: ${card.title}`);
    open.setAttribute("aria-haspopup", "dialog");
    const image = document.createElement("img");
    image.src = card.media;
    image.alt = card.title;
    image.decoding = "async";
    image.referrerPolicy = "no-referrer";
    open.addEventListener("click", () => {
      if (card.media) openImageViewer({ src: card.media, alt: card.title, caption: card.content }, open);
    });
    open.append(image);
    frame.append(open);
  }
  return frame;
}

function actionRow(card: FeedCard): HTMLElement {
  const row = document.createElement("footer");
  row.className = "post-actions";

  const left = document.createElement("div");
  left.className = "post-action-cluster";
  const replies = protocolButton("Replies", "conversation");
  replies.className = "text-action";
  replies.prepend(createElement(MessageCircle, { "aria-hidden": "true" }));
  left.append(replies, analytics(card));

  const right = document.createElement("div");
  right.className = "post-action-cluster";
  right.append(socialActions(), protocolActions(card));

  row.append(left, right);
  return row;
}

function objectFacets(card: FeedCard): HTMLElement {
  const list = document.createElement("section");
  list.className = "post-facets";
  list.ariaLabel = "Object facets";
  list.append(
    facet("Kind", card.kind),
    facet("Surface", card.surfaces.length > 0 ? card.surfaces[0]?.target ?? "ready" : "none"),
    facet("Graph", relationText(card)),
  );
  if (card.resourceCount > 0) {
    list.append(facet("Media", String(card.resourceCount)));
  }
  return list;
}

function signalBars(card: FeedCard): HTMLElement {
  const signals = [
    ["Rel", card.signals.relevance],
    ["Evd", card.signals.evidence],
    ["Nov", card.signals.novelty],
  ] as const;
  const section = document.createElement("section");
  section.className = "post-signals";
  section.ariaLabel = "Discovery signals";
  for (const [label, value] of signals) {
    const item = document.createElement("div");
    const caption = document.createElement("span");
    caption.textContent = label;
    const track = document.createElement("i");
    track.style.setProperty("--value", value === null ? "0" : String(Math.round(value * 100)));
    const number = document.createElement("strong");
    number.textContent = percent(value);
    item.append(caption, track, number);
    section.append(item);
  }
  return section;
}

function facet(label: string, value: string): HTMLElement {
  const item = document.createElement("p");
  const name = document.createElement("span");
  name.textContent = label;
  const strong = document.createElement("strong");
  strong.textContent = value;
  item.append(name, strong);
  return item;
}

function analytics(card: FeedCard): HTMLElement {
  const wrap = popoverShell("analytics");
  const button = textButton("analytics", "chart");
  button.ariaLabel = "Show analytics";
  const panel = popover("analytics");
  panel.id = `analytics-${card.id}`;
  panel.setAttribute("role", "region");
  panel.setAttribute("aria-label", "Object analytics");
  panel.tabIndex = 0;
  button.setAttribute("aria-controls", panel.id);
  populateAnalytics(panel, card);
  button.addEventListener("click", () => togglePopover(wrap));
  wrap.append(button, panel);
  return wrap;
}

function populateAnalytics(panel: HTMLElement, card: FeedCard): void {
  panel.replaceChildren();
  panel.append(
    metric("Score", card.score === null ? "n/a" : String(Math.round(card.score * 100) / 100)),
    metric("Relevance", percent(card.signals.relevance)),
    metric("Novelty", percent(card.signals.novelty)),
    metric("Evidence", percent(card.signals.evidence)),
  );
  if (card.rankingProvider) panel.append(metric("Public model", card.rankingProvider.model));
  if (card.temporal) panel.append(temporalMetrics(card.temporal, false));
}

function temporalMetrics(temporal: NonNullable<FeedCard["temporal"]>, full: boolean): HTMLElement {
  const section = document.createElement("section");
  section.className = "temporal-metrics";
  section.setAttribute("aria-label", "Temporal heuristics from public graph activity");
  const { score, provider } = temporal;
  section.append(
    metric("Temporal heuristic", score.survival_score.toFixed(3)),
    metric("Public activity", score.engagement_velocity.toFixed(3)),
  );
  if (!full) return section;
  section.append(
    metric("Recency", score.recency.toFixed(3)),
    metric("Age (hours)", score.age_hours.toFixed(2)),
    metric("Decay rate", score.decay_rate.toFixed(3)),
    metric("Time sensitivity", score.time_sensitivity.toFixed(3)),
  );
  const time = document.createElement("time");
  time.dateTime = temporal.reference_time;
  time.textContent = temporal.reference_time;
  section.append(
    metric("Temporal provider", provider.provider),
    metric("Temporal model", provider.model),
    metric("Model version", provider.version),
    metric("Evaluated", time),
  );
  return section;
}

function refreshAnalytics(article: HTMLElement, card: FeedCard): void {
  const panel = article.querySelector<HTMLElement>('.popover-panel[data-kind="analytics"]');
  if (panel) populateAnalytics(panel, card);
  const inspection = article.querySelector<HTMLElement>(".post-inspection");
  const next = manifest(card).querySelector<HTMLElement>(".post-inspection");
  if (inspection && next) {
    const scrollTop = inspection.scrollTop;
    inspection.replaceChildren(...next.childNodes);
    inspection.scrollTop = scrollTop;
  }
}

function socialActions(): HTMLElement {
  const wrap = popoverShell("social");
  const button = textButton("Social", "social");
  const panel = popover("social");
  panel.append(
    protocolButton("Reply", "reply"),
    protocolButton("Share", "share"),
    protocolButton("Author controls", "author-controls"),
    protocolButton("Report post", "report"),
  );
  button.addEventListener("click", () => togglePopover(wrap));
  wrap.append(button, panel);
  return wrap;
}

function protocolActions(card: FeedCard): HTMLElement {
  const wrap = popoverShell("protocol");
  const button = textButton("Protocol", "protocol");
  const panel = popover("protocol");
  if (card.surfaces.length > 0) panel.append(protocolButton("Open Surface", "surface"));
  if (card.surfaces.length > 0 || card.object.capabilities.length > 0) {
    panel.append(protocolButton("Permissions", "permissions"));
  }
  panel.append(
    protocolButton("Judgments", "judgments"),
    protocolButton("Copy Object ID", "copy-id"),
    protocolButton("Hide author from feed", "hide-author"),
  );
  button.addEventListener("click", () => togglePopover(wrap));
  wrap.append(button, panel);
  return wrap;
}

function manifest(card: FeedCard): HTMLElement {
  const details = document.createElement("details");
  details.className = "post-manifest";
  const summary = document.createElement("summary");
  summary.textContent = "Inspect Object";
  const pre = document.createElement("pre");
  pre.textContent = JSON.stringify(
    {
      id: card.id,
      author: card.author,
      protocol: card.protocol,
      schema: card.schema,
      resources: card.object.resources,
      surfaces: card.object.surfaces,
      capabilities: card.object.capabilities,
      relations: card.object.relations,
      reasons: card.reasons,
      ranking_provider: card.rankingProvider,
      temporal: card.temporal,
      source: card.source,
      lineage: card.lineage,
    },
    null,
    2,
  );
  const inspection = document.createElement("div");
  inspection.className = "post-inspection";
  inspection.append(objectFacets(card), signalBars(card));
  if (card.temporal) inspection.append(temporalMetrics(card.temporal, true));
  inspection.append(pre);
  details.append(summary, inspection);
  return details;
}

function textButton(label: string, icon?: "chart" | "social" | "protocol"): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.ariaExpanded = "false";
  button.className = icon ? "text-action icon-action" : "text-action";
  button.title = icon === "chart" ? "Show analytics"
    : icon === "social" ? "Reply or share"
    : icon === "protocol" ? "Object actions" : label;
  button.ariaLabel = button.title;
  if (icon) {
    const symbol = icon === "chart" ? ChartNoAxesColumn : icon === "social" ? Share2 : Ellipsis;
    const caption = document.createElement("span");
    caption.className = "sr-only";
    caption.textContent = label;
    button.append(createElement(symbol, { "aria-hidden": "true" }), caption);
  } else button.textContent = label;
  return button;
}

function protocolButton(label: string, action: string): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.dataset.action = action;
  button.textContent = label;
  return button;
}

function popoverShell(kind: string): HTMLElement {
  const wrap = document.createElement("div");
  wrap.className = "action-popover";
  wrap.dataset.kind = kind;
  return wrap;
}

function popover(kind: string): HTMLElement {
  const panel = document.createElement("div");
  panel.className = "popover-panel";
  panel.dataset.kind = kind;
  panel.hidden = true;
  return panel;
}

function togglePopover(wrap: HTMLElement, force?: boolean): void {
  const panel = wrap.querySelector<HTMLElement>(".popover-panel");
  if (!panel) {
    return;
  }
  const button = wrap.querySelector("button");
  const nextOpen = force ?? button?.getAttribute("aria-expanded") !== "true";
  if (nextOpen) {
    const card = wrap.closest<HTMLElement>(".post-card");
    for (const other of card?.querySelectorAll<HTMLElement>(".popover-panel") ?? []) {
      if (other !== panel) {
        other.hidden = true;
        delete other.dataset.state;
        other.parentElement?.querySelector("button")?.setAttribute("aria-expanded", "false");
      }
    }
    button?.setAttribute("aria-expanded", "true");
    panel.hidden = false;
    requestAnimationFrame(() => {
      if (button?.getAttribute("aria-expanded") === "true") panel.dataset.state = "open";
    });
    return;
  }
  delete panel.dataset.state;
  button?.setAttribute("aria-expanded", "false");
  window.setTimeout(() => {
    if (button?.getAttribute("aria-expanded") !== "true") {
      panel.hidden = true;
    }
  }, 200);
}

function metric(label: string, value: string | HTMLElement): HTMLElement {
  const item = document.createElement("div");
  const name = document.createElement("span");
  name.textContent = label;
  const strong = document.createElement("strong");
  if (typeof value === "string") strong.textContent = value;
  else strong.append(value);
  item.append(name, strong);
  return item;
}

function cardTransform(offset: number): string {
  const abs = Math.abs(offset);
  const y = offset === 0 ? 0 : abs * 16;
  const scale = offset === 0 ? 1 : 0.94 - abs * 0.06;
  const rotateY = offset * 6;
  return `translate(calc(${offset} * var(--card-step) + ${offset} * var(--card-gap)), ${y}px) rotateY(${rotateY}deg) scale(${scale})`;
}

function authorDisplay(card: FeedCard): string {
  const value = card.author.replace(/^id_/, "").replace(/^identity_/, "");
  return value.length > 10 ? value.slice(0, 6) : value || "babble";
}

function authorInitial(card: FeedCard): string {
  return authorDisplay(card).slice(0, 1).toUpperCase() || "B";
}

function compact(value: string): string {
  if (value.length <= 14) {
    return value;
  }
  return `${value.slice(0, 6)}_${value.slice(-5)}`;
}

function percent(value: number | null): string {
  return value === null ? "n/a" : `${Math.round(value * 100)}%`;
}

function relationText(card: FeedCard): string {
  if (card.relations.length === 0 && card.lineage.length === 0) {
    return "clean";
  }
  return String(card.relations.reduce((total, relation) => total + relation.count, 0) + card.lineage.length);
}

function relativeTime(value: string): string {
  const date = new Date(value);
  const now = Date.now();
  if (Number.isNaN(date.valueOf())) {
    return value;
  }
  const seconds = Math.max(0, Math.round((now - date.valueOf()) / 1000));
  if (seconds < 90) {
    return "now";
  }
  const minutes = Math.round(seconds / 60);
  if (minutes < 90) {
    return `${minutes}m`;
  }
  const hours = Math.round(minutes / 60);
  if (hours < 36) {
    return `${hours}h`;
  }
  const days = Math.round(hours / 24);
  return `${days}d`;
}
