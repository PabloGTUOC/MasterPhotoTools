# `DV-1` · The neutral development, in `rollchrome-core`

> **Status:** briefed
> **Plan:** [`docs/develop-plan.md`](../develop-plan.md), step `DV-1` · **Branch:** `dv-1`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

*Written by the architect. **Self-contained**: the implementer has none of the architect's context,
so anything not written here does not exist for it.*

### Read first

- `CLAUDE.md`: **Ground rules** (G1–G11), **Gates**, *Shape*, *House style*.
- `docs/develop-plan.md`: *Why a RAW needs a development of its own first*, *The pipeline, per
  photograph* steps 1–3, *Things that will catch you*, and the `DV-1` step. The plan's line
  *"rawler has X-Trans demosaicing … called directly"* is **wrong**, and this brief replaces it
  (see *Goal*).
- `docs/rollchrome-plan.md`: rule **R2** (no path-based API) and **R5**.
- `crates/rollchrome-core/src/media/edit/pipeline.rs`, for conventions only: `phototools_core::error::Error`,
  rayon, and the doc-comment style.
- `crates/core/src/media/raw.rs`, the `RawlerDecode` rung. Rawler is called inside `catch_unwind`
  there, because it panics on malformed input. Do the same.

### Goal

Rollchrome's Develop screen (DV-2 onwards) turns a RAW file into a finished photograph with a film
look. Everything it does stands on this step: **the RAW's sensor data, developed neutrally, at 16
bits, never from the camera's embedded preview.** The preview has the camera's own look baked in,
so a film look would be applied on top of another one. It is also only 8 bits.

The function takes the bytes of a RAW file and an exposure offset, and returns a 16-bit image in
**ProPhoto RGB**. That is the working space for the rest of Develop, chosen so that no colour is
squeezed into sRGB before the encode needs it to be. The function also returns a linear float
stage, for the tests and for DV-2.

**rawler 0.7.2 has no X-Trans demosaic**, the colour filter of Fujifilm cameras. It decodes the
6×6 pattern correctly, but `imgop/sensor/xtrans/xtrans_markenstein.rs` is an empty file, and
`imgop/sensor/xtrans/mod.rs` declares no module, so `xtrans_fast.rs` is not compiled. rawler's own
`RawDevelop::develop_intermediate` sends X-Trans data through PPG, a Bayer algorithm. So this step
**writes its own X-Trans demosaic**, from the description in this brief only. It copies or
translates no code from dcraw, LibRaw, darktable, RawTherapee, Filmulator or rawler's
`xtrans_fast.rs`. Those are GPL or LGPL, this repository has no licence, and Rollchrome may one day
be distributed. **Bayer** sensors use rawler's public `PPGDemosaic`, called directly.

No screen and no Tauri command come with this step, and no file is written by the library.

### Change

**1. Dependency.** Add `rawler = "0.7.2"` to `[dependencies]` in `crates/rollchrome-core/Cargo.toml`.
It is the same version `phototools-core` already uses, so `Cargo.lock` gains one line in
`rollchrome-core`'s list and no new crate. Record it in the Report under **G8**: this step
demosaics and colour-converts sensor data itself, which `phototools-core` has no API for. Add no
other dependency.

**2. Module `crates/rollchrome-core/src/media/develop/`**, declared `pub mod develop;` in
`media/mod.rs`. Four files:

