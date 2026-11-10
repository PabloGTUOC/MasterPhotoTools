# `DV-5` · Noise reduction in Develop, before the look

> **Status:** approved
> **Plan:** [`docs/develop-plan.md`](../develop-plan.md), step `DV-5` · **Branch:** `dv-5`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md`: **Ground rules** (G1–G11) and **Gates**.
- `crates/rollchrome-core/src/media/edit/detail.rs` lines 100–180: `Detail` and `apply_detail`, the noise
  reduction Edit already has (RP-7). **This step reuses it; it does not write a new filter.**
- `crates/rollchrome-core/src/media/develop/preview.rs` (the Develop preview: a cached, upright, downscaled
  linear image, re-rendered on every change) and `crates/rollchrome-core/src/tools/develop.rs` (`plan_develop`,
  `execute_develop`: the export).

### Goal

Develop turns a RAW into a finished photograph through a look: a built-in camera look or, since DV-4, a film
stock simulated by spektrafilm. The owner shoots an Olympus XZ-1, a small-sensor camera, often at ISO 400 in
low light. **The sensor's noise survives into the result, and the film looks make it worse.** Each film stock
sets its own exposure and lifts dark photographs, and the film's contrast and edge effects make speckle stand out.
Measured on one of the owner's ISO 400 files in Portra 160 with grain Off, fine noise was 4.7 and colour noise 4.5.
(The figures are the standard deviation of the high-pass residual, in 8-bit levels.) Develop has no noise
reduction; only Edit does, and Edit comes after Develop.

The fix is **noise reduction on the linear RAW, before the look is applied.** The architect checked it on that
file by running Edit's existing `apply_detail`, luminance and colour both set to the same amount, and then
the film. At amount 25, fine noise fell from 4.7 to 2.1 and colour noise from 4.5 to 0.5. At 50 they were
1.6 and 0.4. Brightness was unchanged: the mean was 105.6 before and 105.9 after. It took 0.25 s on the 10 MP
image. The noise has to be removed *before* the look because the film simulation amplifies it and then adds
its own grain on top, and that grain must not be smoothed away.

### Decisions already taken — do not revisit

1. **One setting per photograph: `noise_reduction`, 0 to 100.** It drives `Detail { noise_luminance: n,
   noise_colour: n, ..Default::default() }`. There is one slider, not two, and no sharpening, clarity or hot
   pixels.
2. **It applies to every look:** film stocks, built-in camera looks and LUTs. It always runs on the linear image
   before the look.
3. **At 0 nothing runs,** and the output is **byte-identical** to today's. The serde default is 0, so an item
   without the field, from a script, the CLI or MCP, behaves exactly as it does now.
4. **The screen starts each photograph at 25.** The screen is where the owner looks at the result, and 25 removed
   most of the measured noise. The CLI and MCP keep 0 unless told otherwise, so existing scripts keep their
   output.
5. **The preview passes `scale`.** `apply_detail` takes `scale` as "this buffer's long edge over the source's".
   The preview works on a reduced image, so it must pass `preview long edge / upright source long edge`. The
   export passes `1.0`. Without this the preview filters with export-sized radii and looks blurrier than the
   file.
6. **For film, the order is:** `develop_linear` at exposure 0, then noise reduction, then `render_film` with the
   photograph's exposure. **For a camera look:** `develop_linear` with the exposure, then noise reduction, then
   `linear_to_developed`, then the look. This is what `develop()` does today, with the noise reduction step added
   in between.

### Change

**`crates/rollchrome-core/src/media/develop/mod.rs`**
- Add `pub fn reduce_noise(width: u32, height: u32, pixels: &[[f32; 3]], amount: f32, scale: f32) -> Vec<[f32; 3]>`.
  It flattens into a `LinearBuffer`, calls `apply_detail` with the `Detail` from decision 1, and returns the
  result. **At `amount == 0.0` it returns `pixels.to_vec()` without calling anything.** Write a doc comment
  explaining why the step runs before the look (the Goal's second paragraph, briefly).

**`crates/rollchrome-core/src/tools/develop.rs`**
- `DevelopItem` and `DevelopAction` gain `noise_reduction: f32` with `#[serde(default)]`.
- `plan_develop` refuses a value that is not finite or is outside `0.0..=100.0`, with the same kind of error and
  wording as the exposure check. It copies the value into the action.
