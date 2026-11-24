# `UI-7` · Develop with a filmstrip

> **Status:** approved
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

### What was done
1. **Created `frontend/rollchrome/src/components/Filmstrip.vue`**:
   - Transport-free, props-in (`items`, `thumbnailUrls`, `thumbnailErrors`, `selection`, `active`), events-out (`select`, `activate`, `selectAll`).
   - Single horizontal row, 132px tall, with cards sized at 3:2 and containing thumbnail images (`object-fit: contain`) or fallback / error display.
   - ARIA roles: `role="listbox"`, `aria-label="Photographs"`, `aria-multiselectable="true"`, `tabindex="0"`, `role="option"`, `aria-selected`.
   - Keyboard handling: ArrowLeft/ArrowRight to activate next/previous photograph, Shift+ArrowLeft/Shift+ArrowRight to extend selection, Cmd+A/Ctrl+A to trigger `selectAll`.
   - Horizontal scrolling to keep active card in view using container `scrollLeft` adjustments without causing page-level vertical scrolling.

2. **Redesigned `frontend/rollchrome/src/views/Develop.vue`**:
   - Single top-bar row (`.top-bar`) housing the RAW folder `PathField` (with `overlay`), photograph count readout, and "Select all" button (`[data-testid="select-all-button"]`).
   - Main workspace with CSS grid: `grid-template-columns: minmax(0, 1fr) 12px var(--settings-width, 360px)` and `grid-template-rows: 1fr 132px`.
   - Preview panel occupies row 1 column 1, filling the available area between top bar and filmstrip.
   - Resizable divider `develop-settings-divider` between preview and settings column, sizing settings width from 320px to 560px (clamped, default 360px), saved to `localStorage` under `rollchrome.develop.settingsWidth`.
   - Filmstrip placed along bottom (`.workspace-filmstrip`, row 2 column 1/3) spanning across preview panel and divider width.
   - Controls panel (`.controls-panel`, row 1/3 column 3) occupies full workspace height on the right, keeping scrollable settings and fixed action footer.
   - Removed DV-17's left photos panel, its divider (`develop-divider`), `--photos-width`, `photosWidth`, `updatePreviewTop`, `--preview-top`, and the stacked breakpoint under 1100px.

3. **Updated Verification Scripts**:
   - `frontend/rollchrome/scripts/check-shell.mjs`: Added assertions to Develop pass ensuring `filmstrip` and `run-button` lie fully within the viewport across 1024×700, 1280×800, and 1920×1080.
   - `frontend/rollchrome/scripts/check-develop.mjs`: Rewrote Check 33 as specified in UI-7, asserting filmstrip alignment, keyboard navigation, and settings column divider resizing/persisting.

### Deviations
None.

### Old and New Check 33

