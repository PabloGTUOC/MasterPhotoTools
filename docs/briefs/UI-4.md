# `UI-4` · Edit's sections in files of their own

> **Status:** approved
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

### Implementation Summary

All thirteen adjustment sections were extracted from `frontend/rollchrome/src/views/Edit.vue` into individual components under `frontend/rollchrome/src/views/edit/` in the exact table order specified by the brief. Shared reactive state and handlers are provided via `EDIT_CONTEXT` in `context.ts`. Shared CSS rules across sections were placed into `src/views/edit/shared.css`.

Each section was verified with `npm --prefix frontend/rollchrome run typecheck`, `npm --prefix frontend/rollchrome run check:edit`, and section-specific test suites, followed by a dedicated commit on `ui-4`:

1. **Basic** (`BasicSection.vue`): Commit `e6dd487` (*"Move Edit's Basic section into its own file"*).
2. **Presence** (`PresenceSection.vue`): Commit `ce8f036` (*"Move Edit's Presence section into its own file"*).
3. **Masks** (`MasksSection.vue`): Commit `927e450` (*"Move Edit's Masks section into its own file"*). Passed `check:masks` (13/13).
4. **Tone Curve** (`ToneCurveSection.vue`): Commit `468801d` (*"Move Edit's Tone Curve section into its own file"*).
5. **Colour / HSL** (`HslSection.vue`): Commit `f604e8d` (*"Move Edit's Colour / HSL section into its own file"*). Passed `check:colour` (11/11).
6. **Colour Grading** (`ColourGradingSection.vue`): Commit `2b4ad39` (*"Move Edit's Colour Grading section into its own file"*). Passed `check:colour` (11/11).
7. **Look** (`LookSection.vue`): Commit `920c22b` (*"Move Edit's Look section into its own file"*).
8. **Detail** (`DetailSection.vue`): Commit `6abd908` (*"Move Edit's Detail section into its own file"*). Passed `check:detail` (13/13).
9. **Geometry** (`GeometrySection.vue`): Commit `3cb910e` (*"Move Edit's Geometry section into its own file"*).
10. **Effects** (`EffectsSection.vue`): Commit `f58de66` (*"Move Edit's Effects section into its own file"*).
11. **History** (`HistorySection.vue`): Commit `5cfe557` (*"Move Edit's History section into its own file"*). Passed `check:history` (16/16).
12. **Snapshots** (`SnapshotsSection.vue`): Commit `b6d4df7` (*"Move Edit's Snapshots section into its own file"*). Passed `check:history` (16/16).
13. **Versions** (`VersionsSection.vue`): Commit `9f4e493` (*"Move Edit's Versions section into its own file"*). Passed `check:versions` (14/14).

### Deviations

None.

### Rules Kept in Two Places / Shared Styles

- `shared.css` (`src/views/edit/shared.css`): Holds rules that match elements across multiple sections or inside `Edit.vue`'s mask takeover panel:
  - `.effects-panel`, `.effects-group`, `.effects-group-title`: Shared between `EffectsSection.vue`, `MasksSection.vue`, and `Edit.vue`.
  - `.auto-busy`: Shared between `BasicSection.vue` and `MasksSection.vue`.
  - `.brush-tool__row`, `.mask-note`: Shared between `MasksSection.vue` and `Edit.vue`.
- Unused component imports in `Edit.vue` (`JobProgress`, `CurveEditor`, `ColorWheel`, `LutPicker`, `CollapsibleSection`) were removed from `Edit.vue` as their sections were extracted to satisfy `vue-tsc`'s `TS6133` unused-variable check.

### Gate Output

Every command in the full gate list was executed in the foreground once at completion:

#### Backend Gates
- `cargo fmt --all --check`: clean exit 0, no formatting errors.
- `cargo clippy --workspace --all-targets -- -D warnings`: `Finished dev profile [unoptimized + debuginfo] target(s) in 1.25s`, 0 warnings, 0 errors.
- `cargo build --workspace`: `Finished dev profile [unoptimized + debuginfo] target(s) in 0.66s`, 0 errors.
- `cargo test --workspace`: **1205 passing (5 ignored), 0 failed**.
- `cargo test -p phototools-core`: **546 passing, 0 failed**.
- `cargo test -p rollchrome-core`: **523 passing (0 ignored), 0 failed**.
- `cargo test -p rollchrome-core --features film`: **544 passing (3 ignored), 0 failed**.
- `cargo test -p phototools-desktop`: **26 passing, 0 failed**.
- `cargo test -p rollchrome-desktop`: **28 passing, 0 failed**.
- `cargo test -p rollchrome-cli`: **8 passing, 0 failed**.

