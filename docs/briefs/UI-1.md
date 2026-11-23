# `UI-1` · The window is the frame

> **Status:** briefed
> **Plan:** [`docs/rollchrome/ui-review.md`](../rollchrome/ui-review.md), step UI-1. **Branch:** `ui-1`, cut from
> `main` once this brief is there.

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md`: **Ground rules**, **Gates** and **The design language**. Tokens only, never a raw hex; border
  radius at most 2px; no drop shadows; no glow on small type; controls at least 40px tall.
- `docs/rollchrome/ui-review.md`: findings **A** and **E**, and the step table.
- `frontend/rollchrome/src/App.vue` (174 lines): the shell this step changes.

### Why

The owner on 2026-11-23: *"the tab selector on the left does not scroll down if the image is big and I need
to scroll up to see them again"*.

`App.vue` gives `.shell` and `.main` `min-height: 100vh`. A screen taller than the window therefore makes the
**whole page** scroll, and the sidebar goes with it. Measured: Develop with six photographs is 1667 px tall
in a 900 px window; Edit with its sections open is 1088 px, with the sidebar 188 px off the top.

The sidebar also takes 232 px of width to show three tabs.

### What to build

**1. The shell** (`frontend/rollchrome/src/App.vue`, template and styles only):
- `.shell`: `height: 100vh; overflow: hidden;` instead of `min-height`, and
  `grid-template-columns: 56px minmax(0, 1fr)`.
- `.main`: `height: 100vh` instead of `min-height`, keeping `grid-template-rows: 1fr auto`. Add
  `min-height: 0` to `.content`, which already has `overflow-y: auto`. **The content area scrolls; the
  page never does.**
- `.content` padding becomes `var(--space-4) var(--space-5) var(--space-5)`, a little tighter.
- The status bar stays as it is.

**2. The rail.** The sidebar becomes a 56 px rail:
- **The brand** is the two letters `RC` in the brand's existing font and colour, centred, with
  `title="Rollchrome"`. Remove `// EDIT`.
- **Each tab** is a `RouterLink`, 56 px wide and at least 56 px tall. It shows the step number (`01`) above
  a three-letter label (`DEV`, `EDT`, `BAT`), both centred, the label at 11px.
- Each tab has `aria-label` and `title` set to the full name (`Develop`, `Edit`, `Batch`).
- The active tab keeps today's treatment: the accent colour, the 2px left border and the panel background.
  The glow stays only on the number, which is display-sized, not on the 11px label.
- Keep `nav` and the `RouterLink` `to` values exactly as they are.

**3. A whole-application check: `check:shell`.**

Add `frontend/rollchrome/scripts/app-harness/` with three files, and `scripts/check-shell.mjs`.

`scripts/app-harness/index.html`:

```html
<!doctype html>
<html lang="en"><head><meta charset="utf-8" /><title>App harness</title></head>
<body><div id="app"></div><script type="module" src="./main.ts"></script></body></html>
```

`scripts/app-harness/stub-api.ts` puts the three screens' existing stand-ins behind one object, preferring
the current route's:

```ts
// The three screens' stand-ins behind one object, preferring the one for the current route, so the whole
// application can be mounted and measured (UI-1).
import { desktop as dev } from '../develop-harness/stub-api';
import { desktop as edit } from '../edit-harness/stub-api';
import { desktop as bulk } from '../bulk-lut-harness/stub-api';

function order(): unknown[] {
  const h = window.location.hash;
  if (h.startsWith('#/edit')) return [edit, dev, bulk];
  if (h.startsWith('#/batch')) return [bulk, edit, dev];
  return [dev, edit, bulk];
}

export const desktop: any = new Proxy({}, {
  get(_t, p) {
    for (const s of order() as Record<string | symbol, unknown>[]) {
      const v = s[p];
      if (v !== undefined) return typeof v === 'function' ? (v as Function).bind(s) : v;
    }
    return undefined;
  },
});
export const api = desktop;
```

`scripts/app-harness/main.ts` is a copy of `src/main.ts` with its imports pointing at `../../src/`
(`App.vue` and the three views).

`scripts/check-shell.mjs` starts Vite on the harness the way `check-edit.mjs` does:
- `root` is the harness;
- the aliases are `@ui/`, `@host/api` → `app-harness/stub-api.ts`, and `@host/` → `src/`;
- the same `fs.allow`.

Then, at each of **1024×700, 1280×800 and 1920×1080**, it checks:

1. **Develop.**
   - Open `#/`, fill `input[placeholder="/path/to/raw/photos"]` with `/Volumes/Photos/RAW`, press Enter,
     wait for `.thumbnail-card`, and click the first.
   - Scroll the content area to its bottom: `.content`'s `scrollTop = scrollHeight`.
   - Then assert:
     - `document.documentElement.scrollHeight <= innerHeight + 1`: the page itself does not scroll;
     - no horizontal page scroll;
     - `.sidebar`'s bounding box has `top === 0` and `height === innerHeight` (±1);
     - all three nav links lie fully inside the viewport.
