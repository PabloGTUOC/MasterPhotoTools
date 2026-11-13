# RS-3 · Rename from the folder's name

> Plan: [`rescope-plan.md`](../rescope-plan.md) step RS-3. Branch `rs-3`. Implemented by Claude while agy is
> out of credits, at the owner's instruction (2026-11-13: "carry on with all dev points"). The review states
> that the reviewer was not independent.

## Brief

### Why
The owner (2026-11-13): *"in the rename function, I want to add that either I give the name or an option to
take the name from the current folder the pics are in"*. Asked how, they chose **"Folder name only"**: the
folder's name replaces the whole name, followed only by a number.

### What to build
- `BatchRenameParams` gains `#[serde(default)] name_from_folder: bool`. `false` is today's behaviour: the date,
  subject, camera and film blocks.
- When it is `true`:
  - each photograph is named `<Folder>-NN.<ext>`;
  - the folder's name is sanitised like any block (spaces dropped, `_` becomes `-`, only letters, digits and
    `-` kept);
  - numbering restarts in each folder, padded per folder by the existing rule (at least two digits);
  - the blocks are ignored.
- The separator is `-`, the tool's own, so `Playground-01.jpg`. The planning conversation showed
  `Playground_001.jpg` as an illustration, but the sanitiser deliberately removes `_` from names.
- A folder whose name sanitises to nothing is skipped with the reason. Its photographs are never numbered
  bare.
- Sidecars, collisions and the re-check before each rename behave as today.
- **Wiring:**
  - the server's `RenameRequest` and the desktop's `RenameArgs` pass the field through, defaulting to
    `false`;
  - `RenameRequest` in `frontend/shared/src/types.ts` gains it;
  - the shared Rename screen gets a **Name** choice: "From the fields below" or "From each photograph's
    folder". The fields hide in folder mode.

### Acceptance
- **Tests** in `core/tests/tools_tests.rs`:
  1. Three files in `Playground/` become `Playground-01…03`, and the bytes follow.
  2. Two folders in one batch are numbered separately, with sanitised names.
  3. A folder named `!!!` is skipped with its reason, and the file is untouched.
  4. A sidecar follows its photograph.
  5. A request without the field deserialises to `false`.
- **Existing tests:** unchanged except for the new field, `false`, in 14 struct literals.
- **Counts:** `phototools-core` ≥ 719.
- **Web `check:layout`:** 12 routes clean.

## Report

Implemented by Claude on 2026-11-13.

**What was done**
- `f3_rename.rs`: `name_from_folder`. Planning now builds groups: one group for the blocks, or one per folder
  (sorted by path) under the folder's sanitised name. It numbers within each group. Everything after naming
  (collision, sidecar and duplicate checks) is the existing code, run per group. The folder name is taken from
  the canonical path, so a relative `.` still names the real folder.
- The server's `RenameRequest`, the desktop's `RenameArgs` and the shared `RenameRequest` type carry the field.
- The shared Rename screen gets a **Name** fieldset ("From the fields below — date, subject, camera, film" /
  "From each photograph's folder — Folder-01, numbered per folder"), in the house `.radio` style that clears the
  40 px touch target. The four fields are hidden in folder mode.
- **Deviation:** `-` rather than the `_` in the planning illustration, for the reason given in the brief.
- **Mechanical:** `name_from_folder: false` added to 14 existing `BatchRenameParams` literals: 9 in
  `core/tests/tools_tests.rs`, 4 in `f3_rename.rs`'s unit tests, 1 in `rollchrome-core/tests/tools_tests.rs`.

**Gates** (`gate-rs3.log`)

| Gate | Result |
|---|---|
| clippy / build | rc 0 |
| fmt | **rc 1 on the first run**: the new tests were not formatted. `cargo fmt` was applied, `--check` rerun: rc 0, and `tools_tests` rerun: 38 passed. |
| `--workspace` | **1335 passed**, 0 failed, 4 ignored (1330 + 5) |
| `phototools-core` | **719** (714 + 5) |
| `rollchrome-core` | 469 |
| `--features film` | 482 passed, 2 ignored |
| `phototools-desktop` | 26 |
| `rollchrome-desktop` | 29 |
| `rollchrome-cli` | 8 |
| Every `check:*`, the web and desktop builds | pass |
| Web `check:layout` | 12 routes clean at 390 px, `/rename` included |

**Unverified:** the Rename screen was checked by typecheck and the layout check, not by a run on the owner's
folders. That is MV-43.

## Review

**Approved**, with the caveat that **the reviewer wrote the code**, so this is not an independent review.

**What was checked**
- **Gates:** run on the branch.
- **G7:** no existing assertion changed; only the new field, `false`, was added to existing literals.
- **G6:** the paths still go through `resolve_inputs` in both the server and the desktop. The folder's name is
  only read from them.
- **The old behaviour holds:** the existing rename tests (collisions, sidecars, duplicates, padding, capture
  order) all pass with the field `false`. Test 5 shows an old request still parses to `false`.
- **G11:** nothing outside the brief. `PhotoTools` gains no new dependency.

**For the owner:** MV-43, and whether `-` is right or should be `_`.
