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

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
