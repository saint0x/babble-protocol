import { ChevronLeft, ChevronRight, createElement, Music2, RotateCw, Video } from "lucide";
import { openImageViewer } from "./image-viewer";
import { createMediaPlayer } from "./media-player";
import type { MediaCollection, MediaItem } from "./media-resource";

type GalleryMedia = MediaCollection & { readonly title: string; readonly content: string };

export function createMediaGallery(media: GalleryMedia, compact = false): HTMLElement {
  const items = media.mediaItems;
  if (items.length < 2) throw new Error("A gallery requires multiple media items");
  let index = Math.max(0, items.findIndex(item => item.media === media.media && item.mediaKind === media.mediaKind));
  const gallery = document.createElement("section");
  gallery.className = "media-gallery";
  gallery.dataset.mediaGallery = compact ? "compact" : "full";
  gallery.setAttribute("role", "region");
  gallery.setAttribute("aria-roledescription", "carousel");
  gallery.setAttribute("aria-label", `Attachments: ${media.title}`);
  const stage = document.createElement("div");
  stage.className = "media-gallery-stage";
  stage.setAttribute("role", "group");
  stage.setAttribute("aria-roledescription", "slide");
  const controls = document.createElement("div");
  controls.className = "media-gallery-controls";
  const previous = iconButton("Previous attachment", ChevronLeft);
  previous.dataset.mediaPrevious = "";
  const next = iconButton("Next attachment", ChevronRight);
  next.dataset.mediaNext = "";
  const position = document.createElement("output");
  position.dataset.mediaPosition = "";
  position.setAttribute("aria-live", "polite");
  position.setAttribute("aria-atomic", "true");
  const strip = document.createElement("div");
  strip.className = "media-gallery-strip";
  strip.setAttribute("role", "group");
  strip.setAttribute("aria-label", "Choose attachment");
  const selectors = items.map((item, itemIndex) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "media-gallery-item";
    button.dataset.mediaIndex = String(itemIndex);
    const label = `${kindLabel(item)} ${itemIndex + 1} of ${items.length}`;
    button.title = label;
    button.setAttribute("aria-label", label);
    if (item.mediaKind === "image") {
      const thumbnail = document.createElement("img");
      thumbnail.alt = "";
      thumbnail.loading = "lazy";
      thumbnail.decoding = "async";
      thumbnail.referrerPolicy = "no-referrer";
      thumbnail.draggable = false;
      thumbnail.src = item.media;
      button.append(thumbnail);
    } else button.append(createElement(item.mediaKind === "audio" ? Music2 : Video, { "aria-hidden": "true" }));
    const number = document.createElement("span");
    number.textContent = String(itemIndex + 1);
    button.append(number);
    button.addEventListener("click", () => select(itemIndex));
    strip.append(button);
    return button;
  });

  function select(nextIndex: number): void {
    if (nextIndex === index && stage.firstElementChild) return;
    // Release the outgoing decoder immediately; the shared observer handles detach/reconnect.
    for (const player of stage.querySelectorAll<HTMLMediaElement>("audio, video")) {
      player.pause();
      player.removeAttribute("src");
      player.load();
    }
    for (const image of stage.querySelectorAll<HTMLImageElement>("img")) image.removeAttribute("src");
    index = nextIndex;
    const item = items[index]!;
    gallery.dataset.mediaSelected = String(index);
    stage.dataset.kind = item.mediaKind;
    stage.setAttribute("aria-label", `${kindLabel(item)} ${index + 1} of ${items.length}`);
    stage.replaceChildren(item.mediaKind === "image"
      ? galleryImage(item, media, index + 1)
      : createMediaPlayer({ ...item, title: `${media.title} (${index + 1} of ${items.length})` }));
    position.textContent = `${index + 1} / ${items.length}`;
    position.setAttribute("aria-label", `Attachment ${index + 1} of ${items.length}`);
    selectors.forEach((button, itemIndex) => {
      button.setAttribute("aria-pressed", String(itemIndex === index));
      button.tabIndex = itemIndex === index ? 0 : -1;
    });
    revealSelection();
  }
  function revealSelection(): void {
    const selected = selectors[index]!;
    const left = selected.offsetLeft;
    if (left < strip.scrollLeft) strip.scrollLeft = left;
    else if (left + selected.offsetWidth > strip.scrollLeft + strip.clientWidth) {
      strip.scrollLeft = left + selected.offsetWidth - strip.clientWidth;
    }
  }
  const move = (delta: number) => select((index + delta + items.length) % items.length);
  previous.addEventListener("click", () => move(-1));
  next.addEventListener("click", () => move(1));
  // Only explicit album controls consume navigation. Image drags remain post swipes.
  for (const group of [controls, strip]) {
    group.addEventListener("pointerdown", event => event.stopPropagation());
    group.addEventListener("click", event => event.stopPropagation());
    group.addEventListener("keydown", event => {
      if (event.defaultPrevented || event.isComposing || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
      if (!["ArrowLeft", "ArrowRight", "Home", "End"].includes(event.key)) return;
      event.preventDefault();
      event.stopPropagation();
      if (event.key === "Home") select(0);
      else if (event.key === "End") select(items.length - 1);
      else move(event.key === "ArrowLeft" ? -1 : 1);
      if (group === strip) selectors[index]!.focus({ preventScroll: true });
    });
  }
  controls.append(previous, position, next);
  gallery.append(stage, controls, strip);
  select(index);
  requestAnimationFrame(() => { if (gallery.isConnected) revealSelection(); });
  return gallery;
}

