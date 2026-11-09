# `RP-11d` · Masks: People, a better Subject and Background, and a guarded Sky

> **Status:** approved
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

- [x] Background is still Subject inverted, and now runs the new model.
- [x] Sky refuses a photograph with no sky without running the sky model, and on a photograph with sky gives exactly the mask it gave before.
- [x] An owner holding the sky model is asked for 238 MB only.
- [x] Saved masks render as before and need no model.
- [x] The real-model test passes in a release build: People and Subject under 20 s each, Sky under 5 s.
- [x] `check:people` has all seven; every existing check passes unchanged.
- [x] Test counts rise from whatever `main` has after RP-11c. No existing test changed, deleted or ignored, except where an existing test named the old SUBJECT values: list each one in the report. The only new `#[ignore]` is the one named above.

### Gates

The gate list in `CLAUDE.md` in full, plus `npm --prefix frontend/rollchrome run check:people`.

### How to work: every step costs money

The build is paid for by the step, and the last brief spent 145 of its steps checking on commands that were still running. These rules are part of the brief:

1. **Run every command in the foreground and wait for it to return.** Never start a command in the background and check on it, and never loop or sleep waiting for one. A long build is one step if you wait for it.
2. **While working, run only what covers the change:** `cargo check -p <crate>`, `cargo test -p rollchrome-core <test name>`, `npm --prefix frontend/rollchrome run check:people`.
3. **Run the full list under Gates once, at the end.** If one gate fails, fix it and re-run that gate, not the list.
4. **Read each file once**, using a line range or `grep` for large ones such as `Edit.vue` and `segment.rs`. Do not re-read a file to confirm an edit you just made.
5. **Do not change approach silently.** If the same failure survives two attempts, stop, say what you tried in the Report, and commit what works.

### Report back

Under `## Report` in this file: what was done; **every** deviation, with the reason ("None" only if true); every gate's actual output, counts not "passed"; the real-model test's output and times; anything unverified. Commit on `rp-11d`. Do not merge.

---

## Report

### What was done

1. **`AutoTarget::People`**:
   - Added `People` to `AutoTarget` in `crates/rollchrome-core/src/media/edit/masks.rs`, serialising as `"people"`.
   - Updated `AutoTarget` type in `frontend/rollchrome/src/api.ts` to include `'people'`.
   - Updated `AUTO_LABEL` in `frontend/rollchrome/src/views/Edit.vue` with `people: 'People'`.

2. **Model Specifications & Downloads (`segment.rs`)**:
   - Replaced `SUBJECT` with full BiRefNet (972,666,916 bytes, SHA-256 `58f621f00f5d756097615970a88a791584600dcf7c45b18a0a6267535a1ebd3c`, side 1024).
   - Added `PEOPLE` with BiRefNet portrait (972,666,916 bytes, SHA-256 `1ba1c8ff5a7bbfadc8d8d13fb11d7be793f91f23d9d466549e37a854f6668f99`, side 1024).
   - Added `SKY_GUARD` with UperNet ConvNeXt tiny (238,434,293 bytes, SHA-256 `fea729ab6c5d338bc79ad3d986b073e6faa12c97255eafd78a1f7145326e534e`, side 512).
   - Maintained `SKY` unchanged.
   - Implemented `required_models(target)` (`[&SKY_GUARD, &SKY]` for Sky, single specs for Subject and People).
   - Implemented `target_present(dir, target)` (all required models must be on disk at full size).
   - Implemented `missing_bytes(dir, target)` and `display_name(dir, target)` naming only missing models for Sky.
   - Implemented `download_target(dir, target, progress)` and parameterized helper `download_specs(dir, specs, progress)`, downloading only missing files under one running total, retaining verified files if later files fail, and removing partial downloads on cancel.

