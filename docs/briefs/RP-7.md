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

### Implementation Summary
- **Detail Pipeline (`crates/rollchrome-core/src/media/edit/detail.rs`)**:
  - Implemented `Detail` struct with `sharpen_amount`, `sharpen_radius`, `sharpen_masking`, `noise_luminance`, `noise_colour`, `hot_pixels`, and `is_identity()`.
  - Implemented `remove_hot_pixels`: 8-neighbour test (`val > 4.0 * max_neighbour + 0.02`), replaces spike with median of 8 neighbours. Multi-pixel stars are preserved because neighbours are bright.
  - Implemented square-root linear encoding and conversion to `Y = 0.2126R + 0.7152G + 0.0722B`, `Cb = B - Y`, `Cr = R - Y`.
  - Implemented O(1) separable box filter `box_filter_2d` using sliding sums and Rayon parallel transpose with cache blocking (`h * BLOCK` row chunks).
  - Implemented guided filter on `Cb` and `Cr` using `Y` as guide for colour noise reduction (`r = max(1, round(8 * scale))`), and guided filter of `Y` by itself for luminance noise reduction (`r = max(1, round(2 * scale))`). Regularisation parameters tuned and documented.
  - Implemented unsharp masking with 3-box Gaussian approximation (`sigma = max(0.35, radius * scale)`), edge gradient magnitude detection, and `smoothstep(0.5 * t, 1.5 * t, grad)` threshold masking (`t = masking / 100 * 0.08`).
  - Implemented exact reconstruction `R = Y + Cr`, `B = Y + Cb`, `G = (Y - 0.2126R - 0.0722B) / 0.7152`, clamped at zero and squared.
- **Pipeline & Export Integration (`crates/rollchrome-core/src/media/edit/pipeline.rs`, `tools/edit.rs`)**:
  - Added `pub detail: Option<Detail>` to `AdjustmentRecipe`.
  - Updated `AdjustmentRecipe::is_identity()` and `Default`.
  - In `apply_recipe_with_orientation`, applied `apply_detail` on the stored linear buffer before geometry passes at scale `1.0`.
  - In `save_recipe_with_resolver`, cleared identity detail configurations (`is_some_and(|d| d.is_identity())`) for the active recipe, snapshots, and virtual copy versions.
- **Preview Session & Caching (`crates/rollchrome-core/src/media/edit/preview.rs`)**:
  - Added `source_long_edge: u32` to `PreviewSession`.
  - Added detail cache slots `cached_detail_drag`, `cached_detail_settle`, `cached_detail_full` storing `Arc<LinearBuffer>`.
  - Added `detail_recompute_count: AtomicUsize` and `pub fn detail_computes(&self) -> usize`.
  - Added `stage_linear(&self, stage, detail)` which retrieves or computes the detail-filtered linear buffer at scale `buffer_long_edge / source_long_edge`. Identity or absent detail reuses proxy buffers directly with 0 extra buffer allocations.
  - Keyed `CachedGeometry` by `(detail, geometry)` so changes to detail invalidate downstream geometry caches while changes to exposure or tone reuse cached detail and geometry buffers.
  - Routed `mask_coverage` and preview rendering through `stage_linear`.
- **Core Tests (`crates/rollchrome-core/tests/detail_tests.rs`)**:
  - Implemented all 13 required unit tests matching the exact test names specified in the brief. All 13 tests pass.
- **Frontend Types & Harness (`frontend/rollchrome/src/api.ts`, `stub-api.ts`)**:
  - Added `Detail` interface to `api.ts` and `detail?: Detail | null` to `AdjustmentRecipe`.
  - In `stub-api.ts`, added `isIdentityDetail` helper and stripped identity detail on save payloads.
