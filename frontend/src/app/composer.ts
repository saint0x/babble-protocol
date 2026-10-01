import { AppWindow, ArrowLeft, ArrowRight, createElement, FileImage, Film, ImagePlus, Music2, X } from "lucide";
import type { Drafts } from "./drafts";
import { mediaKind, MAX_IMAGE_BYTES, MAX_PLAYBACK_BYTES } from "./media-kind";

type ComposerDraft = Drafts["current"];

const MAX_ALBUM_FILES = 12;
const MAX_ALBUM_BYTES = 64 * 1024 * 1024;

export function composerAlbumError(files: readonly File[]): string | null {
  if (files.length > MAX_ALBUM_FILES) return `Choose up to ${MAX_ALBUM_FILES} attachments per post`;
  if (new Set(files).size !== files.length) return "The same file is attached more than once. Remove the duplicate attachment";
  for (const [index, file] of files.entries()) {
    const error = composerMediaError(file);
    if (error) return files.length === 1 ? error : `Attachment ${index + 1}: ${error}`;
  }
  if (files.reduce((total, file) => total + file.size, 0) > MAX_ALBUM_BYTES) return "Attachments must total 64 MiB or smaller";
  return null;
}

export function composerMediaError(file: File | null): string | null {
  if (!file) return null;
  const kind = mediaKind(file.type);
  if (!kind) return "Choose a supported image, audio, or video file";
  const limit = kind === "image" ? MAX_IMAGE_BYTES : MAX_PLAYBACK_BYTES;
  if (file.size > limit) return `${kind === "image" ? "Image" : "Audio and video"} media must be ${limit / (1024 * 1024)} MiB or smaller`;
  if (!file.size) return "This media file is empty. Choose another file";
  return null;
}

