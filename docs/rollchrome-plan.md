# Rollchrome — splitting the editor into its own application

> **RC-1 done (branch `rc-1`); RC-2 next.** `main` is green at `9a763ae`; the last
> state with the editor inside PhotoTools is frozen on the **`FullEdition`** branch (`66c49ec`).

PhotoTools grew two products. One is what the specification describes: dates, names, borders,
contact sheets, geotagging, ingest and publishing. The other arrived with the Edit tab and Batch
Grade (ED-1–ED-24): a film emulator with an adjustment pipeline, LUTs and masks. They share image
decoding and metadata, and almost nothing else.

**Rollchrome** is the second one, as its own application. The name is *roll* plus *-chrome*, the
suffix slide film carries (Ektachrome, Kodachrome, Fujichrome), without using any of them.

It has three screens:

| Screen | |
|---|---|
| **Edit** | Moves from PhotoTools: exposure, light, colour, curves, grading, masks. |
| **Batch Grade** | Moves from PhotoTools **unchanged** (owner, 2026-11-05). |
| **Develop** | New: a folder of RAW files, a film look each, developed straight to JPEG or TIFF. Its own plan, [`develop-plan.md`](develop-plan.md), steps DV-1 to DV-4, after RC-1. |

Steps have stable ids (`RC-1`), like the ED and MV numbers, so one can be named in a commit or a
conversation: *"do RC-1"*.

---

## Decisions already taken (owner, 2026-11-04)

| | Decision | Why |
|---|---|---|
| 1 | **One repository, two applications.** Rollchrome is new crates and a new front end in this workspace. | No code is copied, so the RAW decoder and EXIF handling cannot drift apart. Separate repositories later is moving folders, once the boundary has settled. |
| 2 | **Desktop only, never web.** Rollchrome has a Tauri transport and nothing else: no HTTP client, no server routes. | Everything interactive stays in Rust, which is what makes masking fast. A web build would force the pipeline behind a network. |
| 3 | **iPad is a future target, not a current one.** Nothing is built for it now, but nothing may be built that rules it out (rules R1–R2 below). | Tauri v2 builds for iPadOS from the same project; keeping that door open costs nothing now and a rewrite later. |
| 4 | **`FullEdition` is a snapshot**, not a branch to develop on. | It is the last state with both products in one application, kept to compare against. Its CI was red when frozen (since fixed on `main`); that is expected. |

Working names, safe to change before RC-2 creates anything that carries them: crate
`rollchrome-core`, application `rollchrome-desktop`, product name **Rollchrome**, bundle identifier
`com.phototools.rollchrome`. **Confirm the bundle identifier before any iPad or App Store build** —
after that it cannot change without losing the app's identity on every device.

---

## The boundary, as the code stands at `9a763ae`

Measured rather than estimated. The cut is cleaner than expected: **the server uses none of the
editing code**, and only one function crosses from PhotoTools into the editor.

### Moves to Rollchrome

| Where it is now | What |
|---|---|
| `crates/core/src/media/edit/` | The whole engine: `brush`, `color`, `curves`, `geometry`, `grading`, `grain`, `hsl`, `looks`, `lut`, `masks`, `pipeline`, `preview`, `raster`, `vignette` (15 files with `mod.rs`) |
| `crates/core/src/media/segment.rs` | Automatic masks through ONNX Runtime — the **only** user of `ort` |
| `crates/core/src/media/histogram.rs` | Used only by `media/edit/preview.rs` |
| `crates/core/src/tools/` | `edit.rs`, `lut.rs`, `presets.rs`, `bulk_edit.rs`, `lut_library.rs` |
| `crates/core/Cargo.toml` | The `segment` feature, the `ort` dependency, the `auto_mask` example |
| `crates/core/tests/` | `bulk_edit_tests.rs` whole; **the editing tests inside `media_tests.rs` and `tools_tests.rs`** — those two files are mixed and are split, not moved |
| `crates/desktop/src/commands/` | `edit.rs`, `auto_masks.rs`, `bulk_edit.rs`, `presets.rs`, and their entries in `main.rs`'s `generate_handler!` (`load_recipe` through the preset commands) |
| `crates/desktop/src/lib.rs` | The preview-session state on `AppState` (`PreviewSession`, `PreviewDimensions`, `open_preview`, `render_preview`) |
| `crates/core/src/config.rs` | `lut_dir`, `models_dir`, `presets_dir` — editor-only directories |
| `frontend/desktop/src/views/` | `Edit.vue`, `BulkLut.vue` |
| `frontend/shared/src/ui/components/` | Used by nothing else: `AdjustmentSlider`, `CollapsibleSection`, `ColorWheel`, `CropOverlay`, `CurveEditor`, `Histogram`, `LutPicker`, `MaskOverlay`, `PresetBar` |
| `frontend/desktop/scripts/` | `check-edit.mjs`, `check-bulk-lut.mjs`, `edit-harness/`, `bulk-lut-harness/` |
| `docs/` | `edit-plan.md`, `phase-reports/edit-round-3.md`, and MV-20 to MV-22 (25 checks) from `manual-verification.md` |

