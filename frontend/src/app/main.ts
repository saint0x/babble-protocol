import { renderDeck } from "./cards";
import { Conversations } from "./conversations";
import { Accounts } from "./accounts";
import { AccountPanel } from "./account-panel";
import { Profiles } from "./profiles";
import { ObjectVisits } from "./object-visits";
import { Quotes } from "./quotes";
import { FeedPreferences } from "./feed-preferences";
import { PreferencesView } from "./preferences-view";
import type { LocalPreferences as StoredLocalPreferences } from "./local-preferences";
import { FollowingClient, FollowingPages, type FollowingView } from "./following";
import { FollowingControls } from "./following-view";
import { SafetyClient, type SafetySnapshot } from "./safety";
import { SafetyControls } from "./safety-view";
import { ModerationClient, type ModerationCase } from "./moderation";
import { ModerationControls } from "./moderation-view";
import { ReactionPanel } from "./reaction-view";
import { agreementSummary, judgmentInputView } from "./judgment-view";
import { captureReading, restoreReading, type ReadingPosition } from "./reading";
import { Drafts, draftTransport, type DraftTarget } from "./drafts";
import { ComposerView, composerAlbumError } from "./composer";
import { BundlePicker } from "./bundle-picker";
import { publishBundle } from "./bundle-publication";
import { PermissionPanel } from "./permission-view";
import { Surfaces, type SurfaceState } from "./surfaces";
import { HostActions } from "./host-actions";
import { SurfaceInvocations } from "./surface-invocations";
import { ChevronLeft, ChevronRight, createElement, Maximize2, Minimize2, Plus, RotateCw, ShieldCheck, Undo2, X } from "lucide";
import {
  BabbleFrontendClient,
  judgmentDefinitions,
  type FeedCard,
  type FeedDiversity,
  type JudgmentDefinition,
  type LensMode,
  type FeedPersonalization,
  type ObjectJudgment,
  type PlatformOverview,
  type SocialTextKind,
} from "./protocol";
import { createPersonalizationFilter, summarizeDiscoveryObject, type LocalUserModelInput } from "@babble-protocol/sdk";

const deck = required(document.querySelector<HTMLElement>("[data-deck]"), "deck");
const empty = required(document.querySelector<HTMLElement>("[data-empty]"), "empty state");
const error = required(document.querySelector<HTMLElement>("[data-error]"), "error state");
const status = required(document.querySelector<HTMLElement>("[data-status]"), "connection status");
const statusText = required(document.querySelector<HTMLElement>("[data-status-text]"), "connection status label");
const catalogLabel = required(document.querySelector<HTMLElement>("[data-catalog-label]"), "catalog label");
const sourceLabel = required(document.querySelector<HTMLElement>("[data-source-label]"), "source label");
const localLabel = required(document.querySelector<HTMLElement>("[data-local-label]"), "local personalization label");
const diversityLabel = required(document.querySelector<HTMLElement>("[data-diversity-label]"), "diversity label");
const countLabel = required(document.querySelector<HTMLElement>("[data-count-label]"), "count label");
const positionLabel = required(document.querySelector<HTMLElement>("[data-position-label]"), "position label");
const lensLabel = required(document.querySelector<HTMLElement>("[data-lens-label]"), "Lens label");
const feedSummary = required(document.querySelector<HTMLElement>("[data-feed-summary]"), "feed summary");
const searchForm = required(document.querySelector<HTMLFormElement>("[data-search-form]"), "search form");
const searchInput = required(document.querySelector<HTMLInputElement>("[data-search-input]"), "search input");
const lensButtons = [...document.querySelectorAll<HTMLButtonElement>("[data-lens]")];
const composeForm = required(document.querySelector<HTMLFormElement>("[data-compose-form]"), "composer form");
const authorHandleInput = required(document.querySelector<HTMLInputElement>("[data-author-handle]"), "author handle");
const composeText = required(document.querySelector<HTMLTextAreaElement>("[data-compose-text]"), "composer text");
const composeMedia = required(document.querySelector<HTMLInputElement>("[data-compose-media]"), "composer media");
const composeSubmit = required(document.querySelector<HTMLButtonElement>("[data-compose-submit]"), "composer submit");
const authorStatus = required(document.querySelector<HTMLElement>("[data-author-status]"), "author status");
const surfacePanel = required(document.querySelector<HTMLElement>("[data-surface-panel]"), "surface panel");
const surfaceTitle = required(document.querySelector<HTMLElement>("[data-surface-title]"), "surface title");
const surfaceBridgeStatus = required(document.querySelector<HTMLElement>("[data-surface-bridge-status]"), "Surface bridge status");
const surfaceMeta = required(document.querySelector<HTMLElement>("[data-surface-meta]"), "Surface metadata");
const surfaceHost = required(document.querySelector<HTMLElement>("[data-surface-host]"), "surface host");
const judgmentPanel = required(document.querySelector<HTMLElement>("[data-judgment-panel]"), "Judgment panel");
const judgmentTitle = required(document.querySelector<HTMLElement>("[data-judgment-title]"), "Judgment title");
const judgmentStatus = required(document.querySelector<HTMLElement>("[data-judgment-status]"), "Judgment status");
const judgmentList = required(document.querySelector<HTMLElement>("[data-judgment-list]"), "Judgment list");
const judgmentForm = required(document.querySelector<HTMLFormElement>("[data-judgment-form]"), "Judgment form");
const judgmentDefinitionInput = required(
  document.querySelector<HTMLSelectElement>("[data-judgment-definition]"),
  "Judgment definition",
);
const judgmentQueryInput = required(document.querySelector<HTMLInputElement>("[data-judgment-query]"), "Judgment query");
const judgmentQueryLabel = required(
  document.querySelector<HTMLElement>("[data-judgment-query-label]"),
  "Judgment query label",
);
const judgmentRelationInput = required(
  document.querySelector<HTMLSelectElement>("[data-judgment-relation]"),
  "Judgment relation",
);
const judgmentRelationLabel = required(
  document.querySelector<HTMLElement>("[data-judgment-relation-label]"),
  "Judgment relation label",
);
const judgmentSubmit = required(document.querySelector<HTMLButtonElement>("[data-judgment-submit]"), "Judgment submit");
const prevButton = required(document.querySelector<HTMLButtonElement>("[data-prev]"), "previous button");
const nextButton = required(document.querySelector<HTMLButtonElement>("[data-next]"), "next button");
const refreshButton = required(document.querySelector<HTMLButtonElement>("[data-refresh]"), "refresh button");
const closeSurfaceButton = required(document.querySelector<HTMLButtonElement>("[data-close-surface]"), "close Surface button");
const closeJudgmentsButton = required(
  document.querySelector<HTMLButtonElement>("[data-close-judgments]"),
  "close Judgments button",
);
const reloadJudgmentsButton = required(
  document.querySelector<HTMLButtonElement>("[data-reload-judgments]"),
  "reload Judgments button",
);
const composerPanel = document.querySelector<HTMLElement>("[data-composer-panel]");
const toggleComposerButton = document.querySelector<HTMLButtonElement>("[data-toggle-composer]");
const closeComposerButton = document.querySelector<HTMLButtonElement>("[data-close-composer]");
const toggleProfileButton = document.querySelector<HTMLButtonElement>("[data-toggle-profile]");
const profileDropdown = document.querySelector<HTMLElement>("[data-profile-dropdown]");
const profileMenu = document.querySelector<HTMLElement>("[data-profile-menu]");
const settingsPanel = document.querySelector<HTMLElement>("[data-settings-panel]");
const helpPanel = document.querySelector<HTMLElement>("[data-help-panel]");
const closeSettingsButton = document.querySelector<HTMLButtonElement>("[data-close-settings]");
const closeHelpButton = document.querySelector<HTMLButtonElement>("[data-close-help]");
const settingsSourceLabel = document.querySelector<HTMLElement>("[data-settings-source]");
const settingsLocalLabel = document.querySelector<HTMLElement>("[data-settings-local]");
const settingsDiversityLabel = document.querySelector<HTMLElement>("[data-settings-diversity]");
const settingsCatalogLabel = document.querySelector<HTMLElement>("[data-settings-catalog]");
const settingsCountLabel = document.querySelector<HTMLElement>("[data-settings-count]");
const settingsPositionLabel = document.querySelector<HTMLElement>("[data-settings-position]");
const refreshPlatformButton = document.querySelector<HTMLButtonElement>("[data-refresh-platform]");
const platformStatus = document.querySelector<HTMLElement>("[data-platform-status]");
const platformCapabilities = document.querySelector<HTMLElement>("[data-platform-capabilities]");
const platformLenses = document.querySelector<HTMLElement>("[data-platform-lenses]");
const platformJudges = document.querySelector<HTMLElement>("[data-platform-judges]");
const platformCapabilityList = document.querySelector<HTMLElement>("[data-platform-capability-list]");

const apiUrl = document.documentElement.dataset.babbleApi;
if (!apiUrl) {
  throw new Error("Babble frontend requires data-babble-api on the document element");
}

const PANEL_ANIMATION_MS = 220;
const panelAnimations = new WeakMap<HTMLElement, { frame: number | null; timer: number | null }>();
const accounts = new Accounts(apiUrl, sessionStorageOrNull());
const drafts = new Drafts({ origin: accounts.origin.origin, identityId: accounts.current?.identity.id ?? null }, draftStorage());
const bundlePicker = new BundlePicker(required(document.querySelector<HTMLElement>("[data-bundle-picker]"), "application picker"), (bundle) => {
  saveComposerDraft();
  drafts.editBundle(bundle);
  renderComposerDraft();
});
const composerView = new ComposerView(required(composerPanel, "composer panel"), (files) => {
  if (drafts.current?.pending) return;
  drafts.edit(composeText.value, files);
  renderComposerDraft();
});
const client = new BabbleFrontendClient(apiUrl, accounts.fetch);
const followingClient = new FollowingClient(apiUrl, accounts.authenticatedFetch);
let safetyRevision: { owner: string; revision: number } | null = null;
let safetyUnavailable = false;
let profileSafetyTarget: string | null = null;
const safetyControls = new SafetyControls({
  source: new SafetyClient(apiUrl, accounts.authenticatedFetch),
  signIn: () => accountPanel.open(), changed: safetyChanged,
});
const moderationControls = new ModerationControls({
  source: new ModerationClient(apiUrl, accounts.authenticatedFetch),
  signIn: () => accountPanel.open(),
  openObject: (objectId, opener) => { void openReportedObject(objectId, opener); },
  changed: refreshModeratedFeed,
});
const conversations = new Conversations(async (objectId, cursor) => {
  await ensureFeedSafety();
  const page = await client.replies(objectId, cursor);
  return { ...page, replies: page.replies.filter(card => !safetyControls.blocked(card.author)) };
});
const quotes = new Quotes(async (objectId, cursor, signal) => {
  await ensureFeedSafety();
  signal.throwIfAborted();
  const page = await client.quotes(objectId, cursor, signal);
  return { ...page, items: page.items.filter(item => !item.card || !safetyControls.blocked(item.card.author)) };
},
  (card, opener) => objectVisits.open(card, opener));