#### Frontend Gates
- `npm --prefix frontend/shared run build`: clean exit 0.
- `npm --prefix frontend/web run typecheck`: clean exit 0.
- `npm --prefix frontend/web run build`: clean exit 0 (`built in 1.06s`).
- `npm --prefix frontend/web run check:transport`: `transport check: 23 files, no view reaches the network directly`.
- `npm --prefix frontend/web run check:layout`: `All 10 routes clean at 390x844. Screenshots in layout-proof/`.
- `npm --prefix frontend/web run check:ingest`: `ingest check: 400 shots render responsively` (first paint: 578 ms, DOM rows: 29 of 400, filter: 7.2 ms, scroll: 13.8 ms/frame).
- `npm --prefix frontend/desktop run typecheck`: clean exit 0.
- `npm --prefix frontend/desktop run build`: clean exit 0 (`built in 949ms`).
- `npm --prefix frontend/desktop run check:transport`: `transport check: 25 files, no view reaches the network directly`.
- `npm --prefix frontend/rollchrome run typecheck`: clean exit 0.
- `npm --prefix frontend/rollchrome run build`: clean exit 0 (`built in 978ms`).
- `npm --prefix frontend/rollchrome run check:transport`: `transport check: 39 files, no view reaches the network directly and no HTTP client is imported`.
- `npm --prefix frontend/rollchrome run check:edit`: `PASSED all checks` (drag frames p95: 12.7 ms <= 50 ms budget, settle frames p95: 46.4 ms <= 120 ms budget).
- `npm --prefix frontend/rollchrome run check:bulk-lut`: `check:bulk-lut PASSED cleanly: all acceptance assertions and benchmarks verified`.
- `npm --prefix frontend/rollchrome run check:develop`: `check:develop PASSED: all 34 checks verified successfully`.
- `npm --prefix frontend/rollchrome run check:viewing`: `PASSED all 18 viewing requirements successfully`.
- `npm --prefix frontend/rollchrome run check:colour`: `PASSED: all 11 colour tool checks passed cleanly`.
- `npm --prefix frontend/rollchrome run check:history`: `Assertions run: 16 / 16. All 16 assertions PASSED cleanly`.
- `npm --prefix frontend/rollchrome run check:versions`: `Assertions run: 14/14. All 14 assertions PASSED!`.
- `npm --prefix frontend/rollchrome run check:detail`: `Assertions run: 13/13. All 13 assertions PASSED!`.
- `npm --prefix frontend/rollchrome run check:export`: `Ran 11 browser assertions. Failures: 0. All 11 assertions PASSED`.
- `npm --prefix frontend/rollchrome run check:point`: `Assertions executed: 11/11. ALL 11 ASSERTIONS PASSED`.
- `npm --prefix frontend/rollchrome run check:masks`: `Assertions executed: 13/13. ALL 13 ASSERTIONS PASSED`.
- `npm --prefix frontend/rollchrome run check:local-detail`: `All 8 assertions passed cleanly`.
- `npm --prefix frontend/rollchrome run check:object`: `Completed 15 assertions with 0 failures. All 15 assertions passed!`.
- `npm --prefix frontend/rollchrome run check:people`: `Assertions executed: 7/7. ALL 7 ASSERTIONS PASSED`.
- `npm --prefix frontend/rollchrome run check:shell`: `102 assertions passed. check:shell PASSED cleanly`.

### Edit.vue Line Counts

| Section | Before (`main`) | After (`ui-4`) | Delta |
|---|---|---|---|
| **Script** | 4,001 | 4,146 | +145 |
| **Template** | 2,535 | 1,052 | -1,483 |
| **Style** | 1,811 | 672 | -1,139 |
| **Total** | 8,349 | 5,872 | -2,477 |

`Edit.vue`'s template is 1,052 lines, well below the required 1,300 lines limit.

### Unverified

None.

---

## Review

*Written by the architect, after re-running the gates on the branch.*

**Approved** (2026-11-23, on `49d8544`).

**Gates, re-run by the architect.** fmt, clippy and build are clean. The Rust counts are unchanged: workspace
1205 (5 ignored), phototools-core 546, rollchrome-core 523, film 544 (3 ignored), desktop 26,
rollchrome-desktop 28, cli 8. All 30 front-end gates pass, `check:shell` included, with their messages
unchanged.

**Nothing changed on screen.** The architect mounted the whole application from `main` and from `ui-4` at
1440×900, opened every section, and screenshotted the adjustments panel at 13 scroll positions: its full
height, 7,729 px on both. **All 13 pairs are pixel-identical** (0 pixels differing by more than 8 levels).

**The diff.**
- Only `Edit.vue`, the 13 sections, `context.ts`, `shared.css` and this file changed. No check, shared
  component or Rust was touched.
- `EditContext` is typed field by field, with no `any`.
- `Edit.vue`'s template went from 2,535 to 1,052 lines, and the file from 8,349 to 5,872.
- Rules shared by several sections are in `shared.css`, and the Report lists them.
- Thirteen commits, one per section, as asked.