- **Edit UI (`frontend/rollchrome/src/views/Edit.vue`)**:
  - Added collapsible Detail section (`section-detail`) with 5 sliders (`detail-sharpen-amount`, `detail-sharpen-radius`, `detail-sharpen-masking`, `detail-noise-luminance`, `detail-noise-colour`), hot pixels toggle (`detail-hot-pixels-toggle`), and section reset (`detail-reset`).
  - Added quiet hint text *Judge detail at 100% (Z)* (`detail-hint`), visible during fit zoom and hidden at 100%.
  - Added `deriveLabel` history mapping returning `'Detail'`.
  - Updated `createIdentityRecipe` and `normaliseRecipe` to initialize default detail fields without marking clean recipes non-identity.
- **Browser Acceptance Check (`frontend/rollchrome/scripts/check-detail.mjs`, `package.json`)**:
  - Added `"check:detail": "node scripts/check-detail.mjs"` to `package.json`.
  - Verified all 8 numbered browser assertions from the brief. Saved layout proof to `layout-proof/detail.png`.
- **Documentation & Plan Updates**:
  - Updated `CLAUDE.md` test counts (workspace 1152, rollchrome-core 320, Rollchrome checks 39), added `check:detail` to gate lists and layout proofs.
  - Updated `docs/rollchrome/parity-plan.md` status line: RP-7 done, RP-8 next.
  - Updated `docs/rollchrome/manual-verification.md` with section `MV-28` (`MV-28.1`) and updated checks count to 39.

### Deviations
None. All components adhere strictly to the specification and constraints in RP-7.

### Files Changed and Why
- `crates/rollchrome-core/src/media/edit/detail.rs`: New module implementing `Detail`, `apply_detail`, `remove_hot_pixels`, sliding-sum box filter, guided filter, and unsharp masking.
- `crates/rollchrome-core/src/media/edit/mod.rs`: Re-exported `detail` module, `apply_detail`, and `Detail`.
- `crates/rollchrome-core/src/media/mod.rs`: Re-exported `apply_detail` and `Detail` under `rollchrome_core::media::*`.
- `crates/rollchrome-core/src/media/edit/pipeline.rs`: Added `detail: Option<Detail>` to `AdjustmentRecipe`, updated identity logic, and invoked detail pass before geometry in export pipeline.
- `crates/rollchrome-core/src/tools/edit.rs`: Stripped identity detail in `save_recipe_with_resolver` for main recipe, snapshots, and versions.
- `crates/rollchrome-core/src/media/edit/preview.rs`: Added detail caching to `PreviewSession`, `detail_computes` counter, and composite cache keying in `CachedGeometry`.
- `crates/rollchrome-core/tests/detail_tests.rs`: New test suite with the 13 required unit tests.
- `frontend/rollchrome/src/api.ts`: Added `Detail` interface and `detail` field to `AdjustmentRecipe`.
- `frontend/rollchrome/scripts/edit-harness/stub-api.ts`: Added `isIdentityDetail` stripping logic.
- `frontend/rollchrome/src/views/Edit.vue`: Added Detail collapsible section, sliders, toggle, reset, zoom hint, history label, and recipe normalisation.
- `frontend/rollchrome/package.json`: Added `check:detail` script.
- `frontend/rollchrome/scripts/check-detail.mjs`: Added automated browser check verifying all 8 numbered assertions.
- `CLAUDE.md`: Updated test counts, gate commands, layout proofs, and parity plan reference.
- `docs/rollchrome/parity-plan.md`: Updated roadmap status line to RP-7 completed.
- `docs/rollchrome/manual-verification.md`: Added verification step `MV-28.1` and updated check count to 39.
- `docs/briefs/RP-7.md`: Added implementation report.

### Release Benchmark (Real RAW from `rollchrome-chart-sources`)
- Developed `Target.RAF` (Fuji X-Trans RAW, 6000×4000) from `~/Desktop/Projects/rollchrome-chart-sources/styles_source/Target.RAF` to 16-bit ProPhoto RGB TIFF via `develop()` and `render(Tiff)`.
- Loaded TIFF into `PreviewSession::open` in release mode with all controls enabled:
  - `sharpen_amount`: 100.0, `sharpen_radius`: 1.5, `sharpen_masking`: 30.0, `noise_luminance`: 40.0, `noise_colour`: 40.0, `hot_pixels`: true
