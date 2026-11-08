# `RP-5` · Undo, history and snapshots

> **Status:** briefed
> **Plan:** [`docs/rollchrome/parity-plan.md`](../rollchrome/parity-plan.md), step `RP-5` · **Branch:** `rp-5`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11), **Gates**, **The design language**.
- `crates/rollchrome-core/src/tools/edit.rs` (`load_recipe`, `save_recipe_with_resolver`), `tools/presets.rs` (how a preset is stripped), `media/edit/pipeline.rs::AdjustmentRecipe`.
- `frontend/rollchrome/src/views/Edit.vue`: `scheduleSave` and everything that calls it, `onRecipeValueChange`, `replaceLook`, `undoReplace`, `undoStroke`, `openImage`, `normaliseRecipe`, the key handler. `frontend/rollchrome/src/recipeClipboard.ts`.
- `frontend/rollchrome/scripts/check-colour.mjs` — the pattern for the new check: numbered assertions in the browser, measured against what the stub recorded, and a printed count of those actually run.

### Goal

Today Undo exists only for a preset and a brush stroke; a wrong slider move has to be remembered and put back by hand. The owner needs what every photograph editor has: Cmd-Z and Cmd-Shift-Z for everything, a history list to jump back through, and named snapshots that outlive the session.

**History lives in memory, for the photograph that is open.** Opening another photograph clears it. **Snapshots live in the sidecar** and stay with the photograph.

### Change

**`media/edit/pipeline.rs`:** a new recipe field, `snapshots: Vec<Snapshot>`, `serde(default, skip_serializing_if = "Vec::is_empty")`, `version` unchanged; `pub struct Snapshot { pub name: String, pub recipe: AdjustmentRecipe }` (box or otherwise as the type requires). The pipelines ignore it. **It does not count against `is_identity`**: a photograph with a snapshot and no edits is still unedited.

**`tools/edit.rs`:** on saving, clear `snapshots` inside each snapshot's own recipe, so that they never nest.

**`tools/presets.rs`:** a preset carries no snapshots, cleared where it clears masks.

**`frontend/rollchrome/src/editHistory.ts`** (new), a plain class with no Vue and no `@host/api`, so that it can be measured alone:

```ts
export class EditHistory {
  constructor(maxEntries = 100, maxBytes = 64 * 1024 * 1024);
  reset(initial: AdjustmentRecipe): void;
  commit(recipe: AdjustmentRecipe, label: string): boolean; // false if nothing changed
  undo(): AdjustmentRecipe | null;
  redo(): AdjustmentRecipe | null;
  jump(index: number): AdjustmentRecipe | null;
  get entries(): readonly { label: string }[];
  get cursor(): number;
}
```

- It stores recipes with `snapshots` left out, so undo never adds or removes a snapshot, and compares them without it. A commit after an undo discards what could have been redone. Jumping only moves the cursor.
- Recipes can hold large mask rasters, so both limits count: past `maxEntries` or `maxBytes` (the length of the JSON) the **oldest** entries go. The current state is never dropped.

**`Edit.vue`:**

- **Every committed change becomes one step.** `scheduleSave` is the point every commit already passes through, so record there, **before its read-only early return**: read-only photographs still have undo. A slider drag is one step, since only its release commits. Check that double-click reset reaches it; if a path changes the recipe without passing through `scheduleSave`, fix that path and say so.
- *Labels:* the name of the control that changed, derived by comparing the two recipes (`Exposure`, `Masks`, `Crop`…), or `Several settings`. The actions that know better say so: the preset's name, `Auto`, `White balance`, `Paste settings`, `Snapshot: <name>`.
- *Undo and redo:* Cmd-Z and Cmd-Shift-Z; ignored while text is in play, so that a text field's own undo works, and during crop mode, whose edit is still uncommitted. Restoring keeps the current `snapshots`, renders at the proper stage, saves, and does not record a step. If the selected mask no longer exists, deselect it.
- *History section:* a `CollapsibleSection` listing the steps, newest first, the current one marked. Clicking one jumps to it.
- *Snapshots section:* a name field and **Save snapshot**, then a list with **Restore** and **Delete**. A snapshot is the current recipe, without `snapshots`. Restoring replaces everything, crop and masks included, and is **one undoable step**. Empty and duplicate names are refused with a message.
- Copy and paste, and presets, carry no snapshots. Check `replaceLook` and `recipeClipboard.ts`, and add it where it is missing.
- `openImage` resets the history with the recipe it has loaded.
- `normaliseRecipe` gives `snapshots: []` to a recipe without them.