export function composerFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KiB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MiB`;
}

/** Presentation only: Drafts remains the authority for content and publication. */
export class ComposerView {
  private readonly text: HTMLTextAreaElement;
  private readonly media: HTMLInputElement;
  private readonly preview: HTMLElement;
  private readonly image: HTMLImageElement;
  private readonly audio: HTMLAudioElement;
  private readonly video: HTMLVideoElement;
  private readonly previewStatus: HTMLElement;
  private readonly remove: HTMLButtonElement;
  private readonly album: HTMLElement;
  private readonly thumbnails: HTMLElement;
  private readonly ordinal: HTMLElement;
  private readonly albumStatus: HTMLElement;
  private readonly previous: HTMLButtonElement;
  private readonly next: HTMLButtonElement;
  private readonly removeSelected: HTMLButtonElement;
  private files: readonly File[] = [];
  private selectedIndex = 0;
  private pending = false;
  private readonly urls = new Map<File, string>();
  private thumbnailButtons: HTMLButtonElement[] = [];
  private file: File | null = null;
  private previewUrl: string | null = null;
  private opener: HTMLElement | null = null;
  private open = false;
  private draftKey: string | null = null;
  private readonly onVisibilityChange = (): void => {
    if (document.hidden) this.pausePlayback();
  };

  constructor(private readonly root: HTMLElement, private readonly onMediaChange?: (files: readonly File[]) => void) {
    this.text = this.element<HTMLTextAreaElement>("[data-compose-text]");
    this.media = this.element<HTMLInputElement>("[data-compose-media]");
    this.preview = this.element("[data-compose-preview]");
    this.image = this.element<HTMLImageElement>("[data-compose-preview-image]");
    this.audio = this.element<HTMLAudioElement>("[data-compose-preview-audio]");
    this.video = this.element<HTMLVideoElement>("[data-compose-preview-video]");
    this.previewStatus = this.element("[data-compose-preview-status]");
    this.remove = this.element<HTMLButtonElement>("[data-remove-compose-media]");
    this.album = document.createElement("div");
    this.album.className = "composer-album";
    this.album.setAttribute("data-compose-album", "");
    this.album.hidden = true;
    this.thumbnails = document.createElement("div");
    this.thumbnails.className = "composer-album-strip";
    this.thumbnails.setAttribute("role", "group");
    this.thumbnails.setAttribute("aria-label", "Attachments in publication order");
    this.ordinal = document.createElement("span");
    this.ordinal.className = "composer-album-ordinal";
    this.ordinal.setAttribute("data-compose-album-ordinal", "");
    this.ordinal.setAttribute("data-compose-media-position", "");
    this.ordinal.setAttribute("aria-live", "polite");
    const actions = document.createElement("div");
    actions.className = "composer-album-actions";
    const action = (name: string, label: string, icon: typeof X, run: () => void): HTMLButtonElement => {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "composer-icon";
      button.setAttribute(`data-compose-album-${name}`, "");
      button.setAttribute("aria-label", label);
      button.title = label;
      const tooltip = document.createElement("span");
      tooltip.className = "composer-tooltip";
      tooltip.textContent = label;
      tooltip.setAttribute("aria-hidden", "true");
      button.append(createElement(icon, { "aria-hidden": "true" }), tooltip);
      button.addEventListener("click", () => { if (!button.disabled && !this.pending) run(); });
      return button;
    };
    this.previous = action("left", "Move attachment left", ArrowLeft, () => this.moveSelected(-1));
    this.next = action("right", "Move attachment right", ArrowRight, () => this.moveSelected(1));
    this.removeSelected = action("remove", "Remove selected attachment", X, () => {
      this.onMediaChange?.(Object.freeze(this.files.filter((_, index) => index !== this.selectedIndex)));
      (this.thumbnailButtons[this.selectedIndex] ?? this.media).focus({ preventScroll: true });
    });
    this.previous.setAttribute("data-compose-media-earlier", "");
    this.next.setAttribute("data-compose-media-later", "");
    this.removeSelected.setAttribute("data-compose-media-remove", "");
    actions.append(this.ordinal, this.previous, this.next, this.removeSelected);
    this.albumStatus = document.createElement("p");
    this.albumStatus.className = "composer-album-status";
    this.albumStatus.setAttribute("role", "status");
    this.albumStatus.setAttribute("data-compose-album-status", "");
    this.album.append(this.thumbnails, actions, this.albumStatus);
    this.preview.append(this.album);
    for (const [selector, icon] of [
      ["[data-compose-image-icon]", ImagePlus], ["[data-compose-bundle-icon]", AppWindow],
      ["[data-close-composer]", X], ["[data-remove-compose-media]", X],
    ] as const) this.element(selector).replaceChildren(createElement(icon, { "aria-hidden": "true" }));
    this.image.addEventListener("load", () => {
      if (!this.previewUrl || this.preview.dataset.kind !== "image") return;
      this.preview.dataset.state = "ready";
      this.previewStatus.textContent = "";
    });
    this.image.addEventListener("error", () => {
      if (!this.previewUrl || this.preview.dataset.kind !== "image") return;
      this.preview.dataset.state = "error";
      this.previewStatus.textContent = "This image could not be previewed. Choose another image if it is damaged.";
    });
    for (const [kind, player] of [["audio", this.audio], ["video", this.video]] as const) {
      player.controls = true;
      player.autoplay = false;
      player.preload = "metadata";
      const current = (): boolean => !!this.previewUrl && this.preview.dataset.kind === kind && player.getAttribute("src") === this.previewUrl;
      player.addEventListener("loadeddata", () => {
        if (!current() || player.readyState < 2 || player.error) return;
        this.preview.dataset.state = "ready";
        this.previewStatus.textContent = "";
      });
      player.addEventListener("loadedmetadata", () => {
        if (!current() || player.readyState < 1 || player.error) return;
        this.preview.dataset.state = "ready";
        this.previewStatus.textContent = "";
      });
      player.addEventListener("error", () => {
        if (!current() || !player.error) return;
        this.preview.dataset.state = "error";
        this.previewStatus.textContent = `This ${kind} could not be played. Its codec may be unsupported, or the file may be damaged. Your file is still attached.`;
      });
      player.addEventListener("play", () => {
        if (!current() || !this.open || document.hidden) player.pause();
      });
    }
    this.video.playsInline = true;
  }

  render(draft: ComposerDraft, handle: string | null): void {
    if (this.draftKey !== (draft?.key ?? null)) {
      this.dispose();
      this.selectedIndex = 0;
    }
    this.draftKey = draft?.key ?? null;
    const mode = draft?.target.mode ?? "publish";
    const pending = !!draft?.pending;
    this.pending = pending;
    this.root.dataset.mode = mode;
    this.root.dataset.pending = String(pending);
    this.element("[data-compose-title]").textContent = mode === "publish" ? "New post" : mode === "reply" ? "Write a reply" : "Share this post";
    this.element("[data-compose-avatar]").textContent = handle ? Array.from(handle)[0]!.toLocaleUpperCase() : "?";
    this.element("[data-compose-sign-in]").hidden = !!handle;
    const context = this.element("[data-compose-context]");
    const parent = draft?.target.parent;
    context.textContent = parent
      ? `${mode === "reply" ? "Replying to" : "Sharing"} ${parent.length > 22 ? `${parent.slice(0, 12)}...${parent.slice(-6)}` : parent}`
      : "Public post";
    context.title = parent ?? "";
    this.text.placeholder = mode === "publish" ? "What's on your mind?" : mode === "reply" ? "Write your reply..." : "Add your thoughts...";
    this.text.setAttribute("aria-label", mode === "publish" ? "Post text" : mode === "reply" ? "Reply text" : "Share text");
    this.element("[data-compose-attachments]").hidden = false;
    this.media.disabled = pending;
    this.text.disabled = pending;
    this.remove.disabled = pending;
    this.element("[data-compose-submit]").setAttribute("aria-busy", String(pending));
    this.renderAlbum(draft?.media ?? []);
    this.resizeText();
  }

  resizeText(): void {
    this.text.style.height = "auto";
    this.text.style.height = `${Math.max(176, Math.min(this.text.scrollHeight, 280))}px`;
  }

  visibility(open: boolean): void {
    if (open && !this.open && document.activeElement instanceof HTMLElement && !this.root.contains(document.activeElement)) {
      this.opener = document.activeElement;
    }
    this.open = open;
    if (open) this.resizeText();
    else {
      this.pausePlayback();
      if (this.root.contains(document.activeElement) && this.opener?.isConnected) this.opener.focus({ preventScroll: true });
    }
  }

  dispose(): void {
    this.releasePreview();
    for (const url of this.urls.values()) URL.revokeObjectURL(url);
    this.urls.clear();
    this.thumbnails.replaceChildren();
    this.thumbnailButtons = [];
    this.files = [];
    this.file = null;
  }

  private pausePlayback(): void {
    this.audio.pause();
    this.video.pause();
  }

  private releasePreview(): void {
    this.previewUrl = null;
    document.removeEventListener("visibilitychange", this.onVisibilityChange);
    this.pausePlayback();
    for (const player of [this.audio, this.video]) {
      const hadSource = player.hasAttribute("src");
      player.removeAttribute("src");
      if (hadSource) player.load();
      player.hidden = true;
    }
    this.image.removeAttribute("src");
    this.image.hidden = true;
  }

  private moveSelected(direction: -1 | 1): void {
    const index = this.selectedIndex;
    const destination = index + direction;
    if (destination < 0 || destination >= this.files.length) return;
    const files = [...this.files];
    [files[index], files[destination]] = [files[destination]!, files[index]!];
    this.onMediaChange?.(Object.freeze(files));
    const button = direction === -1 ? this.previous : this.next;
    (button.disabled ? this.thumbnailButtons[this.selectedIndex] : button)?.focus({ preventScroll: true });
  }

  private objectUrl(file: File): string {
    let url = this.urls.get(file);
    if (!url) { url = URL.createObjectURL(file); this.urls.set(file, url); }
    return url;
  }

  private renderAlbum(files: readonly File[]): void {
    const changed = files.length !== this.files.length || files.some((file, index) => file !== this.files[index]);
    if (changed) {
      const selected = this.files[this.selectedIndex];
      const retained = selected ? files.indexOf(selected) : -1;
      this.selectedIndex = retained >= 0 ? retained : Math.min(this.selectedIndex, Math.max(0, files.length - 1));
      this.files = Object.freeze([...files]);
      this.thumbnails.replaceChildren();
      this.thumbnailButtons = files.map((file, index) => {
        const button = document.createElement("button");
        button.type = "button";
        button.className = "composer-album-item";
        button.setAttribute("data-compose-album-index", String(index));
        button.setAttribute("data-compose-media-index", String(index));
        button.setAttribute("aria-label", `Attachment ${index + 1}: ${file.name}`);
        button.title = file.name;
        const visual = document.createElement("span");
        visual.className = "composer-album-thumbnail";
        const kind = mediaKind(file.type);
        visual.append(createElement(kind === "audio" ? Music2 : kind === "video" ? Film : FileImage, { "aria-hidden": "true" }));
        if (kind === "image" && !composerMediaError(file)) {
          try {
            const image = document.createElement("img");
            image.alt = "";
            image.src = this.objectUrl(file);
            image.addEventListener("error", () => { image.hidden = true; });
            visual.append(image);
          } catch {
            // The filename and media icon remain usable if preview allocation fails.
          }
        }
        const name = document.createElement("span");
        name.className = "composer-album-name";
        name.textContent = `${index + 1}. ${file.name}`;
        button.append(visual, name);
        button.addEventListener("click", () => {
          if (this.pending || button.disabled) return;
          this.selectAttachment(index);
        });
        button.addEventListener("keydown", (event) => {
          if (this.pending || event.altKey || event.ctrlKey || event.metaKey) return;
          const destination = event.key === "ArrowLeft" ? Math.max(0, index - 1)
            : event.key === "ArrowRight" ? Math.min(this.files.length - 1, index + 1)
            : event.key === "Home" ? 0 : event.key === "End" ? this.files.length - 1 : null;
          if (destination === null) return;
          event.preventDefault();
          this.selectAttachment(destination);
        });
        this.thumbnails.append(button);
        return button;
      });
    }
    this.renderMedia(files[this.selectedIndex] ?? null);
    for (const [file, url] of this.urls) {
      if (!files.includes(file)) { URL.revokeObjectURL(url); this.urls.delete(file); }
    }
    this.album.hidden = files.length <= 1;
    this.ordinal.textContent = files.length ? `${this.selectedIndex + 1} / ${files.length}` : "";
    this.previous.disabled = this.pending || !this.onMediaChange || this.selectedIndex === 0;
    this.next.disabled = this.pending || !this.onMediaChange || this.selectedIndex >= files.length - 1;
    this.removeSelected.disabled = this.pending || !this.onMediaChange || !files.length;
    this.thumbnailButtons.forEach((button, index) => {
      button.disabled = this.pending;
      button.tabIndex = index === this.selectedIndex ? 0 : -1;
      button.setAttribute("aria-pressed", String(index === this.selectedIndex));
    });
    if (changed && this.open) this.scrollSelectionIntoView();
    const invalid = composerAlbumError(files);
    this.albumStatus.textContent = invalid ?? "";
    if (invalid) this.media.setAttribute("aria-invalid", "true");
    else this.media.removeAttribute("aria-invalid");
  }

  private selectAttachment(index: number): void {
    this.selectedIndex = index;
    this.renderAlbum(this.files);
    this.thumbnailButtons[index]?.focus({ preventScroll: true });
    this.scrollSelectionIntoView();
  }

  private scrollSelectionIntoView(): void {
    this.thumbnailButtons[this.selectedIndex]?.scrollIntoView({ block: "nearest", inline: "nearest", behavior: "instant" });
  }

  private renderMedia(file: File | null): void {
    this.preview.hidden = !file;
    if (file === this.file) return;
    this.releasePreview();
    this.file = file;
    this.media.removeAttribute("aria-invalid");
    this.previewStatus.textContent = "";
    this.element("[data-compose-media-name]").textContent = file?.name ?? "";
    this.element("[data-compose-media-size]").textContent = file ? composerFileSize(file.size) : "";
    this.preview.dataset.kind = file ? mediaKind(file.type) ?? "unknown" : "";
    if (!file) return;
    this.image.alt = `Preview of ${file.name}`;
    const invalid = composerMediaError(file);
    if (invalid) {
      this.media.setAttribute("aria-invalid", "true");
      this.preview.dataset.state = "error";
      this.previewStatus.textContent = invalid;
      return;
    }
    this.preview.dataset.state = "loading";
    this.previewStatus.textContent = "Loading preview...";
    try {
      this.previewUrl = this.objectUrl(file);
      document.addEventListener("visibilitychange", this.onVisibilityChange);
      const kind = mediaKind(file.type);
      if (kind === "image") {
        this.image.hidden = false;
        this.image.src = this.previewUrl;
      } else {
        const player = kind === "audio" ? this.audio : this.video;
        player.hidden = false;
        player.setAttribute("aria-label", `Preview of ${file.name}`);
        if (!player.canPlayType(file.type)) {
          this.preview.dataset.state = "unsupported";
          this.previewStatus.textContent = `This browser may not support this ${kind} format. Your file is still attached.`;
        }
        player.src = this.previewUrl;
        player.load();
      }
    } catch {
      this.releasePreview();
      this.preview.dataset.state = "error";
      this.previewStatus.textContent = "Preview unavailable. Your media is still attached.";
    }
  }

  private element<T extends HTMLElement = HTMLElement>(selector: string): T {
    const element = this.root.querySelector<T>(selector);
    if (!element) throw new Error(`Composer is missing ${selector}`);
    return element;
  }
}
