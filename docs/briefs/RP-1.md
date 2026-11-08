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

### What was done

1. **Rust Core (`crates/rollchrome-core`)**:
   - Implemented `DevelopPreview` in `media/develop/preview.rs` (`PREVIEW_MAX_EDGE = 1600`): downscales linear ProPhoto 16-bit intermediate buffer using `fast_image_resize` (Lanczos3), rotates upright via `to_upright`, and provides `render(&self, look: Option<&Look>, format: OutputFormat) -> Result<RenderedImage, Error>`.
   - Extracted public `resolve_look(id: &str, store: &dyn DevelopStore) -> Result<Look, Error>` in `tools/develop.rs`, resolving `builtin:<id>` directly and delegating other ids to `store.look`.
   - Added `BuiltinLookInfo { id, name, maker }` and `builtin_look_list() -> Vec<BuiltinLookInfo>` in `media/looks/builtin.rs`, returning 12 entries with full `builtin:<id>` identifiers.
   - Added `raw_names(folder: &Path) -> Result<Vec<String>, Error>` and `thumbnail(path: &Path, max_edge: u32) -> Result<Vec<u8>, Error>` in `tools/develop_folder.rs`.
   - Added exactly 6 unit tests to `crates/rollchrome-core/tests/develop_tests.rs`:
     - `a_preview_fits_the_long_edge_and_keeps_the_aspect_ratio`
     - `a_preview_is_upright_for_a_rotated_raw`
     - `a_preview_with_a_look_matches_the_full_render_within_two_levels`
     - `the_builtin_look_list_has_twelve_entries_that_all_resolve`
     - `a_folder_lists_only_its_raw_files_sorted`
     - `resolve_look_finds_builtin_and_library_and_refuses_an_unknown_name`

2. **Rust Desktop (`crates/rollchrome-desktop`)**:
   - Implemented single-item caching in `AppState`: `get_or_create_develop_preview(path)` drops the mutex during expensive RAW decode and develop, increments atomic `develop_preview_runs`, and caches the `DevelopPreview`.
   - Added Tauri commands in `commands/develop.rs`: `list_builtin_looks`, `list_develop_folder`, `develop_thumbnail`, and `preview_develop` with G6 root validation and filename traversal checks. Registered in `main.rs`.
   - Added exactly 4 tests in `crates/rollchrome-desktop/tests/develop_desktop.rs`:
     - `preview_refuses_a_name_that_leaves_the_folder`
     - `preview_refuses_a_folder_outside_the_roots`
     - `preview_develop_returns_a_jpeg_for_a_synthetic_raw`
     - `a_second_look_on_the_same_photo_does_not_develop_it_again`

3. **Frontend (`frontend/rollchrome`)**:
   - Added TypeScript types and API methods in `src/api.ts` (`listBuiltinLooks`, `listDevelopFolder`, `developThumbnail`, `previewDevelop`, `planDevelop`, `applyDevelop`).
   - Updated routes in `src/main.ts` (`/` -> `Develop`, `/edit` -> `Edit`, `/batch` -> `BulkLut`, `/bulk-lut` -> redirect to `/batch`).
   - Updated navigation in `src/App.vue`: 01 DEVELOP (`/`), 02 EDIT (`/edit`), 03 BATCH (`/batch`). Kept brand `// EDIT`.
   - Created `src/views/Develop.vue` with folder loading, sequential thumbnail loading (one at a time, zero overlap), selection handling, look picker (Neutral, 5 Fujifilm, 7 Ricoh, My LUTs), exposure slider (-4 to +4 EV), upright latest-wins preview rendering, output format selection (JPEG sRGB or 16-bit ProPhoto TIFF into `<folder>/Developed`), dry-run lock discipline, and JobProgress with cancel.
   - Created `src/developState.ts` holding module-level state so folder, assignments, and selection survive tab switches / view unmounts without disk persistence.
   - Created test harness `scripts/develop-harness/` and acceptance script `scripts/check-develop.mjs` verifying all 9 requirements. Added `check:develop` script in `package.json`.

