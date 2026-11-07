# `DV-3` · The built-in looks

> **Status:** briefed
> **Plan:** [`docs/develop-plan.md`](../develop-plan.md), step `DV-3` · **Branch:** `dv-3`, cut from `main` **after DV-1b is merged** (the Fujifilm chart cannot be developed without it)

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

*Written by the architect. **Self-contained**: the implementer has none of the architect's context,
so anything not written here does not exist for it.*

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11), **Gates**, *House style*.
- `docs/develop-plan.md` — *Where the looks come from*, *Looks and the TIFF's colour*, step `DV-3`,
  and *The looks are fitted to the base tone*.
- `docs/briefs/DV-2a.md` and `DV-2b.md` — `render`, `OutputFormat`, `execute_develop`,
  `DevelopStore::look`.
- Code you will build on:
  - `crates/rollchrome-core/src/media/develop/` — `develop`, `Developed` (16-bit **ROMM-encoded**
    ProPhoto: `u16 / 65535` is the encoded value), `tone::prophoto_encode/decode`,
    `colour::prophoto_to_xyz_d50`, `colour::bradford_d50_to_d65`, `render.rs`;
  - `crates/rollchrome-core/src/media/edit/lut.rs` — `Lut`, `Lut::from_cube`, `Lut::sample`;
  - `crates/rollchrome-core/src/tools/develop.rs` — where a look name becomes a `Lut`.

### Goal

Develop can already apply any `.cube` from the owner's LUT library. This step gives it **fourteen
looks of its own**, fitted by us from photographs of a colour chart, shipped inside the application
so they need no import.

The material is in the public-domain repository `bastibe/Darktable-Film-Simulation-Panel`, commit
`d6effd55cf37ce208c24cf8d072ac14028ca289b`, already cloned **outside this repository** at
`~/Desktop/Projects/rollchrome-chart-sources`. In `styles_source/`:

| Camera | RAW | The camera's own conversions of that RAW (sRGB JPEGs, 6000 × 4000) |
|---|---|---|
| Fujifilm X-Pro2 (X-Trans) | `Target.RAF` | `Provia.jpg`, `Velvia.jpg`, `Astia.jpg`, `Classic Chrome.jpg`, `Pro Neg High.jpg`, `Pro Neg Std.jpg`, `Eterna.jpg` |
| Ricoh GR III (Bayer) | `Target.DNG` | `Standard.JPG`, `Vivid.JPG`, `Positive Film.JPG`, `Negative Film.JPG`, `Retro.JPG`, `Bleach Bypass.JPG`, `Cross Processing.JPG` |

Each photograph is a 24-patch ColorChecker. **The fit:** develop the RAW through *our* pipeline
(`develop`, exposure 0), measure the 24 patches; measure the same patches in each camera JPEG;
fit a mapping from ours to the camera's; bake it into a 33³ `.cube`. **None of these photographs
enters this repository** — only the fitted `.cube` files, the fit's configuration and its report.

**Why the model below, and not a free-form fit.** The architect prototyped three before writing
this (on the same files, numbers in *Measured before briefing*). A thin-plate spline in Lab — the
method darktable's own chart tool uses — matches the 24 patches almost exactly but, extended over
ProPhoto's gamut, sent a fifth to two thirds of all colours out of range and **folded** the strong
looks back on themselves (a more saturated input coming out less saturated). A hue-indexed
saturation model folded too. **A 3×3 matrix followed by one monotone curve per channel cannot
fold** — a linear map with positive determinant composed with strictly increasing per-channel
curves is invertible everywhere — stays inside 0..1 by construction, and generalised *better* to a
held-out patch than the spline did. It is the classic camera-profile shape, and cross processing
*is* per-channel curves.

**Why ProPhoto, and the second kind of look.** A library `.cube` is built for sRGB, so Develop feeds
it sRGB (DV-2a). The built-in looks are ours, so they are defined on the ROMM-encoded ProPhoto values
`Developed` already holds, and the TIFF keeps colours sRGB cannot (develop-plan, decision 3). So a
look now carries **which space it expects**.

### The model, exactly

For one look, parameters: a gain `g` (fixed before fitting, not optimised), a 3×3 matrix `M`, and for
each channel `c` a curve `C_c` with `K` knots (`K = 8`; you may choose 6–12, recorded in the
report). Input and output are **ROMM-encoded ProPhoto**, each channel in 0..1 — the LUT's domain.

1. `x = g · decode(in)` — linear ProPhoto (`tone::prophoto_decode` per channel).
2. `y = M · x`.
3. Per channel, a soft floor: `s = τ · ln(1 + e^(y/τ))`, `τ = 1e-4` — strictly positive and
   increasing, ≈ `y` above a few `τ`. (Computed stably for large `|y/τ|`.)