const initialParams = new URLSearchParams(window.location.search);
const initialQuery = initialParams.get("q") ?? "";
let activeLens = lensMode(initialParams.get("lens"));
const initialSurface = initialParams.get("surface");
let cards: readonly FeedCard[] = [];
let currentIndex = 0;
let surfaceOpenSequence = 0;
let surfaceRestoreFocus = false;
let surfaceActions: HostActions | null = null;
let surfaceInvocations: SurfaceInvocations | null = null;
let surfacePlacement: {
  column: HTMLElement; primary: HTMLElement; opener: HTMLElement | null; reading: ReadingPosition; objectId: string;
  returnFeed?: { cards: readonly FeedCard[]; index: number; source: string | null; reading: ReadingPosition | null };
} | null = null;
const surfaceHome = document.createComment("inline Surface home");
surfacePanel.before(surfaceHome);
const expandSurfaceButton = required(document.querySelector<HTMLButtonElement>("[data-expand-surface]"), "expand Surface");
const retrySurfaceButton = required(document.querySelector<HTMLButtonElement>("[data-retry-surface]"), "retry Surface");
closeSurfaceButton.replaceChildren(createElement(X, { "aria-hidden": "true" }));
prevButton.replaceChildren(createElement(ChevronLeft, { "aria-hidden": "true" }));
nextButton.replaceChildren(createElement(ChevronRight, { "aria-hidden": "true" }));
refreshButton.replaceChildren(createElement(RotateCw, { "aria-hidden": "true" }));
document.querySelector("[data-toggle-composer]")?.replaceChildren(createElement(Plus, { "aria-hidden": "true" }));
for (const button of document.querySelectorAll<HTMLButtonElement>(".header-icon, .carousel-control")) {
  button.title = button.getAttribute("aria-label") ?? "";
}
expandSurfaceButton.replaceChildren(createElement(Maximize2, { "aria-hidden": "true" }));
retrySurfaceButton.replaceChildren(createElement(RotateCw, { "aria-hidden": "true" }));
const surfaces = new Surfaces({
  onState: renderSurfaceState,
  onCleanupError: (cause) => showError(errorMessage(cause)),
});
const permissions = new PermissionPanel(
  required(document.querySelector<HTMLDialogElement>("[data-permission-dialog]"), "permission dialog"),
  accounts, () => closeSurface(), id => void openSurface(id), () => accountPanel.open(),
);
const permissionButton = required(document.querySelector<HTMLButtonElement>("[data-surface-permissions]"), "Surface permissions");
permissionButton.replaceChildren(createElement(ShieldCheck, { "aria-hidden": "true" }));
permissionButton.addEventListener("click", () => {
  const id = surfacePlacement?.objectId;
  if (id) permissions.open(id);
});
window.addEventListener("pagehide", () => closeSurface());
window.addEventListener("pageshow", () => surfaces.checkLease());
document.addEventListener("visibilitychange", () => surfaces.checkLease());
let pendingSurfaceObjectId: string | null = null;
let activeSurfaceLabel = "Prepared runtime";
let activeJudgmentObjectId: string | null = null;
let judgmentRequestSequence = 0;
let loadSequence = 0;
let accountFeedRefresh: Promise<void> = Promise.resolve();
let author: StoredAuthor | null = null;
let localPersonalization: FeedPersonalization = {
  boundary: "none",
  filtered: 0,
  modelRevision: null,
};
let feedDiversity: FeedDiversity = {
  active: false,
  filtered: 0,
  floors: [],
  maxSourceShare: null,
};
const seenThisSession = new Set<string>();
let followingDirty = false;
const followingToolbar = required(document.querySelector<HTMLElement>("[data-following-toolbar]"), "Following toolbar");
const followingMore = required(document.querySelector<HTMLButtonElement>("[data-following-more]"), "Following pagination");
const followingSignIn = required(document.querySelector<HTMLButtonElement>("[data-following-signin]"), "Following sign in");
const followingStatus = required(document.querySelector<HTMLElement>("[data-following-status]"), "Following status");
const followingFeed = new FollowingPages<FeedCard>(async (cursor, query, signal) => {
  await ensureFeedSafety();
  signal.throwIfAborted();
  const page = await followingClient.feed(query, cursor, signal);
  const items = await Promise.all(page.objects.map((object) => client.objectToCard({
    object, source: "following", score: null, signals: null, reasons: [],
  })));
  signal.throwIfAborted();
  return { items, next: page.next_cursor };
}, renderFollowingFeed);
const profileBack = required(document.querySelector<HTMLButtonElement>("[data-back-profile]"), "profile return");
let profileReturn: { cards: readonly FeedCard[]; index: number; reading: ReadingPosition | null; source: string | null;
  visits: ReturnType<ObjectVisits["checkpoint"]> } | null = null;
const objectBack = required(document.querySelector<HTMLButtonElement>("[data-back-object]"), "Object return");
objectBack.prepend(createElement(ChevronLeft, { "aria-hidden": "true" }));
const objectVisits = new ObjectVisits({
  capture: () => {
    const current = deck.querySelector<HTMLElement>('.post-card[data-offset="0"]:not([data-exiting])');
    return { cards, index: currentIndex, source: sourceLabel.textContent, reading: current ? captureReading(current) : null };
  },
  beforeVisit: () => {
    ++loadSequence;
    closeSurface();
    closeJudgments();
    toggleComposer(false);
    toggleProfile(false);
  },
  show: (snapshot) => {
    cards = snapshot.cards;
    currentIndex = snapshot.index;
    sourceLabel.textContent = snapshot.source;
    followingToolbar.hidden = activeLens !== "following" || objectVisits.active || !!profileReturn;
    error.hidden = true;
    const resumeFollowing = activeLens === "following" && !objectVisits.active && !profileReturn
      && followingFeed.view.phase !== "idle";
    if (resumeFollowing) renderFollowingFeed(followingFeed.view);
    else {
      render();
      setStatus("online", "Online");
    }
    const current = deck.querySelector<HTMLElement>('.post-card[data-offset="0"]:not([data-exiting])');
    if (current && snapshot.reading) restoreReading(current, snapshot.reading);
    else if (current) current.scrollTop = 0;
  },
  focus: (opener) => {
    const target = opener?.isConnected && !opener.closest('[inert], [hidden]') ? opener
      : deck.querySelector<HTMLElement>('.post-card[data-offset="0"]:not([data-exiting])');
    target?.focus({ preventScroll: true });
  },
  availability: (back) => { objectBack.hidden = !back; },
});
objectBack.addEventListener("click", () => objectVisits.back());
const profiles = new Profiles(
  required(document.querySelector<HTMLDialogElement>("[data-public-profile]"), "public profile"), client,
  (selected, authored) => {
    if (!profileReturn) {
      const current = deck.querySelector<HTMLElement>('.post-card[data-offset="0"]:not([data-exiting])');
      profileReturn = { cards, index: currentIndex, reading: current ? captureReading(current) : null,
        source: sourceLabel.textContent, visits: objectVisits.checkpoint() };
    }
    objectVisits.clear();
    ++loadSequence;
    closeSurface();
    closeJudgments();
    cards = authored;
    currentIndex = Math.max(0, authored.findIndex((card) => card.id === selected.id));
    profileBack.hidden = false;
    followingToolbar.hidden = true;
    sourceLabel.textContent = "public profile";
    error.hidden = true;
    render();
    setStatus("online", "Online");
    deck.querySelector<HTMLElement>('.post-card[data-offset="0"]:not([data-exiting])')?.focus({ preventScroll: true });
  },
  () => {
    profileBack.hidden = true;
    followingToolbar.hidden = activeLens !== "following";
    if (followingDirty && activeLens === "following") {
      followingDirty = false;
      profileReturn = null;
      void loadFeed(searchInput.value);
      return;
    }
    if (!profileReturn) return;
    const saved = profileReturn;
    profileReturn = null;
    closeSurface();
    closeJudgments();
    cards = saved.cards;
    currentIndex = saved.index;
    sourceLabel.textContent = saved.source;
    objectVisits.restore(saved.visits);
    followingToolbar.hidden = activeLens !== "following" || objectVisits.active;
    if (activeLens === "following") renderFollowingFeed(followingFeed.view);
    render();
    const current = deck.querySelector<HTMLElement>('.post-card[data-offset="0"]:not([data-exiting])');
    if (current && saved.reading) restoreReading(current, saved.reading);
    current?.focus({ preventScroll: true });
  },
  profileTargetChanged,
);
const followingControls = new FollowingControls(followingClient, () => {
  profiles.close();
  accountPanel.open();
}, openPublicProfile, () => { followingDirty = true; });
const authorControlsButton = required(document.querySelector<HTMLButtonElement>("[data-author-controls]"), "author controls");
authorControlsButton.replaceChildren(createElement(ShieldCheck, { "aria-hidden": "true" }));
authorControlsButton.addEventListener("click", () => {
  if (profileSafetyTarget) safetyControls.openAuthor(profileSafetyTarget, authorControlsButton);
});
followingMore.addEventListener("click", () => void followingFeed.more());
followingSignIn.addEventListener("click", () => accountPanel.open());
document.querySelector("[data-following-people]")?.addEventListener("click", (event) => {
  profiles.close();
  followingControls.openList(event.currentTarget instanceof HTMLElement ? event.currentTarget : null);
});
profileBack.addEventListener("click", () => {
  closeSurface();
  closeJudgments();
  toggleComposer(false);
  profiles.resume();
});

function openPublicProfile(identityId: string, opener: HTMLElement | null): void {
  toggleProfile(false);
  toggleComposer(false);
  toggleSettings(false);
  toggleHelp(false);
  closeSurface();
  closeJudgments();
  profiles.open(identityId, opener);
}

