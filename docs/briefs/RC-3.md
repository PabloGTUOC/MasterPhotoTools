# `RC-3` · Tidy PhotoTools, and teach CI about Rollchrome

> **Status:** briefed
> **Plan:** [`docs/rollchrome-plan.md`](../rollchrome-plan.md), step `RC-3` · **Branch:** `rc-3`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

*Written by the architect. **Self-contained**: the implementer has none of the architect's context,
so anything not written here does not exist for it.*

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11), **Gates**, *Shape*, and *The front-end boundary*.
- `docs/rollchrome-plan.md` — the RC-3 section. Its *Documents* bullet is **already done** by the
  architect on `main`; do not redo it.
- `docs/briefs/RC-2.md` — the step before this one, and its Review. The Review's *Carried to RC-3*
  list is part of this brief (step 4 below).

### Goal

RC-1 and RC-2 split the photo editor out of PhotoTools into **Rollchrome**: the Rust crates
`rollchrome-core` and `rollchrome-desktop`, and the front end `frontend/rollchrome`. Both
applications build, pass their gates and open on the Mac. What remains is cleanup:

- PhotoTools still owns three editor-only directory helpers on its `Config`;
- the shared front-end package still carries the editor's types;
- two dependencies are now unused;
- CI does not build or test Rollchrome's front end at all;
- CI does not test `rollchrome-core` on its own (G2).

This step removes the leftovers and puts Rollchrome under CI. **Nothing anyone can see changes.**
Every test that exists today still exists and passes, and no count moves.

### Change

#### 1. The editor's directories move from PhotoTools' `Config` to `rollchrome-core`

`crates/core/src/config.rs` has three methods on `impl Config` that only Rollchrome uses:
`lut_dir`, `models_dir` and `presets_dir` (around lines 124–167). They derive the LUT library, the
mask models and the presets folder from `self.database`.

- **Move** them into `crates/rollchrome-core/src/config.rs` (the file RC-2 created, which already
  holds `ledger_path`). Each becomes a free function, `pub fn lut_dir(config: &Config) -> PathBuf`
  and likewise for the other two. Keep the bodies and doc comments as they are, with `self.`
  becoming `config.`. Remove them from `crates/core/src/config.rs`.
- **Call sites.** Change every `config.lut_dir()` / `.models_dir()` / `.presets_dir()` to
  `rollchrome_core::config::lut_dir(&config)` and so on. Measured, there are 20:
  - 19 in `crates/rollchrome-desktop/src/commands/` (`auto_masks.rs` 4, `bulk_edit.rs` 4,
    `edit.rs` 6, `presets.rs` 5);
  - 1 in `crates/rollchrome-desktop/tests/local_tools.rs`, line ~143
    (`f.config.lut_dir().join("test.cube")`). This is the only permitted change to a test: the
    call, and nothing else on the line or in the test.
- Two doc comments name the old methods: `crates/rollchrome-core/src/media/segment.rs:5`
  (`Config::models_dir`) and `crates/rollchrome-core/src/tools/bulk_edit.rs:34`
  (`Config::lut_dir()`). Point them at `rollchrome_core::config::models_dir` / `lut_dir`.
- **The directories themselves do not change.** The same paths result: the owner's LUTs, presets
  and models stay where they are. The functions are moved, not rewritten.

#### 2. The editor's types leave the shared front-end package

`frontend/shared/src/types.ts` ends with a block of editor types: `CurvePoint`, `ToneCurves`,
`HslBand`, `HslAdjustments`, `ColorWheel`, `ColorGrading`, `NormalizedCrop`, `Geometry`,
`Vignette` and `FilmGrain`, from the comment `/** A 2D control point for tone curves (ED-10) … */`
(line ~885) to the end of the file. **Every one of them is already defined, identically, in
`frontend/rollchrome/src/api.ts`.** The shared copies are duplicates, and only three Rollchrome
components use them:

- `frontend/rollchrome/src/components/ColorWheel.vue` (`ColorWheel`),
- `CropOverlay.vue` (`NormalizedCrop`),
- `CurveEditor.vue` (`CurvePoint`, `ToneCurves`).

In those three, change `import type { … } from '@phototools/shared'` to
`import type { … } from '../api'`. Then delete the block from `frontend/shared/src/types.ts`.
Nothing in `frontend/web`, `frontend/desktop` or `frontend/shared` uses any of them; the typecheck of
all four packages proves it.

#### 3. Unused dependencies

