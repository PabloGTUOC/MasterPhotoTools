# `RC-1` · Carve the engine into `rollchrome-core` — no behaviour change

> **Status:** briefed
> **Plan:** [`docs/rollchrome-plan.md`](../rollchrome-plan.md), step `RC-1` · **Branch:** `rc-1`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

*Written by the architect. **Self-contained**: the implementer has none of the architect's context,
so anything not written here does not exist for it.*

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11) and **Gates**. They apply to every line of this work.
  G2, G7 and G11 are the ones this step is most likely to break.
- `docs/rollchrome-plan.md` — *The boundary*, *Rules that hold throughout* (R1–R5) and *Things that
  will catch you*. This brief is the RC-1 section of that plan made exact.
- `Cargo.toml` (workspace root) — the `[profile.dev.package.phototools-core]` section and the
  comment above it. The new crate needs the same treatment, for the reason given there.

### Goal

PhotoTools has grown a second product inside it: the photo editor (Edit tab, Batch Grade, masks,
LUTs), which will become its own desktop application, **Rollchrome**. This step moves the editor's
**library code** out of `phototools-core` into a new library crate, `rollchrome-core`, which depends
on `phototools-core` for the things both products share (decoding, RAW, metadata, the `Tool`
machinery). Nothing else changes: the PhotoTools desktop application depends on both crates and
still shows Edit and Batch Grade exactly as before, the server is untouched, and **no test is
added, removed or weakened — every test that exists today still exists and passes, in one crate or
the other.** The point of the step is to prove the boundary compiles, so a later step (RC-2) can
move the screens into an application of their own.

It is a **move**, not a rewrite. If you find yourself changing what a function does, stop: that is
out of scope (G11) and belongs in the report as a suggestion.

### Change

Do the work in this order. Use `git mv` for every file that moves whole, so history follows it.

#### 1. The shared sidecar names, in `phototools-core`

The editor saves its adjustments in a sidecar beside the photograph, `IMG_0001.JPG.photoedit`. F3
Rename (`crates/core/src/tools/f3_rename.rs`, lines ~210, ~221, ~296–298) renames that sidecar with
its photograph, using `is_sidecar` and `sidecar_path` from `tools/edit.rs`. `tools/edit.rs` is
moving to Rollchrome, but F3 stays in PhotoTools, so these names must stay where F3 can reach them.

- Create `crates/core/src/sidecar.rs` holding, moved verbatim from `crates/core/src/tools/edit.rs`
  (lines ~19, ~31–41) with their doc comments: `pub const SIDECAR_EXTENSION`, `pub fn sidecar_path`,
  `pub fn is_sidecar`. Add a module doc comment saying *why* it lives here: it is the one contract
  both applications must agree on — a photograph edited in Rollchrome and renamed in PhotoTools must
  keep its edits.
- Add `pub mod sidecar;` to `crates/core/src/lib.rs`.
- Point `f3_rename.rs` at `crate::sidecar::{is_sidecar, sidecar_path}`.
- In `tools/edit.rs` (before it moves), replace the three definitions with
  `pub use phototools_core::sidecar::{is_sidecar, sidecar_path, SIDECAR_EXTENSION};` (written as
  `crate::sidecar` while still in `phototools-core`), so every existing caller of
  `tools::edit::sidecar_path` keeps compiling unchanged.

#### 2. The new crate `crates/rollchrome-core`

- `crates/rollchrome-core/Cargo.toml`: package `rollchrome-core`, `version = "0.1.0"`,
  `edition = "2021"`. Dependencies — only what the moved code uses, at **exactly the versions
  `crates/core/Cargo.toml` already pins** (no new crate enters the dependency graph):
  - `phototools-core = { path = "../core" }`
  - `image`, `serde` (with `derive`), `serde_json`, `rayon`, `sha2`, `fast_image_resize`,
    `reqwest` (with `blocking`) — the last for `segment.rs`'s model download.
  - `ort = { version = "=2.0.0-rc.13", optional = true }` and `[features] segment = ["dep:ort"]`,
    **moved** from `crates/core/Cargo.toml` with their comment.
  - `[dev-dependencies]`: whatever the moved tests need — expect `tempfile`, `chrono`, `image`,
    `sha2`, same versions as `phototools-core`'s.
  - The `[[example]] auto_mask` section with `required-features = ["segment"]`, moved with its
    comment.
  - If the compiler asks for a crate not listed here, add it at `phototools-core`'s version and
    name it in the report. A crate `phototools-core` does not already depend on is **not** to be
    added — stop and report instead (G8).
- `crates/rollchrome-core/src/lib.rs`: a doc comment (the editor's engine and tools, split from
  PhotoTools in RC-1; desktop-only; depends on `phototools-core` for decoding, RAW and metadata),
  then `pub mod media;` and `pub mod tools;`.
