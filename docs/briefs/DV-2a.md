# `DV-2a` · From a developed RAW to a finished file: the look and the encode

> **Status:** briefed
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

---

## Review

*Written by the architect, after re-running the gates on the branch.*

**Approved** — or **Changes requested:**

1. <specific, actionable, one change per item>