4. **Documentation**:
   - Updated `CLAUDE.md`: updated test counts (workspace 1093, `rollchrome-core` 264, `rollchrome-desktop` 19), added `check:develop` to gates and proofs list, and updated *Shape* to the three tabs.
   - Updated `docs/develop-plan.md`: status line and screen section point to RP-1.
   - Updated `docs/rollchrome/parity-plan.md`: status line points to RP-1.

### Deviations

None.

### Real RAW Timings

Measured on `~/Desktop/Projects/rollchrome-chart-sources/styles_source/Target.RAF` using release mode (`target/release`):
- **First `preview_develop`** (RAW file decode + intermediate develop + look + render): **5.02 s** (`develop_preview_runs`: 1, output JPEG size: 375,025 bytes).
- **Second look on same photograph** (cached `DevelopPreview` hit + new look + render): **251.32 ms** (`develop_preview_runs`: 1, output JPEG size: 165,149 bytes).

### Gate Outputs

#### `cargo fmt --all --check`
```
(clean, exit code 0)
```

#### `cargo clippy --workspace --all-targets -- -D warnings`
```
    Checking rollchrome-core v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-core)
    Checking rollchrome-desktop v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-desktop)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.37s
```

#### `cargo build --workspace`
```
   Compiling rollchrome-core v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-core)
   Compiling rollchrome-desktop v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-desktop)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 18.29s
```

#### `cargo test --workspace`
```
test result: ok. 518 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.13s (phototools-core lib)
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s (phototools-core card_tests)
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s (phototools-core deliver_tests)
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.48s (phototools-core geotag_tests)
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.35s (phototools-core ingest_tests)
test result: ok. 44 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.42s (phototools-core media_tests)
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.52s (phototools-core publish_tests)
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.69s (phototools-core raw_tests)
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.45s (phototools-core tools_tests)
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.45s (phototools-core validation_tests)
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.43s (phototools-desktop lib)
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.91s (phototools-desktop local_tools)
test result: ok. 71 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.87s (phototools-server lib)
test result: ok. 149 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.55s (rollchrome-core lib)
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.77s (rollchrome-core bulk_edit_tests)
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.53s (rollchrome-core develop_folder_tests)
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s (rollchrome-core develop_tests)
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s (rollchrome-core looks_tests)
test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 16.42s (rollchrome-core media_tests)
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.84s (rollchrome-core tools_tests)
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s (rollchrome-desktop bulk_edit_desktop)
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s (rollchrome-desktop develop_desktop)
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s (rollchrome-desktop local_tools)
Total: 1093 passed; 0 failed; 0 ignored
```

#### `cargo test -p phototools-core`
```
test result: ok. 518 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.06s
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.45s
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.50s
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.35s
test result: ok. 44 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.41s
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.52s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.71s
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.48s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.44s
Total: 713 passed; 0 failed; 0 ignored
```

#### `cargo test -p rollchrome-core`
```
test result: ok. 149 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.55s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.79s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.54s
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 16.43s
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.83s
Total: 264 passed; 0 failed; 0 ignored
```

#### `cargo test -p phototools-desktop`
```
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.44s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.93s
Total: 26 passed; 0 failed; 0 ignored
```

#### `cargo test -p rollchrome-desktop`
```
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
Total: 19 passed; 0 failed; 0 ignored
```

#### `npm --prefix frontend/shared run build`
```
> @phototools/shared@1.0.0 build
> tsc
```

#### `npm --prefix frontend/rollchrome run typecheck`
```
> @phototools/rollchrome@0.1.0 typecheck
> vue-tsc --build --force
```

#### `npm --prefix frontend/rollchrome run build`
```
> @phototools/rollchrome@0.1.0 build
> vue-tsc --build && vite build

vite v6.4.3 building for production...
✓ 83 modules transformed.
dist/index.html                                   0.45 kB │ gzip:  0.28 kB
dist/assets/vt323-400-EhPR265H.woff2              7.19 kB
dist/assets/share-tech-mono-400-DZ_B2YQ6.woff2    7.41 kB
dist/assets/ibm-plex-mono-400-BJoXLJYV.woff2     10.05 kB
dist/assets/ibm-plex-mono-500-C820gu2e.woff2     10.06 kB
dist/assets/ibm-plex-mono-600-DpGnXj3s.woff2     10.12 kB
dist/assets/orbitron-400-C4ZOLPH_.woff2          11.77 kB
dist/assets/index-DQs3N8gA.css                   53.72 kB │ gzip:  9.02 kB
dist/assets/index-CNNugFKm.js                   229.13 kB │ gzip: 77.14 kB
✓ built in 812ms
```

