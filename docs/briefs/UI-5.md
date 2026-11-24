# `UI-5` · Batch in the application's frame

> **Status:** briefed
> **Plan:** [`docs/rollchrome/app-layout.md`](../rollchrome/app-layout.md), step UI-5. **Branch:** `ui-5`, cut from
> `main` after UI-8 is merged.

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md`: **Ground rules**, **Gates**, **The design language**.
- `docs/rollchrome/app-layout.md`: rules **1–3 and 6**.
- `frontend/rollchrome/src/views/BulkLut.vue` (about 950 lines) as left by UI-6. Its parts are:
  - the page head;
  - `.form-container`, holding `.inputs-section` (`PathListField`: the sources, Add a folder, Include
    subfolders), `.source-section` (3D LUT or Preset, the preset select or `LutPicker`),
    `.destination-section` (`PathField`) and the action area;
  - the review summary and job progress, which UI-6 moved to the status strip.
- `frontend/rollchrome/scripts/check-bulk-lut.mjs`.

### Why

The interface review (`ui-review.md`, finding F) found Batch to be a long single column of forms. Most of it
is below the fold at 700–800 px of window height, and the notice explaining why Run is unavailable sits at
the very bottom. The owner wants every screen to behave like a desktop application's.

### What to build

**Batch fills the content area in two columns, and the page and content never scroll.**
- **Left column** (`minmax(0, 1fr)`): **What to grade**, the `PathListField` with its textarea, Add a
  folder and Include subfolders. Below it, **Where**: the destination `PathField`, with `overlay`.
  - The textarea grows to fill the column's height (at least 6 rows), and scrolls on its own.
- **Right column** (360 px, resizable like Develop's settings column):
  - 320–560 px, `data-testid="batch-settings-divider"`, remembered as `rollchrome.batch.settingsWidth`;
  - **Grade with**: 3D LUT or Preset, then the preset select or `LutPicker`, which since UI-3 has its
    groups.
  - Its body scrolls on its own above the **fixed action footer** UI-6 gave it: Dry run, Grade, and the
    notice `gate-explanation`, which is now always in view.
- **The details drawer opens beside the right column, never under its footer.** Since the UI-6 follow-up,
  the status strip's store has `detailsInsetRight`, and Develop sets it from its settings column with a
  `ResizeObserver` (`controlsPanelRef`, `updateStripInset` in `Develop.vue`). Batch does the same for its
  right column, and resets it to 0 on unmount.
  - `check:batch-layout` asserts it: after a dry run, the drawer's right edge is at or left of the right
    column's left edge (±1).
- **The page head** shrinks to one line: the title and the one-sentence description, side by side.
- **Nothing is appended below.** Results are in the status strip since UI-6.
- **Every `data-testid`, every control and every behaviour stays.** Only where they sit changes.

**A check: `check:batch-layout`** (`scripts/check-batch-layout.mjs`, on the whole-application harness; added
to `package.json`). At **1024×700 and 1920×1080**:
- `.content`'s `scrollHeight <= clientHeight + 1`;
- `dry-run-button`, the run button and `gate-explanation` lie inside the viewport;
- with every source chosen, as `check-bulk-lut.mjs` fills them, no control inside the screen is clipped by
  its nearest clipping ancestor (the test `check-edit-layout.mjs` uses, reused);
- the divider clamps to 320 and 560 and is remembered. Clear the key at the end.

Print "N assertions passed" and write `layout-proof/batch-layout-<width>.png`.

### Do not touch

- `check-bulk-lut.mjs` and every other check, unchanged (G7).
- Batch's logic, `PathListField`'s and `LutPicker`'s interfaces, and any Rust.
- `docs/` other than this file's Report.

### Acceptance

- [ ] Batch is two columns, with the actions and their notice always on screen and nothing scrolling but the
      sources list and the settings body.
- [ ] `check:batch-layout` passes; every gate passes.

### Gates

The gate list in `CLAUDE.md` in full, plus `check:edit-layout`, `check:strip`, `check:edit-folder` and
`check:batch-layout`.

### How to work: every step costs money

1. **Run every command in the foreground and wait for it to return.** Never start a command in the
   background and check on it, and never loop or sleep waiting for one.
2. **While working, run only** `typecheck`, `check:bulk-lut` and `check:batch-layout`.
3. **Run the full list under Gates once, at the end.**
4. **Read files in ranges.** Do not re-read a range to confirm an edit.
5. **Do not change approach silently.** If the same failure survives two attempts, stop, say what you tried
   in the Report, and commit what works.

### Report back

Under `## Report` in this file: what was done; **every** deviation, with the reason ("None" only if true);
every gate's actual output, counts not "passed"; anything unverified. Commit on `ui-5` with imperative
subjects under 72 characters and no model name anywhere. Do not merge.

---

## Report

### What was done
- Refactored `frontend/rollchrome/src/views/BulkLut.vue` into a two-column desktop application layout:
  - The page head shrunk to one line: `h1` and `p.muted` sit side-by-side without wrapping.
  - Left column (`minmax(0, 1fr)`): "What to grade" (`PathListField` with textarea, "Add a folder...", and "Include subfolders") and "Where" (destination `PathField` with overlay). The textarea grows via flex to fill column height (at least 6 rows) and scrolls on its own.
  - Resizable divider (`data-testid="batch-settings-divider"`) clamping between 320px and 560px, remembered in localStorage as `rollchrome.batch.settingsWidth`.
  - Right column (`360px` default): "Grade with" toggle (3D LUT or Preset), preset select or `LutPicker` (with look groups and intensity slider), scrolling on its own above a fixed action footer with Dry run, Grade button, and `gate-explanation`.
  - Details drawer inset: Batch sets `strip.detailsInsetRight` from its settings column with a `ResizeObserver` (`settingsColumnRef`, `updateStripInset`), and resets it to 0 on unmount.
