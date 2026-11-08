# `RP-2b` · RAW only in Develop

> **Status:** briefed
> **Plan:** [`docs/rollchrome/parity-plan.md`](../rollchrome/parity-plan.md), step `RP-2b` · **Branch:** `rp-2b`, cut from `main` **after RP-3 is merged**

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md` — **Ground rules** (G1–G11) and **Gates**.
- `docs/rollchrome/parity-plan.md` — *Three tabs*.
- `crates/rollchrome-core/src/media/edit/pipeline.rs::decode_image`; `tools/edit.rs::export_edited_image_with_resolver`; `tools/lut.rs::ACCEPTED`.

### Goal

**The owner's decision (2026-11-08): RAW files are accepted only in Develop. Edit and Batch take JPEG or TIFF, and nothing else.** Today a RAW opened in either is
not developed at all: `decode_image` takes its embedded camera JPEG, so the work starts from 8 bits with the camera's look baked in, and looks as if it were
fine. This step removes that path, and PNG with it, so the three tabs mean what they say.

### Change

**`media/edit/pipeline.rs::decode_image`:**
- A RAW path (`is_raw`) is `Error::Refused` with exactly: `RAW files are developed in the Develop tab. Open the JPEG or TIFF it writes.` The `raw_to_jpeg` branch goes.
- After the format is guessed from the content, anything other than JPEG or TIFF is `Error::Refused`, saying that Edit and Batch take JPEG or TIFF. Guess from content as now: a TIFF named `.jpg` still opens.
- Update the function's doc comment. If anything outside Edit and Batch relies on this function reading a PNG, stop and say so in the Report.

**`tools/edit.rs` export:** refuse a RAW source at the top, with the same message, **before** the identity byte-copy, which would otherwise copy a RAW and call it an export. The `!is_raw &&` condition then goes.

**`tools/lut.rs`:** `ACCEPTED` becomes `["jpg", "jpeg", "tif", "tiff"]`, used by `tools/bulk_edit.rs` as it is now. Update its doc comment.

**`frontend/rollchrome/src/views/Edit.vue`:** the file picker's `selectable` becomes `['jpg', 'jpeg', 'tif', 'tiff']`. **`BulkLut.vue`:** where it describes its input, say JPEG or TIFF.

**Existing `.photoedit` sidecars beside RAW files are left where they are**: nothing deletes or rewrites them. They simply no longer open, which the owner accepted.

**Docs.** `docs/rollchrome/edit-plan.md`, its statements about opening a camera RAW; `docs/develop-plan.md`, *Leaves alone … the RAW path of Edit and Batch Grade*, noting that RP-2b withdrew it; `docs/rollchrome/manual-verification.md`, any check that opens a RAW in Edit or Batch: reword it and say why. `CLAUDE.md`: counts. The plan's status line.

### Do not touch

- `SPECIFICATION.md` (G9). `media/develop/**`, `tools/develop*.rs`, and `phototools-core`'s `media::raw`, which F14 and publishing use.
- `check:edit`, `check:bulk-lut`: unchanged and passing. No new dependencies.

### Acceptance

- [ ] The existing unit test in `pipeline.rs` that decodes a synthetic `.nef` through `decode_image` asserted the behaviour the owner has withdrawn. Rewrite it, in place, as
  `a_raw_file_is_refused_for_editing_and_points_at_develop`, asserting `Error::Refused` with the exact message, for `decode_image` and `PreviewSession::open`. Say so in the Report. It is the **only** existing test whose meaning may change (G7).
- [ ] `rollchrome-core` gains exactly three more: `a_png_is_refused_for_editing`, `an_edit_export_of_a_raw_file_is_refused_and_writes_nothing`, `a_batch_given_a_raw_file_skips_it_and_says_so`.
- [ ] Counts: workspace 1108 → 1111, `rollchrome-core` 278 → 281; everything else unchanged. If a Rust test elsewhere fed a RAW or a PNG to Edit or Batch and now fails, stop and list it in the Report rather than changing it.
- [ ] No `todo!()` or swallowed error on a shipped path (G10).

### Gates

Run `. "$HOME/.cargo/env"` first.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace             # 1111
cargo test -p phototools-core      # 713
cargo test -p rollchrome-core      # 281
cargo test -p phototools-desktop   # 26
cargo test -p rollchrome-desktop   # 20
npm --prefix frontend/shared run build
npm --prefix frontend/rollchrome run typecheck
npm --prefix frontend/rollchrome run build
npm --prefix frontend/rollchrome run check:transport
npm --prefix frontend/rollchrome run check:edit
npm --prefix frontend/rollchrome run check:bulk-lut
npm --prefix frontend/rollchrome run check:develop
npm --prefix frontend/rollchrome run check:viewing
```

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why, **including any change not listed in it**; the **actual output** of every gate, counts and failures, never just "passed"; and anything you could not verify, said plainly. Do not tick the acceptance boxes; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
