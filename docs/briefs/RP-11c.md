# `RP-11c` · Masks: click to select an object

> **Status:** briefed
> **Plan:** [`docs/rollchrome/parity-plan.md`](../rollchrome/parity-plan.md), step `RP-11c` · **Branch:** `rp-11c`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11), **Gates**, **The design language**.
- `crates/rollchrome-core/src/media/segment.rs`, all of it. `media/edit/raster.rs`. `media/edit/masks.rs`: `MaskKind`, `MaskComponent`, `compile_mask`, `compile_shape`.
- `crates/rollchrome-desktop/src/commands/auto_masks.rs`; `crates/rollchrome-desktop/src/lib.rs`: `ActivePreview`, `segmentation_input`; `main.rs`, the command list.
- `frontend/rollchrome/src/views/Edit.vue`: the eyedropper path (`eyedropperTarget`, the click handler around line 427, `frameToStored`, `exitEyedropper`), `addAutoMask`, `autoDownload`, `onModelDownloaded`, `runAutoMask`, `addComponent`, `componentSummary`, the `auto-download` block. `components/MaskOverlay.vue`.
- `frontend/rollchrome/scripts/check-masks.mjs` and `scripts/edit-harness/stub-api.ts`: the pattern for the new check.

### Goal

The owner is replacing Lightroom, whose "Select Object" lets you point at a thing and get a mask of it. Rollchrome finds *the* subject and *the* sky, but cannot be told *that* person, *that* car. This step adds it: **click an object, it is selected; click again to add to it; Alt-click to take away.**

### Decision: the model, measured

Measured on this M1 Max, CPU provider, arena and memory pattern off (as `run_model`), on five Commons photographs (fur, a touching couple, two overlapping vehicles, a tree against sky, a cyclist), eleven to fourteen prompts:

| Model | Download | Load | Encode one photo | One click | Peak memory | Result |
|---|---|---|---|---|---|---|
| **MobileSAM** | **44.7 MB** | 0.2 s | **0.44 s** | 33 ms | **1.0 GB** | Whole objects from one click; two clicks combine; Alt-clicks are obeyed |
| SlimSAM-77 | 39.8 MB | 0.3 s | 1.53 s | 30 ms | 2.4 GB | Parts of objects |
| SAM 2.1 tiny | 155 MB | 0.4 s | 1.14 s | 29 ms | 2.3 GB | Parts, or the object plus its neighbour; two clicks fragment; subtraction weak |
| SAM 2.1 small | 183 MB | 0.5 s | 1.33 s | 28 ms | 2.4 GB | As tiny |

**MobileSAM it is.** No new crate: `ort` is already here. Weights Apache-2.0 (`ChaoningZhang/MobileSAM`), ONNX export by `Acly/MobileSAM`, MIT.

| File | URL | Bytes | SHA-256 |
|---|---|---|---|
| Encoder `mobile_sam_image_encoder.onnx` | `https://huggingface.co/Acly/MobileSAM/resolve/main/mobile_sam_image_encoder.onnx` | 28,157,093 | `580f5fb648ea1062c0aabc26217aed56921985f03f0cbbd852bba81d760cc749` |
| Decoder `sam_mask_decoder_multi.onnx` | `https://huggingface.co/Acly/MobileSAM/resolve/main/sam_mask_decoder_multi.onnx` | 16,496,559 | `8976b90a87ba50a6a72217a5ff994f7d25ce16f2229fcc1ed259e1294c622ffe` |

**The exact contract, which is what was measured. Do not "improve" it:**

