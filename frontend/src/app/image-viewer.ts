import { createElement, Maximize, RotateCcw, X, ZoomIn, ZoomOut } from "lucide";
import "../styles/image-viewer.css";

interface ImageContent {
  readonly src: string;
  readonly alt: string;
  readonly caption: string;
}

let active: HTMLDialogElement | null = null;

export function imageSize(width: number, height: number, viewportWidth: number, viewportHeight: number, zoom: number) {
  if (![width, height, viewportWidth, viewportHeight, zoom].every(value => Number.isFinite(value) && value > 0)) return null;
  const fit = Math.min(viewportWidth / width, viewportHeight / height, 1);
  const scale = fit * Math.min(Math.max(zoom, 1), 4);
  return { width: width * scale, height: height * scale };
}

export function openImageViewer(content: ImageContent, trigger: HTMLElement): HTMLDialogElement {
  active?.close();
  const dialog = document.createElement("dialog");
  dialog.className = "image-viewer";
  dialog.setAttribute("aria-label", "Image viewer");
  const header = document.createElement("header");
  header.className = "image-viewer-toolbar";
  const title = document.createElement("p");
  title.className = "image-viewer-title";
  title.textContent = content.alt;
  const controls = document.createElement("div");
  controls.className = "image-viewer-controls";
  const out = iconButton("Zoom out", ZoomOut);
  const level = document.createElement("output");
  level.setAttribute("aria-label", "Zoom level");
  const into = iconButton("Zoom in", ZoomIn);
  const fit = iconButton("Fit image", Maximize);
  const close = iconButton("Close image", X);
  controls.append(out, level, into, fit, close);
  header.append(title, controls);

  const stage = document.createElement("div");
  stage.className = "image-viewer-stage";
  stage.tabIndex = 0;
  stage.setAttribute("aria-label", "Full image");
  const canvas = document.createElement("div");
  canvas.className = "image-viewer-canvas";
  const image = document.createElement("img");
  image.alt = content.alt;
  image.draggable = false;
  image.referrerPolicy = "no-referrer";
  canvas.append(image);
  stage.append(canvas);
  const status = document.createElement("p");
  status.className = "image-viewer-status";
  status.setAttribute("role", "status");
  const retry = iconButton("Retry image", RotateCcw);
  retry.classList.add("image-viewer-retry");
  retry.hidden = true;
  const caption = document.createElement("p");
  caption.className = "image-viewer-caption";
  caption.textContent = content.caption;
  caption.hidden = !content.caption;
  dialog.append(header, stage, status, retry, caption);

  let zoom = 1;
  let loaded = false;
  let disposed = false;
  let pan: { id: number; x: number; y: number; left: number; top: number } | null = null;
  const render = () => {
    const size = loaded ? imageSize(image.naturalWidth, image.naturalHeight, stage.clientWidth, stage.clientHeight, zoom) : null;
    if (size) {
      image.style.width = `${size.width}px`;
      image.style.height = `${size.height}px`;
    }
    image.hidden = !size;
    level.value = zoom === 1 ? "Fit" : `${zoom}×`;
    out.disabled = !loaded || zoom === 1;
    into.disabled = !loaded || zoom === 4;
    fit.disabled = !loaded || zoom === 1;
    stage.dataset.zoomed = String(zoom > 1);
    stage.setAttribute("aria-busy", String(!loaded && retry.hidden));
  };
  const setZoom = (next: number) => {
    if (!loaded) return;
    const before = { width: image.width, height: image.height };
    const centerX = (stage.scrollLeft + stage.clientWidth / 2 - Math.max(0, (stage.clientWidth - before.width) / 2)) / before.width;
    const centerY = (stage.scrollTop + stage.clientHeight / 2 - Math.max(0, (stage.clientHeight - before.height) / 2)) / before.height;
    zoom = Math.min(Math.max(next, 1), 4);
    render();
    stage.scrollLeft = centerX * image.width - stage.clientWidth / 2;
    stage.scrollTop = centerY * image.height - stage.clientHeight / 2;
  };
  const load = () => {
    loaded = false;
    retry.hidden = true;
    status.textContent = "Loading image";
    status.hidden = false;
    render();
    image.src = content.src;
  };
  image.addEventListener("load", () => {
    if (disposed) return;
    loaded = true;
    status.hidden = true;
    render();
  });
  image.addEventListener("error", () => {
    if (disposed) return;
    loaded = false;
    status.textContent = "Image could not be loaded";
    status.hidden = false;
    retry.hidden = false;
    render();
  });
  close.addEventListener("click", () => dialog.close());
  retry.addEventListener("click", load);
  out.addEventListener("click", () => setZoom(zoom / 2));
  into.addEventListener("click", () => setZoom(zoom * 2));
  fit.addEventListener("click", () => setZoom(1));
  image.addEventListener("dblclick", () => setZoom(zoom === 1 ? 2 : 1));
  dialog.addEventListener("keydown", event => {
    if (event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.key === "+" || event.key === "=") { event.preventDefault(); setZoom(zoom * 2); }
    if (event.key === "-") { event.preventDefault(); setZoom(zoom / 2); }
    if (event.key === "0") { event.preventDefault(); setZoom(1); }
    if (event.key === "Escape") { event.preventDefault(); dialog.close(); }
    event.stopPropagation();
  });
  stage.addEventListener("pointerdown", event => {
    if (event.pointerType !== "mouse" || event.button !== 0 || !event.isPrimary || zoom === 1 || pan) return;
    pan = { id: event.pointerId, x: event.clientX, y: event.clientY, left: stage.scrollLeft, top: stage.scrollTop };
    stage.setPointerCapture(event.pointerId);
    stage.dataset.panning = "true";
    event.preventDefault();
  });
  stage.addEventListener("pointermove", event => {
    if (!pan || event.pointerId !== pan.id) return;
    stage.scrollLeft = pan.left - (event.clientX - pan.x);
    stage.scrollTop = pan.top - (event.clientY - pan.y);
  });
  const release = () => {
    const previous = pan;
    pan = null;
    delete stage.dataset.panning;
    if (previous && stage.hasPointerCapture(previous.id)) stage.releasePointerCapture(previous.id);
  };
  stage.addEventListener("pointerup", release);
  stage.addEventListener("pointercancel", release);
  stage.addEventListener("lostpointercapture", release);
  const resize = new ResizeObserver(render);
  dialog.addEventListener("close", () => {
    disposed = true;
    release();
    resize.disconnect();
    image.removeAttribute("src");
    dialog.remove();
    if (active === dialog) {
      active = null;
      if (trigger.isConnected && !trigger.closest("[inert]")) trigger.focus({ preventScroll: true });
    }
  }, { once: true });
  document.body.append(dialog);
  active = dialog;
  dialog.showModal();
  resize.observe(stage);
  load();
  close.focus({ preventScroll: true });
  return dialog;
}

function iconButton(label: string, icon: typeof X): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.title = label;
  button.setAttribute("aria-label", label);
  button.append(createElement(icon, { "aria-hidden": "true" }));
  return button;
}