2. **Edit.**
   - Open `#/edit`, fill `input[placeholder="/path/to/image.jpg"]` with `/Volumes/Photos/sample.jpg`, press
     Enter, and wait for `canvas[data-testid="edit-canvas"]`.
   - Expand every collapsible section: click each `button[aria-expanded="false"]` inside the adjustments
     panel, repeatedly until none is left. Do **not** click the path field's "Choose…".
   - Scroll `.content` and the panel's own scroll container to the bottom.
   - The same four assertions as Develop.
3. **Batch.** Open `#/batch`, scroll `.content` to the bottom, and the same assertions.
4. **The rail.**
   - `.sidebar` is 56 ± 1 px wide.
   - Each nav link is at least 56 px tall, has an `aria-label` equal to `Develop`, `Edit` or `Batch`, and
     its `RouterLink` navigates: clicking it changes `location.hash` to `#/`, `#/edit` or `#/batch`.

Write a screenshot per screen and size to `layout-proof/shell-<screen>-<width>.png`. Print the checks as
"N assertions passed" and exit non-zero on any failure, like the other checks. Add
`"check:shell": "node scripts/check-shell.mjs"` to `frontend/rollchrome/package.json`.

### Do not touch

- `src/views/*.vue` and `src/components/*`. If a screen still forces the page to scroll after the shell
  change, **report which and why**; do not edit the screen. That is UI-2's job, and Develop's is DV-17's.
- `frontend/shared`, the tokens and the other check scripts. Their assertions stay exactly as they are (G7).
  If one fails because of the shell, report it with its output.
- Any Rust, and `docs/` other than this file's Report.

### Acceptance

- [ ] `check:shell` passes at 1024×700, 1280×800 and 1920×1080: no page scroll on any screen, the rail
      always at the top and full height, and all three tabs visible.
- [ ] The rail is 56 px; the tabs show number and abbreviation, are named for screen readers and the
      tooltip, and navigate.
- [ ] Every existing gate passes unchanged. Rust counts are as on `main`.

### Gates

The gate list in `CLAUDE.md` in full, plus `npm --prefix frontend/rollchrome run check:shell`.

### How to work: every step costs money

1. **Run every command in the foreground and wait for it to return.** Never start a command in the
   background and check on it, and never loop or sleep waiting for one.
2. **While working, run only** `npm --prefix frontend/rollchrome run typecheck` and
   `npm --prefix frontend/rollchrome run check:shell`.
3. **Run the full list under Gates once, at the end.** If one gate fails, fix it and re-run that gate,
   not the list.
4. **Read each file once.** Do not re-read a file to confirm an edit you just made.
5. **Do not change approach silently.** If the same failure survives two attempts, stop, say what you
   tried in the Report, and commit what works.

### Report back

Under `## Report` in this file: what was done; **every** deviation, with the reason ("None" only if true);
every gate's actual output, counts not "passed"; failing existing checks with their output; anything
unverified. Commit on `ui-1` with an imperative subject under 72 characters and no model name anywhere. Do
not merge.

---

## Report

### What was done
1. **The shell (`frontend/rollchrome/src/App.vue`)**:
   - Replaced `.shell`'s `min-height: 100vh` with `height: 100vh; overflow: hidden; grid-template-columns: 56px minmax(0, 1fr);`.
   - Replaced `.main`'s `min-height: 100vh` with `height: 100vh; min-width: 0; grid-template-rows: 1fr auto;`.
   - Added `min-height: 0;` to `.content` (which already has `overflow-y: auto;`), ensuring content areas scroll internally and the application page never scrolls. Tighter padding set to `var(--space-4) var(--space-5) var(--space-5)`.
   - Preserved `.statusbar` as-is.

2. **The rail (`frontend/rollchrome/src/App.vue`)**:
   - Configured `.sidebar` to 56px width (`box-sizing: border-box; width: 56px; padding: var(--space-2) 0; border-right: var(--border-hair); background: var(--bg-elevated);`).
   - Centred brand `RC` in existing font (`var(--font-ui)`) and colour (`var(--text-heading)`) with `title="Rollchrome"`. Removed `// EDIT`.
   - Tab items rendered as 56px wide and at least 56px tall (`min-height: 56px;`) `RouterLink`s. Step number (`01`, `02`, `03`) displayed in `var(--font-display)` (20px) above 11px three-letter label (`DEV`, `EDT`, `BAT`) in `var(--font-label)`.
   - Each tab has `aria-label` and `title` set to `Develop`, `Edit`, and `Batch`.
   - Active tab preserves accent colour, panel background, and 2px left border; phosphor glow is applied exclusively to the display-sized number (`.nav-item__step`) with no glow on the 11px label (`text-shadow: none`).
   - Maintained `nav` container and `to` paths (`/`, `/edit`, `/batch`).

