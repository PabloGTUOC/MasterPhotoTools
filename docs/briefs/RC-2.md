# `RC-2` · The Rollchrome application

> **Status:** briefed
> **Plan:** [`docs/rollchrome-plan.md`](../rollchrome-plan.md), step `RC-2` · **Branch:** `rc-2`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

*Written by the architect. **Self-contained**: the implementer has none of the architect's context,
so anything not written here does not exist for it.*

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11), **Gates**, *The front-end boundary*, *The design
  language*. They apply to every line of this work. G1, G7 and G11 are the ones this step is most
  likely to break.
- `docs/rollchrome-plan.md` — decisions 1–4, rules R1–R5, and the RC-2 section. This brief is that
  section made exact.
- `docs/briefs/RC-1.md` — the step before this one. It moved the editor's **library** code into
  `crates/rollchrome-core`. This step moves the editor's **application** code: its Tauri commands
  and its screens.
- `frontend/shared/src/ui/README.md` — how `@ui/*` and `@host/api` work. Rollchrome uses both.

### Goal

PhotoTools' desktop application still carries the photo editor: the **Edit** and **Batch Grade**
tabs, their Tauri commands and the preview state behind them. This step gives the editor an
application of its own, **Rollchrome**. That means a new Tauri crate, `crates/rollchrome-desktop`,
and a new front end, `frontend/rollchrome`. The two screens move into it **unchanged**, and they
come out of PhotoTools. Afterwards there are two desktop applications: PhotoTools (Ingest, Dates,
Rename, Timeline, Geotag and the image tools), and Rollchrome (Edit and Batch Grade).

It is a **move**, as RC-1 was. The screens, the commands and the preview state go across as they
are, and each changes only where a path or a name has to. Rollchrome's look stays PhotoTools' look
for now; a look of its own is a later product decision (G11).

### Decisions this brief takes (architect, 2026-11-06)

The owner can overturn any of these, but build to them as written.

| | Decision | Why |
|---|---|---|
| A | **Rollchrome reads PhotoTools' configuration file** (`Config::load()`, `~/Library/Application Support/masterphototools/config.json`) and uses its roots, its publishing folder and the directories derived from it. | The LUT library, the presets and the 400 MB of mask models live beside PhotoTools' database (`Config::lut_dir`, `presets_dir`, `models_dir`). Reading the same configuration means everything the owner already has works in Rollchrome on its first launch, with nothing to copy. The refusal to write into the publishing folder (MV-16.7) keeps working for the same reason. |
| B | **Rollchrome keeps its jobs in a ledger of its own**, `rollchrome.sqlite3`, beside PhotoTools' `db.sqlite3`. | At start-up each application marks every job still recorded as running as *interrupted*. That is how it recovers from a crash (F17). With one shared ledger, starting Rollchrome while PhotoTools was ingesting a card would mark PhotoTools' live job as interrupted. |
| C | **Rollchrome's job event is `rollchrome://job`**, not `phototools://job`. | Two applications, two event names. Nothing listens across them. |
| D | **The views keep calling `desktop.*`.** Rollchrome's `api.ts` exports one client under the names `api` and `desktop`. | Then `Edit.vue` and `BulkLut.vue` change only their import paths, and the review can check that they did. |
| E | **Rollchrome borrows PhotoTools' icons** for now (copies of the files in `crates/desktop/icons/`). | An icon of its own is the same later product decision as a look of its own. |

### Change

Do the work in this order. Use `git mv` for every file that moves whole, so history follows it.

#### 1. `rollchrome-core`: where Rollchrome's ledger lives (decision B)

The rule that names the file is logic, so it lives in the library, not the binary (G1).

- Create `crates/rollchrome-core/src/config.rs` with
  `pub fn ledger_path(config: &phototools_core::config::Config) -> PathBuf`. It returns
  `config.database.with_file_name("rollchrome.sqlite3")`, with a doc comment giving decision B's
  reason. Add `pub mod config;` to `crates/rollchrome-core/src/lib.rs`.
