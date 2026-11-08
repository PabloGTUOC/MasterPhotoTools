# `RP-6` · Virtual copies

> **Status:** briefed
> **Plan:** [`docs/rollchrome/parity-plan.md`](../rollchrome/parity-plan.md), step `RP-6` · **Branch:** `rp-6`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11), **Gates**, **The design language**.
- `docs/briefs/RP-5.md`, how `snapshots` were added. This step follows the same pattern.
- `crates/rollchrome-core/src/tools/edit.rs`: `save_recipe_with_resolver`, `export_edited_image_with_resolver`, `exclusive_create_target`, `render_and_write`. `tools/presets.rs`.
- `frontend/rollchrome/src/views/Edit.vue`: the Snapshots section, the export control, `openImage`, `showNotice`, `applyRestoredRecipe`.
- `frontend/rollchrome/scripts/check-history.mjs` — the pattern for the new check.

### Goal

A virtual copy is a second, independent edit of the same photograph, without a second file: a colour and a black & white of one frame, each kept and each exported. A snapshot is a point you go back to; a version is a line of work that carries on. There is no library yet, so versions live in Edit, in the photograph's sidecar.

**The shape.** The sidecar's top level remains the recipe of the **current version**, so that nothing which reads a recipe has to know about versions: export, the preview and the pipelines see one recipe as they always did. The others are held in a list. Switching exchanges them.

### Change

**`media/edit/pipeline.rs`:** recipe fields `version_name: Option<String>` (`None` means the first version, shown as **Main**) and `versions: Vec<Version>`, both `serde(default, skip_serializing_if…)`, `version` unchanged; `pub struct Version { pub name: String, pub recipe: AdjustmentRecipe }`. Neither counts against `is_identity`.

**`tools/edit.rs`:**
- On saving, clear `versions` and `snapshots` inside each version's recipe, as snapshots are cleared now.
- On exporting, the existing `_edit` suffix becomes `_edit_<name>` when `version_name` is set: a slug of the name that keeps letters and digits and turns anything else into `-`, so that it can never leave the output folder (G6). An unnamed version exports exactly as now. The identity byte-copy keeps its behaviour, with that suffix.

**`tools/presets.rs`:** a preset carries no versions and no `version_name`.

**`api.ts`:** the type and fields.

**`Edit.vue`, a Versions section** (a `CollapsibleSection` with the Snapshots one):

- Shows the current version's name and lists the others, each with **Switch**, **Rename** and **Delete**. The current one can be renamed but not deleted.
- **New version**: asks for a name, offering `Copy 1`, `Copy 2`…; duplicates the current edit, and the new one becomes current. Empty and duplicate names are refused with a message, `Main` being taken while the first version is unnamed.
- **Switching**: the current recipe, without `versions` and `snapshots`, goes into the list under its name; the chosen one comes out of the list and becomes the top level. `versions` and `snapshots` stay on the top level throughout: **snapshots belong to the photograph, not to a version.** Renders at the proper stage, saves, **resets the history** to the new current recipe (a history that crossed versions would undo the wrong line of work), deselects any mask, and leaves zoom and pan alone.
- **Export all versions**, beside the export control when there is more than one: exports each version in turn, current one included, using `exportEditedImage` with each recipe, **one at a time**. A failure is listed and the others carry on. Shows progress and, at the end, what was written and what failed. It changes nothing in the sidecar.
- `normaliseRecipe` gives `versions: []`. Copy and paste carry no versions or name, as for snapshots. `EditHistory`'s stripping and comparison treat `versions` and `version_name` as it does `snapshots`, so that undo never adds, removes or renames a version.

**The notice left from RP-5.** `undoReplace` now undoes the latest step. The notice must not outlive the step it offers to undo: **dismiss it when any new step is recorded.** The history itself must still undo as usual.