- In `execute_develop`:
  - Film branch: call `reduce_noise(linear.width, linear.height, &linear.pixels, action.noise_reduction, 1.0)`
    after `develop_linear` and before both `render_film` calls (JPEG and TIFF).
  - Camera-look branch: replace the `develop(...)` call with `develop_linear` (with the exposure), then
    `reduce_noise(..., 1.0)`, then `linear_to_developed(w, h, &pixels, linear.orientation, linear.demosaic)`.
  - If a LUT path also calls `develop`, treat it the same way.

**`crates/rollchrome-core/src/media/develop/preview.rs`**
- `DevelopPreview` records the upright source's long edge in `from_linear`.
- `render_with_data_dir` (and `render`) take `noise_reduction: f32`. They apply `reduce_noise` to the chosen
  buffer (full or draft) with `scale = buffer long edge / source long edge`, before either branch: film, or
  exposure plus `linear_to_developed`. Validate the value as `plan_develop` does.
- Fix every caller, including tests, by passing `0.0` where the test is not about noise.

**`crates/rollchrome-desktop/src/commands/develop.rs`**
- `preview_develop` and `preview_develop_impl` take `noise_reduction: Option<f32>` and pass `unwrap_or(0.0)`.
  This is transport only (G1): no other logic.

**`crates/rollchrome-cli/src/main.rs`**
- Add `--noise <0..100>` (default 0) to `develop`, in the usage text and the parser. Reject a non-number with
  exit code 2, the same way the existing `--exposure` is rejected.

**`crates/rollchrome-core/src/automation/mcp.rs`** (and `automation/mod.rs` if the item is built there)
- Add an optional `noise_reduction` number argument (0–100, default 0) next to `grain`, in both schemas. A
  non-number is an error that names the argument, as for `grain`.

**Front end, `frontend/rollchrome/`**
- `src/api.ts`: `DevelopItem.noise_reduction?: number`, and `previewDevelop(...)` sends `noise_reduction`.
- `src/developState.ts`: persist `noise_reduction` per photograph, as `grain` is persisted.
- `src/views/Develop.vue`:
  - A **Noise reduction** slider, 0–100 in steps of 5, `data-testid="noise-slider"`, placed under the
    exposure slider. It shows for every look, unlike Grain.
  - Every default assignment `{ look: null, exposure_ev: 0.0, grain: 'fine' }` gains `noise_reduction: 25`.
  - The slider behaves like the exposure slider: input updates the preview, and it shows a mixed state for a
    selection that differs.
  - Changing it invalidates the dry run, the preview request carries it, and the dry run's items carry it.
- `scripts/develop-harness/stub-api.ts`: record `noise_reduction` from preview requests and from items, as it does
  for `grain`.
- `scripts/check-develop.mjs`: add checks 20–22 (below) and update the header comment list.

### Tests

In `crates/rollchrome-core/tests/develop_tests.rs`, unless named otherwise. For the noisy image, use a fixed
seeded generator (a small LCG written in the test). Do not add a dependency.

1. `noise_reduction_at_zero_returns_the_pixels_unchanged`: `reduce_noise(..., 0.0, 1.0)` is bit-identical to its
   input.
2. `noise_reduction_quiets_a_flat_area_and_keeps_its_brightness`: build a 256×256 linear image, grey 0.18, with
   Gaussian-like noise of σ 0.02 added independently per channel. Measure the high-pass residual of luminance in
   sqrt space (pixel minus its 5×5 box mean). After amount 50, the luminance noise is **at most 50%** of before,
   the chroma (B−Y, R−Y) noise **at most 25%**, and the mean of every channel is **within 1%**.
3. `noise_reduction_keeps_an_edge`: a vertical step from 0.05 to 0.5 with the same noise. After amount 50, the
   difference between the column means 8 px either side of the step keeps **at least 90%** of its height before.
4. `the_plan_refuses_noise_outside_0_to_100`: refuses -1, 100.5 and NaN, accepts 0 and 100, and the action carries
   the value.
