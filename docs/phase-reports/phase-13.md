## Phase 13 — Ingest UI

**Status:** complete with one gate. All three acceptance criteria are measured in a real browser
rather than asserted. The gate is that the desktop's Ingest screen has never been *rendered* — Tauri
needs macOS — though the components it is built from have been, against four hundred shots.

### Delivered

- **Task 1 — the review grid.** One row per shot: stem, pairing, capture date, megapixels, size, and
  a status chip per check. **Windowed**, for the reason below.

- **Task 2 — filter by failure class, and a bulk action bar.** Pressing a class filters the grid to
  it; each class carries one action and one button.

- **Task 3 — auto-resize on by default**, and it needed no special case. F13's `default_action` for
  `too_many_pixels` *is* `resize`, so the bar seeds every class from what `core` already decided.
  The UI does not have its own opinion about the default, which is what stops the two drifting apart.

- **Task 4 — the mandatory dry-run preview.** Publish is disabled until a dry run for *that session*
  has come back, and changing the session clears it. The server refuses it independently; the button
  is disabled because a disabled button explains itself better than a rejection does, not because
  the server trusts the screen.

- **Task 5 — live progress, and an unmistakable handover.** The job progress component follows every
  long operation, and when the handoff finishes the screen says *"The server has taken over… you can
  close the lid"* and shows the session id. "Nothing is happening here any more" is otherwise
  indistinguishable from "something has gone quiet".

### Acceptance

- [x] `npm run typecheck` and `npm run build` — both applications, and the shared package.
- [x] `npm run check:transport` — both applications. No view reaches the network directly.
- [x] `npm run check:layout` — **10 routes clean at 390×844**, `/publish` among them.
- [x] **A 400-shot session renders and stays responsive.** Measured, see below.
- [x] **Bulk-approving all resizes is one action.** Measured: `resize` arrives preselected, one press
      emits exactly one request covering 360 shots.
- [x] **Publish is unreachable in the UI until a dry run has been reviewed.** Driven against the real
      built application, not a harness.
- [x] **Append to `docs/manual-verification.md`.** Six entries.
- [x] Rust gates re-run after the one `core`-side change: **461 workspace, 400 core (G2)**.

### The measurements

`npm run check:ingest` builds the components on their own, drives them in Chromium, and asserts
numbers rather than shapes:

| | Measured | Budget |
|---|---|---|
| First paint, 400 shots | **599–615 ms** | 700 ms |
| Rows built into the DOM | **29** of 400 | fewer than 400 |
| Filter to one failure class | **8.6–12.6 ms** | 100 ms |
| Scrolling | **15.2–16.7 ms** per frame | 33 ms (two frames at 60 Hz) |
| One press covers | **360** shots | at least 300 |

The dev server is warmed with a one-shot page before the clock starts, because a cold Vite compile is
several hundred milliseconds of tooling that has nothing to do with whether the grid is fast.

### Why the grid is windowed

Four hundred rows would render. The criterion is that the session *stays responsive*, and the
difference shows up on the interaction the bulk bar drives: a filter change on a non-windowed grid
tears down and rebuilds four hundred components, each with three chips. The window builds 29.

The DOM count is asserted, not just the timing — a grid that rendered all four hundred could still
pass a timing budget on a fast machine and then fall over on the Mac. Asserting the structure is
asserting the reason it is fast.

### How the three claims were made real

Each acceptance criterion is a claim about behaviour, and each is driven rather than inspected:

1. **Responsive** — timed, in a browser, at four hundred shots, with the last shot asserted reachable
   after scrolling. A window that quietly gave up past row 200 would pass every timing check.
2. **One action** — the harness records what the component *emits*. The assertion is that one press
   produces exactly one request, that it is a `resize` for `too_many_pixels`, and that the class it
   covers holds 360 shots. Checking that a button exists would prove none of that.
3. **Publish gated** — the real built application is served and driven; the button's disabled state
   and the sentence explaining it are both asserted. A harness could have been written to agree with
   itself.

### Two bugs the screenshots caught

- **The capture date wrapped to two lines**, which put row heights out of step with the spacer that
  gives the scrollbar its length — the windowing arithmetic assumes a fixed row height. Every cell is
  now one line, clipped with an ellipsis.
- **A misconfigured API address produced `Unexpected token '<', "<!doctype "... is not valid JSON`.**
  True about a string, useless about the system. `HttpApiClient` now reads bodies through one helper
  that says *"Asked for the Google Photos connection and got text/html instead of JSON. The address
  configured for the server is probably not the server."* This affects every call, not just this
  phase's.