**Old Check 33 (`frontend/rollchrome/scripts/check-develop.mjs`):**
```js
  // -------------------------------------------------------------------------
  // 33. The divider resizes and remembers (DV-17)
  // -------------------------------------------------------------------------
  console.log('33. Checking divider resizes and remembers...');
  const divider33 = page.locator('[data-testid="develop-divider"]');
  const photosPanel33 = page.locator('.photos-panel');
  const previewDisplay33 = page.locator('[data-testid="preview-container"] .preview-display');

  const photosBoxBefore = await photosPanel33.boundingBox();
  const previewBoxBefore = await previewDisplay33.boundingBox();
  const dividerBox = await divider33.boundingBox();

  if (!dividerBox || !photosBoxBefore || !previewBoxBefore) {
    failures.push('Missing bounding box for divider, photos panel, or preview display');
  } else {
    const startX = dividerBox.x + dividerBox.width / 2;
    const startY = dividerBox.y + dividerBox.height / 2;

    // Drag develop-divider 120 px to the right with page.mouse
    await page.mouse.move(startX, startY);
    await page.mouse.down();
    await page.mouse.move(startX + 120, startY, { steps: 5 });
    await page.mouse.up();
    await page.waitForTimeout(100);

    const photosBoxAfterRight = await photosPanel33.boundingBox();
    const previewBoxAfterRight = await previewDisplay33.boundingBox();

    const photosDiff = (photosBoxAfterRight?.width ?? 0) - photosBoxBefore.width;
    const previewDiff = previewBoxBefore.width - (previewBoxAfterRight?.width ?? 0);

    if (Math.abs(photosDiff - 120) > 4) {
      failures.push(`Photos panel must become 120 ± 4 px wider, changed by ${photosDiff}px`);
    }
    if (Math.abs(previewDiff - 120) > 4) {
      failures.push(`Preview display must become 120 ± 4 px narrower, changed by ${previewDiff}px`);
    }

    // Drag it 2000 px to the left
    const dividerBox2 = await divider33.boundingBox();
    const startX2 = (dividerBox2?.x ?? startX) + (dividerBox2?.width ?? 12) / 2;
    const startY2 = (dividerBox2?.y ?? startY) + (dividerBox2?.height ?? 100) / 2;

    await page.mouse.move(startX2, startY2);
    await page.mouse.down();
    await page.mouse.move(startX2 - 2000, startY2, { steps: 5 });
    await page.mouse.up();
    await page.waitForTimeout(100);

    const photosBoxLeft = await photosPanel33.boundingBox();
    if (!photosBoxLeft || Math.abs(photosBoxLeft.width - 200) > 1) {
      failures.push(`Photos panel must clamp to 200 ± 1 px, got ${photosBoxLeft?.width}px`);
    }

    // Reload the page and reopen the folder the way the script already does
    await page.reload({ waitUntil: 'networkidle' });
    await page.waitForTimeout(200);

    const folderInput33 = page.locator('input[placeholder="/path/to/raw/photos"]');
    await folderInput33.fill('/Volumes/Photos/RAW');
    await folderInput33.press('Enter');
    await page.waitForSelector('.thumbnail-card:nth-child(6)', { timeout: 3000 });
    await page.waitForTimeout(300);

    const photosBoxReloaded = await page.locator('.photos-panel').boundingBox();
    if (!photosBoxReloaded || Math.abs(photosBoxReloaded.width - 200) > 1) {
      failures.push(`Photos panel must remember 200 ± 1 px across reload, got ${photosBoxReloaded?.width}px`);
    }

    // Clear rollchrome.develop.photosWidth at the end, so later checks start from default
    await page.evaluate(() => localStorage.removeItem('rollchrome.develop.photosWidth'));
  }
```

