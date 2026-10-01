import { ProfileRequestError, type FeedCard, type ProfilePage, type PublicIdentity } from "./protocol";

interface ProfileSource {
  publicIdentity(id: string, signal: AbortSignal): Promise<PublicIdentity>;
  profileObjects(id: string, cursor: string | null, signal: AbortSignal): Promise<ProfilePage>;
}

/** One modal session owns its requests, focus and pagination. Closing, switching
 * authors or changing accounts invalidates every outstanding response. */
export class Profiles {
  private generation = 0;
  private controller: AbortController | null = null;
  private author: string | null = null;
  private opener: HTMLElement | null = null;
  private visible = false;
  private pending = false;
  private next: string | null = null;
  private requested: string | null = null;
  private failed = false;
  private restart = false;
  private identityLoaded = false;
  private scrollTop = 0;
  private readonly cursors = new Set<string>();
  private readonly objects = new Map<string, FeedCard>();
  private readonly title: HTMLElement;
  private readonly identity: HTMLElement;
  private readonly status: HTMLElement;
  private readonly list: HTMLElement;
  private readonly more: HTMLButtonElement;
  private readonly closeButton: HTMLButtonElement;

  constructor(private readonly dialog: HTMLDialogElement, private readonly source: ProfileSource,
    private readonly select: (card: FeedCard, cards: readonly FeedCard[]) => void,
    private readonly closed: () => void,
    private readonly targetChanged: (id: string | null) => void = () => undefined) {
    const find = <T extends HTMLElement>(selector: string): T => {
      const element = dialog.querySelector<T>(selector);
      if (!element) throw new Error(`Missing profile element: ${selector}`);
      return element;
    };
    this.title = find("[data-public-profile-title]");
    this.identity = find("[data-public-profile-identity]");
    this.status = find("[data-public-profile-status]");
    this.list = find("[data-public-profile-list]");
    this.more = find("[data-public-profile-more]");
    this.closeButton = find("[data-public-profile-close]");
    this.closeButton.addEventListener("click", () => this.close());
    dialog.addEventListener("cancel", (event) => { event.preventDefault(); this.close(); });
    dialog.addEventListener("close", () => { if (this.visible && !dialog.open) this.close(); });
    this.more.addEventListener("click", () => {
      if (this.pending) return;
      if (this.restart || !this.identityLoaded) {
        if (this.author) this.open(this.author, this.opener);
      } else void this.request(this.failed ? this.requested : this.next);
    });
  }

  open(id: string, opener: HTMLElement | null = null): void {
    this.invalidate();
    this.author = id;
    this.targetChanged(id);
    this.opener = opener ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    this.visible = true;
    this.identityLoaded = false;
    this.objects.clear();
    this.cursors.clear();
    this.next = null;
    this.restart = false;
    this.title.textContent = "Public profile";
    this.identity.textContent = id;
    this.list.replaceChildren();
    this.scrollTop = 0;
    this.dialog.scrollTop = 0;
    if (!this.dialog.open) this.dialog.showModal();
    this.closeButton.focus({ preventScroll: true });
    void this.request(null);
  }

  resume(): void {
    if (!this.author) return;
    this.visible = true;
    if (!this.dialog.open) this.dialog.showModal();
    this.dialog.scrollTop = this.scrollTop;
    this.list.querySelector<HTMLButtonElement>("[aria-current='true']")?.focus({ preventScroll: true });
  }

  close(): void {
    if (!this.author) return;
    const opener = this.opener;
    this.invalidate();
    this.author = null;
    this.targetChanged(null);
    this.visible = false;
    this.dialog.close();
    this.closed();
    if (opener?.isConnected && !opener.closest("[inert], [hidden]")) opener.focus({ preventScroll: true });
  }

  private invalidate(): void {
    this.generation += 1;
    this.controller?.abort();
    this.controller = null;
    this.pending = false;
  }

  private async request(cursor: string | null): Promise<void> {
    if (!this.author || this.pending) return;
    const author = this.author;
    const generation = this.generation;
    const controller = new AbortController();
    this.controller = controller;
    this.pending = true;
    this.failed = false;
    this.requested = cursor;
    this.more.hidden = false;
    this.more.disabled = true;
    this.more.textContent = "Loading...";
    this.status.textContent = cursor === null ? "Loading profile..." : "Loading more Objects...";
    this.status.dataset.state = "loading";
    this.list.setAttribute("aria-busy", "true");
    const current = () => generation === this.generation && !controller.signal.aborted;
    try {
      if (!this.identityLoaded) {
        const identity = await this.source.publicIdentity(author, controller.signal);
        if (!current()) return;
        this.showIdentity(identity);
        this.identityLoaded = true;
      }
      const page = await this.source.profileObjects(author, cursor, controller.signal);
      if (!current()) return;
      if (page.identity.id !== author || page.cards.some((card) => card.author !== author)
        || (page.nextCursor !== null && (page.nextCursor === cursor || this.cursors.has(page.nextCursor)))) {
        this.restart = true;
        throw new Error("The profile returned an invalid page. Please refresh.");
      }
      this.showIdentity(page.identity);
      if (cursor !== null) this.cursors.add(cursor);
      const focusMore = document.activeElement === this.more;
      let firstNew: HTMLButtonElement | null = null;
      for (const card of page.cards) {
        if (this.objects.has(card.id)) continue;
        this.objects.set(card.id, card);
        const row = this.row(card);
        firstNew ??= row;
        this.list.append(row);
      }
      this.next = page.nextCursor;
      this.status.textContent = this.objects.size === 0 ? "No public Objects yet."
        : this.next === null ? "All published Objects loaded." : "More Objects available.";
      this.status.dataset.state = "ready";
      this.more.textContent = "Load more";
      this.more.hidden = this.next === null;
      if (focusMore && this.visible) (firstNew ?? this.closeButton).focus({ preventScroll: true });
    } catch (cause) {
      if (!current()) return;
      this.failed = true;
      this.restart ||= cause instanceof ProfileRequestError && (cause.status === 409 || cause.status === 400);
      this.status.textContent = cause instanceof Error ? cause.message : "Could not load this profile. Please try again.";
      this.status.dataset.state = "error";
      this.more.textContent = this.restart ? "Refresh profile" : "Retry";
      this.more.hidden = false;
    } finally {
      if (current()) {
        this.pending = false;
        this.more.disabled = false;
        this.list.setAttribute("aria-busy", "false");
      }
    }
  }

  private showIdentity(identity: PublicIdentity): void {
    if (identity.id !== this.author) throw new Error("Profile identity does not match the requested author.");
    this.title.textContent = identity.handle;
    this.identity.textContent = identity.id;
  }

  private row(card: FeedCard): HTMLButtonElement {
    const row = document.createElement("button");
    row.type = "button";
    row.className = "profile-object";
    row.dataset.profileObjectId = card.id;
    const title = document.createElement("strong");
    title.textContent = card.title;
    const preview = document.createElement("span");
    preview.textContent = card.content;
    const time = document.createElement("time");
    const date = new Date(card.createdAt);
    if (Number.isFinite(date.getTime())) {
      time.dateTime = date.toISOString();
      time.textContent = date.toLocaleString();
    } else time.textContent = "Unknown time";
    row.append(title, preview, time);
    row.addEventListener("click", () => {
      for (const element of this.list.querySelectorAll("[aria-current]")) element.removeAttribute("aria-current");
      row.setAttribute("aria-current", "true");
      this.scrollTop = this.dialog.scrollTop;
      this.visible = false;
      this.dialog.close();
      this.select(card, [...this.objects.values()]);
    });
    return row;
  }
}