const accountPanel = new AccountPanel(accounts, () => {
  profiles.close();
  closeSurface();
  drafts.invalidate();
  toggleComposer(false);
});
const reactions = new ReactionPanel(accounts, { signIn: () => accountPanel.open() });
const preferencesView = new PreferencesView(required(document.querySelector<HTMLElement>("[data-feed-preferences]"), "feed preferences"), {
  save: value => feedPreferences.save(value),
  reset: () => feedPreferences.reset(),
  clearHistory: () => feedPreferences.clearHistory(),
});
const feedPreferences = new FeedPreferences(accounts, localStorageOrNull, preferencesView, {
  changed: refreshLocalFeed,
  seenCount: () => Object.keys(loadSeenObjects()).length,
  historyCleared: () => {
    seenThisSession.clear();
    const current = cards[currentIndex]?.id;
    if (current) seenThisSession.add(current);
  },
});
window.addEventListener("storage", event => {
  if (event.storageArea === localStorageOrNull()) feedPreferences.externalChange(event.key);
});
document.querySelector("[data-open-feed-preferences]")?.addEventListener("click", () => toggleSettings(true));
document.querySelector("[data-account-dialog]")?.addEventListener("close", () => {
  followingControls.resumeAfterSignIn();
  const objectId = pendingSurfaceObjectId;
  pendingSurfaceObjectId = null;
  const identityId = accounts.current?.identity.id;
  if (objectId && identityId) void accountFeedRefresh.then(() => {
    if (accounts.current?.identity.id === identityId) void openSurface(objectId);
  });
});
accounts.addEventListener("change", syncAccount);
syncAccount();
searchInput.value = initialQuery;
syncLensButtons();
void accounts.restore().catch((cause: unknown) => {
  setAuthorStatus(errorMessage(cause), "error");
}).then(() => loadFeed(initialQuery)).then(() => {
  if (initialSurface === "first") {
    const firstSurface = cards.find(hasSurface);
    if (firstSurface) {
      void openSurface(firstSurface.id);
    }
    return;
  }
  if (initialSurface) {
    void openSurface(initialSurface);
  }
});

searchForm.addEventListener("submit", (event) => {
  event.preventDefault();
  void loadFeed(searchInput.value);
});

composeForm.addEventListener("submit", (event) => {
  event.preventDefault();
  void publishFromComposer();
});
composeText.addEventListener("input", () => {
  saveComposerDraft();
  composerView.resizeText();
});
composeMedia.addEventListener("change", () => {
  if (composeMedia.disabled || !composeMedia.files?.length) return;
  drafts.edit(composeText.value, [...(drafts.current?.media ?? []), ...composeMedia.files]);
  renderComposerDraft();
  const invalid = composerAlbumError(drafts.current?.media ?? []);
  if (invalid) setAuthorStatus(invalid, "error");
});
document.querySelector<HTMLButtonElement>("[data-remove-compose-media]")?.addEventListener("click", () => {
  if (drafts.current?.pending) return;
  drafts.edit(composeText.value, []);
  renderComposerDraft();
  composeMedia.focus();
});
document.querySelector<HTMLButtonElement>("[data-compose-sign-in]")?.addEventListener("click", () => accountPanel.open());
window.addEventListener("pagehide", () => composerView.dispose());
window.addEventListener("pageshow", (event) => { if (event.persisted) renderComposerDraft(false); });

for (const button of lensButtons) {
  button.addEventListener("click", () => {
    const next = lensMode(button.dataset.lens ?? null);
    if (next === activeLens) {
      return;
    }
    activeLens = next;
    syncLensButtons();
    void loadFeed(searchInput.value);
  });
}

prevButton.addEventListener("click", () => move(-1));
nextButton.addEventListener("click", () => move(1));
refreshButton.addEventListener("click", () => void refreshAccountFeed());
window.addEventListener("focus", () => {
  void safetyControls.refresh().catch(cause => setAuthorStatus(errorMessage(cause), "error"));
});
closeSurfaceButton.addEventListener("click", () => closeSurface(true));
retrySurfaceButton.addEventListener("click", () => {
  if (surfacePlacement) void openSurface(surfacePlacement.objectId);
});
expandSurfaceButton.addEventListener("click", () => {
  if (!surfacePlacement) return;
  const expanded = deck.toggleAttribute("data-surface-expanded");
  expandSurfaceButton.setAttribute("aria-pressed", String(expanded));
  expandSurfaceButton.ariaLabel = expanded ? "Collapse Object" : "Expand Object";
  expandSurfaceButton.title = expandSurfaceButton.ariaLabel;
  expandSurfaceButton.replaceChildren(createElement(expanded ? Minimize2 : Maximize2, { "aria-hidden": "true" }));
});
closeJudgmentsButton.addEventListener("click", closeJudgments);
toggleComposerButton?.addEventListener("click", startPublishComposer);
closeComposerButton?.addEventListener("click", () => toggleComposer(false));
toggleProfileButton?.addEventListener("click", () => toggleProfile());
closeSettingsButton?.addEventListener("click", () => toggleSettings(false));
closeHelpButton?.addEventListener("click", () => toggleHelp(false));
refreshPlatformButton?.addEventListener("click", () => void refreshPlatformOverview());
profileDropdown?.addEventListener("click", (event) => {
  const target = event.target;
  if (!(target instanceof HTMLElement)) {
    return;
  }
  const action = target.closest<HTMLButtonElement>("button[data-menu-action]")?.dataset.menuAction;
  if (!action) {
    return;
  }
  toggleProfile(false);
  if (action === "profile") {
    const id = accounts.current?.identity.id;
    if (id) openPublicProfile(id, toggleProfileButton);
    else accountPanel.open();
  }
  if (action === "account") {
    accountPanel.open();
  }
  if (action === "safety") safetyControls.openList(toggleProfileButton);
  if (action === "reports") moderationControls.openInbox(toggleProfileButton);
  if (action === "settings") {
    toggleSettings(true);
  }
  if (action === "help") {
    toggleHelp(true);
  }
  if (action === "sign-out") {
    void accountPanel.logout();
  }
});
reloadJudgmentsButton.addEventListener("click", () => {
  if (activeJudgmentObjectId) {
    void openJudgments(activeJudgmentObjectId);
  }
});
judgmentDefinitionInput.addEventListener("change", syncJudgmentParameterInputs);
judgmentForm.addEventListener("submit", (event) => {
  event.preventDefault();
  void evaluateActiveJudgment();
});

deck.addEventListener("click", (event) => {
  const target = event.target;
  if (!(target instanceof Element)) {
    return;
  }
  const profileAuthor = target.closest<HTMLButtonElement>("[data-profile-author]");
  if (profileAuthor?.dataset.profileAuthor) {
    openPublicProfile(profileAuthor.dataset.profileAuthor, profileAuthor);
    return;
  }
  const action = target.closest<HTMLButtonElement>("button[data-action]")?.dataset.action;
  const objectId = target.closest<HTMLElement>("[data-object-id]")?.dataset.objectId;
  if (!action || !objectId) {
    return;
  }
  if (action === "conversation") {
    const column = target.closest<HTMLElement>(".post-card");
    const conversation = column?.querySelector<HTMLElement>("[data-conversation-root]");
    if (column && conversation) {
      const top = conversation.getBoundingClientRect().top - column.getBoundingClientRect().top + column.scrollTop;
      column.scrollTo({ top, behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches ? "instant" : "smooth" });
      conversation.querySelector<HTMLElement>("h2, h3")?.focus({ preventScroll: true });
    }
  }
  if (action === "surface") {
    void openSurface(objectId);
  }
  if (action === "permissions") {
    permissions.open(objectId, Boolean(cards.find(card => card.id === objectId)?.surfaces.length));
  }
  if (action === "copy-id") {
    void copyObjectId(objectId);
  }
  if (action === "judgments") {
    void openJudgments(objectId);
  }
  if (action === "reply" || action === "share") {
    startSocialTextComposer(action, objectId);
  }
  if (action === "hide-author") {
    hideFeedAuthor(objectId);
  }
  if (action === "author-controls") {
    const card = cards.find(candidate => candidate.id === objectId);
    if (card) safetyControls.openAuthor(card.author, target.closest<HTMLButtonElement>("button[data-action]"));
  }
  if (action === "report") {
    moderationControls.openReport(objectId, target.closest<HTMLButtonElement>("button[data-action]"));
  }
});

window.addEventListener("keydown", (event) => {
  if (event.defaultPrevented || event.isComposing || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) {
    return;
  }
  if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
    if (deckNavigationBlocked() || isInteractiveTarget(event.target, true) || isInteractiveTarget(document.activeElement, true)) {
      return;
    }
    if (cards.length > 0) {
      event.preventDefault();
      move(event.key === "ArrowLeft" ? -1 : 1);
    }
  }
  if (event.key === "Escape") {
    if (document.querySelector("dialog[open]")) return;
    toggleComposer(false);
    toggleProfile(false);
    toggleSettings(false);
    toggleHelp(false);
    closeSurface(true);
    closeJudgments();
  }
});

document.addEventListener("mousedown", (event) => {
  const target = event.target;
  if (target instanceof Node && profileMenu && !profileMenu.contains(target)) {
    toggleProfile(false);
  }
});

installDeckInteractions();

function isInteractiveTarget(target: EventTarget | null, keyboard = false): boolean {
  // Image taps open the viewer; horizontal image drags still navigate the deck.
  if (!keyboard && target instanceof Element && target.closest(".post-image-open")) return false;
  if (keyboard && target instanceof Element && target.getAttribute("class") === "post-card") {
    return false;
  }
  return target instanceof Element && (
    (target instanceof HTMLElement && target.isContentEditable)
    || target.closest(
      "a, button, input, textarea, select, option, label, summary, iframe, audio, video, "
      + "[contenteditable]:not([contenteditable='false']), [role='button'], [role='link'], "
      + "[role='textbox'], [role='combobox'], [role='slider'], [role='spinbutton'], "
      + "[role='menu'], [role='listbox'], [role='tablist'], [data-surface-panel]"
      + (keyboard ? ", [tabindex]:not([tabindex='-1'])" : ""),
    ) !== null
  );
}

function deckNavigationBlocked(): boolean {
  return [composerPanel, profileDropdown, settingsPanel, helpPanel, judgmentPanel]
    .some((panel) => panel !== null && !isHidden(panel))
    || document.querySelector("dialog[open], [role='dialog']:not([hidden]), .popover-panel:not([hidden])") !== null;
}