- One unit test in that file:
  `rollchrome_keeps_its_jobs_in_a_ledger_of_its_own_beside_phototools`. Given a `Config` whose
  `database` is `<dir>/db.sqlite3`, it asserts that the result is `<dir>/rollchrome.sqlite3` and
  that it differs from `config.database`. Build the `Config` the way
  `crates/desktop/tests/local_tools.rs` does (`roots`, `staging_dir`, `publishing_dir`,
  `thresholds`, `database`). **This is the only new test in this step.**

#### 2. The new crate `crates/rollchrome-desktop`

Model every file on its counterpart in `crates/desktop`. Where this brief is silent, do what
`crates/desktop` does.

- `Cargo.toml`: package `rollchrome-desktop`, `version = "0.1.0"`, `edition = "2021"`. `[lib]`
  named `rollchrome_desktop` at `src/lib.rs`, and `[[bin]]` named `rollchrome-desktop` at
  `src/main.rs`.
  - Build dependency `tauri-build`; dependencies `phototools-core = { path = "../core" }`,
    `rollchrome-core = { path = "../rollchrome-core", features = ["segment"] }`, `tauri`, `serde`,
    `serde_json`; dev-dependencies `tempfile`, `image`.
  - All at **exactly the versions `crates/desktop/Cargo.toml` already uses**. Add any other crate
    only if the compiler asks for it, at `crates/desktop`'s version, and name it in the report. A
    crate no existing manifest already uses is **not** to be added: stop and report instead (G8).
- `build.rs`: as `crates/desktop/build.rs`.
- `tauri.conf.json`, modelled on `crates/desktop/tauri.conf.json`:
  - `productName` `Rollchrome`, `identifier` `com.phototools.rollchrome` (the working name in the
    plan);
  - `beforeDevCommand` / `beforeBuildCommand` with `cwd` `../../frontend/rollchrome`;
  - `devUrl` `http://localhost:5175`, a different port from PhotoTools' 5174, so both can run in
    development at once;
  - `frontendDist` `../../frontend/rollchrome/dist`;
  - one window titled `Rollchrome`, with the same size limits as PhotoTools';
  - `shortDescription` `Non-destructive photo editing and LUT grading`, and a `longDescription` of
    one sentence in the same vein;
  - the same bundle targets, category and `minimumSystemVersion`.
- `capabilities/default.json`: `"windows": ["main"]`, `"permissions": ["core:default"]` and nothing
  else. Rollchrome raises no notifications and opens no network connection from the webview.
- `icons/`: copies of the six files in `crates/desktop/icons/` (decision E).
- `gen/`: whatever `tauri-build` generates on the first build. Commit it, as `crates/desktop/gen/`
  is committed.

#### 3. Move the editor's Rust out of `crates/desktop`

| From `crates/desktop/` | To `crates/rollchrome-desktop/` | How |
|---|---|---|
| `src/commands/edit.rs`, `auto_masks.rs`, `bulk_edit.rs`, `presets.rs` | `src/commands/` | `git mv` |
| `tests/bulk_edit_desktop.rs` | `tests/bulk_edit_desktop.rs` | `git mv` |
| `src/lib.rs`: `NEXT_SESSION_ID`, `PreviewDimensions`, `PreviewSessionInfo`, `ActivePreview`, and the `AppState` methods `open_preview`, `close_preview`, `close_any_preview`, `render_preview`, `segmentation_input`, `mask_coverage` | `src/lib.rs` | cut and paste, doc comments included |

