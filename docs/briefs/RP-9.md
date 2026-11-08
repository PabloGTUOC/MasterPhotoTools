# `RP-9` · Export options

> **Status:** briefed
> **Plan:** [`docs/rollchrome/parity-plan.md`](../rollchrome/parity-plan.md), step `RP-9` · **Branch:** `rp-9`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11), **Gates**, **The design language**.
- `crates/rollchrome-core/src/tools/edit.rs`: `render_and_write`, `export_edited_image_with_resolver`, `exclusive_create_target`. `media/edit/icc.rs`, the sRGB corrections. `media/edit/detail.rs`. `crates/core/src/media/image_ops.rs` (`dimensions_for_megapixels`), `media/jpeg.rs`, and `tools/mod.rs::carry_metadata_correcting`.
- `crates/rollchrome-desktop/src/commands/edit.rs`. `frontend/rollchrome/src/views/Edit.vue`: the Export Render section, `handleExport`, `exportAllVersions`.
- `frontend/rollchrome/scripts/check-versions.mjs` — the pattern for the new check.

### Goal

Edit always writes a JPEG at quality 95 or a 16-bit TIFF, at full size, named `_edit`. The owner needs what an export dialog gives: the format, the quality, the size, sharpening for where it is going, the file's name, and what metadata leaves with it. **This step is Edit's export, and Export all with it. Batch and Develop keep theirs.**

### Change

**`crates/rollchrome-core/src/tools/edit.rs`**, a `serde` type, every field defaulted:

```rust
pub struct ExportOptions {
    pub format: ExportFormat,          // default AsSource
    pub resize: ExportResize,          // default None
    pub output_sharpening: OutputSharpening, // default Off
    pub name_template: Option<String>, // default None
    pub metadata: MetadataPolicy,      // default All
}
pub enum ExportFormat { AsSource, Jpeg { quality: u8 }, Tiff { bits: u8 /* 8 or 16 */ }, Png { bits: u8 }, Webp, Avif { quality: u8 } }
pub enum ExportResize { None, LongEdge(u32), Megapixels(u32) }
pub enum OutputSharpening { Off, Screen, Print }
pub enum MetadataPolicy { All, NoLocation, None }
```

and `pub fn export_edited_image_with_options(source, recipe, lut, out_dir, options)`. `export_edited_image` calls it with the defaults, and **with the defaults every byte is what it is today**, including the identity byte-copy. Any other option means rendering: an identity recipe with a resize is not a copy.

