import assert from "node:assert/strict";
import test from "node:test";
import { mediaKinds, mediaResource } from "./media-modules.mjs";

const { mediaKind } = mediaKinds;
const { resolveCardMedia } = mediaResource;
const api = new URL("https://babel.test/rpc");
const blob = (media_type = "video/mp4", hash = "a".repeat(64)) => ({ integrity: hash, media_type, uri: `babel://blobs/${hash}` });
const object = (resources, primary) => ({ id: "obj_test", resources, payload: primary ? { primary_resource: primary } : {} });

test("media classification excludes executable and unknown content without guessing extensions", () => {
  for (const [mime, kind] of [["IMAGE/PNG", "image"], ["audio/mpeg", "audio"], ["audio/wav", "audio"], ["video/mp4", "video"], ["video/webm", "video"]]) assert.equal(mediaKind(mime), kind);
  for (const mime of ["image/svg+xml", "text/html", "application/javascript", "audio/unknown", "video/mp4;codecs=avc1", ""]) assert.equal(mediaKind(mime), null);
});

test("local media resolves to object-bound binary URLs, never a hex download", () => {
  for (const [mime, kind] of [["image/png", "image"], ["audio/mpeg", "audio"], ["video/mp4", "video"]]) {
    const resource = blob(mime), result = resolveCardMedia(object([resource]), api);
    assert.equal(result.media, `https://babel.test/objects/obj_test/media/${resource.integrity}`);
    assert.equal(result.mediaKind, kind);
    assert.equal(result.mediaType, mime);
  }
});

test("the declared primary resource wins over an image thumbnail; invalid primary never substitutes other content", () => {
  const image = blob("image/png", "b".repeat(64)), video = blob();
  assert.equal(resolveCardMedia(object([image, video], video), api).mediaKind, "video");
  assert.equal(resolveCardMedia(object([image], video), api).media, null);
});

test("resource locations are bounded by their declaration and browser-safe schemes", () => {
  const resource = blob();
  for (const uri of ["javascript:alert(1)", "file:///secret", "blob:https://babel.test/foreign", "babel://blobs/other", "https://name:secret@example.com/file", "data:text/html;base64,YQ==", "/relative"]) {
    assert.equal(resolveCardMedia(object([{ ...resource, uri }]), api).media, null, uri);
  }
  for (const uri of ["https://media.test/video.mp4", "http://localhost/video.mp4", "data:video/mp4;base64,YQ=="]) {
    assert.equal(resolveCardMedia(object([{ ...resource, uri }]), api).media, uri);
  }
  assert.equal(resolveCardMedia(object([blob("image/svg+xml")]), api).media, null);
  assert.equal(resolveCardMedia(object([]), api).media, null);
});

test("albums preserve resource order, primary selection, and unique integrity", () => {
  const image = blob("image/png", "b".repeat(64));
  const video = blob();
  const audio = blob("audio/mpeg", "c".repeat(64));
  const result = resolveCardMedia(object([image, video, image, audio], video), api);
  assert.deepEqual(Array.from(result.mediaItems, item => item.integrity), [image.integrity, video.integrity, audio.integrity]);
  assert.equal(result.media, result.mediaItems[1].media);
  assert.equal(result.mediaKind, "video");
  for (const item of result.mediaItems) {
    assert.ok(item.media && item.mediaType && item.mediaKind && item.integrity);
  }
});

test("unsafe and unsupported later attachments are skipped without hiding safe later media", () => {
  const image = blob("image/png", "b".repeat(64));
  const video = blob();
  const unsafe = { ...blob("audio/mpeg", "c".repeat(64)), uri: "javascript:alert(1)" };
  const svg = blob("image/svg+xml", "d".repeat(64));
  const audio = { ...unsafe, uri: "https://media.test/audio.mp3" };
  const result = resolveCardMedia(object([image, unsafe, svg, video, audio, video]), api);
  assert.deepEqual(Array.from(result.mediaItems, item => item.mediaKind), ["image", "video", "audio"]);
  assert.equal(result.media, result.mediaItems[0].media);
});

test("invalid declared or implicit primary yields no collection, even when later media is safe", () => {
  const image = blob("image/png", "b".repeat(64));
  const unsafe = { ...blob(), uri: "file:///private/video.mp4" };
  const unsupported = blob("text/html", "c".repeat(64));
  for (const value of [
    object([unsafe, image]), object([unsafe, image], unsafe), object([unsupported, image], unsupported),
    object([image], blob()),
    ...[null, {}, "bad", [], { integrity: 1 }].map(primary_resource => ({ ...object([image]), payload: { primary_resource } })),
  ]) {
    const result = resolveCardMedia(value, api);
    assert.equal(result.media, null);
    assert.equal(result.mediaKind, null);
    assert.equal(result.mediaType, null);
    assert.equal(result.mediaItems.length, 0);
  }
});

