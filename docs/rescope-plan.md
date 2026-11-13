# Rescope — PhotoTools manages, Rollchrome develops

> **Planned 2026-11-13. Nothing built yet.** Steps have stable ids, `RS-1` onwards for PhotoTools and
> `DV-6` onwards for Develop, like the other plans.

## Why

Mature open-source RAW editors now exist that already have what this project would take months to
build: a library with culling, lens correction, healing, masks, Lightroom catalog import. The
owner read one (greycard, GPL-3.0, Rust) and decided on 2026-11-13: *"there are right now many open
source options for editing so it makes no sense to go so deep into that direction."*

The project keeps what no editor does for this owner: **managing photographs** (names, dates,
locations, borders, formats, the NAS library) and **developing many RAW files at once, with or
without a physical film simulation**. Spektrafilm's film stocks are what the other editors lack.

## Decisions (owner, 2026-11-13)

| | Decision | What follows |
|---|---|---|
| 1 | **The library stays.** | The NAS archive, the ledger, Ingest on the Mac, and the folder tools are untouched except where listed below. |
| 2 | **The website runs on the NAS, as today.** It stores the geopositions (tracks, OwnTracks, timeline), attaches them to photographs, and keeps the file tools: Dates, Rename, Split, Border, TIFF, Sheet, Transform, Geotag. | No new hosting. Editing is not on the website. |
| 3 | **Google Photos publishing is deleted.** | RS-1. Specification F15 no longer describes the system. G9 forbids editing the specification, so this is recorded in `known-gaps.md` instead. |
| 4 | **PhotoTools' RAW tab is removed.** RAW development lives in Rollchrome's Develop only. | RS-2. The core RAW decoding that Develop and Ingest use stays. |
| 5 | **Rename takes a name, or the folder's name.** Folder mode gives `<Folder>_001.jpg`: the folder name **replaces the whole name**, followed only by a number. | RS-3 |
| 6 | **The desktop app runs every website tool locally.** | The shared tool routes already do this. After RS-1 removes Publish, the web-only Publish screen is gone, and the two applications differ only by Ingest (Mac-only: the card reader) and the server's phone endpoint. RS-4 checks it, with no new code expected. |
| 7 | **Rollchrome's Edit and Batch Grade tabs stay as they are, and are not developed further.** | No new ED or RP steps. Their gates still run, so nothing regresses. Their pending manual checks are optional. |
| 8 | **Develop improves, in this order of value: output choices, speed, image quality.** "Many folders at once" was not chosen. | DV-7 to DV-10 |

## Steps

### PhotoTools

#### `RS-1` · Remove Google Photos publishing
**Delete:**
- the web `Publish` screen and its route;
- the server's `/api/publish/folder/*`, `/api/connectors/google/*` and `/api/ingest/sessions/*`
  routes;
- `core::publish` (`api`, `auth`, `crypto`, `publisher`, `folder`);
- the desktop `fill_publishing` and `hand_off_card` / `stage_card` commands;
- `ingest::handoff`, `ingest::staging` and `ingest::remediation` (WF-5's withdrawal: what they
  handed over to no longer exists);
- the Google environment variables and their documentation;
- any dependency that only publishing used (G8: name each in the report).

**Keep:**
- Ingest's scan, validate and deliver (card → folder, verified by hash; G5);
- Takeout import (`f2_takeout`): it reads local files and never talks to Google.

**Ledger:** leave old ledger tables in place with no migration that drops them. A database on the
NAS may still hold rows, and a published-photo record must not disappear. The code simply stops
reading and writing them.

**Docs:** `deployment.md`, `publish-folder-plan.md` and `workflow-plan.md` (status: withdrawn),
`known-gaps.md` (F15), and `manual-verification.md` (MV-16 and the publish checks become
withdrawn, with the date and the reason).

**Acceptance:**
- No route, command, view or test mentions Google publishing.
- Every gate passes.
- Each removed test is listed in the report by name, with why it went. G7 still applies: a removed
  test must belong to removed code, never to code that remains.

**Implementer:** agy. Large, but mechanical.

#### `RS-2` · Remove the RAW tab from PhotoTools
**Delete:**
- `RawToJpeg.vue` and its shared route;
- the server's and desktop's RAW-conversion endpoint and command (`derive_raw`, `/api/ingest/derive`);
- their tests.

**Keep:** the core decoding and derivation functions that Rollchrome or Ingest call. The brief
lists them from a `grep`, so nothing Rollchrome needs is deleted (G2: `rollchrome-core` must
still build).

**Implementer:** agy, or the local model as one narrow job.

#### `RS-3` · Rename from the folder's name
`BatchRenameParams` gains a name source:
- **blocks**: today's date / subject / camera / film, unchanged and the default, so existing
  scripts and saved settings behave the same;
