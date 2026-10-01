import assert from "node:assert/strict";
import test from "node:test";
import {
  PERSONALIZATION_SYNC_ALGORITHM,
  PERSONALIZATION_SYNC_DATA_CLASS,
  PERSONALIZATION_SYNC_VERSION,
  createLocalUserModel,
  createPersonalizationFilter,
  createPersonalizationSyncRecipient,
  personalizeCandidates,
  summarizeDiscoveryObject,
  validateEncryptedLocalUserModelEnvelope,
} from "../dist/index.js";

test("personalizeCandidates reranks public Lens output with local private preferences", () => {
  const model = createLocalUserModel({
    model_revision: "device-rev-7",
    interests: ["crdt collaborative canvas"],
    expertise: ["distributed systems"],
    creator_affinity: { id_alice: 0.9 },
    novelty_tolerance: 0.7,
    exploration_preference: 0.8,
    evidence_preference: 0.9,
    contradiction_tolerance: 0.4,
  });
  const celebrity = rankedCandidate("obj_celebrity", 0.92, "2026-09-27T00:00:10Z", 0.4, 0.2, 0.4);
  const crdt = rankedCandidate("obj_crdt", 0.5, "2026-09-27T00:00:20Z", 0.75, 0.8, 0.8);
  const trace = personalizeCandidates(model, [celebrity, crdt], [
    summary("obj_celebrity", "id_other", "celebrity recap", ["pop"]),
    summary("obj_crdt", "id_alice", "collaborative CRDT canvas for distributed systems", ["protocol"]),
  ]);

  assert.equal(trace.privacy_boundary, "local_only");
  assert.equal(trace.model_revision, "device-rev-7");
  assert.equal(trace.ranked[0].ranked.candidate.object_id, "obj_crdt");
  assert.ok(trace.ranked[0].reasons.some((reason) => reason.signal === "private.interest_match"));
  assert.ok(!JSON.stringify(trace).includes("distributed systems"));
});

test("personalizeCandidates applies local hidden author and muted term filters", () => {
  const model = createLocalUserModel({
    hidden_authors: ["id_blocked"],
    hidden_terms: ["spoiler"],
    muted_terms: ["ragebait"],
    novelty_tolerance: 0.5,
    exploration_preference: 0.5,
    evidence_preference: 0.5,
    contradiction_tolerance: 1,
  });
  const trace = personalizeCandidates(
    model,
    [
      rankedCandidate("obj_hidden", 0.8, "2026-09-27T00:00:10Z", 0.5, 0.5, 0.5),
      rankedCandidate("obj_muted", 0.7, "2026-09-27T00:00:20Z", 0.5, 0.5, 0.5),
      rankedCandidate("obj_visible", 0.6, "2026-09-27T00:00:30Z", 0.5, 0.5, 0.5),
    ],
    [
      summary("obj_hidden", "id_blocked", "neutral text", []),
      summary("obj_muted", "id_ok", "ragebait spoiler", []),
      summary("obj_visible", "id_ok", "plain protocol note", []),
    ],
  );

  assert.deepEqual(trace.ranked.map((entry) => entry.ranked.candidate.object_id), ["obj_visible"]);
  assert.equal(trace.filtered.length, 2);
  assert.ok(trace.filtered.some((entry) => entry.object_id === "obj_hidden" && entry.reasons.includes("private.hidden_author")));
  assert.ok(trace.filtered.some((entry) => entry.object_id === "obj_muted" && entry.reasons.includes("private.hidden_term")));
  assert.ok(trace.filtered.some((entry) => entry.object_id === "obj_muted" && entry.reasons.includes("private.muted_term")));
});

test("personalizeCandidates uses deterministic ObjectId ordering for equal scores", () => {
  const model = createLocalUserModel({
    novelty_tolerance: 0.5,
    exploration_preference: 0.5,
    evidence_preference: 0.5,
    contradiction_tolerance: 0.5,
  });
  const trace = personalizeCandidates(
    model,
    [
      rankedCandidate("obj_b", 0.5, "2026-09-27T00:00:10Z", 0.5, 0.5, 0.5),
      rankedCandidate("obj_a", 0.5, "2026-09-27T00:00:10Z", 0.5, 0.5, 0.5),
    ],
    [],
  );

  assert.deepEqual(trace.ranked.map((entry) => entry.ranked.candidate.object_id), ["obj_a", "obj_b"]);
});