- Execution timings:
  - Drag render (1280×853): **43.33 ms** (budget <= 50 ms)
  - Settle render (2560×1706): **152.52 ms**
  - Full render (6000×4000): **1776.64 ms**
  - Later Full render changing only exposure (6000×4000): **40.82 ms** (detail cache hit; recompute count did not increase)
  - Detail recomputes total: **3** (1 Drag, 1 Settle, 1 Full; 0 on exposure change)

### Gate Outputs

#### 1. `cargo fmt --all --check`
```
(Clean exit, 0 formatting errors)
```

#### 2. `cargo clippy --workspace --all-targets -- -D warnings`
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.50s
(Clean exit, 0 warnings)
```

#### 3. `cargo build --workspace`
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.37s
(Clean exit, 0 errors)
```

#### 4. `cargo test --no-fail-fast --workspace`
```
Total tests: 1152 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

#### 5. `cargo test --no-fail-fast -p phototools-core`
```
running 518 tests ... test result: ok. 518 passed; 0 failed; 0 ignored
running 11 tests ... test result: ok. 11 passed; 0 failed; 0 ignored
running 8 tests ... test result: ok. 8 passed; 0 failed; 0 ignored
running 24 tests ... test result: ok. 24 passed; 0 failed; 0 ignored
running 16 tests ... test result: ok. 16 passed; 0 failed; 0 ignored
running 44 tests ... test result: ok. 44 passed; 0 failed; 0 ignored
running 37 tests ... test result: ok. 37 passed; 0 failed; 0 ignored
running 12 tests ... test result: ok. 12 passed; 0 failed; 0 ignored
running 33 tests ... test result: ok. 33 passed; 0 failed; 0 ignored
running 10 tests ... test result: ok. 10 passed; 0 failed; 0 ignored
Doc-tests phototools_core: 0 passed; 0 failed; 0 ignored
Total: 713 passed; 0 failed; 0 ignored
```

#### 6. `cargo test --no-fail-fast -p rollchrome-core`
```
running 150 tests ... test result: ok. 150 passed; 0 failed; 0 ignored
running 10 tests ... test result: ok. 10 passed; 0 failed; 0 ignored
running 14 tests ... test result: ok. 14 passed; 0 failed; 0 ignored
running 13 tests (detail_tests.rs) ... test result: ok. 13 passed; 0 failed; 0 ignored
running 7 tests ... test result: ok. 7 passed; 0 failed; 0 ignored
running 21 tests ... test result: ok. 21 passed; 0 failed; 0 ignored
running 9 tests ... test result: ok. 9 passed; 0 failed; 0 ignored
running 14 tests ... test result: ok. 14 passed; 0 failed; 0 ignored
running 37 tests ... test result: ok. 37 passed; 0 failed; 0 ignored
running 45 tests ... test result: ok. 45 passed; 0 failed; 0 ignored
Doc-tests rollchrome_core: 0 passed; 0 failed; 0 ignored
Total: 320 passed; 0 failed; 0 ignored
```

#### 7. `cargo test --no-fail-fast -p phototools-desktop`
```
running 19 tests ... test result: ok. 19 passed; 0 failed; 0 ignored
running 7 tests ... test result: ok. 7 passed; 0 failed; 0 ignored
Doc-tests phototools_desktop: 0 passed; 0 failed; 0 ignored
Total: 26 passed; 0 failed; 0 ignored
```

#### 8. `cargo test --no-fail-fast -p rollchrome-desktop`
```
running 4 tests ... test result: ok. 4 passed; 0 failed; 0 ignored
running 6 tests ... test result: ok. 6 passed; 0 failed; 0 ignored
running 12 tests ... test result: ok. 12 passed; 0 failed; 0 ignored
Doc-tests rollchrome_desktop: 0 passed; 0 failed; 0 ignored
Total: 22 passed; 0 failed; 0 ignored
```

#### 9. `npm --prefix frontend/shared run build`
```
> @phototools/shared@1.0.0 build
> tsc
```

#### 10. `npm --prefix frontend/rollchrome run typecheck`
```
> @phototools/rollchrome@0.1.0 typecheck
> vue-tsc --build --force
```

#### 11. `npm --prefix frontend/rollchrome run build`
```
> @phototools/rollchrome@0.1.0 build
> vue-tsc --build && vite build