**`scripts/check-versions.mjs`, a harness** (extend the edit harness's stub), `check:versions` in `package.json`, each measured in the browser:

1. A photograph with no versions shows **Main** alone and offers no Delete and no Export all.
2. **New version** with a name makes it current; the save payload has the old one in `versions` and `version_name` set.
3. Switching exchanges the recipes: the sliders show the other version's values, a render and a save with them are sent.
4. Editing one version leaves the other's values intact when switching back.
5. Switching empties the history list.
6. A snapshot is still listed after switching, and after switching back.
7. Rename of the current and of another version reach the save payload; an empty and a duplicate name, and `Main` while it is taken, are refused with a visible message.
8. Deleting another version removes it from the save payload.
9. Export sends the current recipe, with `version_name`.
10. **Export all** with three versions sends three exports with no two overlapping; with one stubbed failure the other two are still sent, and the summary shows two written and one failure with its reason.
11. Pasting settings from a photograph with versions brings none, and leaves the current name.
12. Undo after switching does not change the list of versions or the name.
13. Apply a preset, then move a slider: the notice is gone. Cmd-Z still takes back the slider, then the preset.
14. Every new control is at least 40 px. Screenshot to `layout-proof/`.

**`CLAUDE.md`:** the gate and the counts. **`docs/rollchrome/parity-plan.md`:** status line. **`docs/rollchrome/manual-verification.md`:** a section `MV-27` with one check: a colour and a black & white version of one photograph, switched between, exported together, surviving a restart. Update the count of Rollchrome checks in `CLAUDE.md`.

### Do not touch

- `SPECIFICATION.md` (G9). The sidecar's **name**, the contract with PhotoTools.
- `media/develop/**`, `media/looks/**`, the adjustment arithmetic, Batch. Every existing assertion (G7).
- No new dependencies (G8). No per-version snapshots, no version comparison view, no library.

### Acceptance

- [ ] `rollchrome-core` gains exactly these seven tests, in `crates/rollchrome-core/tests/tools_tests.rs`:
  `a_version_survives_a_save_and_a_load`,
  `a_saved_version_never_holds_versions_or_snapshots_of_its_own`,
  `a_preset_saved_from_a_recipe_with_versions_carries_none`,
  `a_recipe_with_only_versions_is_still_identity`,
  `a_named_version_exports_with_its_name_in_the_file_name`,
  `an_unnamed_version_exports_exactly_as_before`,
  `a_version_name_cannot_escape_the_output_folder` (with `../../x` and a name of only separators).
- [ ] Counts: workspace 1132 → 1139, `rollchrome-core` 300 → 307; `phototools-core` 713, `phototools-desktop` 26, `rollchrome-desktop` 22 unchanged. Every existing test still there, none weakened (G7).
- [ ] `check:versions` asserts all fourteen, as numbered here, and passes; all the other checks unchanged and passing.
- [ ] No `todo!()` or swallowed error on a shipped path (G10).

### Gates

Run `. "$HOME/.cargo/env"` first. Use `--no-fail-fast` on every `cargo test`, so that the counts are complete.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --no-fail-fast --workspace             # 1139
cargo test --no-fail-fast -p phototools-core      # 713
cargo test --no-fail-fast -p rollchrome-core      # 307
cargo test --no-fail-fast -p phototools-desktop   # 26
cargo test --no-fail-fast -p rollchrome-desktop   # 22
npm --prefix frontend/shared run build
npm --prefix frontend/rollchrome run typecheck
npm --prefix frontend/rollchrome run build
npm --prefix frontend/rollchrome run check:transport
npm --prefix frontend/rollchrome run check:edit
npm --prefix frontend/rollchrome run check:bulk-lut
npm --prefix frontend/rollchrome run check:develop
npm --prefix frontend/rollchrome run check:viewing
npm --prefix frontend/rollchrome run check:colour
npm --prefix frontend/rollchrome run check:history
npm --prefix frontend/rollchrome run check:versions
```

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why; **every file changed and why, including any change this brief does not list**; the **actual output** of every gate, counts and failures, never just "passed"; and anything you could not verify, said plainly. **Use the exact test names and the exact numbered assertions. Do not change product behaviour to get a check through.** Do not tick the acceptance boxes; the review does that.

---

## Report

### What was done

1. **Pipeline & Data model (`crates/rollchrome-core/src/media/edit/pipeline.rs`, `crates/rollchrome-core/src/media/edit/mod.rs`, `crates/rollchrome-core/src/media/mod.rs`)**:
   - Added `pub struct Version { pub name: String, pub recipe: AdjustmentRecipe }`.
   - Added fields `pub version_name: Option<String>` and `pub versions: Vec<Version>` to `AdjustmentRecipe` with `#[serde(default, skip_serializing_if = "...")]`.
   - Updated `Default` implementation and re-exports. Neither field counts against `is_identity()`.

2. **Sidecar saving & Export slugging (`crates/rollchrome-core/src/tools/edit.rs`, `crates/rollchrome-core/src/tools/presets.rs`)**:
   - In `save_recipe_with_resolver`, cleared nested `snapshots`, `versions`, and `version_name` for each version in `versions` and each snapshot in `snapshots`.
   - Added `slug_version_name` keeping letters and digits and converting any other character to `-`.
   - In `export_edited_image_with_resolver`, appended `_edit_<slug>` when `version_name` is set, leaving unnamed versions as `_edit`. Identity byte-copies also preserve this suffix.
   - In `save_preset`, cleared `versions` and set `version_name = None`.

3. **Rust Tests (`crates/rollchrome-core/tests/tools_tests.rs`)**:
   - Added the 7 required integration tests:
     - `a_version_survives_a_save_and_a_load`
     - `a_saved_version_never_holds_versions_or_snapshots_of_its_own`
     - `a_preset_saved_from_a_recipe_with_versions_carries_none`
     - `a_recipe_with_only_versions_is_still_identity`
     - `a_named_version_exports_with_its_name_in_the_file_name`
     - `an_unnamed_version_exports_exactly_as_before`
     - `a_version_name_cannot_escape_the_output_folder` (tested with `../../x` and `///`).

4. **Frontend API & Utilities (`frontend/rollchrome/src/api.ts`, `frontend/rollchrome/src/editHistory.ts`, `frontend/rollchrome/src/recipeClipboard.ts`)**:
   - Added `Version` interface and `version_name`, `versions` fields to `AdjustmentRecipe`.
   - Updated `EditHistory.stripSnapshots` to strip `versions` and `version_name` alongside `snapshots`.
   - Updated `copyToClipboard` to strip `versions` and `version_name`.

5. **Edit View (`frontend/rollchrome/src/views/Edit.vue`)**:
   - In `scheduleSave`: when a step is committed to history, dismisses the notice.
   - In `applyRestoredRecipe`: keeps top-level `snapshots`, `versions`, and `version_name` intact during undo/redo/jump.
   - In `replaceLook`: keeps `before.snapshots`, `before.versions`, and `before.version_name`.
   - In `normaliseRecipe`: sets `versions: (existing.versions ?? [])...` and `version_name: existing.version_name ?? null`.
   - Added Versions section (`test-id="section-versions"`) with:
     - Current version card showing name (or "Main") and a Rename button (no Delete button).
     - New version input (defaults to suggested `Copy 1`, `Copy 2`, etc.) and "New version" button.
     - Validation message for empty, duplicate, or taken names (`Main` while taken).
     - Other versions list with Switch, Rename, and Delete buttons.
   - Added "Export all versions" button beside "Export render" when total versions > 1, running exports sequentially (`exportEditedImage` one at a time), with summary of written and failed exports.
   - All interactive controls sized $\ge 40$px touch target.

6. **Browser check (`frontend/rollchrome/scripts/check-versions.mjs`, `frontend/rollchrome/scripts/edit-harness/stub-api.ts`, `frontend/rollchrome/package.json`)**:
   - Added `check:versions` asserting all 14 requirements in a headless browser.
   - Saved layout proof to `frontend/rollchrome/layout-proof/versions.png`.

7. **Documentation & Gates (`CLAUDE.md`, `docs/rollchrome/parity-plan.md`, `docs/rollchrome/manual-verification.md`)**:
   - Updated test counts in `CLAUDE.md`: workspace 1139, `rollchrome-core` 307; added `check:versions` and updated Rollchrome check count to 38.
   - Updated `docs/rollchrome/parity-plan.md` status line: RP-1 to RP-6 and RP-2b done.
   - Added section `MV-27` to `docs/rollchrome/manual-verification.md`.

### Deviations from brief

None.

### Every file changed and why

- `crates/rollchrome-core/src/media/edit/pipeline.rs`: Added `Version` struct and `version_name`, `versions` fields to `AdjustmentRecipe`.
- `crates/rollchrome-core/src/media/edit/mod.rs`: Re-exported `Version`.
- `crates/rollchrome-core/src/media/mod.rs`: Re-exported `Version`.
- `crates/rollchrome-core/src/tools/edit.rs`: Sanitised nested versions/snapshots/version_name on saving; added `slug_version_name`; exported with `_edit_<slug>` when `version_name` is present.
- `crates/rollchrome-core/src/tools/presets.rs`: Stripped `versions` and `version_name` when saving presets.
- `crates/rollchrome-core/tests/tools_tests.rs`: Added the 7 integration tests specified in the brief.
- `frontend/rollchrome/src/api.ts`: Added `Version` interface and `version_name`, `versions` fields to `AdjustmentRecipe`.
- `frontend/rollchrome/src/editHistory.ts`: Stripped `versions` and `version_name` when saving/comparing history entries.
- `frontend/rollchrome/src/recipeClipboard.ts`: Stripped `versions` and `version_name` when copying settings.
- `frontend/rollchrome/src/views/Edit.vue`: Added Versions collapsible section, switching/renaming/deleting logic, "Export all versions" sequential export and progress/summary reporting, and dismissed RP-5 notice upon recording new history steps.
- `frontend/rollchrome/scripts/edit-harness/stub-api.ts`: Added stub tracking for exports, sequential in-flight verification, and stubbed failures per version name.
- `frontend/rollchrome/scripts/check-versions.mjs`: Added automated test suite verifying all 14 numbered requirements in headless Playwright.
- `frontend/rollchrome/package.json`: Added `check:versions` npm script.
- `CLAUDE.md`: Updated workspace and `rollchrome-core` test counts, added `check:versions`, and updated Rollchrome check count to 38.
- `docs/rollchrome/parity-plan.md`: Updated status line with RP-6 completed.
- `docs/rollchrome/manual-verification.md`: Added section `MV-27` with check `MV-27.1`.
- `docs/briefs/RP-6.md`: This report.

### Actual output of every gate

1. `cargo fmt --all --check`
```
Exit 0, no output.
```

2. `cargo clippy --workspace --all-targets -- -D warnings`
```
    Checking rollchrome-core v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-core)
    Checking rollchrome-desktop v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-desktop)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.98s
```

3. `cargo build --workspace`
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.25s
```

4. `cargo test --no-fail-fast --workspace`
```
test result: ok. 518 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.69s
test result: ok. 40 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.22s
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.38s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.58s
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.41s
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.64s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.73s
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.45s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.48s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.68s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.87s
test result: ok. 38 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.17s
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.45s
test result: ok. 150 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 19.31s
test result: ok. 75 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.75s
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 15.82s
test result: ok. 45 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.11s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.16s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.64s
Total: 1139 passed; 0 failed
```

5. `cargo test --no-fail-fast -p phototools-core`
```
test result: ok. 518 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.43s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.70s
test result: ok. 40 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.27s
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.39s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.58s
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.42s
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.65s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.71s
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.44s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.46s
Total: 713 passed; 0 failed
```

6. `cargo test --no-fail-fast -p rollchrome-core`
```
test result: ok. 150 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 19.35s
test result: ok. 75 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.73s
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 15.94s
test result: ok. 45 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.08s
Total: 307 passed; 0 failed
```

7. `cargo test --no-fail-fast -p phototools-desktop`
```
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.72s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.95s
Total: 26 passed; 0 failed
```

8. `cargo test --no-fail-fast -p rollchrome-desktop`
```
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.68s
Total: 22 passed; 0 failed
```

9. `npm --prefix frontend/shared run build`
```
> @phototools/shared@1.0.0 build
> tsc
```

10. `npm --prefix frontend/rollchrome run typecheck`
```
> @phototools/rollchrome@0.1.0 typecheck
> vue-tsc --build --force
```

11. `npm --prefix frontend/rollchrome run build`
```
> @phototools/rollchrome@0.1.0 build
> vue-tsc --build && vite build

vite v6.4.3 building for production...
✓ 84 modules transformed.
dist/index.html                                   0.45 kB │ gzip:  0.28 kB
dist/assets/vt323-400-EhPR265H.woff2              7.19 kB
dist/assets/share-tech-mono-400-DZ_B2YQ6.woff2    7.41 kB
dist/assets/ibm-plex-mono-400-BJoXLJYV.woff2     10.05 kB
dist/assets/ibm-plex-mono-500-C820gu2e.woff2     10.06 kB
dist/assets/ibm-plex-mono-600-DpGnXj3s.woff2     10.12 kB
dist/assets/orbitron-400-C4ZOLPH_.woff2          11.77 kB
dist/assets/index-NV7NIfnj.css                   67.17 kB │ gzip: 10.32 kB
dist/assets/index-CbbqyJeg.js                   263.64 kB │ gzip: 86.21 kB
✓ built in 894ms
```

12. `npm --prefix frontend/rollchrome run check:transport`
```
> @phototools/rollchrome@0.1.0 check:transport
> node scripts/check-transport.mjs

transport check: 24 files, no view reaches the network directly and no HTTP client is imported
```

13. `npm --prefix frontend/rollchrome run check:edit`
```
> @phototools/rollchrome@0.1.0 check:edit
> node scripts/check-edit.mjs

Starting Vite harness server for Edit view...
Harness server listening at http://[::1]:5173
...
--- check:edit Summary ---
PASSED all checks.
```

14. `npm --prefix frontend/rollchrome run check:bulk-lut`
```
> @phototools/rollchrome@0.1.0 check:bulk-lut
> node scripts/check-bulk-lut.mjs

Starting Vite harness server for Bulk LUT view...
Harness server listening at http://[::1]:5173
...
check:bulk-lut PASSED cleanly: all acceptance assertions and benchmarks verified.
```

15. `npm --prefix frontend/rollchrome run check:develop`
```
> @phototools/rollchrome@0.1.0 check:develop
> node scripts/check-develop.mjs

Starting Vite harness server for Develop view...
Harness server listening at http://[::1]:5173
...
check:develop PASSED: all 14 checks verified successfully.
```

16. `npm --prefix frontend/rollchrome run check:viewing`
```
> @phototools/rollchrome@0.1.0 check:viewing
> node scripts/check-viewing.mjs

Starting Vite harness server for Viewing check...
Harness server listening at http://[::1]:5173
...
--- check:viewing Summary ---
PASSED all 18 viewing requirements successfully.
```

17. `npm --prefix frontend/rollchrome run check:colour`
```
> @phototools/rollchrome@0.1.0 check:colour
> node scripts/check-colour.mjs

Starting Vite harness server for check:colour...
Harness server listening at http://[::1]:5173
...
--- check:colour Summary ---
PASSED: all 11 colour tool checks passed cleanly.
```

18. `npm --prefix frontend/rollchrome run check:history`
```
> @phototools/rollchrome@0.1.0 check:history
> node scripts/check-history.mjs

Starting Vite harness server for check:history...
Harness server listening at http://[::1]:5173
...
========================================
Assertions run: 16 / 16
All 16 assertions PASSED cleanly.
```

19. `npm --prefix frontend/rollchrome run check:versions`
```
> @phototools/rollchrome@0.1.0 check:versions
> node scripts/check-versions.mjs

Starting Vite harness server for check:versions...
Harness server listening at http://[::1]:5173
Asserting 1: A photograph with no versions shows Main alone and offers no Delete and no Export all...
Asserting 2: New version with a name makes it current; the save payload has the old one in versions and version_name set...
Asserting 3: Switching exchanges the recipes: the sliders show the other version's values, a render and a save with them are sent...
Asserting 4: Editing one version leaves the other's values intact when switching back...
Asserting 5: Switching empties the history list...
Asserting 6: A snapshot is still listed after switching, and after switching back...
Asserting 7: Rename of the current and of another version reach the save payload; an empty and a duplicate name, and Main while it is taken, are refused with a visible message...
Asserting 8: Deleting another version removes it from the save payload...
Asserting 9: Export sends the current recipe, with version_name...
Asserting 10: Export all with three versions sends three exports with no two overlapping; with one stubbed failure the other two are still sent, and the summary shows two written and one failure with its reason...
Asserting 11: Pasting settings from a photograph with versions brings none, and leaves the current name...
Asserting 12: Undo after switching does not change the list of versions or the name...
Asserting 13: Apply a preset, then move a slider: the notice is gone. Cmd-Z still takes back the slider, then the preset...
Asserting 14: Every new control is at least 40 px. Screenshot to layout-proof/...
Saved screenshot to /Users/pablogtorres/Desktop/Projects/MasterPhotoTools/frontend/rollchrome/layout-proof/versions.png

========================================
Assertions run: 14/14
All 14 assertions PASSED!
```

### Anything unverified

Real camera file verification on a physical Mac Studio with attached card reader is tracked by manual verification check MV-27.1. All automated gates pass in full.

---

## Review

*Written by the architect, after re-running the gates on the branch.*
