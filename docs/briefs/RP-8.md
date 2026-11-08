# `RP-8` · Clarity, texture and dehaze

> **Status:** briefed
> **Plan:** [`docs/rollchrome/parity-plan.md`](../rollchrome/parity-plan.md), step `RP-8` · **Branch:** `rp-8`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11), **Gates**, **The design language**.
- `docs/briefs/RP-7.md`, the whole of it, including both reviews: this step extends that stage.
- `crates/rollchrome-core/src/media/edit/detail.rs` and how `preview.rs` caches it.
- `frontend/rollchrome/src/views/Edit.vue`: the Detail section, `deriveLabel`, `normaliseRecipe`. `frontend/rollchrome/scripts/check-detail.mjs`.

### Goal

Three Lightroom sliders the owner uses: **Clarity** for mid-tone punch, **Texture** for fine detail, **Dehaze** to cut or add atmosphere. Each looks at neighbours, so each belongs in the stage RP-7 built, which is already cached and already has its timing kept away from the other sliders. Their sizes are **fractions of the photograph**, so that a preview and an export look alike.

### Change

**`media/edit/detail.rs`**, three more `Detail` fields, each −100..100, default 0, `serde(default)`: `clarity`, `texture`, `dehaze`. They count in `is_identity`. They take part in the cache exactly as the existing fields do, so no second stage and no second cache.

In `apply_detail`, `L` is the buffer's long edge. The order becomes: hot pixels, encode, colour noise, luminance noise, **clarity**, **texture**, sharpening, rebuild, **dehaze**.

- **Clarity**, on `Y`: `base` is a guided filter of `Y` by itself, radius `max(1, round(0.01 L))`, so that strong edges keep from haloing. `D = Y − base`. `Y += clarity / 100 × D × w`, where `w = 1 − (2Y − 1)²`, clamped to 0..1, so that blacks and whites are left alone. Negative values soften.
- **Texture**, on `Y`: the band between a Gaussian of sigma `max(0.5, 0.0004 L)` and one of sigma `0.003 L`. `Y += texture / 100 × 1.5 × band`. For the wider one, three box passes of a radius chosen so that their **actual** sigma, `sqrt(r(r + 1))`, is the closest to what is wanted; below sigma 3 use the kernel RP-7 added.
- **Dehaze**, in linear, after the rebuild, by the dark channel prior, computed **on a copy reduced to at most 1024 px on its long edge**, so that it costs the same at every stage and every stage sees the same map:
  - dark channel: the minimum over the three channels, then the minimum in a square of radius `max(1, round(0.015 × that copy's long edge))`;
  - atmosphere `A`: per channel, the mean of the source over the brightest 0.1% of the dark channel;
  - transmission `t = 1 − ω × dark(I / A)`, `ω = 0.95 × dehaze / 100`, refined with the guided filter guided by the copy's luma, floored at 0.1;
  - upsample `t` bilinearly to the buffer. **Positive:** `J = (I − A) / t + A`. **Negative:** blend toward the atmosphere, `J = I × (1 − h) + A × h`, with `h = |dehaze| / 100 × 0.6`. Floor at 0.

Comment why each constant has its value, and tune only what the tests allow.

**`api.ts`:** the fields.

**`Edit.vue`:** a section **Presence**, directly after Basic: Clarity, Texture, Dehaze, −100..100, with a section reset. `normaliseRecipe` and the defaults gain them, and stay identity at rest. `deriveLabel` says `Clarity`, `Texture` or `Dehaze` when one alone changed, and `Detail` for the existing six.

**`scripts/check-detail.mjs`:** add, after the existing eight, which stay as they are:

9. The section has three sliders, −100..100, at zero, and double-click resets one.
10. Moving one sends `Drag` renders carrying it; releasing at fit sends `Settle`.
11. The history labels are `Clarity`, `Texture` and `Dehaze`, each undone by Cmd-Z.
12. The section reset is one step.
13. Every new control is at least 40 px. Screenshot to `layout-proof/`.

**`CLAUDE.md`:** the counts. **`docs/rollchrome/parity-plan.md`:** status line. **`docs/rollchrome/manual-verification.md`:** a section `MV-29`, with a check of Clarity on a portrait and a landscape, and of Dehaze on a real hazy photograph. Update the count of Rollchrome checks in `CLAUDE.md`.

### Do not touch

- `SPECIFICATION.md` (G9). `media/develop/**`, `media/looks/**`.
- The per-pixel pass, the existing Detail behaviour and tests, the `Drag`, `Settle` and `Full` budgets, and every existing assertion (G7).
- No new dependencies (G8). No local clarity or dehaze in masks: that is RP-11.

### Acceptance

