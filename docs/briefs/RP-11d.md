# `RP-11d` · Masks: People, a better Subject and Background, and a guarded Sky

> **Status:** briefed
> **Plan:** [`docs/rollchrome/parity-plan.md`](../rollchrome/parity-plan.md), step `RP-11d` · **Branch:** `rp-11d`, cut from `main` **after RP-11c is merged**, because both change `segment.rs` and the Masks panel

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11), **Gates**.
- `crates/rollchrome-core/src/media/segment.rs`, all of it. `media/edit/masks.rs`: `AutoTarget`.
- `crates/rollchrome-desktop/src/commands/auto_masks.rs`.
- `frontend/rollchrome/src/views/Edit.vue`: `AUTO_LABEL`, `addAutoMask`, `autoDownload`, `onModelDownloaded`, `runAutoMask`, `addComponent`, `finishAddAutoComponent`, the `+ Subject` buttons (the panel's and Add component's).
- `frontend/rollchrome/scripts/check-masks.mjs`, `scripts/edit-harness/stub-api.ts`: the pattern for the new check.

### Goal

The owner is replacing Lightroom, and has said the quality of sky, background and people masks is what matters most to them. Three changes, each decided by measurement on 14 photographs on this M1 Max (Python ONNX Runtime, CPU, arena off):

| Mask | Decision | Download | Per photo | Peak memory | Why |
|---|---|---|---|---|---|
| **People** (new) | BiRefNet portrait | 973 MB | 10.9 s | 8.8 GB | Targets people: leaves out the bicycle and the tub that the general model keeps |
| **Subject and Background** | Replace BiRefNet lite with full BiRefNet | 973 MB | 11.0 s (lite: 5.6 s) | 8.9 GB | The finest hair, whiskers and thin objects. Background is Subject inverted, so one change improves both. **The owner chose this, knowing it is about twice as slow** |
| **Sky** | **Keep** the U²-Net sky model, add a guard before it | 238 MB more | adds about 1.5 s | not isolated | EoMT, UperNet and OneFormer all lost the fine branches and palm fronds the present model keeps. Its fault is inventing sky: 13% of a photograph with none. The guard scored 0.0% on the 8 photographs with no sky and 25–61% on the 6 with sky |

The same measurements rejected, on purpose: judging separate regions against the guard (it punched holes in a tree crown) and refining edges against the photograph (smudges). **Do not add either** (G11).

### Change

**`media/edit/masks.rs`:** `AutoTarget::People`, serialised `people`. Saved `subject` and `sky` masks load and render unchanged: **a stored mask is never recomputed**.

**`media/segment.rs`:**

```rust
pub const SUBJECT: ModelSpec = ModelSpec {
    target: AutoTarget::Subject,
    name: "BiRefNet (MIT), 973 MB",
    file: "birefnet_general.onnx",
    url: "https://huggingface.co/onnx-community/BiRefNet-ONNX/resolve/main/onnx/model.onnx",
    sha256: "58f621f00f5d756097615970a88a791584600dcf7c45b18a0a6267535a1ebd3c",
    bytes: 972_666_916,
    side: 1024,
};

pub const PEOPLE: ModelSpec = ModelSpec {
    target: AutoTarget::People,
    name: "BiRefNet portrait (MIT), 973 MB",
    file: "birefnet_portrait.onnx",
    url: "https://huggingface.co/onnx-community/BiRefNet-portrait-ONNX/resolve/main/onnx/model.onnx",
    sha256: "1ba1c8ff5a7bbfadc8d8d13fb11d7be793f91f23d9d466549e37a854f6668f99",
    bytes: 972_666_916,
    side: 1024,
};

pub const SKY_GUARD: ModelSpec = ModelSpec {
    target: AutoTarget::Sky,
    name: "UperNet ConvNeXt tiny (MIT), 238 MB",
    file: "sky_guard.onnx",
    url: "https://huggingface.co/JElvis/upernet-convnext-tiny-ade-onnx/resolve/main/model.onnx",
    sha256: "fea729ab6c5d338bc79ad3d986b073e6faa12c97255eafd78a1f7145326e534e",
    bytes: 238_434_293,
    side: 512,
};
```

`SKY` is unchanged. A file left behind by the old Subject model (`birefnet_lite.onnx`) is left alone: deleting things in the person's folder is not part of this step.

*The model contracts, as measured:*

- *BiRefNet, both:* exactly the present Subject path. `prepare_input`, input `input_image` `[1, 3, 1024, 1024]`, one output `[1, 1, 1024, 1024]` of **logits** (measured -76 to 471), through the logistic function. `finish_output`'s Subject arm serves People.
- *Guard:* `prepare_input(upright, 512)`, input `pixel_values` `[1, 3, 512, 512]`; output `logits` `[1, 150, 128, 128]`, class first. **Class 2 is sky.** Per cell, the class with the highest value.

*What a target needs:* `required_models(target) -> &'static [&'static ModelSpec]`: `[&SKY_GUARD, &SKY]` for Sky, one file for the others. `spec` stays for the single file a target's mask is made by. `target_present(dir, target)` is true only when every required file is at full size. `download_target(dir, target, progress)` fetches **only the missing files**, reporting one running total over those, stopping on cancel; a file that verified stays if a later one fails. The owner already has the sky model on disk and must be asked for 238 MB, not 414. `missing_bytes(dir, target)` and a `display_name` for the person, which for Sky names only what is missing.

*The guard:* without the `segment` feature, so `core` tests it (G2):

```rust
/// True when at least `SKY_GUARD_MIN_SHARE` (0.005) of the scene model's cells are sky.
pub fn scene_has_sky(logits: &[f32], classes: usize, width: usize, height: usize) -> Result<bool, Error>;
```

