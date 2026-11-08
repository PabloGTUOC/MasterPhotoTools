# `RP-3` · Viewing: zoom, pan and before/after compare

> **Status:** briefed
> **Plan:** [`docs/rollchrome/parity-plan.md`](../rollchrome/parity-plan.md), step `RP-3` · **Branch:** `rp-3`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11), **Gates**, **The design language**.
- `crates/rollchrome-core/src/media/edit/preview.rs`, all of `PreviewSession`, including why the proxies exist.
- `crates/rollchrome-desktop/src/commands/edit.rs` (`open_preview`, `render_preview`), `commands/develop.rs::preview_develop` for
  an `async` command with `spawn_blocking`.
- `frontend/rollchrome/src/views/Edit.vue`: `displayedDimensions`, `canvasStyle`, `paintPixels`, `executeRender`, `requestRender`, `setBefore`, the
  key handlers, and the viewport template.
- `frontend/rollchrome/scripts/check-edit.mjs` and `scripts/edit-harness/` — the pattern for the new check.

### Goal

Today a photograph is only ever seen fitted to the window, from a proxy 2560 px on its long edge. Focus, sharpening and noise cannot be judged, and
RP-7 will add controls that can only be judged at 100%. **So zoom must reach real pixels**: scaling the proxy would show a soft picture and
call it 100%. And the only comparison is holding `\`; the owner needs the before and the after side by side, and a split.

### Change

**A third preview stage, `Full`.** In `media/edit/preview.rs`:

- `PreviewStage::Full`: every source pixel, up to `FULL_MAX_EDGE = 8192` on the long edge, because a browser canvas has an area limit. A
  larger photograph is reduced to that, and that is what 100% shows. Comment the reason.
- It goes through **the same paths** as `Drag` and `Settle`: `render`, `CachedGeometry` (a `full_transformed` slot), `compile_masks`, `stored_dims`, `mask_coverage`.
  Export and the other two stages do not change.
- **Decoded only when first asked for**, held in the session, and freed with it. `open` remembers the path and decodes again with `decode_image`, so that a session that
  never zooms costs what it costs today. Sessions built from an `ImageBuffer` keep a clone to decode from; check that only tests and harnesses use those.
- Add `pub fn full_decodes(&self) -> usize` (a counter, as `geom_recompute_count` is), `pub fn full_dimensions(&self) -> (u32, u32)` (from the source's size, without decoding), and a pure
  `fit_full_edge(w, h, max_edge) -> (u32, u32)`.
- No new bare `unwrap()` on a lock: `unwrap_or_else(|e| e.into_inner())`.

**`crates/rollchrome-desktop`:** `render_preview` becomes `async` with `spawn_blocking`, as `preview_develop` is, because a sync command runs on the main thread and a first `Full`
takes most of a second. `render_preview_impl` stays as the tests call it. `OpenPreviewResult` gains `full_width`, `full_height` and `settle_width`, `settle_height` (the stored proxy's size).

**`frontend/rollchrome/src/api.ts`:** `PreviewStage` gains `'Full'`, and the open result its four fields.

**`frontend/rollchrome/src/views/Edit.vue`:**

*Zoom.* State `zoom`: `'fit'`, or a scale where **100% means one source pixel per device pixel**. So at 100% the displayed CSS width of the uncropped photograph is `full_width / devicePixelRatio`; with a crop, scale by the ratio of
the proxy to the source (`full_width / settle_width`). Range: fit to 400%.

- Size the **stage** by multiplying `displayedDimensions`, not with a CSS `scale`, so that the overlays inside it keep working and their 40 px handles stay 40 px.
- The container clips; the stage moves by a pan offset. **Clamp the pan** so the photograph cannot leave the view, and centre any axis that is smaller than the view. Re-clamp on window resize.
- **Stage choice:** wherever a render now asks for `Settle`, ask for `Full` instead if the displayed size needs more pixels than the settle proxy has. Slider drags stay `Drag`, and releasing gives `Full`. Changing zoom renders only if the needed stage changes.
- Show a quiet "Loading full resolution…" while the first `Full` is on its way.

*Gestures.* Cmd-1 is 100%, Cmd-0 is fit, Cmd-= and Cmd-- step through 25, 50, 100, 200, 400%, `Z` toggles fit and 100%, double-clicking the photograph toggles too. Ctrl-wheel or a pinch zooms about the pointer; plain wheel pans when zoomed. Dragging pans, except where a
mask tool or a mask handle takes the pointer; holding **Space** and dragging always pans. Keys are ignored while text is in play, as the existing shortcuts do. Zooming **about a point keeps that image point under it.** Buttons in the viewport bar: Fit, 100%, and a percentage readout.

*Compare.* A `compareMode`: `off`, `side-by-side`, `split`. `Y` toggles side-by-side and Shift-Y toggles split, plus buttons in the viewport bar. Holding `\` still works and does not change.

- The before is the identity recipe **with the current geometry**, so that the two frames line up. Paint it on a second canvas.
- **Render it only when its geometry or stage changes**, never for a slider step, in the same in-flight queue as the after, one at a time.
- *Side-by-side:* each half fits half the viewport; zoom and pan are shared. *Split:* the before over the after, clipped with `clip-path` at a divider with a 40 px draggable handle, default 50%.
- Label the halves **Before** and **After**.
- While comparing, mask and crop overlays are hidden. **Entering crop mode turns compare off and returns to fit**, and zoom buttons are disabled in crop mode, because `CropOverlay` assumes the fitted stage.

**`frontend/rollchrome/scripts/check-viewing.mjs`, a harness** (reuse the edit harness's stub, extended), `check:viewing` in `package.json`, asserting numbers, at a fixed `devicePixelRatio` of 2, with a stub 6000×4000 source and 2560×1707 settle proxy:

1. On opening, zoom is fit and no `Full` is requested.
2. Cmd-1: the canvas's CSS width is 3000 ± 1, and `Full` is requested exactly once.
3. At a zoom the settle proxy can serve, only `Settle` is requested.
4. During a slider drag at 100% every request is `Drag`, and on release one `Full`.
5. Ctrl-wheel at a point keeps the image point under it within 2 px.
6. Panning far past an edge leaves no gap on that side.
7. With a mask brush active, a plain drag paints and does not pan; with Space held it pans and paints nothing.
8. With text focused, `Z` does nothing.
9. Side-by-side: two canvases of equal CSS size fit the container. The before's request has identity adjustments and the recipe's geometry.
10. Twenty slider steps while comparing send no further before requests.
11. Split: dragging the divider to 30% gives `clip-path` at 30% ± 1.
12. Entering crop mode turns compare off and zoom to fit.
13. Every new control is at least 40 px. Screenshots of zoomed, side-by-side and split go to `layout-proof/`.

**`CLAUDE.md`:** add the gate and the counts. **`docs/rollchrome/parity-plan.md`:** status line.

### Do not touch

- `SPECIFICATION.md` (G9). Export, `tools/**`, `media/develop/**`, `media/looks/**`.
- The `Drag` and `Settle` budgets and every existing assertion in `check:edit` (G7).
- The adjustment pipeline's arithmetic. No new dependencies (G8). No region rendering: `Full` renders the whole frame.
- `Develop.vue`, `BulkLut.vue`.

### Acceptance

- [ ] `rollchrome-core` gains exactly these five tests: `the_full_stage_renders_every_source_pixel`,
  `the_full_stage_fits_the_canvas_limit` (on `fit_full_edge`),
  `opening_a_session_and_rendering_drag_and_settle_decodes_nothing_more` (`full_decodes() == 0`),
  `the_full_buffer_is_decoded_once` (two renders, `full_decodes() == 1`),
  `the_full_stage_applies_crop_and_masks_as_settle_does` (uniform patches, a crop and a radial mask; within two levels at corresponding positions).
- [ ] `rollchrome-desktop` gains exactly one: `open_preview_reports_the_full_and_settle_dimensions`.
- [ ] Counts: workspace 1102 → 1108, `rollchrome-core` 273 → 278, `rollchrome-desktop` 19 → 20, `phototools-core` 713, `phototools-desktop` 26 unchanged. Every existing test still there, none weakened (G7).
- [ ] `check:viewing` asserts all thirteen and passes; `check:edit`, `check:bulk-lut`, `check:develop` unchanged and passing.
- [ ] No `todo!()` or swallowed error on a shipped path (G10); no logic in the binary (G1).

### Gates

Run `. "$HOME/.cargo/env"` first.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace             # 1108
cargo test -p phototools-core      # 713
cargo test -p rollchrome-core      # 278
cargo test -p phototools-desktop   # 26
cargo test -p rollchrome-desktop   # 20
npm --prefix frontend/shared run build
npm --prefix frontend/rollchrome run typecheck
npm --prefix frontend/rollchrome run build
npm --prefix frontend/rollchrome run check:transport
npm --prefix frontend/rollchrome run check:edit
npm --prefix frontend/rollchrome run check:bulk-lut
npm --prefix frontend/rollchrome run check:develop
npm --prefix frontend/rollchrome run check:viewing
```

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why, **including any change not listed in it**; the **actual output** of every gate, counts and failures, never just "passed"; and anything you could not
verify, said plainly. In release mode, on a TIFF developed from a real RAW in `~/Desktop/Projects/rollchrome-chart-sources/`, give the seconds for the first `Full` render and a later one with a LUT applied. Never add those files to the repository. Do not tick the
acceptance boxes; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
