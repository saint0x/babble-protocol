import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { canonicalFloat, canonicalValueHex } from "../dist/index.js";

const fixtures = JSON.parse(
  readFileSync(new URL("../../fixtures/protocol/v1/fixtures.json", import.meta.url), "utf8"),
);

test("canonical encoder matches Rust fixture bytes", () => {
  const sample = {
    zeta: [1, "1", canonicalFloat(1.0)],
    alpha: {
      nested: true,
      empty: null,
    },
  };

  assert.equal(fixtures.canonical_encoding.version, "babel.canonical.v1");
  assert.equal(canonicalValueHex(sample), fixtures.canonical_encoding.bytes_hex);
});

test("signed bundle inventories match the Rust canonical commitment bytes", () => {
  const fixture = fixtures.bundle_manifest;
  assert.equal(fixture.version, "babel.canonical.v1");
  assert.equal(canonicalValueHex(fixture.sample), fixture.bytes_hex);
  const changed = structuredClone(fixture.sample);
  changed.files[0].size_bytes += 1;
  assert.notEqual(canonicalValueHex(changed), fixture.bytes_hex);
});

test("canonical encoder preserves type boundaries and UTF-8 key ordering", () => {
  assert.notEqual(canonicalValueHex("1"), canonicalValueHex(1));
  assert.notEqual(canonicalValueHex(1), canonicalValueHex(canonicalFloat(1.0)));
  assert.equal(
    canonicalValueHex({ beta: [2, 3], alpha: { z: true, a: null } }),
    canonicalValueHex({ alpha: { a: null, z: true }, beta: [2, 3] }),
  );
});