**`crates/rollchrome-desktop/src/lib.rs`** — a doc comment on the G1 model ("transport, platform
integration and process lifecycle only; everything Rollchrome does lives in `rollchrome-core` and
`phototools-core`"), then `pub mod commands; pub mod jobs;` and an `AppState` with exactly three
fields:

- `config: RwLock<Arc<Config>>`
- `pub jobs: JobRunner`
- `active_preview: Mutex<Option<ActivePreview>>`, with its doc comment about the 65 MB

It takes `AppState::new(config: Config, ledger: Ledger, sink: Arc<dyn JobEventSink>)`, the same
signature as PhotoTools', so the moved tests construct it unchanged. It has `config()` (copied from
PhotoTools' `AppState`) and the six moved preview methods. No `server`, no card watcher and no
`set_config`: Rollchrome reads the configuration and never writes it.

**`crates/rollchrome-desktop/src/jobs.rs`** — a copy of `crates/desktop/src/jobs.rs`, with
`JOB_EVENT` set to `"rollchrome://job"` (decision C).

**`crates/rollchrome-desktop/src/commands.rs`** — the module root the four moved files expect
(they `use super::{describe, resolve_input, resolve_inputs, resolve_output, CommandResult}` and
`use crate::AppState`). Copy these from `crates/desktop/src/commands.rs`, verbatim, doc comments
included:

- `CommandResult`, `describe`, `resolve_input`, `resolve_inputs`, `resolve_output`;
- the four commands Edit and Batch Grade call besides their own: `get_job`, `cancel_job`,
  `cancel_job_impl`, `list_directory`, `list_roots`.

Then `pub mod auto_masks; pub use auto_masks::*;` and the same for the other three. This copies
about sixty lines of transport. It is deliberate: these are command wrappers, not functionality (G1),
and a crate shared between two binaries for sixty lines would be invented scope (G11).

**`crates/rollchrome-desktop/src/main.rs`**, modelled on `crates/desktop/src/main.rs`:

1. Load the configuration exactly as PhotoTools does, with the same fallback and message
   (decision A).
2. Open the ledger at `rollchrome_core::config::ledger_path(&config)`, failing the same way
   PhotoTools does if it cannot (decision B).
3. In `.setup`, build the `TauriSink` and the `AppState`, and run `state.jobs.recover()` with
   PhotoTools' handling of its result.
4. `.on_window_event`: the block that closes the preview session when the window closes, moved
   **from** `crates/desktop/src/main.rs` with its comment.
5. `generate_handler!` with: `get_job`, `cancel_job`, `list_directory`, `list_roots`, and the
   editor's 21 commands as they appear in PhotoTools' list today, `load_recipe` through
   `apply_bulk_edit`, in the same order.

No plugins: no notification, no autostart.

**The preview stays fast (R3).** `render_preview` and `render_mask_coverage` return raw bytes
through `tauri::ipc::Response`, and the session stays in Rust between renders. That code moves
untouched. Do not route a preview through JSON, base64 or a data URL.

**Desktop tests that move.** Create `crates/rollchrome-desktop/tests/local_tools.rs` and **move**
(cut, not copy) these nine tests into it from `crates/desktop/tests/local_tools.rs`, together with
`create_test_cube`, which only they use:

`export_edited_image_refuses_destination_inside_publishing_folder`,
`bulk_lut_refuses_output_directory_inside_publishing_folder`,
`apply_bulk_lut_refuses_when_the_lut_changed_after_the_reviewed_plan`,
`render_preview_returns_width_height_and_rgba_of_that_size`,
`mask_coverage_is_served_for_the_open_preview`,
`an_automatic_mask_without_its_model_is_refused_by_name`,
`opening_a_second_preview_closes_the_first`,
`a_saved_recipe_is_loaded_again_when_the_photo_is_reopened`,
`an_unreadable_sidecar_is_reported_not_treated_as_no_edits`.

**Copy** the `Fixture` struct and `fixture()` into the new file; the seven tests that stay use them
too. In the moved tests and in `bulk_edit_desktop.rs`, the **only** permitted change is the crate
name in paths: `phototools_desktop::` becomes `rollchrome_desktop::`. Every other line, assertions
included, stays as it is. The review compares them function by function.

These seven stay in `crates/desktop/tests/local_tools.rs`:
`a_date_scan_runs_locally_with_no_server_present`, `a_path_outside_the_roots_is_refused`,
`cancel_job_stops_an_active_job_and_refuses_a_finished_job`,
`local_jobs_run_while_the_server_is_unreachable`,
`the_server_address_can_be_changed_without_a_restart`,
`a_wrong_address_is_reported_differently_from_a_stopped_server`,
`a_two_hundred_from_the_wrong_service_is_not_reachable`.

Prune the `use` lines in both files to what each uses.

#### 4. What PhotoTools' desktop crate loses

- `src/commands.rs`: remove the four `pub mod … ; pub use …::*;` pairs.
- `src/main.rs`: remove the 21 editor entries from `generate_handler!` (`load_recipe` through
  `apply_bulk_edit`) and the `.on_window_event` block (it moved).
- `src/lib.rs`: remove what moved (see the table above) and the `rollchrome_core` imports. The
  `AppState` keeps `config`, `jobs`, `server` and `card_watcher`. Remove the doc comment that
  describes `active_preview`, not the others.
- `Cargo.toml`: remove the `rollchrome-core` dependency and its comment. **Leave every other
  dependency and dev-dependency**, including any that is now unused: tidying them is RC-3's.

#### 5. The new front end, `frontend/rollchrome`

Model every file on its counterpart in `frontend/desktop`. Where this brief is silent, do what
`frontend/desktop` does.

- `package.json`: name `@phototools/rollchrome`, `private`, `type: module`. Scripts `dev`
  (`vite --port 5175 --strictPort`), `build`, `typecheck`, `check:transport`, `check:edit`,
  `check:bulk-lut`, written as `frontend/desktop`'s are. Dependencies, at `frontend/desktop`'s
  versions: `@phototools/shared` (`file:../shared`), `@tauri-apps/api`, `vue`, `vue-router`.
  Dev-dependencies, at the same versions: `@types/node`, `@vitejs/plugin-vue`, `@vue/tsconfig`,
  `playwright`, `typescript`, `vite`, `vue-tsc`. **No `leaflet`**, and nothing that is not already
  in `frontend/desktop/package.json`. Run `npm install` to create `package-lock.json`, and commit
  it.
- `.gitignore`: a copy of `frontend/desktop/.gitignore`, so `layout-proof/` stays out of git.
- `index.html`, `tsconfig.json`, `tsconfig.node.json`, `vite.config.ts`, `src/vite-env.d.ts`:
  copies of `frontend/desktop`'s, with the port set to `5175` and `leaflet` dropped from `dedupe`.
- `tsconfig.app.json`: as `frontend/desktop`'s, except that its `include` lists `src/**` and **only
  the shared files Rollchrome compiles**, not `../shared/src/ui/**`. The shared views use
  `ApiClient` methods Rollchrome does not have, and would fail its typecheck. Expected list:
  `../shared/src/ui/useRoots.ts` and `../shared/src/ui/components/` `JobProgress.vue`,
  `PathField.vue`, `PathListField.vue` and `FolderPicker.vue`. If the compiler asks for one more
  (TS6307), add it and name it in the report.

**Moves into it** (`git mv`):

| From | To |
|---|---|
| `frontend/desktop/src/views/Edit.vue`, `BulkLut.vue` | `frontend/rollchrome/src/views/` |
| `frontend/desktop/src/recipeClipboard.ts` | `frontend/rollchrome/src/recipeClipboard.ts` |
| `frontend/shared/src/ui/components/` `AdjustmentSlider.vue`, `CollapsibleSection.vue`, `ColorWheel.vue`, `CropOverlay.vue`, `CurveEditor.vue`, `Histogram.vue`, `LutPicker.vue`, `MaskOverlay.vue`, `PresetBar.vue` | `frontend/rollchrome/src/components/` |
| `frontend/desktop/scripts/check-edit.mjs`, `check-bulk-lut.mjs`, `edit-harness/`, `bulk-lut-harness/` | `frontend/rollchrome/scripts/` |

Nothing outside the editor uses those nine components; `git grep` confirms it.

**What may change in what moved:**

- In `Edit.vue` and `BulkLut.vue`, only import specifiers: the nine components become
  `../components/X.vue`. `@ui/components/JobProgress.vue`, `PathField.vue` and `PathListField.vue`,
  `@ui/useRoots` and `@host/api` stay as they are.
- In the nine components, only import specifiers. `LutPicker.vue`'s `./PathField.vue` becomes
  `@ui/components/PathField.vue`. Their `import type { … } from '@phototools/shared'` stays: the
  editor's types remain in the shared package for now, and moving them is RC-3's.
- In the two check scripts and their harnesses, only paths. Every assertion, threshold and timing
  budget stays exactly as it is (G7). The screenshots land in `frontend/rollchrome/layout-proof/`.

**New files:**

- `src/api.ts` — Rollchrome's transport, the only file allowed to call `invoke` or `listen`. It
  holds:
  - a header comment: the Tauri transport for Rollchrome; desktop-only (plan, decision 2); no HTTP
    client, ever;
  - a class `RollchromeClient` with:
    - `roots()`, `list(path)`, `job(id)`, `watchJob(id, onEvent, signal)` and `cancelJob(id)`,
      copied from `frontend/desktop/src/api.ts` (`watchJob` with its comments, and with its event
      name `'rollchrome://job'`, decision C);
    - every method in the *Editing & LUT commands* section of `TauriApiClient`
      (`frontend/desktop/src/api.ts`, from `loadRecipe` to `deletePreset`), moved;
  - every editor type and function after the class, **moved**: `PreviewHistogram` through
    `LutLibraryList`, including `decodePreviewFrame`;
  - `export const rollchrome = new RollchromeClient(); export const api = rollchrome; export const
    desktop = rollchrome;` (decision D).

  `RollchromeClient` does **not** implement `ApiClient`: it offers none of PhotoTools' tools, and a
  method one transport has to throw for is worse than one it never offers (`CLAUDE.md`). Types may
  be imported from `@phototools/shared` with `import type` only. The compiled HTTP client must
  never be imported.
- `src/main.ts` — hash history, `import '@ui/style.css'`, routes `/` → `Edit` and `/bulk-lut` →
  `BulkLut`.
- `src/App.vue` — the shell, modelled on `frontend/desktop/src/App.vue`: the same sidebar, the same
  status bar, the same scoped styles. Brand `ROLLCHROME`, sub-line `// EDIT`, nav `01 Edit` and
  `02 Batch Grade`, status bar `ROLLCHROME v0.1.0` and the clock. No server probe and no sync.
- `scripts/check-transport.mjs` — a copy of `frontend/desktop`'s, scanning `src/` and the shared
  files Rollchrome compiles, with one more rule: no file imports `HttpApiClient` or imports a value
  (as opposed to `import type`) from `@phototools/shared`. That is what makes "Rollchrome has no
  HTTP client" a check rather than a claim.

#### 6. What PhotoTools' desktop front end loses

- `src/main.ts`: the `Edit` and `BulkLut` imports, their comments and their two routes.
- `src/App.vue`: the two `rawLinks.push` lines for Edit and Batch Grade, and the sentence about
  ED-7 in the comment above them. The tabs after Split renumber; that is correct.
- `src/api.ts`: the *Editing & LUT commands* section of `TauriApiClient` and the editor types and
  functions after it (what moved to Rollchrome). `ServerSettings`, `syncTimeline` and everything
  above the section stay.
- `package.json`: the `check:edit` and `check:bulk-lut` scripts. **Leave the dependencies.**

#### 7. The workspace and the documents

- Root `Cargo.toml`: add `"crates/rollchrome-desktop"` to `members`. No profile section: binaries
  stay at the default (see the comment there).
- `CLAUDE.md`, **Gates** block only:
  - set the counts to the new ones (see *Acceptance*);
  - replace the two `frontend/desktop` lines for `check:edit` and `check:bulk-lut` with a
    `frontend/rollchrome` block (`typecheck`, `build`, `check:transport`, `check:edit`,
    `check:bulk-lut`);
  - in the sentence after the block, `frontend/desktop/layout-proof/` becomes
    `frontend/rollchrome/layout-proof/`.

  The rest of `CLAUDE.md` is RC-3's.
- `docs/testing.md`, section *4. Running the two applications*: add **Rollchrome** — from
  `crates/rollchrome-desktop`, `cargo tauri dev` / `cargo tauri build` — with one line saying it
  reads PhotoTools' configuration and LUT library (decision A).
- `docs/rollchrome-plan.md`: the status line becomes *"RC-2 done (branch `rc-2`); RC-3 next."*

### Do not touch

- `crates/core/`, `crates/rollchrome-core/` beyond step 1, `crates/server/`, `frontend/web/`.
- `frontend/shared/` beyond the nine components that leave it. That includes `types.ts`, whose
  editor types stay for now, and `ui/README.md`. Both are RC-3's.
- `.github/workflows/ci.yml`: putting Rollchrome in CI is RC-3.
- `SPECIFICATION.md` (G9), and `CLAUDE.md` outside the Gates block.
- Any dependency in `crates/desktop/Cargo.toml` or `frontend/desktop/package.json` other than the
  `rollchrome-core` line and the two scripts named above.
- `carry_metadata` and exiftool (R1, RC-4); path-based APIs (R2, RC-5).
- Tempting and out of scope:
  - a shared crate for the sixty lines of command helpers;
  - a Rollchrome configuration file of its own, or a settings screen;
  - a new look, colour or icon;
  - renaming `desktop.*` in the views, or tidying anything you move.

  Put ideas in the report.
- **Timing budgets and assertions in every test and every check script** (G7).

### Acceptance

Each one checkable by someone who did not do the work.

- [ ] **Baseline recorded first**, on `rc-2` as cut, before any change: `cargo test --workspace`
      **1011**; `phototools-core` **713**; `rollchrome-core` **188**; `phototools-desktop` **39**;
      `phototools-server` **71**. If any differs, report it before going further.
- [ ] **Test counts: `1011` → `1012`, every test accounted for.**
      - `cargo test -p phototools-core`: **713**, unchanged.
      - `cargo test -p rollchrome-core`: **189**, the 188 plus the one new ledger-path test.
      - `cargo test -p phototools-desktop`: **26** — its 19 unit tests and the 7 integration tests
        that stay.
      - `cargo test -p rollchrome-desktop`: **13** — the 9 moved from `local_tools.rs` and the 4
        in `bulk_edit_desktop.rs`.
      - `cargo test -p phototools-server`: **71**, unchanged.
      - `cargo test --workspace`: **1012**. All with 0 failed and 0 ignored.
- [ ] `git grep -n "rollchrome" -- crates/desktop frontend/desktop` finds nothing, and
      `cargo tree -p phototools-desktop -e normal --prefix none | grep -E "^(ort|rollchrome-core) "`
      finds nothing. PhotoTools no longer links the editor or ONNX Runtime.
- [ ] `git grep -nE "load_recipe|open_preview|render_preview|plan_bulk_lut|plan_bulk_edit|list_luts|list_presets" -- crates/desktop/src frontend/desktop/src`
      finds nothing.
- [ ] `git grep -nE "Edit\.vue|BulkLut\.vue|AdjustmentSlider|LutPicker|MaskOverlay|PresetBar|CurveEditor" -- frontend/desktop frontend/shared/src/ui frontend/web`
      finds nothing.
- [ ] `frontend/rollchrome` has no HTTP client: `check:transport` passes there with its extra rule,
      and `git grep -nE "fetch\(|XMLHttpRequest|EventSource|HttpApiClient" -- frontend/rollchrome/src`
      finds nothing.
- [ ] **The screens moved, not rewritten.** `git diff -M main --stat` shows `Edit.vue`,
      `BulkLut.vue`, the nine components and the two check scripts as renames. In the diff of each,
      only import specifiers or paths change.
- [ ] **The moved tests are unchanged.** Every test in `crates/rollchrome-desktop/tests/` differs
      from its original only in `phototools_desktop::` → `rollchrome_desktop::`.
- [ ] `check:edit` and `check:bulk-lut` pass **from `frontend/rollchrome`**, with every timing
      budget unchanged (G7). Report the numbers they print: drag p95, settle p95.
- [ ] `check:transport` passes for both `frontend/desktop` and `frontend/rollchrome`.
- [ ] No new dependency: every crate in `crates/rollchrome-desktop/Cargo.toml` and every package in
      `frontend/rollchrome/package.json` is one an existing manifest already uses, at its version.
      `git diff main -- Cargo.lock` adds only the `rollchrome-desktop` package entry and changes
      dependency lists.
- [ ] `git diff main --stat -- crates/core crates/server frontend/web .github SPECIFICATION.md`
      is empty.
- [ ] **On the Mac, after review, by the owner:**
      - Rollchrome opens from `crates/rollchrome-desktop` with `cargo tauri dev`, opens a
        photograph, edits it, exports it, and applies a LUT to a folder. The LUTs and presets that
        were there before are still listed.
      - PhotoTools opens from `crates/desktop` without Edit and Batch Grade, and nothing else in it
        changed.
      - Both can run at the same time.

### Gates

Run them in this order, and **never run `cargo test` at the same time as a browser check**: two
tests time themselves and fail on a loaded machine.

```sh
cargo fmt --all --check                                  # no output
cargo clippy --workspace --all-targets -- -D warnings    # no warnings
cargo build --workspace                                  # ok
cargo test --workspace                                   # 1012 passed
cargo test -p phototools-core                            # 713 passed — G2
cargo test -p rollchrome-core                            # 189 passed — G2
cargo test -p phototools-desktop                         # 26 passed
cargo test -p rollchrome-desktop                         # 13 passed

npm --prefix frontend/shared run build

npm --prefix frontend/web run typecheck
npm --prefix frontend/web run build
npm --prefix frontend/web run check:transport
npm --prefix frontend/web run check:layout        # 12 routes clean at 390 px
npm --prefix frontend/web run check:ingest

npm --prefix frontend/desktop run typecheck
npm --prefix frontend/desktop run build
npm --prefix frontend/desktop run check:transport

npm --prefix frontend/rollchrome ci
npm --prefix frontend/rollchrome run typecheck
npm --prefix frontend/rollchrome run build
npm --prefix frontend/rollchrome run check:transport
npm --prefix frontend/rollchrome run check:edit      # budgets unchanged
npm --prefix frontend/rollchrome run check:bulk-lut
```

Rust is installed through rustup. If `cargo` is not on `PATH` in your shell, run
`. "$HOME/.cargo/env"` first.

Optional, and say plainly in the report whether you did it: `cargo tauri dev` from
`crates/rollchrome-desktop` opens a window. If you could not run it, write "not run".

### Report back

In **Report** below:

- what you did;
- anything that deviates from this brief, and why;
- the **actual output** of every gate: counts and failures, never just "passed";
- every crate or package added that this brief did not name;
- every shared file added to Rollchrome's `tsconfig.app.json` beyond the expected five;
- the drag and settle numbers from `check:edit`;
- anything you could not verify, said plainly.

Do not tick the acceptance boxes above; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*

**Approved** — or **Changes requested:**

1. <specific, actionable, one change per item>