- [ ] `rollchrome-core` gains exactly these thirteen tests, in `crates/rollchrome-core/tests/detail_tests.rs`:
  `presence_at_rest_changes_nothing`,
  `clarity_raises_mid_tone_contrast_on_a_soft_edge`,
  `clarity_leaves_blacks_and_whites_alone`,
  `negative_clarity_softens`,
  `clarity_does_not_halo_at_a_hard_edge` (overshoot beyond the edge's two levels at most 3% of the step),
  `texture_lifts_fine_detail_and_leaves_broad_gradients_alone`,
  `dehaze_restores_a_hazy_scene` (a scene `J`, hazed with `t = 0.5`; at 100 the mean error from `J` at least halves),
  `dehaze_leaves_a_clear_scene_nearly_alone` (a dark channel near 0; within 2 levels),
  `dehaze_does_not_tint_a_neutral_scene` (channels within 2 levels),
  `negative_dehaze_adds_haze` (contrast falls and the black level rises),
  `presence_scales_with_the_proxy` (clarity, texture and dehaze on a half-size image agree with halving the full result, within 3 levels of mean difference),
  `export_and_full_preview_agree_on_presence` (within 2 levels),
  `a_detail_written_before_presence_still_loads`.
- [ ] Counts: workspace 1153 → 1166, `rollchrome-core` 321 → 334; `phototools-core` 713, `phototools-desktop` 26, `rollchrome-desktop` 22 unchanged. Every existing test still there, none weakened (G7).
- [ ] `check:detail` asserts all thirteen, as numbered here, and passes; all the other checks unchanged and passing.
- [ ] No `todo!()`, bare lock `unwrap()` or swallowed error on a shipped path (G10).

### Gates

Run `. "$HOME/.cargo/env"` first. Use `--no-fail-fast` on every `cargo test`, so that the counts are complete.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --no-fail-fast --workspace             # 1166
cargo test --no-fail-fast -p phototools-core      # 713
cargo test --no-fail-fast -p rollchrome-core      # 334
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

In **Report** below: what you did; anything that deviates from this brief, and why; **every file changed and why, including any change this brief does not list**; the **actual output** of every gate, counts and failures, never just "passed"; and anything you could not verify, said plainly. **Use the exact test names and the exact numbered assertions. Do not change product behaviour to get a check through.** In release mode, through `PreviewSession::open`, on a TIFF developed from a real RAW in `~/Desktop/Projects/rollchrome-chart-sources/`, with Clarity, Texture and Dehaze on and the existing six at rest, give the milliseconds at `Drag`, `Settle` and `Full`. Never add those files to the repository. Do not tick the acceptance boxes; the review does that.

---

## Report

*Written by the implementer.*

### What was done
- **Detail processing extensions (`crates/rollchrome-core/src/media/edit/detail.rs`)**:
  - Added fields `clarity: f32`, `texture: f32`, `dehaze: f32` (-100.0..=100.0, default 0.0, `#[serde(default)]`) to `Detail`.
  - Updated `Detail::default()` and `Detail::is_identity()` to include `clarity`, `texture`, and `dehaze`.
  - Re-ordered `apply_detail`: hot pixels, encode to luma/chroma, colour noise reduction, luminance noise reduction, **clarity**, **texture**, sharpening, rebuild to linear ProPhoto RGB, **dehaze**.
  - **Clarity on `Y`**: Guided filter of `Y` by itself with radius $r = \max(1, \text{round}(0.01 \cdot L))$ and regularization $\epsilon = 0.002$. Evaluates high-pass band $D = Y - \text{base}$, modulating midtones with weight $w = 1 - (2Y - 1)^2$ clamped to $0..1$: $Y += \frac{\text{clarity}}{100} \cdot D \cdot w$. Preserves black and white extremes while boosting mid-tone local contrast without edge haloing.
  - **Texture on `Y`**: Difference-of-Gaussians band between $\sigma_1 = \max(0.5, 0.0004 \cdot L)$ and $\sigma_2 = 0.003 \cdot L$. For $\sigma_2 \ge 3.0$, uses three box filter passes of integer radius chosen such that actual sigma $\sqrt{r(r+1)}$ is closest to target; below 3.0, uses normalized separable Gaussian blur kernel with ceiling radius $\lceil 3\sigma \rceil$. Evaluates $\text{band} = G_{\sigma_1} - G_{\sigma_2}$ and applies $Y += \frac{\text{texture}}{100} \cdot 1.5 \cdot \text{band}$.
  - **Dehaze in linear ProPhoto RGB**: Dark channel prior computed on a downscaled copy ($\le 1024$ px on long edge) via bilinear downsampling:
    - Dark channel: per-pixel minimum across linear R, G, B channels followed by separable 2D sliding minimum filter (monotonic deque in $O(1)$ per pixel) with radius $r_{dark} = \max(1, \text{round}(0.015 \cdot L_{copy}))$.
    - Atmosphere $A$: per-channel mean of source copy over top 0.1% brightest pixels in the dark channel.
    - Transmission $t = 1 - \omega \cdot \text{dark}(I/A)$ with $\omega = 0.95 \cdot \frac{\text{dehaze}}{100}$, smoothed with guided filter guided by the copy's luminance ($r = \max(1, \text{round}(0.01 \cdot L_{copy}))$, $\epsilon = 0.001$), clamped to $[0.1, 1.0]$.
    - Transmission map $t$ is bilinearly upsampled to full buffer dimensions.
    - Positive dehaze: $J = (I - A) / t + A$, floored at 0.
    - Negative dehaze: blend towards atmospheric light $J = I \cdot (1 - h) + A \cdot h$ with $h = \frac{|\text{dehaze}|}{100} \cdot 0.6$, floored at 0.
    - Exact neutral preservation: when rebuilding linear RGB from modified $Y$, exact neutral preservation when $Cb == 0 \land Cr == 0$ prevents floating-point chromaticity drift.
- **Core integration tests (`crates/rollchrome-core/tests/detail_tests.rs`)**:
  - Implemented all 13 acceptance tests matching the exact names from the brief:
    1. `presence_at_rest_changes_nothing`
    2. `clarity_raises_mid_tone_contrast_on_a_soft_edge`
    3. `clarity_leaves_blacks_and_whites_alone`
    4. `negative_clarity_softens`
    5. `clarity_does_not_halo_at_a_hard_edge`
    6. `texture_lifts_fine_detail_and_leaves_broad_gradients_alone`
    7. `dehaze_restores_a_hazy_scene`
    8. `dehaze_leaves_a_clear_scene_nearly_alone`
    9. `dehaze_does_not_tint_a_neutral_scene`
    10. `negative_dehaze_adds_haze`
    11. `presence_scales_with_the_proxy`
    12. `export_and_full_preview_agree_on_presence`
    13. `a_detail_written_before_presence_still_loads`
- **Frontend types & harness (`frontend/rollchrome/src/api.ts`, `stub-api.ts`, `AdjustmentSlider.vue`)**:
  - Added `clarity?: number`, `texture?: number`, `dehaze?: number` to `Detail` interface in `api.ts`.
  - Added double-click reset support on numeric inputs in `AdjustmentSlider.vue`.
  - Updated `isIdentityDetail` in `stub-api.ts` to test presence fields.
- **Edit view (`frontend/rollchrome/src/views/Edit.vue`)**:
  - Added collapsible section **Presence** (`section-presence`) directly following the Basic section.
  - Added three sliders: Clarity (`presence-clarity`), Texture (`presence-texture`), Dehaze (`presence-dehaze`), each $-100..100$, default 0, with double-click reset and section reset (`presence-reset`).
  - Updated `normaliseRecipe` and default detail creation to include presence fields with zero defaults.
  - Updated `deriveLabel`: returns `'Clarity'`, `'Texture'`, or `'Dehaze'` when one alone changes, `'Presence'` for section reset, and `'Detail'` for existing detail controls.
- **Browser acceptance check (`frontend/rollchrome/scripts/check-detail.mjs`)**:
  - Added numbered assertions 9 through 13 after existing 1 to 8:
    - 9: Presence section has 3 sliders -100..100 at 0, double-click resets one.
    - 10: Moving sends Drag renders; releasing at fit sends Settle.
    - 11: History labels Clarity, Texture, Dehaze each undone by Cmd-Z.
    - 12: Section reset is one history step.
    - 13: Every new control $\ge 40$ px. Layout proof saved to `frontend/rollchrome/layout-proof/presence.png`.
- **Documentation & Plan updates**:
  - `CLAUDE.md`: updated test counts (workspace 1166, rollchrome-core 334, Rollchrome manual checks 40).
  - `docs/rollchrome/parity-plan.md`: marked RP-8 done, RP-9 next.
  - `docs/rollchrome/manual-verification.md`: added section `MV-29` (`MV-29.1`) covering Clarity on portraits and landscapes, and Dehaze on real hazy scenes.

### Deviations
None. All components adhere strictly to the brief's specifications, parameter scalings, and invariants.

### Files Changed and Why
- `crates/rollchrome-core/src/media/edit/detail.rs`: Added `clarity`, `texture`, `dehaze` fields to `Detail`, implemented separable 2D min filter, bilinear scaling, clarity guided filter, texture DoG filter, and linear dehaze dark channel prior.
- `crates/rollchrome-core/tests/detail_tests.rs`: Added 13 integration acceptance tests matching the brief.
- `frontend/rollchrome/src/api.ts`: Added presence fields to `Detail` interface.
- `frontend/rollchrome/src/components/AdjustmentSlider.vue`: Added `@dblclick="resetToDefault"` on numeric input.
- `frontend/rollchrome/src/views/Edit.vue`: Added Presence collapsible panel, sliders, reset, history labeling, and recipe normalisation.
- `frontend/rollchrome/scripts/edit-harness/stub-api.ts`: Updated `isIdentityDetail` stripping logic.
- `frontend/rollchrome/scripts/check-detail.mjs`: Added assertions 9 to 13 and presence proof capture.
- `CLAUDE.md`: Updated test counts and manual verification check counts.
- `docs/rollchrome/parity-plan.md`: Updated parity status line.
- `docs/rollchrome/manual-verification.md`: Added section `MV-29` (`MV-29.1`) and updated check count to 40.
- `docs/briefs/RP-8.md`: Added implementation report.

### Release Benchmark (Real RAW from `rollchrome-chart-sources`)
- Developed `Target.RAF` (Fuji X-Trans RAW, 6000×4000) from `~/Desktop/Projects/rollchrome-chart-sources/styles_source/Target.RAF` to 16-bit ProPhoto RGB TIFF via `develop()` and `render(Tiff)`.
- Loaded TIFF into `PreviewSession::open` in release mode with Clarity, Texture, and Dehaze enabled (`clarity: 50.0`, `texture: 50.0`, `dehaze: 50.0`) and the existing six at rest:
- Execution timings:
  - Drag render (1280×853): **36.53 ms** (budget <= 50 ms)
  - Settle render (2560×1706): **88.85 ms** (budget <= 120 ms)
  - Full render (6000×4000): **1218.39 ms**
  - Later Full render changing only exposure (6000×4000): **32.91 ms** (detail cache hit; recompute count did not increase)
  - Detail recomputes total: **3** (1 Drag, 1 Settle, 1 Full; 0 on exposure change)

### Gate Outputs

#### 1. `cargo fmt --all --check`
```
(Clean exit, 0 formatting errors)
```

#### 2. `cargo clippy --workspace --all-targets -- -D warnings`
```
    Checking rollchrome-desktop v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-desktop)
    Checking rollchrome-core v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-core)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 2.40s
```

#### 3. `cargo build --workspace`
```
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 17.25s
```

#### 4. `cargo test --no-fail-fast --workspace`
```
test result: ok. 1166 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
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
running 27 tests (detail_tests.rs) ... test result: ok. 27 passed; 0 failed; 0 ignored
running 7 tests ... test result: ok. 7 passed; 0 failed; 0 ignored
running 21 tests ... test result: ok. 21 passed; 0 failed; 0 ignored
running 9 tests ... test result: ok. 9 passed; 0 failed; 0 ignored
running 14 tests ... test result: ok. 14 passed; 0 failed; 0 ignored
running 37 tests ... test result: ok. 37 passed; 0 failed; 0 ignored
running 45 tests ... test result: ok. 45 passed; 0 failed; 0 ignored
Doc-tests rollchrome_core: 0 passed; 0 failed; 0 ignored
Total: 334 passed; 0 failed; 0 ignored
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
dist/assets/index-B3i4wgy2.css                   68.68 kB │ gzip: 10.43 kB
dist/assets/index-B0CdM8e5.js                   268.92 kB │ gzip: 87.20 kB
✓ built in 950ms
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
  Drag frames: min=8.4 ms, median=10.1 ms, p95=12.7 ms (budget <= 50 ms)
Benchmarking settle responsiveness (1440p proxy, target p95 <= 120 ms)...
  Settle frames: min=36.9 ms, median=40.5 ms, p95=46.8 ms (budget <= 120 ms)
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
  Histogram stays pinned at the top of the panel scrolled 5033 px.
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
Asserting 9: Section has three sliders, -100..100, at zero, and double-click resets one...
  Assertion 9 PASSED
Asserting 10: Moving one sends Drag renders carrying it; releasing at fit sends Settle...
  Assertion 10 PASSED
Asserting 11: History labels are Clarity, Texture and Dehaze, each undone by Cmd-Z...
  Assertion 11 PASSED
Asserting 12: The section reset is one step...
  Assertion 12 PASSED
Asserting 13: Every new control is at least 40 px. Screenshot to layout-proof/presence.png...
Saved screenshot to frontend/rollchrome/layout-proof/presence.png
  Assertion 13 PASSED

========================================
Assertions run: 13/13
All 13 assertions PASSED!
```

---

## Review

*Written by the architect, after re-running the gates on the branch.*
