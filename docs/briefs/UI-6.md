# `UI-6` · The status strip

> **Status:** briefed
> **Plan:** [`docs/rollchrome/app-layout.md`](../rollchrome/app-layout.md), step UI-6. **Branch:** `ui-6`, cut from
> `main` after UI-2 is merged.

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md`: **Ground rules**, **Gates**, **The design language**, and **The front-end boundary**
  (`frontend/shared/src/ui/README.md`).
- `docs/rollchrome/app-layout.md`: the whole document, especially rules **2 and 3** and **How it fits the
  code**.
- `frontend/rollchrome/src/App.vue`: the shell, after UI-1.
- `frontend/rollchrome/src/views/Develop.vue`: the action buttons (`dry-run-button`, `run-button`,
  `gate-explanation`), the `review-section`, the plan error banner and the `job-section` with
  `<JobProgress>`. They sit near the end of the template.
- `frontend/rollchrome/src/views/BulkLut.vue`: the same four things.
- `frontend/shared/src/ui/components/JobProgress.vue` (261 lines).

### Why

The owner: *"I need to scroll down to see the develop options after the dry run, that makes no sense"*.

In Develop and Batch, a dry run's **review summary** and a running job's **progress** are sections appended
below the whole screen. Since UI-1 the page no longer scrolls, but the content area does, and the result of
pressing a button still appears somewhere the eye is not. The owner chose a **status strip along the bottom
of the window**, which expands for details.

### What to build

**1. The strip, in the shell** (`App.vue`).
- The footer `.statusbar` becomes the strip. It keeps `min-height: var(--status-h)` (28px) and its right
  end: the version and the clock.
- Its left part is `<div id="status-strip-line" class="status-strip__line">`: one line, ellipsised when
  too long.
- A button **"Details"** (`data-testid="status-details-toggle"`, `aria-expanded`, at least 40px tall).
  The strip's `min-height` grows to 40px to hold it. The button shows only when the current screen has
  details.
- **The drawer**, `<div id="status-strip-details" class="status-strip__details" data-testid="status-details">`:
  - positioned above the strip, over the content (`position: absolute; bottom: <strip height>`), the full
    width of the main column;
  - `max-height: 40vh; overflow-y: auto;` with `var(--bg-elevated)`, a top border
    `var(--border-hair)`, and `z-index: var(--z-status)`;
  - hidden unless open.
- Escape, the toggle, and switching screens close it.
- **A small store**, `src/statusStrip.ts`: a `reactive` object `{ hasDetails: boolean, detailsOpen: boolean }`
  and an `InjectionKey` `STATUS_STRIP`. `App.vue` creates and `provide`s it, and resets `hasDetails` and
  `detailsOpen` on every route change.

**2. Screens send their results to the strip.** In `Develop.vue` and `BulkLut.vue`:
- `const strip = inject(STATUS_STRIP, null)`. When it is `null` the screen is mounted on its own, as every
  existing check mounts it.
- **The details:**
  - Wrap the `review-section`, the plan error banner and the `job-section` in
    `<Teleport defer to="#status-strip-details" :disabled="!strip">`.
  - Their markup, classes and `data-testid`s do not change. With `strip` null they render exactly where
    they do today.
  - Keep `strip.hasDetails` true while any of them is shown.
  - When a dry run completes, or a job starts, set `strip.detailsOpen = true`, so the result appears where
    the owner just clicked.
- **The line:** a new element `<span data-testid="status-line">`, teleported to `#status-strip-line` the
  same way. It reads:
  - after a dry run: **"Dry run · N to develop · M skipped"** (Batch: "N to grade");
  - while a job runs: `<JobProgress compact … />` (item 3);
  - on a plan error: **"Dry run failed"**;
  - otherwise nothing.

**3. `JobProgress` gets a compact form** (`frontend/shared`).
- An optional prop `compact?: boolean`, default `false`, so PhotoTools and every existing use are unchanged.
- When `true` it renders **one row**, at most 28px of content: the label, a thin bar (`flex: 1`), "3 / 6",
  the timing line it already computes, and the Cancel button when `canCancel` (at least 40px tall, which
  the 40px strip allows).
- It reuses the component's own state and events, with no new fetching.
- The full form still renders in the drawer's `job-section`, so a screen's job shows in both places.
  `JobProgress`'s polling is keyed on `jobId`: two instances on the same job must not double any request
  that a check counts. If they would, the compact one takes its numbers from the full one by an emitted
  event instead. **Say which in the Report.**

**4. The actions stay put.** In Develop and Batch:
- The action area (scope options where they exist, `dry-run-button`, `run-button`, `gate-explanation`)
  becomes the **fixed footer of the settings column**, as UI-2 does for Edit's Export.
- The settings above it scroll inside the column. The buttons are visible at the column's top and bottom
  scroll positions.
- **Edit:** its export progress ("Exporting i/N…") and its export status go to the strip's line the same
  way.

**5. A check: `check:strip`** (`scripts/check-strip.mjs`, on the whole-application harness, added to
`package.json`). At **1024×700 and 1920×1080**:
1. **Develop.**
   - Open the folder as `check-shell.mjs` does, then click `dry-run-button`.
   - `status-line` reads "Dry run · … to develop", and it lies inside the viewport.
   - The drawer is open and `review-section` is inside it, inside the viewport.
   - `run-button` is inside the viewport and enabled.
   - `.content`'s `scrollTop` is still 0, so nothing scrolled.
   - Click `run-button`: the strip's line shows the compact progress with a Cancel button inside the
     viewport.
2. **Escape** closes the drawer. The toggle reopens it.
3. **Switching to Edit** and back clears the line and closes the drawer.
4. **Batch.**
   - Fill the inputs as `check-bulk-lut.mjs` does and click its `dry-run-button`.
   - The same assertions for its line, drawer and `run-button`.
5. **No page or content scroll** was needed at any point: `document.scrollingElement.scrollTop === 0`, and
   `.content`'s `scrollTop === 0`.

Print "N assertions passed" and write `layout-proof/strip-<screen>-<width>.png`.

### Do not touch

- Every existing check script, unchanged (G7). `check:develop` and `check:bulk-lut` mount their screen
  alone, so the teleports are disabled there and they must pass as they are. If one fails, report it with
  its output.
- The logic of planning, running and cancelling: only where things are rendered changes.
- `docs/` other than this file's Report, and any Rust.

### Acceptance

- [ ] `check:strip` passes. After a dry run or a start, the result and the next action are visible with
      nothing scrolled.
- [ ] `JobProgress` without `compact` renders as before, and PhotoTools' gates pass unchanged.
- [ ] Every gate passes, `check:shell` and `check:edit-layout` included.

### Gates

The gate list in `CLAUDE.md` in full, plus `check:edit-layout` and `check:strip`.

### How to work: every step costs money

1. **Run every command in the foreground and wait for it to return.** Never start a command in the
   background and check on it, and never loop or sleep waiting for one.
2. **While working, run only** `typecheck`, `check:develop`, `check:bulk-lut` and `check:strip`.
3. **Run the full list under Gates once, at the end.**
4. **Read files in ranges.** Do not re-read a range to confirm an edit.
5. **Do not change approach silently.** If the same failure survives two attempts, stop, say what you tried
   in the Report, and commit what works.

### Report back

Under `## Report` in this file: what was done; **every** deviation, with the reason ("None" only if true);
how the compact `JobProgress` gets its numbers; every gate's actual output, counts not "passed"; failing
existing checks with their output; anything unverified. Commit on `ui-6` with imperative subjects under 72
characters and no model name anywhere. Do not merge.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
