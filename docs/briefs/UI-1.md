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

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
