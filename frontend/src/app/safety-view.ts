import { Ban, Check, createElement, LogIn, RotateCcw, Volume2, VolumeX, X } from "lucide";
import { SafetyController, safetyIdentityId, safetyMessage } from "./safety";
import type { SafetyField, SafetySnapshot, SafetySource } from "./safety";

export interface SafetyControlsOptions {
  readonly source: SafetySource;
  readonly signIn: () => void;
  readonly changed: (snapshot: SafetySnapshot | null) => void;
}
const blockScope = "Blocking hides this author's posts from discovery and Following, and prevents new local-node follows, replies, shares, reactions, and graph relationships in either direction. Existing follows and signed history are retained. Public posts stay public: this does not control other nodes, logged-out readers, or alternate identities.";
let instance = 0;
function element<K extends keyof HTMLElementTagNameMap>(tag: K, className = "", text = ""): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag); node.className = className; node.textContent = text; return node;
}

/** Owns its native modal; parent supplies only the authenticated host source. */
export class SafetyControls {
  private readonly controller: SafetyController;
  private readonly dialog = element("dialog", "safety-dialog");
  private readonly title = element("h2");
  private readonly body = element("div", "safety-body");
  private readonly status = element("p", "safety-status");
  private readonly closeButton: HTMLButtonElement;
  private opener: HTMLElement | null = null;
  private mode: "author" | "list" | null = null;
  private context = 0;
  private confirming = false;
  private listLoading = false;
  private listError = "";
  private disposed = false;

  constructor(private readonly options: SafetyControlsOptions) {
    const id = `safety-${++instance}`;
    this.title.id = `${id}-title`; this.status.id = `${id}-status`;
    this.status.setAttribute("role", "status"); this.status.setAttribute("aria-live", "polite");
    this.status.setAttribute("aria-atomic", "true");
    this.dialog.setAttribute("data-safety-dialog", "");
    this.dialog.setAttribute("aria-labelledby", this.title.id);
    this.dialog.setAttribute("aria-describedby", this.status.id);
    this.closeButton = this.button("Close", "close", X, () => this.close(), true);
    this.closeButton.className = "safety-close";
    const header = element("header", "safety-header"); header.append(this.title, this.closeButton);
    this.dialog.append(header, this.body, this.status);
    this.dialog.addEventListener("cancel", (event) => { event.preventDefault(); this.close(); });
    this.dialog.addEventListener("close", () => { if (!this.dialog.open) this.dismiss(); });
    this.controller = new SafetyController(options.source, options.changed, () => this.render());
    document.body.append(this.dialog);
  }

  get snapshot(): SafetySnapshot | null { return this.controller.snapshot; }
  hidden(author: string): boolean { return this.controller.hidden(author); }
  blocked(author: string): boolean { return this.controller.blocked(author); }

  account(owner: string | null): void {
    if (this.disposed || owner === this.controller.owner) return;
    this.close();
    this.controller.account(owner);
  }
  ensure(): Promise<SafetySnapshot | null> { return this.controller.ensure(); }
  async refresh(): Promise<void> {
    const context = this.context;
    this.listError = "";
    if (this.mode === "list") { this.listLoading = true; this.listError = ""; this.render(); }
    try {
      await this.controller.refresh();
      if (context === this.context && this.mode === "author" && !this.controller.view.pending) await this.controller.read();
    }
    catch (cause) {
      if (context === this.context) this.listError = safetyMessage(cause);
      throw cause;
    } finally {
      if (context === this.context) { this.listLoading = false; this.render(); }
    }
  }

  openAuthor(target: string, opener: HTMLElement | null = null): void {
    if (this.disposed) return;
    this.begin("author", opener);
    if (!safetyIdentityId(target)) {
      void this.controller.show(null);
      this.message("This author identity is not valid.", true);
      return;
    }
    void this.controller.show(target);
  }

  openList(opener: HTMLElement | null = null): void {
    if (this.disposed) return;
    this.begin("list", opener);
    void this.controller.show(null);
    if (this.controller.owner) void this.refresh().catch(() => undefined);
  }

