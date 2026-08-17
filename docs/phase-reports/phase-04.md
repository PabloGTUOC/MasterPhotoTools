## Phase 4 — Archive tools, part 2

**Status:** complete with one gate — the JPEG encoder cannot express the chroma subsampling and
progressive options three of these requirements specify. Reported, not worked around.

### Delivered

All five tools were rewritten. Every one of them previously called `create_dir_all` inside `plan`,
breaking §7's "dry run never touches disk"; none does now.

- **Task 1 — F6 transform.** The order of operations is now the specification's: EXIF orientation
  first, then rotate, then downscale, then convert. Previously orientation was never applied at all,
  and **rotation silently ignored any angle that was not 90/180/270** — a request to rotate 45° went
  through as a no-op. `rotate_expanding` now handles arbitrary angles with bilinear resampling and
  an expanded canvas, right angles keeping their exact fast paths.

  F6 also now accepts directories as well as files, which the specification asks for and the old
  signature could not express. `.heic`/`.heif` are accepted by the specification but the `image`
  crate has no decoder for them, so they are reported as skipped with a reason rather than failing
  obscurely.

- **Task 2 — F8 TIFF to JPEG.** **Multi-page was not implemented at all** — the old code said so in
  a comment and fell back to page one via `image::open`, so `{base}_p001.jpg` naming did not exist.
  It now drives the `tiff` crate directly (a dependency that was declared and unused), decoding
  every page and handling RGB8, RGBA8, Gray8, GrayA8 and 16-bit RGB/Gray, narrowing 16-bit scanner
  output rather than refusing it. An unsupported colour type is reported, not guessed at.

- **Task 3 — F5 contact sheet.** The height formula was **wrong**: it omitted the `label_height`
  term because captions did not exist. Captions are now implemented — a 30 px label strip, font size
  `max(10, cell_size × 0.04)`, and the 28-character truncation to `name[:18] + "..." + extension`.
  Also added: background choice with the caption colour inverting to match, and sort by filename or
  modification date. The red crossed box was already there and now has a test.

  Rendering text needed a font. Rather than add a font crate and ship a typeface (G8), `media/text`
  provides a built-in 5×7 bitmap font. See Deviations.

- **Task 4 — F7 print border.** The canvas rule **could not be expressed** by the old parameters: it
  took `long_edge` and `short_edge` from the caller, so no single pair yields both 3000×3750 portrait
  and 3000×2400 landscape. `canvas_for()` now encodes the rule itself. Dark-edge trimming (step 1)
  did not exist and now uses the Phase 2 `trim_dark_edges` primitive with the specification's
  thresholds. The margin was 5% of the canvas and is now a minimum of 50 px with the image enlarged
  to fill the space. The corner radius was a caller parameter and is now 2% of the image's short
  side, antialiased by a 4× supersampled mask — the old code's comment conceded the antialiasing
  "could go here".

- **Task 5 — F4 half-frame split.** Previously one darkest-column search and nothing else: **three
  of the four stages were missing**, all seven parameters were absent, the search margin was
  hard-coded to 0.35–0.65 instead of 0.20, and output was `_1`/`_2` keeping the source extension.

  Now the full procedure on the Phase 2 slice primitives: lab-border removal via
  `scan_border_inward`, divider location via `column_mean_profile` + `darkest_column` with the
  ±window refinement, the split, then residual dark-band trimming with a landscape half rotated
  upright and any excess beyond 10% of ratio 24/17 removed from the bottom only. All seven
  parameters are `SplitSettings` with the specification's defaults, and `ratio` is configurable for
  other cameras. Output is `{base}_A.jpg` / `{base}_B.jpg`. Preview mode, which did not exist,
  returns the border-cropped whole image plus both halves and writes nothing.

- **`Config::resolve_for_create`.** Flagged in Phases 1 and 3 as the blocker for validating an
  output directory: `resolve` canonicalises, which fails on a path that does not exist yet. The new
  variant resolves the nearest existing ancestor, checks *that* against the roots, and re-appends
  the remainder, rejecting any `..` in it so the check cannot be walked back out of.