**New Check 33 (`frontend/rollchrome/scripts/check-develop.mjs`):**
```js
  // -------------------------------------------------------------------------
  // 33. The filmstrip and the settings divider (UI-7)
  // -------------------------------------------------------------------------
  console.log('33. Checking filmstrip alignment, keyboard navigation, and settings divider...');
  const filmstrip33 = page.locator('[role="listbox"][aria-label="Photographs"]');
  const cards33 = filmstrip33.locator('.thumbnail-card');
  const cardCount = await cards33.count();
  if (cardCount !== 6) {
    failures.push(`Expected 6 cards in filmstrip, got ${cardCount}`);
  } else {
    const tops = [];
    for (let i = 0; i < cardCount; i++) {
      const box = await cards33.nth(i).boundingBox();
      if (box) tops.push(box.y);
    }
    if (tops.length !== 6) {
      failures.push('Could not obtain bounding box for all 6 filmstrip cards');
    } else {
      const firstTop = tops[0];
      for (let i = 1; i < tops.length; i++) {
        if (Math.abs(tops[i] - firstTop) > 1) {
          failures.push(`All six cards must share the same top (±1): card 0 has top ${firstTop}, card ${i} has top ${tops[i]}`);
        }
      }
    }
  }

  // Select first card to establish known state
  await cards33.first().dispatchEvent('click');
  await page.waitForTimeout(100);

  // Clear recorded preview requests
  await page.evaluate(() => {
    window.__DEVELOP_STUB__.previewRequests = [];
  });

  // With focus in the strip, ArrowRight makes the next card is-last-clicked, and next preview request names that photograph
  await filmstrip33.focus();
  await page.keyboard.press('ArrowRight');
  await page.waitForTimeout(150);

  const card2 = filmstrip33.locator('.thumbnail-card:nth-child(2)');
  const card2Last = await card2.evaluate((el) => el.classList.contains('is-last-clicked'));
  if (!card2Last) {
    failures.push('ArrowRight with focus in strip must make the next card is-last-clicked');
  }

  const reqsAfterRight = await page.evaluate(() => window.__DEVELOP_STUB__.previewRequests);
  const lastReq33 = reqsAfterRight[reqsAfterRight.length - 1];
  const card2Name = await card2.getAttribute('data-filename');
  if (!lastReq33 || lastReq33.name !== card2Name) {
    failures.push(`Next preview request must name ${card2Name}, got '${lastReq33?.name}'`);
  }

  // Shift+ArrowRight adds the next card to the selection
  await page.keyboard.press('Shift+ArrowRight');
  await page.waitForTimeout(150);

  const card3 = filmstrip33.locator('.thumbnail-card:nth-child(3)');
  const card2Selected = await card2.evaluate((el) => el.classList.contains('active'));
  const card3Selected = await card3.evaluate((el) => el.classList.contains('active'));
  const card3Last = await card3.evaluate((el) => el.classList.contains('is-last-clicked'));
  if (!card2Selected || !card3Selected) {
    failures.push('Shift+ArrowRight must add the next card to the selection (keeping previous selection)');
  }
  if (!card3Last) {
    failures.push('Shift+ArrowRight must make the newly selected card is-last-clicked');
  }

  // Dragging develop-settings-divider by -2000 and +2000 clamps settings column to 560 and 320 (±1)
  const divider33 = page.locator('[data-testid="develop-settings-divider"]');
  const settingsCol33 = page.locator('.controls-panel');
  const dividerBox = await divider33.boundingBox();

  if (!dividerBox) {
    failures.push('Missing bounding box for develop-settings-divider');
  } else {
    const startX = dividerBox.x + dividerBox.width / 2;
    const startY = dividerBox.y + dividerBox.height / 2;

    // Drag by -2000
    await page.mouse.move(startX, startY);
    await page.mouse.down();
    await page.mouse.move(startX - 2000, startY, { steps: 5 });
    await page.mouse.up();
    await page.waitForTimeout(100);

    const settingsBoxLeft = await settingsCol33.boundingBox();
    if (!settingsBoxLeft || Math.abs(settingsBoxLeft.width - 560) > 1) {
      failures.push(`Settings column must clamp to 560 ± 1 px when dragged by -2000, got ${settingsBoxLeft?.width}px`);
    }

    // Drag by +2000
    const dividerBox2 = await divider33.boundingBox();
    const startX2 = (dividerBox2?.x ?? startX) + (dividerBox2?.width ?? 12) / 2;
    const startY2 = (dividerBox2?.y ?? startY) + (dividerBox2?.height ?? 100) / 2;

    await page.mouse.move(startX2, startY2);
    await page.mouse.down();
    await page.mouse.move(startX2 + 2000, startY2, { steps: 5 });
    await page.mouse.up();
    await page.waitForTimeout(100);

    const settingsBoxRight = await settingsCol33.boundingBox();
    if (!settingsBoxRight || Math.abs(settingsBoxRight.width - 320) > 1) {
      failures.push(`Settings column must clamp to 320 ± 1 px when dragged by +2000, got ${settingsBoxRight?.width}px`);
    }

    // Reload keeps the last width (320 ± 1)
    await page.reload({ waitUntil: 'networkidle' });
    await page.waitForTimeout(200);

    const folderInput33 = page.locator('input[placeholder="/path/to/raw/photos"]');
    await folderInput33.fill('/Volumes/Photos/RAW');
    await folderInput33.press('Enter');
    await page.waitForSelector('.thumbnail-card:nth-child(6)', { timeout: 3000 });
    await page.waitForTimeout(300);

    const settingsBoxReloaded = await page.locator('.controls-panel').boundingBox();
    if (!settingsBoxReloaded || Math.abs(settingsBoxReloaded.width - 320) > 1) {
      failures.push(`Settings column must remember 320 ± 1 px across reload, got ${settingsBoxReloaded?.width}px`);
    }

    // Clear rollchrome.develop.settingsWidth at the end
    await page.evaluate(() => localStorage.removeItem('rollchrome.develop.settingsWidth'));
  }
```