- *Encoder:* input `input_image`, shape `[H, W, 3]`, **f32, RGB, values 0–255, HWC**, the upright photograph resized so its **long edge is exactly 1024**, not padded, not normalised (the graph does both). Output `image_embeddings` `[1, 256, 64, 64]`. Resize with `imageops::resize(.., FilterType::Triangle)`, as `prepare_input` does.
- *Decoder inputs:* `image_embeddings`; `point_coords` `[1, N+1, 2]`, f32, x then y **in pixels of the resized frame**, with one extra sentinel point `(0, 0)` last; `point_labels` `[1, N+1]`, f32, `1` add, `0` subtract, sentinel `-1`; `mask_input` zeros `[1, 1, 256, 256]`; `has_mask_input` `[0.0]`; `orig_im_size` `[resized_h, resized_w]`, f32, height first.
- *Decoder outputs:* `masks` `[1, 4, h, w]` logits at `orig_im_size`; `iou_predictions` `[1, 4]`.
- *Choosing the mask:* **one click:** the highest `iou_predictions` of indices **1, 2, 3**. **Two or more clicks:** index **0**.

### Change

**`media/edit/masks.rs`:**

```rust
/// One click on the photograph, stored-frame normalised like every mask position.
pub struct ObjectClick { pub u: f32, pub v: f32, pub add: bool }

MaskKind::Object { clicks: Vec<ObjectClick>, mask: StoredRaster }
```

`Object` renders exactly as `Auto` does: in `compile_mask` both for the base and for a component, through `rasters.stored`. `clicks` are kept so a mask can be refined after the photograph is reopened; **the stored raster is what renders and exports**, never recomputed by a render. Saved masks load unchanged.

**`media/segment.rs`.** The object model is two files, so give it a type of its own rather than bending `ModelSpec`, whose `target` is an `AutoTarget`:

- `OBJECT_ENCODER`, `OBJECT_DECODER`: name, file, url, sha256, bytes. `object_model_present(dir)`: both files at full size. `download_object_model(dir, progress)`: both through the existing `fetch`, sequentially, reporting **one** running total over both files' bytes, stopping on cancel, each kept only if it verifies. If the second fails, the first stays: it verified.
- Without the feature, so `core` tests them (G2):
  - `upright_click(u, v, orientation) -> (u, v)`: stored-normalised to upright-normalised, the same map as `to_upright`, all eight orientations.
  - `prepare_object_input(upright: &RgbImage) -> ObjectInput { data: Vec<f32>, width, height }`, the encoder contract.
  - `object_prompt(clicks, orientation, width, height) -> (Vec<f32>, Vec<f32>)`: coordinates and labels with the sentinel. Refuses (`Error::Refused`, in words) no clicks; more than **32** clicks; no `add` click at all; any coordinate not finite or outside 0..=1.
  - `choose_object_mask(iou: &[f32], clicks: usize) -> usize`, the rule above.
  - `finish_object_mask(logits: &[f32], width, height, orientation) -> Result<StoredRaster, Error>`: each value through the logistic function to 0..255 (**not** a threshold: the soft edge is what makes a mask look right), then `from_upright`, then `StoredRaster::encode` with the decoder's SHA. If **no pixel reaches 128**, `Error::Refused("nothing was found there; click on the object itself")`.
- Behind `segment`:

```rust
pub struct ObjectSelector { /* decoder session, embedding, width, height, orientation */ }
impl ObjectSelector {
    /// Verifies both files, runs the encoder once, and drops it: its memory is not held.
    pub fn new(stored: &RgbImage, orientation: u32, models_dir: &Path) -> Result<Self, Error>;
    /// One decoder run. Tens of milliseconds, so the person can click freely.
    pub fn select(&mut self, clicks: &[ObjectClick]) -> Result<StoredRaster, Error>;
}
```

Sessions built as `run_model` builds them: CPU, arena off, memory pattern off. No CoreML; `segment.rs`'s header says why.

**`rollchrome-desktop`.** G1: transport only.

- `ActivePreview` gains `object_selector: Arc<Mutex<Option<ObjectSelector>>>`, so it dies with the photograph. The `active_preview` lock is held only to clone that `Arc` and copy the pixels, **never while a model runs**: `mask_coverage` and rendering use the same lock and must not stall.
- `commands/auto_masks.rs`: `object_model_status() -> { name, present, bytes }`; `download_object_model() -> job id`, as `download_mask_model_impl`; `select_object(session_id, clicks) -> StoredRaster`, async, on a blocking thread as `make_auto_mask`, building the selector on the first click of a photograph and reusing it after. A session that is not the open one is refused. Register in `main.rs`.