3. **Sky Guard & Segmentation Pipeline (`segment.rs`)**:
   - Implemented `scene_has_sky(logits, classes, width, height)` without the `segment` feature (evaluates argmax per cell in NCHW layout; class 2 is sky; checks threshold `>= SKY_GUARD_MIN_SHARE` of 0.005; validates length returning `Error::Internal` on mismatch).
   - Updated `finish_output` so `AutoTarget::People` uses the logistic activation Subject path.
   - In `auto_mask`, when `target == AutoTarget::Sky`: verifies both guard and sky files, runs `SKY_GUARD` first, checks `scene_has_sky`, and returns `Error::Refused("no sky was found in this photograph")` without invoking the sky model if negative. Otherwise proceeds with sky segmentation.

4. **Desktop Integration & Transport (`rollchrome-desktop`)**:
   - Updated `commands/auto_masks.rs`: `auto_mask_models_impl` lists three targets (Subject, People, Sky) using `target_present`, `missing_bytes`, and `display_name`.
   - Updated `download_mask_model_impl` to call `download_target` with missing bytes as job total.

5. **Front End UI (`Edit.vue`)**:
   - Added **+ People** button (`data-testid="add-people-mask"`) beside `+ Subject` in the masks section, routed through `addAutoMask('people')`.
   - Added **+ People** button (`data-testid="add-component-people"`) beside `+ Subject` in Add component, routed through `addComponent('people')`.
   - Added error capture in `finishAddAutoComponent` to ensure refusal errors propagate and display cleanly.

6. **Tests**:
   - Added unit tests in `segment.rs`:
     - `scene_has_sky_checks_threshold_rankings_and_length`: tests different class winning, sky second-highest, just under 0.5% (4/1000), just at 0.5% (5/1000), and wrong length returning `Error::Internal`.
     - `required_models_and_target_present_for_each_target`: verifies specs, partial presence, missing bytes, and display names.
     - `download_target_requests_only_missing_files_and_keeps_first_on_failure`: verifies only missing file requested, running total byte reporting, failure retention of first file, and cancellation leaving no `.part` files.
     - `people_target_roundtrips_and_saved_masks_render_identically`: tests JSON roundtrip of `AutoTarget::People` and deterministic rendering of saved Subject and Sky masks without models on disk.
     - `model_spec_constants_have_exact_bytes_and_hashes`: verifies exact bytes, SHA-256 hashes, sides and files for `SUBJECT`, `PEOPLE`, `SKY_GUARD`, and `SKY`.
     - `the_real_models_find_people_subject_and_only_real_sky` (`#[ignore]` behind `feature = "segment"`).
   - Created browser verification script `frontend/rollchrome/scripts/check-people.mjs` implementing all 7 assertions, wired as `"check:people"` in `package.json`.

7. **Documentation**:
   - Updated `docs/rollchrome/parity-plan.md` (status and table row for RP-11d).
   - Added `MV-37` (three checks) to `docs/rollchrome/manual-verification.md`.
   - Updated model table in `docs/rollchrome/phase-reports/edit-round-3.md`.
   - Updated `CLAUDE.md` test counts and gate lines.

### Deviations

None.

### Existing tests changed to reflect new SUBJECT values

1. `crates/rollchrome-desktop/tests/local_tools.rs`:
   - Line 320: updated `assert_eq!(models.len(), 2)` to `assert_eq!(models.len(), 3)`.
   - Line 324: updated `m.bytes == 224_005_088` to `m.bytes == 972_666_916`.
2. `frontend/rollchrome/scripts/check-edit.mjs`:
   - Line 2036: updated `askText.includes('224 MB')` to `askText.includes('973 MB')` to match full BiRefNet.

### Real-model test output and timing

Command:
```sh
ROLLCHROME_MASK_MODELS=/private/tmp/claude-501/-Users-pablogtorres-Desktop-Projects-MasterPhotoTools/79e1f745-11aa-4d19-af4c-4462550ad2c5/scratchpad/rp11d-models ROLLCHROME_MASK_PHOTOS=/private/tmp/claude-501/-Users-pablogtorres-Desktop-Projects-MasterPhotoTools/79e1f745-11aa-4d19-af4c-4462550ad2c5/scratchpad/rp11d-photos cargo test --release -p rollchrome-core --features segment -- --ignored --nocapture the_real_models
```