- `crates/desktop/Cargo.toml`, `[dev-dependencies]`: remove `image`. Its only users were the editor
  tests that moved in RC-2. Keep `tempfile`, which `server.rs` and `detection.rs` tests still use.
- `frontend/desktop/package.json`, `devDependencies`: remove `playwright`. Its only users were
  `check:edit` and `check:bulk-lut`, which moved in RC-2. Use `npm uninstall --prefix
  frontend/desktop playwright` so the lockfile is updated by npm, not by hand. Then check that
  `git diff frontend/desktop/package-lock.json` removes only `playwright`, `playwright-core` and `playwright`'s
  nested `fsevents`.
- **Nothing else.** If you believe another dependency is unused, say so in the report; do not
  remove it.

#### 4. The two nits carried from RC-2's review

1. `frontend/rollchrome/src/api.ts`: restore the doc comment `/** Plan summary for Bulk LUT tool
   (ED-6). */` on the line above `export interface BulkLutPlanSummary`. It was lost in the move.
2. `frontend/rollchrome/scripts/edit-harness/stub-api.ts` and
   `frontend/rollchrome/scripts/bulk-lut-harness/stub-api.ts`: remove `ServerStatus` from the
   `import type { … } from '../../src/api'` list, and remove the `async serverStatus()` method.
   Rollchrome's client has no such type or method, and nothing calls the stub's. Change nothing
   else in either harness.

#### 5. CI (`.github/workflows/ci.yml`)

**Rust job** (`test`): after the step *Isolated core tests* (`cargo test -p phototools-core`), add
one step, named *Isolated rollchrome-core tests*, that runs `cargo test -p rollchrome-core`. Give it
a one-line comment: G2, for the second library.

**Front-end job** (`frontend`):

- Add `frontend/rollchrome/package-lock.json` to `cache-dependency-path`.
- After the step *Desktop front end*, add a step *Rollchrome front end* that runs, as one `run`
  block like the desktop's: `npm --prefix frontend/rollchrome ci`, then `typecheck`, `build` and
  `check:transport`. Comment it in the voice of the desktop step above: Rollchrome compiles a few
  shared components over its own Tauri transport, and has no HTTP client.
- After *Install Chromium* and the existing browser steps, add two steps, *Rollchrome Edit —
  timing budgets* (`npm --prefix frontend/rollchrome run check:edit`) and *Rollchrome Batch Grade*
  (`npm --prefix frontend/rollchrome run check:bulk-lut`). They use the Chromium installed by the
  *Install Chromium* step, which installs through `frontend/web`'s Playwright. Both packages pin
  Playwright `1.62.1`, so the same browser serves both. Comment why they are in CI: until now
  they ran only on the Mac.
- Extend *Upload layout screenshots*, or add a second upload step, so that
  `frontend/rollchrome/layout-proof/` is uploaded too, with `if: always()` and
  `if-no-files-found: ignore` as the existing one has.

**Timing budgets in CI.** `check:edit` asserts a drag p95 of ≤ 50 ms and a settle p95 of
≤ 120 ms against a stub with simulated render delays; on the Mac it measures about 11 ms and 42 ms.
A CI runner is slower. **Do not change a budget, a delay or a threshold to suit CI** (G7). If you
have reason to think a budget will fail on a runner, say so in the report.

**Validate the workflow locally.** You cannot push, so CI will not run before the review. At least
check that the file parses: `ruby -e 'require "yaml"; YAML.load_file(".github/workflows/ci.yml")'`
(Ruby ships with macOS), or `actionlint` if it is installed. Report which you used and its output.

#### 6. Documents

- `frontend/shared/src/ui/README.md`: in *Views, components, and where each belongs* (or next to
  it), add one short paragraph: a third application, Rollchrome (`frontend/rollchrome`), compiles
  **some** shared components (`JobProgress`, `PathField`, `PathListField`, `FolderPicker`) and
  `useRoots`, and **no shared views**. Its `@host/api` offers `roots`, `list`, `job`, `watchJob`
  and `cancelJob`, not `ApiClient`. So a component that reaches for any other `api` method breaks
  Rollchrome's typecheck. Also say that the editor's own components live in
  `frontend/rollchrome/src/components/`, not here.
- `docs/rollchrome-plan.md`: the status line becomes *"RC-3 done (branch `rc-3`); DV-1 next."*
- `CLAUDE.md`: **no change.** The architect has already updated it; the test counts do not move.

### Do not touch