function installDeckInteractions(): void {
  let gesture: { id: number; x: number; y: number; horizontal: boolean } | null = null;
  let suppressedClick: { id: number; time: number } | null = null;

  function cancelGesture(): void {
    const previous = gesture;
    gesture = null;
    if (previous && deck.hasPointerCapture(previous.id)) {
      deck.releasePointerCapture(previous.id);
    }
  }

  function horizontalIntent(event: PointerEvent): boolean {
    if (!gesture) {
      return false;
    }
    const x = Math.abs(event.clientX - gesture.x);
    const y = Math.abs(event.clientY - gesture.y);
    if (!gesture.horizontal && Math.max(x, y) >= 10) {
      if (y >= x) {
        cancelGesture();
        return false;
      }
      gesture.horizontal = x > y * 1.25;
    }
    return gesture.horizontal;
  }

  deck.addEventListener("pointerdown", (event) => {
    suppressedClick = null;
    if (gesture) {
      cancelGesture();
      return;
    }
    if (!event.isPrimary || event.button !== 0 || event.defaultPrevented
      || isInteractiveTarget(event.target) || deckNavigationBlocked() || cards.length < 2) {
      return;
    }
    gesture = { id: event.pointerId, x: event.clientX, y: event.clientY, horizontal: false };
  });

  window.addEventListener("pointermove", (event) => {
    if (!gesture || gesture.id !== event.pointerId) {
      return;
    }
    if (deckNavigationBlocked()) {
      cancelGesture();
      return;
    }
    if (horizontalIntent(event)) {
      if (!deck.hasPointerCapture(event.pointerId)) {
        deck.setPointerCapture(event.pointerId);
      }
      event.preventDefault();
    }
  });

  window.addEventListener("pointerup", (event) => {
    if (!gesture || gesture.id !== event.pointerId) {
      return;
    }
    const delta = event.clientX - gesture.x;
    const horizontal = horizontalIntent(event);
    cancelGesture();
    if (!horizontal) {
      return;
    }
    // A completed drag must not activate whatever is under its release point.
    suppressedClick = { id: event.pointerId, time: event.timeStamp };
    if (Math.abs(delta) > 42 && !deckNavigationBlocked()) {
      move(delta < 0 ? 1 : -1);
    }
  });

  window.addEventListener("pointercancel", (event) => {
    if (gesture?.id === event.pointerId) {
      cancelGesture();
    }
  });
  deck.addEventListener("lostpointercapture", (event) => {
    // Touch begins with implicit capture on the child; transferring it is not cancellation.
    if (event.target === deck && gesture?.id === event.pointerId) {
      cancelGesture();
    }
  });
  window.addEventListener("blur", cancelGesture);
  deck.addEventListener("surfacechange", cancelGesture);
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) {
      cancelGesture();
    }
  });
  deck.addEventListener("dragstart", (event) => {
    if (gesture) {
      event.preventDefault();
    }
  });
  deck.addEventListener("click", (event) => {
    if (!suppressedClick || event.detail === 0) {
      return;
    }
    const previous = suppressedClick;
    suppressedClick = null;
    if (event.timeStamp - previous.time <= 500
      && (!(event instanceof PointerEvent) || event.pointerId === previous.id)) {
      event.preventDefault();
      event.stopImmediatePropagation();
    }
  }, true);

  window.addEventListener("resize", cancelGesture);
}

async function publishFromComposer(): Promise<void> {
  const session = accounts.current;
  if (!session) {
    accountPanel.open();
    setAuthorStatus("Sign in before publishing", "error");
    return;
  }
  saveComposerDraft();
  const draft = drafts.current;
  if (!draft || draft.pending) return;
  const text = draft.text.trim();
  if (bundlePicker.error) {
    setAuthorStatus(bundlePicker.error, "error");
    return;
  }
  if (draft.bundle && !text) {
    setAuthorStatus("Add Object text before publishing an application", "error");
    composeText.focus();
    return;
  }
  const mediaSelection = selectedMediaFiles();
  if (!mediaSelection.valid) return;
  if (!text && !mediaSelection.files.length && !draft.bundle) {
    setAuthorStatus(draft.target.mode === "publish" ? "Write a post or add media before publishing"
      : `Write text or add media before ${draft.target.mode === "reply" ? "replying" : "sharing"}`, "error");
    composeText.focus();
    return;
  }
  const submission = drafts.begin();
  if (!submission) return;
  const authorized = () => accounts.current?.token === session.token && drafts.owns(submission);
  const publisher = new BabbleFrontendClient(accounts.origin.href, draftTransport(accounts.fetch, authorized, submission.operationId), submission.operationId);
  const { target } = submission;
  renderComposerDraft();
  let published: Awaited<ReturnType<BabbleFrontendClient["publishText"]>>;
  try {
    published = submission.media.length
      ? await publishMediaWithAuthor(publisher, session.identity.id, text, submission.media, target)
      : target.mode !== "publish"
        ? await publisher.socialText(target.mode, session.identity.id, target.parent, text)
      : submission.bundle
        ? await publishBundle(publisher, session.identity.id, text, submission.bundle)
        : await publisher.publishText(session.identity.id, text);
  } catch (cause) {
    const active = drafts.current?.pending === submission;
    const current = drafts.finish(submission, false);
    if (active) renderComposerDraft();
    if (current) setAuthorStatus(`Publish failed: ${errorMessage(cause)}`, "error");
    return;
  }
  const active = drafts.current?.pending === submission;
  const current = drafts.finish(submission, true);
  if (active) renderComposerDraft();
  if (!current || !authorized()) return;
  toggleComposer(false);
  setAuthorStatus(`Published ${compactId(published.id)} from ${session.identity.handle}`, "ready");
  const view = drafts.viewRevision;
  const currentView = () => authorized() && drafts.viewRevision === view;
  try {
    if (target.mode === "reply") {
      const card = await client.describeObject(published);
      if (currentView()) await conversations.refresh(target.parent, card);
    } else {
      const confirmed = await client.objectToCard({
        object: published, score: null, source: "published", signals: null, reasons: [],
      });
      if (currentView()) await loadFeed(searchInput.value, published.id, currentView, confirmed);
    }
  } catch (cause) {
    // Publication already succeeded; a refresh failure must not invite a duplicate retry.
    if (currentView()) {
      setAuthorStatus(`Published ${compactId(published.id)}; refresh failed: ${errorMessage(cause)}`, "error");
    }
  }
}

function startPublishComposer(): void {
  saveComposerDraft();
  drafts.open({ mode: "publish", parent: null });
  renderComposerDraft();
  toggleComposer(true);
  if (!accounts.current) accountPanel.open();
}

function startSocialTextComposer(kind: SocialTextKind, objectId: string): void {
  saveComposerDraft();
  drafts.open({ mode: kind, parent: objectId });
  renderComposerDraft();
  toggleComposer(true);
  if (!accounts.current) accountPanel.open();
}

function saveComposerDraft(): void {
  drafts.edit(composeText.value, drafts.current?.media ?? []);
  if (drafts.storageError) setAuthorStatus(drafts.storageError, "error");
}

function renderComposerDraft(showStatus = true): void {
  const draft = drafts.current;
  composeText.value = draft?.text ?? "";
  composeMedia.value = "";
  if (draft?.media.length && typeof DataTransfer !== "undefined") {
    const transfer = new DataTransfer();
    for (const file of draft.media) transfer.items.add(file);
    composeMedia.files = transfer.files;
  }
  const mode = draft?.target.mode ?? "publish";
  composeSubmit.textContent = draft?.pending ? "Publishing..." : mode === "publish" ? "Post" : mode === "reply" ? "Reply" : "Share";
  composeSubmit.disabled = !!draft?.pending || !accounts.current;
  composeMedia.disabled = !!draft?.pending;
  bundlePicker.render(draft?.bundle ?? null, mode === "publish", !!draft?.pending);
  composerView.render(draft, accounts.current?.identity.handle ?? null);
  if (!showStatus) return;
  const invalid = composerAlbumError(draft?.media ?? []);
  setAuthorStatus(draft?.pending ? "Publishing..."
    : invalid ?? drafts.storageError ?? (!accounts.current ? "Sign in to publish" : draft?.media.length || draft?.bundle
      ? "Attachments stay in this tab" : draft?.text ? "Draft saved" : "Draft ready"),
  draft?.pending ? "busy" : invalid || drafts.storageError ? "error" : "ready");
}

function draftStorage(): Storage | null {
  try { return window.localStorage; }
  catch { return null; }
}

function toggleComposer(force?: boolean): void {
  if (!composerPanel) {
    return;
  }
  const open = force === undefined ? isHidden(composerPanel) : force;
  if (!open) {
    saveComposerDraft();
    drafts.close();
  }
  setAnimatedVisibility(composerPanel, open);
  composerView.visibility(open);
  if (open) {
    toggleProfile(false);
    toggleSettings(false);
    toggleHelp(false);
    composeText.focus();
  }
}

function toggleProfile(force?: boolean): void {
  if (!profileDropdown) {
    return;
  }
  setAnimatedVisibility(profileDropdown, force === undefined ? isHidden(profileDropdown) : force);
}

function toggleSettings(force?: boolean): void {
  if (!settingsPanel) {
    return;
  }
  const open = force === undefined ? isHidden(settingsPanel) : force;
  setAnimatedVisibility(settingsPanel, open);
  if (open) {
    feedPreferences.show();
    settingsPanel.querySelector<HTMLElement>('[data-preference-tab][aria-selected="true"]')?.focus({ preventScroll: true });
    toggleComposer(false);
    toggleHelp(false);
    closeSurface();
    closeJudgments();
    void refreshPlatformOverview();
  } else if (settingsPanel.contains(document.activeElement)) {
    toggleProfileButton?.focus({ preventScroll: true });
  }
}

function toggleHelp(force?: boolean): void {
  if (!helpPanel) {
    return;
  }
  const open = force === undefined ? isHidden(helpPanel) : force;
  setAnimatedVisibility(helpPanel, open);
  if (open) {
    toggleComposer(false);
    toggleSettings(false);
    closeSurface();
    closeJudgments();
  }
}

function syncAccount(): void {
  const identity = accounts.current?.identity;
  feedPreferences.account();
  const draftOwnerChanged = drafts.setOwner({ origin: accounts.origin.origin, identityId: identity?.id ?? null });
  const changed = author?.identityId !== identity?.id;
  if (changed) safetyUnavailable = false;
  safetyControls.account(identity?.id ?? null);
  moderationControls.account(identity?.id ?? null);
  followingDirty = false;
  followingControls.account(identity?.id ?? null);
  profiles.close();
  if (author && author.identityId !== identity?.id) closeSurface();
  author = identity ? { handle: identity.handle, identityId: identity.id } : null;
  authorHandleInput.value = identity?.handle ?? "";
  composerView.render(drafts.current, identity?.handle ?? null);
  setAuthorStatus(identity ? `Signed in as ${identity.handle}` : "Not signed in", "ready");
  if (draftOwnerChanged) {
    renderComposerDraft(!!drafts.current && drafts.isOpen);
  }
  const label = profileDropdown?.querySelector('[data-menu-action="profile"] span');
  if (label) label.textContent = identity ? "My public profile" : "Sign in";
  const accountEntry = profileDropdown?.querySelector<HTMLButtonElement>('[data-menu-action="account"]');
  if (accountEntry) accountEntry.hidden = !identity;
  const avatar = toggleProfileButton?.querySelector("span");
  if (avatar) avatar.textContent = identity ? Array.from(identity.handle)[0]!.toLocaleUpperCase() : "?";
  toggleProfileButton?.setAttribute("aria-label", identity ? `Account menu for ${identity.handle}` : "Account menu");
  const signOut = profileDropdown?.querySelector<HTMLButtonElement>('[data-menu-action="sign-out"]');
  if (signOut) signOut.hidden = !identity;
  if (changed) {
    objectVisits.clear();
    quotes.clear();
    conversations.clear();
    void reactions.select(null);
    const feedStarted = loadSequence > 0;
    ++loadSequence;
    followingFeed.clear();
    profileReturn = null;
    cards = [];
    currentIndex = 0;
    deck.replaceChildren();
    seenThisSession.clear();
    if (feedStarted) accountFeedRefresh = loadFeed(searchInput.value);
  }
}

