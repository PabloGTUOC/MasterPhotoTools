# `UI-4` · Edit's sections in files of their own

> **Status:** briefed
> **Plan:** [`docs/rollchrome/ui-review.md`](../rollchrome/ui-review.md), step UI-4 (the owner chose it before
> UI-2). **Branch:** `ui-4`, cut from `main` once this brief is there.

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md`: **Ground rules**, **Gates** and **The design language**.
- `docs/rollchrome/ui-review.md`: finding **G** and the step table.
- `frontend/rollchrome/src/views/Edit.vue`. It has 8,349 lines: script 1–4001, template 4003–6537, styles
  6539–8349. **Read it in ranges**, never whole.

### Why

`Edit.vue` holds the whole Edit screen in one file. The next step, UI-2, fixes the adjustments panel's
layout: controls clipped at every window size. It has to change the markup and styles of individual
sections, and doing that inside an 8,349-line file is where mistakes get made. **This step moves each
adjustment section's markup and styles into a file of its own and changes nothing else.** The screen must
look and behave exactly as before; the existing checks are the proof.

### What to build

**The thirteen sections.** Each `<CollapsibleSection>` in Edit's template becomes one component in a new
folder `frontend/rollchrome/src/views/edit/`:

| Template lines (on `main`) | Section title | New file |
|---|---|---|
| 4779–4949 | Basic | `BasicSection.vue` |
| 4952–4997 | Presence | `PresenceSection.vue` |
| 5000–5222 | Masks | `MasksSection.vue` |
| 5229–5243 | Tone Curve | `ToneCurveSection.vue` |
| 5246–5501 | Colour / HSL | `HslSection.vue` |
| 5504–5591 | Colour Grading | `ColourGradingSection.vue` |
| 5594–5738 | Look | `LookSection.vue` |
| 5741–5839 | Detail | `DetailSection.vue` |
| 5842–5942 | Geometry | `GeometrySection.vue` |
| 5945–6047 | Effects | `EffectsSection.vue` |
| 6050–6075 | History | `HistorySection.vue` |
| 6078–6146 | Snapshots | `SnapshotsSection.vue` |
| 6149–6302 | Versions | `VersionsSection.vue` |

**How state reaches a section: one injected context.** Do not turn state into props and events; that would
rewrite every binding.
- Create `src/views/edit/context.ts` exporting `export const EDIT_CONTEXT: InjectionKey<EditContext> = Symbol('edit')`
  and the `EditContext` type: an object holding every ref, reactive value, computed and function that any
  section's template uses. Its type is written out field by field, with no `any`.
- In `Edit.vue`'s script, after everything is declared, `provide(EDIT_CONTEXT, { … })` with those same
  values. They are the **same objects**, not copies.
- In each section component: `const { recipe, commit, … } = inject(EDIT_CONTEXT)!`, destructuring exactly
  the names its template uses. **The template is then moved verbatim**: the same elements, classes,
  attributes, `data-testid`s, `v-if`s and bindings, character for character. Template refs used across
  sections stay in `Edit.vue` and are handed over through the context.
- `Edit.vue`'s template replaces each block with `<BasicSection />` and so on, in the same place.
- **The script logic stays in `Edit.vue`.** Moving it into composables is a later step, not this one.

**Styles move with the markup.** `Edit.vue`'s styles are `scoped`, and a parent's scoped rule does **not**
reach elements inside a child component. So:
- A rule whose selectors only match elements now inside **one** section moves verbatim into that section's
  `<style scoped>`.
- A rule matching elements in **two or more** sections, or in a section and in `Edit.vue`, moves verbatim
  into `src/views/edit/shared.css`. Every component that needs it, `Edit.vue` included, adds
  `<style scoped src="./shared.css"></style>` (`./edit/shared.css` from `Edit.vue`).
- `:deep()` rules move with the element they start from.
- Do not rewrite, merge, rename or "tidy" any rule. If a rule's placement is unclear, keep a copy in both
  places and say so in the Report.

**Work one section at a time**, in the table's order. After each section:
`npm --prefix frontend/rollchrome run typecheck` and `npm --prefix frontend/rollchrome run check:edit`, then
`git commit` on `ui-4`, with a subject like "Move Edit's Basic section into its own file". Thirteen commits
let the review find any regression's section at once. If a section breaks a check and two attempts do not
fix it, leave that section in `Edit.vue`, say so in the Report, and carry on with the next.

### Do not touch

- **Behaviour, markup and appearance.** Every existing check script stays exactly as it is (G7), and all of
  them must pass unchanged: `check:edit`, `check:viewing`, `check:colour`, `check:history`,
  `check:versions`, `check:detail`, `check:export`, `check:point`, `check:masks`, `check:local-detail`,
  `check:object`, `check:people` and `check:shell`.
- The image area, the toolbar, the path field and the export panel stay in `Edit.vue`. They are not
  sections.
- `src/components/*`, `Develop.vue`, `BulkLut.vue`, `App.vue`, `frontend/shared`, any Rust, and `docs/` other
  than this file's Report.
- **No fixes.** The clipped controls, the spin arrows and the rest are UI-2's. If you notice something, put
  it in the Report.

### Acceptance

- [ ] Thirteen components in `src/views/edit/` (or fewer, each one missing explained), plus `context.ts` and,
      if needed, `shared.css`.
- [ ] `Edit.vue`'s template is under 1,300 lines. Its script is unchanged apart from the `provide` and the
      imports.
- [ ] Every gate passes with its current counts and messages; `check:shell` included.
- [ ] **The architect's own check:** screenshots of Edit with every section expanded at 1440×900, from the
      whole-app harness on `main` and on `ui-4`, are pixel-identical.

### Gates

The gate list in `CLAUDE.md` in full, plus `npm --prefix frontend/rollchrome run check:shell`.

### How to work: every step costs money

1. **Run every command in the foreground and wait for it to return.** Never start a command in the
   background and check on it, and never loop or sleep waiting for one.
2. **While working, run only** `typecheck` and `check:edit` after each section, plus `check:masks` after
   Masks, `check:colour` after HSL and Colour Grading, `check:history` after History and Snapshots,
   `check:versions` after Versions, and `check:detail` after Detail.
3. **Run the full list under Gates once, at the end.**
4. **Read `Edit.vue` in ranges**, the section you are moving and the style rules it uses. Use `grep` to find
   where a class is styled and which names a template uses. Do not re-read a range to confirm an edit.
5. **Do not change approach silently.** If the same failure survives two attempts, stop on that section,
   say what you tried in the Report, and move on.

### Report back

Under `## Report` in this file:
- what was done, section by section, with its commit;
- **every** deviation, with the reason ("None" only if true);
- every rule kept in two places;
- every gate's actual output: counts, not "passed";
- `Edit.vue`'s line counts (script, template, style) before and after;
- anything unverified.

Commit on `ui-4` with imperative subjects under 72 characters and no model name anywhere. Do not merge.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
