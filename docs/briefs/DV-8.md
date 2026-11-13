# DV-8 · Speed: developing many photographs quickly

> Plan: [`rescope-plan.md`](../rescope-plan.md) step DV-8. Branch `dv-8`. The owner asked Claude to implement
> it (2026-11-13: "push and you do DV8"), so Claude measures, writes, implements and reviews it. The review
> states that the reviewer was not independent.

## Brief

### Why

The owner develops many RAW files at once (2026-11-13). The rescope plan asked for photographs to be
developed concurrently, with the limit set from measured memory rather than from cores alone.

### Measure first

Measured with `examples/develop_bench.rs`:
- **Files:** the owner's six Olympus XZ-1 ORFs (10 MP, about 11 MB each), and one 24 MP X-Trans RAF from the
  chart sources (20 MB).
- **Method:** each file is copied to a temporary folder first, with no configuration read. Release build,
  10-core Mac with 32 GB. Peak memory is `/usr/bin/time -l` maximum resident size.

**Per-stage timings, one XZ-1 file, no look (before any change):**

| Stage | Time |
|---|---|
| Read | 0.01 s |
| Decode + demosaic | 0.27 s |
| Denoise | 0.22 s |
| **Linear → 16-bit ProPhoto** | **0.98 s** |
| **sRGB + JPEG encode** | **1.52 s** |
| Total | 3.00 s, peak **4.1 GB** |

The two conversions are plain per-pixel arithmetic. They cost more than demosaicing because each was written
as `par_iter().flat_map(|p| [a, b, c]).collect()`, which rayon builds out of many small vectors and then
joins. The same pattern is in the three LUT render paths, both film encoders and the preview.

So the step has two parts.

### What to build

**A. Fill output buffers in place.** Add one helper, `par_map_rgb`, that allocates the output once and fills
it with `par_chunks_exact_mut(3).zip(..)`. Use it at the seven per-pixel sites on the Develop path. The
per-pixel closures stay as they were, so **every output is byte-for-byte the same**: existing tests already
compare bytes against `render()`, and they must pass unchanged.

**B. Several photographs at once, inside a memory budget.**

`execute_develop_with(plan, store, progress, Concurrency { max_jobs, memory_budget })`:

| Topic | Rule |
|---|---|
| **The store** | Used only from the calling thread. Reads, look resolution, file creation and metadata stay serial, so one persistent exiftool serves the run (G4). |
| **Workers** | Develop and encode only. A panic fails that photograph, not the run (G10). |
| **Write order** | Outputs are written **in plan order**, whatever finishes first. Summary, progress and files are the same as a sequential run's. |
| **Window** | At most `max_jobs` photographs between "read" and "written". Finished photographs are written before another is read. |
| **Memory budget** | `estimated_peak(file size, film, two outputs)` is reserved per photograph. A photograph starts only if it fits the budget, except that one photograph always runs. |
| **Cancel** | Starts nothing new. Photographs already started are written. |
| **Existing callers** | `execute_develop` is `execute_develop_with(…, Concurrency::sequential())`, so every existing test runs the new code. |

**Settings:**
- `Concurrency::for_this_machine(DEFAULT_JOBS)`, where the budget is half the physical memory from
  `sysconf`. That needs `libc`, already in the tree as a transitive dependency; recorded here (G8).
- The desktop app's `apply_develop` and automation's `run_develop` (CLI, MCP) use it.
- `DEFAULT_JOBS` comes from the measurements.

**C. Time per photograph on the screen.** The shared `JobProgress` gains an optional `timing-item` prop.
Develop sets it to "photograph" and shows `1.0 s per photograph · about 2 min 5 s left`. Without the prop,
every other screen is unchanged.

### Out of scope
GPU-side changes to the film simulation; the X-Trans demosaic; developing several folders in one run (not
chosen); a CLI `--jobs` flag.

### Acceptance

**Tests.** Every existing test passes unchanged, including the byte-equality tests. New tests in
`rollchrome-core`:

1. Four at once writes the same bytes, the same `developed` order and the same create/carry sequence as one at
   a time, for a neutral and a LUT look.
2. One at a time, the store sees read, create, carry for each photograph before the next.
3. With three jobs, no more than three photographs are ever read ahead of the writes, and at least two are.
4. With a budget smaller than one photograph, every photograph is still developed, with at most two read
   ahead.
5. Cancelling after the second report writes exactly the first two.
6. An unreadable photograph in the middle fails alone, keeps its place, and the others are developed.
7. The estimate scales with size, film and a second output.
8. This machine reports its memory, and the budget is half of it.

**`check:develop` 26:** a running job shows "… s per photograph · about … left".

**Benchmark:** report photographs a minute for one to five at once, neutral and with Portra 400, and the
peak memory.

**Expected counts:**

| Gate | Target |
|---|---|
| `rollchrome-core` | ≥ 469 |
| `--features film` | ≥ 482 |
| `check:develop` | 26 |

All other gates are unchanged.

### Gates
The full list in `CLAUDE.md`, with counts reported.

## Report

*(to be filled)*

## Review

*(to be filled)*