5. `an_item_without_noise_reduction_means_none`: deserialising a `DevelopItem` JSON with no `noise_reduction`
   gives 0.
6. `a_camera_look_export_at_zero_noise_is_unchanged`: `execute_develop` with a built-in look at
   `noise_reduction: 0` produces **the same bytes** as the same run done with `develop()`, rendered the way the
   code did before this step. Use the RAW fixture the existing execute tests use.
7. `a_camera_look_export_with_noise_reduction_differs`: the same run at 50 produces different bytes.
8. `the_preview_scales_noise_reduction_to_its_size`: `DevelopPreview` gets `pub fn noise_scale(&self, draft: bool)
   -> f32`, which `render_with_data_dir` uses. On a `DevelopPreview::from_linear` of a noisy 1200×800 image with
   `max_edge` 600, `noise_scale(false)` is 0.5 (± 0.01), and `noise_scale(true)` is the draft's long edge over 1200.
   Without a look, `render(..., noise_reduction 50, draft false)` gives different bytes from
   `noise_reduction 0`. To show that `scale` matters at all, `reduce_noise` on the same buffer at scale 0.5 and
   at 1.0 also gives different pixels. (A test that passes whatever `scale` is proves nothing.)

In `crates/rollchrome-core/tests/film_tests.rs`:

9. `film_noise_reduction_happens_before_the_film`: `execute_develop` with a film stock, grain Off and
   `noise_reduction: 50` matches, with mean absolute difference **< 0.01**, the CPU oracle
   `render_film(reduce_noise(develop_linear(raw at 0 EV), 50, 1.0), …, FilmBackend::Cpu)`. It must **not** match
   the oracle without noise reduction (mean difference > 0.001), so the order is proved, not assumed.
10. `#[ignore]` timing, `noise_reduction_preview_cost_is_printed`: on a 1600×1067 noisy linear image in release
    mode, print the time of `reduce_noise(..., 50, scale)` (median of 5) and assert **≤ 100 ms**.

In `crates/rollchrome-desktop/tests/develop_desktop.rs`:

11. `preview_develop_passes_noise_reduction`: `preview_develop_impl` with `Some(50.0)` returns bytes different from
    `None` on the same photograph.

In `check-develop.mjs`:

20. A new photograph's Noise reduction slider shows 25, and it is visible for a camera look and for a film look.
21. Changing it disables Run until a new dry run, and that dry run's items carry the new `noise_reduction`.
22. Changing it sends a new preview request carrying `noise_reduction`.

### Do not touch

- `media/edit/detail.rs`: reuse it as it is. If it needs changing, stop and say why in the Report.
- `media/film.rs` and the film data. Grain stays exactly as DV-4 left it.
- `resize_linear` in `preview.rs`. DV-4's review restored it; do not rewrite it.
- Edit, Batch, and every other check script. `SPECIFICATION.md` (G9). `CLAUDE.md` and the plan documents: the
  architect updates them at review.
- No new dependency, crate feature or file outside those listed.

### Acceptance

- [ ] Tests 1–11 exist under these names and pass; test 10 prints its time.
- [ ] `check:develop` passes **22** checks (19 before).
- [ ] Every existing test still passes **unmodified**, apart from adding `0.0` / `noise_reduction: 0` / `None` to
      calls and literals that need the new field. No assertion is changed.
- [ ] Test counts: rollchrome-core 442 → **≥ 450**; rollchrome-core with `--features film` 453 (1 ignored) →
      **≥ 462 (2 ignored)**; rollchrome-desktop 27 → **≥ 28**. Report the exact numbers.

### Gates

The gate list in `CLAUDE.md` in full, plus:

```sh
cargo test -p rollchrome-core --features film
cargo test -p rollchrome-core --features film --release --test film_tests -- --ignored   # prints test 10's time
```

### How to work: every step costs money

1. **Run every command in the foreground and wait for it to return.** Never start one in the background and check
   on it, and never loop or sleep while waiting.
2. **While working, run only what covers the change:** `cargo check -p rollchrome-core --features film`,
   `cargo test -p rollchrome-core --test develop_tests`, `cargo test -p rollchrome-core --features film --test
   film_tests`, `npm --prefix frontend/rollchrome run check:develop`.
