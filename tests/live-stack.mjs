import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { verifyConversationReading } from "./conversation-browser.mjs";
import { verifyComposerDrafts } from "./drafts-browser.mjs";
import { verifyPublicProfiles } from "./profiles-browser.mjs";
import { verifyCardPresentation } from "./card-style-browser.mjs";
import { verifyFollowing } from "./following-browser.mjs";
import { verifyFeedPreferences } from "./preferences-browser.mjs";
import { assertFeedDiversityIsolation, verifyFeedDiversity } from "./feed-diversity-browser.mjs";
import { verifyInlineSurface } from "./inline-surface-browser.mjs";
import { verifySurfaceLease } from "./lease-browser.mjs";
import { prepareReactionRestart } from "./reactions-api.mjs";
import { verifyReactions } from "./reactions-browser.mjs";
import { prepareSecurityRestart } from "./security-api.mjs";
import { verifyAccountSecurity } from "./security-browser.mjs";
import { verifyTemporalPresentation } from "./temporal-browser.mjs";
import { verifySearchRecovery } from "./search-browser.mjs";
import { verifySourceAgreement } from "./agreement-browser.mjs";
import { verifyDocumentBridge } from "./document-bridge-browser.mjs";
import { verifyDocumentRegistration } from "./document-registration-browser.mjs";
import { verifyInvocationConsent } from "./invocation-consent-browser.mjs";
import { verifySocialSafety } from "./safety-browser.mjs";
import { prepareModerationAccounts, verifyModeration, assertModerationIsolation } from "./moderation-browser.mjs";
import { verifyBridgeCancellation } from "./bridge-cancellation-browser.mjs";
import { verifyImageViewer } from "./image-viewer-browser.mjs";
import { verifyRichMedia } from "./rich-media-browser.mjs";
import { verifyMediaAlbums } from "./media-albums-browser.mjs";
import { assertBrowserInvocationIsolation, verifyHostActions } from "./host-actions-browser.mjs";
import { verifyResourceDelivery } from "./resource-integrity-api.mjs";
import { verifyBundleAuthoring } from "./bundle-authoring-browser.mjs";
import { verifyPermissions } from "./permissions-browser.mjs";
import { DatabaseSync } from "node:sqlite";
import { rewriteBrowserTestImports } from "./browser-test-modules.mjs";
import { assertLiveStackIsolation } from "./live-stack-isolation.mjs";
import { LiveStackLifecycle } from "./live-stack-lifecycle.mjs";
import { startHttpObserver } from "./http-observer.mjs";

const root = resolve(new URL("..", import.meta.url).pathname);
const apiPort = Number(process.env.BABEL_LIVE_API_PORT ?? 18787);
const gatewayPort = Number(process.env.BABEL_LIVE_GATEWAY_PORT ?? 18788);
const frontendPort = Number(process.env.BABEL_LIVE_FRONTEND_PORT ?? 14329);
const aegisAddr = process.env.AEGIS_SERVER_ADDR ?? "127.0.0.1:17878";
const apiUrl = `http://127.0.0.1:${apiPort}`;
const frontendOrigin = `http://127.0.0.1:${frontendPort}`;
const frontendUrl = `${frontendOrigin}/?surface=first&lens=weird&q=Surface&run=${process.pid}-${Date.now()}`;
const focus = process.env.BABEL_LIVE_FOCUS;
const frontendMode = process.env.BABEL_LIVE_FRONTEND_MODE ?? "dev";
assert.ok(["dev", "production"].includes(frontendMode), "Unsupported frontend mode");
if (frontendMode === "production" && focus && !["browser-invocations", "moderation", "feed-diversity"].includes(focus)) {
  throw new Error(`Production acceptance is not yet validated for the ${focus} fixture`);
}
if (focus && !["preferences", "feed-diversity", "albums", "documents", "invocation-consent", "browser-invocations", "safety", "moderation"].includes(focus)) throw new Error(`Unsupported live-stack focus: ${focus}`);
const observedApiPort = ["moderation", "feed-diversity"].includes(focus) ? 18789 : undefined;
if (frontendMode === "production" || observedApiPort) {
  const freezeVariable = frontendMode === "production" ? "BABEL_LIVE_SOURCE_FROZEN"
    : focus === "moderation" ? "BABEL_MODERATION_SOURCE_FROZEN" : "BABEL_FEED_DIVERSITY_SOURCE_FROZEN";
  await assertLiveStackIsolation({ apiPort, gatewayPort, frontendPort, aegisAddr, observedApiPort }, freezeVariable);
}
if (focus === "browser-invocations") {
  await assertBrowserInvocationIsolation({ apiPort, gatewayPort, frontendPort, aegisAddr });
}
if (focus === "moderation") await assertModerationIsolation({ apiPort, gatewayPort, frontendPort, aegisAddr });
if (focus === "feed-diversity") await assertFeedDiversityIsolation({ apiPort, gatewayPort, frontendPort, aegisAddr });
const storeRoot = await mkdtemp(join(tmpdir(), "babel-live-stack-"));
let observer;
const lifecycle = new LiveStackLifecycle(storeRoot, { beforeStop: () => observer?.close() });
lifecycle.installSignalHandlers();
let apiProcess;
let astroProcess;
let authToken;
const accountPassword = "Live-stack account password 929026!";
let moderationAccounts;