**`frontend/rollchrome/src/api.ts`:** the types and three methods. The `object` kind wherever `MaskOverlay.vue` and `Edit.vue` list the raster kinds (`auto`, `luminance_range`, `colour_range`): an object mask has no handles. `componentSummary` and the mask's label say `Object`.

**`Edit.vue`:**

- **`+ Object`** (`data-testid="add-object-mask"`) beside `+ Subject`, disabled at the mask limit. If the model is absent, the existing `autoDownload` block asks first, using this model's name and bytes; when the download finishes, **the intent is kept** (RP-11 Review 1 found that bug once) and the mode starts.
- Entering makes `eyedropperTarget` the new `'object'`. **Unlike every other target, a click does not leave the mode.** A plain click adds, Alt-click subtracts. A notice in the panel (`data-testid="object-hint"`): *"Click the object. Click again to add, Alt-click to take away."* and a **Done** button (`data-testid="object-done"`). Esc also leaves, and changes nothing further.
- The click goes through the same `frameToStored` path as the eyedropper, so crop, rotation and flip are already handled. Do not write a second mapping.
- The first click of a new mask creates it only on success (`Object 1`, `Object 2`, …), selected, overlay shown. A failed first click creates nothing and shows the error in words. A failed later click leaves the mask as it was, and the click is not recorded.
- Each successful click is **one undo step** labelled `Masks`.
- **Ignore a click while one is running** (the first takes about half a second; show `Looking at the photograph…`). A result arriving for another photograph, or after the mode has ended, is dropped, using the `seq` / session pattern of `runAutoMask`.
- Undo that removes the mask being refined leaves the mode. So does changing photograph.
- For a selected Object mask, a **Refine** button (`data-testid="refine-object"`) re-enters the mode with its saved clicks.
- **Add component** gains **+ Object** (`data-testid="add-component-object"`): the same mode, its clicks and raster going to a new `Add` component of the selected mask, so "this gradient but not that person" works. Refine works on a selected Object component too. Use one state, `{ maskId, componentIndex | null }`.

**`scripts/check-object.mjs`** with `check:object` in `package.json`, and the stub extended, each measured in the browser and each failing if the control is missing:

1. `+ Object` with the model present enters the mode: hint shown, crosshair cursor, no mask yet.
2. A click sends `select_object` once with one click, `add: true`, at the stored coordinates expected for that canvas point; a mask `Object 1` of kind `object` is selected, with the raster in the save payload.
3. A second click sends **both** clicks and replaces the raster; still one mask.
4. Alt-click sends `add: false`.
5. Each click is one undo step labelled `Masks`: Cmd-Z after the second click returns to one click and the first raster.
6. Esc and Done each leave the mode; a click afterwards sends nothing.
7. With the photograph rotated 90° and cropped, the coordinates sent equal what the eyedropper's `colour_at` receives for the same point.
8. A click while one is in flight sends nothing more (hold the stub's reply).
9. A refused first click creates no mask and shows the words; a refused later click leaves clicks and raster unchanged.
10. A reply that arrives after a change of photograph changes nothing.
11. Model absent: the download is asked for with this model's name, and after it completes the mode starts.
12. Refine on a saved Object mask sends its saved clicks plus the new one.
13. Add component → Object makes a component with `op: add`, the base shape unchanged.
14. Undoing the mask's creation leaves the mode.
15. `+ Object`, Done, Refine and Add component's Object are at least 40 px; screenshot to `layout-proof/`.

**Tests in `rollchrome-core`** (new `tests/object_mask_tests.rs`, plus unit tests in `segment.rs`), names as sentences, at least:

- `upright_click` for all eight orientations, proved against `to_upright` with a marker pixel, as `orientation_round_trips_and_agrees_with_the_geometry` does.
- The encoder input is HWC, 0–255, long edge exactly 1024 on landscape and portrait, and a known pixel value survives.
- The prompt puts x before y in resized pixels, appends `(0, 0)` with label `-1`, labels add `1` and subtract `0`.
- Each refusal, one test each, the message asserted.
- `choose_object_mask`: index 0 is never chosen for one click even when its score is highest; index 0 is always chosen for two.
- `finish_object_mask`: a logit of 0 is 127 or 128, not 0 or 255 (the edge is soft); orientation 6 lands where the geometry says; nothing found is refused.
- `download_object_model` over a local server (`segment.rs` has `serve_once`; you will need both files served): both kept; a wrong second file leaves the first and no second; cancel leaves no `.part`; the running total reaches the sum of both files.
- `MaskKind::Object` renders byte-identically to `MaskKind::Auto` with the same raster, as a base and as a component, in preview **and** export.
- A sidecar with an Object mask round-trips with its clicks; a sidecar with none loads unchanged. A preset never carries one.
- Changing an Object mask's raster changes the next render (the raster cache is keyed by the PNG's hash; prove it for this kind).

