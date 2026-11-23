# `UI-3` · Every look, and RAW files, in Edit

> **Status:** briefed
> **Plan:** [`docs/rollchrome/app-layout.md`](../rollchrome/app-layout.md), step UI-3. **Branch:** `ui-3`, cut from
> `main` after UI-7 is merged.

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md`: **Ground rules** (G1 logic in core, G2, G7, G10, G11), **Gates**, **The design language**.
- `docs/rollchrome/ui-review.md`, *"What Edit lacks that the owner asked for"*, and the owner's decisions.
- In `crates/rollchrome-core`:
  - `media/edit/pipeline.rs`: `LutRef`, `decode_image` (about line 1150) and its RAW refusal;
  - `tools/edit.rs`: the export's RAW refusal, about line 878;
  - `automation/mod.rs`: the MCP export's RAW refusal, about line 398;
  - `tools/lut_library.rs`: how a library LUT is stored and resolved;
  - `media/looks/builtin.rs`;
  - `media/film.rs`: `film_lut` and `FILM_STOCKS`;
  - `media/develop/render.rs`: `Look` and `LookSpace`, and how a `ProPhoto` look is applied;
  - `media/develop/mod.rs`: `develop_neutral`, `prophoto_to_srgb8`;
  - `tools/develop.rs`: `develop_one`, to see the order Develop applies lens correction, geometry, noise
    reduction and sharpening.
- In `crates/rollchrome-desktop/src/commands/develop.rs`: `list_builtin_looks`, `list_film_looks` and
  `save_film_lut_impl`.
- `frontend/rollchrome/src/components/LutPicker.vue` and the Look section (`src/views/edit/LookSection.vue`
  after UI-4).

### Why

The owner on 2026-11-23: *"in the edit window, I want to have all film styles that are in the develop one
again available if I want to play in detail a bit more with an image, and also why I cannot open a raw file
there? If I ask for that before, we need to offer that option, my mistake."*

- Edit's look picker lists only the **LUT library**. Develop also offers **12 built-in looks** (camera
  looks, `builtin:<id>`) and **18 film simulations** (`film:<stock>`).
- **RAW files are refused** in Edit. RP-2b did that, at the owner's earlier request, which they now reverse.

**The owner's decision:** in Edit a film simulation applies **as its 3D LUT**, as DV-6 already saves it:
the film's colour and tone, without grain or halation. That is enough.

### What to build

**1. Every look as a library LUT, made on demand** (`crates/rollchrome-core`, new
`tools/look_luts.rs`).

Edit already applies a library LUT by `LutRef { name, sha256 }` in **display-encoded sRGB**
(`pipeline.rs`, "the 3D LUT is evaluated in display-encoded space"). Rather than teach the pipeline new
kinds of look, each built-in look and film stock is **written into the LUT library once**, as an sRGB-domain
`.cube`, and from then on it is an ordinary library LUT. Recipes, sidecars, Batch and export need no
change.

- `pub fn ensure_look_lut(look_id: &str, library_dir: &Path, film_data_dir: &Path) -> Result<LutRef, Error>`:
  - `look_id` is `builtin:<id>` or `film:<stock>`, the same ids Develop uses.
  - **The file name** is fixed per look: `Builtin - <Maker> <Name>.cube` for a camera look, and the name
    `FilmLut::file_name` gives for a film stock. Film stocks use the **as-film** balance
    (`LutBalance::AsFilm`), or neutral grey if the stock is tungsten-balanced, as Develop's "Save as LUT"
    defaults.
  - If a file of that name already exists in the library, parse it and return its `LutRef`; do not
    regenerate. Otherwise generate it, write it atomically (a temporary file, then rename), and return its
    `LutRef`, computing the SHA-256 the way the library does.
  - **A film stock** is `film_lut(stock, balance, film_data_dir)`, whose cube is already sRGB-domain. Under
    `--features film` only; without the feature, `film:` ids return `Error::Refused`, as `resolve_look`
    does.
  - **A built-in look** is a `LookSpace::ProPhoto` LUT, which Edit cannot use directly. Bake an **sRGB-domain
    cube, 33³**: for each grid point, an sRGB code value → exactly the transform `render.rs` applies around
    a ProPhoto look's LUT (decode, convert to the space and encoding the LUT expects, sample the LUT, and
    convert its output back) → sRGB code value. Reuse `render.rs`'s and `colour.rs`'s functions; do not
    restate the matrices or curves.
- Tests, in `look_luts.rs`:
  - `a_builtin_look_becomes_an_srgb_library_lut`: the file appears, it parses, and a second call returns
    the same `LutRef` without rewriting. Check the file's modification time, or count writes through an
    injectable writer.
  - `a_baked_builtin_look_matches_develop_on_the_grid`:
    - for 64 sRGB colours, the baked cube's output equals applying the original look the way `render.rs`
      does, within **1/255** per channel;
    - grey 0.5 stays within the look's own result; it is not forced to grey.
  - `a_film_look_becomes_a_library_lut` (`#[cfg(feature = "film")]`): uses the test film data the film
    tests already use.
  - `an_unknown_look_is_refused`.

**2. The desktop command and Edit's picker.**
- `rollchrome-desktop`: a command `ensure_look_lut(look_id: String) -> Result<LutRefDto, String>` that calls
  the core function with the configured library and film data directories, on `spawn_blocking`, and returns
  `{ name, sha256 }`. Register it. Add `ensureLookLut` to `src/api.ts`, and to the Edit harness's stand-in
  `stub-api.ts`, which returns a `LutRef` and records the call for checks.
