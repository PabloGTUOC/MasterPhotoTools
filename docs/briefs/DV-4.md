# `DV-4` · Film stocks in Develop, simulated by spektrafilm

> **Status:** briefed
> **Plan:** [`docs/develop-plan.md`](../develop-plan.md), step `DV-4` · **Branch:** `dv-4`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11) and **Gates**.
- [`docs/develop-plan.md`](../develop-plan.md) — *The pipeline, per photograph* and the `DV-4` section.
- `crates/rollchrome-core/src/tools/develop.rs` — `DevelopItem`, `plan_develop`, `resolve_look`, `execute_develop`.
- `crates/rollchrome-core/src/media/develop/` — `mod.rs` (`develop_linear`, `LinearProPhoto`, `linear_to_developed`),
  `render.rs` (`render`, `encode_tiff`, `prophoto_icc_profile`), `preview.rs` (`DevelopPreview::render`), `tone.rs` (`prophoto_encode`).
- `crates/rollchrome-core/src/automation/mod.rs` — `list_looks` and the plan hash (lines ~200–225).
- `crates/rollchrome-desktop/src/commands/develop.rs`, `frontend/rollchrome/src/views/Develop.vue`,
  `frontend/rollchrome/src/api.ts`, `frontend/rollchrome/scripts/check-develop.mjs` and its `develop-harness/`.
- The engine, at the pinned commit below: `crates/spektrafilm-cli/src/main.rs` (`run_process`, lines ~140–260: how it
  loads profiles, chooses the print, builds `RuntimeParams` and calls `Pipeline`), `crates/spektrafilm-core/src/params.rs`,
  `crates/spektrafilm-core/src/pipeline.rs` (`new_with_spectral`, `process`), `crates/spektrafilm-gpu/src/lib.rs`.

### Goal

The owner shoots film stocks, not camera settings: Portra 160, Ektar 100, Ektachrome 100, Vision3 250D. Develop's
twelve looks imitate Fujifilm's and Ricoh's *camera* simulations, because camera JPEGs of a chart were the only
reference we had. **[spektrafilm-rs](https://github.com/turbasvin/spektrafilm-rs)** (a Rust port of Andrea Volpato's
spektrafilm) simulates the film itself from the manufacturers' datasheets: light through the negative, the enlarger,
the paper and a scanner, with grain and halation. The architect ran it on the owner's XZ-1 RAWs (2026-11-09); the
owner liked the result and accepted GPL-3 for the repository so the engine can be built in. This step makes
**18 film stocks** choosable in Develop beside the existing looks, with a grain control, for preview and export.

Measured on the owner's Mac (Apple M1 Max), engine only, `precision-f64` build:

| Image | GPU (wgpu), warm | CPU, warm |
|---|---|---|
| 800×600 (draft preview) | 21 ms | 240 ms |
| 1600×1200 (preview) | 50 ms | 1.0 s |
| 3648×2736 (XZ-1) | 0.3 s | 5.5 s |
| 7728×5152 (40 MP, X100VI) | 1.5 s | 26 s |

So the **GPU is used for preview and export**, falling back to the CPU when no GPU adapter exists. With grain off,
GPU and CPU differ by a mean of 1.2/255 (99th percentile 5/255); grain differs per pixel by design. The CPU path in an
f64 build is bit-identical run to run, which is what the tests use.

### Decisions already taken — do not revisit

1. **Licence.** The repository becomes **GPL-3.0-or-later** (owner, 2026-11-09/10). spektrafilm-rs is GPL-3.0; its
   profiles and any LUT made from them are CC BY-SA 4.0 and must be credited.
2. **The engine is used as it is.** No fork, no patch, no copy of its source into this repository. If it needs a
   change to work here, stop and write it in the Report.
3. **Pinned:** git `https://github.com/turbasvin/spektrafilm-rs`, rev `9dd59b0380194b93686aaa230a8bb9680aa270a4`.
4. **Exposure.** The engine's auto exposure stays on (`camera.auto_exposure = true`, its default
   `center_weighted`). Develop's existing exposure slider becomes `camera.exposure_compensation_ev`. The linear image
   given to the engine is the development at **0 EV** — scaling the input would be undone by auto exposure.
5. **Every other engine parameter keeps its default.** No tuning of halation, couplers, enlarger, scanner.

### Change

**1. Dependencies — `crates/rollchrome-core/Cargo.toml`.** A new optional feature `film`, beside `segment`:

