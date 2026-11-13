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

*(below)*

## Review

*(below)*