### Stays in `phototools-core`, shared by both

| What | Why both need it |
|---|---|
| Image decode and encode (`media`, mozjpeg) | Every tool in both applications reads and writes images |
| `media::raw` (`rawler`) | PhotoTools' RAW→JPEG (F14, `ingest/derivation/f14.rs`) and the editor's pipeline both develop RAW files |
| `media::meta` — `nom-exif` reads, `ExifWriter` | Metadata, both ways (but see R1) |
| `tools::carry_metadata`, `Derived`, `Skip` | How every tool's output keeps its source's metadata |
| **The sidecar naming: `is_sidecar`, `sidecar_path`** | **The one cross-link.** They live in `tools/edit.rs` today, and F3 rename uses them so that renaming a photograph renames its `.photoedit` file with it (ED-3). After the split a photograph is edited in one application and renamed in the other, so both must agree on that name. They move out of `tools/edit.rs` into shared code (e.g. `phototools_core::sidecar`) — not into Rollchrome. |

### What the split buys PhotoTools

ONNX Runtime leaves its build entirely: `segment.rs` is its only user. That is the dependency that
raised the MSRV from 1.80 to 1.88 and the one that needs a download from `cdn.pyke.io` at build
time.

---

## Rules that hold throughout

**R1 — No exiftool in Rollchrome's export path, once it moves.** Export currently copies metadata
through `carry_metadata` (`tools/edit.rs:465`), which drives exiftool, a Perl program. iPadOS has no
Perl. Worse than failing: a metadata failure is recorded as a `Skip`, so on an iPad exports would
succeed and **quietly lose their capture date, camera and GPS**. The move itself (RC-1) may keep the
current call; anything new in Rollchrome that writes metadata must not add another exiftool path.
Replacing it is RC-4.

**R2 — Do not make file paths Rollchrome's API.** On iPadOS a file arrives from the document picker
or from Photos, not as a path under configured `ROOTS`. Rollchrome's core should take an image and
hand one back; reading and writing paths is how the desktop supplies them, one adapter among
possible others. Existing path-based functions may move as they are; new ones should not be added.

**R3 — Keep the preview fast path.** `render_preview` returns raw bytes through Tauri's binary IPC
(`tauri::ipc::Response`) and the decoded photograph stays resident in Rust between renders, keyed by
a session id. That is why masking feels immediate, and it carries over to an iPad unchanged. Do not
route previews through JSON, base64 or data URLs.

**R4 — Every step leaves `main` green.** All gates in `CLAUDE.md`, and no test lost: the test count
before a step equals the sum across crates after it. A step that moves tests must account for every
one.

**R5 — G1–G11 apply to the new crates as they do to the old.** `rollchrome-core` is a library that
must build and test with no binary present (G2); `rollchrome-desktop` holds transport and lifecycle
only (G1).

---

## Steps

### `RC-1` · Carve the engine into `rollchrome-core` — no behaviour change

Create `crates/rollchrome-core` (package `rollchrome-core`), depending on `phototools-core`. Move
everything in *Moves to Rollchrome* that lives in `crates/core`: the engine, `segment`,
`histogram`, the five tools, the `segment` feature with `ort` and the `auto_mask` example, and the
editing tests. Move `is_sidecar` and `sidecar_path` into shared code in `phototools-core` first,
and point F3 rename at them.

The desktop application depends on both crates and **still shows Edit and Batch Grade exactly as
before.** This step proves the boundary compiles; nobody using the application should notice it.

Expect to widen some visibility in `phototools-core` (`pub(crate)` → `pub`) for what the engine
uses: the image buffer types, `media::raw`, `carry_metadata`. Widen only what the compiler asks for.

**Acceptance:**
- All gates in `CLAUDE.md` pass.
- `cargo test -p phototools-core` and `cargo test -p rollchrome-core` each pass **with no binary
  crate present** (G2, now for two libraries).
- Their two counts add up to **901**, the `phototools-core` count before the step. Workspace total
  stays **1011**.
- `grep -r "media::edit\|media::segment\|tools::edit" crates/core/src` finds nothing.
- `cargo tree -p phototools-core -e normal | grep ort` finds nothing.