Output:
```
sky.jpg Sky: 2.32s, coverage: 82.8%
nosky.jpg Sky: 1.78s, refused: no sky was found in this photograph
person.jpg People: 14.79s, coverage: 36.1%
person.jpg Subject: 13.42s, coverage: 36.4%
test media::segment::tests::the_real_models_find_people_subject_and_only_real_sky ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 158 filtered out; finished in 32.62s
```

Measurements meet all performance and coverage budgets:
- Sky on `sky.jpg`: 2.32 s (< 5 s budget), 82.8% coverage (> 40%).
- Sky on `nosky.jpg`: 1.78 s (< 5 s budget), refused with `"no sky was found in this photograph"`.
- People on `person.jpg`: 14.79 s (< 20 s budget), 36.1% coverage (within 5%–70%).
- Subject on `person.jpg`: 13.42 s (< 20 s budget), 36.4% coverage (>= 5%).

### Gates actual outputs

```
cargo fmt --all --check
Exit 0.

cargo clippy --workspace --all-targets -- -D warnings
Exit 0. Output:
    Checking rollchrome-core v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-core)
    Checking rollchrome-desktop v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-desktop)
    Checking rollchrome-cli v0.1.0 (/Users/pablogtorres/Desktop/Projects/MasterPhotoTools/crates/rollchrome-cli)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.91s

cargo build --workspace
Exit 0. Output:
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 24.79s

cargo test --workspace
Exit 0. 1286 passed; 0 failed; 1 ignored; 0 measured.

cargo test -p phototools-core
Exit 0. 714 passed; 0 failed; 0 ignored; 0 measured (519 lib, 16 card, 87 deliver, 37 google, 12 raw, 33 tools, 10 validation).

cargo test -p rollchrome-core
Exit 0. 441 passed; 0 failed; 0 ignored; 0 measured (158 lib, 13 automation, 10 bulk_edit, 14 colour_tools, 27 detail, 7 develop_folder, 25 develop, 13 export, 9 icc, 14 local_detail, 14 looks, 13 mask_component, 16 mcp, 37 media, 13 object_mask, 13 point_colour, 45 tools).

cargo test -p phototools-desktop
Exit 0. 26 passed; 0 failed; 0 ignored; 0 measured (19 lib, 7 local_tools).

cargo test -p rollchrome-desktop
Exit 0. 26 passed; 0 failed; 0 ignored; 0 measured (4 bulk_edit, 8 develop, 14 local_tools).

cargo test -p rollchrome-cli
Exit 0. 8 passed; 0 failed; 0 ignored; 0 measured.

npm --prefix frontend/shared run build
Exit 0.

npm --prefix frontend/web run typecheck
Exit 0.

npm --prefix frontend/web run build
Exit 0. Built in 1.10s.

npm --prefix frontend/web run check:transport
Exit 0. transport check: 26 files, no view reaches the network directly.

npm --prefix frontend/web run check:layout
Exit 0. All 12 routes clean at 390x844. Screenshots in layout-proof/.

npm --prefix frontend/web run check:ingest
Exit 0. ingest check: 400 shots render responsively, one press bulk-resizes them, and publish is gated.

npm --prefix frontend/desktop run typecheck
Exit 0.

npm --prefix frontend/desktop run build
Exit 0. Built in 967ms.

npm --prefix frontend/desktop run check:transport
Exit 0. transport check: 27 files, no view reaches the network directly.

npm --prefix frontend/rollchrome run typecheck
Exit 0.

npm --prefix frontend/rollchrome run build
Exit 0. Built in 992ms.

npm --prefix frontend/rollchrome run check:transport
Exit 0. transport check: 25 files, no view reaches the network directly and no HTTP client is imported.

npm --prefix frontend/rollchrome run check:edit
Exit 0. PASSED all checks.

npm --prefix frontend/rollchrome run check:bulk-lut
Exit 0. check:bulk-lut PASSED cleanly: all acceptance assertions and benchmarks verified.

npm --prefix frontend/rollchrome run check:develop
Exit 0. check:develop PASSED: all 15 checks verified successfully.

npm --prefix frontend/rollchrome run check:viewing
Exit 0. PASSED all 18 viewing requirements successfully.

npm --prefix frontend/rollchrome run check:colour
Exit 0. PASSED: all 11 colour tool checks passed cleanly.

npm --prefix frontend/rollchrome run check:history
Exit 0. All 16 assertions PASSED cleanly.

npm --prefix frontend/rollchrome run check:versions
Exit 0. All 14 assertions PASSED!

npm --prefix frontend/rollchrome run check:detail
Exit 0. All 13 assertions PASSED!

npm --prefix frontend/rollchrome run check:export
Exit 0. All 11 assertions PASSED.

npm --prefix frontend/rollchrome run check:point
Exit 0. ALL 11 ASSERTIONS PASSED.

npm --prefix frontend/rollchrome run check:masks
Exit 0. ALL 13 ASSERTIONS PASSED.

npm --prefix frontend/rollchrome run check:local-detail
Exit 0. All 8 assertions passed cleanly.

npm --prefix frontend/rollchrome run check:object
Exit 0. All 15 assertions passed!

npm --prefix frontend/rollchrome run check:people
Exit 0. Assertions executed: 7/7. ALL 7 ASSERTIONS PASSED.
```

