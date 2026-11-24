# `UI-9` · Edit says when a RAW's lens went uncorrected; looks named in one place

> **Status:** briefed
> **Plan:** [`docs/rollchrome/app-layout.md`](../rollchrome/app-layout.md), follow-up to UI-3. **Branch:** `ui-9`, cut
> from `main` after this brief.

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md`: **Ground rules** (G1, G2, G7, G10, G11), **Gates**, **The design language**.
- `crates/rollchrome-core/src/tools/develop.rs`: `develop_for_edit` (around line 39), and how Develop turns the
  same `correct_lens(...).err()` into a warning (`lens_note`, and `"developed without lens correction: {reason}"`
  around line 947).
- `crates/rollchrome-core/src/media/edit/pipeline.rs`: `decode_image`.
- `crates/rollchrome-core/src/media/edit/preview.rs`: `PreviewSession::open`.
- `crates/rollchrome-desktop/src/lib.rs` (`open_preview`, `PreviewSessionInfo`) and
  `crates/rollchrome-desktop/src/commands/edit.rs` (`OpenPreviewResult`, `open_preview_impl`).
- `crates/rollchrome-core/src/tools/look_luts.rs`: `ensure_look_lut`.
- `crates/rollchrome-core/src/media/looks/builtin.rs` (`BuiltinLookInfo`, `builtin_look_list`) and
  `crates/rollchrome-core/src/media/film.rs` (`film_look_list`, and the `title` in `film_lut`, around line 770).
- `frontend/rollchrome/src/components/LutPicker.vue` and `frontend/rollchrome/src/views/Edit.vue`.

### Why

Two gaps left by UI-3, both small:

1. **Edit opens a RAW without saying its lens was left uncorrected.** `develop_for_edit` drops
   `correct_lens`'s error on purpose (comment "Known gap" in the code), because `decode_image` has no way to
   carry a warning. Develop reports the same case as a warning on the job. In Edit the photograph just looks
   slightly different (vignetting, distortion) from what Develop would produce, with no explanation. This is a
   silently swallowed outcome on a shipped path (G10).
2. **The names of the look LUT files are worked out in three places.** `ensure_look_lut` names the file it writes
   (`Builtin - <Maker> <Name>.cube`, `<Name> (spektrafilm).cube`, `<Name> neutral (spektrafilm).cube` for tungsten
   stocks), and `LutPicker.vue` rebuilds the same strings by hand to know which library entries are camera looks
   or film simulations and which option is selected. The test stand-ins rebuild them a third time. If the
   naming in core ever changes, the picker silently files camera looks under "Your LUTs" and loses the
   selection. The name must come from core, once.

### What to build

**Part A — the lens warning.**

1. **Core, `tools/develop.rs`.** `develop_for_edit` returns the image **and** its warnings:
   `pub fn develop_for_edit(raw: &[u8]) -> Result<(ImageBuffer<Rgb<u16>, Vec<u16>>, Vec<String>), Error>`.
   When `correct_lens` fails, push exactly `format!("developed without lens correction: {reason}")` (the same
   words Develop uses). Remove the "Known gap" comment and the `_uncorrected` binding; keep a one-line comment
   on why a lens the database does not know is a warning and not an error.
2. **Core, `media/edit/pipeline.rs`.** Add
   `pub fn decode_image_with_warnings(path: &Path) -> Result<(ImageBuffer, Vec<String>), Error>`, which holds
   today's body of `decode_image`. JPEG and TIFF return an empty list. `decode_image` becomes a thin wrapper
   that calls it and drops the warnings, **with a comment saying why that is acceptable**: its callers
   (export, batch, full-stage decode, masks) re-decode a photograph that Edit has already opened and warned
   about. Do not change `decode_image`'s signature or any of its callers.
3. **Core, `media/edit/preview.rs`.** `PreviewSession::open` uses `decode_image_with_warnings` and keeps the
   list in a new `pub warnings: Vec<String>` field (or a `warnings()` accessor; say which in the Report).
   `ensure_full_linear` keeps using `decode_image`.
4. **Desktop.** `PreviewSessionInfo` and `OpenPreviewResult` gain `warnings: Vec<String>`, filled from the
   session. Transport only (G1).
5. **Front end.**
   - `src/api.ts`: `OpenPreviewResult` gains `warnings?: string[]`.
   - `Edit.vue`: after a successful `openPreview`, if `warnings` is non-empty, show them in a new element
     `<div class="edit-screen__open-warning" data-testid="edit-open-warning" role="status">`, placed right after
     the existing `edit-error` block (and before the card read-only banner), one line per warning. It is
     cleared at the start of every `openImage`, so it always describes the photograph on screen. It is a
     warning, not an error: it does not set `error`, and editing goes on. Style with existing tokens only
     (amber/warning colour if the design language has one; no raw hex, no radius above 2px, no glow).
6. **Stand-in, `scripts/edit-harness/stub-api.ts`.** `openPreview` returns
   `warnings: ['developed without lens correction: the lens is not in the database']` when the path ends in
   `.RW2` (case-insensitive), and `warnings: []` otherwise. Do not add a `.RW2` file to the `/Volumes/Photos`
   folder listing: `check:edit-folder` counts three cards.

**Part B — look file names from core.**

7. **Core.** Add `pub library_file_name: String` to `BuiltinLookInfo`, documented as *"the file name
   `ensure_look_lut` gives this look in the LUT library"*, with `#[serde(default)]`.
   - Add one function per kind that both the listing and `ensure_look_lut` call, so the string is built in
     exactly one place:
     - `media::looks::builtin`: `pub fn builtin_lut_file_name(maker: &str, display_name: &str) -> String`.
     - `media::film`: `pub fn film_lut_file_name(name: &str, balance: LutBalance) -> String`, used by
       `film_lut` for its `file_name` (and its `TITLE` is that name without `.cube`). Edit's balance rule
       (tungsten stocks use `LutBalance::NeutralGrey`, all others `LutBalance::AsFilm`) moves into
       `media::film` as `pub fn edit_balance(tungsten: bool) -> LutBalance`, used by `ensure_look_lut` and by
       `film_look_list` to fill `library_file_name`. These must exist with and without the `film` feature
       wherever `film_look_list` does.
   - `builtin_look_list` and `film_look_list` fill `library_file_name`. `ensure_look_lut` uses the same
     functions; its behaviour and the names it produces do not change.