  close(): void {
    if (this.dialog.open) this.dialog.close();
    this.dismiss();
  }
  dispose(): void {
    if (this.disposed) return;
    this.close(); this.controller.dispose(); this.dialog.remove(); this.disposed = true;
  }

  private begin(mode: "author" | "list", opener: HTMLElement | null): void {
    ++this.context; this.confirming = false; this.listLoading = false; this.listError = "";
    if (!this.dialog.open) this.opener = opener ?? (document.activeElement instanceof HTMLElement ? document.activeElement : null);
    this.mode = mode;
    this.render();
    if (!this.dialog.open) this.dialog.showModal();
    this.closeButton.focus();
  }

  private dismiss(): void {
    if (this.mode === null) return;
    ++this.context; this.mode = null; this.confirming = false; this.listLoading = false; this.listError = "";
    void this.controller.show(null);
    this.body.replaceChildren(); this.status.textContent = ""; this.title.textContent = "";
    const opener = this.opener; this.opener = null;
    if (opener?.isConnected) opener.focus();
  }

  private render(): void {
    if (!this.mode) return;
    const active = document.activeElement;
    const focusKey = active instanceof HTMLElement && this.body.contains(active) ? active.getAttribute("data-safety-focus") : null;
    this.body.replaceChildren();
    this.title.textContent = this.mode === "list" ? "Blocked and muted" : "Author controls";
    this.dialog.setAttribute("aria-busy", String(this.mode === "list" ? this.listLoading : this.controller.view.pending));
    this.message("");
    if (!this.controller.owner) {
      this.body.append(element("p", "", "Sign in to manage blocked and muted authors on your node."),
        this.button("Sign in", "sign-in", LogIn, () => { this.close(); this.options.signIn(); }));
    } else if (this.mode === "list") this.renderList();
    else this.renderAuthor();
    if (focusKey && this.dialog.open) {
      const replacement = [...this.body.querySelectorAll<HTMLButtonElement>("[data-safety-focus]")]
        .find((node) => node.getAttribute("data-safety-focus") === focusKey && !node.disabled);
      (replacement ?? this.closeButton).focus();
    }
  }

  private renderList(): void {
    const list = element("ul", "safety-list"); list.setAttribute("data-safety-list", "");
    this.body.append(element("p", "", "Private to your account. Blocking and muting are separate choices. Explicit profiles remain available."), list);
    if (this.listLoading) { this.message("Loading blocked and muted authors..."); return; }
    if (this.listError || !this.snapshot) {
      this.message(this.listError || "Could not load blocked and muted authors.", true);
      this.body.append(this.button("Retry", "retry", RotateCcw, () => { void this.refresh().catch(() => undefined); }));
      return;
    }
    if (!this.snapshot.entries.length) list.append(element("li", "", "No blocked or muted authors."));
    for (const entry of this.snapshot.entries) {
      const row = element("li", "safety-row"); row.setAttribute("data-safety-author", entry.identity.id);
      row.append(element("h3", "", entry.identity.handle), element("p", "safety-identity", entry.identity.id),
        element("p", "", [entry.state.blocked ? "Blocked" : "", entry.state.muted ? "Muted" : ""].filter(Boolean).join(" and ")));
      const actions = element("div", "safety-actions");
      if (entry.state.blocked) actions.append(this.button("Unblock", "unblock", Ban, () => { void this.remove(entry.identity.id, "blocked"); }, false, entry.identity.id));
      if (entry.state.muted) actions.append(this.button("Unmute", "unmute", Volume2, () => { void this.remove(entry.identity.id, "muted"); }, false, entry.identity.id));
      row.append(actions); list.append(row);
      if (this.controller.view.pending) for (const button of actions.querySelectorAll("button")) button.disabled = true;
    }
  }

  private async remove(target: string, field: SafetyField): Promise<void> {
    // The list choice is explicit, but the CAS revision comes from a fresh pair read.
    this.begin("author", null);
    const context = this.context;
    await this.controller.show(target);
    if (context === this.context && this.mode === "author") await this.controller.change(field, false);
  }

