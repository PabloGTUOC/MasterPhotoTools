## Phase 6 — Web front end

**Status:** complete, with live Firebase sign-in as the one gate.

The web front end **did not build** at the start of this phase. It does now, and both acceptance
checks are automated rather than asserted.

### What was broken

- `src/main.ts` imported `./components/F1Dates.vue`, **which did not exist**.
- `vue-router` and `lucide-vue-next` were imported by `main.ts` and `App.vue` but were **not in
  `package.json`** and not in the lockfile.
- There was **no `typecheck` script**, so build plan §4.2's mandatory
  `npm --prefix frontend/web run typecheck` could not run at all.
- `LibraryBrowser.vue` was hard-coded mock data with a comment saying so; it never called the API.
- The shared client exposed 2 of the ~10 operations, and no view imported it.

### Delivered

- **Task 1 — Vite + Vue 3.** Rebuilt around the real API. The Vite scaffold's leftovers
  (`HelloWorld.vue`, `hero.png`, the boilerplate README) are gone.

- **Task 2 — one interface, no transport in views.** `frontend/shared` is now a real package:
  `types.ts` mirrors the server's DTOs, each type naming the Rust type it matches so a drift on
  either side is visible; `client.ts` declares `ApiClient` and implements `HttpApiClient`.

  `src/api.ts` is the **only** module that constructs a transport. Phase 7 replaces that one file
  with a Tauri implementation of the same interface and every view is unchanged.

- **Task 3 — Firebase sign-in and transparent refresh.** `auth.ts` wires Google sign-in and exposes
  a `TokenProvider`. The client attaches the ID token to every request, and on a `401` whose code is
  `token_expired` it fetches a fresh token and **retries once** before surfacing anything — §5.3's
  requirement that an expired token never drops the user to a login screen. Any other code is final.

  A build with no Firebase configuration still builds and runs, and says plainly why it cannot sign
  in, rather than failing obscurely.

- **Task 4 — a view per tool, with the dry-run gate.** Dashboard, Library (F9), Dates (F1/F2),
  Rename (F3), Split (F4), Contact sheet (F5), Transform (F6), Border (F7), TIFF to JPEG (F8).

  **The apply button is disabled until a preview has been reviewed** — enforced in `ToolPage`, so no
  view can forget it. Rename shows the real plan table with the skipped list and why; Dates runs the
  server's `dry_run` and reports what would change; the image tools show an explicit statement of
  how many inputs will be written where.

- **Task 5 — job progress with cancel.** `JobProgress` follows the SSE stream to its terminal event.
  `EventSource` cannot carry an `Authorization` header, so the client reads the stream from a
  `fetch` body and parses SSE frames itself — every request carries the ID token, including this one.
  "Stop watching" aborts the stream and says what it does: the job continues on the server.

- **Task 6 — library browser with breadcrumbs.** Each path segment is a separate 44 px target, the
  listing rows are 44 px, and long names wrap rather than widening the page.

### Acceptance

- [x] **`npm run build` clean.** 280 kB, 78 kB gzipped.
- [x] **`npm run typecheck` clean.** `vue-tsc --build --force`, strict, with `noUnusedLocals` and
      `noUnusedParameters`.
- [x] **No view imports `fetch` directly.** `npm run check:transport` scans every `.ts` and `.vue`
      under `src/` for `fetch(`, `XMLHttpRequest`, `EventSource` and `axios`, allowing only
      `src/api.ts` and `src/auth.ts`. It is a build step, not a convention anyone has to remember.
- [x] **Layout verified at 390 px.** `npm run check:layout` serves the production build and drives
      real Chromium at 390×844, asserting for **all nine routes** that the page does not scroll
      sideways, that nothing outside a deliberately scrollable strip extends past the viewport, and
      that every interactive control is at least 40 px tall. Screenshots are written to
      `layout-proof/` and uploaded as a CI artefact.

All four now run in CI, in a new `frontend` job.

### Measurements

Production bundle 280.21 kB raw, 78.30 kB gzipped, including the Firebase auth SDK. 72 modules.

The layout check found three real problems on its first run, which are fixed:

| Finding | Verdict |
|---|---|
| The `PhotoTools` brand link was a 24 px touch target | **Real.** Fixed with an explicit 44 px minimum. |
| Tab-strip links reported past the viewport | **Check bug.** They sit inside a deliberately scrollable strip; the check now walks ancestors for `overflow-x`. |
| Checkbox and radio inputs reported at 13 px | **Check bug.** The tappable thing is the wrapping label, which is 40 px; the check now measures that. |

Both check corrections make it more accurate, not more lenient — the page-level "does not scroll
sideways" assertion was passing throughout and is unchanged.

### Gates

- **A real Firebase project.** Sign-in is written but cannot be exercised without
  `VITE_FIREBASE_API_KEY`, `VITE_FIREBASE_AUTH_DOMAIN`, `VITE_FIREBASE_PROJECT_ID` and
  `VITE_FIREBASE_APP_ID`. This is the gate build plan §9 lists for this phase.
- **A human on a real phone.** The check proves geometry, not feel. Whether the tool views are
  actually pleasant to drive one-handed needs a person.

### Deviations

1. **Three npm dependencies added:** `vue-router` (routing), `firebase` (specification §5.2 names
   Firebase Authentication) and `playwright` as a dev dependency for the 390 px check. Specification
   §2.6 names only "Vue 3 + Vite" for the front end, so these are recorded here per G8.
   `lucide-vue-next` was imported by the old code but never declared; it is **not** reintroduced —
   the icons it provided were decorative.
2. **`/api/tools/*` has no dry run for F4–F8.** Specification §9.2 rule 3 says every destructive
   operation supports a dry run, but §8 exposes `dry_run` only on `dates/fix`. The views gate those
   tools behind an explicit confirmation naming the inputs and destination, which is the strongest
   thing the current API allows. **Closing this properly needs a `dry_run` flag on those five
   endpoints**, which is an API change and so a specification question rather than mine to make.
3. **The layout check installs Chromium in CI.** The runner has no browser, and this environment's
   pre-installed one is a different build from the version Playwright pins, so the script points at
   whichever pre-installed binary exists and falls back to Playwright's own lookup.
4. **`frontend/shared/dist` is no longer committed.** It is build output; CI builds it before the
   web package, which needs it via the `file:../shared` dependency.

### Added to manual-verification.md

One Phase 6 entry: the 390 px check proves geometry and touch-target size, not whether the views are
pleasant to drive one-handed on a real phone, and sign-in cannot be exercised until a Firebase
project exists.

### Notes for the next phase

- **Phase 7 replaces exactly one file.** `src/api.ts` constructs `HttpApiClient`; the Tauri build
  substitutes a client that calls `invoke` and implements the same `ApiClient`. No view changes.
  `check:transport` should be copied into the desktop package to keep that boundary honest.
- **Specification §8's note applies to the desktop build**: HTTP calls to the server are made from
  the Rust side with `reqwest`, never from the webview. That means the Tauri `ApiClient` forwards
  everything through `invoke`, including the job stream, which will need a Tauri event channel
  rather than SSE.
- **`frontend/desktop` still does not exist.** Specification §2.7 requires it and Phase 7 creates it.
- **The dry-run gap in item 2 above is worth raising before Phase 13**, since the ingest UI has the
  same requirement and a harder version of it: publishing *must* be refused without a dry run.
