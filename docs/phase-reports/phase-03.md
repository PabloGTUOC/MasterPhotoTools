## Phase 3 — Archive tools, part 1

**Status:** complete

### Delivered

- **Task 1 — F9 library browser.** Was already close to specification. Verified and given the
  end-to-end G6 tests the phase asks for: browsing outside a root, via `..`, and via a symlink that
  escapes are all rejected *through the tool*, not just at `Config::resolve`.

  One real fix: the listing used `DirEntry::metadata()`, which is `lstat` and succeeds for a symlink
  whose target is gone, so a broken link was listed as a file the caller could not open. It now uses
  `fs::metadata`, so an entry that cannot be read is skipped as F9 requires.

- **Task 2 — F1 date scan.** Rewritten. Classification is unchanged in shape but the results now
  carry which of the seven tags supplied the date (`EXIF:DateTimeOriginal` rather than a `Debug`
  rendering) and **which filesystem timestamp was used**. Metadata reads are parallelised with
  `rayon`; the §9.1 target is 500 files in under five seconds and the reads are independent.

  A file whose metadata date cannot be compared against anything is `Mismatch`, not `Ok` — an
  unverifiable date is not a good one.

- **Task 3 — F1 repair.** All four modes go through `plan`/`apply`.

  `RepairMode::Shift` carried an `i64` of **seconds**, which cannot express the specification's
  `+1:0:0 0:0:0` month and year offsets. It is now a delta string with a real parser, `ShiftDelta`,
  which validates the form, rejects per-field signs as ambiguous, and can **apply the delta in the
  plan** so a dry run states the resulting date before anything is written. `apply` then hands the
  delta to `ExifWriter::shift_dates` so exiftool does the per-tag arithmetic rather than the tool
  writing one absolute stamp over every tag.

  A malformed delta now fails the whole plan rather than silently skipping every file.

- **Task 4 — platform-dependent filesystem timestamps.** This was the sharpest gap. The previous
  code branched on platform and, on Linux, returned `metadata.modified()` **labelled as a creation
  time**. That is reporting an outcome that was not verified.

  Now `FsTime` carries a `FsTimeSource` of `Created` or `Modified`, `birth_time_is_settable()` is a
  `const fn` on the platform, and `apply` **verifies every write by reading the file back**:

  - `DateRepairOutcome` has `metadata_verified` and `filesystem_verified`, both set from a re-read,
    never from the fact a command was issued.
  - On a platform with no settable birth time the outcome carries an explicit note saying only the
    modification time was changed.
  - `DateRepairSummary::fully_verified()` is true only when every file was written *and* confirmed.

  `Summary` was `()`; it is now that report.

- **Task 5 — F2 Takeout sidecars.** Was single-file only with a hard-coded truncation guess.
  Rewritten around a candidate list — exact name, duplicate suffix moved onto the sidecar, duplicate
  suffix dropped, and the truncated form of each — tried in order, first hit wins. Added
  `scan_sidecars` for recursive folder operation, which the specification requires and which did not
  exist. `creationTime` is read as a fallback when `photoTakenTime` is absent.

- **Task 6 — F3 batch rename.** Prefix assembly, sanitising, padding and the plan/apply split were
  already correct. Three fixes:

  - **`capture` ordering never read metadata** — it sorted by filesystem time only. It now sorts by
    best metadata datetime, falling back to modification time, then filename, exactly as specified.
    Keys are computed in parallel, since that is the expensive part of a large batch.
  - **A missing input consumed a sequence number**, leaving gaps. Missing files are now filtered
    before numbering.
  - **A file listed twice got two sequence numbers**, and the second rename would fail with its
    source already moved. Duplicate sources are now detected by canonical path and skipped.

  `apply` re-checks the target immediately before renaming: `fs::rename` replaces its target
  silently on Unix, and a plan may be minutes old. `Summary` is now a real report of what moved and
  what failed.

### Acceptance

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — 124 passed, 0 failed
- [x] `cargo test -p phototools-core` (G2 isolation) — 118 passed, 0 failed
- [x] **F1 `shift`: a 2019 fixture shifted by `+5:0:0 0:0:0` reads back as 2024.**
      `f1_shift_moves_a_2019_fixture_to_2024`, which also asserts the *plan* predicted 2024 before
      anything was written.