- **`mod.rs`**: the public API and the pipeline.

  ```rust
  pub struct DevelopOptions { pub exposure_ev: f32 }          // Default: 0.0
  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
  pub enum Demosaic { Bayer, XTrans }
  pub struct LinearProPhoto {                                 // linear, scene-referred
      pub width: u32, pub height: u32,
      pub pixels: Vec<[f32; 3]>,                              // row-major, ProPhoto RGB primaries
      pub orientation: u16,                                   // EXIF 1–8, as rawler reports it
      pub demosaic: Demosaic,
  }
  pub struct Developed {
      pub image: image::ImageBuffer<image::Rgb<u16>, Vec<u16>>, // ProPhoto RGB, ROMM-encoded
      pub orientation: u16,
      pub demosaic: Demosaic,
  }
  pub fn develop_linear(raw: &[u8], options: &DevelopOptions) -> Result<LinearProPhoto, Error>;
  pub fn develop(raw: &[u8], options: &DevelopOptions) -> Result<Developed, Error>;
  pub fn prophoto_to_srgb8(developed: &Developed) -> image::RgbImage;
  ```

  `Error` is `phototools_core::error::Error`. A file Develop cannot handle is `Error::Refused`
  with a reason a person can read. A rawler panic is `Error::Internal`. Bytes in, image out (R2):
  no `Path` anywhere in the module's API.

  **`develop_linear`, in this order:**

  1. Decode the bytes with `rawler::decode(&RawSource::new_from_slice(raw), &RawDecodeParams::default())`,
     inside `catch_unwind`. If rawler fails, refuse with its message.
  2. Accept only `RawPhotometricInterpretation::Cfa` with `cpp == 1`, and a CFA that is either a
     2×2 R/G/B pattern (→ `Demosaic::Bayer`) or a 6×6 R/G/B pattern (→ `Demosaic::XTrans`). Refuse
     everything else, naming what was found: linear DNG, monochrome, four-colour, other sizes.
     **Never** fall back to an embedded preview. An output made from the preview would look
     plausible and be wrong (G10).
  3. `RawImage::apply_scaling()`. The data is then f32, from 0 (black level) to 1 (white level).
  4. Demosaic over the active area (`active_area`, or the whole frame if there is none). The CFA
     must be shifted for the area's origin, as rawler's `develop_intermediate` does. Bayer goes
     through `rawler::imgop::sensor::bayer::ppg::PPGDemosaic`, X-Trans through `xtrans.rs`.
  5. **White balance as shot**: `wb_coeffs[0..3]`. If any is NaN or ≤ 0, refuse ("no white
     balance recorded"). Normalise the three so that **the smallest is 1.0**, multiply, then clamp
     each channel to **at most 1.0**. The clamp is what keeps clipped highlights neutral. A pixel
     clipped on the sensor is 1.0 in every channel before white balance, and every coefficient is
     ≥ 1, so after the clamp it is (1, 1, 1) again rather than magenta. Say so in the comment.
  6. **Camera to linear ProPhoto** (`colour.rs`), with the matrix for `Illuminant::D65` if there is
     one, otherwise the first one. No matrix means refuse. Then clamp negatives to 0.
  7. **Default crop**: `crop_area` adapted to the active area, as rawler's `develop_intermediate`
     does. Output dimensions are the crop's.
  8. Multiply by `2^exposure_ev`. `exposure_ev` outside **−4.0..=4.0**, or not finite, is refused.

  **`develop`** = `develop_linear`, then per channel `tone::base_tone`, then
  `tone::prophoto_encode`, then `round(x * 65535)` to u16. Parallel over rows with rayon.

  **`prophoto_to_srgb8`**: for viewing, and for DV-2's JPEG later. ROMM-decode, ProPhoto (D50)
  → XYZ → Bradford D50→D65 → linear sRGB, clamp to 0..1, the sRGB transfer function, round to u8.
  Colours outside sRGB are clipped. Say so in the doc comment.

- **`xtrans.rs`**: `pub(crate) fn demosaic(pixels: &rawler::pixarray::PixF32, cfa: &rawler::CFA, roi: rawler::imgop::Rect) -> Vec<[f32; 3]>`,
  returning `roi.width() * roi.height()` pixels in the CFA's channel order (0 = R, 1 = G, 2 = B).
  Read colours through `cfa.color_at(row, col)`. The algorithm:

  1. **Green everywhere.** Green pixels keep their value. At each red or blue pixel, estimate the
     missing green in four directions: horizontal, vertical and the two diagonals. Each estimate
     is linear interpolation between the nearest green sample on either side along that
     direction, within 3 pixels. Skip a direction with no green on one side. Give each direction
     a **gradient**: the absolute difference of its two greens, plus the absolute difference of
     the nearest same-colour samples on either side, if there are any within 3 pixels. Take the
     mean of the estimates whose gradient is within 10% of the smallest gradient, or within
     `1e-6` of it when the smallest is near zero. Interpolating along an edge rather than across it
     is what prevents the maze pattern.
  2. **Red and blue from colour differences.** At every pixel missing red, take the colour
     difference `R − G` at each red sample in the 5×5 window centred on the pixel. Weight each by
     the inverse of its distance, plus the inverse of `1 + |G_sample − G_centre| * 10`, so that a
     sample across an edge counts for less. Add the weighted mean back to the pixel's green. Blue
     is the same with `B − G`. X-Trans guarantees red and blue in every 5×5 window.
  3. Near the frame's edge, use only the samples that exist. Never index out of bounds, and never
     write zeros.

  The module doc explains the method and why, as above. It says in one line that it was written
  from this description, and names the licences that rule out the alternatives.

- **`colour.rs`**: matrices computed **from published primaries in code**, not typed in as
  opaque constants.
  - ProPhoto (ROMM): R (0.7347, 0.2653), G (0.1596, 0.8404), B (0.0366, 0.0001), white D50
    (0.3457, 0.3585).
  - sRGB: R (0.64, 0.33), G (0.30, 0.60), B (0.15, 0.06), white D65 (0.3127, 0.3290).
  - The standard Bradford cone matrix.

  `camera_to_prophoto(xyz2cam: [[f32;3];3]) -> [[f32;3];3]`, the dcraw-style construction rawler
  uses for sRGB in `imgop/raw.rs::map_3ch_to_rgb`, with ProPhoto in place of sRGB:

  1. `rgb2cam = xyz2cam · PROPHOTO_TO_XYZ_D65`. That is ProPhoto→XYZ(D50) followed by Bradford
     D50→D65, because DNG colour matrices are relative to D65 here.
  2. Normalise each row of `rgb2cam` to sum to 1, so that white-balanced camera white maps to
     ProPhoto (1, 1, 1).
  3. Invert it. The 3×3 case needs no pseudo-inverse. A singular matrix is refused.

  Keep rawler's flat matrix layout in mind: `color_matrix` holds 9 values, row by row, XYZ →
  camera.

- **`tone.rs`**: the **base tone**, one fixed and documented transform from linear to display.
  It is pinned by a test, because DV-3's built-in looks will be fitted against exactly this curve,
  and a quiet change would put every look off.

  ```
  k = 0.6,  c = 2 / (1 + k) = 1.25,  A = c / (2 (1 − k)) = 1.5625
  base_tone(v) = c·v                 for v ≤ k
               = 1 − A·(1 − v)²      for k < v < 1
               = 1                   for v ≥ 1        (and 0 for v ≤ 0)
  ```

  It is linear with a gentle lift (+0.32 EV) up to 0.6, then a quadratic shoulder that meets the
  line in value and slope and reaches 1.0, with zero slope, exactly at the sensor's clip point. So a
  clipped sky is white, approached softly, and never a flat grey. Write the comment explaining
  those three properties.

  `prophoto_encode(v)`, the ROMM transfer function: `16·v` for `v < 1/512`, otherwise
  `v^(1/1.8)`, clamped to 0..1. `prophoto_decode` is its inverse.

**3. Tests: exactly these 24, names as given.** The step is measured by them.

Unit tests, in their modules:

| # | Module | Test | Asserts |
|---|---|---|---|
| 1 | xtrans | `a_flat_colour_field_comes_back_unchanged_through_the_xtrans_demosaic` | mosaic a flat (0.2, 0.5, 0.3) 48×48 through the Fujifilm X-Trans pattern below; every output pixel within **1e-5** of it, edges included |
| 2 | xtrans | `the_demosaic_is_the_same_at_every_offset_of_the_pattern` | test 1 at all 36 shifts of the pattern (`cfa.shift`): same result |
| 3 | xtrans | `a_smooth_ramp_comes_back_within_half_a_percent` | a horizontal and a vertical linear ramp 0.05→0.95, 96×96, every channel; every pixel ≥ 3 px from the border within **0.005** |
| 4 | xtrans | `a_fine_grating_comes_back_without_a_maze` | a sinusoidal grating, period 8 px, 0.3 ± 0.2, every channel in phase, 96×96, at 0° and 90°: RMS error per channel, ≥ 4 px from the border, **≤ 0.006** (3% of the amplitude) |
| 5 | xtrans | `a_fine_diagonal_grating_comes_back_without_a_maze` | as 4 at 45°: **≤ 0.010** (5%) |
| 6 | xtrans | `a_step_edge_leaves_the_far_field_untouched` | a step from (0.2, 0.5, 0.3) to (0.6, 0.4, 0.1), vertical, horizontal and 45°: every pixel more than 3 px from the edge and the border within **0.002** of its side's colour |
| 7 | tone | `the_base_tone_is_pinned` | `base_tone` at 0, 0.18, 0.6, 0.8, 1.0, 2.0 equals 0, 0.225, 0.75, 0.9375, 1, 1, within **1e-6** |
| 8 | tone | `the_base_tone_meets_its_shoulder_smoothly` | value and finite-difference slope either side of 0.6 agree within **1e-4**; slope just below 1.0 < 0.01 |
| 9 | tone | `the_base_tone_never_decreases` | monotonic over 0..4 in steps of 1e-3 |
| 10 | tone | `prophoto_encoding_follows_romm_and_round_trips` | 16·v below 1/512, v^(1/1.8) above; continuous at 1/512 within 1e-4; decode(encode(v)) within 1e-6 over 0..1 |
| 11 | colour | `the_prophoto_matrix_matches_the_published_one` | ProPhoto→XYZ(D50) from primaries equals 0.7976749 0.1351917 0.0313534 / 0.2880402 0.7118741 0.0000857 / 0 0 0.8252100 within **1e-4** |
| 12 | colour | `a_neutral_under_the_shot_white_balance_develops_to_equal_channels` | with the test camera matrix below, `camera_to_prophoto` maps (1, 1, 1) to (1, 1, 1) and (0.18, 0.18, 0.18) to equal channels, within **1e-5** |
| 13 | colour | `prophoto_to_srgb_maps_white_grey_and_an_in_gamut_colour` | ProPhoto white → sRGB (255, 255, 255); linear grey 0.18 → sRGB 118 ± 1; a ProPhoto colour made from sRGB (200, 80, 40) comes back as (200, 80, 40) ± 1 |

Integration tests in a new `crates/rollchrome-core/tests/develop_tests.rs`, with a DNG writer in
`crates/rollchrome-core/tests/support/dng.rs` (see below):

| # | Test | Asserts |
|---|---|---|
| 14 | `a_handmade_bayer_dng_develops_to_its_known_grey_and_patches` | an RGGB DNG 96×96: a grey field 0.18 and four colour patches given as linear ProPhoto values, turned into sensor values by the inverse of the pipeline (camera matrix, then divide by the white-balance coefficients, then black and white level). Choose patches whose sensor values all land between 5% and 80% of the range, and list them in the test. `develop_linear` returns them within **1%** relative at each patch's centre (≥ 4 px inside it); `demosaic == Bayer` |
| 15 | `a_handmade_xtrans_dng_develops_to_its_known_grey_and_patches` | as 14 with the X-Trans pattern; `demosaic == XTrans` |
| 16 | `the_output_keeps_more_than_256_levels_of_a_smooth_gradient` | an X-Trans DNG 2048×16, 14-bit, a horizontal grey ramp from 0.02 to 0.9 linear: `develop` gives **≥ 1024 distinct** green values along the middle row |
| 17 | `clipped_highlights_stay_neutral` | X-Trans and Bayer DNGs with AsShotNeutral (0.5, 1, 0.7) and a patch at the white level in every channel: in `develop`, R = G = B = 65535 at its centre; the surround, a 0.18 grey, has equal channels within 1% |
| 18 | `an_exposure_offset_of_one_stop_doubles_the_linear_values` | `develop_linear` at +1 EV over 0 EV on test 14's file: ratio 2 within **0.1%** at the grey; −1 EV halves it |
| 19 | `an_exposure_outside_four_stops_is_refused` | ±4.5 EV and NaN → `Error::Refused` |
| 20 | `bytes_that_are_not_a_raw_are_refused_with_a_reason` | a small JPEG (built with `image`) and 100 random bytes → `Err`, never a panic, never `Ok` |
| 21 | `a_dng_with_an_unsupported_layout_is_refused_rather_than_previewed` | a DNG with `PhotometricInterpretation` 34892 (LinearRaw, three samples) → `Error::Refused` naming the layout |
| 22 | `the_output_takes_the_default_crop` | a DNG with `ActiveArea` and `DefaultCropOrigin`/`DefaultCropSize`: output dimensions equal the crop size |
| 23 | `the_orientation_is_reported_not_applied` | a DNG with `Orientation` 6 (90° CW): `orientation == 6`, and width and height are the unrotated ones |
| 24 | `develop_never_reads_the_embedded_preview` | a Bayer DNG whose IFD0 is an 8-bit RGB preview of solid red (NewSubFileType 1) with the raw in a SubIFD of solid grey: the output is grey (R, G, B within 1% of each other), not red |

**Test data, fixed so that results are reproducible:**

- The X-Trans pattern, rows top to bottom, as Fujifilm X-Trans III: `GGRGGB` `GGBGGR` `BRGRBG`
  `GGBGGR` `GGRGGB` `RBGBRG`. Check it with `rawler::CFA::new` on those 36 letters. Every 3×3
  block must hold all three colours, so assert that in a helper.
- The test camera matrix (XYZ → camera, D65): `[[0.9, -0.3, -0.1], [-0.4, 1.2, 0.2], [-0.05, 0.15, 0.7]]`,
  together with `CalibrationIlluminant1` = 21 (D65).
- The DNG writer in `tests/support/dng.rs`: little-endian TIFF, uncompressed 16-bit strips. Tags:
  NewSubFileType, ImageWidth, ImageLength, BitsPerSample, Compression 1, PhotometricInterpretation
  32803, StripOffsets, SamplesPerPixel 1, RowsPerStrip, StripByteCounts, PlanarConfiguration 1,
  CFARepeatPatternDim, CFAPattern, DNGVersion 1.4.0.0, UniqueCameraModel, Make, Model,
  BlackLevel, WhiteLevel, ColorMatrix1, CalibrationIlluminant1, AsShotNeutral, and optionally
  Orientation, ActiveArea, DefaultCropOrigin, DefaultCropSize and a SubIFD (test 24).
  **First, before anything else, prove that rawler decodes one such file** (dimensions, CFA,
  levels, white balance), and say in the Report which tags turned out to be required. If rawler
  wants something not listed, add it and say so.
- Black level 512, white level 16383 (14-bit), except where a test says otherwise.

**4. The measuring tool, `crates/rollchrome-core/examples/develop.rs`.** It includes
`tests/support/dng.rs` with `#[path]`, and is thin: argument parsing, timing and file writing, with
the logic left in the library.

- `cargo run --release -p rollchrome-core --example develop -- --synthetic <W>x<H> <bayer|xtrans>`
  builds a DNG of that size in memory: noise, so that it is not trivially compressible, plus a few
  patches. It prints the time of `develop`, excluding building the DNG, as the median of three
  runs.
- `… --example develop -- <out dir> <raw files…>` reads each file, develops it, and writes
  `<out dir>/<stem>.developed.png` through `prophoto_to_srgb8`. It prints the time, the demosaic
  used and the dimensions, or the refusal. This is what the owner will run on real files.

Run the synthetic mode at **6000x4000** (24 MP) and **7728x5152** (40 MP), Bayer and X-Trans, and
put all four medians in the Report. **No budget is set yet.** The architect sets one from these
numbers, and no test asserts a time.

**5. Find out about lens corrections, and record what you find.** Does rawler 0.7.2 apply a DNG's
`OpcodeList1/2/3`, or any Fujifilm lens-correction data, during decode? Read its source (the DNG
decoder copies the opcode lists in `decoders/dng.rs` around line 138) and state the answer, with
file and line, in the Report. Change nothing because of it.

### Do not touch

- `crates/core/` entirely, including `media::raw` and its `RawlerDecode` rung. F14 and
  publishing keep the camera's look. PhotoTools' rawler fallback also sends X-Trans files
  through PPG; that is recorded separately and is not this step's job.
- `crates/rollchrome-core/src/media/edit/` and `tools/`. Edit's and Batch Grade's RAW path stays
  the embedded preview (develop-plan, decision 1).
- `crates/rollchrome-desktop/`, every front end, CI, `SPECIFICATION.md` (G9), `CLAUDE.md`,
  and every document except the Report section of this file.
- Any existing test (G7).
- Tempting and out of scope (G11): highlight *reconstruction* beyond the clamp; noise reduction;
  sharpening; lens corrections; an ICC profile or a TIFF/JPEG encoder in the library (DV-2); LUTs
  (DV-2); a Tauri command; a cache; SIMD or `multiversion`. Put ideas in the Report.

### Acceptance

- [ ] **Baseline first**, on `dv-1` as cut: workspace **1012**, `phototools-core` **713**,
      `rollchrome-core` **189**, `phototools-desktop` **26**, `rollchrome-desktop` **13**,
      `phototools-server` **71**. If any differs, stop and report.
- [ ] **Counts after:** `rollchrome-core` **213** (189 + 24), workspace **1035**, all others
      unchanged; 0 failed, 0 ignored. The 24 tests carry exactly the names above.
- [ ] Every threshold in the table is as written. **If the X-Trans demosaic cannot meet tests 4–6,
      do not loosen them.** Stop, put the measured numbers in the Report, and the architect
      decides.
- [ ] `git diff main --stat -- crates/core crates/rollchrome-desktop crates/server frontend .github SPECIFICATION.md CLAUDE.md`
      is empty. `Cargo.lock` changes only by `rawler` in `rollchrome-core`'s list.
- [ ] `git grep -n "Path" -- crates/rollchrome-core/src/media/develop` finds nothing (R2).
- [ ] `git grep -nE "unwrap\(\)|expect\(|todo!|unimplemented!" -- crates/rollchrome-core/src/media/develop`
      finds nothing outside `#[cfg(test)]` (G10).
- [ ] `git grep -n "embedded\|preview" -- crates/rollchrome-core/src/media/develop` shows only
      comments saying it is never used.
- [ ] The Report carries: the tags rawler required; the four timing medians; the lens-correction
      answer with file and line; the measured values of tests 4, 5 and 6 (the actual RMS and
      maximum errors, not only that they passed).

### Gates

Run them in this order, and **never run `cargo test` at the same time as a browser check**.

```sh
cargo fmt --all --check                                  # no output
cargo clippy --workspace --all-targets -- -D warnings    # no warnings
cargo build --workspace                                  # ok
cargo test --workspace                                   # 1035 passed
cargo test -p phototools-core                            # 713 passed — G2
cargo test -p rollchrome-core                            # 213 passed — G2
cargo test -p phototools-desktop                         # 26 passed
cargo test -p rollchrome-desktop                         # 13 passed
cargo run --release -p rollchrome-core --example develop -- --synthetic 6000x4000 xtrans
cargo run --release -p rollchrome-core --example develop -- --synthetic 6000x4000 bayer
cargo run --release -p rollchrome-core --example develop -- --synthetic 7728x5152 xtrans
cargo run --release -p rollchrome-core --example develop -- --synthetic 7728x5152 bayer
```

No front end changes, so the front-end gates are not needed. Run `npm --prefix frontend/rollchrome run typecheck`
once anyway, to show it is untouched. If `cargo` is not on `PATH`, run `. "$HOME/.cargo/env"` first.

### Report back

In **Report** below:

- what you did;
- anything that deviates from this brief, and why. A deviation not listed is a finding;
- the **actual output** of every gate, with counts;
- everything the last Acceptance item lists;
- anything you could not verify, said plainly. On a real camera file, everything is unverified
  until the owner runs MV-23.

Do not tick the acceptance boxes above; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*

**Approved** — or **Changes requested:**

1. <specific, actionable, one change per item>
