# `RP-4` · Colour tools: eyedropper, auto settings, black & white

> **Status:** briefed
> **Plan:** [`docs/rollchrome/parity-plan.md`](../rollchrome/parity-plan.md), step `RP-4` · **Branch:** `rp-4`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11), **Gates**, **The design language**.
- `crates/rollchrome-core/src/media/edit/preview.rs`: `PreviewSession`, `render_rgba_frame_masked`. `media/edit/pipeline.rs`: the export path, which is a **second implementation** of the same stages. `media/edit/hsl.rs`, all of it, in particular `chroma_fade`.
- `frontend/rollchrome/src/views/Edit.vue`: `showNotice` and its Undo; the viewport pointer handlers; the Basic and Colour sections. `components/MaskOverlay.vue`: how a click becomes a stored-frame coordinate (`to-stored`, orientation).
- `frontend/rollchrome/scripts/check-viewing.mjs` — the pattern for the new check.

### Goal

Three tools the owner uses constantly in Lightroom. An **eyedropper** that makes the clicked spot neutral. An **Auto** button that gets exposure and range about right. A **black & white** mode with a mix per colour. Each is small. What has to be right is that each one works from the photograph, repeats exactly, tells the truth when it cannot do what was asked, and looks the same on screen and in the export.

### Change

**Eyedropper.** In `PreviewSession`:

```rust
pub struct WhiteBalancePick { pub temperature: f32, pub tint: f32, pub clamped: bool }
/// `x`, `y` are 0..1 in the stored frame. Reads the unadjusted settle proxy.
pub fn white_balance_at(&self, x: f32, y: f32) -> Result<WhiteBalancePick, Error>;
```

- Mean linear R, G, B of a 5×5 box at that point, on `settle_linear`. **Always the unadjusted proxy**, so that picking the same spot twice gives the same answer whatever the sliders say.
- The white balance gains are `R·2^(t/2)`, `G·2^(−g/2)`, `B·2^(−t/2)`, with `t`, `g` being the sliders over 100. Neutral means `t = log2(B/R)` and `g = log2(G² / (R·B))`. Multiply by 100, clamp to ±100, and set `clamped` if either was limited.
- A point outside 0..1, a box with any channel at or above 0.98 (clipped) or with luminance below 0.005 (nothing to read) is `Error::Refused`, in words the owner can act on.

**Auto.** `pub struct AutoTone { pub exposure: f32, pub whites: f32, pub blacks: f32 }` and `pub fn auto_tone(&self) -> Result<AutoTone, Error>` on `PreviewSession`, working on the drag proxy from an **identity recipe**: it analyses the photograph, not the current edit. Use the **real renderer** with bisection, 12 steps each, so that the numbers are the ones the pipeline will produce:

1. *Exposure*, in ±2 EV: median display luma (Rec. 709 of the 8-bit output) to `clamp(median at zero, 95, 135)`. A photograph already in that band gets exactly 0.
2. *Whites*, in ±50, with that exposure: if the 99.5th percentile of the display luma is below 235, raise it to 235; if it is 254 or above, lower it to 250; otherwise 0.
3. *Blacks*, in ±50: if the 0.5th percentile is above 12, lower it to 12; otherwise 0.

It does not touch white balance, because a JPEG or a developed TIFF is already balanced and a grey-world guess would spoil it. Comment that.

**Black & white.** In `pipeline.rs`, a recipe field `black_and_white: Option<BlackAndWhite>`, `serde(default, skip_serializing_if = "Option::is_none")`, `version` unchanged. `BlackAndWhite` holds one `f32` for each of the eight bands, −100..100, in `hsl.rs`, named as `HslAdjustments`'s are. It joins `is_identity`.

- At the existing OkLCh stage, in **both** pipelines: lightness moves by the band-weighted mix at the pixel's hue, computed with the same weights and scale as the HSL luminance control, and then **chroma is set to exactly 0**. Do not do this with HSL saturation at −100: `chroma_fade` leaves near-neutral pixels faintly coloured.
- While it is on, the HSL bands and hue rotation are ignored. Global saturation and vibrance have no effect by construction. Colour grading and LUTs come later in the pipeline and still apply: that is how a selenium or sepia print is made.

**`crates/rollchrome-desktop`:** commands `white_balance_at(session_id, x, y)` and `auto_tone(session_id)`, `async` with `spawn_blocking` as `render_preview` is, with `_impl`s and delegating methods on `AppState` (G1), registered in `main.rs`.

**`frontend/rollchrome/src/api.ts`:** the two commands and the recipe field.

**`Edit.vue`:**