4. `u = tone::prophoto_encode(s)`.
5. `out_c = C_c(u)`:
   - on `0 ≤ u ≤ 1`: a **monotone cubic Hermite** (Fritsch–Carlson) through knots
     `(k/K, y_k)`, `k = 0..K`, with `y_0 = 0`, `y_K = 1 − ε`, `ε = 0.005`, and the increments
     `y_k − y_(k−1)` = `softplus(θ_ck)` rescaled to sum to `1 − ε` — so every curve strictly
     increases;
   - above `u = 1`: `1 − ε · exp(−C_c'(1) · (u − 1) / ε)` — continuous in value and slope, strictly
     increasing, never reaching 1.

**The gain.** `g` = the median, over the four middle grey patches (the 2nd to 5th of the six greys),
of `Y_target / Y_ours` (CIE Y). It takes up the exposure difference between our development and the
camera's (the X-Pro2's JPEGs are about a stop brighter than our development), which otherwise bends
`M` far from identity.

**The fit.** Parameters `M` (9) and `θ` (3K), starting from `M = I` and all increments equal.
Minimise, with a **Levenberg–Marquardt written in this crate** (no new dependency — G8; numerical
Jacobian, a 1e-6 step, damping that adapts, a fixed iteration cap and tolerance, single-threaded so
the result is deterministic):

- for each patch, `w · (Lab(model(ours)) − Lab(camera))`, CIELAB **D50**, with `w = 2` for the six
  greys, `1` for the colour patches, and `0.25` for a patch whose camera JPEG median has any channel
  ≤ 1 or ≥ 254 (clipped by the camera; its true target is unknown);
- plus a ridge `0.02 · (M − I)` on the nine entries.

If the fitted `det M ≤ 0`, the fit is refused with that reason — the no-fold guarantee depends on it.

**Colour conversions.** ProPhoto linear → XYZ D50 with `colour::prophoto_to_xyz_d50`; Lab with
white D50 `(0.96422, 1, 0.82521)`. The camera JPEG: sRGB transfer decoded, sRGB primaries → XYZ D65
→ XYZ D50 by **Bradford** (the inverse of `colour::bradford_d50_to_d65`). The JPEGs are tagged sRGB
(checked: `sRGB IEC61966-2.1`, `ColorSpace sRGB`); a JPEG with an ICC profile that is not sRGB is
refused rather than measured wrongly. ΔE is **CIEDE2000**, implemented here and tested against the
published pairs below.

### Measuring the patches

Patch centres, in pixels on the 6000 × 4000 images, both orientations as the files store them. The
chart's top row is the six greys, darkest at the left; **the first six patches, read left to right
along the top row, are the greys**; then row by row. Our development and the camera's JPEGs share
geometry for each camera (checked by eye at 1000 px; the fit's spread check below catches a box that
misses).

| Camera | column centres x | row centres y |
|---|---|---|
| X-Pro2 (`Target.RAF` and its seven JPEGs) | 2220, 2778, 3330, 3882, 4440, 5010 | 1410, 1962, 2508, 3072 |
| GR III (`Target.DNG` and its seven JPEGs) | 1680, 2190, 2694, 3192, 3672, 4140 | 1368, 1860, 2364, 2880 |

Each patch is the **per-channel median** of a 120 × 120 box at its centre, on the encoded values
(ours: the `Developed` u16; the camera's: the JPEG's u8), then converted. **A box whose spread is
large** — inter-quartile range above 0.04 on any channel, in 0..1 units — means it straddles a patch
edge: the fit refuses, naming the image and the patch.

These coordinates live in a checked-in configuration, `crates/rollchrome-core/looks/fit.json`
(source repository and commit, each camera's RAW, centres and box, each look's id, display name and
JPEG). `serde_json` is already a dependency.

### Change

