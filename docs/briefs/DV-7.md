# DV-7 · Output choices, and developing without a look

> Plan: [`rescope-plan.md`](../rescope-plan.md) step DV-7. Branch `dv-7`. agy is out of credits until
> 2026-11-14, and the owner asked Claude to implement this step (2026-11-13), so Claude writes, implements and
> reviews it. The review states that the reviewer was not independent.

## Brief

### Why

The owner wants to develop many RAW files into JPEG or TIFF, **with or without** a film simulation (2026-11-13).
Two things prevent that today:

1. **A photograph cannot be developed without a look.** The picker's "None (neutral)" sends `look: null`, and
   `plan_develop` skips that as *"no look chosen"*. The CLI and MCP behave the same way when no `--look` is given.
   `render(…, None, …)` has always been able to produce a neutral development; nothing reaches it.
2. **The output is fixed.** Outputs always go to `<folder>/Developed`, at full size, in one format per run.

### What to build

**A. Neutral is a look.**
- The reserved look id `neutral` resolves to `ResolvedLook::Neutral` and renders through
  `render(&developed, None, format)`.
- `look: None` on a `DevelopItem` keeps its meaning: leave this photograph out.
- The Develop screen shows an unassigned photograph as "None" and sends it as `neutral`. That is what the label
  "None (neutral)" has always promised.
- A new option in the Neutral group, **"Skip (don't develop)"**, sends `null`.
- `rollchrome-cli develop` and MCP `plan_develop` accept `--look neutral` / `"look": "neutral"`.

**B. Output settings for the run.** They are additive: with the defaults, a request serialises exactly as
before, so plans, plan hashes and output bytes are unchanged.

| Setting | Default | Rule |
|---|---|---|
| **Output folder** | `<folder>/Developed` | Any folder inside the configured roots (G6), and not on a card (G5). An existing file is skipped, never overwritten. |
| **Long edge** | original | Downscale only, to at most N pixels on the long edge, preserving aspect ratio (floor, at least 1). It runs **after the look**, so film grain is rendered at the full frame and keeps its scale. `0` is refused. |
| **Format** | as today | JPEG, TIFF, or **JPEG + TIFF** (`also_tiff` with a JPEG format). The RAW is decoded and denoised once. A LUT or neutral look is rendered once per format. A film look is rendered once per format, because the two outputs use different colour spaces. A photograph is skipped if **either** output already exists. |
| **JPEG quality** | 95 | Already exists. Unchanged. |

**Where each setting lives:**

| Layer | Change |
|---|---|
| `DevelopPlan` | Gains `also: Option<OutputFormat>`, `long_edge: Option<u32>` and `output_folder: Option<String>`. Each is `#[serde(default, skip_serializing_if = "Option::is_none")]`, which is what keeps old plans byte-identical. |
| `DevelopAction` | Gains `also_output: Option<String>`, with the same serde attributes. |
| `plan_develop` | Keeps its signature. A new `plan_develop_with(items, format, &PlanOutput, existing)` carries the settings. |
| `FolderStore` | Gains `with_output_dir`. |
| Desktop `plan_develop` and `apply_develop` | Take an optional `output` argument (`{ folder?, long_edge?, also_tiff }`), resolved and checked with the input. |
| `DevelopRequest` (CLI, MCP) | Gains `#[serde(default)] output`. |
| CLI | Gains `--output-folder`, `--long-edge` and `--also-tiff`. |
| MCP | Gains the same three as properties. |

**C. The screen.**
- **Output settings box:**
  - a third format choice, "JPEG + TIFF";
  - a **Long edge** field (empty = original);
  - an **Output folder** field (empty = `Developed` beside the photographs).
- The destination note shows the folder actually used.
- Changing any of these locks Run until a new dry run, as a format change does today.

### Out of scope
Developing several photographs at once (DV-8); lens correction (DV-9); sharpening (DV-10); Edit; Batch Grade;
PhotoTools.

### Acceptance

**Tests.** Every existing test passes unchanged. New tests in `rollchrome-core`:

1. `neutral` develops a photograph: one output, and the same bytes as `render(&developed, None, fmt)`.
2. `look: None` is still skipped as *"no look chosen"*.
3. With default output settings, `serde_json` of a plan equals the plan built by `plan_develop`, and contains
   none of the new keys.
4. JPEG + TIFF writes both files, and metadata is carried to both.
5. If one of the two outputs already exists, the photograph is skipped with that file's name in the reason.
6. Long edge 64 on a 120×80 frame writes a 64×42 output (JPEG and TIFF), and the metadata carries the new size.
7. A long edge larger than the frame leaves it at its own size.
8. Long edge 0 is refused by `PlanOutput::check`.
9. A custom output folder receives the files, and nothing is created under `Developed`.
10. Film (`--features film`): JPEG + TIFF with `film:kodak_portra_400` writes both, and long edge 64 downsizes
    the film output.
11. Automation: the hash of a default request is unchanged by the new field. A custom output folder outside the
    roots is refused.

In `rollchrome-desktop`:

12. A custom output folder outside the roots is refused.

**`check:develop`** gains:

- **23.** An unassigned photograph is sent to the dry run as `"look": "neutral"`. "Skip (don't develop)" sends
  `null`.
- **24.** Choosing JPEG + TIFF, a long edge or an output folder locks Run until a new dry run. That dry run
  carries `output: { also_tiff, long_edge, folder }`.
- **25.** The destination note names the custom folder.

**Expected counts:**

| Gate | Before | After |
|---|---|---|
| `rollchrome-core` | 450 | ≥ 460 |
| `--features film` | 462 | ≥ 473 |
| `rollchrome-desktop` | 28 | ≥ 29 |
| `check:develop` | 22 | 25 |