- **folder**: the photograph's parent folder name, sanitised with `sanitise_block`, then
  `_001`, `_002` … in the existing order (capture time or numeric).

The Rename screen gets the choice and shows the folder-mode name in its preview.

**Rules:**
- **Photographs from several folders:** each takes its own folder's name, and numbering restarts
  per folder.
- **A folder name that sanitises to nothing** (for example `!!!`): the photograph is skipped with
  that reason, never named `_001`.
- **Sidecars** (`.photoedit`) follow, as today.
- The zero-padding width covers the folder's count, the same rule as today.

**Tests:** a folder `Playground` with three files → `Playground_001…003`; two folders in one run;
an empty sanitised name; sidecar moved; blocks mode byte-identical to before.

**Implementer:** the local model (small and exact) or agy.

#### `RS-4` · Desktop parity check
No code expected. Claude checks that every website screen left after RS-1 and RS-2 is in the
desktop navigation and runs over Tauri, and records the result here. Anything missing becomes a
step of its own.

### Rollchrome — Develop

#### `DV-7` · Output choices, and developing without a look
**The defect found while planning:** "None (neutral)" sends no look, and the planner skips such a
photograph as *"no look chosen"*. **Today there is no way to develop a RAW file without a film look.**

Separate the two meanings:
- **Neutral**: develop with no look applied.
- **Leave alone**: not in this run.

**Output settings for the whole run:**
- **Output folder:** default `Developed` beside the RAW files, or one chosen folder. The decision
  that an existing file is skipped, never overwritten, still holds.
- **JPEG quality:** 1–100, default today's value.
- **Long edge:** original, or a number of pixels (resampled after the look, so grain is rendered
  at full size).
- **Format:** JPEG, TIFF, or **both in one pass**, developed once and encoded twice.

CLI and MCP get the same options. The defaults reproduce today's output byte for byte.

#### `DV-8` · Speed: several photographs at once
Today the run develops one photograph at a time, with parallel work only inside each one.

**Change:** develop *N* photographs concurrently. *N* is chosen from memory, not only from cores:
a 40 MP film render at f64 holds several GB, so *N* = min(cores ÷ 2, available memory ÷ measured
peak per photograph), at least 1.

**Shared resources:**
- **The GPU film path:** one device, shared, with no new contention bugs.
- **ExifWriter:** one persistent driver (G4), with writes serialised through it.

**Keep:** progress order and cancellation, plus a time per photograph and an estimate of time
remaining on the screen.

**Measure first, then set the target in the brief:**
- photographs per minute on the owner's six XZ-1 files and on a 26 MP and a 40 MP synthetic;
- one at a time against *N*;
- peak memory (the 40 MP export memory is unmeasured today).

#### `DV-6` · Save a film stock as a LUT
Already briefed ([DV-6](briefs/DV-6.md)) and still wanted. It is small and stands alone, so it
fits between the larger steps whenever an implementer is free.

#### `DV-9` · Lens correction
**Each camera behaves differently:**
- **X100VI:** fixed lens.
- **XZ-1:** fixed zoom lens. The RAF and ORF files carry the maker's correction data.
- **D60:** interchangeable lenses, with no correction data in the NEF, which needs a lens database
  (lensfun).

**First, a design note from Claude, not code:**
- what each camera's file carries;
- how greycard's `greycard-lens` does it (GPL-3.0, Rust, so code can be ported with attribution
  in each file);
- where in the pipeline it runs (on linear data, before the look).

Then a brief: distortion, vignetting and lateral chromatic aberration, each of which can be turned
off.

**Acceptance:** a straight line in a test frame stays straight to within a stated number of
pixels, and the corrections are measured on the owner's files.

#### `DV-10` · Capture sharpening
Deconvolution sharpening on the linear image (RawTherapee's capture sharpening, as greycard ports
it). It runs after noise reduction and before the look, so grain stays unsharpened, as on film.
A strength setting, with automatic radius.

**Acceptance:** edge sharpness measured on a test chart, with no halo beyond a stated overshoot,
and a speed budget per photograph.

## Order

| # | Step | Why here | Who |
|---|---|---|---|
| 1 | DV-7 | The neutral defect blocks half of what the owner wants Develop for | agy |
| 2 | DV-8 | Speed for many files; it needs DV-7's output path settled first | agy |
| 3 | RS-3 | Small, wanted, independent | local model |
| 4 | RS-1 + RS-2 | Removal, with no user-facing gain beyond less to maintain, so it does not block Develop | agy |
| 5 | RS-4 | Check after removal | Claude |
| 6 | DV-6 | Already briefed | local model or agy |
| 7 | DV-9 | Needs a design note first | Claude, then agy |
| 8 | DV-10 | After lens correction, in pipeline order | agy |

agy runs only on Gemini Flash. One brief per step, a fresh conversation each time, and the credit
cost reported per brief.