- **`LutPicker.vue`:**
  - The list shows three groups, with group headings: **Camera looks** (from `list_builtin_looks`),
    **Film simulations** (from `list_film_looks`), and **Your LUTs** (the library, as today).
  - A camera look or film stock already in the library (by its file name) appears **only** in its group,
    not again under Your LUTs.
  - Choosing a camera look or film stock calls `ensureLookLut(id)` and then emits the same
    `update:modelValue` as choosing a library LUT. Show "Preparing…" while it runs: a film LUT takes a few
    seconds to print.
  - On failure, show the error in the picker's existing error area. The recipe stays as it was.
- The Look section's intensity slider and everything after the choice work as they do for any LUT.
- **Batch's** LUT list (`BulkLut.vue`) shows the same groups through the same component. That is free, since
  it uses `LutPicker`; confirm it in the Report.

**3. RAW files open in Edit.**
- `crates/rollchrome-core/src/tools/develop.rs`:
  - `pub const DEFAULT_NOISE_REDUCTION: f32 = 25.0;`, `pub const DEFAULT_CAPTURE_SHARPENING: f32 = 50.0;`
    and `pub const DEFAULT_LENS_CORRECTION: bool = true;`, each with a comment saying they are the values
    Develop gives a new photograph (`developState.ts`).
  - `pub fn develop_for_edit(raw: &[u8]) -> Result<image::ImageBuffer<image::Rgb<u16>, Vec<u16>>, Error>`:
    the neutral development exactly as Develop exports it with those defaults (the same steps and order as
    `develop_one` for `ResolvedLook::Neutral`), in sensor orientation, converted to **16-bit sRGB**.
  - Add `prophoto_to_srgb16` beside `prophoto_to_srgb8` in `media/develop/mod.rs`, sharing its code with a
    generic or a common helper rather than a copy.
- `media/edit/pipeline.rs`, `decode_image`: a RAW path (`is_raw`) is read and passed to `develop_for_edit`,
  giving `ImageBuffer::Rgb16`. Remove the refusal and update the doc comment.
- `tools/edit.rs` and `automation/mod.rs`: remove the two refusals, so export and MCP export work on RAW.
  Export decodes through `decode_image`, so a RAW is developed again at full size.
- **Orientation:** a RAW's EXIF orientation reaches Edit through `read_meta`, as a JPEG's does. DV-15 made
  that work for RW2.
- **The sidecar** stays `<file>.photoedit` beside the RAW (`NAME.RW2.photoedit`). Its `source_sha256` is the
  RAW file's, as for any source.
- **Edit's path field** accepts RAW extensions: its `selectable` list adds `media::raw::RAW_EXTENSIONS`.
  Batch's input selection is not widened here; Batch stays JPEG and TIFF.
- Tests:
  - `a_raw_file_decodes_for_edit_as_sixteen_bit_srgb`: a synthetic DNG of the kind the develop tests
    already build gives an `Rgb16` buffer of the developed size.
  - `edit_export_accepts_a_raw_source`: replaces the existing test that asserted the refusal.

  **These are the only existing tests that may change:** the ones asserting that RAW is refused, since this
  step is the owner reversing that decision. **Name each in the Report, with its old and new assertion.**
  Changing any other test is G7.

### Do not touch

- The edit pipeline's maths, `LutRef`'s shape, the sidecar format, Batch's file selection, Develop's screen
  and its look ids.
- The film simulation's grain and halation in Edit: not wanted (the owner's decision).
- `docs/` other than this file's Report.

### Acceptance

- [ ] Edit's look picker offers 12 camera looks, 18 film simulations and the library. Choosing one renders
      it and saves an ordinary `LutRef` in the sidecar.
- [ ] A baked camera look matches Develop's application of it within 1/255 on a 64-colour grid.
- [ ] A RAW file (`.RW2`, `.ORF`, `.RAF`, `.NEF`, `.DNG`) opens in Edit, upright, edits, saves its sidecar,
      and exports at full size.
- [ ] Rust tests: the new ones listed, plus the refusal tests changed as named. All gates pass.

### Gates

The gate list in `CLAUDE.md` in full, plus `check:edit-layout` and `check:strip`.

### How to work: every step costs money

1. **Run every command in the foreground and wait for it to return.** Never start a command in the
   background and check on it, and never loop or sleep waiting for one.
2. **While working, run only** `cargo test -p rollchrome-core --lib look_luts`,
   `cargo test -p rollchrome-core --features film --lib look_luts`, `cargo test -p rollchrome-core --lib edit`,
   `npm --prefix frontend/rollchrome run typecheck` and `check:edit`.
3. **Run the full list under Gates once, at the end.**
4. **Read files in ranges.** Do not re-read a range to confirm an edit.
5. **Do not change approach silently.** If the same failure survives two attempts, stop, say what you tried
   in the Report, and commit what works.

### Report back

Under `## Report` in this file:
- what was done;
- **every** deviation, with the reason ("None" only if true);
- each changed existing test, with its old and new assertion;
- the time a film LUT takes to prepare;
- every gate's actual output: counts, not "passed";
- anything unverified.

Commit on `ui-3` with imperative subjects under 72 characters and no model name anywhere. Do not merge.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
