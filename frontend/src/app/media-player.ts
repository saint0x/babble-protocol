import { createElement, Music2, RotateCw } from "lucide";
import type { CardMedia } from "./media-resource";

const sources = new WeakMap<HTMLMediaElement, string>();
let observing = false;
let visibility: IntersectionObserver | null = null;

export function pauseMedia(root: ParentNode): void {
  for (const player of root.querySelectorAll<HTMLMediaElement>("audio, video")) player.pause();
}

function available(player: HTMLMediaElement): boolean {
  return player.isConnected && !document.hidden && !player.closest('[hidden], [inert], details:not([open])');
}

function visit(node: Node, action: (player: HTMLMediaElement) => void): void {
  if (!(node instanceof Element)) return;
  if (node.matches("[data-babble-playback]")) action(node as HTMLMediaElement);
  node.querySelectorAll<HTMLMediaElement>("[data-babble-playback]").forEach(action);
}

function attach(player: HTMLMediaElement): void {
  const src = sources.get(player);
  if (src && !player.hasAttribute("src")) player.src = src;
  visibility?.observe(player);
}

function observePlayback(): void {
  if (observing) return;
  observing = true;
  visibility = new IntersectionObserver((entries) => {
    for (const entry of entries) if (!entry.isIntersecting) (entry.target as HTMLMediaElement).pause();
  });
  // Native media events do not bubble. Capturing also coordinates composer previews.
  document.addEventListener("play", (event) => {
    const player = event.target;
    if (!(player instanceof HTMLMediaElement)) return;
    if (!available(player)) { player.pause(); return; }
    for (const other of document.querySelectorAll<HTMLMediaElement>("audio, video")) {
      if (other !== player) other.pause();
    }
  }, true);
  document.addEventListener("visibilitychange", () => { if (document.hidden) pauseMedia(document); });
  window.addEventListener("pagehide", () => pauseMedia(document));
  new MutationObserver((records) => {
    for (const record of records) {
      for (const removed of record.removedNodes) visit(removed, (player) => {
        if (player.isConnected) return;
        player.pause();
        visibility?.unobserve(player);
        player.removeAttribute("src");
        player.load();
      });
      for (const added of record.addedNodes) visit(added, (player) => { if (player.isConnected) attach(player); });
    }
    for (const player of document.querySelectorAll<HTMLMediaElement>("[data-babble-playback]")) {
      if (!available(player)) player.pause();
    }
  }).observe(document.documentElement, {
    subtree: true, childList: true, attributes: true, attributeFilter: ["hidden", "inert", "open"],
  });
}

export function createMediaPlayer(media: CardMedia & { readonly title: string }): HTMLElement {
  if (!media.media || (media.mediaKind !== "audio" && media.mediaKind !== "video")) {
    throw new Error("Playback requires an audio or video resource");
  }
  observePlayback();
  const frame = document.createElement("figure");
  frame.className = "media-player";
  frame.dataset.kind = media.mediaKind;
  const player = document.createElement(media.mediaKind);
  player.controls = true;
  player.preload = "metadata";
  player.autoplay = false;
  player.dataset.babblePlayback = "true";
  player.setAttribute("aria-label", media.title);
  if (player instanceof HTMLVideoElement) player.playsInline = true;
  if (media.mediaKind === "audio") {
    const heading = document.createElement("figcaption");
    heading.className = "media-player-heading";
    const title = document.createElement("strong");
    title.textContent = media.title;
    heading.append(createElement(Music2, { "aria-hidden": "true" }), title);
    frame.append(heading);
  }
  const feedback = document.createElement("div");
  feedback.className = "media-player-feedback";
  const status = document.createElement("p");
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");
  const retry = document.createElement("button");
  retry.type = "button";
  retry.className = "media-player-retry";
  retry.title = "Retry media";
  retry.setAttribute("aria-label", "Retry media");
  retry.append(createElement(RotateCw, { "aria-hidden": "true" }));
  retry.hidden = true;
  const message = (text: string, failed = false) => {
    status.textContent = text;
    retry.hidden = !failed;
    feedback.hidden = !text;
    frame.dataset.state = failed ? "error" : text ? "loading" : "ready";
  };
  player.addEventListener("loadedmetadata", () => message(""));
  player.addEventListener("canplay", () => message(""));
  player.addEventListener("playing", () => message(""));
  player.addEventListener("waiting", () => message("Buffering..."));
  player.addEventListener("error", () => {
    if (!player.hasAttribute("src")) return;
    const code = player.error?.code;
    message(code === 3 || code === 4 ? "This browser cannot play this file, or the file is damaged."
      : "Media could not be loaded. Try again.", true);
  });
  retry.addEventListener("click", () => {
    message("Loading media...");
    player.load();
  });
  feedback.append(status, retry);
  frame.append(player, feedback);
  sources.set(player, media.media);
  message("Loading media...");
  attach(player);
  return frame;
}