**One test with the real model, `#[ignore]`d** as it needs a download, behind `segment`: `the_real_model_selects_the_object_that_was_clicked`. It reads the folder from `ROLLCHROME_OBJECT_MODELS` and fails in words if unset. A synthetic 3000 × 2000 picture, dark background, one bright disc and one bright square apart from it. Click the disc: at least 90% IoU with the disc, and the square not selected. Click the disc and the square: both selected. Click the disc then Alt-click the square: disc only. Print the encoder and click times. Run it and put those numbers in the report.

Docs: `docs/rollchrome/parity-plan.md` status and row; `CLAUDE.md` status line, counts and the `check:object` gate lines; a new **MV-36** in `docs/rollchrome/manual-verification.md`, three checks; the two models' row in the table in `docs/rollchrome/phase-reports/edit-round-3.md`'s section **The models**, marked RP-11c.

### Do not touch

- `run_model`, `auto_mask`, `SUBJECT`, `SKY` and how those masks behave.
- `MaskOverlay.vue`'s behaviour for any other kind.
- The eyedropper behaviour of any other target.
- `rollchrome-cli` and `automation`: no click selection there.
- `SPECIFICATION.md` (G9). No new crate (G8). No click markers drawn on the picture, no box selection, no brush-to-object, no other model (G11: ideas go in the report).
- **Never read or change the owner's own configuration file** in tests or the check.

### Acceptance

- [ ] `MaskKind::Object` renders as `Auto` does, and a mask from a saved sidecar needs no model to render or export.
- [ ] One click selects, more clicks refine, Alt-click subtracts, one undo step each.
- [ ] The encoder runs once a photograph and its session is dropped; the decoder is reused. `the_real_model…` passes and its encoder time is under 2 s and click under 150 ms in a release build on this Mac.
- [ ] `active_preview` is not held while a model runs.
- [ ] A photograph with no Object mask renders exactly as before: the golden tests are untouched and pass.
- [ ] `check:object` has all fifteen assertions; every existing check passes unchanged.
- [ ] Test counts: workspace 1264 → more, `rollchrome-core` 420 → more, `rollchrome-desktop` 25 → 25 or more, the others unchanged. No existing test changed, deleted or ignored; the only new `#[ignore]` is the one named above.

### Gates

```sh
. "$HOME/.cargo/env"
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace
cargo test -p phototools-core
cargo test -p rollchrome-core
cargo test -p phototools-desktop
cargo test -p rollchrome-desktop
cargo test -p rollchrome-cli
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
npm --prefix frontend/rollchrome run check:object
```

### Report back

Under `## Report` in this file: what was done; **every** deviation from this brief, with the reason ("None" only if true); every gate's actual output, counts not "passed"; the real-model test's output and times; anything unverified. Commit on `rp-11c`. Do not merge.

---

## Report

---

## Review