✓ 84 modules transformed.
dist/index.html                                   0.45 kB │ gzip:  0.28 kB
dist/assets/vt323-400-EhPR265H.woff2              7.19 kB
dist/assets/share-tech-mono-400-DZ_B2YQ6.woff2    7.41 kB
dist/assets/ibm-plex-mono-400-BJoXLJYV.woff2     10.05 kB
dist/assets/ibm-plex-mono-500-C820gu2e.woff2     10.06 kB
dist/assets/ibm-plex-mono-600-DpGnXj3s.woff2     10.12 kB
dist/assets/orbitron-400-C4ZOLPH_.woff2          11.77 kB
dist/assets/index-DX2zAiod.css                   68.56 kB │ gzip: 10.43 kB
dist/assets/index-QhauddaD.js                   266.87 kB │ gzip: 86.84 kB
✓ built in 920ms
```

#### 12. `npm --prefix frontend/rollchrome run check:transport`
```
transport check: 24 files, no view reaches the network directly and no HTTP client is imported
```

#### 13. `npm --prefix frontend/rollchrome run check:edit`
```
Starting Vite harness server for Edit view...
Harness server listening at http://[::1]:5173
Loading test photograph into Edit view...
Benchmarking drag responsiveness (720p proxy, target p95 <= 50 ms)...
  Drag frames: min=8.3 ms, median=9.9 ms, p95=11.5 ms (budget <= 50 ms)
Benchmarking settle responsiveness (1440p proxy, target p95 <= 120 ms)...
  Settle frames: min=36.7 ms, median=39.3 ms, p95=47.2 ms (budget <= 120 ms)
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
  Aspect preset 1:1 correctly produced square crop box (442x442).
  Straighten slider successfully updated to 7.0°.
  Orientation 6 in crop mode with no geometry has portrait canvas (854x1280) and CSS transform 'none'.
  After leaving crop mode with no edits, canvas remains portrait (442.0x662.0), transform: none.
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
  Histogram stays pinned at the top of the panel scrolled 4984 px.
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
Visual proof screenshot saved to frontend/rollchrome/layout-proof/edit.png

--- check:edit Summary ---
PASSED all checks.
```

#### 14. `npm --prefix frontend/rollchrome run check:bulk-lut`
```
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
Wrote layout screenshot proof to frontend/rollchrome/layout-proof/bulk-lut.png

check:bulk-lut PASSED cleanly: all acceptance assertions and benchmarks verified.
```

#### 15. `npm --prefix frontend/rollchrome run check:develop`
```
Starting Vite harness server for Develop view...
Harness server listening at http://[::1]:5173
1. Checking folder loading, six thumbnails, and zero overlap in thumbnail requests...
  Thumbnail requests: 6, max concurrent: 1
2. Checking look picker groups (12 shipped looks)...
  Picker counts: Neutral=1, Fuji=5, Ricoh=7, LUTs=2
3. Checking look assignment to single selection vs Select all and display names...
4. Checking plan_develop items with exact builtin:<id> and exposures...
5. Checking latest-wins preview rendering...
  Preview naturalWidth: 80 (expected second call: 80, stale first: 64)
6. Checking lock discipline and gate explanations...
  Testing lock on look change...
  Testing lock on exposure change...
  Testing lock on format change...