function startApi() {
  apiProcess = lifecycle.spawn("cargo", ["run", "-p", "babel-api", "--bin", "babel-api"], {
    cwd: join(root, "backend"),
    env: {
      ...process.env,
      CARGO_INCREMENTAL: "0",
      CARGO_BUILD_JOBS: "2",
      BABEL_MODERATOR_IDS: moderationAccounts?.reviewerIds.join(",") ?? "",
      ...(["moderation", "feed-diversity"].includes(focus) ? { BABEL_OPERATOR_TOKEN: "" } : {}),
      BABEL_API_ADDR: `127.0.0.1:${observedApiPort ?? apiPort}`,
      BABEL_STORE_ROOT: storeRoot,
      BABEL_SEED_PROFILE: "card-feed",
      BABEL_PUBLIC_ORIGIN: apiUrl,
      BABEL_CORS_ORIGINS: frontendOrigin,
      BABEL_BUNDLE_GATEWAY_ADDR: `127.0.0.1:${gatewayPort}`,
      BABEL_JUDGMENT_PROVIDER: "python",
      BABEL_ALGORITHMS_DIR: join(root, "algorithms"),
      BABEL_PYTHON_EXECUTABLE: join(root, "algorithms/.venv/bin/python"),
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  apiProcess.stdout.on("data", (chunk) => process.stdout.write(`[babel-api] ${chunk}`));
  apiProcess.stderr.on("data", (chunk) => process.stderr.write(`[babel-api] ${chunk}`));
  apiProcess.on("exit", (code, signal) => {
    if (code !== null && code !== 0) {
      process.stderr.write(`[babel-api] exited with code ${code}\n`);
    }
    if (signal) {
      process.stderr.write(`[babel-api] exited from signal ${signal}\n`);
    }
  });
}

try {
  if (observedApiPort) observer = await startHttpObserver({ port: apiPort, upstreamPort: observedApiPort,
    capture: path => path === "/rpc" || path.startsWith("/moderation/") });
  startApi();
  await waitForJson(`${apiUrl}/health`, (body) => body.ok === true);
  const health = await (await fetch(`${apiUrl}/health`)).json();
  assert.deepEqual(health.judgment_provider, { provider: "babel-python", model: "lexical-v1", version: "1" });
  assert.deepEqual(health.ranking_provider, { provider: "babel-python", model: "lenses-v1", version: "1" });
  assert.deepEqual(health.temporal_provider, { provider: "babel-python", model: "temporal-v1", version: "1" });
  await startAstro();
  await waitForText(frontendUrl, "Babel Protocol");
  if (frontendMode === "production") await verifyProductionAssets();

  const discovery = await postJson(`${apiUrl}/discovery/candidates`, {
    anchors: [],
    search: "Surface",
    followed_objects: [],
    limit: 3,
    exploration_slots: 1,
    lens: null,
  });
  assert.ok(discovery.discovery.objects.length > 0, "seeded API should return discovery Objects");
  assert.deepEqual(discovery.discovery.ranking_provider, health.ranking_provider);
  assert.deepEqual(discovery.discovery.temporal.provider, health.temporal_provider);
  assert.ok(Math.abs(Date.now() - Date.parse(discovery.discovery.temporal.reference_time)) < 10000);
  assert.deepEqual(discovery.discovery.temporal.scores.map((score) => score.object_id), discovery.discovery.ranked.map((entry) => entry.candidate.object_id));
  for (const [index, score] of discovery.discovery.temporal.scores.entries()) {
    assert.equal(score.survival_score, discovery.discovery.ranked[index].candidate.signals.temporal);
    assert.ok(score.age_hours >= 0 && Number.isFinite(score.age_hours));
    assert.ok(score.recency >= 0 && score.recency <= 1);
  }
  assert.deepEqual(discovery.discovery.objects.map((object) => object.id), discovery.discovery.ranked.map((entry) => entry.candidate.object_id));

  const author = await postJson(`${apiUrl}/auth/register`, {
    kind: "Person",
    handle: `live-author-${process.pid}`,
    password: accountPassword,
  });
  authToken = author.token;
  const publishedText = `Live authoring Object ${process.pid} ${Date.now()}`;
  const publicationKey = `durable-publication-${process.pid}`;
  const publicationPayload = {
    author_id: author.identity.id,
    text: publishedText,
  };
  const published = await postRpc("babel.object.publish_text.v1", publicationPayload, publicationKey);
  assert.equal(published.object.author, author.identity.id);
  assert.equal(published.object.payload.text, publishedText);

  const derivedResponse = await fetch(`${apiUrl}/objects/${published.object.id}/judgments`);
  assert.equal(derivedResponse.status, 200);
  const derived = await derivedResponse.json();
  assert.equal(derived.judgments.length, 4, "publication executes all four Python ingestion definitions");
  for (const judgment of derived.judgments) {
    assert.deepEqual(judgment.provider, health.judgment_provider);
    assert.ok(judgment.id.startsWith("jud_"));
  }
  const evaluated = [];
  for (const definition of ["spam", "evidence_quality", "relevance", "relationship", "content_analysis", "moderation", "source_agreement"]) {
    const request = {
      object_id: published.object.id,
      definition: `babel.judgment.${definition}.v1`,
      parameters: definition === "relevance" ? { query: "Live authoring" }
        : definition === "relationship" ? { relation: "related" } : {},
    };
    const result = await postRpc("babel.judgment.object.evaluate.v1", request);
    assert.deepEqual(result.judgment.provider, health.judgment_provider);
    const cached = await postRpc("babel.judgment.object.evaluate.v1", request);
    assert.equal(cached.judgment.id, result.judgment.id, "same definition/model/parameters reuse the Judgment");
    evaluated.push(definition);
  }

  assert.deepEqual(await postRpc("babel.object.publish_text.v1", publicationPayload, publicationKey), published);
  const verifyReactionRestart = await prepareReactionRestart(apiUrl, authToken, published.object.id, author.identity.id);
  const verifySecurityRestart = await prepareSecurityRestart(apiUrl);
  if (focus === "moderation") moderationAccounts = await prepareModerationAccounts(apiUrl);
  await stopApi();
  startApi();
  await waitForJson(`${apiUrl}/health`, (body) => body.ok === true);
  await verifyReactionRestart();
  await verifySecurityRestart();
  assert.deepEqual(await postRpc("babel.object.publish_text.v1", publicationPayload, publicationKey), published,
    "a restarted Python-backed API must replay the committed Object instead of publishing again");
  const profileReadback = await (await fetch(`${apiUrl}/identities/${author.identity.id}/objects?limit=50`)).json();
  assert.deepEqual(profileReadback.objects.map((object) => object.id), [published.object.id],
    "author index must rebuild after restart without retry duplicates");
  process.stdout.write("[publication-retry] same key returns the original publication before and after real process restart\n");

  const authoredDiscovery = await postRpc("babel.search.objects.v1", {
    q: publishedText,
    author: null,
    kind: null,
    limit: 5,
  });
  assert.ok(
    authoredDiscovery.results.some((result) => result.object.id === published.object.id),
    "authored Object should be searchable",
  );

  const object = discovery.discovery.objects[0];
  const prepared = await postJson(`${apiUrl}/runtime/surfaces/prepare`, {
    object_id: object.id,
    role: "Feed",
  });
  assert.equal(prepared.plan.admission, "ready");
  assert.equal(prepared.plan.surface.target, "Web");

  const surfaceResponse = await fetch(prepared.plan.surface.entry);
  assert.equal(surfaceResponse.ok, true);
  assert.equal(surfaceResponse.headers.get("content-type"), "text/html");
  assert.match(await surfaceResponse.text(), /Babel Object Surface/);
  await verifyResourceDelivery(apiUrl, storeRoot, prepared.plan.surface);

  await ensureAegis();
  await navigateAegis(frontendUrl);
  await waitForAegisUrl(frontendOrigin);
  await waitForAegisText(/Create account/);
  await observeSeedBridge();
  await postAegisExecute([{ type: "eval", code: `
    (() => {
      document.querySelector('[data-account-mode="register"]').click();
      document.querySelector('[data-account-register]').value = 'browser-author-${process.pid}';
      document.querySelector('[data-account-password]').value = ${JSON.stringify(accountPassword)};
      document.querySelector('[data-account-form]').requestSubmit();
      return { submitted: true };
    })()
  ` }]);
  const browserAccount = await waitForAegisEval(`
    (() => ({
      status: document.querySelector('[data-account-status]').textContent,
      id: document.querySelector('[data-account-id]').value
    }))()
  `, (value) => value?.status === 'Account created' && value.id);
  if (focus === "feed-diversity") {
    // Dedicated fixtures and capture are isolated from the separate full-stack
    // regression. Run focused acceptance and full regression serially.
    await postAegisExecute([{ type: "eval", code: "document.querySelector('[data-account-close]').click(); document.querySelector('[data-close-surface]').click(); ({closed:true})" }]);
    await waitForAegisEval("({state:document.querySelector('[data-status]')?.dataset.state})", value => value.state === "online");
    await verifyFeedDiversity({ execute: postAegisExecute, waitFor: waitForAegisEval, apiUrl, identityId: browserAccount.id,
      readRequests: options => observer.readRequests(options) });
    console.log(JSON.stringify({ ok: true, focus, api: apiUrl, frontend: frontendUrl }));
  } else if (focus === "moderation") {
    // Moderation is a separate acceptance run: its configured reviewer accounts,
    // API restart during restriction, and role frames are isolated from full-stack
    // fixtures. Release verification runs both full-stack and this focus.
    await postAegisExecute([{ type: "eval", code: "document.querySelector('[data-account-close]').click(); document.querySelector('[data-close-surface]').click(); ({closed:true})" }]);
    await verifyModeration({ execute: postAegisExecute, waitFor: waitForAegisEval, apiUrl,
      readRequests: options => observer.readRequests(options),
      accounts: moderationAccounts, seed: object, restart: async () => {
        await stopApi(); startApi(); await waitForJson(`${apiUrl}/health`, body => body.ok === true);
      } });
    console.log(JSON.stringify({ ok: true, focus, api: apiUrl, frontend: frontendUrl }));
  } else if (focus === "safety") {
    await postAegisExecute([{ type: "eval", code: "document.querySelector('[data-account-close]').click(); document.querySelector('[data-close-surface]').click(); ({closed:true})" }]);
    await verifySocialSafety({ execute: postAegisExecute, waitFor: waitForAegisEval, rpc: postRpc, authorId: author.identity.id, apiUrl });
    console.log(JSON.stringify({ ok: true, focus, api: apiUrl, frontend: frontendUrl }));
  } else if (focus === "browser-invocations") {
    await postAegisExecute([{ type: "eval", code: "document.querySelector('[data-account-close]').click(); document.querySelector('[data-close-surface]').click(); ({closed:true})" }]);
    await verifyHostActions({ execute: postAegisExecute, waitFor: waitForAegisEval, apiUrl, password: accountPassword, readSurfaceDocument });
    console.log(JSON.stringify({ ok: true, focus, api: apiUrl, frontend: frontendUrl }));
  } else if (focus === "invocation-consent") {
    await postAegisExecute([{ type: "eval", code: "document.querySelector('[data-account-close]').click(); document.querySelector('[data-close-surface]').click(); ({closed:true})" }]);
    await verifyInvocationConsent(postAegisExecute, waitForAegisEval, { apiUrl, readSurfaceDocument });
    console.log(JSON.stringify({ ok: true, focus, api: apiUrl, frontend: frontendUrl }));
  } else if (focus === "documents") {
    await postAegisExecute([{ type: "eval", code: "document.querySelector('[data-account-close]').click(); document.querySelector('[data-close-surface]').click(); ({closed:true})" }]);
    await verifyDocumentRegistration(postAegisExecute, waitForAegisEval, {
      apiUrl, objectId: prepared.plan.object_id, password: accountPassword,
    });
    await verifyDocumentBridge(postAegisExecute, waitForAegisEval, prepared.plan);
    console.log(JSON.stringify({ ok: true, focus, api: apiUrl, frontend: frontendUrl }));
  } else if (focus === "albums") {
    await postAegisExecute([{ type: "eval", code: "document.querySelector('[data-account-close]').click(); document.querySelector('[data-close-surface]').click(); ({closed:true})" }]);
    await verifyMediaAlbums(postAegisExecute, waitForAegisEval);
    console.log(JSON.stringify({ ok: true, focus, api: apiUrl, frontend: frontendUrl }));
  } else if (focus === "preferences") {
    await postAegisExecute([{ type: "eval", code: "document.querySelector('[data-account-close]').click(); ({closed:true})" }]);
    await verifyPreferencesAndFollowing(author.identity, browserAccount.id, false);
    console.log(JSON.stringify({ ok: true, focus, api: apiUrl, frontend: frontendUrl }));
  } else {
    const impersonation = await fetch(`${apiUrl}/objects/text`, {
      method: "POST", headers: { "content-type": "application/json", authorization: `Bearer ${authToken}` },
      body: JSON.stringify({ author_id: browserAccount.id, text: "Must not be signed" }),
    });
    assert.equal(impersonation.status, 403, "one account cannot ask REST to sign as another");
    await postAegisExecute([{ type: "eval", code: `
      (() => {
        document.querySelector('[data-account-close]').click();
        return { opened: true };
      })()
    ` }]);
    await verifySeedBridge();
    const browserText = await waitForAegisText(/Feed Surface/);
    assert.match(browserText, /Online/);
    assert.match(browserText, /Catalog \d+ methods/);
    await verifyDocumentBridge(postAegisExecute, waitForAegisEval, prepared.plan);
    await verifyDocumentRegistration(postAegisExecute, waitForAegisEval, {
      apiUrl, objectId: prepared.plan.object_id, password: accountPassword,
    });
    await verifyBridgeCancellation(postAegisExecute, waitForAegisEval);
    assert.match(browserText, /Local Local/);
    const browserDiscovery = await postRpc("babel.discovery.candidates.v1", {
      anchors: [], search: "Surface", followed_objects: [], limit: 9, exploration_slots: 2,
      lens: { id: "babel.lens.stack.weird.v1", weights: [{ lens: "Weird", weight: 1 }] },
    });
    const diversityTrace = browserDiscovery.discovery.diversity_trace;
    const expectedDiversity = diversityTrace.policy.max_source_share === null ? "Off"
      : diversityTrace.filtered.length === 0 && diversityTrace.candidates.some(candidate => candidate.reasons.length > 0)
        ? "Active" : `${diversityTrace.policy.source_floors.length} floors`;
    await waitForAegisEval(`({ diversity: document.querySelector('[data-diversity-label]').textContent })`,
      value => value.diversity === expectedDiversity);
    assert.match(browserText, /Lens Weird/);
    assert.match(browserText, /Search Objects/);
    assert.match(browserText, /Replies/);
    // The active Surface replaces its card controls; normal-card controls are
    // checked in the temporal presentation frames below, not on hidden neighbors.
    assert.match(browserText, /Feed Surface/);
    assert.match(browserText, /Inspect Object/);
    const rankingInspection = await waitForAegisEval(`
      (() => ({ providers: [...document.querySelectorAll('.post-manifest pre')]
        .map((entry) => JSON.parse(entry.textContent).ranking_provider).filter(Boolean) }))()
    `, (value) => value?.providers?.length > 0);
    for (const provider of rankingInspection.providers) assert.deepEqual(provider, health.ranking_provider);
    await verifyTemporalPresentation(postAegisExecute, waitForAegisEval, health.temporal_provider);
    assert.match(browserText, /browser-author-/);

    const openedComposer = await postAegisExecute([
      {
        type: "eval",
        code: `
      (() => {
        document.querySelector('[data-toggle-composer]')?.click();
        return {
          hidden: document.querySelector('[data-composer-panel]')?.hidden ?? null,
          author: document.querySelector('[data-author-handle]')?.value,
          authorLabel: document.querySelector('[data-author-handle]')?.getAttribute('aria-label'),
          authorReadonly: document.querySelector('[data-author-handle]')?.readOnly,
          text: document.body.textContent ?? ''
        };
      })()
    `,
      },
    ]);
    assert.equal(openedComposer.results.length, 1);
    assert.equal(openedComposer.results[0].ok, true, JSON.stringify(openedComposer.results[0]));
    assert.equal(openedComposer.results[0].value.hidden, false);
    assert.equal(openedComposer.results[0].value.author, `browser-author-${process.pid}`);
    assert.equal(openedComposer.results[0].value.authorLabel, 'Author');
    assert.equal(openedComposer.results[0].value.authorReadonly, true);
    assert.match(openedComposer.results[0].value.text, /New post/);
    assert.match(openedComposer.results[0].value.text, /Post text/);
    assert.match(openedComposer.results[0].value.text, /Add media/);

    const formText = runAegis(["page", "forms"]);
    assert.match(formText, /Post/);
    assert.match(formText, /Add media/);

    const surfaceFrame = await waitForAegisEval(`
      (() => {
        const frame = document.querySelector('iframe[data-babel-surface-role="Feed"]');
        if (!frame) {
          return null;
        }
        return {
          allow: frame.getAttribute("allow"),
          csp: frame.getAttribute("csp"),
          credentialless: frame.hasAttribute("credentialless"),
          lifecycle: frame.getAttribute("data-babel-lifecycle"),
          target: frame.getAttribute("data-babel-surface-target"),
          memoryBudget: frame.getAttribute("data-babel-memory-budget"),
          gpuExpected: frame.getAttribute("data-babel-gpu-expected"),
        };
      })()
    `, (value) => typeof value?.allow === "string" && typeof value?.csp === "string");
    assert.match(surfaceFrame.allow, /camera 'none'/);
    assert.match(surfaceFrame.allow, /microphone 'none'/);
    assert.match(surfaceFrame.allow, /usb 'none'/);
    assert.match(surfaceFrame.allow, /webgpu 'none'/);
    assert.match(surfaceFrame.csp, /worker-src 'none'/);
    assert.match(surfaceFrame.csp, /object-src 'none'/);
    assert.equal(surfaceFrame.credentialless, true);
    assert.equal(surfaceFrame.target, "Web");
    assert.equal(surfaceFrame.memoryBudget, "33554432");
    assert.equal(surfaceFrame.gpuExpected, "false");
    await postAegisExecute([{ type: "eval", code: "document.querySelector('[data-close-composer]').click(); ({ closed: true })" }]);
    await verifyInlineSurface(postAegisExecute, waitForAegisEval);
    const leaseDb = new DatabaseSync(join(storeRoot, "auth/accounts.sqlite3"));
    try {
      leaseDb.exec("PRAGMA busy_timeout=5000");
      await verifySurfaceLease(postAegisExecute, waitForAegisEval, {
        read: id => leaseDb.prepare("SELECT lease_expires_at, retired FROM surface_owners WHERE session_id=?").get(id),
        expire: id => leaseDb.prepare("UPDATE surface_owners SET lease_expires_at=? WHERE session_id=?").run(Date.now() - 1000, id),
      });
    } finally { leaseDb.close(); }

    const menuBehavior = await postAegisExecute([
      {
        type: "eval",
        code: `
      (() => {
        const profile = document.querySelector('[data-toggle-profile]');
        profile?.click();
        const menuOpen = !(document.querySelector('[data-profile-dropdown]')?.hidden ?? true);
        document.querySelector('[data-menu-action=settings]')?.click();
        const settingsOpen = !(document.querySelector('[data-settings-panel]')?.hidden ?? true);
        const settingsText = document.querySelector('[data-settings-panel]')?.textContent ?? '';
        document.querySelector('[data-close-settings]')?.click();
        profile?.click();
        document.querySelector('[data-menu-action=help]')?.click();
        const helpOpen = !(document.querySelector('[data-help-panel]')?.hidden ?? true);
        const helpText = document.querySelector('[data-help-panel]')?.textContent ?? '';
        document.querySelector('[data-close-help]')?.click();
        profile?.click();
        document.querySelector('[data-menu-action=sign-out]')?.click();
        const authorStatus = document.querySelector('[data-author-status]')?.textContent ?? '';
        return { menuOpen, settingsOpen, settingsText, helpOpen, helpText, authorStatus };
      })()
    `,
      },
    ]);
    assert.equal(menuBehavior.results.length, 1);
    assert.equal(menuBehavior.results[0].ok, true, JSON.stringify(menuBehavior.results[0]));
    assert.equal(menuBehavior.results[0].value.menuOpen, true);
    assert.equal(menuBehavior.results[0].value.settingsOpen, true);
    assert.match(menuBehavior.results[0].value.settingsText, /Feed controls/);
    assert.match(menuBehavior.results[0].value.settingsText, /BalancedFollowingResearchWeird/);
    assert.equal(menuBehavior.results[0].value.helpOpen, true);
    assert.match(menuBehavior.results[0].value.helpText, /Swipe-card controls/);
    assert.match(menuBehavior.results[0].value.helpText, /Open Surfaces/);
    await waitForAegisEval(`({ visible: !!document.querySelector('[data-security-confirm]')?.getClientRects().length })`, value => value.visible);
    await postAegisExecute([{ type: "eval", code: "document.querySelector('[data-security-confirm]').click(); ({ confirmed: true })" }]);
    await waitForAegisEval(`
      (() => ({ status: document.querySelector('[data-author-status]').textContent }))()
    `, (value) => value?.status === 'Not signed in');
    await postAegisExecute([{ type: "eval", code: `
      (() => {
        document.querySelector('[data-toggle-profile]').click();
        document.querySelector('[data-menu-action="profile"]').click();
        document.querySelector('[data-account-mode="login"]').click();
        document.querySelector('[data-account-login]').value = ${JSON.stringify(browserAccount.id)};
        document.querySelector('[data-account-password]').value = ${JSON.stringify(accountPassword)};
        document.querySelector('[data-account-form]').requestSubmit();
        return { submitted: true };
      })()
    ` }]);
    await waitForAegisEval(`
      (() => ({ status: document.querySelector('[data-account-status]').textContent }))()
    `, (value) => value?.status === 'Signed in');
    await postAegisExecute([{ type: "eval", code: `
      (() => { document.querySelector('[data-account-close]').click(); return { closed: true }; })()
    ` }]);
    const restoredUrl = new URL(frontendUrl);
    restoredUrl.searchParams.delete("surface");
    restoredUrl.searchParams.set("session_check", "restored");
    await postAegisExecute([{ type: "eval", code: "window.__restoreNavigationGuard = true; ({ marked: true })" }]);
    await navigateAegis(restoredUrl.href);
    const restoredAccount = await waitForAegisEval(`
      (() => ({ handle: document.querySelector('[data-author-handle]').value,
        fresh: window.__restoreNavigationGuard === undefined,
        online: document.querySelector('[data-status]')?.dataset.state === 'online',
        dialogOpen: document.querySelector('[data-account-dialog]').open }))()
    `, (value) => value?.fresh && value.online && value.handle === `browser-author-${process.pid}`);
    assert.equal(restoredAccount.dialogOpen, false, "valid authenticated sessions survive a page reload");
    await observeSeedBridge();
    await postAegisExecute([{ type: "eval", code: `
      document.querySelector('.post-card[data-offset="0"] [data-action="surface"]').click(); ({ opened: true })
    ` }]);
    await verifySeedBridge();

    // The Surface search intentionally has one match. Return through the visible
    // search form before exercising multi-post draft and conversation workflows.
    await postAegisExecute([{ type: "eval", code: `(() => {
      document.querySelector('[data-close-surface]').click();
      document.querySelector('[data-search-input]').value = '';
      document.querySelector('[data-search-form]').requestSubmit();
      return { submitted: true };
    })()` }]);
    await waitForAegisEval(`({
      online: document.querySelector('[data-status]').dataset.state === 'online',
      cards: document.querySelectorAll('.post-card:not([data-exiting])').length,
      surfaceClosed: document.querySelector('[data-surface-panel]').hidden,
      query: new URL(location.href).searchParams.get('q')
    })`, value => value.online && value.cards >= 2 && value.surfaceClosed && value.query === null);

    const platformOverview = await waitForAegisEval(`
      (() => {
        const profile = document.querySelector('[data-toggle-profile]');
        if (document.querySelector('[data-settings-panel]')?.hidden ?? true) {
          profile?.click();
          document.querySelector('[data-menu-action=settings]')?.click();
        }
        const panel = document.querySelector('[data-settings-panel]');
        return {
          open: !(panel?.hidden ?? true),
          status: document.querySelector('[data-platform-status]')?.textContent ?? '',
          events: document.querySelector('[data-platform-events]')?.textContent ?? '',
          objects: document.querySelector('[data-platform-objects]')?.textContent ?? '',
          capabilities: document.querySelector('[data-platform-capabilities]')?.textContent ?? '',
          lenses: document.querySelector('[data-platform-lenses]')?.textContent ?? '',
          judges: document.querySelector('[data-platform-judges]')?.textContent ?? '',
          text: panel?.textContent ?? ''
        };
      })()
    `, (value) => value?.open === true && /Updated/.test(value.status));
    assert.match(platformOverview.status, /Updated/);
    assert.match(platformOverview.capabilities, /^\d+$/);
    assert.match(platformOverview.lenses, /^\d+$/);
    assert.match(platformOverview.judges, /^\d+$/);
    assert.match(platformOverview.text, /Protocol catalogs/);
    assert.match(platformOverview.text, /Capability Classes/);
    assert.match(platformOverview.text, /babel\./);
    await postAegisExecute([
      {
        type: "eval",
        code: "document.querySelector('[data-close-settings]')?.click(); true",
      },
    ]);

    await postAegisExecute([{ type: "eval", code: `
      (() => {
        const card = document.querySelector('.post-card[data-offset="0"]');
        [...card.querySelectorAll('button')].find(button => button.textContent.trim() === 'Protocol').click();
        card.querySelector('[data-action="judgments"]').click();
        return true;
      })()
    ` }]);
    const judgmentPanel = await waitForAegisEval(`
      (() => ({ state: document.querySelector('[data-judgment-status]').dataset.state,
        text: document.querySelector('[data-judgment-list]').textContent,
        confidence: [...document.querySelectorAll('.judgment-metric')]
          .filter(metric => metric.querySelector('span')?.textContent === 'Confidence')
          .map(metric => metric.querySelector('strong').textContent) }))()
    `, value => value?.state === "ready" && value.confidence.length >= 4);
    assert.match(judgmentPanel.text, /babel-python\/lexical-v1@1/);
    assert.ok(judgmentPanel.confidence.includes("Uncalibrated"));
    assert.ok(judgmentPanel.confidence.includes("Heuristic"));
    await verifySourceAgreement(postAegisExecute, waitForAegisEval, postRpc, apiUrl, author.identity.id);
    await postAegisExecute([{ type: "eval", code: "document.querySelector('[data-close-judgments]').click(); true" }]);

    await verifyComposerDrafts(postAegisExecute, waitForAegisEval, browserAccount.id);

    const socialReply = await postAegisExecute([
      {
        type: "eval",
        code: `
      (() => {
        const card = document.querySelector('.post-card[data-offset="0"]');
        const social = [...card?.querySelectorAll('button') ?? []]
          .find((button) => button.textContent?.trim() === 'Social');
        social?.click();
        const panel = card?.querySelector('.action-popover[data-kind="social"] .popover-panel');
        if (!panel || panel.hidden || !panel.getClientRects().length) throw new Error('Social menu did not open');
        card?.querySelector('button[data-action="reply"]')?.click();
        const textarea = document.querySelector('textarea[data-compose-text]');
        const form = document.querySelector('form[data-compose-form]');
        if (!textarea || !form) {
          return { submitted: false, reason: 'composer missing' };
        }
        textarea.value = 'A signed live-stack reply.';
        textarea.dispatchEvent(new Event('input', { bubbles: true }));
        setTimeout(() => form.requestSubmit(), 0);
        return {
          submitted: true,
          objectId: card?.getAttribute('data-object-id') ?? null,
          composerHidden: document.querySelector('[data-composer-panel]')?.hidden ?? null,
          submitLabel: document.querySelector('[data-compose-submit]')?.textContent ?? ''
        };
      })()
    `,
      },
    ]);
    assert.equal(socialReply.results.length, 1);
    assert.equal(socialReply.results[0].ok, true, JSON.stringify(socialReply.results[0]));
    assert.equal(socialReply.results[0].value.submitted, true);
    assert.equal(socialReply.results[0].value.submitLabel, "Reply");
    const replyStatus = await waitForAegisText(/Published obj_.* from browser-author-/);
    assert.match(replyStatus, /Published obj_.* from browser-author-/);
    const conversation = await waitForAegisEval(`
      (() => {
        const column = document.querySelector('.post-card[data-offset="0"]');
        const row = [...column?.querySelectorAll('.reply-row') ?? []]
          .find((reply) => reply.textContent.includes('A signed live-stack reply.'));
        return {
          parentId: column?.dataset.objectId,
          replyId: row?.dataset.objectId,
          text: row?.textContent,
          canReply: !!row?.querySelector('[data-action="reply"]'),
          compact: row ? row.getBoundingClientRect().height < column.querySelector('.post-primary').getBoundingClientRect().height / 2 : false,
          widthRatio: row ? row.getBoundingClientRect().width / column.querySelector('.post-primary').getBoundingClientRect().width : null,
          radius: row ? parseFloat(getComputedStyle(row).borderRadius) : null,
          hue: row ? getComputedStyle(row).getPropertyValue('--reply-to').trim() : null
        };
      })()
    `, (value) => value?.text?.includes('A signed live-stack reply.'));
    assert.equal(conversation.parentId, socialReply.results[0].value.objectId);
    assert.match(conversation.replyId, /^obj_/);
    assert.equal(conversation.canReply, true);
    assert.equal(conversation.compact, true, "short replies remain subordinate to the primary card");
    assert.ok(conversation.widthRatio <= 0.9);
    assert.equal(conversation.radius, 18);
    assert.match(conversation.hue, /^#[0-9a-f]{6}$/);

    const replyAuthor = await postAegisExecute([{ type: "eval", code: `
      (() => {
        const row = [...document.querySelectorAll('.post-card[data-offset="0"] .reply-row')]
          .find((reply) => reply.dataset.objectId === ${JSON.stringify(conversation.replyId)});
        const author = row.querySelector('[data-profile-author]');
        author.focus({ preventScroll: true });
        author.click();
        return { identity: author.dataset.profileAuthor, open: document.querySelector('[data-public-profile]').open };
      })()
    ` }]);
    assert.equal(replyAuthor.results[0].ok, true);
    assert.deepEqual(replyAuthor.results[0].value, { identity: browserAccount.id, open: true });
    await waitForAegisEval(`({ state: document.querySelector('[data-public-profile-status]').dataset.state,
      identity: document.querySelector('[data-public-profile-identity]').textContent })`,
      (value) => value?.state === 'ready' && value.identity === browserAccount.id);
    const replyReturn = await postAegisExecute([{ type: "eval", code: `
      (() => {
        document.querySelector('[data-public-profile-close]').click();
        return { focus: document.activeElement.dataset.profileAuthor,
          parent: document.querySelector('.post-card[data-offset="0"]').dataset.objectId };
      })()
    ` }]);
    assert.deepEqual(replyReturn.results[0].value, { focus: browserAccount.id, parent: conversation.parentId });

    const scrolledColumn = await postAegisExecute([{ type: "eval", code: `
      (() => {
        const column = document.querySelector('.post-card[data-offset="0"]');
        column.scrollTop = Math.min(120, column.scrollHeight - column.clientHeight);
        return { id: column.dataset.objectId, top: column.scrollTop };
      })()
    ` }]);
    assert.equal(scrolledColumn.results[0].ok, true);
    assert.ok(scrolledColumn.results[0].value.top > 0, "conversation belongs to the vertically scrollable post column");
    await postAegisExecute([{ type: "eval", code: `
      (() => {
        document.querySelector('[data-next]').click();
        return { current: document.querySelector('.post-card[data-offset="0"]').dataset.objectId };
      })()
    ` }]);
    await postAegisExecute([{ type: "eval", code: `
      (() => {
        document.querySelector('[data-prev]').click();
        const column = document.querySelector('.post-card[data-offset="0"]');
        return { id: column.dataset.objectId, top: column.scrollTop };
      })()
    ` }]).then((result) => {
      assert.equal(result.results[0].ok, true);
      assert.equal(result.results[0].value.id, scrolledColumn.results[0].value.id);
      assert.equal(result.results[0].value.top, scrolledColumn.results[0].value.top);
    });

    const nestedThread = await postAegisExecute([{ type: "eval", code: `
      (() => {
        const column = document.querySelector('.post-card[data-offset="0"]');
        const row = [...column.querySelectorAll('.reply-row')]
          .find((reply) => reply.textContent.includes('A signed live-stack reply.'));
        const replyId = row.dataset.objectId;
        [...row.querySelectorAll('button')].find((button) => button.textContent === 'View replies').click();
        return { replyId, threadId: column.querySelector('.conversation').dataset.conversationThread };
      })()
    ` }]);
    assert.equal(nestedThread.results[0].ok, true);
    assert.equal(nestedThread.results[0].value.threadId, conversation.replyId);
    await waitForAegisEval(`
      (() => {
        const panel = document.querySelector('.post-card[data-offset="0"] .conversation');
        return { busy: panel.getAttribute('aria-busy'), status: panel.querySelector('.conversation-status').textContent };
      })()
    `, (value) => value?.busy === 'false' && value.status === 'No replies yet.');
    const returnedThread = await postAegisExecute([{ type: "eval", code: `
      (() => {
        const column = document.querySelector('.post-card[data-offset="0"]');
        [...column.querySelectorAll('.conversation-header button')].find((button) => button.textContent === 'Back').click();
        return {
          threadId: column.querySelector('.conversation').dataset.conversationThread,
          top: column.scrollTop,
          text: column.querySelector('.conversation-list').textContent
        };
      })()
    ` }]);
    assert.equal(returnedThread.results[0].ok, true);
    assert.equal(returnedThread.results[0].value.threadId, conversation.parentId);
    assert.equal(returnedThread.results[0].value.top, scrolledColumn.results[0].value.top);
    assert.match(returnedThread.results[0].value.text, /A signed live-stack reply/);
    await verifyConversationReading(postAegisExecute, waitForAegisEval);

    const socialShare = await postAegisExecute([
      {
        type: "eval",
        code: `
      (() => {
        const card = document.querySelector('.post-card[data-offset="0"]');
        const social = [...card?.querySelectorAll('button') ?? []]
          .find((button) => button.textContent?.trim() === 'Social');
        social?.click();
        const panel = card?.querySelector('.action-popover[data-kind="social"] .popover-panel');
        if (panel) {
          panel.hidden = false;
          panel.dataset.state = 'open';
        }
        card?.querySelector('button[data-action="share"]')?.click();
        const textarea = document.querySelector('textarea[data-compose-text]');
        const form = document.querySelector('form[data-compose-form]');
        if (!textarea || !form) {
          return { submitted: false, reason: 'composer missing' };
        }
        textarea.value = 'Sharing this Object with signed context.';
        textarea.dispatchEvent(new Event('input', { bubbles: true }));
        setTimeout(() => form.requestSubmit(), 0);
        return {
          submitted: true,
          submitLabel: document.querySelector('[data-compose-submit]')?.textContent ?? ''
        };
      })()
    `,
      },
    ]);
    assert.equal(socialShare.results.length, 1);
    assert.equal(socialShare.results[0].ok, true, JSON.stringify(socialShare.results[0]));
    assert.equal(socialShare.results[0].value.submitted, true);
    assert.equal(socialShare.results[0].value.submitLabel, "Share");
    const shareStatus = await waitForAegisText(/Published obj_.* from browser-author-/);
    assert.match(shareStatus, /Published obj_.* from browser-author-/);

    const mediaSubmitBody = await postAegisExecute([
      {
        type: "eval",
        code: `
      (() => {
        document.querySelector('[data-toggle-composer]').click();
        const fileInput = document.querySelector('input[data-compose-media]');
        const textarea = document.querySelector('textarea[data-compose-text]');
        const form = document.querySelector('form[data-compose-form]');
        if (!fileInput || !textarea || !form) {
          return { submitted: false, reason: 'composer controls missing' };
        }
        const bytes = Uint8Array.from([
          137, 80, 78, 71, 13, 10, 26, 10,
          0, 0, 0, 13, 73, 72, 68, 82,
          0, 0, 0, 1, 0, 0, 0, 1,
          8, 6, 0, 0, 0, 31, 21, 196,
          137, 0, 0, 0, 13, 73, 68, 65,
          84, 120, 156, 99, 248, 15, 4,
          0, 9, 251, 3, 253, 167, 89, 231,
          219, 0, 0, 0, 0, 73, 69, 78,
          68, 174, 66, 96, 130
        ]);
        const file = new File([bytes], 'surface-live.png', { type: 'image/png' });
        if (typeof DataTransfer === 'function') {
          const transfer = new DataTransfer();
          transfer.items.add(file);
          fileInput.files = transfer.files;
        } else {
          Object.defineProperty(fileInput, 'files', {
            configurable: true,
            value: {
              0: file,
              length: 1,
              item: (index) => index === 0 ? file : null
            }
          });
        }
        textarea.value = 'Surface live image Object';
        textarea.dispatchEvent(new Event('input', { bubbles: true }));
        fileInput.dispatchEvent(new Event('change', { bubbles: true }));
        window.__babelMediaSubmitStarted = true;
        return { submitted: true, fileCount: fileInput.files?.length ?? 0 };
      })()
    `,
      },
    ]);
    assert.equal(mediaSubmitBody.results.length, 1);
    assert.equal(mediaSubmitBody.results[0].ok, true, JSON.stringify(mediaSubmitBody.results[0]));
    assert.deepEqual(mediaSubmitBody.results[0].value, { submitted: true, fileCount: 1 });
    await waitForAegisEval(`({ state: document.querySelector('[data-compose-preview]').dataset.state })`, value => value.state === 'ready');
    const attachment = await postAegisExecute([{ type: "eval", code: `(() => {
      const fileInput = document.querySelector('[data-compose-media]');
      const file = fileInput.files[0];
      const name = document.querySelector('[data-compose-media-name]').textContent;
      document.querySelector('[data-remove-compose-media]').click();
      const removed = document.querySelector('[data-compose-preview]').hidden && fileInput.files.length === 0;
      const text = document.querySelector('[data-compose-text]').value;
      const transfer = new DataTransfer(); transfer.items.add(file); fileInput.files = transfer.files;
      fileInput.dispatchEvent(new Event('change', { bubbles: true }));
      document.querySelector('[data-compose-form]').requestSubmit();
      return { name, removed, text };
    })()` }]);
    assert.deepEqual(attachment.results[0].value, { name: 'surface-live.png', removed: true, text: 'Surface live image Object' });
    const mediaPublished = await waitForAegisEval(`
      (() => {
        const status = document.querySelector('[data-author-status]');
        if (!status) {
          return null;
        }
        return {
          text: status.textContent ?? '',
          state: status.getAttribute('data-state'),
          composerHidden: document.querySelector('[data-composer-panel]')?.hidden ?? null
        };
      })()
    `, (value) => typeof value?.text === "string" && value.text.endsWith(`from browser-author-${process.pid}`)
      && value.text.startsWith("Published ") && value.composerHidden === true);
    assert.equal(mediaPublished.state, "ready");
    assert.equal(mediaPublished.composerHidden, true);
    const mediaPreview = await waitForAegisEval(`
      (() => {
        const image = [...document.querySelectorAll('.post-card img')]
          .find((candidate) => candidate.closest('.post-card')?.textContent.includes('Surface live image Object'));
        if (!image) {
          return null;
        }
        const card = image.closest('[data-object-id]');
        return {
          src: image.getAttribute('src'),
          objectId: card?.getAttribute('data-object-id') ?? null,
          text: card?.textContent ?? '',
          caption: card?.querySelector('.post-context .post-content')?.textContent ?? '',
          loaded: image.complete && image.naturalWidth > 0
        };
      })()
    `, (value) => typeof value?.src === "string" && typeof value?.text === "string" && value.loaded);
    assert.match(mediaPreview.src, /\/objects\/obj_[a-f0-9]+\/media\/[a-f0-9]+$/);
    assert.match(mediaPreview.text, /image\/png/);
    assert.equal(mediaPreview.caption, "Surface live image Object");
    assert.ok(!mediaPreview.caption.includes("primary_resource"));
    await verifyImageViewer(postAegisExecute, waitForAegisEval, mediaPreview.objectId);
    await verifyRichMedia(postAegisExecute, waitForAegisEval);
    await verifyMediaAlbums(postAegisExecute, waitForAegisEval);

    await verifyPublicProfiles({ postAegisExecute, waitForAegisEval, identityId: browserAccount.id });
    await verifyReactions(postAegisExecute, waitForAegisEval, browserAccount.id);
    await verifyCardPresentation(postAegisExecute, waitForAegisEval);
    await verifySearchRecovery(postAegisExecute, waitForAegisEval);
    await verifyBundleAuthoring(postAegisExecute, waitForAegisEval);
    await verifyPermissions({ execute: postAegisExecute, waitFor: waitForAegisEval, rpc: postRpc, authorId: author.identity.id });
    await verifyHostActions({ execute: postAegisExecute, waitFor: waitForAegisEval, apiUrl, password: accountPassword, rpc: postRpc, readSurfaceDocument });
    await verifyInvocationConsent(postAegisExecute, waitForAegisEval, { apiUrl, readSurfaceDocument });
    await verifyPreferencesAndFollowing(author.identity, browserAccount.id, true);
    await verifySocialSafety({ execute: postAegisExecute, waitFor: waitForAegisEval, rpc: postRpc, authorId: author.identity.id, apiUrl });

    await verifyAccountSecurity({ execute: postAegisExecute, waitFor: waitForAegisEval,
      api: apiUrl, identityId: browserAccount.id, password: accountPassword });

    console.log(
      JSON.stringify(
        {
          ok: true,
          algorithmProvider: health.judgment_provider,
          rankingProvider: health.ranking_provider,
          temporalProvider: health.temporal_provider,
          evaluatedDefinitions: evaluated,
          api: apiUrl,
          frontend: frontendUrl,
          surface_entry: prepared.plan.surface.entry,
        },
        null,
        2,
      ),
    );
  }
} finally {
  await lifecycle.cleanup();
}

async function verifyPreferencesAndFollowing(target, identityId, includeFollowing) {
  const search = `Following verification ${process.pid}`;
  const posts = [];
  for (let index = 0; index < 21; index++) {
    const result = await postRpc("babel.object.publish_text.v1", {
      author_id: target.id, text: `${search} post ${index}`,
    }, `following-verification-${process.pid}-${index}`);
    posts.push(result.object);
  }
  posts.sort((left, right) => {
    const time = Date.parse(right.created_at) - Date.parse(left.created_at);
    const remainder = timestamp => Number(
      (timestamp.match(/\.(\d+)(?:Z|[+-]\d{2}:\d{2})$/i)?.[1] ?? "").padEnd(9, "0").slice(3, 9),
    );
    return time || remainder(right.created_at) - remainder(left.created_at)
      || (right.id > left.id ? 1 : right.id < left.id ? -1 : 0);
  });
  await verifyFeedPreferences({ execute: postAegisExecute, waitFor: waitForAegisEval, navigate: navigateAegis,
    identityId, targetIdentityId: target.id, password: accountPassword, search });
  if (includeFollowing) await verifyFollowing({ postAegisExecute, waitForAegisEval,
    identityId, targetIdentityId: target.id, targetHandle: target.handle, search,
    targetPostIds: posts.map(object => object.id) });
}

// Observe registration in this run's disposable store; never patch the app's
// module instances or fetch transport to obtain the fixture's document context.
function readSurfaceDocument({ actorId, objectId }) {
  assert.equal(typeof actorId, "string");
  assert.equal(typeof objectId, "string");
  const db = new DatabaseSync(join(storeRoot, "auth/accounts.sqlite3"), { readOnly: true });
  try {
    const rows = db.prepare(`SELECT session_id AS sessionId, document_id AS documentId
      FROM surface_owners WHERE identity_id = ? AND object_id = ?
      AND retired = 0 AND document_id IS NOT NULL`).all(actorId, objectId);
    assert.equal(rows.length, 1, "fixture must have exactly one registered live Surface document");
    return { ...rows[0] };
  } finally { db.close(); }
}

// Observe real transferred-port traffic without relying on a transient status
// label. The opaque child is not made same-origin for test convenience.
async function observeSeedBridge() {
  await postAegisExecute([{ type: "eval", code: `(() => {
    const state = window.__seedBridgeProbe = { controls: [], request: null, response: null, restore: [] };
    const listener = event => {
      const frame = document.querySelector('[data-surface-host] iframe');
      if (event.source !== frame?.contentWindow || event.data?.type !== 'babel.surface.connect' || event.ports.length !== 1) return;
      const port = event.ports[0], send = port.postMessage;
      state.origin = event.origin;
      const receive = message => {
        if (message.data?.type === 'babel.rpc.request' && message.data.envelope?.id === 'surface-auto-1') state.request = message.data.envelope;
      };
      port.addEventListener('message', receive);
      port.postMessage = function(message, ...args) {
        send.call(this, message, ...args);
        if (message?.type?.startsWith('babel.surface.')) state.controls.push(message.type);
        if (message?.type === 'babel.rpc.response' && message.response?.id === 'surface-auto-1') state.response = message.response;
      };
      state.restore.push(() => { port.postMessage = send; port.removeEventListener('message', receive); });
    };
    window.addEventListener('message', listener);
    state.cleanup = () => {
      window.removeEventListener('message', listener);
      for (const restore of state.restore) restore();
      delete window.__seedBridgeProbe;
    };
    return { observing: true };
  })()` }]);
}

async function verifySeedBridge() {
  try {
    const result = await waitForAegisEval(`(() => {
      const s = window.__seedBridgeProbe;
      return { origin: s.origin, controls: s.controls, request: s.request, response: s.response,
        active: document.querySelector('[data-surface-host]')?.dataset.state === 'active' };
    })()`, value => value.response !== null && value.active);
    assert.equal(result.origin, "null");
    assert.deepEqual(result.controls, ["babel.surface.accept", "babel.surface.ready"]);
    assert.equal(result.request.method, "babel.search.objects.v1");
    assert.equal(result.response.error, null, JSON.stringify(result.response));
    assert.equal(result.response.result.results.length, 1);
    console.log("Bundled Surface bridge PASS: opaque-origin handshake, real RPC request and successful backend response, active lifecycle");
  } finally {
    await postAegisExecute([{ type: "eval", code: "window.__seedBridgeProbe?.cleanup(); ({ cleaned: true })" }]);
  }
}

async function stopApi() {
  await stopChild(apiProcess);
}

async function stopChild(child) {
  await lifecycle.stop(child);
}

async function startAstro() {
  astroProcess = lifecycle.spawn(process.execPath, [join(root, "tests/astro-server.mjs"), String(frontendPort), join(storeRoot, "astro"), frontendMode], {
    cwd: join(root, "frontend"),
    env: {
      ...process.env,
      PUBLIC_BABEL_API_URL: apiUrl,
      ASTRO_TELEMETRY_DISABLED: "1",
    },
    stdio: ["ignore", "pipe", "pipe", "ipc"],
  });
  astroProcess.stdout.on("data", chunk => process.stdout.write(`[astro] ${chunk}`));
  astroProcess.stderr.on("data", chunk => process.stderr.write(`[astro] ${chunk}`));
  const ready = await new Promise((resolve, reject) => {
    const child = astroProcess;
    const cleanup = () => {
      clearTimeout(timer);
      child.off("message", message);
      child.off("exit", exit);
      child.off("error", error);
    };
    const message = value => { cleanup(); resolve(value); };
    const exit = (code, signal) => { cleanup(); reject(new Error(`Astro exited before readiness: ${code ?? signal}`)); };
    const error = cause => { cleanup(); reject(cause); };
    const timer = setTimeout(() => { cleanup(); reject(new Error("Astro did not announce readiness within 120 seconds")); }, 120_000);
    child.once("message", message);
    child.once("exit", exit);
    child.once("error", error);
  });
  assert.deepEqual(ready, { ready: true, port: frontendPort, mode: frontendMode });
}

async function verifyProductionAssets() {
  const html = await (await fetch(frontendUrl)).text();
  assert.doesNotMatch(html, /\/@vite\/|astro-dev-toolbar|src="\/src\//, "production page must not load development runtime");
  assert.doesNotMatch(html, /__test_modules/, "production application must not load auxiliary test modules");
  const assets = [...html.matchAll(/(?:src|href)="(\/assets\/[^"?#]+\.(?:js|css))"/g)].map(match => match[1]);
  assert.ok(assets.some(path => path.endsWith(".js")), "production page must load built JavaScript");
  for (const path of new Set(assets)) {
    const response = await fetch(new URL(path, frontendOrigin));
    assert.equal(response.status, 200, `missing production asset ${path}`);
    assert.match(response.headers.get("content-type"), path.endsWith(".js") ? /javascript/ : /css/);
    const content = await response.text();
    assert.ok(content.length > 0);
    assert.doesNotMatch(content, /__test_modules/, "production assets must not reference auxiliary test modules");
  }
  for (const path of ["/@vite/client", "/src/app/protocol.ts"]) {
    assert.equal((await fetch(new URL(path, frontendOrigin))).status, 404, "development source must not be served");
  }
  console.log(JSON.stringify({ phase: "production-assets", frontendMode, assets: [...new Set(assets)], auxiliaryModules: "compiled test fixtures only" }));
}

async function waitForJson(url, predicate) {
  await waitFor(async () => {
    const response = await fetch(url);
    if (!response.ok) {
      return false;
    }
    return predicate(await response.json());
  }, `JSON endpoint ${url}`);
}

async function waitForText(url, expected) {
  await waitFor(async () => {
    const response = await fetch(url);
    if (!response.ok) {
      return false;
    }
    return (await response.text()).includes(expected);
  }, `text endpoint ${url}`);
}

async function waitFor(check, label) {
  const startedAt = Date.now();
  let lastError;
  while (Date.now() - startedAt < 120_000) {
    if (apiProcess && (apiProcess.exitCode !== null || apiProcess.signalCode !== null)) {
      throw new Error(`API stopped while waiting for ${label}`);
    }
    if (astroProcess && (astroProcess.exitCode !== null || astroProcess.signalCode !== null)) {
      throw new Error(`Astro stopped while waiting for ${label}`);
    }
    try {
      if (await check()) {
        return;
      }
    } catch (error) {
      lastError = error;
    }
    await delay(250);
  }
  throw new Error(`${label} did not become ready${lastError ? `: ${lastError.message}` : ""}`);
}

async function postJson(url, body) {
  const response = await fetch(url, {
    method: "POST",
    headers: { "content-type": "application/json", ...(authToken ? { authorization: `Bearer ${authToken}` } : {}) },
    body: JSON.stringify(body),
  });
  assert.equal(response.ok, true, `${url} returned ${response.status}`);
  return response.json();
}

async function postRpc(method, payload, idempotencyKey = `live-${method}-${process.pid}-${Date.now()}`) {
  const response = await postJson(`${apiUrl}/rpc`, {
    protocol: "babel.rpc.v1",
    id: `live-${method}-${process.pid}-${Date.now()}`,
    method,
    binding: {
      object_id: null,
      surface_session_id: null,
      runtime_id: "babel-live-stack",
      origin: frontendOrigin,
      capability_grants: [],
    },
    payload,
    idempotency_key: idempotencyKey,
    deadline: {
      timeout_ms: 30000,
      client_started_at: new Date().toISOString(),
    },
    trace_id: null,
  });
  assert.equal(response.error, null);
  assert.ok(response.result, `${method} should return a result`);
  return response.result;
}

async function ensureAegis() {
  try {
    runAegis(["page", "inspect"]);
    return;
  } catch {
    execFileSync("aegis", ["--mode", "headless", "serve", "--detach", "--addr", aegisAddr], {
      env: process.env,
      cwd: root,
      encoding: "utf8",
      stdio: "pipe",
    });
    await delay(1_000);
  }
}

async function waitForAegisUrl(expectedOrigin) {
  let lastUrl = "";
  await waitFor(async () => {
    const page = JSON.parse(runAegis(["page", "inspect"]));
    lastUrl = page.url ?? "";
    return lastUrl.startsWith(expectedOrigin);
  }, `Aegis navigation to ${expectedOrigin}${lastUrl ? `; last URL ${lastUrl}` : ""}`);
}

function runAegis(args) {
  return execFileSync("aegis", ["--server-addr", aegisAddr, ...args], {
    cwd: root,
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
}

async function waitForAegisText(pattern) {
  let lastText = "";
  await waitFor(async () => {
    lastText = runAegis(["page", "text", "--scope", "main"]);
    return pattern.test(lastText);
  }, `Aegis page text ${pattern}`);
  return lastText;
}

async function waitForAegisEval(code, predicate) {
  let lastValue = null;
  try { await waitFor(async () => {
    const body = await postAegisExecute([{ type: "eval", code }]);
    assert.equal(body.results.length, 1);
    const result = body.results[0];
    assert.equal(result.ok, true, JSON.stringify(result));
    lastValue = result.value;
    return predicate(lastValue);
  }, "Aegis execute predicate"); }
  catch (cause) { throw new Error(`${cause.message}; last value: ${JSON.stringify(lastValue)}; expression: ${code}`); }
  return lastValue;
}

async function postAegisExecute(commands) {
  if (frontendMode === "production") commands = commands.map(command => command.type === "eval"
    ? { ...command, code: rewriteBrowserTestImports(command.code) } : command);
  const response = await fetch(`http://${aegisAddr}/execute`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ commands }),
    signal: AbortSignal.timeout(15_000),
  });
  assert.equal(response.ok, true, `Aegis execute returned ${response.status}`);
  return response.json();
}

async function navigateAegis(url) {
  const response = await fetch(`http://${aegisAddr}/navigate`, {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ url }), signal: AbortSignal.timeout(15_000),
  });
  assert.equal(response.ok, true, `Aegis navigation returned ${response.status}`);
  return response.json();
}