  private renderAuthor(): void {
    const view = this.controller.view, target = view.target;
    if (!target) return;
    const author = element("section"); author.setAttribute("data-safety-author", target);
    const identity = view.identity ?? this.snapshot?.entries.find((entry) => entry.identity.id === target)?.identity;
    if (identity) author.append(element("h3", "", identity.handle));
    author.append(element("p", "safety-identity", target)); this.body.append(author);
    if (target === this.controller.owner) { this.message("You cannot block or mute your own account."); return; }
    if (this.listError) this.message(this.listError, true);
    else if (view.message) this.message(view.message, !view.pending && view.message !== "Current safety state refreshed.");
    if (view.pending) { this.confirming = false; return; }
    if (!view.state) {
      this.body.append(this.button("Retry", "retry", RotateCcw, () => { void this.controller.retry(); }));
      return;
    }
    this.body.append(element("p", "", `Blocked: ${view.state.blocked ? "Yes" : "No"}. Muted: ${view.state.muted ? "Yes" : "No"}.`));
    if (view.retry) {
      this.body.append(element("p", "", "Confirm the previous change's outcome before choosing another change."),
        this.button("Retry previous change", "retry", RotateCcw, () => { this.confirming = false; void this.controller.retry(); }));
      return;
    }
    else if (!this.snapshot) this.body.append(this.button("Refresh safety state", "retry", RotateCcw, () => {
      void this.refresh().catch(() => undefined);
    }));
    if (this.confirming) {
      const confirmation = element("section", "safety-confirmation");
      const heading = element("h3", "", "Block this author?");
      const actions = element("div", "safety-actions");
      actions.append(this.button("Cancel", "cancel-block", X, () => {
        this.confirming = false; this.render(); this.focus("block");
      }), this.button("Block author", "confirm-block", Ban, () => {
        if (!this.confirming) return;
        this.confirming = false; void this.controller.change("blocked", true);
      }));
      actions.lastElementChild?.classList.add("safety-danger");
      confirmation.append(heading, element("p", "", blockScope), actions); this.body.append(confirmation);
      return;
    }
    const actions = element("div", "safety-actions");
    const blocked = view.state.blocked, muted = view.state.muted;
    actions.append(this.button(blocked ? "Unblock" : "Block author", blocked ? "unblock" : "block", Ban, () => {
      if (blocked) void this.controller.change("blocked", false);
      else { this.confirming = true; this.render(); this.focus("cancel-block"); }
    }), this.button(muted ? "Unmute" : "Mute author", muted ? "unmute" : "mute", muted ? Volume2 : VolumeX, () => {
      void this.controller.change("muted", !muted);
    }));
    this.body.append(element("p", "", "Muting hides posts from discovery and Following only. Explicit profiles and interactions remain available. Unblocking does not unmute."), actions,
      this.button("Blocked and muted", "list", Check, () => this.openList()));
  }

  private button(label: string, action: string, icon: typeof X, handler: () => void, iconOnly = false, target = ""): HTMLButtonElement {
    const node = element("button"); node.type = "button";
    node.setAttribute("data-safety-action", action); node.setAttribute("data-safety-focus", `${target}:${action}`);
    node.title = label; node.setAttribute("aria-label", label);
    node.append(createElement(icon, { "aria-hidden": "true", focusable: "false" }));
    if (!iconOnly) node.append(element("span", "", label));
    const context = this.context;
    node.addEventListener("click", () => {
      if (this.disposed || !this.dialog.open || !node.isConnected || (action !== "close" && context !== this.context)) return;
      handler();
    });
    return node;
  }
  private focus(action: string): void { this.body.querySelector<HTMLButtonElement>(`[data-safety-action="${action}"]`)?.focus(); }
  private message(text: string, error = false): void {
    this.status.textContent = text; this.status.setAttribute("data-error", String(error));
    this.status.setAttribute("role", error ? "alert" : "status");
  }
}