test("summarizeDiscoveryObject extracts text and topics from protocol Objects", () => {
  const object = {
    id: "obj_note",
    author: "id_alice",
    kind: "babble.text.v1",
    payload: { text: "A protocol note", topics: ["protocol", 7] },
  };

  assert.deepEqual(summarizeDiscoveryObject(object), {
    object_id: "obj_note",
    author: "id_alice",
    kind: "babble.text.v1",
    text: "A protocol note",
    topics: ["protocol"],
  });
});

test("createPersonalizationFilter agrees with ranked trace for every local filter reason", () => {
  const input = {
    model_revision: " private-revision ",
    hidden_authors: ["id_blocked"],
    hidden_terms: ["SPOILER", "classified"],
    muted_terms: ["RAGEBAIT", "media"],
  };
  const summaries = [
    summary("obj_author", "id_blocked", "neutral", []),
    summary("obj_hidden", "id_ok", "A spoiler!", []),
    summary("obj_muted", "id_ok", "ragebait", []),
    summary("obj_all", "id_blocked", "spoiler and ragebait", []),
    summary("obj_topic", "id_ok", "neutral", ["CLASSIFIED"]),
    { ...summary("obj_kind", "id_ok", "neutral", []), kind: "babble.media.v1" },
    summary("obj_visible", "id_ok", "neutral", []),
  ];
  const candidates = [...summaries.map((entry) => entry.object_id), "obj_unknown"]
    .map((id) => rankedCandidate(id, 0.5, "2026-09-27T00:00:00Z", 0.5, 0.5, 0.5));
  const allow = createPersonalizationFilter(input);
  const trace = personalizeCandidates(input, candidates, summaries);
  const expectedReasons = {
    obj_all: ["private.hidden_author", "private.hidden_term", "private.muted_term"],
    obj_author: ["private.hidden_author"],
    obj_hidden: ["private.hidden_term"],
    obj_kind: ["private.muted_term"],
    obj_muted: ["private.muted_term"],
    obj_topic: ["private.hidden_term"],
  };
  assert.deepEqual(trace.filtered, Object.entries(expectedReasons).map(([object_id, reasons]) => ({ object_id, reasons })));
  for (const candidate of candidates) {
    const id = candidate.candidate.object_id;
    assert.equal(allow(summaries.find((entry) => entry.object_id === id) ?? null),
      trace.ranked.some((entry) => entry.ranked.candidate.object_id === id), id);
  }
  assert.deepEqual(trace, personalizeCandidates(input, [...candidates].reverse(), [...summaries].reverse()));
  assert.equal(trace.model_revision, "private-revision");
  assert.equal(trace.privacy_boundary, "local_only");
  assert.ok(!JSON.stringify(trace).includes("classified"));
  assert.ok(!JSON.stringify(trace).includes("id_blocked"));
});

test("createPersonalizationFilter captures normalized preferences once without mutating its input", () => {
  const input = { hidden_authors: ["id_blocked"], hidden_terms: ["SPOILER!"], muted_terms: ["RAGEBAIT"] };
  const original = structuredClone(input);
  const allow = createPersonalizationFilter(input);
  assert.deepEqual(input, original);
  input.hidden_authors.length = 0;
  input.hidden_terms[0] = "neutral";
  input.muted_terms.length = 0;
  assert.equal(allow(summary("obj_author", "id_blocked", "", [])), false);
  assert.equal(allow(summary("obj_hidden", "id_ok", "spoiler", [])), false);
  assert.equal(allow(summary("obj_muted", "id_ok", "ragebait", [])), false);
  assert.equal(allow(summary("obj_allowed", "id_ok", "neutral", [])), true);
});

test("unknown object summaries remain allowed consistently with ranked traces", () => {
  const input = { hidden_authors: ["id_blocked"], hidden_terms: ["secret"], muted_terms: ["text"] };
  assert.equal(createPersonalizationFilter(input)(null), true);
  const candidate = rankedCandidate("obj_unknown", 0.5, "2026-09-27T00:00:00Z", 0.5, 0.5, 0.5);
  const trace = personalizeCandidates(input, [candidate], []);
  assert.deepEqual(trace.filtered, []);
  assert.equal(trace.ranked[0].ranked.candidate.object_id, "obj_unknown");
});