### Acceptance

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — 174 passed, 0 failed
- [x] `cargo test -p phototools-core` (G2 isolation) — 168 passed, 0 failed
- [x] **F5: a sheet from 9 fixtures where the 5th is corrupt.** Output is 1340×1110, matching the
      formulas (`4×300 + 3×20 + 2×40` and `3×(300+30) + 2×20 + 2×40`), the sheet is produced
      despite the bad file, and the 5th cell is scanned pixel by pixel for red — over 100 red pixels
      found.
- [x] **F7: portrait yields exactly 3000×3750, landscape exactly 3000×2400, with at least 50 px of
      white on every side.** The margin assertion walks every pixel of all four 50 px bands, and
      also checks the centre is *not* white so the photograph is actually present.
- [x] **F4: on `half_frame_scan` with a divider at a known column**, the detected split is within
      8 px of it, both halves are portrait, and both are within 10% of ratio 24/17.
- [x] **`plan` makes no filesystem modification.** Extended to all five tools, asserting both that
      the tree hash is unchanged *and* that the output directory was never created.
- [x] **Appended to `docs/manual-verification.md`**: F4 and F7 need a human on real scans, with what
      specifically to look at.

Core tests went from 118 to 168.

### Measurements

Phase 4 specifies no benchmarks.

### Gates

**The JPEG encoder cannot express three requirements' output options.** The `image` crate's encoder
is fixed at **4:2:2** subsampling with no progressive mode and no way to change either:

| Requirement | Specified | Actual |
|---|---|---|
| F4 | quality 95, **no chroma subsampling** | quality 95, 4:2:2 |
| F7 | quality 95, **no chroma subsampling** | quality 95, 4:2:2 |
| F8 | quality 90, **4:2:0**, **progressive**, optimised | quality 90, 4:2:2, baseline |

This is the same root cause as the Phase 2 benchmark miss, and it strengthens that phase's
recommendation: a real JPEG encoder (`mozjpeg`/`turbojpeg`) would fix the 463 ms-versus-150 ms
performance gap *and* these three output specs at once. It is a new dependency, so G8 says report
before substituting. `image_ops::ENCODER_CHROMA_SUBSAMPLING` records the gap at the point of use so
it is visible in the code, not only here.

Also outstanding: real photographs, for the F4 and F7 judgements now listed in
`docs/manual-verification.md`.

### Deviations

1. **F5 captions use a built-in 5×7 bitmap font** (`media/text`) rather than a real typeface.
   Specification §2.6 lists no font library, and adding one plus a shipped font file would be two
   new dependencies under G8. The font is legible at caption sizes, which is what F5 asks of it, but
   it is not typographically good. Added to `docs/manual-verification.md`.
2. **`media/text` is a new module** under `media`, which is where it belongs — it is the only module
   permitted to touch image bytes — but it is not in §2.5's sketch.
3. **F6's `optimise` flag is accepted and stored but has no effect**, because the encoder exposes no
   optimisation setting. It is not silently dropped from the API, since the flag becomes meaningful
   the moment the encoder question above is settled.

### Added to manual-verification.md

Four Phase 4 entries: F4 and F7 on real scans with specifics on what to check, the chroma
subsampling and progressive gap, and the bitmap font's legibility.

### Notes for the next phase

- **Phase 5 can wire all nine tools now.** Every tool has a real `Summary` type, `plan`/`apply` is
  uniform, and `expand_inputs` gives handlers one way to accept files or directories.
- **`Config::resolve_for_create` is what output-directory validation needs.** Phase 5 task 5 should
  route input paths through `Config::resolve` and output paths through `resolve_for_create`. Neither
  is called by any handler yet — G6 is still not wired at the API boundary, which remains the
  highest-severity open item in the repository.
- **The tools do not resolve paths themselves**, by design: they take resolved paths. That keeps the
  policy decision at the boundary, but it means a handler that forgets to resolve has no second line
  of defence. Phase 5's path-traversal test is what proves the boundary holds.
- **`f5_contact::thumbnail` decodes and scales one file at a time.** A 200-image sheet is the §9.1
  target at under 20 seconds; parallelising the decode with `rayon` is the obvious move if it misses.
