# `RP-2` · Edit and Batch read developed files correctly

> **Status:** briefed
> **Plan:** [`docs/rollchrome/parity-plan.md`](../rollchrome/parity-plan.md), step `RP-2` · **Branch:** `rp-2`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11) and **Gates**.
- `docs/rollchrome/parity-plan.md` — *Three tabs*, and what it says this model corrects.
- `crates/rollchrome-core/src/media/edit/pipeline.rs::decode_image`, `media/edit/preview.rs::open`.
- `crates/rollchrome-core/src/tools/edit.rs` (`render_and_write`, export), `tools/lut.rs`, `tools/bulk_edit.rs`.
- `crates/rollchrome-core/src/media/develop/render.rs::prophoto_icc_profile`, for how `moxcms` is used.
- `crates/rollchrome-core/src/tools/develop_folder.rs::carry_metadata`, for the colour tag corrections.
- `crates/core/src/tools/mod.rs::carry_metadata_with` and `media/meta.rs::{copy_metadata_to_derivative, set_tags}`.

### Goal

Develop writes 16-bit TIFFs in ProPhoto RGB with an embedded ICC profile. Edit and Batch ignore embedded
profiles and take every pixel for sRGB, so such a TIFF opens in Edit with muted, wrong colours, and
that error is then baked into whatever is exported. The owner's workflow is RAW → Develop → Edit or
Batch, so this must be right before anything else builds on it. **The fix is to convert at the one point
where files are decoded.** Edit's pipeline stays in sRGB. Colours beyond sRGB clip when such a file is opened
in Edit; that is accepted and recorded in the plan.

The second half is the metadata. Copying tags across from a ProPhoto or Adobe RGB source onto a converted
sRGB output would make the output lie about its own colours.

### Change

**`crates/rollchrome-core/src/media/edit/icc.rs`** (new; declared in `media/edit/mod.rs`):

```rust
/// Returns `buffer` in sRGB, given the profile embedded in the file it came from.
pub fn to_srgb(buffer: ImageBuffer, icc: Option<&[u8]>) -> Result<ImageBuffer, Error>;
```

The rules, in this order:

1. **No profile:** unchanged, as today.
2. **A profile `moxcms` cannot parse:** `Error::Refused`, saying the file's colour profile could not be read and
   that its colours would be wrong. Today such a file opens with wrong colours and nobody is told.
