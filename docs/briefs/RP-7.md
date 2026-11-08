# `RP-7` · Detail: sharpening, noise reduction, hot pixels

> **Status:** briefed
> **Plan:** [`docs/rollchrome/parity-plan.md`](../rollchrome/parity-plan.md), step `RP-7` · **Branch:** `rp-7`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11), **Gates**, **The design language**.
- `docs/rollchrome/parity-plan.md`, *The risk across RP-7 and RP-8*.
- `crates/rollchrome-core/src/media/edit/preview.rs`: `PreviewSession::render`, `CachedGeometry`, the header on why the proxies exist. `media/edit/pipeline.rs::apply_recipe_with_orientation`.
- `frontend/rollchrome/src/views/Edit.vue`: a `CollapsibleSection` of sliders, `deriveLabel`, `normaliseRecipe`, the zoom state.
- `frontend/rollchrome/scripts/check-colour.mjs` — the pattern for the new check.

### Goal

Sharpening, luminance and colour noise reduction, and hot pixel removal. Everything in the pipeline so far looks at one pixel at a time; these look at neighbours, so **they cannot go in the per-pixel pass without breaking its timing budget.** They become a stage of their own, run once on the source proxy and cached, so that moving any other slider costs nothing more than it does today. RP-8 will use the same stage.

### Change

**`crates/rollchrome-core/src/media/edit/detail.rs`** (new):

```rust
pub struct Detail {
    pub sharpen_amount: f32,    // 0..150, default 0
    pub sharpen_radius: f32,    // 0.5..3.0 source pixels, default 1.0
    pub sharpen_masking: f32,   // 0..100, default 0
    pub noise_luminance: f32,   // 0..100, default 0
    pub noise_colour: f32,      // 0..100, default 0
    pub hot_pixels: bool,       // default false
}
pub fn is_identity(&self) -> bool;
/// `scale` is this buffer's long edge over the source's: 1.0 for an export.
pub fn apply_detail(src: &LinearBuffer, detail: &Detail, scale: f32) -> LinearBuffer;
```

All with `serde(default)` and a recipe field `detail: Option<Detail>`, `skip_serializing_if = "Option::is_none"`, counted by `is_identity`. `rayon`, no new dependency. Box filters by sliding sum, so that cost does not grow with the radius.

The order, and how each works, so that the result is the one this brief means:

1. **Hot pixels**, in linear. Per channel: if a value exceeds `4 × the largest of its 8 neighbours + 0.02`, replace it with the median of those neighbours. A star, wider than one pixel, has bright neighbours and survives.
2. Convert to `sqrt` of linear (noise is closer to uniform there). `Y = 0.2126R + 0.7152G + 0.0722B`, `Cb = B − Y`, `Cr = R − Y`, on those encoded values.
3. **Colour noise:** guided filter on `Cb` and `Cr` with `Y` as the guide, radius `max(1, round(8 × scale))`, regularisation rising with the slider.
4. **Luminance noise:** guided filter of `Y` by itself, radius `max(1, round(2 × scale))`, regularisation rising with the slider.
5. **Sharpening:** `D = Y − gaussian(Y)`, three box passes approximating sigma `max(0.35, radius × scale)`. Masking: `t = masking / 100 × 0.08`; a mask `smoothstep(0.5 t, 1.5 t, |gradient of the blurred Y|)`, or 1 when masking is 0. `Y += amount / 100 × D × mask`.
6. Rebuild: `R = Y + Cr`, `B = Y + Cb`, `G = (Y − 0.2126R − 0.0722B) / 0.7152`; floor at 0; square.

The regularisation constants are yours to tune against the tests; put why they have their values in a comment.

**Where it runs.** On the **stored** frame, before geometry, in both paths:

- *Export:* `apply_recipe_with_orientation`, right after `LinearBuffer::from_image_buffer`, `scale` 1.0.
- *Preview:* a cache in `PreviewSession`, one slot per stage, keyed by the `Detail`: `Arc<LinearBuffer>`, replacing `drag_linear`, `settle_linear` or the full buffer as the input to everything after it, geometry and `mask_coverage` included. `CachedGeometry` becomes keyed by geometry **and** detail. `scale` is the proxy's long edge over the **source's**, so that a radius means the same at every stage. Add `pub fn detail_computes(&self) -> usize`, a counter. With no `detail`, or an identity one, no extra buffer is made and the bytes are exactly what they are today.
- `is_identity` counts it, so that a sharpened photograph is not copied byte for byte. Presets carry it.

**`api.ts`:** the type and field.