test("Unicode canonical normalization and case matching are shared by filters and ranking", () => {
  const pairs = [
    ["CAF\u00c9", "cafe\u0301"],
    ["CAFE\u0301", "caf\u00e9"],
    ["\u041f\u0420\u0418\u0412\u0415\u0422", "\u043f\u0440\u0438\u0432\u0435\u0442"],
    ["\u0419", "\u0438\u0306"],
    ["\u039a\u038c\u03a3\u039c\u039f\u03a3", "\u03ba\u03bf\u0301\u03c3\u03bc\u03bf\u03c2"],
    ["\u4e2d\u6587", "\u4e2d\u6587"],
    ["\u0915\u093f", "\u0915\u093f"],
  ];
  for (const [term, text] of pairs) {
    const entry = summary("obj_unicode", "id_ok", text, []);
    const candidate = rankedCandidate(entry.object_id, 0.5, "2026-09-27T00:00:00Z", 0.5, 0.5, 0.5);
    assert.deepEqual(createLocalUserModel({ interests: [term] }).interests,
      createLocalUserModel({ interests: [text] }).interests);
    for (const field of ["hidden_terms", "muted_terms"]) {
      const input = { [field]: [term] };
      assert.equal(createPersonalizationFilter(input)(entry), false, `${field}: ${term}`);
      assert.equal(personalizeCandidates(input, [candidate], [entry]).filtered.length, 1);
    }
    const ranked = personalizeCandidates({ interests: [term], expertise: [term] }, [candidate], [entry]).ranked[0];
    assert.equal(ranked.reasons.find((reason) => reason.signal === "private.interest_match").contribution, 0.18);
    assert.equal(ranked.reasons.find((reason) => reason.signal === "private.expertise_match").contribution, 0.09);
  }
  const terms = pairs.flat().concat(["x", "7"]);
  const normalized = createLocalUserModel({ interests: terms }).interests;
  assert.deepEqual(normalized, createLocalUserModel({ interests: [...terms].reverse() }).interests);
  assert.equal(normalized.length, 8);
  assert.ok(normalized.includes("x"));
  assert.ok(normalized.includes("7"));
});

test("Unicode token filters respect punctuation, single characters, and whole-token boundaries", () => {
  const cases = [
    ["x", "(X)!", false],
    ["x", "extra", true],
    ["7", "[7]", false],
    ["7", "77", true],
    ["art", "earth cartography", true],
    ["art", "earth\u2014ART,cartography", false],
    ["spoiler", "spoilers", true],
    ["spoiler", "plot_spoiler-free", false],
    ["\u00e9", "(E\u0301)", false],
    ["\u00e9", "caf\u00e9", true],
    ["\u4e2d", "\u4e2d\u3002", false],
    ["\u4e2d", "\u4e2d\u6587", true],
    ["\u4e2d\u6587", "\u4e2d\u6587\u3001\u65e5\u672c\u8a9e", false],
    ["\u4e2d\u6587", "\u5b66\u4e2d\u6587", true],
    ["!!!", "anything", true],
  ];
  for (const [term, text, allowed] of cases) {
    const entry = summary("obj_boundary", "id_ok", text, []);
    const candidate = rankedCandidate(entry.object_id, 0.5, "2026-09-27T00:00:00Z", 0.5, 0.5, 0.5);
    const input = { muted_terms: [term] };
    assert.equal(createPersonalizationFilter(input)(entry), allowed, `${term}: ${text}`);
    assert.equal(personalizeCandidates(input, [candidate], [entry]).filtered.length, allowed ? 0 : 1);
  }
});

test("every human-readable media field participates in filtering, including full descriptions", () => {
  const payload = {
    text: "Caption with spoiler",
    title: "A ragebait title",
    description: `${"A long caption. ".repeat(2000)}classified ending`,
    alt: "An image of a secret",
  };
  const entry = summarizeDiscoveryObject({ id: "obj_media", author: "id_ok", kind: "babble.media.v1", payload });
  assert.equal(entry.text, [payload.text, payload.title, payload.description, payload.alt].join("\n"));
  const candidate = rankedCandidate(entry.object_id, 0.5, "2026-09-27T00:00:00Z", 0.5, 0.5, 0.5);
  for (const term of ["spoiler", "ragebait", "classified", "secret"]) {
    for (const field of ["hidden_terms", "muted_terms"]) {
      const input = { [field]: [term] };
      assert.equal(createPersonalizationFilter(input)(entry), false, `${field}: ${term}`);
      assert.equal(personalizeCandidates(input, [candidate], [entry]).filtered.length, 1);
    }
  }
});