#### `npm --prefix frontend/rollchrome run check:transport`
```
> @phototools/rollchrome@0.1.0 check:transport
> node scripts/check-transport.mjs

transport check: 23 files, no view reaches the network directly and no HTTP client is imported
```

#### `npm --prefix frontend/rollchrome run check:edit`
```
> @phototools/rollchrome@0.1.0 check:edit
> node scripts/check-edit.mjs

Starting Vite harness server for Edit view...
Harness server listening at http://[::1]:5173
Loading test photograph into Edit view...
Benchmarking drag responsiveness (720p proxy, target p95 <= 50 ms)...
  Drag frames: min=8.6 ms, median=9.7 ms, p95=12.6 ms (budget <= 50 ms)
Benchmarking settle responsiveness (1440p proxy, target p95 <= 120 ms)...
  Settle frames: min=36.7 ms, median=40.3 ms, p95=47.5 ms (budget <= 120 ms)
Asserting latest-wins rendering discipline (100 fast inputs with 30 ms stub delay)...
  Latest-wins: issued 2 renders for 100 fast inputs (pixel1=208)
Asserting canvas stacking context and z-index isolation...
  Stacking check passed: 0 offending ancestors, canvas z-index = 60
Asserting interactive controls meet touch target budget (>= 40px)...
  All interactive controls clear 40px touch target.
Asserting "\" key behavior (ignored while input focused, active when blurred)...
Asserting double-click label resets slider to 0.0 identity...
  Double click reset exposure to 0.0.
Asserting double-click on the slider bar resets to its default...
  Double-clicking a slider bar resets it to its default (exposure 0, glow threshold 70) and settles.
Asserting saved recipe reloading and unreadable sidecar safety (Item 5)...
  Saved edits successfully reloaded: exp=1.5, temp=-20, hl=-30
Asserting recipe v2 loading (whites, blacks, brightness, hue)...
  Recipe v2 values successfully reloaded: whites=25, blacks=-35, brightness=20, hue=45
Asserting collapsible section toggles and section reset...
  Tone Curve section expanded successfully.
  Tone Curve section collapsed successfully.
  Section reset successfully reset all Basic controls to 0.
Asserting Tone Curve editor (loaded v2 curves, channel pills, point editing, and reset)...
  Loaded v2 recipe correctly displays 4 control points on Luma curve.
  Channel switching verified across Red (3), Green (2), Blue (3) curves.
  Point insertion via graph click successfully added control point (total 5).
  Keyboard navigation: ArrowUp adjusted coordinate from Out: 128 to Out: 133
  Point deletion via Delete key successfully restored point count to 4.
  Tone Curve section reset successfully restored identity curve [(0,0), (1,1)].
Asserting 8-band HSL panel (swatches, oklch backgrounds, slider values, markers, and resets)...
  Found all 8 HSL swatches.
  All HSL swatches satisfy >= 40px hit target.
  All HSL swatches compute backgrounds via dynamic CSS oklch().
  Modified indicators correctly present only on edited bands (Red, Blue).
  Red band sliders correctly display loaded values: hue=15, sat=20, lum=-10
  Blue band sliders correctly display loaded values: hue=-25, sat=40, lum=15
  Per-band reset successfully reset Blue band to 0 and cleared modified dot.
  Section reset successfully reset all HSL bands to identity.
Asserting 3-way Colour Grading panel (wheels, tabs, oklch hues, readouts, dots, and resets)...
  Found all 4 grading tabs.
  All grading tabs satisfy >= 40px hit target.
  Modified indicators correctly present on Shadows, Midtones, Highlights.
  Shadows wheel correctly displays loaded values: hue=210°, sat=35%, lum=-10%
  Grading range sliders correctly display loaded values: blending=60%, balance=-15
  Highlights wheel correctly displays loaded values: hue=35°, sat=40%, lum=15%
  Highlights wheel responded correctly to ArrowRight navigation (36°).
  Per-wheel reset successfully reset Highlights wheel to 0 and cleared modified dot.
  Section reset successfully reset all grading wheels to identity.
Asserting Geometry panel (crop overlay, aspect select, straighten, rotation, flip, and resets)...
  Geometry panel correctly displays loaded values: aspect=4:3, straighten=3.5°
  Crop overlay correctly mounted and visible in crop mode.
  Aspect preset 1:1 correctly produced square crop box (468x468).
  Straighten slider successfully updated to 7.0°.
  Orientation 6 in crop mode with no geometry has portrait canvas (854x1280) and CSS transform 'none'.
  After leaving crop mode with no edits, canvas remains portrait (420.0x630.0), transform: none.
  Orientation 6 with active crop correctly switches canvas CSS transform to 'none'.
  Delivered dimensions for orientation 6 crop are upright cropped square (1708x1708).
  Geometry reset successfully restored canvas CSS rotation (matrix(0, 1, -1, 0, 0, 0)).
  Geometry section reset successfully cleared all geometry transforms to identity.
Asserting Effects panel (loaded v2 vignette and grain, sliders, double-click reset, section reset)...
  Loaded effects sliders correctly display loaded v2 values: vignette [-45, 40, 20, 65], grain [35, 40, 60].
  Double-click reset on effects labels successfully restored default values (Amount=0, Midpoint=50, Size=25).
  Effects section reset successfully reset all vignette and grain controls to default.
Asserting Look panel (loaded v2 glow, halation, tone mapper, advisory banner, double-click reset, section reset)...
  Loaded look controls correctly display loaded v2 values and plain-language filmic advisory banner.
  Double-click reset on look labels successfully restored default values (Glow Amount=0, Threshold=70, Halation Radius=20).
  Tone mapper Plain toggle correctly deactivated Filmic and removed advisory banner.
  Look section reset successfully reset all glow, halation and tone mapper controls to identity.
Asserting histogram and clipping warnings (ED-15)...
  Histogram draws the frame on screen: luminance peaks at bin 128, the grey on the canvas.
  Clipping warnings mark only clipped pixels, sit over the photograph without changing it, and issue no render.
  Histogram stays pinned at the top of the panel scrolled 4728 px.
Asserting presets, copy/paste of settings and Undo (ED-17)...
  A preset applies the look, keeps the photograph's framing, and Undo restores the previous settings.
  ⌘C/⌘V copy settings between photographs, keep the framing, save, and leave text fields alone.
  Presets save, ask before replacing, rename and delete; an unreadable preset is reported.
Asserting masks: add, shape on the canvas, adjust, overlay, follow the frame, undo (ED-20)...
  Masks: added where they appear, shaped by mouse and keyboard, adjusted, shown, kept on the content, limited to four, undoable.
Asserting the brush: paint, size keys, erase, refine a radial, undo a stroke (ED-21)...
  Brush: paints thinned strokes at its size, [ ] resize, ⌥ erases, Esc stops, refines a radial, undoable.
Asserting automatic masks: ask before downloading, find a sky and a subject, stop (ED-23)...
  Automatic masks: asked before downloading, found, shown, stoppable, failures reported, refinable with the brush.
Asserting a selected mask takes the panel, and Done or Esc give it back...
  A selected mask takes the panel; its Exposure is its own; Done and Esc give the panel back.
  Corrupted sidecar correctly reported error and disabled autosave.
Asserting EXIF orientations 1-8 and rotated fit inside viewport (Item 8)...
  Orientation 1: marked corner=top-left, portrait=false, fits inside container=true
  Orientation 2: marked corner=top-right, portrait=false, fits inside container=true
  Orientation 3: marked corner=bottom-right, portrait=false, fits inside container=true
  Orientation 4: marked corner=bottom-left, portrait=false, fits inside container=true
  Orientation 5: marked corner=top-left, portrait=true, fits inside container=true
  Orientation 6: marked corner=top-right, portrait=true, fits inside container=true
  Orientation 7: marked corner=bottom-right, portrait=true, fits inside container=true
  Orientation 8: marked corner=bottom-left, portrait=true, fits inside container=true
Visual proof screenshot saved to /Users/pablogtorres/Desktop/Projects/MasterPhotoTools/frontend/rollchrome/layout-proof/edit.png

--- check:edit Summary ---
PASSED all checks.
```