3. **Run the full list under Gates once, at the end.** If a gate fails, fix it and re-run that gate, not the list.
4. **Read each file once,** using a line range or `grep` for large files such as `Develop.vue` and `develop.rs`.
   Do not re-read a file to confirm an edit.
5. **Do not change approach silently.** If the same failure survives two attempts, stop, write what you tried in the
   Report, and commit what works.

### Report back

Under `## Report` in this file, give:
- What was done.
- **Every** deviation from this brief, with the reason. Write "None" only if that is true. Rewriting code the
  brief did not name counts as a deviation.
- Test 10's printed time.
- Every gate's actual output: counts, not "passed".
- Anything you could not verify.

Commit on `dv-5`. Do not merge.

---

## Report

*Written by the implementer.*

**Not written.** agy (Gemini 3.8 Flash) implemented the whole brief, then ran out of credits during its first
full clippy run, before writing this section, and committed nothing. The architect committed its work as it stood
(`f3bfec0`). Five compile and lint errors remained. The owner's local model (Qwen3 Coder 30B, through opencode) fixed
three of them before LM Studio ran out of GPU memory. The architect fixed the other two (`e58cc5c`). The review
below is therefore the only account of the gates.

---

## Review

*Written by the architect, after re-running the gates on the branch.*

### Review 1: approved

**Checked.** Every gate re-run on `e58cc5c`:

| Gate | Result |
|---|---|
| fmt, clippy, build | clean |
| `--workspace` | 1309 passed, 4 ignored |
| `phototools-core` | 714 passed |
| `rollchrome-core` | 450 passed (442 before) |
| `rollchrome-core --features film` | 462 passed, 2 ignored (453 and 1 before) |
| `phototools-desktop` | 26 |
| `rollchrome-desktop` | 28 (27 before) |
| `rollchrome-cli` | 8 |
| Rollchrome front-end checks | all pass; `check:develop` 22 (19 before) |
| Web and desktop typecheck, build, transport | pass |

Test 10 printed **50.6 ms** against its 100 ms budget. Test 12 printed a 65.9 ms GPU preview, and the GPU and
CPU renders still differ by a mean of 0.000274.

**`phototools-core` in the first gate run:** 3 failures, 711 passed. DV-5 changes nothing in that crate. The
same tests passed inside the workspace run, and a re-run passed 714 of 714. The log did not keep the test names,
so this is recorded as a flaky test under load, not identified.

**Diff read in full.**
- `reduce_noise` calls `apply_detail` unchanged, with luminance and colour at the same amount. At 0 it returns
  the input untouched.
- **Export.** The camera-look branch is now `develop_linear`, then `reduce_noise`, then `linear_to_developed`,
  which is what `develop()` did, with the noise reduction step added. The film branch removes noise after
  `develop_linear` at 0 EV and before both `render_film` calls.
- **Preview.** It records the upright source's long edge, passes `noise_scale`, and validates the range as the
  plan does.
- **Transports.** The desktop command, CLI `--noise` and MCP `noise_reduction` only pass the value through (G1).
  The two `#[allow(clippy::too_many_arguments)]` carry their reason.
- **Front end.** The slider defaults to 25 for every look, uses the exposure slider's mixed-state rule, and the
  preview request and the dry run's items carry the value.
- **Scope.** No other code is touched, and `detail.rs`, `film.rs` and `resize_linear` are unchanged. The existing
  tests only gained the new field, and one plan test gained an assertion on it.

**Tests checked for what they prove.**
- Test 6 compares bytes with the code path from before this step.
- Test 7 uses a noisy RAW, so "differs" means something.
- Test 9 matches the CPU oracle with noise reduction before the film (mean difference < 0.01) **and** differs
  from the oracle without it (> 0.001), so the order is proved.
- Test 8 shows that `scale` changes the result.

**On a real file.** Before briefing, the architect measured the same filter on the owner's ISO 400 XZ-1 file in
Portra 160 with grain Off. At 25, fine noise fell from 4.7 to 2.1 and colour noise from 4.5 to 0.5, and
brightness was unchanged. MV-39 checks it in the application.

**Approved.**
