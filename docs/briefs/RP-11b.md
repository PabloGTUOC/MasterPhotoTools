# `RP-11b` · Masks: local clarity, texture, dehaze and sharpness

> **Status:** briefed
> **Plan:** [`docs/rollchrome/parity-plan.md`](../rollchrome/parity-plan.md), step `RP-11b` · **Branch:** `rp-11b`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11), **Gates**, **The design language**.
- `crates/rollchrome-core/src/media/edit/detail.rs`: `Detail`, `apply_detail(src, detail, scale)`.
- `crates/rollchrome-core/src/media/edit/masks.rs`, all of it. `LocalAdjustments`, `Mask::is_identity`, `CompiledMasks`, `local_at`.
- `preview.rs`: `stage_linear`, the `cached_detail_*` caches, `CachedGeometry`, the `stored` cache and how it is pruned, `render_rgba_frame_masked`. `pipeline.rs`: where export applies detail and geometry, and `apply_adjustments_masked`. **There are two pipelines.**
- `frontend/rollchrome/src/views/Edit.vue`: the mask adjustment sliders, `setMaskLocal`, `normaliseMask`, `cleanRecipeForSave`.
- `frontend/rollchrome/scripts/check-masks.mjs`: the pattern for the new check.

### Goal

A mask can already change exposure, contrast and colour, because those depend on one pixel. Clarity, texture, dehaze and sharpness look at a pixel's **neighbours**, so the per-pixel pass cannot do them. The owner needs them inside a mask: more clarity on the subject, dehaze only on the sky.

Click-to-select is **not** here. It needs a further model and is step RP-11c.

### The design, which is not optional

1. **One extra frame per mask that needs one.** For each active mask with non-zero local detail, compute a frame `D` with `apply_detail` on the **same source the global detail starts from**, using the recipe's global `Detail` with this mask's values added: clarity, texture and dehaze add and clamp to −100..100; `sharpen_amount` gains `sharpness × 1.5` and clamps to 0..150. Radius, masking, noise and hot pixels stay as in the global `Detail`. This way nothing is sharpened twice.
2. `D` goes through the **same geometry** as the base frame.
3. **Blend after geometry, before the pixel pass.** With `B` the base frame and `w` a mask's full weight at that pixel (shape, invert, components, brush, opacity), the composite is `B + Σ w × (D − B)` over the masks with local detail. The composite then goes into the existing pixel pass.
4. **Selection always reads `B`.** Pass `B` as the pixels `local_at` and the blend read for range masks, and the composite as the pixels being adjusted. Otherwise a range mask's selection would move with its own Dehaze slider.
5. **No local detail anywhere means no extra work:** no composite, no second buffer, the same bytes as today.
6. One function does the blend and both pipelines call it. Do not write it twice.

### Change

