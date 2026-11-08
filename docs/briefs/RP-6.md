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

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
