# `RP-1` · Three tabs and the Develop screen

> **Status:** briefed
> **Plan:** [`docs/rollchrome/parity-plan.md`](../rollchrome/parity-plan.md), step `RP-1` · **Branch:** `rp-1`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11), **Gates**, **The design language**, **The front-end boundary**.
- `docs/rollchrome/parity-plan.md` — *Three tabs*.
- `docs/develop-plan.md` — what Develop does and *Things that will catch you*.
- `crates/rollchrome-core/src/tools/develop.rs`, `tools/develop_folder.rs`, `media/develop/render.rs`,
  `media/looks/builtin.rs`.
- `crates/rollchrome-desktop/src/commands/develop.rs`, and `commands/auto_masks.rs` for an `async`
  command with `spawn_blocking` and an `_impl` for tests; `commands/edit.rs::render_preview` for
  binary responses.
- `frontend/rollchrome/src/views/BulkLut.vue` — **the pattern for this screen**: dry-run lock, job
  progress, refusals shown verbatim. `scripts/check-bulk-lut.mjs` and `scripts/bulk-lut-harness/` —
  the pattern for its check.

### Goal

Rollchrome becomes three tabs: **Develop**, **Edit**, **Batch**. Develop's function exists, with
twelve built-in film looks, and has no screen: `plan_develop` and `apply_develop` are registered and
nothing calls them. This step builds the screen. The owner opens a folder of RAWs, sees them, gives
each photograph or a selection a film look, sees what the look does, and develops them to JPEG or
16-bit TIFF. The look must be seen before the run: choosing a simulation blind is the thing this
screen exists to avoid.

### Change

**`crates/rollchrome-core/src/media/develop/preview.rs`** (new; declare it in `media/develop/mod.rs`):

```rust
pub const PREVIEW_MAX_EDGE: u32 = 1600;
pub struct DevelopPreview { /* a Developed, reduced to fit the long edge */ }
impl DevelopPreview {
    pub fn new(raw: &[u8], exposure_ev: f32, max_edge: u32) -> Result<Self, Error>;
    pub fn from_developed(developed: Developed, max_edge: u32) -> Result<Self, Error>;
    /// Upright, JPEG quality 90, with the look if any. Does not develop the RAW again.
    pub fn render(&self, look: Option<&Look>) -> Result<Rendered, Error>;
}
```

Develop once and keep the reduced 16-bit ProPhoto image, so that changing the look only re-applies
the look. Reduce it while it is still 16 bits. Render through the existing `render`, not a second
colour path. The result must be **upright**: apply `Developed.orientation`, and check how `render`
treats it.

**`crates/rollchrome-core/src/tools/develop.rs`:** extract from `execute_develop` a public
`resolve_look(id: &str, store: &S) -> Result<Look, Error>`, which resolves `builtin:<id>` itself and
everything else through `DevelopStore::look`. `execute_develop` calls it and behaves exactly as now.

**`crates/rollchrome-core/src/media/looks/builtin.rs`:** add a serialisable
`BuiltinLookInfo { id, name, maker }` and `builtin_look_list() -> Vec<BuiltinLookInfo>`. Its `id` is
the full `builtin:<id>` a `DevelopItem.look` takes.

**`crates/rollchrome-core/src/tools/develop_folder.rs`:**
- `raw_names(folder: &Path) -> Result<Vec<String>, Error>`: the folder's RAW files by
  `RAW_EXTENSIONS`, not recursive, sorted by name.
- `thumbnail(path: &Path, max_edge: u32) -> Result<Vec<u8>, Error>`: upright JPEG through
  `phototools_core::media::raw::raw_to_jpeg` and `image_ops::downscale_to_max_edge`.

**`crates/rollchrome-desktop/src/commands/develop.rs`** (transport only, G1), each with an `_impl` for
tests, all registered in `main.rs`:

| Command | Returns |
|---|---|
| `list_builtin_looks()` | `Vec<BuiltinLookInfo>` |
| `list_develop_folder(folder)` | `Vec<String>` |
| `develop_thumbnail(folder, name)` | binary JPEG, long edge 360. `async`, `spawn_blocking` |
| `preview_develop(folder, name, look: Option<String>, exposure_ev)` | binary JPEG, as `render_preview`. `async`, `spawn_blocking` |

Every `folder` goes through `resolve_input` (G6). A `name` containing a path separator or equal to
`..` is **refused**.

**`crates/rollchrome-desktop/src/lib.rs`:** `AppState` holds one cached `Arc<DevelopPreview>`, keyed
by resolved path and exposure. **Do not hold the lock while developing.** At most one is kept,
for the same memory reason `active_preview` gives. Add `develop_preview_runs() -> u64`, counting
actual developments, which is how the test asserts the cache.

**`frontend/rollchrome/src/api.ts`:** methods for the four new commands and for `plan_develop` and
`apply_develop` (none exist yet), with types matching the Rust, including `OutputFormat` as its serde
form. Read the derive; do not guess.

**`frontend/rollchrome/src/main.ts` and `App.vue`:** routes `/` Develop, `/edit` Edit, `/batch` Batch.
Navigation labels **Develop** `01`, **Edit** `02`, **Batch** `03`. Search `frontend/rollchrome` for
anything that links to the old paths.

**`frontend/rollchrome/src/views/Develop.vue`** (new):

- **Folder:** `PathField`, then `list_develop_folder`. Empty folder: say so.
- **Photographs:** a grid of thumbnails with the file name and the look assigned to each. Thumbnails
  load **one at a time**, as Batch's previews do; one that fails shows its name, not a broken image.