7. Checking skipped items, job cancellation, and summary counts...
  Summary terminal message: "2 developed, 0 failed, 1 skipped"
8. Checking state persistence across view unmount and remount...
9. Auditing touch target heights (>= 40px) and image naturalWidth...
  Thumbnail naturalWidths: [64,64,64,64,64,64], preview: 64
Wrote layout screenshot proof to frontend/rollchrome/layout-proof/develop.png
10. Checking serialized preview requests during exposure slider scrub...
  Burst preview requests: 2, max concurrent: 1
11. Checking assignments cleared on folder change...
12. Checking unmount and remount during thumbnail loading...
13. Checking running job persistence across remount...
14. Checking scope toggle: Selected only items and lock on selection change...

check:develop PASSED: all 14 checks verified successfully.
```

#### 16. `npm --prefix frontend/rollchrome run check:viewing`
```
Starting Vite harness server for Viewing check...
Harness server listening at http://[::1]:5173
Asserting 1: On opening, zoom is fit and no Full is requested...
  Initial open: 1 renders, Full count = 0, Fit active = true
Asserting 2: Cmd-1 gives canvas CSS width 3000 ± 1, and Full requested exactly once...
  Cmd-1: canvas CSS width = 3000, Full requested = 1
Asserting 3: At a zoom the settle proxy can serve, only Settle is requested...
  Zoom to fit: Settle count = 1, Full count = 0
Asserting 4: During slider drag at 100%, every request is Drag, and on release one Full...
  Slider scrub at 100%: 5 Drag requests, release stage = Full
Asserting 5: Ctrl-wheel at a point keeps the image point under it within 2 px...
  Ctrl-wheel about point: pixel diff = 0.00 px
Asserting 6: Panning far past an edge leaves no gap on that side...
  Pan far right gap = -1642.90625 px, Pan far left gap = -1750.09375 px
Asserting 7: Brush plain drag paints without panning; Space+drag pans without painting...
  Plain drag panned = false, Space-drag panned = true
Asserting 8: With text focused, Z does nothing...
  Text focused Z check: Zoom stayed at fit = true
Asserting 9: Side-by-side gives two canvases of equal CSS size fitting container, before recipe has identity adjustments & geometry...
  Side-by-side: before=541x361, after=541x361, fits=true
Asserting 10: Twenty slider steps while comparing send no further before requests...
  Twenty slider steps: before requests count before=1, after=1
Asserting 11: Split mode: dragging divider to 30% gives clip-path at 30% ± 1...
  Split divider drag clip-path: "polygon(0px 0px, 30% 0px, 30% 100%, 0px 100%)"
Asserting 12: Entering crop mode turns compare off and zoom to fit...
  Entering crop mode: compare off = true, fit = true, zoom disabled = true
Asserting 13: Every new control is at least 40 px...
  Control touch target audit: 0 violations
Asserting 14: Orientation 6, no geometry: at 100% visible canvas is 2000 × 3000 ± 1 and Full is requested...
  Orientation 6 (no geom): canvas = 2000x3000, Full count = 1
Asserting 15: Orientation 6 with geometry, and orientation 1 with rotate: 90: visible canvas is 2000 × 3000 ± 1...
  Orientation 6 with geom: 2000x3000, Orientation 1 rotate 90: 2000x3000
Asserting 16: A 40% crop at 100% requests Full, and visible canvas is 40% of full width ± 1...
  40% crop at 100%: canvas width = 1200 (expected 1200 ± 1), Full count = 1
Asserting 17: Orientation 6 panning past each edge leaves no gap, measured from visible canvas box...
  Orientation 6 pan gaps: left = -460.0 px, right = -458.0 px, top = -1169.0 px, bottom = -1169.0 px
Asserting 18: With brush active, double-click changes neither zoom nor pan...
  With brush active: zoom stayed "33%", pan position stayed (46, 87.796875)

