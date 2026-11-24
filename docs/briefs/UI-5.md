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

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