8. **Front end.** `src/api.ts`'s `BuiltinLookInfo` gains `library_file_name: string`. `LutPicker.vue` uses
   `l.library_file_name` everywhere it builds a name today (`builtinFileNames`, `filmFileNames`,
   `selectedOptionValue`) and contains no `"Builtin - "` or `"(spektrafilm)"` string afterwards.
9. **Stand-ins.** `scripts/edit-harness/stub-api.ts`, `scripts/bulk-lut-harness/stub-api.ts` and
   `scripts/develop-harness/stub-api.ts`: their look listings return `library_file_name`, written out per look
   with the same names as now, and their `ensureLookLut` returns that listed name instead of building it again.
   The develop harness's `Stub LUT (spektrafilm).cube` (Develop's "save as LUT", DV-6) is a different thing:
   leave it.

**Tests (new; existing tests stay as they are, G7).**

- `crates/rollchrome-core/src/tools/develop.rs` test module (it has `make_test_dng`):
  `a_raw_whose_lens_cannot_be_corrected_opens_in_edit_with_a_warning`: a handmade DNG has no lens the
  database knows, so `develop_for_edit` returns an image and exactly one warning starting
  `"developed without lens correction: "`.
- `crates/rollchrome-core/src/media/edit/pipeline.rs` or `tests/`: `a_jpeg_opens_with_no_warnings` via
  `decode_image_with_warnings`.
- `crates/rollchrome-core/src/tools/look_luts.rs` test module:
  `every_camera_look_is_written_under_its_listed_name`: for **every** entry of `builtin_look_list()`,
  `ensure_look_lut(&entry.id, tmp, …)` returns `name == entry.library_file_name`.
