# `DV-6` · Save a film stock as a LUT

> **Status:** briefed
> **Plan:** [`docs/develop-plan.md`](../develop-plan.md), step `DV-6` · **Branch:** `dv-6`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md`: **Ground rules** (G1–G11) and **Gates**.
- `crates/rollchrome-core/src/media/film.rs`: `FILM_STOCKS`, `ensure_film_data`, and the pipeline builder around
  line 420–480 (`get_or_build_pipeline`). This step builds a **different** pipeline next to it and leaves that one
  alone.
- `crates/rollchrome-desktop/src/commands/develop.rs`: `resolve_input`, the `Developed` folder (line ~195), and
  `list_film_looks`.

### Goal

The owner edits in Photomator as well as in Rollchrome. They asked for the film stocks as `.cube` LUTs to load
there. The architect made Portra 400 and Vision3 500T by hand, and the owner confirmed they look right in
Photomator. This step turns that into a button in Develop: **Save as LUT** for the selected film stock, written
next to the photographs Develop exports.

A LUT maps one colour to one colour. It **cannot** hold grain, halation, the film's edge effects (DIR couplers)
or auto exposure, so those are off when it is made. Three things are easy to get wrong, and the architect hit
all three by hand:

1. **The input is gamma-encoded sRGB**, which is what photo editors feed a LUT. It is not the linear ProPhoto RGB
   that Rollchrome's renderer uses. Decode each grid value with the sRGB curve to linear sRGB, and tell the engine
   the input is `"sRGB"` with `input_cctf_decoding = false`.
2. **The pipeline must be built with `Pipeline::new_with_spectral(film, print, params, &data_path)`**, exactly as
   `film.rs` does today. The engine's own `export-lut` uses `Pipeline::new`, which skips the print-exposure
   calibration. Its LUTs come out about two stops dark and green: mid grey 0.5 → (0.085, 0.138, 0.124) instead of
   (0.49, 0.50, 0.52).
3. **Each grid colour is a separate 1×1 image.** The scanner's unsharp mask and the glare depend on neighbouring
   pixels. If the grid were rendered as one picture, they would blend neighbouring LUT entries together.

**Tungsten film needs a choice.** Vision3 500T and 200T are balanced for tungsten light, so a daylight photograph
comes out strongly blue: mid grey → (0.22, 0.45, 0.59). A lab corrects this at the enlarger with yellow and
magenta filters. The architect found that `enlarger.y_filter_shift = -38.2` and `m_filter_shift = -13.2` print
18% grey neutral for 500T. This step **finds the shifts automatically**, for any stock, when the user asks for a
neutral grey.

### Decisions already taken — do not revisit

1. **Size 33** (`LUT_3D_SIZE 33`, 35,937 entries). It is the size Photomator was tested with.
2. **Balance:** `LutBalance::AsFilm` (the film's own colour) or `LutBalance::NeutralGrey` (enlarger shifts
   found so that 18% grey prints neutral). The screen's checkbox, **"Neutral grey (daylight)"**, is ticked by
   default for tungsten stocks and unticked for the rest. Add `tungsten: bool` to `FilmStockInfo`: true for
   `kodak_vision3_200t` and `kodak_vision3_500t`, false for every other stock.
3. **Slides:** `scan_film = stock.is_slide`, as `film.rs` already does. `NeutralGrey` works the same way, through
   the enlarger shifts. If the engine ignores the enlarger when scanning, the search will not converge; return a
   refusal in that case (see *Change*), never a silently wrong LUT.
4. **Where it goes:** `<folder>/Developed/<Name> (spektrafilm).cube`. For example, `Kodak Portra 400
   (spektrafilm).cube`, and `Kodak Vision3 500T neutral (spektrafilm).cube` for the neutral version. Use the
   stock's `name` from `FILM_STOCKS`. An existing file of the same name is overwritten: the content is
   deterministic.
5. **The file's header** names spektrafilm and its licences, because the data is CC BY-SA 4.0 and its credit must
   travel with the file:
   ```
   # Created by Rollchrome with spektrafilm (GPL-3.0); film data CC BY-SA 4.0, spektrafilm by Andrea Volpato
   # Input: sRGB (gamma-encoded). Output: sRGB. <film id> printed on <print id> (or: scanned).
   # No grain, halation, DIR couplers or auto exposure. Enlarger Y/M shift <y> <m>.
   TITLE "<Name> (spektrafilm)"
   LUT_3D_SIZE 33
   DOMAIN_MIN 0.0 0.0 0.0
   DOMAIN_MAX 1.0 1.0 1.0
   ```
   Then 35,937 lines of `r g b` with 6 decimals, each clamped to 0..1. **Red varies fastest, then green, then
   blue**, which is the `.cube` order.
6. **The CPU backend** (`FilmBackend::Cpu`), so the file is deterministic. The architect measured 0.8 s for a
   33-point LUT on the CPU with rayon.

### Change

**`crates/rollchrome-core/src/media/film.rs`** (behind the `film` feature, with a refusing stub without it, as
`render_film` has):
- `pub enum LutBalance { AsFilm, NeutralGrey }` (serde, lowercase).
- `pub struct FilmLut { pub file_name: String, pub cube: String, pub y_shift: f32, pub m_shift: f32 }`.
- `pub fn film_lut(stock: &str, balance: LutBalance, data_dir: &Path) -> Result<FilmLut, Error>`. Build the
  params as in the Goal:
  - grain, halation and DIR couplers off;
  - `camera.auto_exposure = false`;
  - input `"sRGB"` without decoding, output `"sRGB"` with encoding;
  - `scan_film` as the stock says;
  - `Pipeline::new_with_spectral`.

  Sample the grid in parallel with rayon, one 1×1 image per entry, decoding sRGB before the engine. Write the
  text described in Decision 5. **Do not** use or change `PIPELINE_CACHE` or `get_or_build_pipeline`: a LUT is
  made rarely, and its parameters differ.
- **Neutral search,** private: render one 1×1 sample of sRGB 118/255 (18% grey) and measure `d = (r − g, b − g)`.
  Solve for `(y, m)` with Newton's method: estimate the 2×2 Jacobian by central differences with a step of 2.0,
  starting from (0, 0). Stop when both components are under 0.002, or after 12 iterations. If it has not
  converged by then, return `Error::Refused("could not find a neutral print filter for <stock>")`. Each evaluation
  builds a new pipeline, because the enlarger shifts live in the params.

**`crates/rollchrome-desktop/src/commands/develop.rs`**
- `#[tauri::command] save_film_lut(folder: String, stock: String, balance: LutBalance) -> CommandResult<String>`:
  - resolves `folder` with `resolve_input`, as the other develop commands do (G6);
  - creates `Developed` if missing;
  - calls `film_lut` with `config::film_data_dir`;
  - writes the file and returns its full path.

  Run the work on `spawn_blocking`, as `preview_develop` does. This is transport only (G1). Register it in
  `main.rs`.