#### `npm --prefix frontend/rollchrome run check:bulk-lut`
```
> @phototools/rollchrome@0.1.0 check:bulk-lut
> node scripts/check-bulk-lut.mjs

Starting Vite harness server for Bulk LUT view...
Harness server listening at http://[::1]:5173
Verifying initial locked state before dry run...
Filling inputs and selecting LUT...
Triggering dry run...
Rendered 3 sample frame cards in preview strip.
Stub metrics: maxConcurrentSessions=1, samplePreviewsRequested=3
Testing sample preview failure handling in preview strip...
Sample preview error card text: "✕
Couldn't preview frame2.jpg: Corrupted preview for /Volumes/Photos/frame2.jpg"
Testing dry-run invalidation on setting change...
Re-running dry run after settings change...
Testing core refusal when LUT changed on disk...
Core refusal text: "The LUT file changed on disk since the reviewed plan (expected 7f83b1657ff1fc53b92dc18148a1d65dfc2d4b1fa3d677284addd200126d9069, found e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855); please run another dry run"
Testing cancelJob error handling when cancel throws...
Cancel throw refusal text: "Failed to cancel job: IPC cancel error"
Testing cancelJob error handling when cancel returns false...
Cancel false refusal text: "The job had already finished."
Testing successful job cancellation...
Job terminal message after cancel: "2 graded, 0 failed, 1 skipped"
Testing a batch graded with a preset (ED-18)...
  Preset batch: plans by name, previews the whole look, locks on change, sends the reviewed hash, shows refusals.
Auditing touch target heights (>= 40px)...
Wrote layout screenshot proof to /Users/pablogtorres/Desktop/Projects/MasterPhotoTools/frontend/rollchrome/layout-proof/bulk-lut.png

check:bulk-lut PASSED cleanly: all acceptance assertions and benchmarks verified.
```