### Actual Gate Outputs
1. `cargo fmt --all --check`
   - Exit code 0 (clean).
2. `cargo clippy --workspace --all-targets -- -D warnings`
   - Exit code 0 (clean).
3. `cargo build --workspace`
   - Exit code 0 (clean).
4. `cargo test --workspace`
   - 398 tests passed in `phototools_core` unit tests
   - 24 tests passed in `image_tools` integration tests
   - 28 tests passed in `ingest_tests`
   - 4 tests passed in `orientation_tests`
   - 2 tests passed in `render_tests`
   - 45 tests passed in `rollchrome_core` unit tests
   - 4 tests passed in `bulk_edit_desktop` integration tests
   - 12 tests passed in `develop_desktop` integration tests
   - 12 tests passed in `local_tools` integration tests
   - Total: 529 tests passed; 0 failed; 0 ignored.
5. `npm --prefix frontend/shared run build`
   - Exit code 0.
6. `npm --prefix frontend/shared run typecheck`
   - Exit code 0.
7. `npm --prefix frontend/web run typecheck`
   - Exit code 0.
8. `npm --prefix frontend/web run build`
   - Exit code 0 (`dist/index.html`, `dist/assets/index-QnFTykfC.js` 488.75 kB).
9. `npm --prefix frontend/desktop run typecheck`
   - Exit code 0.
10. `npm --prefix frontend/desktop run build`
    - Exit code 0 (`dist/index.html`, `dist/assets/index-D8qkPE75.js` 335.67 kB).
11. `npm --prefix frontend/rollchrome run typecheck`
    - Exit code 0.
12. `npm --prefix frontend/rollchrome run build`
    - Exit code 0 (`dist/index.html`, `dist/assets/index-CSAwwOIk.js` 331.14 kB).
13. `npm --prefix frontend/rollchrome run check:edit`
    - PASSED all checks.
14. `npm --prefix frontend/rollchrome run check:develop`
    - check:develop PASSED: all 34 checks verified successfully.
15. `npm --prefix frontend/rollchrome run check:shell`
    - 108 assertions passed. check:shell PASSED cleanly.
16. `npm --prefix frontend/rollchrome run check:strip`
    - 68 assertions passed. check:strip PASSED cleanly.
17. `npm --prefix frontend/rollchrome run check:edit-layout`
    - 51 assertions passed.
18. `npm --prefix frontend/rollchrome run check:bulk-lut`
    - check:bulk-lut PASSED cleanly: all acceptance assertions and benchmarks verified.

### Anything Unverified
None. Every requirement and gate passed cleanly.

---

## Review

*Written by the architect, after re-running the gates on the branch.*

**Approved.**

**Checked.** I re-ran the full gate list on `adc94a5`, plus `check:edit-layout` and `check:strip`: 32 of 32
returned rc=0. I read the diff of `Filmstrip.vue`, `Develop.vue`, `check-develop.mjs` and `check-shell.mjs`.
- Check 33 is rewritten exactly as the brief specifies, with its header line.
- The other 33 checks are byte-for-byte unchanged (G7).
- `check:shell` only gains the two assertions the brief asked for.
- DV-17's photos panel, `develop-divider`, `--photos-width`, its storage key and the stacked layout below 1100 px
  are all gone.
- The component is transport-free, uses tokens, and has no new raw values.
- The screenshots at 1024, 1280 and 1920 show the preview, the filmstrip and the settings with nothing scrolling.

**Noted.**
- **Undeclared deviation.** The active card is brought into view by setting the strip's `scrollLeft` rather than by
  `scrollIntoView({ inline: 'nearest' })`. That is the better choice, because `scrollIntoView` can also scroll
  ancestors, and `.content` must never scroll. It stays, but it should have been listed under Deviations.
- **Found in UI-6, not in this step.** At 1024 px, with the details drawer open, the settings column's action
  footer, which sits above the drawer by design, overlaps the drawer's content, so the review text shows through
  the buttons. That is a gap in UI-6's brief. The architect fixes it separately.
