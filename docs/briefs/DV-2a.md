# `DV-2a` · From a developed RAW to a finished file: the look and the encode

> **Status:** **approved** (2026-11-06)
> **Plan:** [`docs/develop-plan.md`](../develop-plan.md), step `DV-2`, first half · **Branch:** `dv-2a`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

*Written by the architect. **Self-contained**: the implementer has none of the architect's context,
so anything not written here does not exist for it.*

### Read first

- `CLAUDE.md`: **Ground rules** (G1–G11), **Gates**, *House style*.
- `docs/develop-plan.md`: *What it does*, *Decisions* (3 especially), *The pipeline, per
  photograph* steps 4–5, and *Looks and the TIFF's colour*.
- `docs/briefs/DV-1.md`, the step before this. Its Brief describes the module you extend:
  `crates/rollchrome-core/src/media/develop/` (`Developed`, `tone::prophoto_encode` and
  `prophoto_decode`, `colour.rs`'s matrices, `prophoto_to_srgb8`).
- `crates/rollchrome-core/src/media/edit/lut.rs`: `Lut` and `Lut::sample`, which takes
  display-encoded sRGB in 0..1.
- `crates/core/src/media/jpeg.rs`: `encode` and `JpegOptions::deliverable`, the encoder every
  finished JPEG in this repository goes through.

### Goal

The develop plan's step DV-2 is split in two. **This half (DV-2a)** turns a developed RAW
(`Developed`, from DV-1) into the bytes of a finished file, with or without a film look. **DV-2b**
does the folder, the plan, never overwriting, the metadata and the Tauri commands.

Two outputs (develop-plan decision 3):

- **JPEG**, 8-bit **sRGB**: the photograph to share.
- **TIFF**, 16-bit **ProPhoto RGB** *with its ICC profile embedded*, for further editing in
  Photomator. Without the profile, Photomator reads ProPhoto numbers as sRGB, and every colour comes
  out dull. So the profile must describe exactly the encoding DV-1 writes.

A **look** is a 3D LUT from the library. A `.cube` cannot say which colour space it expects, and
nearly every one is built for sRGB, so Develop feeds it **sRGB**. Applied to the TIFF, that
confines the colours to sRGB's inside a ProPhoto file. That is faithful to the LUT and no wider, and
it is the intended behaviour (develop plan, *Looks and the TIFF's colour*). Without a look, the TIFF
keeps everything DV-1 developed.

Bytes in, bytes out: no `Path` in this API (Rollchrome plan, rule R2).

### Change

**1. Dependencies.** In `crates/rollchrome-core/Cargo.toml`, `[dependencies]`:

- `moxcms = "0.8.1"`, to build the ICC profile;
- `tiff = "0.11.3"`, moved up from `[dev-dependencies]` (keep it there too only if Cargo
  requires it).

Both are already compiled into the build through `image`, so `Cargo.lock` gains edges in
`rollchrome-core`'s list and no new crate. Record both under **G8** in the Report:

- **`moxcms`** builds the ProPhoto profile with a tone curve that matches DV-1's encoding.
  Its own `new_pro_photo_rgb()` uses a pure 1.8 gamma, which differs in the deep shadows.
- **`tiff`**: `image`'s `TiffEncoder` has no compression, so a 24 MP 16-bit file would be 144 MB.
  `tiff` writes Deflate and an ICC tag.

Add nothing else.

**2. `crates/rollchrome-core/src/media/develop/render.rs`**, declared in `develop/mod.rs`, with its
public items re-exported from `develop`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat { Jpeg { quality: u8 }, Tiff }
pub struct Rendered { pub bytes: Vec<u8>, pub width: u32, pub height: u32, pub extension: &'static str } // "jpg" | "tif"
pub fn render(developed: &Developed, look: Option<&Lut>, format: OutputFormat) -> Result<Rendered, Error>;
pub fn prophoto_icc_profile() -> Result<Vec<u8>, Error>;
```

`Error` is `phototools_core::error::Error`; a refusal is `Error::Refused` with a readable reason.

The colour path for each pixel, in f32 throughout, rayon over rows:

- **No look, TIFF:** the developed u16 pixels, unchanged.
- **No look, JPEG:** ROMM-decode → linear ProPhoto → linear sRGB (DV-1's matrix) → clamp 0..1 →
  sRGB encode → round to u8. This must give the same result as DV-1's `prophoto_to_srgb8`: reuse
  it or share its code, but never have two copies of the conversion.
- **With a look, either format:** ROMM-decode → linear sRGB → clamp 0..1 → sRGB encode →
  `lut.sample` → the LUT's output, sRGB-encoded, clamped 0..1. Then:
  - **JPEG:** round to u8.
  - **TIFF:** sRGB decode → linear sRGB → linear ProPhoto (the inverse matrix, computed from
    DV-1's in `colour.rs`, not typed in) → `prophoto_encode` → round to u16.

Then encode:

- **JPEG:** `phototools_core::media::jpeg::encode(&DynamicImage::ImageRgb8(..), &JpegOptions::deliverable(quality))`.
  A `quality` outside 1..=100 is refused. No ICC profile: an untagged JPEG is read as sRGB, and the
  metadata (DV-2b) will say sRGB.
- **TIFF:** with the `tiff` crate:
  - RGB, 16 bits per sample, chunky;
  - **Deflate** compression, plus the horizontal predictor if `tiff` 0.11.3 supports it for 16-bit.
    Say in the Report which;
  - tag **34675 (InterColorProfile)** carrying `prophoto_icc_profile()`.

**`prophoto_icc_profile()`** builds the profile with `moxcms`:

- ProPhoto's colorants (D50), as `ColorProfile::new_pro_photo_rgb()` sets them;
- red, green and blue TRCs **replaced** by the ICC parametric curve type 3, matching DV-1's
  `prophoto_encode` exactly: `g = 1.8, a = 1, b = 0, c = 1/16, d = 1/32`, i.e. `Y = X^1.8` for
  `X ≥ 1/32` and `Y = X/16` below;
- description `"ProPhoto RGB (ROMM)"`, the copyright as moxcms sets it.

Encode it with `ColorProfile::encode`. Check `moxcms`'s `ToneReprCurve::Parametric` for the
parameter order it expects, and say in the Report what you found.

**3. Tests: exactly these 9**, as unit tests in `render.rs` unless noted. They need `Developed`
values built in code. Construct them directly, since the fields are public, so no RAW is needed.

| # | Test | Asserts |
|---|---|---|
| 1 | `the_tiff_is_16_bit_prophoto_and_keeps_the_developed_pixels_without_a_look` | decode the bytes with `tiff`: RGB, 16-bit, Deflate, the same dimensions, and **every pixel identical** to `developed.image` |
| 2 | `the_tiff_carries_a_profile_that_decodes_as_its_own_encoding` | read tag 34675 back with `tiff`, parse with `moxcms::ColorProfile::new_from_slice`: colorants within **1e-3** of ProPhoto's. Evaluating its red TRC at X = 0.5 gives 0.5^1.8, and at X = 1/64 gives (1/64)/16, both within **1e-4** |
| 3 | `the_jpeg_without_a_look_matches_the_srgb_conversion` | decode the JPEG (`image`): each of four flat 32×32 patches (a grey, a skin tone, a sky blue, a foliage green, all inside sRGB) is within **±2** of `prophoto_to_srgb8` at the patch centre |
| 4 | `an_identity_look_leaves_colours_inside_srgb_unchanged` | a 17³ identity `.cube` built in the test (`Lut::from_cube`): TIFF pixels within **±64/65535** of the no-look TIFF for the four patches of test 3, and the JPEG within **±1** before encoding (compare the u8 buffer, by exposing a `pub(crate)` helper that `render` also uses) |
| 5 | `a_look_confines_the_tiff_to_srgb_colours` | a saturated ProPhoto green outside sRGB, through the identity look, as TIFF: converted back to linear sRGB, every component within **−1e-3..=1+1e-3**. Without the look, the same pixel converts to a component below −0.05 |
| 6 | `a_look_is_applied` | a 17³ `.cube` that inverts (out = 1 − in): a mid grey ProPhoto pixel whose sRGB value is s comes back in the JPEG as 1 − s, within **±2** |
| 7 | `a_jpeg_quality_outside_1_to_100_is_refused` | 0 and 101 → `Error::Refused`; 1 and 100 → `Ok` |
| 8 | `the_extension_follows_the_format` | `"jpg"` and `"tif"`; dimensions equal the developed image's |
| 9 | `a_developed_raw_renders_end_to_end`, in `crates/rollchrome-core/tests/develop_tests.rs` | one handmade X-Trans DNG from DV-1's `tests/support/dng.rs` → `develop` → `render` as both formats with the inverting look: both decode, the TIFF has its profile, and dimensions match |

### Do not touch

- `develop/mod.rs`, `xtrans.rs`, `tone.rs` and `colour.rs` beyond:
  - declaring and re-exporting `render`;
  - a `pub(crate)` inverse matrix or a shared conversion helper, if test 3's single conversion
    needs one.

  DV-1's 24 tests and their thresholds stay exactly as they are (G7).
- `crates/core/`, `crates/rollchrome-desktop/`, every front end, CI, `SPECIFICATION.md` (G9),
  `CLAUDE.md`, and every document except the Report section of this file.
- Out of scope (G11): folders, file names, writing files, metadata (all DV-2b); a Tauri command;
  an ICC profile in the JPEG; LUTs in other colour spaces; the built-in looks (DV-3).

### Acceptance

- [ ] **Baseline first**, on `dv-2a` as cut: workspace **1036**, `phototools-core` **713**,
      `rollchrome-core` **213**, `phototools-desktop` **26**, `rollchrome-desktop` **13**,
      `phototools-server` **71**. If any differs, stop and report.
- [ ] **After:** `rollchrome-core` **222** (213 + 9), workspace **1045**, all others unchanged; 0
      failed, 0 ignored. Paste the `test result:` lines themselves: a count copied from this
      brief is not evidence.
- [ ] Every threshold is as written. If one cannot be met, stop and report the measured number.
      Do not loosen it.
- [ ] `git diff main --stat -- crates/core crates/rollchrome-desktop crates/server frontend .github SPECIFICATION.md CLAUDE.md`
      is empty. `Cargo.lock` changes only in `rollchrome-core`'s dependency list.
- [ ] `git grep -n "Path" -- crates/rollchrome-core/src/media/develop` finds nothing (R2).
- [ ] No `unwrap()`, `expect(`, `todo!`, `unimplemented!` or `panic!` outside `#[cfg(test)]` (G10).
- [ ] The Report gives:
  - the size of a 6000×4000 TIFF rendered from DV-1's synthetic X-Trans DNG (run the
    `develop` example's synthetic mode, extended to also render a TIFF and a JPEG and print their
    sizes and the render time);
  - the predictor answer;
  - the moxcms parameter-order answer.

### Gates

```sh
cargo fmt --all --check                                  # no output
cargo clippy --workspace --all-targets -- -D warnings    # no warnings
cargo build --workspace                                  # ok
cargo test --workspace                                   # 1045 passed
cargo test -p phototools-core                            # 713 passed — G2
cargo test -p rollchrome-core                            # 222 passed — G2
cargo test -p phototools-desktop                         # 26 passed
cargo test -p rollchrome-desktop                         # 13 passed
cargo run --release -p rollchrome-core --example develop -- --synthetic 6000x4000 xtrans
```

If `cargo` is not on `PATH`, run `. "$HOME/.cargo/env"` first.

### Report back

In **Report** below: what you did; any deviation, and why; the actual `test result:` lines of every
gate; everything the last Acceptance item lists; and what you could not verify, said plainly. Do
not tick the acceptance boxes above; the review does that.

---

## Report

*Written by the implementer.*

### Baseline (recorded on `dv-2a` as cut before changes)
- `phototools-core`: 713 passed
- `rollchrome-core`: 213 passed
- `phototools-desktop`: 26 passed
- `rollchrome-desktop`: 13 passed
- `phototools-server`: 71 passed
- Workspace: 1036 passed (0 failed, 0 ignored)

### Dependencies (G8)
- `moxcms = "0.8.1"`: Added to `crates/rollchrome-core/Cargo.toml` under `[dependencies]` to construct the 16-bit ProPhoto RGB ICC profile. `moxcms`'s standard `new_pro_photo_rgb()` uses a pure 1.8 gamma which diverges in deep shadows; `moxcms` allows replacing the colorant TRCs with ICC parametric curve type 3 ($g=1.8, a=1, b=0, c=1/16, d=1/32$) exactly matching DV-1's ROMM transfer function.
- `tiff = "0.11.3"`: Moved from `[dev-dependencies]` to `[dependencies]` in `crates/rollchrome-core/Cargo.toml` to write 16-bit chunky RGB TIFFs with Deflate compression, horizontal predictor, and the embedded ICC profile tag 34675 (`Tag::IccProfile`).
- No new crates added to workspace (both were already compiled transitively via `image`). `Cargo.lock` only updated `rollchrome-core`'s dependency list.

### Implementation Summary
1. Created `crates/rollchrome-core/src/media/develop/render.rs`:
   - Defined `OutputFormat { Jpeg { quality: u8 }, Tiff }` and `Rendered { bytes: Vec<u8>, width: u32, height: u32, extension: &'static str }`.
   - Implemented `prophoto_icc_profile()` using `moxcms::ColorProfile::new_pro_photo_rgb()` with TRCs replaced by parametric curve type 3 and description `"ProPhoto RGB (ROMM)"`.
   - Implemented pixel colour pipeline with Rayon parallelism:
     - No look, TIFF: 16-bit developed pixels unchanged.
     - No look, JPEG: ROMM decode -> linear ProPhoto -> linear sRGB -> clamp -> sRGB transfer function -> u8.
     - Look: ROMM decode -> linear ProPhoto -> linear sRGB -> clamp -> sRGB transfer function -> `lut.sample`. For JPEG: clamp -> u8. For TIFF: sRGB decode -> linear sRGB -> linear ProPhoto (`srgb_to_prophoto_matrix()`) -> `prophoto_encode` -> u16.
     - Single conversion shared between `render_srgb8_image` and `prophoto_to_srgb8`.
   - JPEG encoding via `phototools_core::media::jpeg::encode` with quality check (`1..=100`, refusing 0 and >100).
   - TIFF encoding via `tiff::encoder::TiffEncoder` with Deflate compression, horizontal predictor, and tag 34675 holding `prophoto_icc_profile()`.
   - Added unit tests 1–8 covering all requirements and thresholds.
2. Updated `crates/rollchrome-core/src/media/develop/mod.rs`:
   - Declared `pub mod render;` and re-exported `OutputFormat`, `Rendered`, `render`, `prophoto_icc_profile`.
   - Updated `prophoto_to_srgb8` to delegate to `render::render_srgb8_image(developed, None)`.
3. Added Test 9 to `crates/rollchrome-core/tests/develop_tests.rs`:
   - `a_developed_raw_renders_end_to_end`: generates handmade X-Trans DNG, develops it, renders with inverting 17³ `.cube` LUT to both JPEG and TIFF, verifies decodes, dimensions, and ICC profile tag 34675.
4. Updated `crates/rollchrome-core/examples/develop.rs`:
   - Extended `run_synthetic` to render TIFF and JPEG and print output sizes and render times.

### Answers to Brief Questions
1. **Predictor support in `tiff` 0.11.3**:
   `tiff` 0.11.3 supports `Predictor::Horizontal` for 16-bit RGB (`colortype::RGB16`). In `tiff` 0.11.3, `RGB16` implements `integer_horizontal_predict!()` for `u16` samples, and `SampleFormat::Uint` passes encoder validation. Round-trip tests confirm exact pixel identity.
2. **moxcms parameter order**:
   `moxcms::ToneReprCurve::Parametric` expects parameters in the order `[g, a, b, c, d]` for ICC type 3 parametric curves ($Y = (aX + b)^g$ for $X \ge d$, $Y = cX$ for $X < d$). For DV-1's ProPhoto encoding: `vec![1.8, 1.0, 0.0, 1.0 / 16.0, 1.0 / 32.0]`.
3. **Synthetic 6000x4000 X-Trans benchmark (`develop --synthetic 6000x4000 xtrans`)**:
   - DNG size: 45.78 MB
   - Develop runs: 5.06s, 4.19s, 3.16s (Median: 4.19s)
   - Rendered TIFF: **0.15 MB** in **148.49ms** (Deflate + Horizontal predictor achieves high compression on flat sensor test pattern)
   - Rendered JPEG: **0.27 MB** in **4.94s**

### Acceptance Checks
- `git diff main --stat -- crates/core crates/rollchrome-desktop crates/server frontend .github SPECIFICATION.md CLAUDE.md`: empty.
- `Cargo.lock`: only `rollchrome-core`'s dependency list updated (`moxcms` added).
- `git grep -n "Path" -- crates/rollchrome-core/src/media/develop`: no matches (rule R2 satisfied).
- `grep -n -E "unwrap\(|expect\(|todo!|unimplemented!|panic!" crates/rollchrome-core/src/media/develop/render.rs`: no matches outside `#[cfg(test)]` (rule G10 satisfied).
- All 9 new tests pass with exact brief thresholds without modification.

### Deviations
None.

### Gate Outputs

#### `cargo fmt --all --check`
No output (clean).

#### `cargo clippy --workspace --all-targets -- -D warnings`
Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.66s. No warnings.

#### `cargo build --workspace`
Finished `dev` profile [unoptimized + debuginfo] target(s) in 23.55s.

#### `cargo test -p phototools-core` (713 passed)
```
test result: ok. 518 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s
test result: ok. 72 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.35s
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.35s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.73s
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.58s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.51s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

#### `cargo test -p rollchrome-core` (222 passed: 213 baseline + 9 new)
```
test result: ok. 137 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.86s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.07s
test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.91s
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.92s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

#### `cargo test -p phototools-desktop` (26 passed)
```
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.05s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.93s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

#### `cargo test -p rollchrome-desktop` (13 passed)
```
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

#### `cargo test -p phototools-server` (71 passed)
```
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.51s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 41 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.86s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.36s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

#### `cargo test --workspace` (1045 passed: 1036 baseline + 9 new)
```
test result: ok. 518 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.14s
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.63s
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.34s
test result: ok. 44 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.41s
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.37s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.74s
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.58s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.50s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.09s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.62s
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.55s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 41 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.92s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.54s
test result: ok. 137 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.29s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.86s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 17.99s
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.90s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

#### `cargo run --release -p rollchrome-core --example develop -- --synthetic 6000x4000 xtrans`
```
Generating synthetic 6000x4000 xtrans raw...
DNG size: 45.78 MB. Developing 3 times...
Run 1: 5.06s
Run 2: 4.19s
Run 3: 3.16s
Median: 4.19s
Rendering 16-bit TIFF and JPEG...
TIFF: 0.15 MB in 148.49ms
JPEG: 0.27 MB in 4.94s
```

#### `npm --prefix frontend/rollchrome run typecheck`
```
> @phototools/rollchrome@0.1.0 typecheck
> vue-tsc --build --force
```
Completed with code 0 (clean).

---

## Review

*Written by the architect, after re-running the gates on the branch.*

**Approved** (2026-11-06).

Gates re-run on `dv-2a` at `96fd142`: fmt and clippy at 0. Tests: workspace **1045**,
`phototools-core` 713, `rollchrome-core` **222**, `phototools-desktop` 26, `rollchrome-desktop` 13,
`phototools-server` 71, all 0 failed and 0 ignored. This time the Report pastes the `test result:`
lines themselves, and they agree with mine.

Checked in the diff:

- DV-1's tests are unchanged; the one addition is test 9.
- `prophoto_to_srgb8` now delegates to `render`'s conversion, so there is one copy of it, as
  asked.
- The ICC curve parameters are `[1.8, 1, 0, 1/16, 1/32]`, and test 2 reads them back through
  moxcms.
- The scope and R2 greps are clean, and `Cargo.lock` gains only edges in `rollchrome-core`.

Two numbers recorded, not acted on:

- **The JPEG takes 4.6 s at 24 MP** (my run), against 0.15 s for the TIFF. That is
  `JpegOptions::deliverable`'s maximum-effort encoder, PhotoTools' deliberate choice for a
  photograph's only encode. Added to the develop time, it nearly doubles a JPEG batch. It is
  weighed when the budget is set after MV-23.3, not before.
- **The 0.15 MB TIFF says nothing about real files.** The example's synthetic DNG is nearly flat,
  although DV-1's brief asked for noise, so Deflate has nothing to do. MV-23 records a real size.