- Same module, under `#[cfg(feature = "film")]`:
  `a_film_simulation_is_written_under_its_listed_name`, for `film:kodak_portra_400` and the tungsten stock
  `film:kodak_vision3_500t` (check the real ids in `FILM_STOCKS`). Do not bake all 18; each takes seconds.
- `crates/rollchrome-desktop/tests/local_tools.rs`:
  `open_preview_carries_no_warnings_for_a_jpeg` (the `OpenPreviewResult` of a fixture JPEG has
  `warnings.is_empty()`).
- `scripts/check-edit-folder.mjs`, new **step 6** at each window size, after step 5's metrics:
  - fill the field with `/Volumes/Photos/lensless.RW2` and press Enter: `[data-testid="edit-open-warning"]` is
    visible and its text contains `developed without lens correction`, and `[data-testid="edit-error"]` is
    absent;
  - then fill `/Volumes/Photos/sample.jpg` and press Enter: the warning is gone.
  - Three new assertions per size: 32 → **38**. Take the screenshot before step 6, as now.
  - Add step 6 to the header comment.

### Do not touch

- `SPECIFICATION.md` (G9). Develop's own warning path (`lens_note`). `decode_image`'s callers.
- The naming of any LUT file: names produced must be byte-identical to today's.
- Batch and Export do not get a lens warning in this step. Note in the Report if you think they should.
- No new dependency. No refactor outside the files named.

### Acceptance

- [ ] Edit shows "developed without lens correction: …" when a RAW's lens cannot be corrected, and nothing
      for a JPEG; the warning clears on the next open.
- [ ] `develop_for_edit` drops no error: `grep -n "_uncorrected\|Known gap" crates/rollchrome-core/src/tools/develop.rs`
      prints nothing.
- [ ] `grep -n "Builtin - \|spektrafilm)" frontend/rollchrome/src/components/LutPicker.vue` prints nothing.
- [ ] `grep -rn "format!(\"Builtin - \|(spektrafilm)\"" crates/rollchrome-core/src` shows each pattern built in
      exactly one function.
- [ ] `check:edit-folder`: **38** assertions. All other front-end checks pass with their current counts.
- [ ] Test counts: `cargo test --workspace` 1215 → **1220** (+5: the workspace build enables `film`, so all
      four core tests and the desktop test run). `rollchrome-core` 531 → **534** (the film test is not in the
      default build); `rollchrome-core --features film` 553 → **557**; `rollchrome-desktop` 29 → **30**. Others
      unchanged.
      If your counts differ, account for every test in the Report.
- [ ] `CLAUDE.md`'s gate counts and the `check:edit-folder` line updated to the new numbers.

### Gates

```sh
export PATH="$HOME/.cargo/bin:$PATH"
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace                                   # 1220 passing (5 ignored)
cargo test -p rollchrome-core                            # 534
cargo test -p rollchrome-core --features film            # 557 (3 ignored)
cargo test -p rollchrome-desktop                         # 30
npm --prefix frontend/shared run build
npm --prefix frontend/rollchrome run typecheck
npm --prefix frontend/rollchrome run build
npm --prefix frontend/rollchrome run check:transport
npm --prefix frontend/rollchrome run check:edit-folder   # 38 assertions passed
npm --prefix frontend/rollchrome run check:edit
npm --prefix frontend/rollchrome run check:bulk-lut
npm --prefix frontend/rollchrome run check:develop
npm --prefix frontend/rollchrome run check:viewing
npm --prefix frontend/rollchrome run check:batch-layout
npm --prefix frontend/rollchrome run check:edit-layout
```

### Report back

In **Report** below: what you did; anything that deviates from this brief, and why; the **actual
output** of every gate — counts and failures, never just "passed"; and anything you could not
verify, said plainly. Do not tick the acceptance boxes above; the review does that. Commit on `ui-9`
with an imperative subject; no model identifier in the commit.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