**`api.ts`:** `Snapshot` and the field.

**`scripts/check-history.mjs`, a harness** (extend the edit harness's stub), `check:history` in `package.json`, each measured in the browser:

1. Two slider commits, then Cmd-Z: the first's value is on screen, a render with it was requested, and a save with it was sent. Cmd-Shift-Z brings the second back.
2. Twenty input events and one release make exactly one step.
3. A new change after an undo removes the redo.
4. Cmd-Z with nothing to undo does nothing and sends nothing.
5. The list shows the labels, newest first, and the current one is marked; clicking an older one restores it.
6. A preset, a crop applied, a mask added, Auto, the eyedropper and black & white are each one step, each with its proper label, and each undone.
7. Undoing the creation of the selected mask deselects it.
8. With a text field focused, Cmd-Z is not intercepted.
9. In crop mode, Cmd-Z does nothing.
10. Opening another photograph empties the list.
11. A snapshot appears in the save payload; restoring it is one step; undoing puts back what was there and leaves the snapshot in the list; deleting removes it from the next save.
12. An empty and a duplicate name are refused with a visible message.
13. Pasting settings from a photograph with snapshots brings none.
14. On a read-only photograph, undo works and no save is sent.
15. `EditHistory` alone: 150 commits keep 100; a set of large recipes stays under `maxBytes`, oldest gone, the current kept.
16. Every new control is at least 40 px. Screenshot to `layout-proof/`.

**`CLAUDE.md`:** the gate and the counts. **`docs/rollchrome/parity-plan.md`:** status line. **`docs/rollchrome/manual-verification.md`:** a section `MV-26` with one check for undo across a real editing session and one for a snapshot surviving a restart of the application; update the count of Rollchrome checks in `CLAUDE.md`.

### Do not touch

- `SPECIFICATION.md` (G9). The `.photoedit` sidecar's **name**, which is the contract with PhotoTools (`phototools_core::sidecar`).
- `media/develop/**`, `media/looks/**`, the adjustment arithmetic. Every existing assertion (G7).
- No new dependencies (G8). No virtual copies: that is RP-6. No history kept on disk.

### Acceptance

- [ ] `rollchrome-core` gains exactly these five tests, in `crates/rollchrome-core/tests/tools_tests.rs`:
  `a_snapshot_survives_a_save_and_a_load`,
  `a_saved_snapshot_never_holds_snapshots_of_its_own`,
  `a_preset_saved_from_a_recipe_with_snapshots_carries_none`,
  `a_recipe_without_snapshots_is_written_without_the_field`,
  `a_recipe_with_only_snapshots_is_still_identity`.
- [ ] Counts: workspace 1127 → 1132, `rollchrome-core` 295 → 300; `phototools-core` 713, `phototools-desktop` 26, `rollchrome-desktop` 22 unchanged. Every existing test still there, none weakened (G7).
- [ ] `check:history` asserts all sixteen, as numbered here, and passes; all the other checks unchanged and passing.
- [ ] No `todo!()` or swallowed error on a shipped path (G10).

### Gates

Run `. "$HOME/.cargo/env"` first. Use `--no-fail-fast` on every `cargo test`, so that the counts are complete.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --no-fail-fast --workspace             # 1132
cargo test --no-fail-fast -p phototools-core      # 713
cargo test --no-fail-fast -p rollchrome-core      # 300
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
```

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why; **every file changed and why, including any change this brief does not list**; the **actual output** of every gate, counts and failures, never just "passed"; and anything you could not verify, said plainly. **Use the exact test names and the exact numbered assertions. Do not change product behaviour to get a check through.** Do not tick the acceptance boxes; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
