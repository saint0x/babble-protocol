import { createElement, RotateCw } from "lucide";
import { Accounts, newPasswordError } from "./accounts";
import { AccountSecurity, SecurityNotice, type AccountSessionInfo, type RevokeTarget } from "./account-security";

function element<K extends keyof HTMLElementTagNameMap>(tag: K, text?: string): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (text) node.textContent = text;
  return node;
}
function button(text: string, attribute: string, action: () => void): HTMLButtonElement {
  const node = element("button", text);
  node.type = "button";
  node.setAttribute(attribute, "");
  node.addEventListener("click", action);
  return node;
}
function status(attribute: string): HTMLParagraphElement {
  const node = element("p");
  node.setAttribute(attribute, "");
  node.setAttribute("role", "status");
  node.setAttribute("aria-live", "polite");
  return node;
}
function password(label: string, attribute: string, autocomplete: "current-password" | "new-password"): { label: HTMLLabelElement; input: HTMLInputElement } {
  const wrapper = element("label", label);
  const input = element("input");
  input.type = "password";
  input.autocomplete = autocomplete;
  input.required = true;
  input.maxLength = 1024;
  input.setAttribute(attribute, "");
  input.addEventListener("input", () => input.setCustomValidity(""));
  wrapper.append(input);
  return { label: wrapper, input };
}

/** Stable form nodes preserve focus, password-manager integration and edit selection. */
export class AccountSecurityView {
  readonly controller: AccountSecurity;
  private readonly list = element("ul");
  private readonly sessionStatus = status("data-security-status");
  private readonly passwordStatus = status("data-security-password-status");
  private readonly refresh: HTMLButtonElement;
  private readonly others: HTMLButtonElement;
  private readonly confirmation = element("div");
  private readonly confirmationText = element("p");
  private readonly confirm: HTMLButtonElement;
  private readonly cancel: HTMLButtonElement;
  private readonly form = element("form");
  private readonly current = password("Current password", "data-security-current-password", "current-password");
  private readonly next = password("New password", "data-security-new-password", "new-password");
  private readonly repeated = password("Confirm new password", "data-security-confirm-password", "new-password");
  private readonly commit = element("button", "Change password and sign out all");
  private readonly recovery: HTMLButtonElement;
  private listSignature = "";
  private previousConfirmation = false;
  private returnFocus: HTMLElement | null = null;
  private readonly onChange = () => this.render();

  constructor(private readonly root: HTMLElement, accounts: Accounts, beforeLogout: () => void, notice: (message: string) => void) {
    this.controller = new AccountSecurity(accounts);
    this.controller.addEventListener("notice", (event) => { if (event instanceof SecurityNotice) notice(event.message); });
    root.classList.add("account-security");
    const header = element("div"); header.className = "security-heading";
    const heading = element("h3", "Active sessions");
    heading.id = "security-sessions-heading";
    this.list.setAttribute("aria-labelledby", heading.id);
    this.refresh = button("", "data-security-refresh", () => { void this.controller.refresh(); });
    this.refresh.title = "Refresh sessions";
    this.refresh.setAttribute("aria-label", "Refresh sessions");
    this.refresh.className = "security-refresh";
    this.refresh.append(createElement(RotateCw, { "aria-hidden": "true" }));
    header.append(heading, this.refresh);
    this.others = button("Sign out other sessions", "data-security-revoke-others", () => this.request("others"));
    this.others.className = "security-secondary";
    this.confirm = button("Sign out", "data-security-confirm", () => {
      const target = this.controller.view.confirmation;
      if (target === "current" || (target && typeof target !== "string" && target.current)) beforeLogout();
      void this.controller.confirmRevoke();
    });
    this.cancel = button("Cancel", "data-security-cancel", () => this.controller.cancelRevoke());
    const confirmActions = element("div"); confirmActions.className = "security-actions";
    confirmActions.append(this.cancel, this.confirm);
    this.confirmation.className = "security-confirmation";
    this.confirmation.setAttribute("role", "group");
    this.confirmation.setAttribute("aria-label", "Confirm session sign-out");
    this.confirmation.append(this.confirmationText, confirmActions);
    this.form.setAttribute("data-security-password-form", "");
    this.form.setAttribute("aria-labelledby", "security-password-heading");
    const passwordHeading = element("h3", "Change password"); passwordHeading.id = "security-password-heading";
    const policy = element("p", "At least 15 characters. Up to 1024 UTF-8 bytes.");
    policy.id = "security-password-policy";
    this.next.input.setAttribute("aria-describedby", policy.id);
    const scope = element("p", "This changes your password and signs out every session, including this one.");
    scope.id = "security-password-scope";
    this.commit.type = "submit";
    this.commit.setAttribute("aria-describedby", scope.id);
    this.commit.setAttribute("data-security-password-submit", "");
    this.commit.className = "security-primary";
    this.recovery = button("Sign in to check password", "data-security-recover", () => this.controller.recoverSignIn());
    this.form.append(passwordHeading, this.current.label, this.next.label, policy, this.repeated.label, scope, this.commit,
      this.passwordStatus, this.recovery);
    this.form.addEventListener("submit", (event) => {
      event.preventDefault();
      const state = this.controller.view;
      if (state.passwordBusy || state.sessionBusy || state.uncertainPassword) return;
      this.next.input.setCustomValidity(newPasswordError(this.next.input.value)
        ?? (this.next.input.value === this.current.input.value ? "Choose a new password different from your current password." : ""));
      this.repeated.input.setCustomValidity(this.next.input.value === this.repeated.input.value ? "" : "The new passwords do not match.");
      if (!this.form.reportValidity()) return;
      const pending = this.controller.changePassword(this.current.input.value, this.next.input.value, this.repeated.input.value);
      this.clearSecrets();
      void pending;
    });
    root.append(header, this.list, this.others, this.confirmation, this.sessionStatus, this.form);
    this.controller.addEventListener("change", this.onChange);
    this.render();
  }

