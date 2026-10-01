# Babel: Object Interaction And Deliberate Return

Research restarted on 2026-09-29 at the user's request. Sources below were
searched and read with Aegis. This replaces the earlier general visual-reference
approach; it does not claim that a visual style proves retention or usability.

Updated direction: each horizontal position is an Object with a connected
vertical conversation, not one card containing every feature. This architecture
is now implemented for text, images, and paginated replies. Executable Surfaces
still launch in the existing expanded host; seamless inline application behavior
remains a separate integration requirement.

## Reality

Babel has signed Objects, relationships, discovery Lenses, and executable
Surfaces. The active frontend renders a horizontal card deck. A text post,
image, and interactive Surface have different reading and input requirements.
People have finite screen space and attention, and a gesture cannot reliably
mean both "change the post" and "operate the application inside this post."

The backend already publishes replies as Objects with signed `ReplyTo` edges
from reply to parent (`backend/crates/node/src/social.rs`). Its incoming-edge
HTTP endpoint supports relation filtering (`backend/crates/api/src/routes.rs`).
That endpoint returns an unpaginated edge list. The new
`babel.social.replies.list.v1` RPC provides bounded hydrated pages of verified
direct replies, ordered by Object timestamp and ID. Its parent-scoped keyset
cursors are live bookmarks, not snapshots: backdated imports require refreshing
from the first page.

Code inspection and Aegis geometry exposed an immediate problem: at 1280x800,
the 720x440 card's text container was only about 16px high for a 24px line.
An empty 112px media preview and permanently visible diagnostics consumed the
space. More padding alone would have made the problem worse.

## Interpretation

The useful product promise is that people can discover something, understand
what it is, and choose how deeply to engage with it. "Premium" is a design
judgment, not a psychological outcome. Our proposed visual expression keeps
Babel's grayscale surfaces, spatial deck, rounded object containers, and smooth
transitions, with stronger content hierarchy and more deliberate spacing.