- Any test other than the one call in `crates/rollchrome-desktop/tests/local_tools.rs` (step 1).
- Any timing budget, threshold or simulated delay in the check scripts or harnesses (G7).
- `crates/server/`, `frontend/web/`, `SPECIFICATION.md` (G9), `CLAUDE.md`, and every document under
  `docs/` except `docs/rollchrome-plan.md`'s status line.
- `frontend/shared/src/ui/styles/`. The histogram and mask colour tokens look like editor
  leftovers, but Rollchrome imports the shared stylesheet and uses them.
- Tempting and out of scope: a `[workspace.dependencies]` table; sharing the copied command helpers
  between the two desktop crates; restructuring CI jobs; a matrix; caching changes beyond the one
  path. Put ideas in the report.

### Acceptance

Each one checkable by someone who did not do the work.

- [ ] **Baseline recorded first**, on `rc-3` as cut, before any change: `cargo test --workspace`
      **1012**; `phototools-core` **713**; `rollchrome-core` **189**; `phototools-desktop` **26**;
      `rollchrome-desktop` **13**; `phototools-server` **71**.
- [ ] **Test counts: unchanged**, all six as above, 0 failed, 0 ignored.
- [ ] `git grep -nE "fn (lut_dir|models_dir|presets_dir)" -- crates/core` finds nothing;
      `git grep -nE "\.(lut_dir|models_dir|presets_dir)\(\)" -- crates` finds nothing (the
      `bulk_edit.rs` field `p.lut_dir` and `plan.actions[0].lut_dir` are fields, not calls, and do
      not match).
- [ ] **The directories are unchanged.** The three functions' bodies, diffed against the removed
      methods, differ only in `self` → `config`.
- [ ] `git grep -nE "CurvePoint|ToneCurves|HslBand|HslAdjustments|ColorWheel|ColorGrading|NormalizedCrop|FilmGrain" -- frontend/shared`
      finds nothing.
- [ ] `git grep -n "ServerStatus\|serverStatus" -- frontend/rollchrome` finds nothing, and
      `git grep -n "Plan summary for Bulk LUT" -- frontend/rollchrome/src/api.ts` finds one line.
- [ ] `git diff main -- crates/desktop/Cargo.toml` removes only the `image` dev-dependency.
      `git diff main -- frontend/desktop/package.json` removes only `playwright`.
      `git diff main -- frontend/desktop/package-lock.json` removes only `playwright`,
      `playwright-core` and the nested `fsevents`. `Cargo.lock` changes only in `phototools-desktop`'s dependency list.
- [ ] The only test-file change in the whole diff is the one `lut_dir` call.
- [ ] `ci.yml` parses (report the tool and its output). It runs `cargo test -p rollchrome-core`
      and the Rollchrome front end's `ci`, `typecheck`, `build`, `check:transport`, `check:edit`
      and `check:bulk-lut`, and uploads `frontend/rollchrome/layout-proof/`.
- [ ] `git diff main --stat -- crates/server frontend/web SPECIFICATION.md CLAUDE.md` is empty.
- [ ] **After review, by the owner:** the branch, merged and pushed, turns CI green, both jobs. If
      a Rollchrome check fails only on the runner, that is a finding for the architect, not a
      budget to loosen.

### Gates

Run them in this order, and **never run `cargo test` at the same time as a browser check**.

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

npm --prefix frontend/desktop ci
npm --prefix frontend/desktop run typecheck
npm --prefix frontend/desktop run build
npm --prefix frontend/desktop run check:transport

npm --prefix frontend/rollchrome ci
npm --prefix frontend/rollchrome run typecheck
npm --prefix frontend/rollchrome run build
npm --prefix frontend/rollchrome run check:transport
npm --prefix frontend/rollchrome run check:edit      # budgets unchanged
npm --prefix frontend/rollchrome run check:bulk-lut

ruby -e 'require "yaml"; YAML.load_file(".github/workflows/ci.yml")' && echo "ci.yml parses"
```

If `cargo` is not on `PATH`, run `. "$HOME/.cargo/env"` first.

### Report back

In **Report** below:

- what you did;
- anything that deviates from this brief, and why;
- the **actual output** of every gate: counts and failures, never just "passed";
- the drag and settle p95 from `check:edit`;
- the YAML check's output;
- anything you could not verify, said plainly. CI itself will be unverified until the owner
  pushes.

Do not tick the acceptance boxes above; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*

**Approved** — or **Changes requested:**

1. <specific, actionable, one change per item>
