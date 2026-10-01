import { Clipboard, Maximize, X, createElement } from "lucide";

export type BrowserAction = { readonly kind: "clipboard"; readonly text: string } | {
  readonly kind: "fullscreen"; readonly navigationUI: FullscreenNavigationUI; readonly targetHint: string | null;
};
export type BrowserPromptOutcome = { readonly kind: "completed" | "denied" | "cancelled" } | {
  readonly kind: "failed"; readonly error: unknown; readonly nativeStarted: boolean;
};
interface Options {
  readonly target: HTMLElement;
  readonly label: string;
  readonly actor: string;
  readonly live: () => boolean;
}
interface Operation {
  readonly authorize: () => Promise<void>;
  readonly execute: () => Promise<void>;
  readonly cancel: () => void;
  readonly signal: AbortSignal;
}

/** Server authorization and native activation are separate gestures in the same dialog. */
export function promptBrowserAction(options: Options, action: BrowserAction, operation: Operation): Promise<BrowserPromptOutcome> {
  const { target } = options, doc = target.ownerDocument;
  const dialog = doc.createElement("dialog");
  dialog.className = "host-action-dialog";
  dialog.setAttribute("data-host-action-dialog", "");
  dialog.setAttribute("data-host-action-stage", "consent");
  dialog.setAttribute("aria-label", action.kind === "clipboard" ? "Copy to clipboard?" : "Enter fullscreen?");
  const previous = doc.activeElement;
  const source = doc.createElement("p");
  source.className = "host-action-source";
  source.textContent = options.label.slice(0, 160);
  const heading = doc.createElement("h2");
  heading.textContent = action.kind === "clipboard" ? "Copy to clipboard?" : "Enter fullscreen?";
  const detail = doc.createElement("p");
  detail.className = "host-action-detail";
  detail.textContent = action.kind === "clipboard"
    ? "This will replace your clipboard with the text below."
    : "This app will fill your screen. You can leave fullscreen with Escape.";
  const actor = doc.createElement("p");
  actor.className = "host-action-source";
  actor.textContent = `Requested for ${options.actor.slice(0, 160)}`;
  const close = doc.createElement("button");
  close.type = "button";
  close.className = "host-action-close";
  close.title = "Decline request";
  close.setAttribute("aria-label", "Decline request");
  close.append(createElement(X, { "aria-hidden": "true" }));
  dialog.append(close, source, heading, detail);
  if (action.kind === "clipboard") {
    const preview = doc.createElement("pre");
    preview.className = "host-action-preview";
    preview.tabIndex = 0;
    preview.setAttribute("aria-label", "Text to copy");
    preview.textContent = action.text || "(Empty text)";
    dialog.append(preview);
  }
  const status = doc.createElement("p");
  status.className = "host-action-status";
  status.setAttribute("role", "status");
  status.setAttribute("aria-live", "polite");
  const footer = doc.createElement("footer");
  const cancel = doc.createElement("button");
  cancel.type = "button";
  cancel.setAttribute("data-host-action-cancel", "");
  cancel.textContent = "Cancel";
  const confirm = doc.createElement("button");
  confirm.type = "button";
  confirm.setAttribute("data-host-action-allow", "");
  confirm.className = "host-action-confirm";
  confirm.append(createElement(action.kind === "clipboard" ? Clipboard : Maximize, { "aria-hidden": "true" }));
  const label = doc.createElement("span");
  label.textContent = "Allow once";
  confirm.append(label);
  footer.append(cancel, confirm);
  dialog.append(actor, status, footer);

  return new Promise(resolve => {
    let phase: "consent" | "authorizing" | "ready" | "executing" = "consent";
    let settled = false, removed = false;
    const live = () => {
      try { return !operation.signal.aborted && options.live(); }
      catch { return false; }
    };
    const isolatedEvents = ["keydown", "keyup", "keypress", "pointerdown", "pointermove", "pointerup", "pointercancel",
      "touchstart", "touchmove", "touchend", "click", "wheel"];
    const cleanup = () => {
      if (removed) return;
      removed = true;
      operation.signal.removeEventListener("abort", abort);
      dialog.removeEventListener("cancel", deny);
      dialog.removeEventListener("close", deny);
      for (const name of isolatedEvents) dialog.removeEventListener(name, isolate);
      cancel.removeEventListener("click", deny);
      close.removeEventListener("click", deny);
      confirm.removeEventListener("click", accept);
      if (dialog.open) dialog.close();
      dialog.remove();
      if (previous instanceof HTMLElement && previous.isConnected && !previous.closest("[inert]") && doc.visibilityState === "visible") {
        previous.focus({ preventScroll: true });
      }
    };
    const finish = (outcome: BrowserPromptOutcome) => {
      if (settled) return;
      settled = true;
      cleanup();
      resolve(outcome);
    };
    const deny = (event: Event) => {
      event.preventDefault();
      event.stopPropagation();
      if (phase === "consent" || phase === "ready") finish({ kind: phase === "consent" ? "denied" : "cancelled" });
      else { operation.cancel(); abort(); }
    };
    const abort = () => {
      cleanup();
      // Keep ownership of in-flight work until it settles; never admit a duplicate.
      if (phase === "consent" || phase === "ready") finish({ kind: "cancelled" });
    };
    const isolate = (event: Event) => { event.stopPropagation(); };
    const busy = (value: boolean) => {
      confirm.disabled = value;
      dialog.setAttribute("aria-busy", String(value));
    };
    const accept = () => {
      if (settled || phase === "authorizing" || phase === "executing") return;
      if (!live()) { finish({ kind: "cancelled" }); return; }
      busy(true);
      if (phase === "consent") {
        phase = "authorizing";
        dialog.setAttribute("data-host-action-stage", phase);
        label.textContent = "Authorizing";
        void Promise.resolve().then(operation.authorize).then(() => {
          if (!live()) { finish({ kind: "cancelled" }); return; }
          phase = "ready";
          dialog.setAttribute("data-host-action-stage", phase);
          status.textContent = action.kind === "clipboard" ? "Approved for one copy." : "Approved for one fullscreen request.";
          label.textContent = action.kind === "clipboard" ? "Copy once" : "Enter fullscreen";
          busy(false);
          confirm.focus({ preventScroll: true });
        }, error => finish({ kind: "failed", error, nativeStarted: false }));
        return;
      }
      phase = "executing";
      dialog.setAttribute("data-host-action-stage", phase);
      label.textContent = action.kind === "clipboard" ? "Copying" : "Opening";
      let native: Promise<void>;
      try {
        // Invoke synchronously in this fresh click; awaiting the server loses activation.
        native = operation.execute();
      } catch (error) { finish({ kind: "failed", error, nativeStarted: true }); return; }
      void native.then(() => finish({ kind: "completed" }), error => finish({ kind: "failed", error, nativeStarted: true }));
    };
    operation.signal.addEventListener("abort", abort, { once: true });
    dialog.addEventListener("cancel", deny);
    dialog.addEventListener("close", deny);
    for (const name of isolatedEvents) dialog.addEventListener(name, isolate);
    cancel.addEventListener("click", deny);
    close.addEventListener("click", deny);
    confirm.addEventListener("click", accept);
    target.append(dialog);
    try { dialog.showModal(); cancel.focus({ preventScroll: true }); }
    catch (error) { finish({ kind: "failed", error, nativeStarted: false }); }
    if (!live()) abort();
  });
}