- `list_film_looks` also returns `tungsten` for each stock, if it does not already.

**Front end, `frontend/rollchrome/`**
- `src/api.ts`: `saveFilmLut(folder, stock, balance): Promise<string>` and the `tungsten` field.
- `src/views/Develop.vue`: when the selected photograph's look is a film stock, and only then, show a
  **"Save as LUT"** button (`data-testid="save-lut"`) and a **"Neutral grey (daylight)"** checkbox
  (`data-testid="lut-neutral"`) next to the Grain select.
  - The checkbox starts ticked for tungsten stocks.
  - While saving, the button is disabled and says "Saving LUT…".
  - On success, show the returned path under it (`data-testid="lut-path"`). On error, show the message.
  - The LUT has nothing to do with the dry run, so it does not invalidate it.
- `scripts/develop-harness/stub-api.ts`: record `saveFilmLut` calls and return a fixed path.
- `scripts/check-develop.mjs`: add checks 23–25 (below) and update the header list.

### Tests, `crates/rollchrome-core/tests/film_tests.rs`

Parse the cube in the test. Grid index `i` on the grey axis is entry `i + 33·i + 33²·i`.

1. `a_film_lut_has_the_cube_layout`: the header lines of Decision 5, `LUT_3D_SIZE 33`, exactly 35,937 data lines,
   every value in 0..1, the TITLE naming the stock, and the file name as in Decision 4.
2. `a_film_lut_is_calibrated_like_the_renderer`: Portra 400, `AsFilm`. Grey index 16 (input 0.5) is within
   **±0.03** of (0.493, 0.504, 0.515), and the grey axis rises monotonically. **This is the test that
   fails if `Pipeline::new` is used.** Check that with a temporary change and say in the Report that you did.