- **Module layout mirrors the old one**, so an import changes only its crate name:
  `phototools_core::media::edit::X` becomes `rollchrome_core::media::edit::X`, and likewise for
  `media::segment`, `media::histogram`, `tools::edit`, `tools::lut`, `tools::presets`,
  `tools::bulk_edit`, `tools::lut_library`.
  - `src/media/mod.rs`: `pub mod edit; pub mod histogram; pub mod segment;` and the two `pub use`
    blocks that re-export them today in `crates/core/src/media/mod.rs` (`pub use edit::{...}` and
    `pub use histogram::{...}`), moved.
  - `src/tools/mod.rs`: `pub mod bulk_edit; pub mod edit; pub mod lut; pub mod lut_library;
    pub mod presets;` and the five matching `pub use` blocks from `crates/core/src/tools/mod.rs`,
    moved.

#### 3. Move the code

`git mv` from `crates/core/` to `crates/rollchrome-core/`:

| From | To |
|---|---|
| `src/media/edit/` (15 files, `mod.rs` included) | `src/media/edit/` |
| `src/media/segment.rs` | `src/media/segment.rs` |
| `src/media/histogram.rs` | `src/media/histogram.rs` |
| `src/tools/edit.rs`, `lut.rs`, `presets.rs`, `bulk_edit.rs`, `lut_library.rs` | `src/tools/` |
| `examples/auto_mask.rs` | `examples/auto_mask.rs` |
| `tests/bulk_edit_tests.rs` | `tests/bulk_edit_tests.rs` |

Then remove from `crates/core/src/media/mod.rs` and `crates/core/src/tools/mod.rs` the `pub mod`
lines and `pub use` blocks for what left, and remove `ort`, the `segment` feature and the
`[[example]]` from `crates/core/Cargo.toml`.

Fix the paths inside the moved code. The rule: a path to something **that moved** keeps `crate::`;
a path to something **that stayed** becomes `phototools_core::`. What stayed and is used, measured:
`error::Error`, `jobs::{Progress, InMemoryProgress, Outcome, ToolResult}`,
`tools::{carry_metadata, carry_metadata_with, expand_inputs, sample_evenly, Derived, Plan, Skip,
Tool}`, `ingest::scanner::hex`, `ingest::card::Card`, `media::raw::{is_raw, raw_to_jpeg}`,
`media::meta::read_meta`, `media::jpeg::{encode, JpegOptions}`. Careful with
`crate::tools::{carry_metadata_with, ...}` in `tools/lut.rs` and `tools/bulk_edit.rs`: after the
move `crate::tools` is *Rollchrome's* `tools`, which does not have them.

**Visibility.** Widen in `phototools-core` only what the compiler refuses, `pub(crate)` → `pub`,
nothing pre-emptively. Expected: `ingest::scanner::hex` (`crates/core/src/ingest/scanner.rs:280`).
List every widening in the report.

**The editor keeps `phototools_core::error::Error`.** Do not create a Rollchrome error type; that is
a redesign, not a move.

#### 4. Split the two mixed test files

`crates/core/tests/media_tests.rs` and `crates/core/tests/tools_tests.rs` hold editing tests among
PhotoTools ones. Create `crates/rollchrome-core/tests/media_tests.rs` and
`crates/rollchrome-core/tests/tools_tests.rs`, and **move** (cut, not copy) the editing tests into
them, bodies unchanged except for `phototools_core::` → `rollchrome_core::` in paths to moved items.

**The fixtures are shared, not copied.** Every Rollchrome test file starts with

```rust
#[path = "../../core/tests/fixtures/mod.rs"]
mod fixtures;
```

`fixtures/mod.rs` already carries `#![allow(dead_code)]`, so a partial use of it is fine.

**`media_tests.rs` — move these 32 tests** (line numbers as on `main` at `f0f401b`):

`decode_tables_match_the_exact_conversion_for_every_code` (1384) and every test after it to the
end of the file — `rendering_never_changes_the_cached_proxies`,
`the_preview_matches_the_export_renderer_within_one_code`,
`a_preview_refuses_a_recipe_whose_lut_is_missing`, `a_preview_frame_is_rgba_of_the_proxy_size`,
`binary_preview_ipc_header_encodes_histograms_and_clipping_accurately`, `benchmark_edit_preview`,
`a_colour_only_change_does_not_recompute_geometry`,
`preview_frame_without_geometry_reports_photo_orientation_and_with_geometry_reports_one`,
`the_vignette_follows_the_crop`, `preview_and_export_seed_grain_identically_for_an_unsaved_recipe`,
`untouched_vignette_and_grain_are_exact_identity`,
`vignette_attenuation_is_identical_on_preview_proxy_and_full_export`,
`grain_is_byte_identical_between_two_runs_of_the_same_photo`,
`grain_evaluation_is_fully_deterministic_across_threads`,
`grain_appearance_and_density_are_scale_independent_between_proxy_and_export`,
`untouched_looks_are_exact_identity`, `tone_mapper_off_preserves_exact_identity`,
`narkowicz_aces_compresses_highlights_monotonically_without_inversion`,
`aces_tone_mapper_places_mid_grey_where_the_doc_says`,
`glow_pyramid_executes_within_interactive_budget_during_drag`,
`glow_and_halation_decay_smoothly_and_scale_with_image_resolution`,
`a_mask_follows_the_crop_and_straighten`, `pixels_no_mask_covers_are_unchanged_by_it`,
`a_mask_with_zero_adjustments_changes_nothing`,
`export_matches_preview_with_masks_within_delta_e_1_5`,
`a_v2_sidecar_loads_with_no_masks_and_masks_round_trip_in_v3`,
`the_mask_overlay_shows_where_the_mask_acts`,
`brush_strokes_rasterise_identically_at_proxy_and_export_size`,
`a_brushed_mask_follows_the_crop_and_rotation`,
`painting_draws_each_new_segment_once_and_erasing_restores_exactly`,
`a_stored_automatic_mask_acts_where_it_covers_and_a_damaged_one_is_refused`.