function kindLabel(item: MediaItem): string {
  return item.mediaKind === "image" ? "Image" : item.mediaKind === "audio" ? "Audio" : "Video";
}

function iconButton(label: string, icon: typeof ChevronLeft): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "media-gallery-control";
  button.title = label;
  button.setAttribute("aria-label", label);
  button.append(createElement(icon, { "aria-hidden": "true" }));
  return button;
}

function galleryImage(item: MediaItem, media: GalleryMedia, position: number): HTMLElement {
  const frame = document.createElement("div");
  frame.className = "media-gallery-image";
  const open = document.createElement("button");
  open.type = "button";
  open.className = "post-image-open";
  open.title = "View full image";
  open.setAttribute("aria-label", `View full image: ${media.title} (${position})`);
  open.setAttribute("aria-haspopup", "dialog");
  const image = document.createElement("img");
  image.alt = `${media.title} (${position})`;
  image.decoding = "async";
  image.referrerPolicy = "no-referrer";
  image.draggable = false;
  const feedback = document.createElement("div");
  feedback.className = "media-gallery-feedback";
  const status = document.createElement("p");
  status.setAttribute("role", "status");
  const retry = iconButton("Retry image", RotateCw);
  const loading = () => {
    frame.dataset.state = "loading";
    open.setAttribute("aria-busy", "true");
    status.textContent = "Loading image...";
    retry.hidden = true;
    feedback.hidden = false;
    image.hidden = true;
    image.src = item.media;
  };
  image.addEventListener("load", () => {
    frame.dataset.state = "ready";
    open.setAttribute("aria-busy", "false");
    feedback.hidden = true;
    image.hidden = false;
  });
  image.addEventListener("error", () => {
    if (!image.hasAttribute("src")) return;
    frame.dataset.state = "error";
    open.setAttribute("aria-busy", "false");
    image.hidden = true;
    status.textContent = "Image could not be loaded.";
    feedback.hidden = false;
    retry.hidden = false;
  });
  retry.addEventListener("click", event => { event.stopPropagation(); loading(); });
  open.addEventListener("click", event => {
    event.stopPropagation();
    openImageViewer({ src: item.media, alt: image.alt, caption: media.content }, open);
  });
  open.append(image);
  feedback.append(status, retry);
  frame.append(open, feedback);
  loading();
  return frame;
}