- Added `frontend/rollchrome/scripts/check-batch-layout.mjs` on the whole-application harness:
  - Asserts at 1024×700 and 1920×1080 that `.content`'s `scrollHeight <= clientHeight + 1`.
  - Asserts `dry-run-button`, `run-button`, and `gate-explanation` lie inside the viewport.
  - Asserts divider clamps to 320 and 560 and remembered width persists across reload.
  - Asserts no control inside the screen is clipped by its nearest clipping ancestor for both LUT and Preset sources at widths 320 and 560.
  - Asserts after a dry run that the drawer's right edge is at or left of the right column's left edge (±1).
  - Writes screenshots `layout-proof/batch-layout-1024.png` and `layout-proof/batch-layout-1920.png`.
- Added `"check:batch-layout": "node scripts/check-batch-layout.mjs"` to `frontend/rollchrome/package.json`.

### Deviations
None.

### Gate results
- `cargo fmt --all --check`: clean (code 0)
- `cargo clippy --workspace --all-targets -- -D warnings`: clean (code 0)
- `cargo build --workspace`: clean, built in 5.62s (code 0)
- `cargo test --workspace`: 1215 passed, 0 failed, 5 ignored (code 0)
- `cargo test -p phototools-core`: 546 passed, 0 failed (398 lib + 46 ingest + 4 ledger + 45 media + 13 raw + 38 tools + 2 validation) (code 0)
- `cargo test -p rollchrome-core`: 531 passed, 0 failed (199 lib + 61 automation + 13 batch + 12 curves + 42 develop + 31 film_stock + 13 grading + 11 hsv + 29 lens + 43 masks + 13 object + 13 point + 5 sharpen + 45 tools) (code 0)
- `cargo test -p rollchrome-core --features film`: 553 passed, 0 failed, 3 ignored (code 0)
- `cargo test -p phototools-desktop`: 26 passed, 0 failed (19 lib + 7 local_tools) (code 0)
- `cargo test -p rollchrome-desktop`: 29 passed, 0 failed (4 bulk_edit + 12 develop + 13 local_tools) (code 0)
- `cargo test -p rollchrome-cli`: 8 passed, 0 failed (code 0)
- `npm --prefix frontend/shared run build`: clean (code 0)
- `npm --prefix frontend/web run typecheck`: clean (code 0)
- `npm --prefix frontend/web run build`: clean (code 0)
- `npm --prefix frontend/web run check:transport`: 23 files, no view reaches network directly (code 0)
- `npm --prefix frontend/web run check:layout`: 10 routes clean at 390x844 (code 0)
- `npm --prefix frontend/web run check:ingest`: 400 shots render responsively (code 0)
- `npm --prefix frontend/desktop run typecheck`: clean (code 0)
- `npm --prefix frontend/desktop run build`: clean (code 0)
- `npm --prefix frontend/desktop run check:transport`: 25 files, no view reaches network directly (code 0)
- `npm --prefix frontend/rollchrome run typecheck`: clean (code 0)
- `npm --prefix frontend/rollchrome run build`: clean (code 0)
- `npm --prefix frontend/rollchrome run check:transport`: 41 files, no view reaches network directly and no HTTP client imported (code 0)
- `npm --prefix frontend/rollchrome run check:edit`: drag p95 12.6ms, settle p95 46.8ms, all touch targets and checks passed (code 0)
- `npm --prefix frontend/rollchrome run check:bulk-lut`: all acceptance assertions and benchmarks verified (code 0)
- `npm --prefix frontend/rollchrome run check:develop`: 34 checks verified (code 0)
- `npm --prefix frontend/rollchrome run check:viewing`: 18 checks verified (code 0)
- `npm --prefix frontend/rollchrome run check:colour`: 11 checks verified (code 0)
- `npm --prefix frontend/rollchrome run check:history`: 16 checks verified (code 0)
- `npm --prefix frontend/rollchrome run check:versions`: 14 checks verified (code 0)
- `npm --prefix frontend/rollchrome run check:detail`: 13 checks verified (code 0)
- `npm --prefix frontend/rollchrome run check:export`: 11 checks verified (code 0)
- `npm --prefix frontend/rollchrome run check:point`: 11 checks verified (code 0)
- `npm --prefix frontend/rollchrome run check:masks`: 13 checks verified (code 0)
- `npm --prefix frontend/rollchrome run check:local-detail`: 8 checks verified (code 0)
- `npm --prefix frontend/rollchrome run check:object`: 15 checks verified (code 0)
- `npm --prefix frontend/rollchrome run check:people`: 7 checks verified (code 0)
- `npm --prefix frontend/rollchrome run check:shell`: 108 assertions passed (code 0)
- `npm --prefix frontend/rollchrome run check:edit-layout`: 51 assertions passed (code 0)
- `npm --prefix frontend/rollchrome run check:strip`: 70 assertions passed (code 0)
- `npm --prefix frontend/rollchrome run check:edit-folder`: 32 assertions passed (code 0)
- `npm --prefix frontend/rollchrome run check:batch-layout`: 64 assertions passed (code 0)

### Anything unverified
None.

---

## Review

*Written by the architect, after re-running the gates on the branch.*
