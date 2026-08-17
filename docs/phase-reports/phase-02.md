## Phase 2 — Media layer

**Status:** complete with gates — **one acceptance criterion is missed and is reported, not relaxed**
(the §9.1 benchmark; see Measurements).

### Delivered

- **Task 1 — the fixture generator (§5).** Was 3 of 8 generators, one of which shelled out to
  `exiftool` and one of which wrote 8-byte text files named `.JPG`. Rewritten with all eight, plus
  helpers the later phases need.

  EXIF is now **written by hand** rather than by subprocess: a minimal little-endian TIFF/IFD writer
  builds IFD0 and an Exif SubIFD and splices the result in as a JPEG APP1. Two reasons — the
  metadata tests must not depend on the tool whose output they validate, and a process per fixture
  makes the suite crawl. Verified against both `exiftool` and `nom-exif`.

  | Generator | Notes |
  |---|---|
  | `jpeg_with_exif` | known size, capture date, camera, and matching pixel-dimension tags |
  | `jpeg_with_tags` | arbitrary tag sets, for priority and normalisation tests |
  | `jpeg_with_orientation` | all eight orientation values |
  | `jpeg_without_exif`, `png` | |
  | `half_frame_scan` | white lab surround, two coloured panels, a dark divider **at a returned known x**, so F4 can be checked against ground truth |
  | `multipage_tiff`, `tiff_with_alpha` | F8's multi-page naming and flatten paths |
  | `raw_stub_with_preview` | TIFF/IFD carrying an embedded JPEG preview; returns the exact preview bytes so F14 can assert byte-identity |
  | `takeout_pair` | all four Takeout naming variants, including both `(1)`-suffix placements |
  | `card_tree` | takes `&[ShotKind]` — real EXIF-carrying JPEGs and RAW stubs, configurable RAW+JPEG pairing |
  | `quicktime` | minimal MOV with an `mvhd` creation time, for the UTC path |

- **Task 2 — `read_meta`.** The preference order was **six entries matched by bare tag name**, so
  `EXIF:CreateDate` and `QuickTime:CreateDate` collided and four of the seven positions were
  unreachable. Rewritten: `TagSource` is now the seven namespaced variants in specification order,
  and resolution is a pure function over a candidate map, so every position is directly testable.

  Normalisation implemented in full — the `0000:00:00 00:00:00` sentinel, timezone suffixes
  (`Z`, `+01:00`, `-05:00`), `YYYY-MM-DD` input, ISO `T` separators, sub-second precision, and
  bare dates as midnight. The previous implementation did `value.replace('-', ":")` across the whole
  string, which turned `2024:05:01 12:00:00-05:00` into an unparseable
  `2024:05:01 12:00:00:05:00`; the timezone scan now starts past the date so a hyphenated date and a
  negative offset can coexist.

  QuickTime timestamps are taken as UTC via `is_utc()`, which is what prevents the double-shift F1
  warns about. Dimensions come from `ExifImageWidth`/`Height` with IFD0's `ImageWidth`/`Height` as
  fallback — read from metadata, never by decoding (F11).

- **Task 3 — `ExifWriter`.** Was a bare pipe with a blocking read and no timeout. Now: a handshake
  on start (`-ver`) that proves framing works before any caller depends on it; a reader thread so a
  hung child surfaces as a timeout instead of a blocked caller; `{ready}` sentinel handling; clean
  shutdown that waits for the child and joins the reader.

  `write_dates` previously wrote **only `-AllDates`**. It now writes F1's full set: images get
  `DateTimeOriginal`, `CreateDate`, `ModifyDate` and `AllDates`; video gets `CreateDate`,
  `ModifyDate`, `MediaCreateDate` and `TrackCreateDate`; both get `FileCreateDate` and
  `FileModifyDate`.

  **`shift_dates` was `todo!()` and is now implemented**, using exiftool's `+=`/`-=` shift syntax —
  the last `todo!()` in `media`, which Phase 2's Definition of Done required removing.