async function publishMediaWithAuthor(
  publisher: BabbleFrontendClient,
  identityId: string,
  text: string,
  media: readonly File[],
  target?: DraftTarget,
): Promise<Awaited<ReturnType<BabbleFrontendClient["publishMedia"]>>> {
  const invalid = composerAlbumError(media);
  if (invalid) throw new Error(invalid);
  const first = media[0];
  if (!first) throw new Error("Choose media before publishing");
  const resources: Awaited<ReturnType<BabbleFrontendClient["putMediaBlob"]>>[] = [];
  const hashes = new Set<string>();
  // Upload sequentially to bound memory; publish only when the whole album is ready.
  for (const file of media) {
    const bytes = new Uint8Array(await file.arrayBuffer());
    const blob = await publisher.putMediaBlob(file.type, bytes);
    if (hashes.has(blob.integrity)) throw new Error("The same media appears more than once. Remove the duplicate attachment before publishing.");
    hashes.add(blob.integrity);
    resources.push(blob);
  }
  const title = mediaTitle(text, first);
  if (target && target.mode !== "publish") {
    return publisher.socialMedia(target.mode, identityId, target.parent, text, { title, resources });
  }
  const description = mediaDescription(text, title);
  return publisher.publishMedia(identityId, title, description, resources);
}

interface MediaSelection {
  readonly files: readonly File[];
  readonly valid: boolean;
}

function selectedMediaFiles(): MediaSelection {
  const files = drafts.current?.media ?? [];
  const invalid = composerAlbumError(files);
  if (invalid) {
    setAuthorStatus(invalid, "error");
    composeMedia.focus();
    return { files: [], valid: false };
  }
  return { files, valid: true };
}

function mediaTitle(text: string, media: File): string {
  const line = text.split(/\r?\n/, 1)[0]?.trim();
  if (line && line.length > 0) {
    return line;
  }
  const name = media.name.trim();
  return name.length > 0 ? name.replace(/\.[^.]+$/, "") : "Untitled media Object";
}

function mediaDescription(text: string, title: string): string | null {
  const trimmed = text.trim();
  if (trimmed.length === 0 || trimmed === title) {
    return null;
  }
  return trimmed;
}

async function loadFeed(query: string, preserveObjectId?: string, acceptResult: () => boolean = () => true, publishedCard?: FeedCard): Promise<void> {
  closeSurface();
  followingDirty = false;
  profiles.close();
  objectVisits.clear();
  for (const card of cards) quotes.refresh(card.id);
  const sequence = ++loadSequence;
  followingFeed.clear();
  followingToolbar.hidden = activeLens !== "following";
  followingMore.hidden = true;
  followingSignIn.hidden = true;
  followingStatus.textContent = "";
  if (activeLens === "following") {
    void reactions.select(null);
    cards = [];
    currentIndex = 0;
    deck.replaceChildren();
    closeSurface();
    closeJudgments();
    syncSearchUrl(query.trim());
    await followingFeed.start(accounts.current?.identity.id ?? null, query.trim());
    return;
  }
  updateEmptyFeed(false);
  const previousSummary = feedSummary.textContent;
  const trimmed = query.trim();
  setStatus("loading", "Connecting");
  delete error.dataset.state;
  error.hidden = true;
  empty.hidden = true;
  feedSummary.textContent = trimmed
    ? `Searching ${lensName(activeLens)} Lens for "${trimmed}"`
    : `Loading ${lensName(activeLens)} Lens candidates`;
  syncSearchUrl(trimmed);
  try {
    await ensureFeedSafety();
    if (sequence !== loadSequence) return;
    const result = await client.loadFeed(trimmed, activeLens, localModel(trimmed, activeLens));
    if (sequence !== loadSequence) {
      return;
    }
    if (!acceptResult()) {
      feedSummary.textContent = previousSummary;
      setStatus("online", "Online");
      return;
    }
    // A confirmed publication is directly opened, even outside the ranked page.
    cards = publishedCard && !result.cards.some((card) => card.id === publishedCard.id)
      ? [publishedCard, ...result.cards] : result.cards;
    localPersonalization = result.personalization;
    feedDiversity = result.diversity;
    currentIndex = Math.max(0, cards.findIndex((card) => card.id === preserveObjectId));
    closeSurface();
    catalogLabel.textContent = `${result.catalogMethods} methods`;
    sourceLabel.textContent = result.source;
    localLabel.textContent = personalizationLabel(localPersonalization);
    diversityLabel.textContent = diversityLabelText(feedDiversity);
    lensLabel.textContent = lensName(activeLens);
    syncSettingsStatus();
    render();
    updateEmptyFeed(result.personalization.filtered > 0 && cards.length === 0);
    setStatus("online", "Online");
  } catch (cause) {
    if (sequence !== loadSequence) {
      return;
    }
    if (!acceptResult()) {
      feedSummary.textContent = previousSummary;
      setStatus("online", "Online");
      return;
    }
    cards = [];
    localPersonalization = {
      boundary: "none",
      filtered: 0,
      modelRevision: null,
    };
    feedDiversity = {
      active: false,
      filtered: 0,
      floors: [],
      maxSourceShare: null,
    };
    localLabel.textContent = personalizationLabel(localPersonalization);
    diversityLabel.textContent = diversityLabelText(feedDiversity);
    syncSettingsStatus();
    render();
    delete error.dataset.state;
    empty.hidden = true;
    showError(feedErrorMessage(cause, client.apiUrl));
    setStatus("error", "Offline");
  }
}

function renderFollowingFeed(view: FollowingView<FeedCard>): void {
  if (activeLens !== "following" || view.phase === "idle" || profileReturn || objectVisits.active || surfacePlacement?.returnFeed) return;
  const preferences = loadLocalPreferences();
  const currentId = cards[currentIndex]?.id;
  cards = filterFollowingCards(view.items, preferences, safetyHiddenAuthors());
  currentIndex = Math.max(0, cards.findIndex((card) => card.id === currentId));
  localPersonalization = { boundary: "local_only", filtered: view.items.length - cards.length, modelRevision: null };
  feedDiversity = { active: false, filtered: 0, floors: [], maxSourceShare: null };
  sourceLabel.textContent = "following";
  catalogLabel.textContent = "Chronological";
  localLabel.textContent = "Local filters only";
  diversityLabel.textContent = "Chronological";
  lensLabel.textContent = "Following";
  error.hidden = true;
  render();
  empty.hidden = view.phase !== "ready" || cards.length !== 0;
  if (!empty.hidden) {
    updateEmptyFeed(view.items.length > 0);
  }
  followingStatus.dataset.state = view.phase;
  followingStatus.textContent = view.message || (view.phase === "ready" ? "Newest first" : "");
  const focusMore = document.activeElement === followingMore;
  followingMore.hidden = view.phase === "guest" || (view.phase === "ready" && view.next === null);
  followingMore.disabled = view.phase === "loading";
  followingMore.textContent = view.phase === "loading" ? "Loading..." : view.restart ? "Refresh" : view.phase === "error" ? "Retry" : "Load more";
  followingSignIn.hidden = view.phase !== "guest";
  if (focusMore && followingMore.hidden) refreshButton.focus({ preventScroll: true });
  feedSummary.textContent = view.phase === "guest" ? "Sign in to see Following" : `Following - ${cards.length} posts, newest first`;
  setStatus(view.phase === "error" ? "error" : view.phase === "loading" ? "loading" : "online",
    view.phase === "error" ? "Following unavailable" : view.phase === "loading" ? "Loading" : "Online");
}

function filterFollowingCards(items: readonly FeedCard[], preferences: StoredLocalPreferences, hiddenAuthors: readonly string[] = []): readonly FeedCard[] {
  const allowed = createPersonalizationFilter({
    hidden_authors: [...preferences.hiddenAuthors, ...hiddenAuthors], hidden_terms: preferences.hiddenTerms, muted_terms: preferences.mutedTerms,
  });
  return items.filter(card => allowed(summarizeDiscoveryObject(card.object)));
}

function updateEmptyFeed(filtered: boolean): void {
  const title = empty.querySelector<HTMLElement>(".empty-title");
  const copy = empty.querySelector<HTMLElement>(".empty-copy");
  const controls = empty.querySelector<HTMLButtonElement>("[data-open-feed-preferences]");
  if (title) title.textContent = filtered ? "No posts match your feed preferences"
    : activeLens === "following" ? "No Following posts" : "No posts found";
  if (copy) copy.textContent = filtered ? "Your feed controls hide the posts in this page."
    : activeLens === "following" ? searchInput.value.trim() ? "No posts from followed authors match this search."
      : "Posts from authors you follow will appear here." : "No posts match this search.";
  if (controls) controls.hidden = !filtered;
}

function safetyHiddenAuthors(): readonly string[] {
  const snapshot = safetyControls.snapshot;
  return snapshot && snapshot.author_id === accounts.current?.identity.id
    ? snapshot.entries.map(entry => entry.state.target_id) : [];
}

function applyFeedSafety(): void {
  if (["public profile", "Shared post", "Object"].includes(sourceLabel.textContent ?? "")) return;
  if (accounts.current && !safetyControls.snapshot) {
    cards = [];
    currentIndex = 0;
    return;
  }
  const selected = cards[currentIndex]?.id;
  cards = cards.filter(card => !safetyControls.hidden(card.author));
  currentIndex = Math.max(0, cards.findIndex(card => card.id === selected));
}

function safetyChanged(snapshot: SafetySnapshot | null): void {
  const owner = accounts.current?.identity.id;
  if (snapshot && snapshot.author_id !== owner) return;
  const previous = safetyRevision;
  syncProfileSafety();
  if (!snapshot) {
    if (owner && previous?.owner === owner) {
      safetyUnavailable = true;
      invalidateUnsafeFeed();
    } else {
      safetyRevision = null;
      safetyUnavailable = false;
    }
    return;
  }
  const recovered = safetyUnavailable;
  safetyUnavailable = false;
  safetyRevision = { owner: snapshot.author_id, revision: snapshot.revision };
  if (recovered || (previous && previous.owner === owner && previous.revision !== snapshot.revision)) {
    conversations.clear();
    quotes.clear();
    refreshLocalFeed();
  }
}

async function ensureFeedSafety(): Promise<void> {
  const owner = accounts.current?.identity.id;
  try {
    // A forced refresh can supersede an in-flight ensure. Follow its replacement
    // instead of treating the cancelled request's null result as an empty policy.
    for (;;) {
      const snapshot = await safetyControls.ensure();
      if (owner !== accounts.current?.identity.id) throw new DOMException("Account changed", "AbortError");
      if (!owner || snapshot?.author_id === owner) return;
    }
  } catch (cause) {
    if (owner && owner === accounts.current?.identity.id) safetyUnavailable = true;
    throw cause;
  }
}

