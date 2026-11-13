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

*(to be filled)*

## Review

*(to be filled)*
