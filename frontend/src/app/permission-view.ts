import { createElement, RotateCw, X } from "lucide";
import type { Accounts } from "./accounts";
import { BabbleFrontendClient } from "./protocol";
import { Permissions, matchingGrants, mayApprove, permitsSurfaceStart, usesInvocationConsent, type PermissionState } from "./permissions";

const labels: Readonly<Record<string, string>> = {
  "babble.identity.current": "Current identity", "babble.storage.local": "Private local storage",
  "babble.storage.object": "Object storage", "babble.network.fetch": "Network requests",
  "babble.social.follow": "Follow Objects", "babble.social.unfollow": "Unfollow Objects",
  "babble.social.share": "Share Objects", "babble.social.reply": "Reply to Objects",
  "babble.realtime.join": "Join realtime rooms", "babble.realtime.send": "Send realtime messages",
  "babble.realtime.leave": "Leave realtime rooms", "babble.payments.checkout": "Payments",
  "babble.ai.judge": "Evaluate Judgments", "babble.ai.generate": "Generate with AI",
  "babble.ai.embed": "Create embeddings", "babble.ai.transcribe": "Transcribe media",
  "babble.media.camera": "Camera", "babble.media.microphone": "Microphone",
  "babble.graphics.webgpu": "GPU rendering", "babble.notifications.request": "Notifications",
  "babble.clipboard.write": "Write to clipboard", "babble.fullscreen.enter": "Enter fullscreen",
  "babble.location": "Location", "babble.files": "Files",
};

export class PermissionPanel {
  private readonly controller: Permissions;
  private readonly list = document.createElement("div");
  private readonly status = document.createElement("p");
  private readonly owner = document.createElement("p");
  private readonly object = document.createElement("p");
  private readonly refresh = document.createElement("button");
  private readonly launch = document.createElement("button");
  private opener: HTMLElement | null = null;
  private focusKey: string | undefined;

  constructor(private readonly dialog: HTMLDialogElement, private readonly accounts: Accounts,
    private readonly stopSurface: () => void, private readonly openSurface: (id: string) => void,
    private readonly signIn: () => void) {
    this.controller = new Permissions(state => this.render(state));
    const header = document.createElement("header");
    const title = document.createElement("h2");
    title.id = "permission-title";
    title.textContent = "Object permissions";
    dialog.setAttribute("aria-labelledby", title.id);
    const close = document.createElement("button");
    close.type = "button";
    close.className = "panel-icon";
    close.ariaLabel = close.title = "Close permissions";
    close.append(createElement(X, { "aria-hidden": "true" }));
    close.addEventListener("click", () => this.close());
    header.append(title, close);
    this.owner.className = "permission-owner";
    this.object.className = "permission-object";
    this.list.className = "permission-list";
    this.list.dataset.permissionList = "";
    this.status.className = "permission-status";
    this.status.setAttribute("role", "status");
    this.status.dataset.permissionStatus = "";
    const footer = document.createElement("footer");
    this.refresh.type = "button";
    this.refresh.className = "panel-icon";
    this.refresh.ariaLabel = this.refresh.title = "Refresh permissions";
    this.refresh.append(createElement(RotateCw, { "aria-hidden": "true" }));
    this.refresh.addEventListener("click", () => void this.controller.refresh());
    this.launch.type = "button";
    this.launch.className = "permission-open";
    this.launch.textContent = "Open Object";
    this.launch.dataset.permissionOpen = "";
    this.launch.addEventListener("click", () => {
      const id = this.controller.state.objectId;
      if (!id || this.controller.state.busy) return;
      this.close();
      this.openSurface(id);
    });
    footer.append(this.refresh, this.launch);
    dialog.replaceChildren(header, this.owner, this.object, this.list, this.status, footer);
    dialog.addEventListener("cancel", event => { event.preventDefault(); this.close(); });
    dialog.addEventListener("close", () => { if (!dialog.open) this.controller.close(); });
    accounts.addEventListener("change", () => this.close());
  }

  open(objectId: string, canLaunch = true): void {
    const session = this.accounts.current;
    if (!session) { this.signIn(); return; }
    this.opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    this.stopSurface();
    this.owner.textContent = `For ${session.identity.handle}`;
    this.object.textContent = objectId;
    this.launch.hidden = !canLaunch;
    if (!this.dialog.open) this.dialog.showModal();
    const authorized = () => this.accounts.current?.token === session.token;
    const source = new BabbleFrontendClient(this.accounts.origin.href, (input, init) => {
      if (!authorized()) throw new Error("The signed-in account changed. Review permissions again.");
      return this.accounts.authenticatedFetch(input, init);
    });
    void this.controller.open(objectId, session.identity.id, source, authorized);
  }

