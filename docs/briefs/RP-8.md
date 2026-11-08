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

---

## Review

*Written by the architect, after re-running the gates on the branch.*