- [x] **F2: all sidecar-naming variants resolve.** `f2_every_takeout_naming_variant_resolves` covers
      the exact name, a suffix on the sidecar, a suffix only on the media file, and a truncated long
      name.
- [x] **F2: a missing sidecar is reported, not fatal.**
      `f2_a_missing_sidecar_is_reported_not_fatal` scans a folder holding one of each and asserts
      both are reported with the unresolved one flagged.
- [x] **F3: a collision test proving no file is ever overwritten.**
      `f3_a_collision_is_skipped_and_the_existing_file_is_untouched` puts a file at the name the
      first rename would claim, runs plan *and* apply, and asserts its contents are still
      `PRECIOUS`.
- [x] **Every tool: `plan` makes no filesystem modification, asserted by hashing the directory
      before and after.** `planning_never_touches_the_filesystem` hashes every file's name and bytes,
      runs all four F1 modes plus F3's plan plus both scans, and asserts the hash is unchanged.
- [~] **F1: for each of the seven tags, a fixture where that tag is the highest available resolves
      to it.** Met at three of seven positions at file level; all seven are covered exhaustively by
      `each_position_in_the_preference_order_wins_when_it_is_highest` in `media::meta`. See
      Deviations.

Core tests went from 78 to 118.

### Measurements

Phase 3 specifies no benchmarks. The §9.1 target for a date scan (500 files in under five seconds)
is not yet measured — the scan is parallelised for it, but a fixture at that scale belongs with the
Phase 8 performance work, where the 400-shot card fixture is also built.

### Gates

None. This phase has no external dependency.

### Deviations

1. **The seven-tag file-level acceptance is met at three positions, not seven.** This is the
   `nom-exif` limitation reported in Phase 2: the crate exposes `EXIF:DateTimeOriginal`,
   `EXIF:CreateDate` and one collapsed QuickTime `CreateDate`, and does not surface
   `QuickTime:CreationDate`, `Keys:CreationDate`, `XMP:CreateDate` or `QuickTime:ModifyDate`
   separately. All seven positions are exercised exhaustively as a unit test over the resolver, and
   the three reachable positions are also tested end-to-end on real files. Already recorded in
   `docs/manual-verification.md`.
2. **Touched `frontend/shared/src/index.ts`** — one line. `RepairMode::Shift` changed from a number
   of seconds to a delta string, and leaving the shared client's type saying `number` would have
   been a defect introduced by this phase.
3. **Removed three helpers I had drafted into F1** (`median_date`, `date_from_modification_time`,
   `format_stamp`). They belong to F12/F13 in Phase 9, and keeping them here would be inventing
   scope (G11).

### Added to manual-verification.md

Nothing new. Phase 2's entry on the four unreachable date tags already covers the one gap this phase
inherits.

### Notes for the next phase

- **Phase 4 has its fixtures.** `half_frame_scan` returns the exact divider column so F4's detector
  can be checked against ground truth, and `multipage_tiff` / `tiff_with_alpha` cover F8's two
  paths. The Phase 2 slice primitives (`scan_border_inward`, `darkest_column`, `trim_dark_edges`)
  are what F4 steps 1, 2 and 4 and F7 step 1 should be built from — they are tested and ready.
- **`plan()` in F4, F6, F7 and F8 currently calls `create_dir_all`**, which breaks §7's "dry run
  never touches disk". Phase 4 owns fixing that, and the
  `planning_never_touches_the_filesystem` test in `tools_tests.rs` is the pattern to extend — add
  those four tools to it.
- **`Config::resolve` still requires the path to exist**, which is exactly what blocks validating an
  `out_dir` that has not been created yet. Phase 4 will hit this immediately. The clean fix is a
  variant that canonicalises the nearest existing ancestor and checks that, then joins the remainder.
- **F7's canvas rule cannot be expressed with the current parameters.** It takes `long_edge` and
  `short_edge` from the caller, so no single pair yields both 3000×3750 portrait and 3000×2400
  landscape. The specification fixes those numbers; the parameters should encode the rule, not the
  dimensions.
- **F8 does not implement multi-page TIFF at all** — the code comment concedes it and falls back to
  page one via `image::open`. The `tiff` crate is already a dependency and unused.
