# `UI-7` · Develop with a filmstrip

> **Status:** briefed
> **Plan:** [`docs/rollchrome/app-layout.md`](../rollchrome/app-layout.md), step UI-7. **Branch:** `ui-7`, cut from
> `main` after UI-6 is merged.

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md`: **Ground rules**, **Gates**, **The design language** and **The front-end boundary**.
- `docs/rollchrome/app-layout.md`: all of it. Rule **4** is this step.
- `docs/briefs/DV-17.md`: the layout this step replaces, and its divider, whose pattern is reused here.
- `frontend/rollchrome/src/views/Develop.vue` as left by DV-17 and UI-6: the workspace grid, the photos panel
  and its thumbnail cards, the selection logic (click, Cmd-click, Shift-click, Select all), the divider, the
  preview panel, the settings column with its fixed action footer.
- `frontend/rollchrome/scripts/check-develop.mjs`: checks 1–34. Note every selector a check uses on the
  thumbnails: `.thumbnail-card[data-filename=…]`, `.thumb-img`, `.thumb-look` and the classes `active` and
  `is-last-clicked`.

### Why

The owner chose Lightroom's Develop arrangement on 2026-11-23: **the preview in the middle, the folder's
photographs as a filmstrip along the bottom, the settings on the right.** DV-17's photographs column at the
left, and its divider, give way to the filmstrip, so the preview gets the full width between the rail and the
settings.

### What to build

**1. `Filmstrip.vue`** (`frontend/rollchrome/src/components/`), shared with Edit in UI-8. Transport-free:
props in, events out, never `@host/api`.
- **Props:**
  - `items: { name: string; caption?: string }[]`;
  - `thumbnailUrls: Record<string, string>`;
  - `thumbnailErrors?: Record<string, string>`;
  - `selection: string[]`;
  - `active: string | null`.
- **Emits:**
  - `select(name, { meta: boolean, shift: boolean })` on click;
  - `activate(name)` on ←/→ with focus in the strip (Shift+←/→ emits `select` with `shift: true`);
  - `selectAll()` on Cmd+A with focus in the strip.
- **Renders one horizontal row**, scrolling on its own (`overflow-x: auto; overflow-y: hidden`), about
  **132 px** tall.
- **Each card** keeps today's markup and hooks exactly:
  - the classes `thumbnail-card`, `active` (selected) and `is-last-clicked` (the active one);
  - the attribute `data-filename`;
  - inside, `.thumb-container` with `.thumb-img`, `.thumb-error` or `.thumb-placeholder`;
  - then `.thumb-meta` with `.thumb-name` and `.thumb-look`, which shows `caption`.
- Cards are about 120 px wide, with the thumbnail at 3:2 and `object-fit: contain`.
- **The active card scrolls into view** when `active` changes (`scrollIntoView({ inline: 'nearest' })`).
- The strip is a focusable `role="listbox"` with `aria-label="Photographs"`, `aria-multiselectable="true"`.
  Cards are `role="option"` with `aria-selected`.

**2. Develop's new layout.**
- **Top bar** (one row):
  - the RAW folder field (`develop-folder-input`, `PathField` with `overlay`);
  - the count "6 photographs";
  - **Select all** (`select-all-button`), moved here from the photos panel.
- **Middle:** `grid-template-columns: minmax(0, 1fr) 12px var(--settings-width)`.
  - The **preview panel**, unchanged inside, with Enlarge.
  - A **divider** (`data-testid="develop-settings-divider"`, DV-17's pattern) sizes the **settings column**:
    320–560 px, default 360, remembered as `rollchrome.develop.settingsWidth`.
  - The settings column keeps UI-6's fixed action footer.
- **Bottom:** the `Filmstrip`, the full width of the preview plus the divider, under the preview. The
  settings column runs the full height, beside both.
- **Remove** DV-17's photos panel, its divider (`develop-divider`), `--photos-width` and the
  `rollchrome.develop.photosWidth` key, and the stacked layout below 1100 px. The three zones fit at
  1024 px.
- **The screen fills `.content`**, and only the filmstrip, the settings column's body and the strip's
  details scroll. **The preview's height is the space between the top bar and the filmstrip.** Remove the
  `--preview-top` measuring if it is no longer needed.
- **Selection behaviour is unchanged:** today's handlers now receive the filmstrip's events. ←/→ changes the
  previewed photograph.

**3. `check:develop`.**
- **Checks 1–32 and 34 stay exactly as they are** (G7). The filmstrip keeps every selector they use, so they
  must pass unchanged.
- **Check 33 tested DV-17's photos divider, which the owner's choice removes.** It is the one check this
  step replaces. Rewrite it, and its line in the header comment, as **"33. The filmstrip and the settings
  divider (UI-7)"**:
  - all six cards share the same `top` (±1), inside `[role="listbox"][aria-label="Photographs"]`;
  - with focus in the strip, ArrowRight makes the next card `is-last-clicked`, and the next preview request
    names that photograph;
  - Shift+ArrowRight adds the next card to the selection;
  - dragging `develop-settings-divider` by −2000 and +2000 clamps the settings column to 560 and 320 (±1),
    and a reload keeps the last width. Clear the key at the end.
- **Say in the Report exactly what changed in check 33.** No other check changes.

**4. `check:shell` and `check:strip` must pass unchanged.** Add to `check:shell`'s Develop pass, without
changing its existing assertions: the filmstrip lies fully inside the viewport, and `run-button` lies
inside the viewport.

### Do not touch

- Develop's logic: planning, assignments, the preview requests, the latest-wins handling, Enlarge, the
  output settings.
- Edit, Batch, `frontend/shared` (except as needed for `PathField`'s `overlay`, which UI-2 added), and any
  Rust.
- `docs/` other than this file's Report.

### Acceptance

- [ ] Develop shows the preview in the middle, the filmstrip below it and the settings on the right, with
      nothing scrolling but the filmstrip, the settings' body and the strip's details.
- [ ] `check:develop` 34/34, with only check 33 rewritten as above. `check:shell` and `check:strip` pass.
- [ ] The preview is at least as large as DV-17's minimums (check 32).

### Gates

The gate list in `CLAUDE.md` in full, plus `check:edit-layout` and `check:strip`.

### How to work: every step costs money

1. **Run every command in the foreground and wait for it to return.** Never start a command in the
   background and check on it, and never loop or sleep waiting for one.
2. **While working, run only** `typecheck`, `check:develop` and `check:shell`.
3. **Run the full list under Gates once, at the end.**
4. **Read files in ranges.** Do not re-read a range to confirm an edit.
5. **Do not change approach silently.** If the same failure survives two attempts, stop, say what you tried
   in the Report, and commit what works.

### Report back

Under `## Report` in this file: what was done; **every** deviation, with the reason ("None" only if true);
the old and new check 33; every gate's actual output, counts not "passed"; anything unverified. Commit on
`ui-7` with imperative subjects under 72 characters and no model name anywhere. Do not merge.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