**`media/edit/masks.rs`:**

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalDetail { pub clarity: f32, pub texture: f32, pub dehaze: f32, pub sharpness: f32 } // the first three -100..100, sharpness 0..100
// on Mask: #[serde(default, skip_serializing_if = "LocalDetail::is_zero")] pub detail: LocalDetail
```

Keep it **out of `LocalAdjustments`**: `local_at` and the per-pixel path stay as they are. `Mask::is_identity` becomes `!enabled || opacity <= 0 || (adjustments.is_zero() && detail.is_zero())`.

**`preview.rs`:** cache the local frames per stage, keyed by mask id, holding the combined `Detail` and the geometry, so that moving exposure recomputes nothing and moving one mask's Clarity recomputes only that mask's frame. Prune entries for masks no longer in the recipe, as `stored` does. Add `pub fn cached_local_detail_count(&self) -> usize`. Count each computation in `detail_recompute_count`.

**`pipeline.rs`:** the same steps for export.

**`Edit.vue`:** four sliders in the selected mask's panel under a **Detail** heading: Clarity, Texture, Dehaze (−100..100) and Sharpness (0..100), test ids `mask-clarity`, `mask-texture`, `mask-dehaze`, `mask-sharpness`. `Drag` while moving, `Settle` on release, one undo step labelled `Masks`. `normaliseMask` must carry `detail`, and a mask whose four values are zero sends no `detail` field.

**`api.ts`:** the type and field.

**`scripts/check-local-detail.mjs`, a harness** (extend the edit harness's stub, without changing what any existing check sees), `check:local-detail` in `package.json`, each measured in the browser:

1. The selected mask's panel shows the four sliders with those ranges.
2. Moving Clarity sends `Drag` renders carrying `detail.clarity`; release sends `Settle`.
3. All four values reach the save payload.
4. One slider release is one undo step, labelled `Masks`, and undo restores the value.
5. A mask with all four at zero sends no `detail` field.
6. Selecting another mask shows that mask's own values.
7. A photograph reopened from a saved recipe shows the saved values.
8. Every new control is at least 40 px, and a missing control fails. Screenshot to `layout-proof/`.

**`docs/rollchrome/parity-plan.md`:** status line. **`CLAUDE.md`:** the gate, the list of checks that drive Chromium, the counts, the plan row. **`docs/rollchrome/manual-verification.md`:** a section `MV-34`, real photographs, one check per slider plus one for export matching the preview. The count of Rollchrome checks in `CLAUDE.md` and in that file's header must equal `grep -c '^- \[.\] \*\*MV'`.

### Do not touch

- `SPECIFICATION.md` (G9). `media/develop/**`, `media/looks/**`, `media/segment.rs`.
- What `apply_detail` computes. Every existing assertion, every budget (G7), and every existing check script.
- No new dependencies (G8). No click selection. No negative sharpness.

### Acceptance

- [ ] `rollchrome-core` gains exactly these fourteen tests, in a new `crates/rollchrome-core/tests/local_detail_tests.rs`:
  `a_recipe_without_local_detail_gives_identical_bytes` (an absent field and four zeros against each other, and against an expected literal generated by running the recipe on `main`),
  `a_mask_with_only_local_detail_is_active`,
  `local_clarity_acts_only_where_the_mask_covers` (uncovered pixels byte-identical to the render without the mask),
  `full_coverage_local_clarity_equals_global_clarity` (within 1 level, preview and export),
  `full_coverage_local_texture_equals_global_texture` (within 1 level),
  `full_coverage_local_dehaze_equals_global_dehaze` (within 1 level),
  `local_and_global_clarity_add` (mask 30 + global 20 equals global 50, within 1 level),
  `local_sharpness_adds_to_the_global_amount` (mask 40 + global 30 equals global amount 90, within 1 level),
  `half_opacity_gives_half_the_change` (within 1 level),
  `a_range_mask_does_not_move_with_its_own_dehaze` (the same pixels are affected at Dehaze 10 and 100, preview and export),
  `local_detail_follows_a_crop_and_a_rotation`,
  `export_and_preview_agree_on_local_detail` (within 2 levels),
  `moving_exposure_recomputes_no_local_detail_and_one_mask_recomputes_alone` (through `detail_computes`),
  `a_deleted_mask_releases_its_local_detail_frames` (through `cached_local_detail_count`).
- [ ] Each test must be able to fail for the reason its name gives. For `a_range_mask_does_not_move_with_its_own_dehaze`, temporarily read the composite instead of `B` in each pipeline, confirm the test fails, revert, and say so in the Report.
- [ ] Counts: workspace 1213 → 1227, `rollchrome-core` 377 → 391; `phototools-core` 714, `phototools-desktop` 26, `rollchrome-desktop` 25 unchanged. Every existing test still there, none weakened (G7).
- [ ] `check:local-detail` asserts all eight, as numbered here, and passes; all the other checks unchanged and passing.
- [ ] No `todo!()`, bare lock `unwrap()` or swallowed error on a shipped path (G10); no logic in the binary (G1).

### Gates

Run `. "$HOME/.cargo/env"` first. Use `--no-fail-fast` on every `cargo test`, so that the counts are complete.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --no-fail-fast --workspace             # 1227
cargo test --no-fail-fast -p phototools-core      # 714
cargo test --no-fail-fast -p rollchrome-core      # 391
cargo test --no-fail-fast -p phototools-desktop   # 26
cargo test --no-fail-fast -p rollchrome-desktop   # 25
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
npm --prefix frontend/rollchrome run check:point
npm --prefix frontend/rollchrome run check:masks
npm --prefix frontend/rollchrome run check:local-detail
```

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why; **every file changed and why, including any change this brief does not list**; the **actual output** of every gate, counts and failures, never just "passed"; and anything you could not verify, said plainly. **Use the exact test names and the exact numbered assertions. Do not change product behaviour to get a check through, and do not shape the stub to produce the answer a check wants.** In release mode, through `PreviewSession::open`, on a TIFF developed from a real RAW in `~/Desktop/Projects/rollchrome-chart-sources/`, give the median milliseconds over at least 10 renders of `Drag` and `Settle` with two masks each using Clarity and Dehaze: (a) while exposure changes, the local frames being cached, and (b) while one mask's Clarity changes. Also give the same for no masks. The budgets are 50 ms for `Drag` and 120 ms for `Settle`; if (b) misses them, say so with the numbers. Name a commit only if `git cat-file -t` confirms it exists. Never add those files to the repository. Do not tick the acceptance boxes; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