function invalidateUnsafeFeed(): void {
  ++loadSequence;
  closeSurface();
  conversations.clear();
  quotes.clear();
  objectVisits.clear();
  profileReturn = null;
  profileBack.hidden = true;
  followingFeed.clear();
  cards = [];
  currentIndex = 0;
  render();
  empty.hidden = true;
  showError("Could not load your blocked and muted authors. Refresh to try again.");
  setStatus("error", "Feed controls unavailable");
}

function profileTargetChanged(id: string | null): void {
  profileSafetyTarget = id;
  followingControls.profile(id);
  syncProfileSafety();
  void safetyControls.ensure().then(syncProfileSafety).catch(cause => setAuthorStatus(errorMessage(cause), "error"));
}

function syncProfileSafety(): void {
  authorControlsButton.hidden = !profileSafetyTarget || profileSafetyTarget === accounts.current?.identity.id;
  followingControls.restrict(!!profileSafetyTarget && safetyControls.blocked(profileSafetyTarget));
}

async function refreshAccountFeed(): Promise<void> {
  const owner = accounts.current?.identity.id;
  try {
    await safetyControls.refresh();
    if (owner === accounts.current?.identity.id) await loadFeed(searchInput.value, cards[currentIndex]?.id);
  } catch (cause) { setAuthorStatus(errorMessage(cause), "error"); }
}

function refreshLocalFeed(): void {
  closeSurface();
  closeJudgments();
  profiles.close();
  objectVisits.clear();
  profileReturn = null;
  profileBack.hidden = true;
  const currentId = cards[currentIndex]?.id;
  cards = filterFollowingCards(cards, loadLocalPreferences());
  currentIndex = Math.max(0, cards.findIndex(card => card.id === currentId));
  render();
  void loadFeed(searchInput.value, cards[currentIndex]?.id);
}

async function openReportedObject(objectId: string, opener: HTMLElement | null): Promise<void> {
  const session = accounts.current;
  const sequence = ++loadSequence;
  setAuthorStatus("Opening reported post...", "busy");
  try {
    const card = await client.publicObject(objectId);
    if (sequence !== loadSequence || accounts.current !== session) return;
    objectVisits.open(card, opener, "Object");
    setAuthorStatus("Reported post opened.", "ready");
  } catch (cause) {
    if (sequence === loadSequence && accounts.current === session) {
      setAuthorStatus(`Could not open reported post: ${errorMessage(cause)}`, "error");
    }
  }
}

function refreshModeratedFeed(record: Pick<ModerationCase, "object_id" | "decisions">): void {
  if (!record.decisions.length) return;
  ++loadSequence;
  closeSurface();
  closeJudgments();
  profiles.close();
  conversations.clear();
  quotes.clear();
  objectVisits.clear();
  profileReturn = null;
  profileBack.hidden = true;
  followingFeed.clear();
  cards = [];
  currentIndex = 0;
  render();
  // The node composes all cases; one reversal cannot locally declare a post clear.
  void loadFeed(searchInput.value, record.object_id);
}

function hideFeedAuthor(objectId: string): void {
  const card = cards.find(candidate => candidate.id === objectId);
  if (!card) return;
  try {
    const undo = feedPreferences.hideAuthor(card.author);
    setAuthorStatus("Author hidden from feed.", "ready");
    if (!undo) return;
    const button = document.createElement("button");
    button.type = "button";
    button.dataset.undoHideAuthor = "";
    button.append(createElement(Undo2, { "aria-hidden": "true" }), "Undo");
    button.addEventListener("click", () => {
      try { undo(); setAuthorStatus("Author restored to your feed.", "ready"); }
      catch (cause) { setAuthorStatus(errorMessage(cause), "error"); }
    });
    document.querySelector("[data-action-status]")?.append(button);
    button.focus({ preventScroll: true });
  } catch (cause) { setAuthorStatus(errorMessage(cause), "error"); }
}

function move(direction: number): void {
  if (cards.length === 0) {
    return;
  }
  const restoreFocus = document.activeElement instanceof Element
    && document.activeElement.getAttribute("class") === "post-card";
  closeSurface();
  currentIndex = (currentIndex + direction + cards.length) % cards.length;
  render();
  if (restoreFocus) {
    deck.querySelector<HTMLElement>('.post-card[data-offset="0"]')?.focus({ preventScroll: true });
  }
}

function hasSurface(card: FeedCard): boolean {
  return card.surfaces.length > 0;
}

function render(): void {
  applyFeedSafety();
  renderDeck(deck, cards, currentIndex, conversations, quotes);
  const reactionContext = deck.querySelector<HTMLElement>('.post-card[data-offset="0"]:not([data-exiting]) .post-context');
  void reactions.select(cards[currentIndex]?.id ?? null, reactionContext ?? undefined);
  empty.hidden = cards.length !== 0;
  const hasCards = cards.length > 0;
  prevButton.disabled = !hasCards;
  nextButton.disabled = !hasCards;
  countLabel.textContent = String(cards.length);
  positionLabel.textContent = hasCards ? `${currentIndex + 1}/${cards.length}` : "-";
  syncSettingsStatus();
  const current = cards[currentIndex];
  if (current) {
    conversations.activate(current.id);
    quotes.activate(current.id);
    recordSeenObject(current.id);
  }
  feedSummary.textContent = current
    ? activeLens === "following" && !profileReturn && !objectVisits.active ? `${current.title} - newest first`
      : `${current.title} - ${current.lineage.length} lineage links, ${current.relations.length} graph relation groups, ${personalizationSummary(localPersonalization)}, ${diversitySummary(feedDiversity)}`
    : "No Objects loaded";
}

async function openJudgments(objectId: string): Promise<void> {
  const sequence = ++judgmentRequestSequence;
  activeJudgmentObjectId = objectId;
  judgmentSubmit.disabled = false;
  const card = cards.find((candidate) => candidate.id === objectId);
  setAnimatedVisibility(judgmentPanel, true);
  judgmentTitle.textContent = card ? `Judgments for ${card.title}` : `Judgments for ${compactId(objectId)}`;
  setJudgmentStatus(`Loading persisted Judgments for ${compactId(objectId)}`, "busy");
  judgmentList.replaceChildren(judgmentMessage("Loading Object Judgments"));
  syncJudgmentParameterInputs();
  try {
    const result = await client.objectJudgments(objectId);
    if (sequence !== judgmentRequestSequence || activeJudgmentObjectId !== objectId) {
      return;
    }
    renderJudgments(result.judgments);
    setJudgmentStatus(`${result.judgments.length} persisted Judgments`, "ready");
  } catch (cause) {
    if (sequence !== judgmentRequestSequence || activeJudgmentObjectId !== objectId) {
      return;
    }
    setJudgmentStatus(`Judgment load failed: ${errorMessage(cause)}`, "error");
    judgmentList.replaceChildren(judgmentMessage(errorMessage(cause)));
  }
}

async function evaluateActiveJudgment(): Promise<void> {
  if (!accounts.current) { accountPanel.open(); return; }
  if (!activeJudgmentObjectId) {
    return;
  }
  const definition = selectedJudgmentDefinition();
  if (!definition) {
    setJudgmentStatus("Choose a supported Judgment definition", "error");
    return;
  }
  const parameters = judgmentParameters(definition);
  const objectId = activeJudgmentObjectId;
  const sequence = ++judgmentRequestSequence;
  judgmentSubmit.disabled = true;
  setJudgmentStatus(`Evaluating ${definitionLabel(definition)}`, "busy");
  try {
    await client.evaluateObject(objectId, definition, parameters);
    if (sequence === judgmentRequestSequence && activeJudgmentObjectId === objectId) {
      await openJudgments(objectId);
    }
  } catch (cause) {
    if (sequence === judgmentRequestSequence && activeJudgmentObjectId === objectId) {
      setJudgmentStatus(`Evaluation failed: ${errorMessage(cause)}`, "error");
    }
  } finally {
    if (sequence === judgmentRequestSequence) judgmentSubmit.disabled = false;
  }
}

function closeJudgments(): void {
  judgmentRequestSequence += 1;
  activeJudgmentObjectId = null;
  judgmentSubmit.disabled = false;
  setAnimatedVisibility(judgmentPanel, false);
  judgmentList.replaceChildren();
  setJudgmentStatus("Idle", "ready");
}

async function openSurface(objectId: string): Promise<void> {
  const session = accounts.current;
  if (!session) {
    pendingSurfaceObjectId = objectId;
    accountPanel.open();
    return;
  }
  closeSurface();
  const sequence = ++surfaceOpenSequence;
  const authorized = () => accounts.current?.token === session.token;
  const source = new BabbleFrontendClient(accounts.origin.href, async (input, init) => {
    if (!authorized()) throw new Error("The signed-in account changed. Reopen this Object.");
    return accounts.authenticatedFetch(input, init);
  });
  try {
    let returnFeed: NonNullable<typeof surfacePlacement>["returnFeed"];
    const originalOpener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    let index = cards.findIndex((card) => card.id === objectId);
    if (index < 0) {
      const card = await source.publicObject(objectId);
      if (sequence !== surfaceOpenSequence || !authorized()) return;
      const previousColumn = deck.querySelector<HTMLElement>('.post-card[data-offset="0"]:not([data-exiting])');
      returnFeed = { cards, index: currentIndex, source: sourceLabel.textContent,
        reading: previousColumn ? captureReading(previousColumn) : null };
      cards = [card];
      sourceLabel.textContent = "Object";
      index = 0;
    }
    if (sequence !== surfaceOpenSequence || !authorized()) return;
    currentIndex = index;
    render();
    const column = required(deck.querySelector<HTMLElement>('.post-card[data-offset="0"]:not([data-exiting])'), "Surface Object column");
    const primary = required(column.querySelector<HTMLElement>(".post-primary"), "Surface Object card");
    const opener = returnFeed ? originalOpener : document.activeElement instanceof HTMLElement && column.contains(document.activeElement)
      ? document.activeElement : column.querySelector<HTMLElement>('[data-action="surface"]');
    surfacePlacement = { column, primary, opener, reading: captureReading(column), objectId, ...(returnFeed ? { returnFeed } : {}) };
    primary.dataset.surfaceOpen = "true";
    deck.dispatchEvent(new Event("surfacechange"));
    primary.append(surfacePanel);
    surfacePanel.hidden = false;
    column.scrollTop = 0;
    closeSurfaceButton.focus({ preventScroll: true });
    let consentBusy = false;
    const acquireConsent = () => {
      if (consentBusy) return null;
      consentBusy = true;
      return () => { consentBusy = false; };
    };
    const actions = new HostActions({
      target: surfacePanel,
      label: cards[index]?.title ?? "This Object",
      identity: session.identity, api: source.browserInvocationApi(), acquireConsent,
      authorized: () => authorized() && sequence === surfaceOpenSequence
        && surfaces.phase === "active" && surfaces.objectId === objectId && surfaces.checkLease(),
    });
    surfaceActions = actions;
    const invocations = new SurfaceInvocations({
      target: surfacePanel, title: cards[index]?.title ?? "This Object", identity: session.identity,
      acquireConsent,
      api: source.invocationApi(), authorized: () => authorized() && sequence === surfaceOpenSequence
        && surfaces.phase === "active" && surfaces.objectId === objectId && surfaces.checkLease(),
    });
    surfaceInvocations = invocations;
    const dispatch = invocations.wrap(actions.wrap(source.bridgeDispatch()));
    await surfaces.open({
      objectId,
      container: surfaceHost,
      currentIdentityId: session.identity.id,
      source,
      authorized: () => authorized() && sequence === surfaceOpenSequence,
      dispatch: async (request, context) => {
        setBridgeStatus(`Bridge request ${request.id}`);
        const response = await dispatch(request, context);
        if (sequence === surfaceOpenSequence && authorized()) setBridgeStatus(`Bridge response ${response.id}`);
        return response;
      },
    });
  } catch (cause) {
    if (sequence === surfaceOpenSequence && authorized()) showError(errorMessage(cause));
  }
}