--- check:viewing Summary ---
PASSED all 18 viewing requirements successfully.
```

#### 17. `npm --prefix frontend/rollchrome run check:colour`
```
Starting Vite harness server for check:colour...
Harness server listening at http://[::1]:5173
Asserting 1: W then click on orientation-1 photograph...
Asserting 2: The same on orientation 6, with point mapped...
Asserting 3: The same with a crop in the recipe, mapped through to-stored...
Asserting 4: Esc leaves the mode and sends nothing...
Asserting 5: At 100%, a drag while picking does not pan...
Asserting 6: Stubbed clamped result shows notice; stubbed refusal leaves sliders unchanged...
Asserting 7: With text focused, W and V do nothing...
Asserting 8: Auto sets the three values and leaves the others; Undo restores all three...
Asserting 9: V sets black_and_white, shows eight sliders in HSL order and hides HSL; V again restores HSL intact...
Asserting 10: In crop mode and while comparing eyedropper is disabled...
Asserting 11: Every new control is at least 40 px. Screenshot to layout-proof/...
  Proof screenshot saved to frontend/rollchrome/layout-proof/colour-tools.png

--- check:colour Summary ---
PASSED: all 11 colour tool checks passed cleanly.
```

#### 18. `npm --prefix frontend/rollchrome run check:history`
```
Starting Vite harness server for check:history...
Harness server listening at http://[::1]:5173
Asserting 1: Two slider commits, then Cmd-Z and Cmd-Shift-Z...
Asserting 2: Twenty input events and one release make exactly one step...
Asserting 3: A new change after an undo removes the redo...
Asserting 4: Cmd-Z with nothing to undo does nothing and sends nothing...
Asserting 5: The list shows labels newest first, current marked, clicking older restores it...
Asserting 6: Preset, crop, mask, Auto, eyedropper, B&W are each one step with proper label and undone...
Asserting 7: Undoing the creation of selected mask deselects it...
Asserting 8: With a text field focused, Cmd-Z is not intercepted...
Asserting 9: In crop mode, Cmd-Z does nothing...
Asserting 10: Opening another photograph empties the list...
Asserting 11: Snapshot save payload, restore as 1 step, undo restores recipe & keeps snapshot, delete removes from save...
Asserting 12: Empty and duplicate names refused with visible message...
Asserting 13: Pasting settings from photograph with snapshots brings none...
Asserting 14: On a read-only photograph, undo works and no save is sent...
Asserting 15: EditHistory alone (maxEntries limit and maxBytes limit)...
Asserting 16: Every new control is at least 40 px; screenshot to layout-proof/...
Saved proof screenshot to frontend/rollchrome/layout-proof/history.png

========================================
Assertions run: 16 / 16
All 16 assertions PASSED cleanly.
```

#### 19. `npm --prefix frontend/rollchrome run check:versions`
```
Starting Vite harness server for check:versions...
Harness server listening at http://[::1]:5173
Asserting 1: A photograph with no versions shows Main alone and offers no Delete and no Export all...
Asserting 2: New version with a name makes it current; the save payload has the old one in versions and version_name set...
Asserting 3: Switching exchanges the recipes: the sliders show the other version's values, a render and a save with them are sent...
Asserting 4: Editing one version leaves the other's values intact when switching back...
Asserting 5: Switching empties the history list...
Asserting 6: A snapshot is still listed after switching, and after switching back...
Asserting 7: Rename of the current and of another version reach the save payload; an empty and a duplicate name, and Main while it is taken, are refused with a visible message...
Asserting 8: Deleting another version removes it from the save payload...
Asserting 9: Export sends the current recipe, with version_name...
Asserting 10: Export all with three versions sends three exports with no two overlapping; with one stubbed failure the other two are still sent, and the summary shows two written and one failure with its reason...
Asserting 11: Pasting settings from a photograph with versions brings none, and leaves the current name...
Asserting 12: Undo after switching does not change the list of versions or the name...
Asserting 13: Apply a preset, then move a slider: the notice is gone. Cmd-Z still takes back the slider, then the preset...
Asserting 14: Every new control is at least 40 px. Screenshot to layout-proof/...
Saved screenshot to frontend/rollchrome/layout-proof/versions.png