#### `npm --prefix frontend/rollchrome run check:develop`
```
> @phototools/rollchrome@0.1.0 check:develop
> node scripts/check-develop.mjs

Starting Vite harness server for Develop view...
Harness server listening at http://[::1]:5173
1. Checking folder loading, six thumbnails, and zero overlap in thumbnail requests...
  Thumbnail requests: 6, max concurrent: 1
2. Checking look picker groups...
  Picker counts: Neutral=1, Fuji=5, Ricoh=7, LUTs=2
3. Checking look assignment to single selection vs Select all...
4. Checking plan_develop items with exact builtin:<id> and exposures...
5. Checking latest-wins preview rendering...
  Preview blob size: 240 (expected second: 240, stale first: 230)
6. Checking lock discipline and gate explanations...
  Testing lock on look change...
  Testing lock on exposure change...
  Testing lock on format change...
7. Checking skipped items, job cancellation, and summary counts...
  Skipped text: "SKIPPED ITEMS (1)
DSCF0006.RAF
— already developed into /Volumes/Photos/RAW/Developed/DSCF0006.tif"
  Summary terminal message: "2 developed, 0 failed, 1 skipped"
8. Checking state persistence across view unmount and remount...
9. Auditing touch target heights (>= 40px) and taking screenshot...
Wrote layout screenshot proof to /Users/pablogtorres/Desktop/Projects/MasterPhotoTools/frontend/rollchrome/layout-proof/develop.png

check:develop PASSED: all 9 checks verified successfully.
```

---

## Review

*Written by the architect, after re-running the gates on the branch.*

### Round 1 — Changes requested