```toml
spektrafilm-core = { git = "https://github.com/turbasvin/spektrafilm-rs", rev = "9dd59b0380194b93686aaa230a8bb9680aa270a4", optional = true, features = ["precision-f64"] }
spektrafilm-gpu  = { same git and rev, optional = true, features = ["precision-f64"] }
spektrafilm-math = { same git and rev, optional = true, features = ["precision-f64"] }
[features]
film = ["dep:spektrafilm-core", "dep:spektrafilm-gpu", "dep:spektrafilm-math"]
```

On Linux the engine links OpenBLAS: add `[target.'cfg(target_os = "linux")'.dependencies] openblas-src =
{ version = "0.10", optional = true, features = ["system"] }` and put `"dep:openblas-src"` in `film`, so CI uses the
system library instead of building one. `rollchrome-desktop` and `rollchrome-cli` enable `film`. Add a
`[[test]] name = "film_tests"` with `required-features = ["film"]`.

**2. The film data — `crates/rollchrome-core/film-data/`.** Copied unmodified from the pinned commit's `data/`,
keeping its layout:

- `profiles/`: the 18 stocks below and the three papers they print on — `kodak_portra_endura`,
  `fujifilm_crystal_archive_typeii`, `kodak_2383`.
- `luts/spectral_upsampling/irradiance_xy_tc.npy`, `filters/neutral_print_filters.json`.
- Anything else the pipeline opens for these stocks. Find out by rendering every stock with only the files above;
  add what fails to load and list each addition, with its size, in the Report. Do not copy the whole `data/`.
- `SPEKTRAFILM_LICENSE.txt`, unmodified, and `CREDITS.md`: spektrafilm by Andrea Volpato, spektrafilm-rs by
  turbasvin, the commit, the CC BY-SA 4.0 licence of the profiles, and the statement that the film names are
  trademarks of their owners, used only to say which stock is simulated.

The engine reads its data from a directory. Embed the files with `include_bytes!` and materialise them on first use
into `<dir>/film-data-<first 12 hex of a SHA-256 over every embedded file's name and bytes>/`: write into a temporary
sibling, then rename, so a crash never leaves half a directory. Reuse the directory when every file is present at
its expected size; rewrite a file that is missing or the wrong size. `<dir>` is a new `config::film_data_dir(&Config)`
next to `models_dir`; tests pass a temporary directory.

**3. The stocks.** Ids are `film:<profile name>`; names are the profile's `info.name`.

| Maker | Stocks |
|---|---|
| Kodak (13) | `kodak_portra_160`, `kodak_portra_400`, `kodak_portra_800`, `kodak_ektar_100`, `kodak_gold_200`, `kodak_ultramax_400`, `kodak_ektachrome_100`, `kodak_kodachrome_64`, `kodak_vision3_50d`, `kodak_vision3_200t`, `kodak_vision3_250d`, `kodak_vision3_500t`, `kodak_verita_200d` |
| Fujifilm (5) | `fujifilm_c200`, `fujifilm_pro_400h`, `fujifilm_xtra_400`, `fujifilm_provia_100f`, `fujifilm_velvia_100` |

Negatives print on their profile's `target_print`. Slides (`info.type == "positive"`: Ektachrome, Kodachrome,
Provia, Velvia) are scanned directly — `io.scan_film = true` with the film as its own print, exactly as the CLI
does with `--scan-film`.

**4. `crates/rollchrome-core/src/media/film.rs`, behind `#[cfg(feature = "film")]`.**

- `FILM_STOCKS` (the table above, with maker and whether it is a slide), `film_look_list() -> Vec<BuiltinLookInfo>`
  (`id`, `name`, `maker`), in table order.