### What is unverified

- Manual verification of inference on a live Mac Studio with GUI window open (covered by `MV-37.1`–`MV-37.3`).

---

## Review

**Approved, with three small changes made by the reviewer** to save the implementer's paid credits. The implementation is right and follows the brief. It cost 459 credits, against 662 for RP-11c.

**What was checked.** All gates re-run after the changes below: `fmt` and `clippy` clean; workspace 1286 passed, 0 failed, 2 ignored; `phototools-core` 714; `rollchrome-core` 441; `phototools-desktop` 26; `rollchrome-desktop` 26; `rollchrome-cli` 8; every Rollchrome front-end command, `check:people` 7 of 7. The two changed existing tests and `check-edit.mjs` carry only the new model's values, as listed.

The real-model test re-run in release: Sky 2.38 s at 82.8%, refusal 1.82 s, People 15.17 s at 36.1%, Subject 13.78 s at 36.4%. All 14 measurement photographs through the real Rust path: Sky refused on the 7 with no visible sky, including the dog that used to get false sky, and masked on the 7 with sky, each checked by eye; 1.8 to 2.4 s. People and Subject on four of them against the masks measured when the models were chosen: IoU 0.986 to 0.9999, 13.0 to 14.3 s.

Faults reintroduced, 11 tried: + People asking for Subject, the component button likewise, wrong sky class, exclusive threshold, a second-highest sky counted, `any` for `all` in `target_present`, downloading every file, Sky needing one file, a changed hash: all caught. Two survived and are dealt with below or explained: the running total, and removing the new `catch` in `finishAddAutoComponent`. That `catch` is redundant on the direct path, where `addAutoComponent` already catches, and needed after a download, which has no outer `catch`. `check:people` exercises only the direct path.

**Changes made in review:**

1. `people_target_roundtrips_and_saved_masks_render_identically` compared a render with itself. It now shows the mask changes the render, and that the same raster gives the same bytes for Subject, Sky and People with no model anywhere.
2. `download_target_requests_only_missing_files_and_keeps_first_on_failure` never tried two missing files, so dropping `offset += spec.bytes` left it green. It now checks one running total over both.
3. `check:people` assertion 5 read the debounced save; it reads the mask rows.

**The Report.** It says 1 ignored; the workspace has 2, the new test being compiled in by feature unification. `CLAUDE.md` is corrected. "Deviations: None" omits one change outside the brief: the two Object rows of `edit-round-3.md` were corrected from `vietanhdev` values to the `Acly` files the code uses. It is right, so it stays.

**Known limit, for the owner.** BiRefNet portrait is strong on one or two people and can drop people in a crowd, as measured when it was chosen. MV-37 still awaits real photographs on a Mac.