A wrong length is `Error::Internal`, as `finish_output` does. In `auto_mask`, for Sky only: verify **both** files, run the guard first, and if it says no, return `Error::Refused("no sky was found in this photograph")` **without running the sky model**, so no blank mask is made. Otherwise the sky path exactly as today. **The guard decides for the whole photograph and nothing else: it does not change a single pixel of the mask.** `run_model` takes the guard's output unchanged if you generalise it; the output's shape is checked by `scene_has_sky`.

**`rollchrome-desktop`, `commands/auto_masks.rs`:** transport only (G1). `auto_mask_models_impl` lists three targets using `target_present`, `missing_bytes` and the display name; `download_mask_model_impl` uses `download_target`, the job's total being the missing bytes.

**`api.ts`:** `AutoTarget` gains `'people'`.

**`Edit.vue`:** `AUTO_LABEL.people = 'People'`. A **+ People** button (`data-testid="add-people-mask"`) beside `+ Subject`, through `addAutoMask`, and one in Add component (`data-testid="add-component-people"`), through `addComponent`, using the same download-and-keep-the-intent path as Subject. Nothing else: the refusal reaches the person through the existing `catch` in `runAutoMask` and `finishAddAutoComponent`; make sure of it in the check.

**`scripts/check-people.mjs`**, `check:people` in `package.json`, each measured in the browser and failing if the control is missing:

1. `+ People` sends `make_auto_mask` with `people`; a mask `People 1` of kind `auto`, target `people`, is selected, overlay shown, and in the save payload.
2. Add component → People makes a component with `op: add` and target `people`, the base shape unchanged.
3. Model absent: the download is asked for with its name, and on completion `People 1` is made.
4. As 3, from Add component: a component, not a new mask.
5. A refused Sky (`no sky was found in this photograph`) creates no mask and shows those words.
6. A refused Sky component leaves the mask's components unchanged and shows those words.
7. Both new buttons are at least 40 px; screenshot to `layout-proof/`.

**Tests in `rollchrome-core`**, names as sentences, at least:

- `scene_has_sky`: just under and just at 0.5%; a cell where sky is second-highest is not sky; a different class winning everywhere is false; a wrong length is an error.
- `required_models` for each target; `target_present` false with only one Sky file.
- `download_target` over a local server: with one Sky file present, **only the other is requested**, and the total is that file's bytes; a bad second file leaves the first; cancel leaves no `.part`. The specs are consts, so take the file list as an inner function's parameter as `download_model` and `fetch` do, and test that.
- `AutoTarget::People` round-trips as `people`; a saved Subject and Sky mask render the same bytes as before this step.
- The specs: SUBJECT, PEOPLE and SKY_GUARD have the bytes and hashes above, and SKY still has its own.

**One test with real models, `#[ignore]`d**, behind `segment`: `the_real_models_find_people_subject_and_only_real_sky`, reading `ROLLCHROME_MASK_MODELS` and `ROLLCHROME_MASK_PHOTOS`, failing in words if unset. `sky.jpg`: Sky covers more than 40%. `nosky.jpg`: Sky is refused with the message above. `person.jpg`: People covers between 5% and 70%, and Subject covers at least as much as 5%. Print each time. Run it with:

```sh
ROLLCHROME_MASK_MODELS=/private/tmp/claude-501/-Users-pablogtorres-Desktop-Projects-MasterPhotoTools/79e1f745-11aa-4d19-af4c-4462550ad2c5/scratchpad/rp11d-models ROLLCHROME_MASK_PHOTOS=/private/tmp/claude-501/-Users-pablogtorres-Desktop-Projects-MasterPhotoTools/79e1f745-11aa-4d19-af4c-4462550ad2c5/scratchpad/rp11d-photos cargo test --release -p rollchrome-core --features segment -- --ignored --nocapture the_real_models
```

Those folders hold the verified models and three photographs: **read them, never copy them into the repository.**

Docs: `docs/rollchrome/parity-plan.md`, a row and the status; `CLAUDE.md` counts and the `check:people` gate lines; `segment.rs`'s header, which names the models; a new **MV-37** in `docs/rollchrome/manual-verification.md`, three checks; the table in `docs/rollchrome/phase-reports/edit-round-3.md`'s **The models**, marked RP-11d.

### Do not touch

- `SKY`, the sky model's preprocessing and output. `MaskKind::Object`, click selection, MobileSAM.
- `rollchrome-cli`, `automation`. The memory settings of `run_model`.
- `SPECIFICATION.md` (G9). No new crate (G8). No other model, no body-part masks (G11).
- **Never read or change the owner's own configuration file** in tests or the check.

### Acceptance

- [ ] Background is still Subject inverted, and now runs the new model.
- [ ] Sky refuses a photograph with no sky without running the sky model, and on a photograph with sky gives exactly the mask it gave before.
- [ ] An owner holding the sky model is asked for 238 MB only.
- [ ] Saved masks render as before and need no model.
- [ ] The real-model test passes in a release build: People and Subject under 20 s each, Sky under 5 s.
- [ ] `check:people` has all seven; every existing check passes unchanged.
- [ ] Test counts rise from whatever `main` has after RP-11c. No existing test changed, deleted or ignored, except where an existing test named the old SUBJECT values: list each one in the report. The only new `#[ignore]` is the one named above.

### Gates

The gate list in `CLAUDE.md` in full, plus `npm --prefix frontend/rollchrome run check:people`.

### Report back

Under `## Report` in this file: what was done; **every** deviation, with the reason ("None" only if true); every gate's actual output, counts not "passed"; the real-model test's output and times; anything unverified. Commit on `rp-11d`. Do not merge.

---

## Report

---

## Review