With them, the helpers only they use: `radial_mask`, `marked_frame`, `centroid`, `brush_mask`,
`brush_stroke`. The 44 tests before line 1384 stay.

**`tools_tests.rs` — move these 31 tests:**

`a_sidecar_round_trips_and_a_future_version_is_refused`,
`a_v1_sidecar_loads_into_v2_with_default_identities`, `a_recipe_version_four_is_refused_as_unsupported`,
`a_v1_sidecar_saved_again_is_written_as_the_current_version`, `a_sidecar_is_written_atomically`,
`f3_rename_carries_masks_in_the_sidecar`,
`editor_refuses_to_save_a_sidecar_for_a_nonexistent_image`,
`editor_refuses_to_write_a_sidecar_on_a_card_volume`, `a_mounted_share_without_dcim_is_writable`,
`a_library_folder_containing_a_copied_dcim_is_not_a_card`,
`export_edited_image_appends_edit_suffix_and_never_overwrites`,
`exporting_a_recipe_whose_lut_is_missing_is_refused`, `a_failed_export_leaves_no_file_behind`,
`export_refuses_a_destination_on_a_card`,
`exporting_an_untouched_jpeg_preserves_source_bytes_without_reencoding`,
`an_exported_jpeg_keeps_its_orientation_tag_and_capture_date`,
`a_16_bit_tiff_exports_as_a_16_bit_tiff`, `bulk_lut_over_many_files_starts_exactly_one_exiftool`,
`bulk_lut_preserves_capture_date_and_gps_on_all_outputs`,
`bulk_lut_summary_reports_processed_skipped_and_failed`,
`bulk_lut_never_overwrites_and_names_outputs_with_lut_suffix`,
`bulk_lut_refuses_an_output_directory_on_a_card`,
`bulk_lut_cancelled_midway_leaves_no_partial_file_and_reports_what_it_wrote`,
`bulk_lut_skips_sidecars_and_hidden_files`,
`sample_frames_spreads_across_the_folder_rather_than_taking_the_first`,
`export_still_carries_metadata_after_the_split`,
`a_malformed_lut_is_refused_at_plan_time_not_after_running`,
`a_lut_changed_after_the_dry_run_refuses_the_run`,
`import_lut_refuses_an_unparseable_file_and_never_overwrites_a_different_lut`,
`a_recipe_whose_lut_left_the_library_is_refused_by_name`,
`a_recipe_with_only_round_two_adjustments_is_rendered_not_byte_copied`.

With them, the helpers only they use: `spawn_counting_shim` (line ~1649) and `create_test_cube`.
`dt` is used on both sides: **copy** it into Rollchrome's file (it is three lines) and leave the
original. `f3_rename_carries_masks_in_the_sidecar` runs F3 Rename, which is fine: `rollchrome-core`
depends on `phototools-core`.

**These four stay in `phototools-core`'s `tools_tests.rs`**, because they test F3 Rename and the
shared sidecar names, which are PhotoTools' behaviour:

- `a_sidecar_is_named_after_the_whole_file_so_raw_and_jpeg_pairs_do_not_share_one` — change only
  its import, to `phototools_core::sidecar::sidecar_path`.
- `f3_rename_carries_companion_photoedit_sidecar`, `f3_rename_plans_a_conflict_when_the_sidecar_target_exists`,
  `f3_rename_reports_a_sidecar_that_could_not_follow_its_photo` — these build their sidecar with
  the editor's `save_recipe`, which `phototools-core` can no longer reach. **This is the one
  permitted change to a test's body**, and only to its setup: replace the
  `AdjustmentRecipe { .. }` / `save_recipe(&img, &recipe)` lines with a call to a new helper in
  the same file,

  ```rust
  /// A sidecar as Rename sees it: an opaque file beside the photograph. Rename moves it and never
  /// reads it, so these tests need no editor. That a real recipe survives a rename, masks and all,
  /// is `f3_rename_carries_masks_in_the_sidecar` in rollchrome-core.
  fn plant_sidecar(img: &Path) -> PathBuf {
      let sidecar = phototools_core::sidecar::sidecar_path(img);
      fs::write(&sidecar, br#"{"version":3,"exposure":1.25}"#).unwrap();
      sidecar
  }
  ```

  and use its return value wherever the test used `save_recipe`'s. **Every assertion stays exactly
  as it is.** The review diffs these three tests line by line.