========================================
Assertions run: 14/14
All 14 assertions PASSED!
```

#### 20. `npm --prefix frontend/rollchrome run check:detail`
```
Starting Vite harness server for check:detail...
Harness server listening at http://[::1]:5173
Asserting 1: Section has five sliders and toggle at defaults, first save clean of detail...
  Assertion 1 PASSED
Asserting 2: Moving slider sends Drag renders carrying detail; releasing at fit sends Settle...
  Assertion 2 PASSED
Asserting 3: Releasing at 100% sends Full...
  Assertion 3 PASSED
Asserting 4: Hint shows at fit and is gone at 100%...
  Assertion 4 PASSED
Asserting 5: Section reset returns all six to defaults and is one history step...
  Assertion 5 PASSED
Asserting 6: A change has the history label Detail, and Cmd-Z takes it back...
  Assertion 6 PASSED
Asserting 7: The toggle commits, and reaches the save payload...
  Assertion 7 PASSED
Asserting 8: Every new control is at least 40 px. Screenshot to layout-proof/...
Saved screenshot to frontend/rollchrome/layout-proof/detail.png
  Assertion 8 PASSED

========================================
Assertions run: 8/8
All 8 assertions PASSED!
```

---

## Review

*Written by the architect, after re-running the gates on the branch.*

### Round 1 — Changes requested

**Checked.** Re-ran every gate on `rp-7` at `5d39e95` with `--no-fail-fast`: fmt and clippy clean, build clean; workspace 1152, `phototools-core` 713, `rollchrome-core` 320, `phototools-desktop` 26, `rollchrome-desktop` 22, none failed or ignored; typecheck, build and every check pass, `check:detail` running its eight numbered assertions and counting them. The thirteen tests have the brief's names. Read `detail.rs` and the cache: the stage runs once, keyed properly, on the stored frame before geometry; hot pixels, guided filters and the rebuild are as specified. Stripping an identity `detail` on save is reported and is the right way to keep untouched sidecars clean.

1. **The sharpening radius does not do what the slider says.** `r_sharp = sigma.round().max(1.0)` is used as the radius of three box passes, and the sigma of three passes of radius `r` is `sqrt(r(r + 1))`: 1.41 for `r = 1`, 2.45 for `r = 2`. So:
   - every radius from 0.5 to 1.49 gives **exactly the same result**, at a sigma of 1.41, so a third of the slider does nothing, and the default is 40% wider than it says;
   - at a proxy, `radius × scale` is below one pixel and rounds up to the same `r = 1`, a sigma of 1.41 *proxy* pixels. At `Drag` (scale 0.2 on a 6000 px file) that is about 7 source pixels, so the halo on screen at fit is nothing like the export's. *A radius means the same at every stage* is the brief's requirement, and `the_radius_scales_with_the_proxy` passes without proving it.

   For this step use a real separable Gaussian: a normalised kernel of radius `ceil(3σ)`, with σ `max(0.35, radius × scale)`. σ is at most 3, so the kernel is small and a sliding sum buys nothing here. The guided filters keep their box filters. Add a test, `sharpening_radius_changes_the_result_and_narrows_below_a_pixel`: radii 0.6 and 1.2 give different results at scale 1.0, and at scale 0.25 with radius 1.0 the sharpening reaches no further than 2 pixels from an edge.

Then re-measure, as the brief asked, the milliseconds at `Drag`, `Settle` and `Full`, and the later `Full`. Counts: workspace 1153, `rollchrome-core` 321. Update `CLAUDE.md`. Write **Report round 2** as new text, deleting nothing, and list every file changed.