- *Order:* render with the recipe, then resize, then output sharpening, then encode. Resizing **never enlarges**. Geometry has already happened, so a crop's size is what is resized.
- *Format:* `AsSource` is today's rule (16-bit gives TIFF, 8-bit gives JPEG 95). JPEG through `phototools_core::media::jpeg`, reducing to 8 bits. TIFF and PNG at the bits chosen, widening or reducing as needed. WebP is **lossless**, because the `image` crate's encoder has no lossy mode: the screen says so. AVIF through `image`'s encoder at speed 6, 8-bit. Refuse a quality outside 1..100, and bits other than 8 or 16.
- *Output sharpening:* through `apply_detail` at scale 1.0, on the resized pixels: `Screen` amount 40, radius 0.7; `Print` amount 70, radius 1.0; masking 0. Keep the bit depth.
- *Name:* with a template, the file's name comes from its tokens, `{name}` (the source's stem), `{version}` (the version's name, or empty), `{date}` (capture date as `YYYY-MM-DD`, from `media::meta`, or empty if there is none) and `{edit}`, which is `_edit` plus the version suffix RP-6 made. Without one, today's name. After substitution, anything other than letters, digits, `-`, `_`, `.` and spaces becomes `-`, so that it **can never leave the output folder** (G6). A template that comes out empty, or is only dots, is `Error::Refused`. A name that exists gets a number from `exclusive_create_target`, as now: **never overwrite.**
- *Metadata:* `All`, as now, with the sRGB corrections. `NoLocation`, the same plus `-gps:all=`. `None` carries nothing at all and starts no exiftool. A format exiftool cannot write is reported in `metadata_skipped`, as any failure is, and the file stays.

**`crates/rollchrome-desktop/src/commands/edit.rs`:** `export_edited_image` takes an optional `options`. Keep `export_edited_image_impl`'s signature so that its tests do not change; add `export_edited_image_with_options_impl`.

**`api.ts`:** the types and the argument.

**`Edit.vue`, the Export Render section:** format; quality for JPEG and AVIF; bits for TIFF and PNG; size (Full, Long edge, Megapixels, with a number); output sharpening (Off, Screen, Print); a name field listing the tokens, with a live example of the resulting name; metadata (All, No location, None). Held at module level, as `recipeClipboard.ts` is, so that the settings stay while the application is open. **Export all versions** uses the same options; a template without `{version}` or `{edit}` gets numbers from the collision rule, and that is fine. A refusal is shown.

**`scripts/check-export.mjs`, a harness** (extend the edit harness's stub), `check:export` in `package.json`, each measured in the browser:

1. At the defaults, the call sends the default options, and the section shows Full, As source.
2. Each format sends its own form: JPEG with quality, TIFF and PNG with bits, WebP, AVIF with quality.
3. The quality control appears only for JPEG and AVIF, the bits control only for TIFF and PNG.
4. WebP says it is lossless.
5. Long edge and Megapixels send their number; an empty or zero number disables Export with a reason.
6. Output sharpening and metadata send their choices.
7. A template is sent, and the example shows the substituted name, using the current version.
8. A refused template from the stub is shown.
9. Leaving the tab and returning keeps the settings.
10. Export all sends the same options for each version.
11. Every new control is at least 40 px. Screenshot to `layout-proof/`.

**`CLAUDE.md`:** the gate and the counts. **`docs/rollchrome/parity-plan.md`:** status line. **`docs/rollchrome/manual-verification.md`:** a section `MV-30`, opening each format from a real export in another application. Update the count of Rollchrome checks in `CLAUDE.md`.

### Do not touch

- `SPECIFICATION.md` (G9). Batch, Develop, `media/develop/**`, `media/looks/**`.
- The Publishing and card-volume refusals (G5, G6). Every existing assertion (G7).
- No new dependency (G8): `image` already has what this needs. If an encoder needs a feature switched on, say so in the Report with the reason.

### Acceptance

- [ ] `rollchrome-core` gains exactly these thirteen tests, in a new `crates/rollchrome-core/tests/export_tests.rs`:
  `the_default_options_export_exactly_as_before` (byte-identical to `export_edited_image`, and the identity copy still a copy),
  `a_sixteen_bit_source_exports_a_jpeg_and_a_higher_quality_is_larger`,
  `an_eight_bit_source_exports_a_sixteen_bit_tiff`,
  `png_and_webp_open_again_with_the_same_pixels`,
  `avif_opens_again_close_to_the_source` (mean within 8 levels),
  `long_edge_and_megapixels_resize_and_never_enlarge`,
  `a_crop_is_resized_after_it_is_applied`,
  `output_sharpening_raises_edge_contrast_at_the_final_size`,
  `an_identity_recipe_with_a_resize_is_rendered_not_copied`,
  `the_name_template_fills_its_tokens`,
  `a_name_template_cannot_leave_the_folder` (`../../x`, `/etc/x`, and one that comes out empty),
  `a_second_export_of_the_same_name_gets_a_number`,
  `the_metadata_policy_keeps_removes_or_drops` (real exiftool: GPS present, GPS gone with other tags kept, nothing carried).
- [ ] Counts: workspace 1166 → 1179, `rollchrome-core` 334 → 347; `phototools-core` 713, `phototools-desktop` 26, `rollchrome-desktop` 22 unchanged. Every existing test still there, none weakened (G7).
- [ ] `check:export` asserts all eleven, as numbered here, and passes; all the other checks unchanged and passing.
- [ ] No `todo!()`, bare lock `unwrap()` or swallowed error on a shipped path (G10).

### Gates

Run `. "$HOME/.cargo/env"` first. Use `--no-fail-fast` on every `cargo test`, so that the counts are complete.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --no-fail-fast --workspace             # 1179
cargo test --no-fail-fast -p phototools-core      # 713
cargo test --no-fail-fast -p rollchrome-core      # 347
cargo test --no-fail-fast -p phototools-desktop   # 26
cargo test --no-fail-fast -p rollchrome-desktop   # 22
npm --prefix frontend/shared run build
npm --prefix frontend/rollchrome run typecheck
npm --prefix frontend/rollchrome run build
npm --prefix frontend/rollchrome run check:transport
npm --prefix frontend/rollchrome run check:edit
npm --prefix frontend/rollchrome run check:bulk-lut
npm --prefix frontend/rollchrome run check:develop
npm --prefix frontend/rollchrome run check:viewing
npm --prefix frontend/rollchrome run check:colour
npm --prefix frontend/rollchrome run check:history
npm --prefix frontend/rollchrome run check:versions
npm --prefix frontend/rollchrome run check:detail
npm --prefix frontend/rollchrome run check:export
```

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why; **every file changed and why, including any change this brief does not list**; the **actual output** of every gate, counts and failures, never just "passed"; and anything you could not verify, said plainly. **Use the exact test names and the exact numbered assertions. Do not change product behaviour to get a check through.** In release mode, on a TIFF developed from a real RAW in `~/Desktop/Projects/rollchrome-chart-sources/`, give the seconds and file size for a full-size JPEG 90, a PNG 8-bit, a WebP and an AVIF 70. Never add those files to the repository. Do not tick the acceptance boxes; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