Everything else in both files stays. After the split, prune the `use` lines at the top of each of
the four files to what that file uses — clippy's `-D warnings` fails on an unused import.

#### 5. The desktop application

- `crates/desktop/Cargo.toml`: `phototools-core = { path = "../core" }` (the `segment` feature no
  longer exists there) and add `rollchrome-core = { path = "../rollchrome-core", features =
  ["segment"] }`, with a one-line comment: the editor's engine, split out in RC-1; leaves this
  crate in RC-2.
- `crates/desktop/src/` and `crates/desktop/tests/`: change `phototools_core::` to
  `rollchrome_core::` in every path to a moved item — measured, these are in `src/lib.rs`
  (lines ~16–18, ~109, ~174), `src/commands/edit.rs`, `auto_masks.rs`, `bulk_edit.rs`,
  `presets.rs`, and `tests/local_tools.rs`, `tests/bulk_edit_desktop.rs`. **Nothing else** in the
  desktop changes: no command renamed, no `generate_handler!` entry moved, no state restructured.

#### 6. The workspace

- Root `Cargo.toml`: add `"crates/rollchrome-core"` to `members`, and add
  `[profile.dev.package.rollchrome-core] opt-level = 3` with a comment pointing at the
  `phototools-core` one: the engine's kernels are generic and monomorphised into this crate now, and
  without it the timing tests (`glow_pyramid_executes_within_interactive_budget_during_drag`, 5 ms)
  measure a debug build.

#### 7. Documents

- `CLAUDE.md`, **Gates** block only: change `# 901 passing — G2` to the new `phototools-core`
  count, and add a line `cargo test -p rollchrome-core   # N passing — G2` beneath it. The
  workspace line stays `1011`. Do not touch the rest of `CLAUDE.md` — its *Shape* section is RC-3's.
- `docs/rollchrome-plan.md`: the status line at the top becomes *"RC-1 done (branch `rc-1`); RC-2
  next."* Add one sentence to RC-3's first bullet that the `segment` feature already left the
  desktop's `phototools-core` dependency in RC-1 (it had to: the feature no longer exists there).

### Do not touch

- `crates/server/` — not one line. The server uses none of the editor.
- `frontend/` — all of it. The screens move in RC-2.
- `crates/core/src/config.rs` — `lut_dir`, `models_dir`, `presets_dir` stay where they are; they
  leave in RC-3.