test("summary text projection is explicit, unique, ordered, and excludes opaque or private fields", () => {
  const object = {
    id: "obj_projection", author: "id_ok", kind: "babble.media.v1",
    metadata: { title: "secretmetadata" },
    capabilities: [{ text: "secretcapability" }],
    resources: [{ uri: "https://example.test/secretresource" }],
    payload: {
      text: "Public caption", title: "Public title", description: "Public caption", alt: "Public alt",
      topics: ["publictopic", { text: "secrettopic" }, 4],
      data: { text: "secretdata" },
      resources: [{ description: "secretresource", url: "https://example.test/secreturl" }],
      url: "https://example.test/secreturl",
      capabilities: ["secretcapability"],
      metadata: { alt: "secretmetadata" },
      private: { text: "secretprivate" },
      private_text: "secretfield",
    },
  };
  const original = structuredClone(object);
  const entry = summarizeDiscoveryObject(object);
  assert.deepEqual(entry, {
    object_id: object.id, author: object.author, kind: object.kind,
    text: "Public caption\nPublic title\nPublic alt", topics: ["publictopic"],
  });
  assert.deepEqual(object, original);
  assert.equal(createPersonalizationFilter({ hidden_terms: [
    "secretdata secretresource secreturl secretcapability secretmetadata secretprivate secretfield secrettopic",
  ] })(entry), true);
  assert.equal(createPersonalizationFilter({ hidden_terms: ["publictopic"] })(entry), false);
});

test("summary projection ignores non-string fields and unsupported payload shapes", () => {
  for (const payload of [null, [], "secret", 3, { text: { text: "secret" }, title: ["secret"], description: 7, alt: null }]) {
    const entry = summarizeDiscoveryObject({ id: "obj_unknown", author: "id_ok", kind: "custom.unknown", payload });
    assert.deepEqual(entry, { object_id: "obj_unknown", author: "id_ok", kind: "custom.unknown", text: "", topics: [] });
    assert.equal(createPersonalizationFilter({ hidden_terms: ["secret"] })(entry), true);
  }
  const entry = summarizeDiscoveryObject({
    id: "obj_description", author: "id_ok", kind: "babble.media.v1",
    payload: { text: "", title: "", description: "A complete description", alt: "" },
  });
  assert.equal(entry.text, "A complete description");
});

test("personalization sync helpers validate encrypted envelope metadata", () => {
  const recipient = createPersonalizationSyncRecipient({
    identity_id: "id_0000000000000000000000000000000000000000000000000000000000000000",
    device_id: "desktop-main",
  });
  const envelope = {
    version: PERSONALIZATION_SYNC_VERSION,
    data_class: PERSONALIZATION_SYNC_DATA_CLASS,
    algorithm: PERSONALIZATION_SYNC_ALGORITHM,
    recipient,
    model_revision: "rev-private-7",
    exported_at: "2026-09-27T00:00:00Z",
    nonce: "00".repeat(24),
    ciphertext: "ab".repeat(48),
  };

  assert.equal(validateEncryptedLocalUserModelEnvelope(envelope, recipient), envelope);
  assert.throws(() =>
    validateEncryptedLocalUserModelEnvelope(envelope, {
      identity_id: recipient.identity_id,
      device_id: "phone-main",
    }),
  );
  assert.throws(() =>
    createPersonalizationSyncRecipient({
      identity_id: recipient.identity_id,
      device_id: "bad device id",
    }),
  );
  assert.throws(() =>
    validateEncryptedLocalUserModelEnvelope({
      ...envelope,
      nonce: "00",
    }),
  );
});

function summary(objectId, author, text, topics) {
  return {
    object_id: objectId,
    author,
    kind: "babble.text.v1",
    text,
    topics,
  };
}

function rankedCandidate(objectId, score, createdAt, novelty, exploration, evidenceQuality) {
  return {
    candidate: {
      object_id: objectId,
      source: "Exploration",
      sources: [{ source: "Exploration", weight: 1 }],
      created_at: createdAt,
      signals: {
        social_distance: 0.5,
        followed_author: false,
        relevance: 0.5,
        novelty,
        evidence_quality: evidenceQuality,
        contradiction: 0,
        evidence: {
          human_support: 0,
          judgment_support: 0,
          human_contradiction: 0,
          judgment_contradiction: 0,
        },
        reputation: {
          epistemic_accuracy: 0,
          evidence_quality: 0,
          social_constructiveness: 0,
          creative_contribution: 0,
          moderation: 0,
          domain_expertise: 0,
        },
        temporal: 0.5,
        exploration,
      },
    },
    score,
    reasons: [],
  };
}
