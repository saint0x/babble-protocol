import { AuthorFollow, FollowingPages, type FollowingClient, type FollowView } from "./following";
import type { PublicIdentity } from "./protocol";

export class FollowingControls {
  readonly author: AuthorFollow;
  readonly people: FollowingPages<PublicIdentity>;
  private owner: string | null = null;
  private target: string | null = null;
  private blocked = false;
  private resumeTarget: string | null = null;
  private opener: HTMLElement | null = null;
  private readonly button = find<HTMLButtonElement>("[data-author-follow]");
  private readonly status = find<HTMLElement>("[data-author-follow-status]");
  private readonly dialog = find<HTMLDialogElement>("[data-following-dialog]");
  private readonly list = find<HTMLElement>("[data-following-list]");
  private readonly more = find<HTMLButtonElement>("[data-following-list-more]");
  private readonly listStatus = find<HTMLElement>("[data-following-list-status]");
  private readonly listSignIn = find<HTMLButtonElement>("[data-following-list-signin]");

  constructor(source: FollowingClient, private readonly signIn: () => void,
    private readonly openProfile: (id: string, opener: HTMLElement | null) => void,
    private readonly mutated: () => void) {
    this.author = new AuthorFollow(source, (view) => this.renderAuthor(view), () => {
      this.people.clear();
      this.mutated();
    });
    this.people = new FollowingPages(async (cursor, _query, signal) => {
      const page = await source.list(cursor, signal);
      return { items: page.identities, next: page.next_cursor };
    }, (view) => {
      const focused = document.activeElement === this.more;
      const previousCount = this.list.children.length;
      this.list.replaceChildren();
      for (const identity of view.items) {
        const row = document.createElement("button");
        row.type = "button";
        row.className = "profile-object";
        row.dataset.followingAuthor = identity.id;
        const handle = document.createElement("strong");
        handle.textContent = identity.handle;
        const id = document.createElement("span");
        id.textContent = identity.id;
        row.append(handle, id);
        row.addEventListener("click", () => { this.closeList(); this.openProfile(identity.id, this.opener); });
        this.list.append(row);
      }
      this.list.setAttribute("aria-busy", String(view.phase === "loading"));
      this.listStatus.dataset.state = view.phase;
      this.listStatus.textContent = view.message || (view.phase === "ready" && !view.items.length ? "You aren't following anyone yet." : "");
      this.more.hidden = view.phase === "idle" || view.phase === "guest" || (view.phase === "ready" && view.next === null);
      this.more.disabled = view.phase === "loading";
      this.more.textContent = view.phase === "loading" ? "Loading..." : view.restart ? "Refresh" : view.phase === "error" ? "Retry" : "Load more";
      this.listSignIn.hidden = view.phase !== "guest";
      if (focused && this.more.hidden && this.dialog.open) {
        const next = this.list.children.item(previousCount);
        (next instanceof HTMLElement ? next : find<HTMLButtonElement>("[data-following-list-close]")).focus({ preventScroll: true });
      }
    });
    this.button.addEventListener("click", () => {
      if (!this.owner) {
        this.resumeTarget = this.target;
        this.signIn();
      } else if (this.blocked && !this.author.view.state) void this.author.read();
      else void this.author.toggle();
    });
    this.more.addEventListener("click", () => void this.people.more());
    this.listSignIn.addEventListener("click", () => { this.closeList(); this.signIn(); });
    find("[data-following-list-close]").addEventListener("click", () => this.closeList());
    this.dialog.addEventListener("cancel", (event) => { event.preventDefault(); this.closeList(); });
    this.dialog.addEventListener("close", () => { if (!this.dialog.open) this.people.clear(); });
  }

  account(owner: string | null): void {
    if (owner === this.owner) return;
    this.owner = owner;
    this.blocked = false;
    this.closeList();
    this.target = null;
    this.author.show(owner, null);
    if (!owner) this.resumeTarget = null;
  }

  profile(target: string | null): void {
    this.target = target;
    this.blocked = false;
    this.author.show(this.owner, target);
  }

  restrict(blocked: boolean): void {
    this.blocked = blocked;
    this.renderAuthor(this.author.view);
  }

  resumeAfterSignIn(): void {
    const target = this.resumeTarget;
    this.resumeTarget = null;
    if (target && this.owner) this.openProfile(target, null);
  }

  openList(opener: HTMLElement | null): void {
    this.opener = opener;
    if (!this.dialog.open) this.dialog.showModal();
    find<HTMLButtonElement>("[data-following-list-close]").focus();
    void this.people.start(this.owner);
  }

  closeList(): void {
    this.people.clear();
    if (this.dialog.open) {
      this.dialog.close();
      this.opener?.focus({ preventScroll: true });
    }
  }

  private renderAuthor(view: FollowView): void {
    this.button.hidden = !view.target || view.owner === view.target;
    this.button.disabled = view.pending || (this.blocked && view.state?.following === false);
    this.button.setAttribute("aria-busy", String(view.pending));
    this.button.setAttribute("aria-pressed", String(view.state?.following ?? false));
    this.button.dataset.following = view.state ? String(view.state.following) : "unknown";
    this.button.textContent = view.pending ? "Loading..." : !view.owner ? "Follow"
      : this.blocked && !view.state ? "Refresh follow state"
      : this.blocked && view.state?.following === false ? "Blocked"
      : view.retry !== null ? view.retry ? "Retry follow" : "Retry unfollow"
      : !view.state ? "Refresh follow state" : view.state.following ? "Following" : "Follow";
    this.button.title = view.state?.following ? "Unfollow author" : this.blocked ? "Unblock in author controls to follow" : "Follow author";
    this.status.textContent = view.message;
    this.status.hidden = this.button.hidden || !view.message;
  }
}

function find<T extends HTMLElement = HTMLElement>(selector: string): T {
  const element = document.querySelector<T>(selector);
  if (!element) throw new Error(`Missing Following element: ${selector}`);
  return element;
}
