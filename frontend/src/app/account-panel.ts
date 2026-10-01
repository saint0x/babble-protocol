import { Accounts, newPasswordError } from "./accounts";
import { AccountSecurityView } from "./account-security-view";
import { uncertainPasswordMessage } from "./account-security";

export class AccountPanel {
  private readonly dialog: HTMLDialogElement;
  private mode: "login" | "register" = "login";
  private busy = false;
  private operation = 0;
  private pendingAuthentication = false;
  private readonly security: AccountSecurityView;

  constructor(private readonly accounts: Accounts, beforeLogout: () => void) {
    this.dialog = document.querySelector<HTMLDialogElement>("[data-account-dialog]")!;
    this.security = new AccountSecurityView(this.element("[data-account-security]"), accounts, beforeLogout, (message) => this.message(message));
    this.element("[data-account-close]").addEventListener("click", () => this.dialog.close());
    this.dialog.addEventListener("close", () => {
      this.operation++;
      this.busy = false;
      this.pendingAuthentication = false;
      this.password.value = "";
      this.security.close();
    });
    this.password.addEventListener("input", () => this.password.setCustomValidity(""));
    for (const button of this.dialog.querySelectorAll<HTMLButtonElement>("[data-account-mode]")) {
      button.addEventListener("click", () => {
        this.mode = button.dataset.accountMode === "register" ? "register" : "login";
        this.password.value = "";
        this.password.setCustomValidity("");
        this.message("");
        this.render();
      });
    }
    this.element("[data-account-form]").addEventListener("submit", (event) => {
      event.preventDefault();
      void this.submit();
    });
    this.element("[data-account-logout]").addEventListener("click", () => void this.logout());
    let token = accounts.current?.token;
    accounts.addEventListener("change", () => {
      const session = accounts.current;
      if (token !== session?.token) {
        // Keep a pending completion eligible; only its returned token can prove success.
        const completingAuthentication = !token && session && this.pendingAuthentication;
        token = session?.token;
        if (!completingAuthentication) {
          this.operation++;
          this.busy = false;
          this.pendingAuthentication = false;
        }
        this.password.value = "";
        this.security.clearSecrets();
        this.busy = false;
        this.message(token ? ""
          : this.security.controller.passwordSignoutPending ? uncertainPasswordMessage : "Session ended. Sign in again.");
      }
      this.render();
      if (this.dialog.open) this.security.open();
    });
    this.render();
  }

  open(): void {
    this.render();
    if (!this.dialog.open) this.dialog.showModal();
    this.security.open();
  }

  async logout(): Promise<void> {
    if (this.busy) return;
    this.open();
    this.security.requestCurrentLogout();
  }

  private async submit(): Promise<void> {
    if (this.busy) return;
    this.password.setCustomValidity(this.mode === "register" ? newPasswordError(this.password.value) ?? "" : "");
    if (!this.password.reportValidity()) return;
    const operation = ++this.operation;
    this.busy = true;
    const password = this.password.value;
    const register = this.mode === "register";
    const identifier = this.input(register ? "[data-account-register]" : "[data-account-login]").value.trim();
    this.pendingAuthentication = true;
    this.message(register ? "Creating account..." : "Signing in...");
    this.render();
    try {
      const session = register ? await this.accounts.register(identifier, password) : await this.accounts.login(identifier, password);
      if (operation === this.operation && this.accounts.isCurrentSession(session)) {
        this.password.value = "";
        this.message(register ? "Account created" : "Signed in");
      }
    } catch (cause) {
      if (operation === this.operation && !this.accounts.current) this.message(errorMessage(cause), true);
    } finally {
      if (operation === this.operation) {
        this.pendingAuthentication = false;
        this.busy = false;
        this.render();
      }
    }
  }

  private render(): void {
    const session = this.accounts.current;
    this.element("[data-account-details]").hidden = session === null;
    this.element("[data-account-form]").hidden = session !== null;
    if (session) {
      this.element("[data-account-handle]").textContent = session.identity.handle;
      this.input("[data-account-id]").value = session.identity.id;
      this.element("[data-account-expiry]").textContent = `Session expires ${new Date(session.expires_at).toLocaleString()}`;
    }
    const register = this.mode === "register";
    this.element("[data-account-login-label]").hidden = register;
    this.element("[data-account-handle-label]").hidden = !register;
    this.input("[data-account-login]").disabled = register || this.busy;
    this.input("[data-account-register]").disabled = !register || this.busy;
    this.input("[data-account-register]").required = register;
    this.password.disabled = this.busy;
    this.password.minLength = 1;
    this.password.autocomplete = register ? "new-password" : "current-password";
    const submit = this.element<HTMLButtonElement>("[data-account-submit]");
    submit.textContent = register ? "Create account" : "Sign in";
    for (const button of this.dialog.querySelectorAll<HTMLButtonElement>("button:not([data-account-close])")) {
      if (button.closest("[data-account-security]")) continue;
      button.disabled = this.busy;
      if (button.dataset.accountMode) button.setAttribute("aria-pressed", String(button.dataset.accountMode === this.mode));
    }
  }

  private message(text: string, error = false): void {
    const status = this.element("[data-account-status]");
    status.textContent = text;
    status.dataset.state = error ? "error" : "ready";
  }

  private get password(): HTMLInputElement { return this.input("[data-account-password]"); }
  private input(selector: string): HTMLInputElement { return this.element<HTMLInputElement>(selector); }
  private element<T extends HTMLElement = HTMLElement>(selector: string): T {
    return this.dialog.querySelector<T>(selector)!;
  }
}

function errorMessage(cause: unknown): string { return cause instanceof Error ? cause.message : "Request failed"; }