### `RC-2` · The Rollchrome application

Create `crates/rollchrome-desktop` (Tauri v2, product name Rollchrome, bundle identifier per the
working names above) and `frontend/rollchrome` (Vue and Vite, like the other two front ends).
**Tauri transport only** — decision 2. Move into it, out of PhotoTools' desktop:

- the four editing command modules and their `generate_handler!` entries;
- the preview-session state from `AppState`;
- `Edit.vue`, `BulkLut.vue`, the nine editor-only components, and `check:edit`, `check:bulk-lut`
  with their harnesses.

Rollchrome reuses `frontend/shared`'s styles as they are. Whether it gets a look of its own is a
product decision for later, not part of this step (G11).

**Acceptance:**
- Rollchrome builds and opens a window on the Mac (`cargo tauri dev` from
  `crates/rollchrome-desktop`), opens a photograph, edits it, applies a LUT to a folder.
- `check:edit` and `check:bulk-lut` pass from `frontend/rollchrome`, timing budgets unchanged (G7).
- PhotoTools' desktop builds and runs **without** Edit and Batch Grade, and nothing else in it
  changed.
- `check:transport` passes for both desktop front ends; Rollchrome's front end has no HTTP client.

### `RC-3` · Tidy PhotoTools, and teach CI about Rollchrome

- Remove from PhotoTools what RC-1 and RC-2 left behind: the `segment` feature on the desktop's
  `phototools-core` dependency, the editor directories in `Config`, navigation entries and styles
  only the editor used. The `segment` feature already left the desktop's `phototools-core`
  dependency in RC-1, because the feature no longer exists there.
- CI builds and tests the Rollchrome crates and runs Rollchrome's front-end checks. Today
  `check:edit` and `check:bulk-lut` run only locally; this is the moment to put them in CI.
- Documents: `CLAUDE.md`'s *Shape* section describes two applications; `edit-plan.md`, the edit
  phase report and MV-20 to MV-22 move under Rollchrome's documentation, ids unchanged so nothing
  that names them breaks.

**Acceptance:** all gates for both applications pass in CI; `CLAUDE.md` describes what exists.

### `RC-4` · Metadata without exiftool in Rollchrome — when the iPad becomes real

Replace Rollchrome's use of `carry_metadata` with metadata copied in Rust (R1). Not started before
RC-3, and not started at all until an iPad build is actually wanted: until then exiftool works on
the Mac and replacing it is scope nobody asked for (G11). Recorded here so that the reason is not
rediscovered.

**Acceptance:** a Rollchrome export carries capture date, camera, lens, exposure and GPS with no
`exiftool` on `PATH`, asserted by a test that runs with it absent.

### `RC-5` · Files from somewhere other than a path — when the iPad becomes real

Give Rollchrome's core an input and output that is not a filesystem path (R2), with the desktop's
path handling as one implementation. Same condition as RC-4.

---

## Things that will catch you

- **ONNX Runtime downloads at build time.** `ort` fetches its library from `cdn.pyke.io` the first
  time `rollchrome-core` builds with `segment`. On the Mac this just works. In a cloud session it
  needs that host allowed; the `MasterPhotoTools` environment allows it since 2026-11-05.
  `ORT_SKIP_DOWNLOAD=1` lets `cargo clippy` and `cargo check` run without it, since neither links.
- **Two tests time themselves.** `glow_pyramid_executes_within_interactive_budget_during_drag`
  (5 ms, moves to Rollchrome in RC-1) and
  `a_slow_restart_is_bounded_by_the_start_up_allowance_not_the_write_timeout` (300 ms, stays) both
  failed once on a machine running Chromium checks at the same time, and passed ten times out of ten
  on a quiet one. Do not run `cargo test` alongside the browser checks. Do not loosen the budgets
  (G7).
- **The toolchain is pinned to 1.99.0.** New crates inherit it from `rust-toolchain.toml`; there is
  nothing to choose.
- **The sidecar is the one shared contract.** If RC-1 puts `sidecar_path` anywhere PhotoTools cannot
  reach it, F3 rename stops carrying `.photoedit` files and nothing fails loudly: the edits are
  simply orphaned. Its tests in `tools_tests.rs` stay with PhotoTools and must still pass.

---

## Before starting RC-1 locally

```sh
git checkout main && git pull
cargo test --workspace            # expect 1011, the baseline RC-1 must preserve
cargo test -p phototools-core     # expect 901
```

Then, to the architect: *"Write the brief for RC-1."* It goes in `docs/briefs/RC-1.md`, and agy
implements from that — see *How work is done* in `CLAUDE.md`.