3. `a_film_lut_follows_srgb_input`: Portra 400, `AsFilm`. Grey index 8 (sRGB 0.25) has a mean between 0.12 and
   0.20. With linear input instead of sRGB, it would be about 0.05.
4. `tungsten_film_as_film_is_blue_in_daylight`: 500T `AsFilm`. At grey index 16, blue minus red > 0.25.
5. `tungsten_film_neutral_prints_grey_neutral`: 500T `NeutralGrey`. At grey index 16, max − min of (r, g, b)
   ≤ **0.01**. The returned shifts are within ±3 of (−38.2, −13.2), and the header names them.
6. `a_daylight_film_neutral_needs_only_a_small_shift`: Portra 400 `NeutralGrey` converges, with |y| and |m|
   both < 15.
7. `the_same_lut_twice_is_identical`: two calls give byte-identical `cube` strings.
8. `#[ignore]` timing, `film_lut_time_is_printed`, release: print the times of Portra `AsFilm` and 500T
   `NeutralGrey`. Assert ≤ **5 s** and ≤ **20 s** respectively.
9. `tungsten_stocks_are_flagged`: exactly 200T and 500T have `tungsten == true`.

`crates/rollchrome-desktop/tests/develop_desktop.rs`:

10. `save_film_lut_writes_into_developed`: with the test `Config` fixture and a temporary folder inside its root,
    `save_film_lut_impl` writes `Developed/Kodak Portra 400 (spektrafilm).cube` and returns that path. A folder
    outside the roots is refused.

`check-develop.mjs`:

23. "Save as LUT" and the checkbox are hidden for a camera look and shown for a film look. The checkbox is
    ticked for 500T and unticked for Portra 400.
24. Clicking it calls `saveFilmLut` with the folder, the stock id and `neutralgrey` or `asfilm` as ticked, then
    shows the returned path.
25. Saving a LUT leaves Run enabled when a dry run was already done.

### Do not touch

- `render_film`, `get_or_build_pipeline`, `PIPELINE_CACHE`, `resize_linear`, `reduce_noise`, and the preview and
  export paths. The LUT is a separate function.
- The film data files and `Cargo.toml` dependencies: nothing new.
- Batch, Edit, the LUT library, and every other check script. `SPECIFICATION.md` (G9). `CLAUDE.md` and the plan
  documents: the architect updates them at review.

### Acceptance

- [ ] Tests 1–10 under these names pass, and test 8 prints its times.
- [ ] `check:develop` passes **25** checks (22 before).
- [ ] Every existing test passes unmodified, except for adding `tungsten` to literals if the compiler requires it.
- [ ] Counts: `rollchrome-core --features film` 462 passed, 2 ignored → **≥ 470 passed, 3 ignored**.
      `rollchrome-core` stays at **450** (the new code is behind `film`, plus the stub). `rollchrome-desktop` 28 →
      **≥ 29**. Report the exact numbers.
- [ ] The Report includes the temporary `Pipeline::new` check from test 2.

### Gates

The full list in `CLAUDE.md`, plus:

```sh
cargo test -p rollchrome-core --features film
cargo test -p rollchrome-core --features film --release --test film_tests -- --ignored   # prints test 8's times
```

### How to work: every step costs money

1. **Foreground commands only.** Wait for each one. Never background a command, poll it, or sleep.
2. **While working, run only** `cargo check -p rollchrome-core --features film`,
   `cargo test -p rollchrome-core --features film --test film_tests`, `cargo test -p rollchrome-desktop` and
   `npm --prefix frontend/rollchrome run check:develop`.
3. **Run the full Gates list once, at the end.** If one gate fails, fix it and re-run only that gate.
4. **Read each file once,** with line ranges or `grep` for big files (`Develop.vue`, `film.rs`, `develop.rs`).
5. **Do not change approach silently.** If the same failure survives two attempts, stop, write it in the Report, and
   commit what works.

### Report back

Under `## Report`, give:
- what was done;
- **every** deviation, with its reason ("None" only if true);
- test 8's times;
- the `Pipeline::new` check;
- every gate's actual output, counts not "passed";
- anything you could not verify.

Commit on `dv-6`. Do not merge.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