### Accessibility

The chips carried their verdict in colour alone. Red against teal at chip size is not a distinction
everybody can make, and a review screen whose entire meaning is *which of these failed* cannot put
that meaning in a hue. Each chip now carries a mark — ✓, ✕ or ! — and a screen-reader-only status
word before the rule name. **`check:ingest` asserts that a passing chip and a failing chip do not
carry the same mark**, so the colour cannot quietly become the only carrier again.

Everything else follows the conventions Phase 6 established: 44 px touch targets, 16 px inputs so iOS
does not zoom, `prefers-reduced-motion` honoured, and the 390 px layout check extended to `/publish`.

### Where the two new screens live, and why

Not in `frontend/shared/src/ui/views/`, which is the *rendered-twice* contract.

| View | Where | Why |
|---|---|---|
| `Ingest` | `frontend/desktop/src/views/` | §2.3 puts the card reader on the Mac. The server has no card to read. |
| `Publish` | `frontend/web/src/views/` | The Google refresh token lives on exactly one machine (§2.3). |

`ApiClient` gained only the four ingest operations **both** transports genuinely perform —
`scanCard`, `validateCard`, `remediate`, `deriveRaw`. Reading a card's rows and handing it over are
on `TauriApiClient`; publishing and the Google connector are on `HttpApiClient`.

The alternative — putting everything on `ApiClient` and having each transport throw for the half it
cannot do — was rejected. **A capability that exists in the type system and fails at runtime is worse
than one the type system never offered**: the first is discovered by a person pressing a button, the
second by a compiler. It is the same reasoning that kept `notify` out of Phase 11.

`ShotGrid` and `BulkActions` are in `shared/src/ui/components/` despite being used by the desktop
alone, because they are transport-free — and being transport-free is exactly what let them be
measured on their own. `frontend/shared/src/ui/README.md` records the distinction: `views/` is the
rendered-twice contract, `components/` is a library.

### Deviations

1. **Two application-specific views**, argued above. The shared-views README previously implied every
   view is rendered twice; it now documents the exception and the reason.

2. **A new `check:ingest` script, and a harness under `frontend/web/scripts/grid-harness/`.** The
   repository's frontend checks are Node scripts driving Playwright rather than a unit-test runner
   (`check:transport`, `check:layout`), so this follows that convention rather than introducing
   Vitest. The harness mounts the components with no application around them, which is what makes
   "four hundred shots" a thing that can be measured at all. No new dependency: Playwright and Vite
   are already in the web package.

3. **`hand_off_card` now reports the session id** in its completion message. One line in
   `crates/desktop/src/commands.rs`. Without it the session is unreachable — publishing is addressed
   by session and nothing else surfaces the id. See the seam note below.

4. **`ApiClient.remediate` returns `RemediationPlan | string`.** A dry run answers with a plan and a
   real run with a job id; the server already distinguishes them by status code. Collapsing the two
   into one shape would have meant inventing a wrapper that neither side sends.

5. **`CheckStatus` gained `'pending'`** in the TypeScript types. `core` has had the variant since
   Phase 9 — a RAW-only shot has no pixels to measure until F14 derives a JPEG — and the wire types
   were simply missing it.

### The seam nobody will like

**The session id is copied by hand from the desktop to the web UI.** The desktop shows it after the
handover; the publish screen takes it as typed input. Specification §8 lists no endpoint that
enumerates sessions, so there is nothing to populate a list from, and inventing one is scope this
phase should not take (G11).

It is a UUID typed by a person, which is the sort of thing that works in a demonstration and grates
on the fortieth card. The fix is a `GET /api/ingest/sessions` returning recent sessions with their
state — a small endpoint, but one the specification does not have, so it belongs to whoever decides
to add it rather than to this phase.

### Notes for the next phase

- **The publish screen shows unconfirmed shots and says what to do about them.** Phase 12 noted that
  they needed a human affordance; the plan panel now names them and explains that they are not
  retried because a second attempt would duplicate whichever succeeded. That is as far as a screen
  can take it — the actual check is a person looking in Google Photos.
- **`layout-proof/` now holds `shot-grid-400.png`, `bulk-actions.png` and `publish-gate.png`** beside
  the route screenshots. They are gitignored, so they are rebuilt by `npm run check:ingest`.
- **Nothing in the desktop build is layout-checked.** `check:layout` is the web package's, and a
  390 px phone viewport is not what a Tauri window is. The desktop's screens have been typechecked
  and built but never rendered, which is the gate.