  open(): void { this.controller.open(); }
  requestCurrentLogout(): void { this.request("current"); }
  close(): void { this.clearSecrets(); this.controller.close(); this.returnFocus = null; }
  clearSecrets(): void {
    for (const field of [this.current.input, this.next.input, this.repeated.input]) {
      field.value = "";
      field.setCustomValidity("");
    }
  }
  dispose(): void { this.close(); this.controller.removeEventListener("change", this.onChange); this.controller.dispose(); }

  private request(target: RevokeTarget): void {
    this.returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    this.controller.requestRevoke(target);
  }

  private render(): void {
    const state = this.controller.view;
    this.root.setAttribute("aria-busy", String(state.loading || state.sessionBusy || state.passwordBusy));
    this.list.setAttribute("aria-busy", String(state.loading || state.sessionBusy));
    this.form.setAttribute("aria-busy", String(state.passwordBusy));
    this.sessionStatus.textContent = state.status;
    this.sessionStatus.dataset.state = state.statusState;
    this.passwordStatus.textContent = state.passwordStatus;
    this.passwordStatus.dataset.state = state.passwordState;
    this.refresh.disabled = state.loading || state.sessionBusy;
    this.others.disabled = state.sessionBusy || state.passwordBusy || !state.sessions.some((session) => !session.current);
    const signature = JSON.stringify(state.sessions);
    if (signature !== this.listSignature) {
      const focused = document.activeElement instanceof HTMLElement
        ? document.activeElement.closest<HTMLElement>("[data-account-session-id]")?.dataset.accountSessionId : null;
      this.list.replaceChildren(...state.sessions.map((session, index) => this.sessionRow(session, index)));
      this.listSignature = signature;
      if (focused) {
        const replacement = Array.from(this.list.children).find((row) => (row as HTMLElement).dataset.accountSessionId === focused);
        (replacement?.querySelector<HTMLButtonElement>("button") ?? this.refresh).focus();
      }
    }
    for (const control of this.list.querySelectorAll<HTMLButtonElement>("button")) control.disabled = state.sessionBusy || state.passwordBusy;
    this.confirmation.hidden = state.confirmation === null;
    this.confirm.disabled = this.cancel.disabled = state.sessionBusy || state.passwordBusy;
    if (state.confirmation) this.confirmationText.textContent = state.confirmation === "others"
      ? "Sign out every other session? This session stays signed in."
      : state.confirmation === "current" || state.confirmation.current ? "Sign out this session? You will need your password to sign in again."
      : "Sign out this session? Its account access will be revoked.";
    if (state.confirmation && !this.previousConfirmation) this.cancel.focus();
    if (!state.confirmation && this.previousConfirmation && this.returnFocus?.isConnected) this.returnFocus.focus();
    this.previousConfirmation = state.confirmation !== null;
    for (const field of [this.current.input, this.next.input, this.repeated.input]) field.disabled = state.passwordBusy || state.uncertainPassword;
    this.commit.disabled = state.passwordBusy || state.sessionBusy || state.uncertainPassword;
    this.commit.textContent = state.passwordBusy ? "Changing password..." : "Change password and sign out all";
    this.recovery.hidden = !state.uncertainPassword;
  }

  private sessionRow(session: AccountSessionInfo, index: number): HTMLLIElement {
    const row = element("li"); row.dataset.accountSessionId = session.id;
    const details = element("div"); details.className = "security-session-details";
    const title = element("div"); title.className = "security-session-title";
    title.append(element("strong", session.current ? "This session" : `Session ${index + 1}`));
    if (session.current) { const badge = element("span", "Current"); badge.className = "security-current"; title.append(badge); }
    details.append(title);
    for (const [label, value] of [["Created", session.created_at], ["Expires", session.expires_at]] as const) {
      const line = element("p", `${label} `);
      if (value === null) line.append(document.createTextNode("unknown (legacy session)"));
      else { const time = element("time", new Date(value).toLocaleString()); time.dateTime = value; line.append(time); }
      details.append(line);
    }
    const revoke = button("Sign out", "data-security-revoke", () => this.request(session));
    revoke.setAttribute("aria-label", session.current ? "Sign out this session" : `Sign out session ${index + 1}`);
    row.append(details, revoke);
    return row;
  }
}