- **Selection:** click selects one, Cmd-click toggles, Shift-click selects a range, *Select all*.
- **Look:** a picker grouped **None (neutral)**, **Fujifilm** (5), **Ricoh** (7), **My LUTs** (from
  `list_luts`). Choosing a look assigns it to the whole selection, and only the selection.
- **Exposure:** one `AdjustmentSlider`, −4 to +4 EV, per selection, as with the look.
- **Preview:** the most recently clicked photograph at its assigned look and exposure. The first
  look at a photograph takes seconds, so show that it is working. **Latest wins:** a response for
  a choice since replaced is never displayed.
- **Output:** JPEG with quality 1–100, default 95, or TIFF, stating *16-bit ProPhoto RGB*. It goes
  to `<folder>/Developed`, and the screen says so.
- **Run:** Batch's discipline. *Develop* is locked until a dry run (`plan_develop`) exists for the
  exact current items and format; any change invalidates it and the reason is visible. Skips are
  listed with their reasons, including those already developed. Then `apply_develop`, `JobProgress`
  with cancel, and the summary.
- **Tabs:** moving to Edit and back keeps the folder, assignments and selection. Hold them at module
  level, as `recipeClipboard.ts` does. Nothing is written to disk.

Tokens only, no raw hex, every control at least 40 px, radius at most 2 px.

**`frontend/rollchrome/scripts/check-develop.mjs`, `scripts/develop-harness/`** (new), and
`check:develop` in `package.json`, against a stub API, asserting:

1. Six stubbed RAWs give six thumbnails, and no two thumbnail requests overlap.
2. The picker offers None, 5 Fujifilm, 7 Ricoh and the stubbed LUTs.
3. A look on one selected photograph changes that photograph alone; after *Select all* it changes all six.
4. The items sent to `plan_develop` carry the exact `builtin:<id>` strings and exposures.
5. With a slow first preview response and a fast second, the second stays on screen.
6. *Develop* is disabled before the dry run and after any change to a look, an exposure or the format,
   with the reason visible.
7. A stubbed skip is listed with its reason; cancel calls the job cancel; the summary shows its counts.
8. Unmounting and remounting the view keeps folder, assignments and selection.
9. Every control is at least 40 px. Write a screenshot to `layout-proof/`.

**`CLAUDE.md`:** add `check:develop` to the gates, update the counts, and change the `// Edit and Batch
Grade` wording in *Shape* to the three tabs. **`docs/develop-plan.md`:** status line and the screen
section say the screen exists, pointing at this brief. **`docs/rollchrome/parity-plan.md`:** status line.

### Do not touch

- `SPECIFICATION.md` (G9).
- `media/develop/{mod,colour,tone,xtrans}.rs` and `media/looks/{colour,fit}.rs`, and every `.cube`.
  **The looks are fitted to the base tone.**
- `Edit.vue` and `BulkLut.vue`, beyond what the route change forces. Which files Batch and Edit accept
  is RP-2. The `// EDIT` brand text stays.
- No new dependencies (G8). No per-photo persistence, no new look controls: other adjustments belong
  to Edit.

### Acceptance

- [ ] Navigation shows Develop, Edit, Batch; `/` is Develop.
- [ ] `check:develop` asserts all nine and passes; `check:edit` and `check:bulk-lut` unchanged and passing.
- [ ] `rollchrome-core` gains exactly these six tests:
  `a_preview_fits_the_long_edge_and_keeps_the_aspect_ratio`,
  `a_preview_is_upright_for_a_rotated_raw`,
  `a_preview_with_a_look_matches_the_full_render_within_two_levels` (uniform patches, on the 8-bit output),
  `the_builtin_look_list_has_twelve_entries_that_all_resolve`,
  `a_folder_lists_only_its_raw_files_sorted`,
  `resolve_look_finds_builtin_and_library_and_refuses_an_unknown_name`.
- [ ] `rollchrome-desktop` gains exactly these four:
  `preview_refuses_a_name_that_leaves_the_folder`,
  `preview_refuses_a_folder_outside_the_roots`,
  `preview_develop_returns_a_jpeg_for_a_synthetic_raw`,
  `a_second_look_on_the_same_photo_does_not_develop_it_again` (asserts `develop_preview_runs() == 1`).
- [ ] Counts: workspace 1083 → 1093, `rollchrome-core` 258 → 264, `rollchrome-desktop` 15 → 19,
  `phototools-core` 713 and `phototools-desktop` 26 unchanged. Every existing test still there,
  none weakened (G7).
- [ ] No `unwrap()`, `todo!()` or swallowed error on a shipped path (G10); no logic in the binary beyond
  the cache (G1).

### Gates

Run `. "$HOME/.cargo/env"` first.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace             # 1093
cargo test -p phototools-core      # 713
cargo test -p rollchrome-core      # 264
cargo test -p phototools-desktop   # 26
cargo test -p rollchrome-desktop   # 19
npm --prefix frontend/shared run build
npm --prefix frontend/rollchrome run typecheck
npm --prefix frontend/rollchrome run build
npm --prefix frontend/rollchrome run check:transport
npm --prefix frontend/rollchrome run check:edit
npm --prefix frontend/rollchrome run check:bulk-lut
npm --prefix frontend/rollchrome run check:develop
```

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why; the **actual
output** of every gate, counts and failures, never just "passed"; and anything you could not verify,
said plainly. Time a first `preview_develop` and a second look on a real RAW from
`~/Desktop/Projects/rollchrome-chart-sources/` and give the numbers; if you cannot, say so. Never add
those files to the repository. Do not tick the acceptance boxes; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