Self-determination theory gives a relevant starting model: autonomy, competence,
and relatedness. The technology application of that model examines several
levels of experience; satisfying interaction inside an app can coexist with
frustration elsewhere in someone's life. For Babel, evaluate useful connection
and perceived control alongside return visits, rather than assuming time spent
indicates benefit. [Source: Center for Self-Determination Theory](https://selfdeterminationtheory.org/topics/application-technology/)

## Contradictions

| Expectation | Observed Conflict | Design Consequence |
| --- | --- | --- |
| Every backend feature should be exposed | Showing every diagnostic on every card crowds out its content | Preserve access through a clearly labeled inspector and contextual views |
| Bigger padding makes an interface calmer | Fixed-height content can shrink or disappear | Reserve content space first; allow long content to scroll |
| One gesture makes the deck effortless | Sliders, text selection, and interactive objects also consume pointer input | Begin navigation only after directional intent and outside interactive controls |
| More engagement means a better platform | Rewarding sessions can coexist with diminished control | Measure chosen outcomes and ease of stopping separately |
| Attractive screens prove quality | Attractive presentation can mask task failures | Assess task completion and errors separately from visual preference |
| Every object must share one internal layout | An image, long text, and a game need different presentation | Share the navigation frame; choose the primary renderer by object type |
| Thin reply cards should have identical heights | Replies vary in length and may contain media | Compact defaults with natural expansion; never hide text without access |
| A wheel should endlessly cycle comments | Loops conceal endpoints and make location ambiguous | Use ordinary vertical scrolling, stable order, and a visible end |

NN/g describes how visual appeal can improve perceived usability and tolerance
for small problems, while substantial task failures remain damaging. Its
usability examples also show why praise for appearance is insufficient evidence
of success. Our implication is to test whether people can read, reply, launch a
Surface, and return to their place, even when they like the look.
[Source: The Aesthetic-Usability Effect](https://www.nngroup.com/articles/aesthetic-usability-effect/)

## What The Lamp Reveals

The distinctive observation is that a post can be an application, and its
conversation is itself a set of linked Objects. The primary card is one view
into that local graph. Horizontal movement chooses another root Object;
vertical movement follows context belonging to the current root. A larger
rectangle alone cannot express that relationship. The surrounding client must
make entry, permissions, loading, errors, exit, and return intelligible.

The same principle applies to the backend: exposing an RPC catalog is not the
same as providing a complete user workflow. A feature is integrated only when
the user can initiate it, understand its outcome, recover from failure, and see
durable state after returning.

## Better Abstraction

Design around an interaction sequence: discover, understand, act, return.

Spatially, the feed is a sequence of independently scrollable Object columns:

```text
Previous Object  <----  Current Object  ---->  Next Object
                             |
                    Primary media / text / app
                             |
                    Author, caption, actions
                             |
                    Compact reply
                    Compact reply
                    Expanded reply + its thread
                             |
                    More replies / conversation end
```

An image fills its primary card, with containment when cropping would lose
meaning; author and caption can sit in a small connected band below. A text
Object uses a reading surface. An executable Object has a preview or explicit
launch state followed by the live Surface. No type must reserve an empty region
for another type's controls.

Replies are compact repeated rows: author, readable content, time, and relevant
actions. Use spacing and a light connecting treatment to communicate belonging.
Avoid shrinking, rotating, or blurring the active comments to imitate a physical
wheel. The wheel metaphor should describe continuity of movement, not reduced
legibility. Nested conversations open inline or as a thread view without adding
a third navigation axis.

### Directional Contract

The unit of navigation is an Object and its context, not a rectangle containing
every field. These are the constraints for the next design iteration:

| Surface | Primary Experience | Connected Context |
| --- | --- | --- |
| Image | Actual image, contained when cropping would remove meaning | Caption only when supplied; author/actions then replies |
| Text | Readable text with comfortable line length and natural height | Author/actions then replies; no reserved media slot |
| Interactive Object | Explicit entry into the executable Surface, with its own input ownership | Conversation remains available after leaving the Surface |
| Reply | Compact horizontal row, growing for real text or media | Its thread replaces the current reply list, with parent context and Back |

Horizontal navigation must not consume vertical scroll, selections, sliders,
keyboard input, or application gestures. Vertical scrolling must never silently
advance to another post. The active conversation is not curved, shrunk, blurred,
or auto-advanced. Endpoints, retries, and pagination remain explicit.

Minimalism means removing competing information, not removing orientation. A
recognizable parent, a stable Back action, and a visible path to Replies earn
their space. Size the primary card against the remaining viewport so a hint of
connected context survives; do not simply enlarge every container. Reply rows
have generous targets even when their visual treatment is slim.

NN/g's spatial-memory guidance describes location recall as approximate and
dependent on repeated access and stable landmarks. Our inference is that
preserving a reply's position should be paired with recognizable parent content
and labels; position alone is insufficient. This is a navigation hypothesis,
not evidence that Babel's two-axis design improves retention.
[Source: Spatial Memory: Why It Matters for UX Design](https://www.nngroup.com/articles/spatial-memory/)

This follow-up source was searched and read using Aegis on 2026-09-29 local time.
The W3C carousel pattern page was also requested, but returned a verification
interstitial; it is not counted as a successfully reviewed source.

Each Object column retains its scroll anchor and expanded-thread state when
switching posts. Prefer a reply ID plus offset as the anchor so new replies do
not move the reader unexpectedly. Deep links identify the root and optional
reply. A visible Replies action and a glimpse of the next connected row reveal
the vertical path. Search and direct links remain alternatives to sequential
horizontal navigation.

NN/g's mobile carousel research identifies weak discoverability, sequential
access costs, and ambiguous swipes near browser edges. It supports visible
continuation cues, alternative controls, and gutters. This is relevant caution,
not direct evidence that a two-axis social feed succeeds. Babel keeps its
user-requested horizontal deck while testing whether people can find and return
to specific objects without swiping through everything.
[Source: Carousels on Mobile Devices](https://www.nngroup.com/articles/mobile-carousels/)

Input ownership is explicit: vertical wheel or touch movement scrolls the current
conversation; deliberate horizontal movement changes Objects. A running game,
range control, text editor, or embedded page owns input within its interactive
region. An explicit expanded Surface view with a persistent exit control gives
immersive content room without trapping the person. Desktop retains visible
navigation; mobile gestures are shortcuts to the same accessible actions.

| Stage | Visible Information | Deeper Interaction |
| --- | --- | --- |
| Discover | Current Object, author context, Lens, adjacent cards | Search and feed controls |
| Understand | Full readable text or actual media; explicit Surface entry | Object inspector, provenance, ranking explanations |
| Act | Relevant social actions and object-specific controls | Composer, conversation, permission and runtime states |
| Return | Preserved place and clear completion or error feedback | Revisit related objects and chosen authors |

Progressive disclosure supports this hierarchy when common tasks remain easy to
reach and secondary controls have informative labels. Hiding everything behind
ambiguous icons would defeat its purpose. For Babel, technical metadata belongs
in "Inspect Object"; access to the actual Surface belongs with its content.
[Source: Progressive Disclosure](https://www.nngroup.com/articles/progressive-disclosure/)

Spacing should communicate relationships. Keep an author's identity together,
separate it from content, then separate content from actions. NN/g's proximity
guidance also cautions that excessive separation can hide relevant controls and
that responsive rearrangement can break groups. Therefore whitespace is a
hierarchy tool, not a quantity to maximize.
[Source: Proximity Principle in Visual Design](https://www.nngroup.com/articles/gestalt-proximity/)

The Object-column renderer now applies the following choices. These are choices to
evaluate, not scientific constants:

- Desktop column: up to 800px wide; text has 32px internal padding and grows
  naturally. Media occupies a viewport-constrained primary card.
- Mobile card: width constrained by viewport, 20px padding, wrapped actions.
- Content, author/actions, and conversation form one vertically scrollable column;
  text-only posts reserve no image region. Image cards show the image without
  overlaid controls; captions and authors sit below.
- Diagnostics remain available in the inspector, which has its own scroll area.
- Primary card controls use generous hit areas; text retains its full content.
- Swipe motion retains spatial continuity; reduced-motion settings are respected.
- Inactive cards cannot receive keyboard focus; inputs keep their native behavior.

### Follow-Up: Separate The Card From The Place

The user's image-plus-connected-comments proposal removes an unnecessary
assumption: the visual card does not have to be the unit of navigation. The
navigation unit is the root Object and its current conversation state. The
primary card is only its content renderer. This distinction should survive
component boundaries, routing, keyboard focus, and scroll restoration.

The next refinement keeps the existing design system and uses this hierarchy:

1. Primary content: an unobstructed image, readable text, or an explicitly
   entered interactive Surface. Content chooses its presentation; the shell
   must not force all types into a dashboard-shaped layout.
2. Connected context: a quiet author/caption/action band, outside the primary
   card. Keep controls near the content they act on without making another
   large boxed section.
3. Conversation: slim horizontal reply rows with natural height. A short reply
   is compact; a long reply or media Object can expand. Slim appearance must
   not imply tiny targets, clipped text, or a fixed-height input area.
4. Thread context: entering a reply's conversation retains a recognizable
   excerpt of that reply and a Back action. Do not solve branching by adding
   another sideways carousel inside the main sideways carousel.

There should be one primary vertical scroll area per active Object, not a
scrolling comment box trapped inside a separately scrolling card. A running
Surface may require its own scroll/input area; that is an explicit mode with
an accessible exit, not an accidental nested-scroll dependency. Draft replies,
thread selection, and reading position belong to their Object, not the current
deck index. Switching posts must not transfer a draft to another parent.

NN/g's 2016 research on the illusion of completeness warns that large images,
strong horizontal boundaries, and excessive gaps can suggest that no content
continues below. It also documents the need for visible horizontal-navigation
cues. This is historical usability evidence, not a current Babel user study.
Our application is to leave a glimpse of connected context where practical,
retain an explicit Replies action, and keep desktop navigation visible. The
image can fill its card without filling the entire usable viewport.
[Source: The Illusion of Completeness](https://www.nngroup.com/articles/illusion-of-completeness/)

#### Implementation Versus Intent

Code inspection on 2026-09-29 confirms image-first rendering, separate context,
compact replies, and paginated nested threads already exist. It also exposes
specific unfinished parts of this interaction contract:

- `reading.ts` now captures content/reply IDs plus viewport offsets, with a pixel
  fallback when the anchor is unavailable. Card remount and thread restoration
  use this contract; it is in-memory navigation state, not a persistent bookmark.
- `conversations.ts` now retains a recognizable parent excerpt in nested threads.
  Its native disclosure reveals full text/media; Back restores the previous
  reading anchor. User-driven thread changes focus the thread heading without
  an extra focus-induced scroll.
- `global.css` now permits a thin native column scrollbar instead of suppressing
  it. These cues still need discovery testing with actual users.
- `main.ts` and `cards.ts` wrap the feed with modulo indexing. Repetition is
  not fresh content. Before calling this production-ready, choose an explicit
  end-of-results treatment rather than silently implying unlimited new posts.
- Executable Surfaces still use the expanded host. Seamless inline input
  ownership has not been established by this design proposal.

The next falsification test is simple: open an image, discover its conversation,
read a long reply, enter its thread, change posts, and return. Losing the parent,
losing the reading position, misdirecting a draft, or changing posts while
operating a Surface is a failure. No retention benefit is claimed until people
can complete this sequence and report that returning was useful and voluntary.

## What Is Proven

The sources support these general design principles. The clipping defect is
observed in this implementation. The protocol and frontend code support multiple
Object and Surface representations. None of this proves product-market fit,
positive retention, or complete production readiness.

Automated browser checks can establish layout bounds and state transitions.
They cannot establish perceived elegance, comprehension, or improved wellbeing.

## What Is Still Unknown

- Which object types will dominate real use and require the most frequent actions.
- Whether users discover secondary controls at the intended point in a task.
- Whether the swipe deck helps orientation for repeated visits and conversations.
- Whether chosen connections and executable objects produce worthwhile returns.
- Whether density, motion, and card size work well with real user content.
- Whether every represented backend capability has a complete user-facing flow.
- Whether people distinguish "another post" from "more about this post" without
  instruction, and whether embedded applications create directional conflicts.

Code findings to track separately from this visual pass: DOM-only confidence and
vote widgets were removed, but durable reactions remain unimplemented. Account
authentication now has a verified HTTP boundary and browser flow; public profiles,
account recovery, and full workflow coverage remain open in
`production-readiness.md`. Removing misleading controls does not satisfy those
requirements.

## Next Experiments

1. Compare the old crowded card with the content-first card on the same real
   content. Ask participants to read the full post, identify its author, reply,
   and find why it appeared. Record missed actions and incomplete tasks before
   asking about visual preference. Reject the change if core tasks become harder.
2. Give participants a text post, image, and executable object. Ask what each
   permits before interaction, then observe launch, interaction, and return.
   Reject the entry design if they confuse a preview with an already running app.
3. Exercise desktop, narrow mobile, long text, long identifiers, open inspectors,
   form controls, canceled gestures, and reduced motion. Any clipped controls,
   accidental navigation, or loss of content is a failure to fix.
4. In an opt-in longitudinal pilot, pair return frequency with whether a visit
   accomplished the user's chosen purpose and whether stopping felt easy.
   Increased visits without increased usefulness or control fails our hypothesis.
5. Audit features by complete workflow: entry point, backend operation, durable
   result, readback, error recovery, and accessibility. A working RPC alone does
   not pass that audit.
6. Test the two-axis proposal with an image and a long real reply thread. Ask
   participants to find a reply, move to another post, then return. Record lost
   position and accidental post changes. Reject any design requiring instructions
   to recover the parent or repeated swiping to find the remembered reply.
7. Test the same navigation around a playable Surface and a scrollable embedded
   page. Any game input that changes posts, or lack of an accessible exit, fails
   the interaction contract.

These are proposed experiments, not completed user studies. Do not infer causal
retention changes from small usability samples or introduce engagement telemetry
as a side effect of this design pass.

### Engineering Verification Of The Interim Changes

- Astro type checking and production build passed; one pre-existing async hint remains.
- Thirteen interaction-handler regression tests passed.
- The real API/Astro/Aegis suite passed, covering publication, follow, reply,
  share, image upload, and executable Surface hosting.
- Fozzy strict doctor/test checks passed on the scripted scenario. The separate
  `tests/live-stack-host.fozzy.json` executes the real process without asserting
  empty stdout against a test that prints dynamic IDs. Its recorded trace at
  `artifacts/live-stack-layout-host.trace.fozzy` passed strict verification,
  replay, and CI. The scripted checks are not evidence of live browser execution;
  the host run provides that evidence.
- Aegis geometry checks covered 1280x800 desktop and same-origin iframe viewports
  at 390x844 and 320x640. No page-width overflow was observed. Long text remained
  scrollable, and the inspector fit within the narrow card. These iframe checks
  do not substitute for physical-device gesture or screenshot review.
- The installed Aegis API has no screenshot or viewport-resize command. Visual
  polish and real-device touch behavior remain review items; another browser
  automation tool was not substituted.

### Engineering Verification Of Conversation Columns

- The reply backend passed 94 focused Rust tests and 45 SDK tests, covering
  ordering, bounds, cursor validation, duplicate relations, import, and restart.
- Eighteen conversation-state tests cover pagination/retry, stale requests,
  published-tail insertion, nested navigation, and scroll restoration. Thirteen
  interaction-handler tests also pass.
- The updated real API/Astro/Aegis run confirms signed reply publication and
  readback inside the parent column, plus scroll preservation across post changes
  and nested-thread Back navigation.
  `artifacts/live-stack-conversations-nested-host.trace.fozzy` passed strict trace
  verification, replay, and CI. This is host execution evidence, not a claim
  that a live browser is deterministic.
- Astro checking reports zero errors/warnings and one existing async hint;
  the production build passes.
- The full Rust workspace test suite passes. Aegis geometry checks on the new
  columns at 1280x800, 390x844, and 320x640 show no page-width overflow; mobile
  checks used same-origin iframes, not native device emulation.
- This original milestone used pixel offsets. The follow-up below replaces them
  with content/reply-ID anchors and a pixel fallback.

### Engineering Verification Of Reading Anchors

- At this milestone, 52 frontend tests pass, including anchor restoration after
  earlier content grows, replacement nodes, missing anchors, positioned ancestors,
  nested parent context, heading focus, and earlier-reply insertion through the
  actual production conversation controller.
- `tests/conversation-browser.mjs` exercises that controller in Aegis with
  controlled reply data and real DOM layout: inserting an earlier reply preserves
  the visible reply offset within one pixel; nested Back restores that offset;
  the full parent text remains accessible; a 360px column does not overflow.
  This is a narrow-column check, not native mobile viewport/device evidence.
- `tests/reading-check.mjs` runs focused unit tests and this actual browser check
  against a running Astro/Aegis instance. Strict scripted doctor/test passed first.
  `artifacts/conversation-reading-host.trace.fozzy` records host execution and
  passes strict verification, replay, and CI. Browser interaction itself is not
  claimed deterministic. The full live-stack script also includes the new check.
- Three scripted Fozzy property-fuzz runs pass as orchestration checks only.
  Attempting host-backed `fuzz` in the installed engine instead selected a strict
  scripted process path and failed with `proc_unmatched`; it did not execute the
  browser. The successful recorded host `run`, not this fuzz attempt, supplies
  browser evidence. No randomized-browser coverage is claimed.
- Astro checking/build pass. No screenshot or physical touch review was possible
  through the installed Aegis API. Reflow from late-loading media, browser zoom,
  refresh across sessions, and assistive-technology review remain separate tests.

### Rounded Card Restoration

- Restored the deprecated card's cool-gray tonal shading, raised shadow, rounded
  silhouette, and visible neighboring-post depth. The primary frame uses a 28px
  radius; reply rows use 18px and a narrower column, with a further inset for
  nested replies. These are the existing card language, not a new flat theme.
- Text-post authors and actions now belong to the main frame again. Image posts
  keep an uninterrupted image area with an attached author/action band. Surface
  and image cards use restrained sage and blue-gray tints respectively. Protocol
  inspection remains available below the card instead of crowding its content.
- Horizontal swipe mechanics and reduced-motion handling remain intact. Deck
  spacing now leaves neighboring cards visible at desktop and narrow widths.
- All 70 frontend tests pass; Astro check/build pass (one existing async hint).
  `tests/card-style-browser.mjs` verifies the real running app at 1280px and in
  390px/320px same-origin frames: rounded shaded frames, contained author/actions,
  narrower conversations, visible neighboring cards, and no card-width overflow.
  The same host run exercises real-DOM reading anchors and nested Back behavior.
- `artifacts/card-style-responsive-host.trace.fozzy` records the successful host
  run. An initial harness attempt timed out because this Aegis version omits
  primitive Boolean eval results; returning a structured object fixed the test.
  No screenshots or physical-device visual/gesture sign-off are claimed.
- The subsequent complete live-stack check passes with the restored cards,
  account-scoped drafts, and atomic publication. Its verified/replayed/CI-checked
  trace is `artifacts/live-stack-rounded-cards-verified-host.trace.fozzy`.
  The frontend now has 73 passing tests, including a reproduced and repaired
  account-status race during the composer's closing animation.

### Card Hierarchy Polish

- Preserved the 28px primary silhouette, 18px reply corners, tonal shading,
  neighboring-card depth, and existing horizontal swipe transitions. Increased
  the desktop frame to 880px of visible content, with 40px text gutters.
- Silver text cards, sage Surface cards, and blue image cards now pass their
  quieter companion hues to replies. Conversation columns occupy about 85-88%
  of the main frame's width, including intermediate tablet widths. Short replies
  remain compact rows rather than competing full-size cards.
- Reduced header weight and shadow, surfaced the existing Lens controls on wide
  desktops, and replaced the navigation/creation/analytics glyphs with Lucide
  icons. Existing mutations, Surface ownership, and reading restoration are
  unchanged.
- All 201 frontend tests, Astro checking, and production build pass. The real
  API/Astro/Aegis suite checks 1280px plus same-origin frames at 1440, 860, 390,
  and 320px: no horizontal overflow, overlapping header controls, or clipped
  author/action rows. It also verifies compact persisted replies and existing
  publishing, account, Following, and inline Surface workflows.
- The first live run caught insufficient reply insets at 860px; the responsive
  margin was corrected before the successful full rerun. Evidence:
  `artifacts/rounded-card-polish-verified-host.trace.fozzy`, passing strict trace
  verification, replay, and CI. Fozzy doctor (five runs), strict scripted test,
  and eight scripted fuzz runs cover orchestration only. Browser geometry is
  not screenshot review or physical-device visual/gesture sign-off; those remain
  unverified through the installed Aegis interface.

### Rounded Hierarchy Refinement

- The current primary silhouette is 32px, with a desktop frame up to 960px
  wide and 48px reading gutters. Replies retain 18px corners and occupy about
  83-88% of the main frame's width. Mobile keeps 20px content gutters.
- Text, executable Surface, and image cards use restrained rose, sage, and blue
  shading respectively, with lighter matching reply surfaces. The neutral page
  background keeps those cards distinct. Short text uses a stronger reading
  hierarchy; the redundant "Author" label is removed, not the profile action.
- Reply layout responds to its own column width: wide columns place actions
  beside the content; narrow columns stack naturally. Nested threads remain
  inset. Surface launch controls no longer stretch across the entire card.
- All 246 frontend tests and the production build pass. Astro checking reports
  zero errors/warnings and two existing async-conversion hints. Aegis verifies
  live layout at 1280px and same-origin frames at 1440, 860, 390, and 320px,
  including header separation, contained content, and visible neighboring cards.
  Real-DOM tests exercise narrow/wide replies, reading anchors, nested Back, and
  next/previous navigation with inactive cards remaining inert.
- Evidence: `artifacts/card-style-hierarchy-ready.trace.fozzy`, with strict trace
  verification, replay, and CI passing. Five-run deterministic doctor, strict
  scripted test, and eight scripted fuzz runs cover the scenario orchestration,
  not randomized browser behavior. `explore` rejects this non-distributed steps
  scenario and provides no additional coverage. An initial style assertion used
  a detached reply after Back; the test now measures the returned live node.
- This is a scoped presentation check, not whole-platform production sign-off.
  Aegis geometry cannot substitute for screenshots or physical touch review;
  neither is claimed here. Earlier full-stack temporal/search integration
  failures remain separate from this visual pass and are not waived by it.

### Swipe Reading Continuity

The full interaction run exposed a layout regression beyond the static card
checks: moving the shared reaction controls off an inactive card shortened it
and clamped its scroll position. ReactionPanel now reserves the outgoing
context's measured height until the controls return, then restores the original
minimum-height rule. The inactive card does not fetch or gain extra controls.

`frontend/tests/reactions-browser.mjs` exercises the real view/controller in a
scrolled column and asserts both unchanged reading position and reservation
release. Its host trace, `artifacts/rounded-card-reading-dom-host.trace.fozzy`,
passes strict verification, replay, and CI. The failing full-stack trace is
retained as `artifacts/rounded-cards-temporal-search-host.trace.fozzy`.

The complete rerun passes in
`artifacts/rounded-cards-reading-restored-host.trace.fozzy` (strictly verified,
replayed, and CI-checked). It covers the current 32px primary/18px reply style,
desktop and narrow-frame geometry, next/previous reading restoration, nested
conversations, public reactions, publishing, profiles, Following, account
security, and inline Surface lifecycle. The earlier temporal/search failures
are now resolved in this full run rather than waived by the presentation pass.
Physical-device touch and screenshot review remain unverified.

## Nested Card Hierarchy Follow-Up

The rounded swipe-card system remains the baseline: 32px primary corners,
18px first-level replies, rose/sage/blue tonal surfaces, and horizontal post
navigation. Nested reply rows now step down to 14px corners, narrower bilateral
insets, and a lighter surface. Parent context uses the same card hue. Keyboard
focus outlines the rounded primary card instead of the rectangular scrolling
column; reply images also retain rounded corners.

The production conversation controller passes nested-width, radius, overflow,
anchored refresh, and Back-position checks at 320/360/760px column widths.
The live page passes main-card geometry checks at desktop and 320/390/860/1440px
same-origin frames, including next/previous reading-position restoration.
`artifacts/rounded-card-hierarchy-host.fozzy` records the real Aegis checks and
passes strict verification, replay, and CI. The 25-run deterministic scripted
fuzz pass checks orchestration only, not randomized browser gestures.
All 248 frontend unit tests pass. Screenshot and physical touch approval remain
separate, unverified gates.

## Card Polish, September 30

The existing rounded swipe-card language remains intact: 32px primary corners,
18px reply rows, and 14px nested rows. The desktop column is now at most 920px
(880px visible primary frame), with 48px reading gutters. Closer neighboring
cards, gentler blur, and stronger rose/sage/blue tonal separation make the deck
more apparent. Only the active post exposes its conversation and inspector;
inactive columns preserve layout and reading positions. Reply rows use compact
padding without reducing their action targets or truncating their content.

Wide desktop headers show the existing Lens selector. Post utility menus use
Lucide icons with accessible names and hover titles; the underlying actions
remain unchanged. Empty reaction status space no longer enlarges the card.

All 256 frontend tests, Astro checking, and the production build pass. The real
Aegis presentation/reading run passes at desktop and 320/390/860/1440px frames,
plus 320/360/760px conversation columns. Checks include descending corner sizes,
narrower replies, header/control containment, horizontal navigation, inactive
focus exclusion, and anchored nested-thread return. The host trace is
`artifacts/card-style-polish-reading-host.fozzy`. Scripted doctor/test and fuzz
checks test process orchestration, not randomized browser interactions.
Screenshot and physical-device touch review remain unverified.

The complete API/Python/Astro/Aegis run also passes with this card polish and the
document-bound Surface bridge in
`artifacts/live-stack-document-bridge-verified-host.fozzy` (strictly verified,
replayed and CI-checked). This combines the presentation checks with actual
account, publishing, social, lease and embedded-RPC workflows; it does not add
visual or physical-device evidence beyond the DOM/geometry checks above.

## Honest Thesis

Babel's opportunity is a social space where each Object has room to be itself,
with its conversation visibly connected below it. Horizontal discovery and
vertical context give the protocol a spatial expression. Whether that feels
natural must be tested through orientation, successful interaction, and return.
Sustainable return should follow useful experiences and relationships; it remains
an empirical question, not something a palette or animation can prove.