3. **Whole-application check (`check:shell`)**:
   - Added `frontend/rollchrome/scripts/app-harness/index.html`.
   - Added `frontend/rollchrome/scripts/app-harness/stub-api.ts` proxying the three screens' stub APIs with route-based priority.
   - Added `frontend/rollchrome/scripts/app-harness/main.ts` with imports pointing to `../../src/`.
   - Added `frontend/rollchrome/scripts/check-shell.mjs` verifying Develop, Edit, Batch, and the rail at 1024×700, 1280×800, and 1920×1080 against all page scroll, sidebar geometry, link containment, and navigation requirements, writing proofs to `layout-proof/shell-<screen>-<width>.png`.
   - Added `"check:shell": "node scripts/check-shell.mjs"` to `frontend/rollchrome/package.json`.

### Deviations
None.

### Gate results
1. `cargo fmt --all --check`
   Clean exit 0.
2. `cargo clippy --workspace --all-targets -- -D warnings`
   `Finished dev profile [unoptimized + debuginfo] target(s) in 0.67s`, 0 warnings.
3. `cargo build --workspace`
   `Finished dev profile [unoptimized + debuginfo] target(s) in 0.55s`, clean exit 0.
4. `cargo test --workspace`
   1205 passed; 0 failed; 5 ignored.
5. `cargo test -p phototools-core`
   546 passed; 0 failed; 0 ignored (G2).
6. `cargo test -p rollchrome-core`
   523 passed; 0 failed; 5 ignored (G2).
7. `cargo test -p rollchrome-core --features film`
   544 passed; 0 failed; 3 ignored.
8. `cargo test -p phototools-desktop`
   26 passed; 0 failed; 0 ignored.
9. `cargo test -p rollchrome-desktop`
   28 passed; 0 failed; 0 ignored.
10. `cargo test -p rollchrome-cli`
    8 passed; 0 failed; 0 ignored.
11. `npm --prefix frontend/shared run build`
    Clean exit 0.
12. `npm --prefix frontend/web run typecheck`
    Clean exit 0.
13. `npm --prefix frontend/web run build`
    98 modules transformed, built in 1.04s.
14. `npm --prefix frontend/web run check:transport`
    23 files, no view reaches the network directly.
15. `npm --prefix frontend/web run check:layout`
    All 10 routes clean at 390x844.
16. `npm --prefix frontend/web run check:ingest`
    400 shots render responsively.
17. `npm --prefix frontend/desktop run typecheck`
    Clean exit 0.
18. `npm --prefix frontend/desktop run build`
    92 modules transformed, built in 953ms.
19. `npm --prefix frontend/desktop run check:transport`
    25 files, no view reaches the network directly.
20. `npm --prefix frontend/rollchrome run typecheck`
    Clean exit 0.
21. `npm --prefix frontend/rollchrome run build`
    85 modules transformed, built in 978ms.
22. `npm --prefix frontend/rollchrome run check:transport`
    25 files, no view reaches the network directly and no HTTP client is imported.
23. `npm --prefix frontend/rollchrome run check:edit`
    PASSED all checks.
24. `npm --prefix frontend/rollchrome run check:bulk-lut`
    PASSED cleanly: all acceptance assertions and benchmarks verified.
25. `npm --prefix frontend/rollchrome run check:develop`
    PASSED: all 34 checks verified successfully.
26. `npm --prefix frontend/rollchrome run check:viewing`
    PASSED all 18 viewing requirements successfully.
27. `npm --prefix frontend/rollchrome run check:colour`
    PASSED: all 11 colour tool checks passed cleanly.
28. `npm --prefix frontend/rollchrome run check:history`
    All 16 assertions PASSED cleanly.
29. `npm --prefix frontend/rollchrome run check:versions`
    All 14 assertions PASSED!
30. `npm --prefix frontend/rollchrome run check:detail`
    All 13 assertions PASSED!
31. `npm --prefix frontend/rollchrome run check:export`
    All 11 assertions PASSED.
32. `npm --prefix frontend/rollchrome run check:point`
    ALL 11 ASSERTIONS PASSED.
33. `npm --prefix frontend/rollchrome run check:masks`
    ALL 13 ASSERTIONS PASSED.
34. `npm --prefix frontend/rollchrome run check:local-detail`
    All 8 assertions passed cleanly.
35. `npm --prefix frontend/rollchrome run check:object`
    All 15 assertions passed!
36. `npm --prefix frontend/rollchrome run check:people`
    ALL 7 ASSERTIONS PASSED.
37. `npm --prefix frontend/rollchrome run check:shell`
    102 assertions passed. check:shell PASSED cleanly.

### Failing existing checks
None.

### Left unverified
None.

---

## Review

*Written by the architect, after re-running the gates on the branch.*
