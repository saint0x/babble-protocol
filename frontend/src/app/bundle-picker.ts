import { bundleEntries, inspectBundleFiles, parseBundleCapabilities, type BundleAttachment } from "./bundle-publication";
import { createElement, X } from "lucide";

export class BundlePicker {
  private readonly input: HTMLInputElement;
  private readonly entry: HTMLSelectElement;
  private readonly remove: HTMLButtonElement;
  private readonly status: HTMLElement;
  private readonly permissions: HTMLDetailsElement;
  private readonly declarations: HTMLTextAreaElement;
  private readonly permissionStatus: HTMLElement;
  private readonly permissionCount: HTMLElement;
  private readonly clipboard: HTMLInputElement;
  private readonly fullscreen: HTMLInputElement;
  private value: BundleAttachment | null = null;
  error: string | null = null;

  constructor(private readonly root: HTMLElement, changed: (value: BundleAttachment | null) => void) {
    this.input = required(root.querySelector<HTMLInputElement>("[data-compose-bundle]"));
    this.entry = required(root.querySelector<HTMLSelectElement>("[data-bundle-entry]"));
    this.remove = required(root.querySelector<HTMLButtonElement>("[data-clear-bundle]"));
    this.status = required(root.querySelector<HTMLElement>("[data-bundle-status]"));
    this.permissions = required(root.querySelector<HTMLDetailsElement>("[data-bundle-permissions]"));
    this.declarations = required(root.querySelector<HTMLTextAreaElement>("[data-bundle-capabilities]"));
    this.permissionStatus = required(root.querySelector<HTMLElement>("[data-bundle-permission-status]"));
    this.permissionCount = required(root.querySelector<HTMLElement>("[data-bundle-permission-count]"));
    this.clipboard = required(root.querySelector<HTMLInputElement>("[data-bundle-clipboard]"));
    this.fullscreen = required(root.querySelector<HTMLInputElement>("[data-bundle-fullscreen]"));
    this.remove.replaceChildren(createElement(X, { "aria-hidden": "true" }));
    this.input.addEventListener("change", () => {
      if (this.input.disabled || !this.input.files?.length) return;
      try {
        const value = inspectBundleFiles([...this.input.files]);
        changed(value);
      } catch (cause) {
        changed(null);
        this.error = cause instanceof Error ? cause.message : "Application files could not be read";
        this.input.setAttribute("aria-invalid", "true");
        this.status.dataset.state = "error";
        this.status.textContent = this.error;
        this.remove.hidden = false;
      }
    });
    this.entry.addEventListener("change", () => {
      if (!this.entry.disabled && this.value) changed({ ...this.value, entryPath: this.entry.value });
    });
    this.declarations.addEventListener("input", () => {
      if (!this.declarations.disabled && this.value) {
        changed({ ...this.value, capabilitiesText: this.declarations.value });
      }
    });
    for (const [input, id] of [[this.clipboard, "babble.clipboard.write"], [this.fullscreen, "babble.fullscreen.enter"]] as const) {
      input.addEventListener("change", () => {
        if (input.disabled || !this.value) return;
        try {
          const previous = parseBundleCapabilities(this.value.capabilitiesText ?? "[]");
          const next = previous.filter(request => request.id !== id || request.version !== 1);
          if (input.checked) next.push({ id, version: 1, scope: {} });
          changed({ ...this.value, capabilitiesText: JSON.stringify(next, null, 2) });
        } catch (cause) { this.showPermissionError(cause); }
      });
    }
    this.remove.addEventListener("click", () => {
      if (this.remove.disabled) return;
      changed(null);
      this.input.focus();
    });
  }

  render(value: BundleAttachment | null, enabled: boolean, pending: boolean): void {
    this.value = value;
    this.error = null;
    this.root.hidden = !enabled;
    this.input.disabled = !enabled || pending;
    this.entry.disabled = !enabled || pending;
    this.remove.disabled = !enabled || pending;
    this.input.value = "";
    this.input.removeAttribute("aria-invalid");
    this.entry.replaceChildren();
    this.entry.closest("label")!.hidden = !value;
    this.remove.hidden = !value;
    this.status.dataset.state = "ready";
    this.renderPermissions(value, enabled, pending);
    if (!value) {
      this.status.textContent = "";
      return;
    }
    for (const path of bundleEntries(value)) {
      const option = document.createElement("option");
      option.value = path;
      option.textContent = path;
      this.entry.append(option);
    }
    this.entry.value = value.entryPath;
    const bytes = value.files.reduce((total, file) => total + file.size, 0);
    this.status.textContent = `${value.files.length} files, ${(bytes / 1024).toFixed(1)} KiB`;
  }

  private renderPermissions(value: BundleAttachment | null, enabled: boolean, pending: boolean): void {
    this.permissions.hidden = !value || !enabled;
    this.declarations.disabled = !value || !enabled || pending;
    const text = value?.capabilitiesText ?? "[]";
    if (this.declarations.value !== text) this.declarations.value = text;
    this.declarations.removeAttribute("aria-invalid");
    this.permissionStatus.textContent = "";
    this.permissionStatus.dataset.state = "ready";
    this.clipboard.checked = false;
    this.fullscreen.checked = false;
    this.clipboard.disabled = this.declarations.disabled;
    this.fullscreen.disabled = this.declarations.disabled;
    this.permissionCount.textContent = "0";
    if (!value) { this.permissions.open = false; return; }
    try {
      const requests = parseBundleCapabilities(text);
      this.permissionCount.textContent = String(requests.length);
      this.clipboard.checked = requests.some(request => request.id === "babble.clipboard.write" && request.version === 1);
      this.fullscreen.checked = requests.some(request => request.id === "babble.fullscreen.enter" && request.version === 1);
    } catch (cause) { this.showPermissionError(cause); }
  }

  private showPermissionError(cause: unknown): void {
    this.error = cause instanceof Error ? cause.message : "Permissions could not be read";
    this.declarations.setAttribute("aria-invalid", "true");
    this.permissionStatus.dataset.state = "error";
    this.permissionStatus.textContent = this.error;
    this.permissionCount.textContent = "!";
    this.clipboard.disabled = true;
    this.fullscreen.disabled = true;
    this.permissions.open = true;
  }
}

function required<T>(value: T | null): T {
  if (!value) throw new Error("Application picker is missing a required control");
  return value;
}