All other gates are unchanged.

### Gates
The full list in `CLAUDE.md`, with counts reported.

## Report

Implemented by Claude on 2026-11-13 (agy out of credits; owner's instruction).

**What was done**
- **`tools/develop.rs`:**
  - `NEUTRAL_LOOK`, `ResolvedLook::Neutral` (resolved before the LUT library, so a LUT named "neutral" cannot
    change its meaning) and `DevelopOutput { also_tiff, long_edge, folder }` with `check`.
  - `plan_develop_with`. `plan_develop` is now a wrapper over it with the default output.
  - New `DevelopPlan` / `DevelopAction` fields, each skipped in serialisation when unset.
- **The executor:**
  - It had four near-copies of decode, render, write and carry, one per look kind and format. They were
    folded into one `develop_one` per photograph, plus `encode_film_jpeg` and `encode_film_tiff`.
  - This is the one refactor in the step. It was needed so the second output and the resize weren't a fifth
    and sixth copy. With the default output it runs the same calls in the same order, which the unchanged
    tests and the byte-equality tests confirm.
- **`media/develop/render.rs`:** `render_sized`, `downscaled_size` and `downscale_u16_rgb`. With no long edge,
  `render_sized` is exactly `render`.
- **`media/develop/preview.rs`:** `resize_linear` is shared with the film path instead of duplicated, and
  `PreviewLook::Neutral` was added.
- **`develop_folder.rs`:** `FolderStore::with_output_dir`. The refusal message names the chosen folder.
- **Automation (CLI and MCP share it):**
  - `DevelopRequest.output` (`#[serde(default)]`).
  - The output folder is resolved against the roots (G6) and refused on a card (G5).
  - `neutral` is left out of the LUT part of the plan hash.
- **MCP:** `output_folder`, `long_edge` and `also_tiff` on `plan_develop` and `run_develop`.
- **CLI:** `--output-folder`, `--long-edge`, `--also-tiff`, and `--look neutral` in the help text.
- **Desktop:** `plan_develop` and `apply_develop` take an optional `output`, resolved by one `develop_folders`
  helper.
- **Screen:**
  - JPEG + TIFF, a Long edge field, and an Output folder picker (the shared `PathField`).
  - "Skip (don't develop)".
  - An unassigned photograph is sent as `neutral`. Its thumbnail still reads "None".
  - The output settings are part of what the dry run reviewed.
  - `NEUTRAL_LOOK` lives in `developState.ts` and not in the API client: the browser harness replaces the
    client, and the first check run found the view would not load there.

**Mechanical test changes, no assertion touched**
- The new struct fields, set to `None` / `Default::default()`, in 23 struct literals: 15 in
  `develop.rs`'s unit tests, 4 in `develop_tests.rs` and 4 in `looks_tests.rs`.
- `output: Default::default()` in 11 `DevelopRequest` literals.
- `DevelopOutput::default()` in two `plan_develop_impl` calls.

**Gates** (`gate-dv7.log`, then a second run of the workspace)

| Gate | Result |
|---|---|
| fmt / clippy `-D warnings` / build | rc 0 |
| `--workspace` | **1322 passed, 0 failed, 4 ignored** on the second run. The first run had 1321 passed and **1 failed**, and its output did not name the test. It did not recur, which matches the intermittent `phototools-core` failures recorded at DV-5. **Unidentified.** |
| `phototools-core` | 714 |
| `rollchrome-core` | **461** (450 + 11) |
| `rollchrome-core --features film` | **474 passed, 2 ignored** (462 + 12) |
| `phototools-desktop` | 26 |
| `rollchrome-desktop` | **29** (28 + 1) |
| `rollchrome-cli` | 8 |
| `check:develop` | **25 / 25** |
| Every other `check:*`, the web and desktop front ends | pass |
| Film GPU budget test | Warm 1600×1200 preview 70.1 ms. Noise-reduction preview cost 44.1 ms. GPU vs CPU difference 0.000274. |

**Found while checking**
- `check:develop` first failed at check 24: a Vue number input gives a number, and the code called `.trim()`
  on it. It was fixed in the view, not in the check.
- Playwright's Chromium had gone from the cache and was reinstalled (`npx playwright install chromium`)
  before the browser checks could run.

**Unverified:** no real RAW file from the owner's cameras went through DV-7, so MV-41 is added for that.
The speed of JPEG + TIFF with a film look (two film renders) is not measured; DV-8 measures speed.

## Review

**Approved**, with the caveat that **the reviewer wrote the code**, so this is not an independent review.

**What was checked**
- **Gates:** run on the branch (above), not taken from the report.
- **Test counts:** every count went up by exactly the new tests, and nothing was removed, ignored or loosened
  (G7). `git diff` shows the test-file changes are only the mechanical field additions listed.
- **Byte-identical defaults:** tested directly. A default plan's JSON equals `plan_develop`'s and carries none
  of the new keys. Neutral output equals `render(…, None, …)`. A long edge larger than the frame gives the
  unresized bytes. The automation hash of a pre-DV-7 request (no `output` key) is the plan's own hash.
- **G5 and G6:** the output folder goes through `resolve_for_create` / `resolve_output` and
  `refuse_card_folder` in both automation and desktop. There are refusal tests for both, and they assert
  that nothing was created.
- **G1:** the desktop holds path resolution and argument plumbing only. Planning, rendering and writing stay
  in `rollchrome-core`.
- **G10:** no `unwrap` on a shipped path. A refused second output is recorded as skipped, not dropped.
- **G11:** the one refactor (the executor) is stated and justified above. No new dependency, since
  `fast_image_resize` was already one.

**For the owner:** MV-41.