**`Edit.vue`:** a section **Detail**: Sharpening (Amount, Radius, Masking), Noise reduction (Luminance, Colour), a **Remove hot pixels** toggle, with a section reset. `normaliseRecipe` supplies `detail` when missing, at defaults, without making the recipe non-identity. `deriveLabel` says `Detail`. A quiet line, *Judge detail at 100% (Z)*, shows while zoom is fit, because a proxy is already smoothed by reducing it.

**`scripts/check-detail.mjs`, a harness** (extend the edit harness's stub), `check:detail` in `package.json`, each measured in the browser:

1. The section has five sliders and the toggle; at their defaults, and a photograph without `detail` is still clean of it in the first save.
2. Moving a slider sends `Drag` renders carrying `detail`; releasing at fit sends `Settle`.
3. Releasing at 100% sends `Full`.
4. The hint shows at fit and is gone at 100%.
5. The section reset returns all six to defaults and is one history step.
6. A change has the history label `Detail`, and Cmd-Z takes it back.
7. The toggle commits, and reaches the save payload.
8. Every new control is at least 40 px. Screenshot to `layout-proof/`.

**`CLAUDE.md`:** the gate and the counts. **`docs/rollchrome/parity-plan.md`:** status line. **`docs/rollchrome/manual-verification.md`:** a section `MV-28` with a check of sharpening and of noise reduction on a real high-ISO photograph at 100%, and update the count of Rollchrome checks in `CLAUDE.md`.

### Do not touch

- `SPECIFICATION.md` (G9). `media/develop/**`, `media/looks/**`.
- The per-pixel pass, the `Drag`, `Settle` and `Full` budgets, and every existing assertion (G7).
- No new dependencies (G8). No local sharpness or noise in masks, no output sharpening (RP-9), no clarity, texture or dehaze (RP-8).

### Acceptance

- [ ] `rollchrome-core` gains exactly these thirteen tests, in a new `crates/rollchrome-core/tests/detail_tests.rs`:
  `detail_at_rest_changes_nothing` (no `detail` and an identity one give identical bytes),
  `sharpening_raises_the_contrast_of_an_edge`,
  `sharpening_leaves_a_flat_area_alone`,
  `masking_keeps_noise_in_flat_areas_from_being_sharpened`,
  `luminance_noise_reduction_halves_the_noise_and_keeps_the_edge` (amount 50, Gaussian noise: standard deviation ≤ 0.5 of the input; the step at an edge ≥ 90% of its height),
  `colour_noise_reduction_halves_chroma_noise_and_leaves_luminance` (chroma standard deviation ≤ 0.5; luma's within 5%),
  `a_hot_pixel_is_removed_and_its_neighbours_are_untouched`,
  `a_bright_star_wider_than_one_pixel_survives`,
  `the_radius_scales_with_the_proxy` (sharpening a half-size image at scale 0.5 agrees with halving the sharpened full image, within 2 levels of mean difference),
  `export_and_full_preview_agree_on_detail` (within 2 levels),
  `changing_another_slider_does_not_recompute_detail` (`detail_computes() == 1` after two renders differing only in exposure),
  `a_recipe_written_before_detail_still_loads`,
  `a_sharpened_photograph_is_not_identity`.
- [ ] Counts: workspace 1139 → 1152, `rollchrome-core` 307 → 320; `phototools-core` 713, `phototools-desktop` 26, `rollchrome-desktop` 22 unchanged. Every existing test still there, none weakened (G7).
- [ ] `check:detail` asserts all eight, as numbered here, and passes; all the other checks unchanged and passing.
- [ ] No `todo!()`, bare lock `unwrap()` or swallowed error on a shipped path (G10).

### Gates

Run `. "$HOME/.cargo/env"` first. Use `--no-fail-fast` on every `cargo test`, so that the counts are complete.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --no-fail-fast --workspace             # 1152
cargo test --no-fail-fast -p phototools-core      # 713
cargo test --no-fail-fast -p rollchrome-core      # 320
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
```

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why; **every file changed and why, including any change this brief does not list**; the **actual output** of every gate, counts and failures, never just "passed"; and anything you could not verify, said plainly. **Use the exact test names and the exact numbered assertions. Do not change product behaviour to get a check through.** In release mode, through `PreviewSession::open`, on a TIFF developed from a real RAW in `~/Desktop/Projects/rollchrome-chart-sources/`, with every control on, give the milliseconds of `apply_detail` at `Drag`, `Settle` and `Full`, and of a later `Full` render that changes only exposure. Never add those files to the repository. Do not tick the acceptance boxes; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