- `.github/workflows/ci.yml` — teaching CI about Rollchrome is RC-3. (`cargo test --workspace`
  already runs the new crate's tests.)
- `carry_metadata` and its exiftool path — it stays as it is (R1; replacing it is RC-4).
- `SPECIFICATION.md` (G9).
- Tempting and out of scope: a `[workspace.dependencies]` table, renaming any public item, a
  Rollchrome error type, flattening `rollchrome_core::media::edit` to `rollchrome_core::edit`,
  tidying the code you move, anything in `is_sidecar`'s hard-coded `".photoedit"`. Put ideas in the
  report.
- **Timing budgets and assertions in every test** (G7). If a test fails after the move, the move is
  wrong; fix the move.

### Acceptance

Each one checkable by someone who did not do the work.

- [ ] **Baseline recorded first.** Before any change, on `rc-1` as cut: `cargo test --workspace`
      and `cargo test -p phototools-core` totals in the report. Expected **1011** and **901**. If
      either differs, report it before going further — every number below assumes them.
- [ ] **Test counts: `901` → `713` + `188`, every test accounted for.**
      - `cargo test -p phototools-core`: **713** passed, 0 failed, 0 ignored.
      - `cargo test -p rollchrome-core`: **188** passed, 0 failed, 0 ignored — the 115 unit tests
        inside the moved `src/` files, 32 from `media_tests.rs`, 31 from `tools_tests.rs`, 10 in
        `bulk_edit_tests.rs`.
      - `cargo test --workspace`: **1011** passed — unchanged; desktop and server keep their 110.
      - If the split lands differently (a test classed as editing turns out to need nothing from
        the editor, or the reverse), the sum must still be 901, and the report names each test
        that deviates from the lists above and why.
- [ ] Both libraries pass **with no binary crate present** (G2): `cargo test -p phototools-core`
      and `cargo test -p rollchrome-core` each succeed on their own.
- [ ] `grep -rnE "media::(edit|segment|histogram)|tools::(edit|lut|lut_library|presets|bulk_edit)\b" crates/core`
      finds nothing.
- [ ] `cargo tree -p phototools-core -e normal --prefix none | grep -E "^ort(-sys)? "` finds
      nothing; `cargo tree -p rollchrome-core -e normal --features segment --prefix none | grep -E "^ort "`
      finds `ort v2.0.0-rc.13`.
- [ ] `grep -rn "rollchrome" crates/core crates/server` finds nothing except the comment in
      `sidecar.rs` — `phototools-core` does not know Rollchrome exists, and the server does not either.
- [ ] `git diff main --stat -- crates/server frontend .github SPECIFICATION.md` is empty.
- [ ] `git log --follow --oneline crates/rollchrome-core/src/media/edit/pipeline.rs` shows the
      file's history before this branch (it was moved, not re-created).
- [ ] The only changed test *bodies* are the three F3 tests in `crates/core/tests/tools_tests.rs`,
      setup only. Every other moved test differs from its original by import paths alone.
- [ ] `cargo build -p rollchrome-core --example auto_mask --features segment` succeeds.
- [ ] No new crate in `Cargo.lock`: `git diff main -- Cargo.lock` adds only the `rollchrome-core`
      package entry and its dependency list.

### Gates

Run them in this order, and **never run `cargo test` at the same time as a browser check** — two
tests time themselves and fail on a loaded machine (rollchrome plan, *Things that will catch you*).

```sh
cargo fmt --all --check                                  # no output
cargo clippy --workspace --all-targets -- -D warnings    # no warnings
cargo build --workspace                                  # ok
cargo test --workspace                                   # 1011 passed
cargo test -p phototools-core                            # 713 passed — G2
cargo test -p rollchrome-core                            # 188 passed — G2
cargo build -p rollchrome-core --example auto_mask --features segment

npm --prefix frontend/shared run build

npm --prefix frontend/web run typecheck
npm --prefix frontend/web run build
npm --prefix frontend/web run check:transport
npm --prefix frontend/web run check:layout        # 12 routes clean at 390 px
npm --prefix frontend/web run check:ingest

npm --prefix frontend/desktop run typecheck
npm --prefix frontend/desktop run build
npm --prefix frontend/desktop run check:transport
npm --prefix frontend/desktop run check:edit      # budgets unchanged
npm --prefix frontend/desktop run check:bulk-lut
```

The first `rollchrome-core` build with `segment` downloads ONNX Runtime from `cdn.pyke.io`; on the
Mac this just works.

Optional, and say plainly in the report whether you did it: `cargo tauri dev` from
`crates/desktop`, open a photograph in Edit, move a slider, apply a LUT in Batch Grade. If you did
not, write "not run".

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why; the **actual
output** of every gate — counts and failures, never just "passed"; every `pub(crate)` → `pub`
widening and every dependency added to `rollchrome-core`'s manifest that this brief did not name;
any test that landed on a different side from the lists above; and anything you could not verify,
said plainly. Do not tick the acceptance boxes above; the review does that.

---

## Report

*Written by the implementer.*

### Stopped at Step 2 — Baseline could not be recorded (`cargo` not installed)

Per the brief's first acceptance criterion (*"Baseline recorded first. Before any change, on `rc-1` as cut: `cargo test --workspace` and `cargo test -p phototools-core` totals in the report. Expected **1011** and **901**. If either differs, report it before going further"*), I cut branch `rc-1` from `main` (`1e14ae7`) and ran the baseline test commands before changing any files.

Actual output:

```text
$ cargo test --workspace
zsh:1: command not found: cargo

$ which rustup rustc cargo; ls -la ~/.cargo ~/.rustup 2>&1
rustup not found
rustc not found
cargo not found
ls: /Users/pablogtorres/.cargo: No such file or directory
ls: /Users/pablogtorres/.rustup: No such file or directory
```

A filesystem search across `/opt`, `/usr/local`, and `$HOME` confirmed that no Rust toolchain (`rustup`, `rustc`, `cargo`) is installed on this machine.

In accordance with the instructions (*"if cargo is not available or the counts are not 1011 and 901, stop and write that in the Report section"*), no code or configuration files were modified. Once the Rust toolchain (`1.99.0` per `rust-toolchain.toml`) is installed on this machine, `RC-1` can be implemented and verified against all gates.

### Implementation Report (after Rust 1.99.0 toolchain installation)

#### Baseline (recorded before any code change)

- `cargo test --workspace`: **1011 passed**, 0 failed, 0 ignored (`phototools-core` 901 + `phototools-desktop` 39 + `phototools-server` 71):
  ```text
  Running unittests src/lib.rs (phototools_core):       633 passed; 0 failed; 0 ignored
  Running tests/bulk_edit_tests.rs:                      10 passed; 0 failed; 0 ignored
  Running tests/geotag_tests.rs:                         11 passed; 0 failed; 0 ignored
  Running tests/handoff_tests.rs:                         8 passed; 0 failed; 0 ignored
  Running tests/image_tools_tests.rs:                    24 passed; 0 failed; 0 ignored
  Running tests/ingest_tests.rs:                         16 passed; 0 failed; 0 ignored
  Running tests/media_tests.rs:                          76 passed; 0 failed; 0 ignored
  Running tests/publish_tests.rs:                        37 passed; 0 failed; 0 ignored
  Running tests/raw_tests.rs:                            12 passed; 0 failed; 0 ignored
  Running tests/tools_tests.rs:                          64 passed; 0 failed; 0 ignored
  Running tests/validation_tests.rs:                     10 passed; 0 failed; 0 ignored
  Running unittests src/lib.rs (phototools_desktop):     19 passed; 0 failed; 0 ignored
  Running unittests src/main.rs (phototools_desktop):     0 passed; 0 failed; 0 ignored
  Running tests/bulk_edit_desktop.rs:                     4 passed; 0 failed; 0 ignored
  Running tests/local_tools.rs:                          16 passed; 0 failed; 0 ignored
  Running unittests src/lib.rs (phototools_server):      24 passed; 0 failed; 0 ignored
  Running unittests src/main.rs (phototools_server):      0 passed; 0 failed; 0 ignored
  Running tests/api_tests.rs:                            41 passed; 0 failed; 0 ignored
  Running tests/web_serving_tests.rs:                     6 passed; 0 failed; 0 ignored
  ```
- `cargo test -p phototools-core`: **901 passed**, 0 failed, 0 ignored (633 + 10 + 11 + 8 + 24 + 16 + 76 + 37 + 12 + 64 + 10 = 901).

#### What was done

1. **Shared sidecar contract (`crates/core/src/sidecar.rs`):** Moved `SIDECAR_EXTENSION`, `sidecar_path`, and `is_sidecar` verbatim from `tools/edit.rs` into `crates/core/src/sidecar.rs`, exported `pub mod sidecar;` from `crates/core/src/lib.rs`, pointed `crates/core/src/tools/f3_rename.rs` at `crate::sidecar::{is_sidecar, sidecar_path}`, and re-exported `pub use phototools_core::sidecar::{is_sidecar, sidecar_path, SIDECAR_EXTENSION};` from `tools/edit.rs`.
2. **Created `crates/rollchrome-core`:** Added `Cargo.toml`, `src/lib.rs`, `src/media/mod.rs`, and `src/tools/mod.rs` mirroring the module layout and re-exports of `phototools-core`.
3. **Moved editor engine, tools, example, and `bulk_edit_tests.rs` via `git mv`:**
   - `crates/core/src/media/edit/` (15 files) → `crates/rollchrome-core/src/media/edit/`
   - `crates/core/src/media/segment.rs` → `crates/rollchrome-core/src/media/segment.rs`
   - `crates/core/src/media/histogram.rs` → `crates/rollchrome-core/src/media/histogram.rs`
   - `crates/core/src/tools/{edit,lut,presets,bulk_edit,lut_library}.rs` → `crates/rollchrome-core/src/tools/`
   - `crates/core/examples/auto_mask.rs` → `crates/rollchrome-core/examples/auto_mask.rs`
   - `crates/core/tests/bulk_edit_tests.rs` → `crates/rollchrome-core/tests/bulk_edit_tests.rs`
   - Removed the moved `pub mod` / `pub use` blocks from `crates/core/src/media/mod.rs` and `crates/core/src/tools/mod.rs`, and removed `ort`, the `segment` feature, and `[[example]] auto_mask` from `crates/core/Cargo.toml`.
   - Updated paths inside moved files to reference `phototools_core::` for items that stayed in `phototools-core`.
4. **Split mixed test files (`media_tests.rs` and `tools_tests.rs`):**
   - Moved the 32 editing tests (from `decode_tables_match_the_exact_conversion_for_every_code` to the end of the file) and their 5 helpers (`radial_mask`, `marked_frame`, `centroid`, `brush_mask`, `brush_stroke`) into `crates/rollchrome-core/tests/media_tests.rs`, leaving the 44 PhotoTools media tests in `crates/core/tests/media_tests.rs`.
   - Moved the 31 editing tests and their 2 exclusive helpers (`spawn_counting_shim`, `create_test_cube`, plus a copy of `dt`) into `crates/rollchrome-core/tests/tools_tests.rs`, leaving 33 tests in `crates/core/tests/tools_tests.rs` (including the 4 F3/sidecar tests, with `plant_sidecar` replacing `save_recipe` setup in the 3 F3 sidecar tests).
   - Shared `../../core/tests/fixtures/mod.rs` via `#[path = "../../core/tests/fixtures/mod.rs"] mod fixtures;` across all three Rollchrome test files.
5. **Updated `crates/desktop`:** Updated `crates/desktop/Cargo.toml` to depend on `phototools-core = { path = "../core" }` and `rollchrome-core = { path = "../rollchrome-core", features = ["segment"] }`, and updated paths to moved items in `src/lib.rs`, `src/commands/{edit,auto_masks,bulk_edit,presets}.rs`, and `tests/{local_tools,bulk_edit_desktop}.rs`.
6. **Updated root `Cargo.toml` and docs:** Added `"crates/rollchrome-core"` to workspace `members` and `[profile.dev.package.rollchrome-core] opt-level = 3` in root `Cargo.toml`, updated the Gates block in `CLAUDE.md`, and updated the status line and RC-3 bullet in `docs/rollchrome-plan.md`.

#### Deviations, visibility widenings, and manifest additions

- **Deviations from the brief:** None. All 32 tests from `media_tests.rs` and 31 tests from `tools_tests.rs` landed on the exact sides listed in the brief (`901` → `713` + `188`).
- **Visibility widenings in `phototools-core` (`pub(crate)` → `pub`):**
  - `crates/core/src/ingest/scanner.rs:280`: `pub(crate) fn hex(bytes: &[u8]) -> String` → `pub fn hex(bytes: &[u8]) -> String` (as expected by the brief). No other visibility changes were needed.
- **Dependencies added to `crates/rollchrome-core/Cargo.toml` not explicitly named in the brief's `[dev-dependencies]` list:**
  - `tiff = "0.11.3"` in `[dev-dependencies]` (same version as `crates/core/Cargo.toml`), required because the shared `../../core/tests/fixtures/mod.rs` references `tiff::encoder::TiffEncoder` and `tiff::encoder::colortype::RGB8`. No new crate entered `Cargo.lock`.

#### Gate outputs

1. `cargo fmt --all --check`
   ```text
   (no output, exit code 0)
   ```
2. `cargo clippy --workspace --all-targets -- -D warnings`
   ```text
   Checking rollchrome-core v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-core)
   Checking phototools-desktop v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/desktop)
   Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.52s
   ```
3. `cargo build --workspace`
   ```text
   Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 00s
   ```
4. `cargo test --workspace` — **1011 passed, 0 failed, 0 ignored**:
   ```text
   Running unittests src/lib.rs (phototools_core):       518 passed; 0 failed; 0 ignored
   Running tests/geotag_tests.rs:                         11 passed; 0 failed; 0 ignored
   Running tests/handoff_tests.rs:                         8 passed; 0 failed; 0 ignored
   Running tests/image_tools_tests.rs:                    24 passed; 0 failed; 0 ignored
   Running tests/ingest_tests.rs:                         16 passed; 0 failed; 0 ignored
   Running tests/media_tests.rs (phototools_core):        44 passed; 0 failed; 0 ignored
   Running tests/publish_tests.rs:                        37 passed; 0 failed; 0 ignored
   Running tests/raw_tests.rs:                            12 passed; 0 failed; 0 ignored
   Running tests/tools_tests.rs (phototools_core):        33 passed; 0 failed; 0 ignored
   Running tests/validation_tests.rs:                     10 passed; 0 failed; 0 ignored
   Running unittests src/lib.rs (phototools_desktop):     19 passed; 0 failed; 0 ignored
   Running unittests src/main.rs (phototools_desktop):     0 passed; 0 failed; 0 ignored
   Running tests/bulk_edit_desktop.rs:                     4 passed; 0 failed; 0 ignored
   Running tests/local_tools.rs:                          16 passed; 0 failed; 0 ignored
   Running unittests src/lib.rs (phototools_server):      24 passed; 0 failed; 0 ignored
   Running unittests src/main.rs (phototools_server):      0 passed; 0 failed; 0 ignored
   Running tests/api_tests.rs:                            41 passed; 0 failed; 0 ignored
   Running tests/web_serving_tests.rs:                     6 passed; 0 failed; 0 ignored
   Running unittests src/lib.rs (rollchrome_core):       115 passed; 0 failed; 0 ignored
   Running tests/bulk_edit_tests.rs:                      10 passed; 0 failed; 0 ignored
   Running tests/media_tests.rs (rollchrome_core):        32 passed; 0 failed; 0 ignored
   Running tests/tools_tests.rs (rollchrome_core):        31 passed; 0 failed; 0 ignored
   Doc-tests (all 4 crates):                               0 passed; 0 failed; 0 ignored
   ```
5. `cargo test -p phototools-core` — **713 passed, 0 failed, 0 ignored**:
   ```text
   Running unittests src/lib.rs:       518 passed; 0 failed; 0 ignored; finished in 1.06s
   Running tests/geotag_tests.rs:       11 passed; 0 failed; 0 ignored; finished in 0.42s
   Running tests/handoff_tests.rs:       8 passed; 0 failed; 0 ignored; finished in 0.05s
   Running tests/image_tools_tests.rs:  24 passed; 0 failed; 0 ignored; finished in 1.54s
   Running tests/ingest_tests.rs:       16 passed; 0 failed; 0 ignored; finished in 0.34s
   Running tests/media_tests.rs:        44 passed; 0 failed; 0 ignored; finished in 3.43s
   Running tests/publish_tests.rs:      37 passed; 0 failed; 0 ignored; finished in 0.27s
   Running tests/raw_tests.rs:          12 passed; 0 failed; 0 ignored; finished in 0.75s
   Running tests/tools_tests.rs:        33 passed; 0 failed; 0 ignored; finished in 2.45s
   Running tests/validation_tests.rs:   10 passed; 0 failed; 0 ignored; finished in 1.46s
   Doc-tests phototools_core:            0 passed; 0 failed; 0 ignored
   ```
6. `cargo test -p rollchrome-core` — **188 passed, 0 failed, 0 ignored**:
   ```text
   Running unittests src/lib.rs:       115 passed; 0 failed; 0 ignored; finished in 0.13s
   Running tests/bulk_edit_tests.rs:    10 passed; 0 failed; 0 ignored; finished in 0.78s
   Running tests/media_tests.rs:        32 passed; 0 failed; 0 ignored; finished in 15.66s
   Running tests/tools_tests.rs:        31 passed; 0 failed; 0 ignored; finished in 0.83s
   Doc-tests rollchrome_core:            0 passed; 0 failed; 0 ignored
   ```
7. `cargo build -p rollchrome-core --example auto_mask --features segment`
   ```text
   Compiling rollchrome-core v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-core)
   Finished `dev` profile [unoptimized + debuginfo] target(s) in 21.12s
   ```
8. Boundary & dependency checks:
   - `grep -rnE "media::(edit|segment|histogram)|tools::(edit|lut|lut_library|presets|bulk_edit)\b" crates/core`: no matches.
   - `cargo tree -p phototools-core -e normal --prefix none | grep -E "^ort(-sys)? "`: no matches.
   - `cargo tree -p rollchrome-core -e normal --features segment --prefix none | grep -E "^ort "`: `ort v2.0.0-rc.13`.
   - `grep -rn "rollchrome" crates/core crates/server`: matches only comments in `crates/core/src/sidecar.rs:4` and `crates/core/tests/tools_tests.rs:920` (from the brief's `plant_sidecar` doc comment).
   - `git diff main --stat -- crates/server frontend .github SPECIFICATION.md`: empty.
   - `git diff main -- Cargo.lock`: adds only the `rollchrome-core` package entry and updates `phototools-core` / `phototools-desktop` dependency lists.
9. Front-end gates:
   - `npm --prefix frontend/shared run build`: `tsc` (0 errors).
   - `npm --prefix frontend/web run typecheck`: `vue-tsc --build --force` (0 errors).
   - `npm --prefix frontend/web run build`: `✓ 104 modules transformed. ✓ built in 1.19s`.
   - `npm --prefix frontend/web run check:transport`: `transport check: 35 files, no view reaches the network directly`.
   - `npm --prefix frontend/web run check:layout`: `All 12 routes clean at 390x844. Screenshots in layout-proof/`.
   - `npm --prefix frontend/web run check:ingest`:
     ```text
     first paint with 400 shots: 569 ms
     rows built into the DOM: 29 of 400
     filter to one failure class: 7.4 ms, 29 rows built
     scrolling: 12.8 ms per frame
     shots covered by that one press: 360
     ingest check: 400 shots render responsively, one press bulk-resizes them, and publish is gated
     ```
   - `npm --prefix frontend/desktop run typecheck`: `vue-tsc --build --force` (0 errors).
   - `npm --prefix frontend/desktop run build`: `✓ 129 modules transformed. ✓ built in 1.32s`.
   - `npm --prefix frontend/desktop run check:transport`: `transport check: 39 files, no view reaches the network directly`.
   - `npm --prefix frontend/desktop run check:edit`:
     ```text
     Drag frames: min=8.4 ms, median=10.1 ms, p95=11.8 ms (budget <= 50 ms)
     Settle frames: min=36.7 ms, median=41.0 ms, p95=46.1 ms (budget <= 120 ms)
     Latest-wins: issued 2 renders for 100 fast inputs (pixel1=208)
     --- check:edit Summary ---
     PASSED all checks.
     ```
   - `npm --prefix frontend/desktop run check:bulk-lut`:
     ```text
     Rendered 3 sample frame cards in preview strip.
     Stub metrics: maxConcurrentSessions=1, samplePreviewsRequested=3
     check:bulk-lut PASSED cleanly: all acceptance assertions and benchmarks verified.
     ```

#### Optional & unverified

- Optional `cargo tauri dev` manual check: not run. All automated Rust and Chromium gates were run and verified.

---

## Review

*Written by the architect, after re-running the gates on the branch.*

**Approved** — or **Changes requested:**

1. <specific, actionable, one change per item>
