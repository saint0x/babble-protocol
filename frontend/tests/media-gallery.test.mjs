import assert from "node:assert/strict";
import test from "node:test";
import { album, albumItems, galleryHarness } from "./gallery-dom.mjs";

function setup(media = album, compact = false) {
  const h = galleryHarness();
  const gallery = h.createMediaGallery(media, compact);
  h.document.body.append(gallery);
  const [stage, controls, strip] = gallery.children;
  const [previous, position, next] = controls.children;
  return { ...h, gallery, stage, controls, strip, previous, position, next };
}

test("primary selection keeps resource order, stable controls, and cyclic navigation", () => {
  const h = setup({ ...album, ...albumItems[1] });
  assert.equal(h.gallery.dataset.mediaGallery, "full");
  assert.equal(h.stage.dataset.kind, "audio");
  assert.equal(h.position.textContent, "2 / 3");
  assert.deepEqual(h.strip.children.map(button => button.dataset.mediaIndex), ["0", "1", "2"]);
  assert.equal(h.strip.children[1].getAttribute("aria-pressed"), "true");
  assert.equal(h.strip.children[1].tabIndex, 0);
  h.next.click();
  assert.equal(h.stage.dataset.kind, "video");
  assert.equal(h.document.activeElement, h.next);
  h.next.click();
  assert.equal(h.stage.dataset.kind, "image");
  h.previous.click();
  assert.equal(h.position.textContent, "3 / 3");
  assert.equal(h.document.activeElement, h.previous);
  assert.equal(h.gallery.children[1], h.controls);
  assert.equal(h.next.dataset.mediaNext, "");
  assert.equal(h.previous.dataset.mediaPrevious, "");
});

test("selection keys move roving focus and expose the selected thumbnail without scrolling the post", () => {
  const h = setup();
  h.strip.children[2].offsetLeft = 260;
  h.strip.children[0].focus();
  const event = h.strip.children[0].emit("keydown", { key: "End" });
  assert.equal(event.defaultPrevented, true);
  assert.equal(event.stopped, true);
  assert.equal(h.position.textContent, "3 / 3");
  assert.equal(h.document.activeElement, h.strip.children[2]);
  assert.equal(h.document.activeElement.focusOptions.preventScroll, true);
  assert.equal(h.strip.scrollLeft, 72);
  h.strip.children[2].emit("keydown", { key: "ArrowLeft" });
  assert.equal(h.document.activeElement, h.strip.children[1]);
  h.strip.children[1].emit("keydown", { key: "Home" });
  assert.equal(h.document.activeElement, h.strip.children[0]);
  assert.equal(h.strip.scrollLeft, 0);
  assert.deepEqual(h.strip.children.map(button => button.tabIndex), [0, -1, -1]);
});

test("only gallery controls consume keys and pointers; native players and image swipes remain independent", () => {
  const h = setup();
  assert.equal(h.stage.emit("keydown", { key: "ArrowRight" }).stopped, false);
  assert.equal(h.stage.firstElementChild.firstElementChild.emit("pointerdown").stopped, false);
  assert.equal(h.next.emit("pointerdown").stopped, true);
  assert.equal(h.next.emit("keydown", { key: "ArrowRight", ctrlKey: true }).stopped, false);
  assert.equal(h.next.emit("keydown", { key: "Tab" }).stopped, false);
  assert.equal(h.next.emit("keydown", { key: "ArrowRight" }).stopped, true);
  const audio = h.stage.querySelectorAll("audio, video")[0];
  assert.equal(audio.emit("keydown", { key: "ArrowRight" }).stopped, false);
  assert.equal(h.position.textContent, "2 / 3");
});

test("a later primary thumbnail is revealed after mounting without moving focus", () => {
  const h = setup({ ...album, ...albumItems[2] });
  h.strip.offsetLeft = 100;
  h.strip.children[2].offsetLeft = 260;
  h.next.focus();
  h.flushFrames();
  assert.equal(h.strip.scrollLeft, 72);
  assert.equal(h.document.activeElement, h.next);
});

test("changing selection pauses and unloads native media without autoplay; detach and reattach retain the selection", () => {
  const h = setup({ ...album, ...albumItems[1] });
  const outgoing = h.stage.firstElementChild;
  const audio = h.stage.querySelectorAll("audio, video")[0];
  assert.equal(audio.src, albumItems[1].media);
  assert.equal(audio.autoplay, false);
  assert.equal(audio.controls, true);
  assert.equal(audio.preload, "metadata");
  audio.paused = false;
  h.next.click();
  assert.equal(audio.paused, true);
  assert.equal(audio.src, "");
  assert.equal(audio.loadCalls, 1);
  h.flush([outgoing], [h.stage.firstElementChild]);
  assert.equal(h.intersections[0].observed.has(audio), false);
  const video = h.stage.querySelectorAll("audio, video")[0];
  assert.equal(video.playsInline, true);
  assert.equal(video.autoplay, false);
  video.emit("loadedmetadata");
  assert.equal(h.stage.firstElementChild.dataset.state, "ready");
  h.gallery.remove();
  h.flush([h.gallery]);
  assert.equal(video.src, "");
  assert.equal(video.paused, true);
  h.document.body.append(h.gallery);
  h.flush([], [h.gallery]);
  assert.equal(video.src, albumItems[2].media);
  assert.equal(video.paused, true);
  assert.equal(h.position.textContent, "3 / 3");
});

test("images show loading, retry failures, and open the selected original in the existing viewer", () => {
  const second = { ...albumItems[0], media: "https://media.test/second.png", integrity: "d" };
  const h = setup({ ...album, mediaItems: [albumItems[0], second] });
  let frame = h.stage.firstElementChild;
  let [open, feedback] = frame.children;
  const image = open.firstElementChild;
  assert.equal(frame.dataset.state, "loading");
  assert.equal(image.hidden, true);
  image.emit("error");
  assert.equal(frame.dataset.state, "error");
  assert.equal(feedback.children[1].hidden, false);
  feedback.children[1].click();
  assert.equal(frame.dataset.state, "loading");
  image.emit("load");
  assert.equal(image.hidden, false);
  assert.equal(feedback.hidden, true);
  assert.equal(open.getAttribute("aria-busy"), "false");
  h.next.click();
  assert.equal(image.src, "");
  frame = h.stage.firstElementChild;
  open = frame.firstElementChild;
  open.click();
  assert.equal(h.viewers[0].media.src, second.media);
  assert.equal(h.viewers[0].media.caption, album.content);
  assert.equal(h.viewers[0].trigger, open);
});

test("compact galleries keep every item selectable and never render gallery controls for single media", () => {
  const h = setup(album, true);
  assert.equal(h.gallery.dataset.mediaGallery, "compact");
  for (let index = 0; index < albumItems.length; index++) {
    h.strip.children[index].click();
    assert.equal(h.stage.dataset.kind, albumItems[index].mediaKind);
    assert.equal(h.gallery.dataset.mediaSelected, String(index));
  }
  assert.throws(() => h.createMediaGallery({ ...album, mediaItems: [albumItems[0]] }), /multiple media/);
});