- **`crates/rollchrome-core/src/media/looks/`** (new module, `pub mod looks` in `media/mod.rs`):
  - `colour.rs` (or inside `fit.rs`) — Lab D50 ↔ linear ProPhoto, sRGB JPEG → Lab D50, CIEDE2000.
  - `fit.rs` — measuring a patch, the gain, the model, Levenberg–Marquardt, baking a 33³ `.cube`
    (standard order, red fastest; `LUT_3D_SIZE 33`; a `TITLE` line; values with **7** decimals;
    each node is the model at the node's encoded RGB), and a fit report per look: per-patch
    ΔE2000, mean, max over unclipped patches, **leave-one-out** mean and max (refit without each
    colour patch in turn and score it — the honest measure of the colours between the patches),
    `det M`, `g`, clipped patches.
  - `builtin.rs` — the registry: for each shipped look an id (`fujifilm-provia`,
    `ricoh-positive-film`, …), a display name (`Provia`, `Positive Film`, …), the maker, and its
    bytes through `include_bytes!` from `crates/rollchrome-core/looks/<id>.cube`.
    `builtin_looks()` lists them; `builtin_look(id)` parses one into a `Look`.
- **A look carries its space.** In `media/develop/render.rs` (or beside `Lut`):
  `pub enum LookSpace { Srgb, ProPhoto }` and `pub struct Look { pub lut: Lut, pub space: LookSpace }`.
  `render(developed, look: Option<&Look>, format)`.
  - `Srgb` — exactly today's behaviour, unchanged.
  - `ProPhoto` — **TIFF**: each pixel's encoded value goes straight through `lut.sample`, clamped to
    0..1, back to u16; no trip through sRGB. **JPEG**: the looked ProPhoto value then goes through
    the same ProPhoto → sRGB 8-bit conversion the no-look JPEG uses.
  - Existing call sites and tests change **only** by wrapping a `Lut` as `Look { lut, space:
    LookSpace::Srgb }` (a constructor is fine). No assertion in an existing test changes (G7).
- **`tools/develop.rs`** — a look named `builtin:<id>` is resolved from the registry and **never**
  reaches `DevelopStore::look`; any other name goes to the library as now. An unknown
  `builtin:` id is skipped with the reason `the built-in look '<id>' does not exist`. `DevelopStore`
  returns `Look` (the library's are `Srgb`).
- **`crates/rollchrome-core/examples/fit_looks.rs`** — thin: reads `looks/fit.json`, takes the
  sources directory as its argument, develops each RAW, runs the fit, writes
  `looks/<id>.cube` and **`looks/fit-report.md`** (one table: every look, `g`, `det M`, mean / max
  ΔE2000 through the shipped path, leave-one-out mean / max, clipped patches, and pass or fail
  against the thresholds below). Logic lives in the module (G1 in spirit). **The numbers in the
  report are measured through the shipped path**: the look parsed back from the `.cube` it just
  wrote, applied by `render` to the developed chart as a TIFF, the TIFF decoded, the patches
  measured from it.
- **`crates/rollchrome-core/looks/`** — `fit.json`, `fit-report.md`, and one `.cube` per look that
  **passes**. A look that fails is not written, not registered, and its row in the report says
  failed and why.
- **Docs:** `docs/develop-plan.md` and `CLAUDE.md` are the architect's; leave them.

### Do not touch

- The base tone, `develop`, the demosaics, colour steps, metadata, `develop_folder.rs` apart from
  what returning `Look` requires.
- Edit and Batch Grade — built-in looks are Develop's only, for now (G11).
- The four Fujifilm styles with no camera JPEG (Classic Neg, Eterna Bleach Bypass, Nostalgic Neg,
  Reala Ace): **not fitted**. If you see a way to fit them honestly, write it in the report.
- No Tauri command in this step: the screen that would list the looks is not designed yet.
- No new dependency.

### Acceptance

**Fit quality, per look, from `fit-report.md`, measured through the shipped path:**

| | threshold |
|---|---|
| mean ΔE2000 over the 24 patches | ≤ 2.5 |
| max ΔE2000 over the patches the camera did not clip | ≤ 7.0 |
| leave-one-out mean over the 18 colour patches | ≤ 5.0 |

Set from the architect's prototype, not guessed. **Expect Velvia to fail** (prototype: mean 3.4,
worst unclipped patch 10 — its hue-dependent saturation is beyond a matrix and three curves), and
Astia to be close (2.4 / 6.4). Within the model you may tune `K` (6–12), the clipped-patch weight and
the ridge, the same values for every look, recorded in the report. **Do not loosen a threshold or
change the model's form to pass a look** (G7, G11) — a look that fails is reported with its numbers,
and the owner decides.

- [ ] `fit-report.md` lists all 14; every shipped `.cube` passes all three thresholds.
- [ ] **Deterministic:** running the example twice gives byte-identical `.cube` files and report
      (`shasum` both runs, in the report).
- [ ] No photograph, JPEG or RAW in the repository (`git diff --stat main` shows only code,
      `fit.json`, `fit-report.md` and `.cube` files).

**Tests** (names are sentences; these, or clearer):

1. `ciede2000_matches_the_published_pairs` — Sharma, Wu and Dalal (2005): `(50, 2.6772, −79.7751)`
   vs `(50, 0, −82.7485)` → **2.0425**; `(50, 0, 0)` vs `(50, −1, 2)` → **2.3669**;
   `(50, 2.5, 0)` vs `(73, 25, −18)` → **27.1492**; each to ±0.0001.
2. `lab_d50_round_trips_through_prophoto` — 1000 seeded random linear ProPhoto values in 0..1, to
   Lab and back, within 1e-5.
3. `the_fit_recovers_a_known_look` — 24 synthetic patch pairs made by a known `g`, `M` and curves
   (from the model above); the fit lands within mean ΔE2000 0.2 of the targets.
4. `the_fit_is_deterministic` — the same inputs fitted twice give identical `.cube` bytes.
5. `the_model_stays_strictly_inside_zero_to_one` — seeded inputs including linear values below 0
   and above 4 after `M`: every output in `(0, 1)`, except input black, which may be 0.
6. `a_patch_box_across_an_edge_is_refused` — a synthetic image where the box straddles two colours;
   the error names the patch.
7. `every_builtin_look_parses_at_33` — iterates the registry.
8. `every_builtin_look_stays_inside_zero_to_one`.
9. `every_builtin_look_keeps_the_greys_in_order` — along the 33 nodes `r = g = b`, output
   lightness strictly increases.
10. `no_builtin_look_folds` — for every cell of the lattice, the forward-difference Jacobian's
    determinant is ≥ 0, and > 0 for at least 99% of cells.
11. `a_prophoto_look_keeps_colours_outside_srgb` — an identity look tagged `ProPhoto` leaves the
    TIFF's pixels within 1/65535 of the no-look TIFF, including a colour outside sRGB (the
    counterpart of DV-2a's `a_look_confines_the_tiff_to_srgb_colours`, which must still pass).
12. `a_prophoto_look_in_a_jpeg_is_applied_before_the_srgb_conversion` — a JPEG rendered with a
    `ProPhoto` look equals the no-look JPEG of the looked image (build both from the same pixels).
13. `a_builtin_look_never_reaches_the_library` — `execute_develop` with `builtin:<a shipped id>`
    produces its output, and the test store's `look` is called **zero** times.
14. `an_unknown_builtin_look_is_skipped_with_its_reason`.

- [ ] Test counts: `rollchrome-core` **244 → 258**, workspace **1069 → 1083**, others unchanged —
      assuming DV-1b merged with its 3 tests; if those numbers differ when you start, say so and
      add 14 to what you find.

### Measured before briefing

The architect's prototype, Python on 8-bit renderings, so indicative only — your numbers come from
the 16-bit shipped path and will differ somewhat. Matrix-plus-curves, baked to 33³, with the gain:

| Look | mean | max | leave-one-out mean |
|---|---|---|---|
| Fujifilm Provia | 1.43 | 3.29 | 2.0 |
| Fujifilm Velvia | 3.41 | 10.11 | 5.3 |
| Fujifilm Astia | 2.36 | 8.20 (6.43 unclipped) | 3.6 |
| Fujifilm Classic Chrome | 0.82 | 2.10 | 1.4 |
| Fujifilm Pro Neg High | 1.35 | 3.58 | 2.1 |
| Fujifilm Pro Neg Std | 1.04 | 2.23 | 1.6 |
| Fujifilm Eterna | 0.85 | 1.84 | 1.3 |
| Ricoh Standard | 0.92 | 1.92 | 1.4 |
| Ricoh Vivid | 1.47 | 3.19 | 2.2 |
| Ricoh Positive Film | 1.46 | 3.57 | 2.6 |
| Ricoh Negative Film | 0.92 | 1.76 | 1.4 |
| Ricoh Retro | 0.95 | 3.18 | 1.4 |
| Ricoh Bleach Bypass | 0.99 | 2.36 | 2.3 |
| Ricoh Cross Processing | 2.26 | 5.32 | 3.9 |

(Ricoh rows without the gain step; it changes little there.)

### Gates

```sh
cargo fmt --all --check                                  # clean
cargo clippy --workspace --all-targets -- -D warnings    # clean
cargo build --workspace                                  # clean
cargo test --workspace                                   # 1083 passed, 0 failed
cargo test -p phototools-core                            # 713 passed
cargo test -p rollchrome-core                            # 258 passed
cargo test -p phototools-desktop                         # 26 passed
cargo test -p rollchrome-desktop                         # 15 passed
cargo run --release -p rollchrome-core --example fit_looks -- ~/Desktop/Projects/rollchrome-chart-sources/styles_source
                                                         # twice; identical shasums
```

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why; the **actual
output** of every gate — paste the `test result:` lines; the whole of `fit-report.md`; the
`shasum`s of both fit runs; which looks failed and by how much; the time the fit takes; and
anything you could not verify, said plainly. Do not tick the acceptance boxes; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