- `#[serde(rename_all = "snake_case")] pub enum Grain { Off, Fine, Standard }`, default `Fine`. `Off` sets
  `film_render.grain.active = false`; `Fine` sets `camera.film_format_mm = 60.0` (medium format); `Standard` 35.0
  (the engine's default, which the owner found strong at 100 % on a 10 MP file).
- `pub enum FilmBackend { Auto, Cpu }`. `Auto` is wgpu when an adapter exists, else the CPU. Choose **once per
  process** and keep it in a `OnceLock<Mutex<…>>`: preview and export share it and run one at a time. Tests use `Cpu`.
- `pub fn render_film(linear: &[[f32; 3]], width, height, stock, grain, exposure_ev, output: FilmOutput, backend,
  data_dir) -> Result<Vec<[f32; 3]>, Error>`, where `FilmOutput::Srgb` gives display sRGB with its transfer
  function (`io.output_color_space = "sRGB"`, `output_cctf_encoding = true`) and `FilmOutput::ProPhotoLinear`
  gives linear ProPhoto (`"ProPhoto RGB"`, `false`). Input: `io.input_color_space = "ProPhoto RGB"`,
  `input_cctf_decoding = false`. Cache built `Pipeline`s for the last four `(stock, grain, output)` keys.
- Run the engine inside `catch_unwind`, as `develop_linear` does for rawler: a panic becomes `Error::Internal`
  naming the stock (G10). An unknown stock is `Error::Refused` naming it.

**5. Develop, `crates/rollchrome-core/src/tools/develop.rs`.**

- `DevelopItem` and `DevelopAction` gain `#[serde(default)] pub grain: Grain`.
- `resolve_look` returns an enum: the existing LUT `Look`, or `Film { stock }` for `film:` ids. Without the `film`
  feature a `film:` id is `Error::Refused("film simulation is not built into this program")`.
- `execute_develop`, film branch: `develop_linear(raw, exposure 0)` → `render_film` with the action's exposure and
  grain → JPEG through the same `phototools_core::media::jpeg` path as today (8-bit sRGB), or TIFF through
  `prophoto_encode` and `encode_tiff` with the ProPhoto ICC profile (16-bit). Orientation and metadata are handled
  exactly as for LUT looks today. LUT looks are unchanged.
- `DevelopPreview::render` gains `grain`; with a film look it sends its stored exposure-0 linear image (full or
  draft) through `render_film(..., FilmOutput::Srgb, Auto)` and encodes JPEG 90 as today.

**6. Automation, `crates/rollchrome-core/src/automation/mod.rs`.** `list_looks` appends `film_look_list()` when the
feature is on. The plan hash treats `film:` ids like `builtin:` ids (no LUT lookup); `grain` is already in the
serialised plan. The MCP tool descriptions mention film looks.

**7. Desktop, `crates/rollchrome-desktop/src/commands/develop.rs`.** `list_film_looks` (its `_impl` returns
`film_look_list()`), registered in `lib.rs`. `preview_develop` takes `grain: Option<Grain>` (absent means `Fine`).
`plan_develop`/`apply_develop` pass the data directory from `config::film_data_dir`.

**8. The screen, `frontend/rollchrome/src/views/Develop.vue` and `api.ts`.**

- `listFilmLooks()`; `DevelopItem.grain`; `previewDevelop(..., grain)`.
- Look picker order: Neutral · **Kodak film** · **Fujifilm film** · Fujifilm · Ricoh · My LUTs. Existing group labels
  stay as they are.
- A **Grain** select (Off / Fine / 35 mm), `data-testid="grain-select"`, shown only when every selected photograph has
  a film look, applied to the selection, stored with the look and exposure in the module state. A change invalidates
  the dry run and requests a new preview, like exposure does.
- When the shown photograph has a film look, one line under the picker, `data-testid="film-credit"`:
  *Film simulation: spektrafilm (GPL-3.0) · film data CC BY-SA 4.0.*

**9. Licence.** `LICENSE` at the root: the GPL-3.0 text, copied from the pinned commit's `LICENSE`. `license =
"GPL-3.0-or-later"` in every crate's `Cargo.toml` and `"license": "GPL-3.0-or-later"` in every `frontend/*/package.json`.

**10. CI, `.github/workflows/ci.yml`.** Add `libopenblas-dev` to the existing `apt-get install` list, and a step
`cargo test -p rollchrome-core --features film` after the isolated rollchrome-core step.

**11. Documents.** `docs/develop-plan.md`: the status line and the DV-4 section say what was built.
`docs/known-gaps.md`: rows in *Dependencies beyond §2.6* for the three spektrafilm crates (and what they bring:
`wgpu`, BLAS), with the reason; an entry saying the Linux CI build with OpenBLAS was not verified locally.
`docs/rollchrome/manual-verification.md`: **MV-38** — develop three of the owner's RAWs in Portra 160, Ektachrome 100
and Vision3 250D at each grain setting; pass when preview and exported file match by eye, grain Off has none,
and a 40 MP X100VI file exports in under 10 s. `CLAUDE.md`: the new counts and the new gate line.

### Tests — `crates/rollchrome-core/tests/film_tests.rs` (CPU backend unless named)

1. `every_stock_renders_a_gradient_that_differs_from_the_neutral_development` — all 18, a 64×48 synthetic
   ProPhoto gradient: right size, every value finite, not equal to the input encoded neutrally.
2. `two_stocks_render_the_same_input_differently` — Portra 160 vs Ektar 100, mean absolute difference > 1/255.
3. `the_cpu_render_is_identical_run_to_run` — same input, stock and grain twice: identical output.
4. `exposure_compensation_brightens` — +1 EV gives a brighter mean than 0 EV on a mid-grey frame.
5. `grain_off_fine_and_standard_order_the_noise` — on a flat mid-grey 256×256 frame, the standard deviation
   satisfies Off < Fine < Standard, and Off is below 0.5/255. Report the three numbers.
6. `slides_are_scanned_without_paper` — Ektachrome, Kodachrome, Provia, Velvia render with `scan_film`; no paper is loaded.
7. `an_unknown_stock_is_refused_by_name`.
8. `the_data_directory_is_written_once_and_repaired` — first use fills an empty directory; a second use rewrites
   nothing (modification times unchanged); a truncated file is rewritten.
9. `a_film_tiff_is_16_bit_prophoto_with_its_profile` and `a_film_jpeg_is_8_bit_srgb` — through `execute_develop` with
   a test store, on a handmade Bayer DNG from `tests/support/dng.rs` (`DngBuilder::new_bayer`), as `develop_tests.rs` does.
10. `grain_is_carried_through_the_plan` — `plan_develop` keeps each item's grain; the automation plan hash changes when
    only the grain changes.
11. `the_preview_renders_a_film_look_upright_at_preview_size`.
12. `#[ignore]` `the_gpu_meets_the_preview_and_export_budgets` — only on a Mac with a GPU, release build: warm 1600×1200
    ≤ 300 ms, warm 7728×5152 ≤ 4 s, and with grain off the mean absolute GPU–CPU difference ≤ 3/255. Print the times.

Outside the feature: one test in `develop_tests.rs`, `a_film_look_is_refused_when_film_is_not_built_in`, under
`#[cfg(not(feature = "film"))]`. Desktop: `list_film_looks_impl` returns 18 entries with `film:` ids.

`check:develop` (`check-develop.mjs` and its stub) gains four checks, 16–19:

16. The picker has a *Kodak film* group of 13 and a *Fujifilm film* group of 5, and the existing groups are unchanged.
17. The Grain select is hidden for a camera look or LUT, shown with *Fine* for a film look; the credit line shows with it.
18. Changing grain disables Run until a new dry run, and that dry run's items carry the grain.
19. Changing grain sends a new preview request carrying `look: "film:…"` and the grain.

### Do not touch

- The built-in looks, `media/looks/`, the fit, the LUT library, the base tone. Edit and Batch.
- The engine's source, beyond reading it. `SPECIFICATION.md` (G9).
- No new controls beyond Grain (no paper choice, no halation slider, no push/pull, no black-and-white stocks: G11).
  Ideas go in the Report.
- **Never read or change the owner's own configuration file** in tests or checks.

### Acceptance

- [ ] The 18 stocks appear in Develop and render in preview and export; LUT looks render exactly as before.
- [ ] Exposure is the engine's compensation over auto exposure; grain has three settings, Fine by default.
- [ ] The data is embedded, materialised once, repaired when damaged; the directory holds only what is needed.
- [ ] `cargo test -p rollchrome-core` passes **without** the feature (G2); `cargo test -p rollchrome-core --features film`
      passes with it; tests 1–11 pass; test 12 passes when run by hand on the Mac (`--release -- --ignored`).
- [ ] `check:develop` reports 19 checks; every other check passes unchanged.
- [ ] GPL-3.0-or-later in `LICENSE` and every manifest; spektrafilm credited in `CREDITS.md` and on screen.
- [ ] Test counts rise from `main`'s (1286 workspace, 2 ignored; rollchrome-core 441; rollchrome-desktop 26). No existing
      test changed, deleted or ignored; the only new `#[ignore]` is test 12.

### Gates

The gate list in `CLAUDE.md` in full, plus:

```sh
cargo test -p rollchrome-core --features film
cargo test -p rollchrome-core --features film --release --test film_tests -- --ignored   # test 12, prints times
```

### How to work: every step costs money

1. **Run every command in the foreground and wait for it to return.** Never start a command in the background and check
   on it, and never loop or sleep waiting for one. The first build compiles `wgpu` and takes a few minutes: that is
   one step if you wait for it.
2. **While working, run only what covers the change:** `cargo check -p rollchrome-core --features film`,
   `cargo test -p rollchrome-core --features film --test film_tests`, `npm --prefix frontend/rollchrome run check:develop`.
3. **Run the full list under Gates once, at the end.** If one gate fails, fix it and re-run that gate, not the list.
4. **Read each file once**, using a line range or `grep` for large ones such as `Develop.vue` and `develop.rs`. Do not
   re-read a file to confirm an edit you just made.
5. **Do not change approach silently.** If the same failure survives two attempts, stop, say what you tried in the
   Report, and commit what works.

### Report back

Under `## Report` in this file: what was done; **every** deviation, with the reason ("None" only if true); the data
files shipped with their sizes; the three grain standard deviations; test 12's printed times; every gate's actual
output, counts not "passed"; anything unverified. Commit on `dv-4`. Do not merge.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