function closeSurface(restoreFocus = false): void {
  surfaceInvocations?.dispose();
  surfaceInvocations = null;
  surfaceActions?.dispose();
  surfaceActions = null;
  deck.dispatchEvent(new Event("surfacechange"));
  ++surfaceOpenSequence;
  surfaceRestoreFocus = restoreFocus;
  void surfaces.close();
  surfaceRestoreFocus = false;
}

function releaseSurfacePlacement(): void {
  surfaceInvocations?.dispose();
  surfaceInvocations = null;
  surfaceActions?.dispose();
  surfaceActions = null;
  const placement = surfacePlacement;
  surfacePlacement = null;
  surfacePanel.hidden = true;
  surfaceHome.after(surfacePanel);
  delete placement?.primary.dataset.surfaceOpen;
  deck.removeAttribute("data-surface-expanded");
  expandSurfaceButton.setAttribute("aria-pressed", "false");
  expandSurfaceButton.ariaLabel = "Expand Object";
  expandSurfaceButton.title = "Expand Object";
  expandSurfaceButton.replaceChildren(createElement(Maximize2, { "aria-hidden": "true" }));
  surfacePanel.querySelector("details")?.removeAttribute("open");
  surfaceHost.replaceChildren();
  surfaceMeta.replaceChildren();
  surfaceHost.removeAttribute("data-state");
  activeSurfaceLabel = "Prepared runtime";
  setBridgeStatus("Bridge idle");
  if (placement?.returnFeed) {
    cards = placement.returnFeed.cards;
    currentIndex = placement.returnFeed.index;
    sourceLabel.textContent = placement.returnFeed.source;
    render();
  }
  const column = placement?.returnFeed
    ? deck.querySelector<HTMLElement>('.post-card[data-offset="0"]:not([data-exiting])')
    : placement?.column;
  const reading = placement?.returnFeed ? placement.returnFeed.reading : placement?.reading;
  if (column?.isConnected) {
    if (reading) restoreReading(column, reading);
    if (surfaceRestoreFocus) (placement?.opener?.isConnected ? placement.opener : column).focus({ preventScroll: true });
  }
}

function renderSurfaceState(state: SurfaceState): void {
  if (state.phase === "idle") { releaseSurfacePlacement(); return; }
  if (state.objectId !== surfacePlacement?.objectId) return;
  surfaceHost.dataset.state = state.phase;
  surfacePanel.setAttribute("aria-busy", String(state.phase === "preparing" || state.phase === "mounting"));
  retrySurfaceButton.hidden = state.phase !== "error" && state.phase !== "blocked";
  if (state.plan) {
    activeSurfaceLabel = `${state.plan.surface.role} Surface`;
    renderSurfacePlan(state.plan);
  }
  if (state.session) surfaceMeta.prepend(metaPill("Session", state.session.id));
  if (state.phase === "preparing") {
    activeSurfaceLabel = "Preparing Surface";
    surfaceMeta.replaceChildren();
    surfaceHost.replaceChildren(surfaceMessage("Preparing Object"));
    setBridgeStatus("Preparing");
  } else if (state.phase === "mounting") {
    surfaceHost.replaceChildren();
    setBridgeStatus("Starting");
  } else if (state.phase === "error" || state.phase === "blocked") {
    surfaceInvocations?.dispose();
    surfaceInvocations = null;
    surfaceActions?.dispose();
    surfaceActions = null;
    surfaceHost.replaceChildren(surfaceMessage(state.message ?? "This Surface could not start."));
    setBridgeStatus(state.phase === "blocked" ? "Access required" : "Could not start");
  } else {
    setBridgeStatus("Ready");
  }
}

function setStatus(state: "loading" | "online" | "error", label: string): void {
  status.dataset.state = state;
  statusText.textContent = label;
}

function showError(message: string): void {
  error.textContent = message;
  setAnimatedVisibility(error, true);
}

function setAnimatedVisibility(element: HTMLElement, visible: boolean): void {
  const previous = panelAnimations.get(element);
  if (previous?.frame != null) {
    cancelAnimationFrame(previous.frame);
  }
  if (previous?.timer != null) {
    window.clearTimeout(previous.timer);
  }
  panelAnimations.delete(element);
  if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
    element.hidden = !visible;
    if (visible) {
      element.dataset.state = "open";
    } else {
      delete element.dataset.state;
    }
    return;
  }
  const animation: { frame: number | null; timer: number | null } = { frame: null, timer: null };
  panelAnimations.set(element, animation);
  if (visible) {
    element.hidden = false;
    animation.frame = requestAnimationFrame(() => {
      element.dataset.state = "open";
      panelAnimations.delete(element);
    });
    return;
  }
  delete element.dataset.state;
  animation.timer = window.setTimeout(() => {
    element.hidden = true;
    panelAnimations.delete(element);
  }, PANEL_ANIMATION_MS);
}

function isHidden(element: HTMLElement): boolean {
  return element.hidden === true || element.getAttribute("hidden") !== null
    || panelAnimations.get(element)?.timer != null;
}

function setBridgeStatus(message: string): void {
  surfaceBridgeStatus.textContent = message;
  surfaceTitle.textContent = activeSurfaceLabel;
}

function setAuthorStatus(message: string, state: "busy" | "error" | "ready"): void {
  authorStatus.textContent = message;
  authorStatus.dataset.state = state;
  const feedback = document.querySelector<HTMLElement>("[data-action-status]");
  if (feedback) {
    feedback.textContent = message;
    feedback.dataset.state = state;
  }
}

function setJudgmentStatus(message: string, state: "busy" | "error" | "ready"): void {
  judgmentStatus.textContent = message;
  judgmentStatus.dataset.state = state;
}

function errorMessage(cause: unknown): string {
  return cause instanceof Error ? cause.message : "Babble frontend encountered an unknown failure";
}

function feedErrorMessage(cause: unknown, apiUrl: URL): string {
  const message = errorMessage(cause);
  if (message === "Failed to fetch") {
    return `Babble API is unavailable at ${apiUrl.origin}`;
  }
  return message;
}

async function copyObjectId(objectId: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(objectId);
    feedSummary.textContent = `Copied ${objectId}`;
  } catch (cause) {
    showError(`Could not copy Object ID: ${errorMessage(cause)}`);
  }
}

function syncSearchUrl(query: string): void {
  const next = new URL(window.location.href);
  if (query.length === 0) {
    next.searchParams.delete("q");
  } else {
    next.searchParams.set("q", query);
  }
  if (activeLens === "balanced") {
    next.searchParams.delete("lens");
  } else {
    next.searchParams.set("lens", activeLens);
  }
  next.searchParams.delete("surface");
  window.history.replaceState(null, "", next);
}

function syncLensButtons(): void {
  lensLabel.textContent = lensName(activeLens);
  for (const button of lensButtons) {
    const selected = button.dataset.lens === activeLens;
    button.setAttribute("aria-pressed", String(selected));
  }
}

function syncSettingsStatus(): void {
  if (settingsSourceLabel) {
    settingsSourceLabel.textContent = sourceLabel.textContent;
  }
  if (settingsLocalLabel) {
    settingsLocalLabel.textContent = localLabel.textContent;
  }
  if (settingsDiversityLabel) {
    settingsDiversityLabel.textContent = diversityLabel.textContent;
  }
  if (settingsCatalogLabel) {
    settingsCatalogLabel.textContent = catalogLabel.textContent;
  }
  if (settingsCountLabel) {
    settingsCountLabel.textContent = countLabel.textContent;
  }
  if (settingsPositionLabel) {
    settingsPositionLabel.textContent = positionLabel.textContent;
  }
}

async function refreshPlatformOverview(): Promise<void> {
  if (!platformStatus) {
    return;
  }
  platformStatus.textContent = "Loading protocol catalogs";
  platformStatus.dataset.state = "busy";
  if (refreshPlatformButton) {
    refreshPlatformButton.disabled = true;
  }
  try {
    const overview = await client.platformOverview();
    renderPlatformOverview(overview);
    platformStatus.textContent = `Updated ${relativeTime(new Date().toISOString())}`;
    platformStatus.dataset.state = "ready";
  } catch (cause) {
    platformStatus.textContent = `Backend overview failed: ${errorMessage(cause)}`;
    platformStatus.dataset.state = "error";
  } finally {
    if (refreshPlatformButton) {
      refreshPlatformButton.disabled = false;
    }
  }
}

function renderPlatformOverview(overview: PlatformOverview): void {
  setOptionalText(platformCapabilities, String(overview.capabilities.length));
  setOptionalText(platformLenses, String(overview.lenses.length));
  setOptionalText(platformJudges, String(overview.providers.length));
  platformCapabilityList?.replaceChildren(...overview.capabilities.slice(0, 6).map(renderCapabilityItem));
}

function renderCapabilityItem(capability: PlatformOverview["capabilities"][number]): HTMLElement {
  const item = document.createElement("p");
  const name = document.createElement("strong");
  name.textContent = capability.id;
  const meta = document.createElement("span");
  meta.textContent = `v${capability.version} · ${capability.permission}`;
  item.append(name, meta);
  return item;
}

function setOptionalText(element: HTMLElement | null, value: string): void {
  if (element) {
    element.textContent = value;
  }
}

function relativeTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) {
    return value;
  }
  const seconds = Math.max(0, Math.round((Date.now() - date.valueOf()) / 1000));
  if (seconds < 90) {
    return "now";
  }
  const minutes = Math.round(seconds / 60);
  if (minutes < 90) {
    return `${minutes}m ago`;
  }
  const hours = Math.round(minutes / 60);
  if (hours < 36) {
    return `${hours}h ago`;
  }
  return `${Math.round(hours / 24)}d ago`;
}