3. **A profile whose data colour space is not RGB** (a greyscale JPEG's grey profile, say): unchanged. `image` has
   already expanded those pixels to RGB, so the RGB transform does not apply and the difference is negligible.
4. **A profile that behaves as sRGB:** unchanged, returning the same pixels, so that the many sRGB-tagged camera
   JPEGs are not re-quantised. Decide this by behaviour, not by name or bytes, since sRGB profiles differ in their
   encoding: build the transform and run a 17×17×17 grid of 8-bit colours through it; if every output is within
   1 level of its input, it is sRGB.
5. **Anything else:** convert to sRGB with `moxcms`, relative colorimetric. **Keep the bit depth:** `Rgb8` stays
   `Rgb8` and `Rgb16` stays `Rgb16`, because `render_and_write` chooses TIFF or JPEG from the buffer's depth.

**`crates/rollchrome-core/src/media/edit/pipeline.rs::decode_image`:** on the non-RAW branch, get the decoder with
`ImageReader::into_decoder`, read `ImageDecoder::icc_profile`, build the `ImageBuffer` as now, and pass it through
`to_srgb`. If reading the profile itself fails, that is rule 2. The RAW branch does not change. Every Edit preview,
Edit export and Batch run goes through this one function; do not add a second path.

**`crates/core/src/tools/mod.rs`:** add

```rust
pub fn carry_metadata_correcting(
    derived: &[Derived], upright: bool, program: Option<&str>, corrections: &[String],
) -> Vec<Skip>;
```

which is `carry_metadata_with` followed by `set_tags` with `corrections` on each output, **on the same `ExifWriter`** (G4:
one exiftool process per batch). `carry_metadata_with` delegates to it with no corrections and behaves exactly as now. A failed
correction is a `Skip` for that output, as a failed copy is.

**`tools/edit.rs`, `tools/lut.rs`, `tools/bulk_edit.rs`:** call it where they call `carry_metadata`/`carry_metadata_with`, with these
corrections, on every output, since every output of these tools is sRGB whatever its source:

```
-ExifIFD:ColorSpace#=1
-InteropIFD:InteropIndex=
-ICC_Profile=
```

Put the list in one place in `rollchrome-core`, with a comment saying why. The identity byte-copy in `export_edited_image` does not
change: an untouched copy is still ProPhoto and still says so.

### Do not touch

- `SPECIFICATION.md` (G9).
- Which files Edit and Batch accept (`ACCEPTED`, the RAW branch): that question is with the owner.
- `media/develop/**`, `media/looks/**`, every `.cube`. Develop's own `carry_metadata`.
- The adjustment pipeline, the preview's timing budget, and `check:edit`. No front-end change is needed.
- No new dependency: `moxcms` is already one of `rollchrome-core`'s (G8).

### Acceptance

- [ ] `rollchrome-core` gains exactly these nine tests, in a new `crates/rollchrome-core/tests/icc_tests.rs` unless a helper forces
  otherwise:
  - `a_developed_tiff_opened_for_editing_matches_the_developed_jpeg_within_two_levels`: one `Developed` of uniform colour patches
    including saturated ones inside sRGB, rendered with `render` to TIFF and to JPEG, with no look and with `builtin:fujifilm-provia`. The TIFF
    through `decode_image`, reduced to 8 bits, against the JPEG decoded. **This test is the claim of this step.**
  - `an_srgb_tagged_jpeg_decodes_to_the_same_pixels_as_an_untagged_one`
  - `an_adobe_rgb_file_is_converted_to_the_expected_srgb_values`: expected values computed in the test from the published
    matrices, not from `moxcms`.
  - `a_conversion_keeps_the_bit_depth_of_the_source`: an 8-bit JPEG and a 16-bit TIFF.
  - `a_file_with_an_unreadable_profile_is_refused_and_says_why`
  - `a_greyscale_file_with_a_grey_profile_still_opens`
  - `an_edit_export_of_a_prophoto_tiff_says_srgb_and_carries_no_prophoto_profile`: real exiftool; read the output's tags back.
  - `a_batch_output_of_a_prophoto_tiff_says_srgb_and_carries_no_prophoto_profile`: the same through the bulk LUT tool.
  - `correcting_metadata_for_a_batch_uses_one_exiftool_process`: three outputs, a shim counting starts, as
    `develop_folder_tests.rs::one_exiftool_process_carries_the_whole_batch` does.
- [ ] Counts: workspace 1093 → 1102, `rollchrome-core` 264 → 273; `phototools-core` 713, `phototools-desktop` 26 and
  `rollchrome-desktop` 19 unchanged. Every existing test still there, none weakened (G7).
- [ ] No `unwrap()`, `todo!()` or swallowed error on a shipped path (G10).
- [ ] `CLAUDE.md` gate counts and the plan's status line describe the result.

### Gates

Run `. "$HOME/.cargo/env"` first.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace             # 1102
cargo test -p phototools-core      # 713
cargo test -p rollchrome-core      # 273
cargo test -p phototools-desktop   # 26
cargo test -p rollchrome-desktop   # 19
npm --prefix frontend/rollchrome run typecheck
npm --prefix frontend/rollchrome run check:edit
npm --prefix frontend/rollchrome run check:bulk-lut
```

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why; the **actual output** of every gate, counts and
failures, never just "passed"; and anything you could not verify, said plainly. Develop a real RAW from
`~/Desktop/Projects/rollchrome-chart-sources/` to TIFF and give the seconds `decode_image` takes on it in release mode, with and without
the conversion. Never add those files to the repository. Do not tick the acceptance boxes; the review does that.

---

## Report

### What was done

1. **`crates/rollchrome-core/src/media/edit/icc.rs`:**
   - Implemented `to_srgb(buffer: ImageBuffer, icc: Option<&[u8]>) -> Result<ImageBuffer, Error>` obeying the five rules in order:
     1. No profile: untouched.
     2. Unparseable / malformed / truncated profile: `Error::Refused("The file's colour profile could not be read; colours would be wrong.")`.
     3. Non-RGB data colour space (e.g. greyscale): untouched.
     4. Profile behaving as sRGB: evaluated on a 17×17×17 grid of 8-bit colours; if every channel is within 1 level, returns the buffer untouched without requantisation.
     5. Anything else: converted to sRGB via `moxcms` relative colorimetric transform, keeping the bit depth (`Rgb8` stays `Rgb8`, `Rgb16` stays `Rgb16`).
   - Defined `SRGB_METADATA_CORREctions` and `srgb_metadata_corrections()` containing `["-ExifIFD:ColorSpace#=1", "-InteropIFD:InteropIndex=", "-ICC_Profile="]` with explanatory documentation.

2. **Single decoding point in `crates/rollchrome-core/src/media/edit/pipeline.rs`:**
   - In `decode_image`, non-RAW files read the embedded ICC profile and pass the decoded `ImageBuffer` through `to_srgb`.
   - For TIFF files, the ICC profile is extracted using `tiff::decoder::Decoder::new` to circumvent an upstream limit-calculation bug in `image::codecs::tiff::TiffDecoder::set_limits` (see deviations below).

3. **Metadata correction on derivatives in `crates/core/src/tools/mod.rs`:**
   - Added `carry_metadata_correcting(derived, upright, program, corrections)` which performs `copy_metadata_to_derivative` and `set_tags` with `corrections` on the exact same `ExifWriter` process (preserving G4: one exiftool process per batch).
   - Delegated `carry_metadata_with` to `carry_metadata_correcting` with empty corrections.
   - Connected `export_edited_image` in `tools/edit.rs` to pass `srgb_metadata_corrections()` on edited exports. The untouched byte-copy path remains untouched.
   - Connected `BulkEditTool` (`tools/bulk_edit.rs`) and `BulkLutTool` (`tools/lut.rs`) to pass `srgb_metadata_corrections()`.

4. **Acceptance tests in `crates/rollchrome-core/tests/icc_tests.rs`:**
   - Implemented all 9 required acceptance tests covering round-trip developed TIFF matching JPEG within 2 levels, sRGB pass-through, Adobe RGB mathematical conversion, 8-bit/16-bit depth preservation, malformed profile refusal, greyscale opening, single-process metadata correction, and ExifTool tag verification for Edit and Batch exports.

### Deviations

1. **TIFF ICC profile extraction in `decode_image`:**
   When `image::ImageReader::into_decoder()` is called, `image` (v0.25.10) executes `decoder.set_limits(self.limits)`. In `image::codecs::tiff::TiffDecoder::set_limits`, `decoding_buffer_size` is calculated as `(max_alloc - max_intermediate_alloc)` where `max_intermediate_alloc = max_alloc.saturating_sub(total_bytes_buffer)`. This reduces to `total_bytes_buffer` (e.g. 48 bytes for a 4×4 image), which causes `TiffDecoder::icc_profile()` to fail with `LimitsExceeded` and return `None`. In `decode_image`, TIFF files extract the profile tag via `tiff::decoder::Decoder::get_tag_u8_vec(Tag::IccProfile)`, propagating read errors to `Error::Refused` while preserving full profile data. Non-TIFF formats continue through `ImageDecoder::icc_profile`.

2. **`moxcms` floating point evaluation:**
   Configured `moxcms::TransformOptions` with `prefer_fixed_point: false`. The default Q2.13 fixed-point math in `moxcms` produced quantization drift of 2–4 levels on high-saturation 16-bit patches; floating point arithmetic brings all patches within 0–1 levels of published sRGB values, comfortably satisfying the 2-level tolerance.

### Real RAW timing

Developed `Target.RAF` (Fujifilm X-Trans, 6000×4000, 24 MP) from `~/Desktop/Projects/rollchrome-chart-sources/styles_source/Target.RAF` to 16-bit ProPhoto TIFF (111 MB) in release mode, measuring `decode_image` over 5 runs:
- **With conversion (`decode_image` with ProPhoto -> sRGB `moxcms` transform):** 0.701 s
- **Without conversion (image decode without ICC transform):** 0.650 s
- **Difference (ICC transform overhead):** 0.051 s (51 ms for 24 million 16-bit pixels)

### Gate outputs

#### 1. `cargo fmt --all --check`
```
(clean, exit code 0)
```

#### 2. `cargo clippy --workspace --all-targets -- -D warnings`
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.71s
```

#### 3. `cargo build --workspace`
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.43s
```

#### 4. `cargo test --workspace`
```
test result: ok. 518 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.08s
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.50s
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.56s
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.35s
test result: ok. 44 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.40s
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.59s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.73s
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.44s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.45s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.64s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.54s
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.74s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 41 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.05s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.52s
test result: ok. 149 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.48s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.82s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.53s
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.62s
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.11s
test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 16.46s
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.91s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.06s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Total: 1102 passed; 0 failed
```

#### 5. `cargo test -p phototools-core`
```
test result: ok. 518 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.08s
test result: ok. 11 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.45s
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.67s
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.34s
test result: ok. 44 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.42s
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.52s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.72s
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.43s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.43s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Total: 713 passed; 0 failed
```

#### 6. `cargo test -p rollchrome-core`
```
test result: ok. 149 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.50s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.84s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.53s
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.61s
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s
test result: ok. 32 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 16.00s
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.92s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Total: 273 passed; 0 failed
```

#### 7. `cargo test -p phototools-desktop`
```
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.44s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.86s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Total: 26 passed; 0 failed
```

#### 8. `cargo test -p rollchrome-desktop`
```
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Total: 19 passed; 0 failed
```

#### 9. `npm --prefix frontend/rollchrome run typecheck`
```
> @phototools/rollchrome@0.1.0 typecheck
> vue-tsc --build --force
```

#### 10. `npm --prefix frontend/rollchrome run check:edit`
```
> @phototools/rollchrome@0.1.0 check:edit
> node scripts/check-edit.mjs

Starting Vite harness server for Edit view...
Harness server listening at http://[::1]:5173
Loading test photograph into Edit view...
Benchmarking drag responsiveness (720p proxy, target p95 <= 50 ms)...
  Drag frames: min=8.5 ms, median=9.5 ms, p95=12.4 ms (budget <= 50 ms)
Benchmarking settle responsiveness (1440p proxy, target p95 <= 120 ms)...
  Settle frames: min=37.1 ms, median=40.9 ms, p95=47.3 ms (budget <= 120 ms)
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

#### 11. `npm --prefix frontend/rollchrome run check:bulk-lut`
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

---

## Review

*Written by the architect, after re-running the gates on the branch.*