test("object-bound resource paths encode object identifiers", () => {
  const resource = blob();
  const result = resolveCardMedia({ ...object([resource]), id: "obj/with ? delimiters" }, api);
  assert.equal(result.mediaItems[0].media, `https://babel.test/objects/obj%2Fwith%20%3F%20delimiters/media/${resource.integrity}`);
});

test("payload album order and membership win over shuffled outer resources and Surface images", () => {
  const image = blob("image/png", "b".repeat(64)), video = blob();
  const audio = blob("audio/mpeg", "c".repeat(64)), surface = blob("image/png", "d".repeat(64));
  const value = { ...object([surface, video, audio, image]), kind: "babel.media",
    payload: { resources: [image, audio, video, image], primary_resource: audio } };
  const result = resolveCardMedia(value, api);
  assert.deepEqual(Array.from(result.mediaItems, item => item.integrity), [image.integrity, audio.integrity, video.integrity]);
  assert.equal(result.media, result.mediaItems[1].media);
  assert.equal(result.mediaKind, "audio");
  const generic = resolveCardMedia(object(value.resources, audio), api);
  assert.deepEqual(Array.from(generic.mediaItems, item => item.integrity), [surface.integrity, video.integrity, audio.integrity, image.integrity]);
});

test("album entries must exactly match signed outer descriptors; safe later members remain accessible", () => {
  const image = blob("image/png", "b".repeat(64)), video = blob(), audio = blob("audio/mpeg", "c".repeat(64));
  for (const mismatch of [
    { ...video, uri: "https://media.test/unsigned.mp4" },
    { ...video, media_type: "image/png" },
    { ...video, integrity: "f".repeat(64) },
    { integrity: video.integrity }, null, "invalid",
  ]) {
    const result = resolveCardMedia({ ...object([video, audio, image]), kind: "babel.media",
      payload: { resources: [image, mismatch, audio], primary_resource: image } }, api);
    assert.deepEqual(Array.from(result.mediaItems, item => item.integrity), [image.integrity, audio.integrity]);
  }
});

test("unmatched primary and malformed or empty album declarations cannot fall back to outer media", () => {
  const image = blob("image/png", "b".repeat(64)), video = blob();
  const altered = [
    { ...video, uri: "https://media.test/unsigned.mp4" },
    { ...video, media_type: "audio/mpeg" },
    { ...video, integrity: "f".repeat(64) },
  ];
  const payloads = [
    { resources: [image], primary_resource: video },
    ...altered.map(primary_resource => ({ resources: [image, video], primary_resource })),
    ...altered.map(resource => ({ resources: [image, resource], primary_resource: resource })),
    ...[[], null, {}, "invalid"].map(resources => ({ resources, primary_resource: image })),
  ];
  for (const payload of payloads) {
    const result = resolveCardMedia({ ...object([video, image]), kind: "babel.media", payload }, api);
    assert.equal(result.media, null);
    assert.equal(result.mediaItems.length, 0);
  }
});

test("custom payload resources do not reinterpret generic objects as media albums", () => {
  const image = blob("image/png", "b".repeat(64)), video = blob();
  for (const kind of ["game.world", "babel.text", "custom.object"]) {
    for (const resources of [{ gold: 20 }, ["wood", "stone"], [video], [], null]) {
      const result = resolveCardMedia({ ...object([image, video]), kind, payload: { resources } }, api);
      assert.deepEqual(Array.from(result.mediaItems, item => item.integrity), [image.integrity, video.integrity]);
      assert.equal(result.media, result.mediaItems[0].media);
    }
  }
});

test("canonical albums reject duplicate outer hashes without choosing an ambiguous descriptor", () => {
  const image = blob("image/png", "b".repeat(64)), video = blob();
  const surface = blob("image/png", "c".repeat(64));
  for (const duplicate of [image, video, surface, { ...video, uri: "https://media.test/other.mp4" },
    { ...image, media_type: "audio/mpeg" }]) {
    const result = resolveCardMedia({ ...object([image, video, surface, duplicate]), kind: "babel.media",
      payload: { resources: [image, video], primary_resource: image } }, api);
    assert.equal(result.media, null);
    assert.equal(result.mediaItems.length, 0);
  }
});