  close(): void {
    this.focusKey = undefined;
    this.controller.close();
    if (!this.dialog.open) return;
    this.dialog.close();
    if (this.opener?.isConnected && !this.opener.closest("[inert]")) this.opener.focus({ preventScroll: true });
    this.opener = null;
  }

  private render(state: PermissionState): void {
    const focus = this.dialog.contains(document.activeElement)
      ? (document.activeElement as HTMLElement).dataset.permissionKey : undefined;
    if (focus !== undefined) this.focusKey = focus;
    this.dialog.setAttribute("aria-busy", String(state.busy));
    this.refresh.disabled = state.busy;
    this.launch.disabled = state.busy || !state.review || state.review.decisions.some(d => !permitsSurfaceStart(d));
    this.status.textContent = state.error ?? (state.busy ? "Checking permissions..."
      : state.review?.decisions.length === 0 ? "No permissions requested." : "");
    this.status.dataset.state = state.error ? "error" : "ready";
    this.list.replaceChildren();
    if (!state.review) {
      if (!state.busy && this.focusKey !== undefined && this.dialog.open) {
        this.refresh.focus({ preventScroll: true });
        this.focusKey = undefined;
      }
      return;
    }
    for (const [index, decision] of state.review.decisions.entries()) {
      const row = document.createElement("section");
      row.className = "permission-row";
      row.dataset.permissionId = decision.request.id;
      const title = document.createElement("h3");
      title.textContent = labels[decision.request.id] ?? decision.request.id;
      const identity = document.createElement("p");
      identity.className = "permission-id";
      identity.textContent = `${decision.request.id} / v${decision.request.version}`;
      const status = document.createElement("p");
      status.textContent = decision.reason;
      status.className = "permission-reason";
      const scope = document.createElement("dl");
      for (const [key, value] of Object.entries(decision.request.scope as Record<string, unknown>)) {
        const label = document.createElement("dt"), detail = document.createElement("dd");
        label.textContent = key.replaceAll("_", " ");
        detail.textContent = typeof value === "string" ? value : JSON.stringify(value);
        scope.append(label, detail);
      }
      if (scope.children.length === 0) {
        const label = document.createElement("dt"), detail = document.createElement("dd");
        label.textContent = "Scope";
        detail.textContent = "No additional scope restrictions declared";
        scope.append(label, detail);
      }
      row.append(title, identity, scope, status);
      if (decision.definition) {
        const limits = document.createElement("details");
        const summary = document.createElement("summary");
        summary.textContent = "Permission limits";
        const values = document.createElement("dl");
        const quota = decision.definition.quota;
        for (const [name, value] of [
          ["Calls per minute", quota.calls_per_minute], ["Bytes per minute", quota.bytes_per_minute],
          ["Persistent bytes", quota.persistent_bytes], ["Realtime connections", quota.realtime_connections],
          ["Maximum call duration (ms)", quota.max_call_ms], ["Background access", quota.background_allowed ? "Allowed" : "Not allowed"],
        ] as const) {
          const label = document.createElement("dt"), detail = document.createElement("dd");
          label.textContent = name; detail.textContent = String(value);
          values.append(label, detail);
        }
        limits.append(summary, values);
        row.append(limits);
      }
      if (decision.definition?.permission === "ask_each_time") {
        const notice = document.createElement("p");
        notice.className = "permission-reason";
        notice.textContent = usesInvocationConsent(decision)
          ? "Ask every time. Each action needs your approval."
          : "This approval remains active until revoked. It is not one-time consent.";
        row.append(notice);
      }
      const grants = matchingGrants(state.review, decision.request);
      const action = grants.length ? "revoke" : mayApprove(decision) ? "approve" : null;
      if (action) {
        if (action === "approve" && decision.definition?.permission === "ask_once") {
          const notice = document.createElement("p");
          notice.className = "permission-reason";
          notice.textContent = "Approval remains active until revoked.";
          row.append(notice);
        }
        const button = document.createElement("button");
        button.type = "button";
        button.textContent = action === "approve" ? "Allow" : usesInvocationConsent(decision) ? "Remove old approval" : "Revoke access";
        button.ariaLabel = `${button.textContent}: ${title.textContent}`;
        button.dataset.permissionAction = action;
        button.dataset.permissionKey = String(index);
        button.disabled = state.busy;
        button.addEventListener("click", () => void this.controller.change(index, action));
        row.append(button);
      }
      this.list.append(row);
    }
    if (this.focusKey !== undefined && !state.busy) {
      const next = [...this.list.querySelectorAll<HTMLButtonElement>("[data-permission-key]")]
        .find(button => button.dataset.permissionKey === this.focusKey);
      (next ?? this.refresh).focus({ preventScroll: true });
      this.focusKey = undefined;
    }
  }
}