- **Task 4 — decode/encode and orientation.** `Orientation` was a single-variant enum
  (`Normal`) — effectively unimplemented. Now all eight EXIF values, with `swaps_axes()` and
  `apply_orientation`, and `decode_oriented` applies it on load.

- **Task 5 — resize and the quality ladder.** Downscale-only helper (never enlarges),
  `dimensions_for_megapixels` implementing F13's `sqrt` formula exactly, and `encode_jpeg_within`
  stepping `95 → 88 → 82 → 75`. When even 75 overshoots, the caller is told `fits = false` rather
  than being handed a result that silently missed the cap (§9.2 invariant 6).

- **Task 6 — EXIF-preserving re-encode.** New `media/exif_jpeg` module: a JPEG segment walker, APP1
  extraction, TIFF/IFD traversal into the Exif SubIFD, and in-place patching of
  `PixelXDimension`/`PixelYDimension` for both endiannesses and both SHORT and LONG storage. The
  source's block is carried forward **verbatim** and only the dimension tags are rewritten, so lens,
  exposure, GPS and maker notes survive untouched. No subprocess involved. Splicing removes any
  existing Exif APP1 so a file never ends up with two.

- **Task 7 — slice primitives.** New `media/slices`: column and row mean profiles, threshold
  fractions, border-line classification, inward border scanning with a per-side crop cap, darkest
  column with margin and ±window refinement, and dark-edge trimming with a safety inset. All operate
  on row and column slices of a luma buffer (§9.1 rule 3). F4 and F7 now have these to build on
  rather than writing them twice.

- **G4 fix.** `ingest/derivation/worker.rs` called `ExifWriter::start()` **inside the per-job rayon
  closure** — one `exiftool` per file, the exact thing §2.6 prohibits. Restructured into two passes:
  decode/resize/encode run in parallel across the pool, then metadata is carried across serially
  through one writer for the whole batch. Hash and size are measured after that pass, since it
  rewrites the file.

### Acceptance

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — 84 passed, 0 failed
- [x] `cargo test -p phototools-core` (G2 isolation) — 78 passed, 0 failed
- [x] **Single-process assertion.** `writing_fifty_files_spawns_exactly_one_exiftool_process`
      installs a shim that logs one line per spawn and execs the real tool, writes 50 files, and
      asserts the log has exactly one line — then checks all 50 writes actually landed. This needed
      a small API addition, `ExifWriter::start_with`, because the G4 guarantee is otherwise not
      observable from inside the process.
- [x] **The specification-mandatory round trip.**
      `f13_resizing_preserves_exif_and_updates_the_pixel_dimensions` generates a JPEG with a known
      capture date, resizes it 800×600 → 400×300, reads the metadata back, and asserts the capture
      date survived, the camera survived, and the recorded pixel dimensions now read 400×300 rather
      than staying stale.
- [x] **Tag-priority tests.** `each_position_in_the_preference_order_wins_when_it_is_highest` walks
      all seven positions: for each, it populates that tag and every lower one and asserts that tag
      wins. Plus end-to-end file-level tests for the positions a real file can carry, and a test
      that a tag holding the sentinel is skipped in favour of the next.
- [ ] **Benchmark under 150 ms — MISSED.** See below.

Core tests went from 15 to 78.

### Measurements

**24 MP resize and encode: 463 ms measured, against a 150 ms target (§9.1). The target is missed by
about 3×.** Release build, this container.

Not relaxed, per build plan §11. The breakdown, measured separately:

| Stage | Before | After | |
|---|---|---|---|
| Resize 6000×4000 → 3872×2581 | 210 ms | **129 ms** | optimised this phase |
| JPEG encode at q95 | 345 ms | 345 ms | unchanged |

The resize improvement came from borrowing the existing pixel buffer via `fast_image_resize`'s
`ImageRef` instead of materialising an RGB8 copy — a 24 MP frame costs ~70 ms and 72 MB to clone
before any scaling starts, which was half the §9.1 budget spent on a memcpy. Output is bit-identical.