- *Eyedropper:* a button beside Temperature and the key `W`. A picking mode with a crosshair cursor. A click turns the viewport position into stored-frame 0..1 **with the existing mapping** (`to-stored`, orientation: factor it out of `MaskOverlay.vue` if you must, without changing what it does), calls the command, sets `temperature` and `tint`, renders and saves, and leaves the mode. Offer Undo through `showNotice`, as presets do. If `clamped`, the notice says the slider range was reached. A refusal is shown, and the sliders do not move. `Esc` leaves the mode. While picking, a drag does not pan. Disabled in crop mode and while comparing.
- *Auto:* a button in the Basic header. Sets the three values, renders, saves, and offers Undo through `showNotice`. Other sliders do not change. Show that it is working.
- *Black & white:* a toggle in the colour area and the key `V`. A **B&W Mix** section with eight sliders appears when it is on, using the same band colours and order as the HSL section, with reset. The HSL section is hidden while it is on, and its values are kept for when it is turned off.
- Masks' local saturation is unaffected. Keys are ignored while text is in play.

**`frontend/rollchrome/scripts/check-colour.mjs`, a harness** (extend the edit harness's stub), and `check:colour` in `package.json`, asserting:

1. `W` then a click on an orientation-1 photograph sends the expected `x`, `y` ± 0.01; the sliders take the stubbed values; a `Settle` render and a save follow.
2. The same on orientation 6, with the point mapped.
3. The same with a crop in the recipe, mapped through `to-stored`.
4. `Esc` leaves the mode and sends nothing.
5. At 100%, a drag while picking does not pan.
6. A stubbed `clamped` result shows the notice; a stubbed refusal is shown and leaves the sliders where they were.
7. With text focused, `W` and `V` do nothing.
8. Auto sets the three values and leaves the others; Undo restores all three.
9. `V` sets `black_and_white`, shows eight sliders in the HSL order and hides the HSL section; `V` again shows it with its values intact.
10. In crop mode and while comparing the eyedropper is disabled.
11. Every new control is at least 40 px. Screenshot to `layout-proof/`.

**`CLAUDE.md`:** the gate and counts. **`docs/rollchrome/parity-plan.md`:** status line, and in the RP-4 row that Auto sets exposure, whites and blacks only.

### Do not touch

- `SPECIFICATION.md` (G9). `media/develop/**`, `media/looks/**`, `tools/**` apart from what a new recipe field forces.
- The `Drag`, `Settle` and `Full` budgets and every existing assertion (G7). With no `black_and_white` the pipelines must produce the same bytes as before.
- No new dependencies (G8). No auto white balance, no auto B&W mix, no point colour.

### Acceptance

- [ ] `rollchrome-core` gains exactly these thirteen tests, in a new `crates/rollchrome-core/tests/colour_tools_tests.rs`:
  `picking_a_grey_patch_with_a_cast_makes_it_neutral` (through the real render, channels within 2 levels),
  `picking_the_same_spot_again_gives_the_same_values_whatever_the_recipe`,
  `picking_beyond_the_slider_range_clamps_and_says_so`,
  `picking_a_clipped_or_black_spot_is_refused`,
  `a_point_outside_the_frame_is_refused`,
  `auto_lifts_an_underexposed_photograph_into_the_band` (median display luma 95 ± 4),
  `auto_leaves_a_well_exposed_photograph_alone` (exposure exactly 0),
  `auto_puts_the_white_and_black_points_on_target` (235 ± 4 and 12 ± 4),
  `black_and_white_output_has_no_colour_anywhere` (every pixel of a gradient that includes near-neutrals, `R == G == B` within 1),
  `the_red_band_darkens_red_and_leaves_blue`,
  `colour_grading_still_tints_a_black_and_white_photograph`,
  `export_and_preview_agree_on_black_and_white` (within 2 levels),
  `a_recipe_written_before_black_and_white_still_loads`.
- [ ] `rollchrome-desktop` gains exactly two: `white_balance_at_refuses_an_unknown_session`, `auto_tone_refuses_an_unknown_session`.
- [ ] Counts: workspace 1111 → 1126, `rollchrome-core` 281 → 294, `rollchrome-desktop` 20 → 22, `phototools-core` 713, `phototools-desktop` 26 unchanged. Every existing test still there, none weakened (G7).
- [ ] `check:colour` asserts all eleven and passes; all the other checks unchanged and passing.
- [ ] No `todo!()`, bare lock `unwrap()` or swallowed error on a shipped path (G10); no logic in the binary (G1).

### Gates

Run `. "$HOME/.cargo/env"` first. Use `--no-fail-fast` on every `cargo test`, so that the counts are complete.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --no-fail-fast --workspace             # 1126
cargo test --no-fail-fast -p phototools-core      # 713
cargo test --no-fail-fast -p rollchrome-core      # 294
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
```

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why; **every file changed and why, including any change this brief does not list**; the **actual output** of every gate, counts and failures, never just "passed"; and anything you could not verify, said plainly. **Do not change product behaviour to get a check through.** In release mode, on a TIFF developed from a real RAW in `~/Desktop/Projects/rollchrome-chart-sources/`, through `PreviewSession::open`, give the seconds `auto_tone` takes and the values it returns. Never add those files to the repository. Do not tick the acceptance boxes; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