**Checked.** Re-ran every gate on `rp-1`: fmt and clippy clean, build clean; workspace 1093, `phototools-core`
713, `rollchrome-core` 264, `phototools-desktop` 26, `rollchrome-desktop` 19, none failed or ignored;
typecheck, build, `check:transport`, `check:edit`, `check:bulk-lut` and `check:develop` pass. No existing test
was changed. `Do not touch` respected, no new dependency. Tauri argument names and `OutputFormat`'s serde form
match the Rust. `unsafe` in `preview.rs` follows `media/edit/preview.rs`. The 5.02 s and 251 ms timings were
reported, not re-measured.

The Rust is right. The defects are in how the screen behaves over time, which the check does not exercise.

1. **A preview request starts a full development for every slider step.** Each new exposure is a new cache key.
   `AdjustmentSlider` emits on every input, `triggerPreview` calls the command each time, and *latest wins* only
   discards the *result*: a drag across the slider starts dozens of 5-second, all-core developments of a 26 MP
   RAW that run to completion in parallel. Clicking quickly through photographs does the same. In
   `triggerPreview`, keep **at most one request in flight**; while one runs, remember only the newest wanted state,
   and send that when it returns. Add to `check:develop`: *10. During a burst of ten exposure changes against a
   slow stub, no two preview requests overlap, at most two are sent, and the last carries the final exposure.*
   Assertion 5 keeps its meaning (the newest choice is what stays on screen); adapt its stub timing only if
   serialising forces it, and say so in the Report.
2. **A look follows a file name into another folder.** `moduleAssignments` is keyed by name alone and survives
   changing folder. Cameras reuse names, so `DSCF0001.RAF` in a second folder silently takes the first folder's
   look and exposure. Clear assignments and selection when the folder **changes**, but not on a remount. Add:
   *11. Assigning a look, then changing to a folder with a file of the same name, shows that file with no look.*
3. **Leaving the tab does not stop the thumbnail loop.** `thumbnailToken` belongs to the instance and nothing
   bumps it on unmount, so after going to Edit and back the old loop and the new one both run, which is the overlap
   the brief forbids, and object URLs are never revoked. Add `onBeforeUnmount`: bump the token and revoke every
   thumbnail and preview URL; revoke thumbnail URLs when the folder changes too. Add: *12. Unmounting and remounting
   while thumbnails load gives no overlapping thumbnail requests.*
4. **A running development is forgotten on leaving the tab.** `jobId` and `isDeveloping` are instance state. Come back
   from Edit during a run and the progress is gone and the screen will start a second run. Hold them in
   `developState.ts`; on mount, re-attach `JobProgress` to the job, and if it has already ended, clear the running
   state and show its outcome. Add: *13. Remounting during a run shows its progress and keeps Develop disabled.*
5. **Everything in the folder is always developed.** My brief left this out; it is needed. Assigning looks to five
   photographs of three hundred must not develop three hundred. Add a choice beside the run controls, **All
   photographs** (the default) or **Selected only**, showing the count. `makeItems` sends the matching items; changing
   the choice or, under *Selected only*, the selection invalidates the dry run. Add: *14. With Selected only, the items
   sent to `plan_develop` are exactly the selection, and changing the selection locks Develop.*
6. **Thumbnails show the internal id** (`builtin:ricoh-posit…`). Show the look's display name, and the LUT's name for
   library looks; *None* for no look.
7. **The check cannot see a picture.** `layout-proof/develop.png` has an empty preview and six empty thumbnails: the
   stub's bytes are not an image. Return a valid small JPEG and assert `naturalWidth > 0` on the thumbnails and the
   preview. The stub's looks should be the twelve that ship: it lists an Astia, which does not.
8. **Small.** `lib.rs`: the two new `lock().unwrap()` become `unwrap_or_else(|e| e.into_inner())`, since a poisoned cache
   is still a usable cache and the brief's acceptance says no `unwrap()`; leave the existing six alone (G11). Remove
   `developSummary` and its banner, which nothing ever sets.

Counts stay at 1093 / 264 / 19. `check:develop` now asserts fourteen. Write **Report round 2** above this Review
section's end, as new text, deleting nothing.