**The remaining cost is the JPEG encoder, at 345 ms of the 463 ms.** That is the `image` crate's
pure-Rust encoder, which is what specification §2.6 names. Reaching 150 ms end-to-end needs a
different encoder — `mozjpeg`/`turbojpeg` bindings are typically 5–10× faster here — and that is a
new dependency, which G8 says to report before adding rather than substitute silently. **This needs a
decision.** Options, in the order I would suggest them:

1. Add `mozjpeg` (libjpeg-turbo bindings) for encoding only. Almost certainly meets the target;
   costs a C dependency in the build and the Docker image.
2. Accept a lower quality rung or a faster resize filter. `fast_image_resize` defaults to Lanczos3;
   CatmullRom would cut convolution cost meaningfully. This trades image quality for speed and
   should be a deliberate choice, not mine to make.
3. Relax §9.1's figure. It may simply have been optimistic for a pure-Rust pipeline.

The benchmark asserts only in release builds, where the number means something; debug builds print
it without asserting. **`cargo test --release` therefore currently fails on this one test, by
design** — it is the visible signal that the target is unmet. CI runs debug and is green.

### Gates

- The decision above on the JPEG encoder.
- Real photographs, for the visual and metadata checks now listed in `docs/manual-verification.md`.

### Deviations

1. **`nom-exif` does not expose four of F1's seven tags.** It surfaces `EXIF:DateTimeOriginal`,
   `EXIF:CreateDate`, and a single collapsed QuickTime `CreateDate`. `QuickTime:CreationDate`,
   `Keys:CreationDate`, `XMP:CreateDate` and `QuickTime:ModifyDate` are not separately available.
   The full order is implemented and unit-tested at all seven positions; positions 3, 5, 6 and 7
   simply cannot be populated from a file today. Reported per §11 rather than quietly reducing the
   order to what the crate supports. Added to `docs/manual-verification.md`.
2. **Touched a module outside `media`** to fix the G4 violation in `ingest/derivation/worker.rs`.
   G4 names the persistent driver as Phase 2's, and shipping a phase that builds the driver while
   leaving its only caller violating the rule seemed worse than the scope bend.
3. **Two shared test files were updated** (`ingest_tests.rs`, `derivation_tests.rs`) because
   `card_tree`'s signature changed from `u32` to `&[ShotKind]`. Their intent is unchanged.
4. **`ExifWriter::start_with` is new API added for testability.** Without it the single-process
   guarantee cannot be asserted from inside the process. It also gave the timeout path a test.

### Added to manual-verification.md

Three Phase 2 entries: the four unreachable date tags, hand-built EXIF fixtures needing a check
against real camera files, and EXIF preservation needing visual confirmation on a real photograph
plus the §6.4 Google Photos date check.

### Notes for the next phase

- **Phase 3 has everything it needs.** `ExifWriter::shift_dates` is implemented and tested with
  exactly Phase 3's acceptance case (a 2019 fixture shifted `+5:0:0 0:0:0` reads back as 2024), and
  `takeout_pair` covers all four sidecar-naming variants F2 must tolerate.
- **F1's `RepairMode::Shift(i64)` carries seconds and should become a delta string.** Seconds cannot
  express the `+1:0:0 0:0:0` month/year offsets the specification uses, and `shift_dates` now takes
  the correct form.
- **`Config::resolve` still requires the path to exist**, so F4/F6/F7/F8's `out_dir` handling needs a
  create-path variant before G6 can be wired into the tools. Unchanged from Phase 1's note.
- **`plan()` currently creates directories in F4, F6, F7 and F8**, breaking §7's "dry run never
  touches disk". Phase 3 and 4 own that.
- **A latent scanner quirk found while testing**: `Scanner::scan` applies its junk filter to the walk
  root itself, so scanning any directory whose own name starts with `.` returns nothing. Harmless for
  `/Volumes/EOS_DIGITAL`, but Phase 8 should fix it — simulated card mode (§6.3) may well point at a
  dotted path.