function lensMode(value: string | null): LensMode {
  if (value === "following" || value === "research" || value === "weird") {
    return value;
  }
  return "balanced";
}

function lensName(value: LensMode): string {
  if (value === "following") {
    return "Following";
  }
  if (value === "research") {
    return "Research";
  }
  if (value === "weird") {
    return "Weird";
  }
  return "Balanced";
}

function selectedJudgmentDefinition(): JudgmentDefinition | null {
  const value = judgmentDefinitionInput.value;
  return (judgmentDefinitions as readonly string[]).includes(value) ? (value as JudgmentDefinition) : null;
}

function syncJudgmentParameterInputs(): void {
  const definition = selectedJudgmentDefinition();
  judgmentQueryLabel.hidden = definition !== "babble.judgment.relevance.v1";
  judgmentRelationLabel.hidden = definition !== "babble.judgment.relationship.v1";
}

function judgmentParameters(definition: JudgmentDefinition): Record<string, string> {
  if (definition === "babble.judgment.relevance.v1") {
    const query = judgmentQueryInput.value.trim();
    return query.length === 0 ? {} : { query };
  }
  if (definition === "babble.judgment.relationship.v1") {
    return { relation: judgmentRelationInput.value };
  }
  return {};
}

function renderJudgments(judgments: readonly ObjectJudgment[]): void {
  if (judgments.length === 0) {
    judgmentList.replaceChildren(judgmentMessage("No persisted Judgments for this Object yet"));
    return;
  }
  judgmentList.replaceChildren(...judgments.map(renderJudgment));
}

function renderJudgment(judgment: ObjectJudgment): HTMLElement {
  const article = document.createElement("article");
  article.className = "judgment-card";
  article.dataset.judgmentId = judgment.id;
  article.dataset.definition = judgment.definition;

  const header = document.createElement("header");
  const title = document.createElement("h3");
  title.textContent = definitionLabel(judgment.definition);
  const meta = document.createElement("p");
  meta.textContent = `${judgment.provider.provider}/${judgment.provider.model}@${judgment.provider.version} - ${formatTimestamp(judgment.created_at)}`;
  header.append(title, meta);

  const metrics = document.createElement("div");
  metrics.className = "judgment-metrics";
  metrics.append(
    judgmentMetric("Confidence", judgmentConfidence(judgment)),
    judgmentMetric("Input", compactId(judgment.input_hash)),
    judgmentMetric("Judgment", compactId(judgment.id)),
  );

  const output = document.createElement("pre");
  output.textContent = JSON.stringify(judgment.output, null, 2);
  const inspection = document.createElement("details");
  const inspectionTitle = document.createElement("summary");
  inspectionTitle.textContent = "Evaluation data";
  inspection.append(inspectionTitle, output);

  const copy = document.createElement("button");
  copy.type = "button";
  copy.textContent = "Copy Judgment ID";
  copy.addEventListener("click", () => void copyJudgmentId(judgment.id));

  article.append(header, metrics);
  const agreement = agreementSummary(judgment.output);
  if (agreement) article.append(agreement);
  article.append(inspection, judgmentInputView(client, judgment.id), copy);
  return article;
}

function judgmentConfidence(judgment: ObjectJudgment): string {
  const output = judgment.output;
  const status = output && typeof output === "object" && !Array.isArray(output)
    ? output.confidence_status : null;
  if (status === "uncalibrated" || judgment.provider.provider === "babble-constant") return "Uncalibrated";
  if (status === "legacy_heuristic" || judgment.provider.provider === "babble-local") return "Heuristic";
  return percent(judgment.confidence);
}

function judgmentMetric(label: string, value: string): HTMLElement {
  const node = document.createElement("div");
  node.className = "judgment-metric";
  const name = document.createElement("span");
  name.textContent = label;
  const strong = document.createElement("strong");
  strong.textContent = value;
  node.append(name, strong);
  return node;
}

function judgmentMessage(message: string): HTMLElement {
  const node = document.createElement("div");
  node.className = "judgment-message";
  node.textContent = message;
  return node;
}

async function copyJudgmentId(judgmentId: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(judgmentId);
    setJudgmentStatus(`Copied ${compactId(judgmentId)}`, "ready");
  } catch (cause) {
    setJudgmentStatus(`Could not copy Judgment ID: ${errorMessage(cause)}`, "error");
  }
}

function definitionLabel(definition: string): string {
  return definition.replace(/^babble\.judgment\./, "").replace(/\.v\d+$/, "").replaceAll("_", " ");
}

function formatTimestamp(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.valueOf())) {
    return value;
  }
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(date);
}

function percent(value: number): string {
  return `${Math.round(value * 100)}%`;
}

interface StoredAuthor {
  readonly handle: string;
  readonly identityId: string;
}


function sessionStorageOrNull(): Storage | null {
  try {
    return window.sessionStorage;
  } catch {
    return null;
  }
}

function localStorageOrNull(): Storage | null {
  try { return window.localStorage; }
  catch { return null; }
}

function localModel(query: string, lens: LensMode): LocalUserModelInput {
  const preferences = loadLocalPreferences();
  return {
    model_revision: "browser-local-v1",
    interests: [...preferences.interests, ...queryInterests(query)],
    expertise: preferences.expertise,
    muted_terms: preferences.mutedTerms,
    hidden_terms: preferences.hiddenTerms,
    hidden_authors: [...preferences.hiddenAuthors, ...safetyHiddenAuthors()],
    creator_affinity: preferences.creatorAffinity,
    seen_objects: loadSeenObjects(),
    novelty_tolerance: preferences.noveltyTolerance ?? lensDefaults(lens).noveltyTolerance,
    exploration_preference: preferences.explorationPreference ?? lensDefaults(lens).explorationPreference,
    evidence_preference: preferences.evidencePreference ?? lensDefaults(lens).evidencePreference,
    contradiction_tolerance: preferences.contradictionTolerance ?? lensDefaults(lens).contradictionTolerance,
  };
}

function loadLocalPreferences(): StoredLocalPreferences {
  return feedPreferences.read();
}

function queryInterests(query: string): readonly string[] {
  const trimmed = query.trim();
  return trimmed.length === 0 ? [] : [trimmed];
}

function lensDefaults(lens: LensMode): {
  readonly noveltyTolerance: number;
  readonly explorationPreference: number;
  readonly evidencePreference: number;
  readonly contradictionTolerance: number;
} {
  if (lens === "research") {
    return {
      noveltyTolerance: 0.45,
      explorationPreference: 0.35,
      evidencePreference: 0.85,
      contradictionTolerance: 0.65,
    };
  }
  if (lens === "weird") {
    return {
      noveltyTolerance: 0.9,
      explorationPreference: 0.92,
      evidencePreference: 0.35,
      contradictionTolerance: 0.85,
    };
  }
  if (lens === "following") {
    return {
      noveltyTolerance: 0.3,
      explorationPreference: 0.15,
      evidencePreference: 0.45,
      contradictionTolerance: 0.35,
    };
  }
  return {
    noveltyTolerance: 0.55,
    explorationPreference: 0.45,
    evidencePreference: 0.6,
    contradictionTolerance: 0.55,
  };
}

function loadSeenObjects(): Record<string, number> {
  try {
    const key = accounts.localDataKey("seen");
    const value = window.localStorage.getItem(key);
    if (!value) {
      return {};
    }
    const parsed = JSON.parse(value) as unknown;
    if (!countRecord(parsed)) {
      window.localStorage.removeItem(key);
      return {};
    }
    return parsed;
  } catch {
    return {};
  }
}

function recordSeenObject(objectId: string): void {
  if (seenThisSession.has(objectId)) {
    return;
  }
  seenThisSession.add(objectId);
  try {
    const next = loadSeenObjects();
    next[objectId] = Math.min((next[objectId] ?? 0) + 1, 1000);
    window.localStorage.setItem(accounts.localDataKey("seen"), JSON.stringify(next));
  } catch (cause) {
    showError(`Could not persist local feed history: ${errorMessage(cause)}`);
  }
}

function personalizationLabel(personalization: FeedPersonalization): string {
  if (personalization.boundary !== "local_only") {
    return "Off";
  }
  return personalization.filtered > 0 ? `Local ${personalization.filtered} filtered` : "Local";
}

function personalizationSummary(personalization: FeedPersonalization): string {
  if (personalization.boundary !== "local_only") {
    return "public ranking";
  }
  return personalization.filtered > 0
    ? `local personalization filtered ${personalization.filtered}`
    : "local personalization active";
}

function diversityLabelText(diversity: FeedDiversity): string {
  if (diversity.maxSourceShare === null) {
    return "Off";
  }
  if (diversity.filtered > 0) {
    return `${diversity.floors.length} floors`;
  }
  return diversity.active ? "Active" : `${diversity.floors.length} floors`;
}

function diversitySummary(diversity: FeedDiversity): string {
  if (diversity.maxSourceShare === null) {
    return "public source mix";
  }
  const percent = Math.round(diversity.maxSourceShare * 100);
  return diversity.filtered > 0
    ? `source diversity ${percent}% max, ${diversity.filtered} filtered`
    : `source diversity ${percent}% max`;
}

function countRecord(value: unknown): value is Record<string, number> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return false;
  }
  return Object.entries(value).every(
    ([key, count]) => key.length > 0 && typeof count === "number" && Number.isSafeInteger(count) && count > 0,
  );
}

function compactId(value: string): string {
  return value.length <= 18 ? value : `${value.slice(0, 12)}...${value.slice(-6)}`;
}

function renderSurfacePlan(plan: Awaited<ReturnType<BabbleFrontendClient["prepareSurface"]>>): void {
  surfaceMeta.replaceChildren(
    metaPill("Admission", plan.admission),
    metaPill("Target", plan.surface.target),
    metaPill("Lifecycle", plan.lifecycle),
    metaPill("Memory", bytes(plan.budget.memory_bytes)),
    metaPill("Network", `${bytes(plan.budget.network_bytes_per_minute)}/min`),
    metaPill("Realtime", String(plan.budget.realtime_connections)),
    ...plan.capability_decisions.map((decision) => metaPill(decision.request.id, decision.status)),
  );
}

function metaPill(label: string, value: string): HTMLElement {
  const pill = document.createElement("span");
  pill.className = "meta-pill";
  pill.textContent = `${label}: ${value}`;
  return pill;
}

function surfaceMessage(message: string): HTMLElement {
  const node = document.createElement("div");
  node.className = "surface-message";
  node.textContent = message;
  return node;
}

function bytes(value: number): string {
  if (value < 1024) {
    return `${value} B`;
  }
  if (value < 1024 * 1024) {
    return `${Math.round(value / 1024)} KiB`;
  }
  return `${Math.round(value / (1024 * 1024))} MiB`;
}

function required<T extends Element>(node: T | null, label: string): T {
  if (!node) {
    throw new Error(`Babble frontend failed to find required DOM node: ${label}`);
  }
  return node;
}
