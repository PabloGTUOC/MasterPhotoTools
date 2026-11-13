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

Implemented by Claude on 2026-11-13.

**A. In-place fill.**
- `media::develop::par_map_rgb` replaces the per-pixel `flat_map(..).collect()` at seven sites:
  `linear_to_developed`, the three render paths in `render.rs`, the two film encoders in `tools/develop.rs`,
  and the film preview.
- The X-Trans demosaic and the crop collect whole rows, not pixels, and were left alone.

**Effect, one XZ-1 file, no look:**

| Stage | Before | After |
|---|---|---|
| Linear → 16-bit | 0.98 s | **0.03 s** |
| sRGB + JPEG | 1.52 s | **0.56 s** |
| Total | 3.00 s | **1.10 s** |
| Peak memory | 4.1 GB | **1.3 GB** |

**B. Concurrency.**
- `Concurrency`, `execute_develop_with`, `estimated_peak`, `physical_memory` and `DEFAULT_JOBS` are in
  `tools/develop.rs`. Workers are `std::thread::scope` threads fed by a channel.
- **One fix found by a test.** Test 4 first failed with three photographs read ahead under a budget of one:
  a finished photograph was held until the next had been read. The executor now writes ready photographs
  first.
- **Wiring:** desktop `apply_develop` and automation `run_develop` use `for_this_machine(DEFAULT_JOBS)`.
- **Dependency:** `libc` 0.2.189 is added to `rollchrome-core` for `sysconf`. It was already in
  `Cargo.lock` through other crates (G8).

**Benchmark, the six XZ-1 ORFs, JPEG, noise reduction 25:**

| At once | No look: photos/min | No look: peak | Portra 400: photos/min | Portra 400: peak |
|---|---|---|---|---|
| Before DV-8 | 19.7 | 4.95 GB | 19.9 | 4.07 GB |
| 1 | 44.3 | 1.47 GB | 27.3 | 2.11 GB |
| 2 | 56.0 | 2.69 GB | 36.3 | 3.55 GB |
| 3 | 70.9 | 3.54 GB | 49.9 | 4.72 GB |
| **4** | **89.6** | 4.52 GB | **58.3** | 5.73 GB |
| 5 | 81.4 | 5.16 GB | 54.6 | 6.46 GB |

So `DEFAULT_JOBS = 4`. Against the code before DV-8:
- **No look:** 3.05 → 0.67 s per photograph, **4.5× faster**.
- **Portra 400:** 3.02 → 1.03 s per photograph, **2.9× faster**.

**C.** `JobProgress` gained the optional `timingItem` prop, and Develop sets it.

**Gates** (`gate-dv8.log`)

| Gate | Result |
|---|---|
| fmt / clippy / build | rc 0 |
| `--workspace` | **1330 passed**, 0 failed, 4 ignored |
| `phototools-core` | 714 |
| `rollchrome-core` | **469** (461 + 8) |
| `--features film` | **482 passed, 2 ignored** (474 + 8) |
| `phototools-desktop` | 26 |
| `rollchrome-desktop` | 29 |
| `rollchrome-cli` | 8 |
| `check:develop` | **26 / 26** |
| Every other `check:*`, the web and desktop builds | pass |
| Web `check:layout` (it renders the shared `JobProgress`) | 12 routes clean at 390 px |
| Film GPU budget test | 65.5 ms warm preview |

**Unverified**
- **Larger files.** No 40 MP X100VI or D60 NEF was measured. The memory estimate for them is extrapolated
  from the 10 MP ORF and the 24 MP RAF.
- **Film grain with several photographs at once.** It was not compared byte for byte with one-at-a-time:
  the film tests check one photograph, and the concurrency tests use neutral and LUT looks. Each film render
  takes the backend lock and its own pipeline clone, so no state is shared.

## Review

**Approved**, with the caveat that **the reviewer wrote the code**, so this is not an independent review.

**What was checked**
- **Gates:** run on the branch, not taken from the report.
- **Byte-identical output (G7):** every pre-existing test passes unchanged, including the tests comparing
  `execute_develop` bytes to `develop()` + `render()`. Test 1 shows four at once gives the same bytes and
  calls as one at a time.
- **G4:** the store, and so the one exiftool, is used only on the calling thread. Test 2 pins the
  sequential call order.
- **G10:** a worker panic becomes that photograph's failure. Test 6 covers a failing photograph mid-run.
  There is no `unwrap` on a shipped path. The one `unsafe` block is the `sysconf` call, documented.
- **G8:** `libc` is recorded above and in `Cargo.toml`.
- **G11:** the in-place fill was not in the rescope plan's DV-8 wording. It is in this brief because the
  measurement found it, and it is the larger part of the gain. Edit's own `flat_map` sites (`tools/edit.rs`)
  were left alone, since Edit is frozen.

**For the owner:** MV-42.
