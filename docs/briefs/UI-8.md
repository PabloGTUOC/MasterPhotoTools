# `UI-8` · Edit opens a folder

> **Status:** briefed
> **Plan:** [`docs/rollchrome/app-layout.md`](../rollchrome/app-layout.md), step UI-8. **Branch:** `ui-8`, cut from
> `main` after UI-3 is merged.

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

### Read first

- `CLAUDE.md`: **Ground rules** (G1, G2, G6, G7, G10), **Gates**, **The design language**, **The front-end
  boundary**.
- `docs/rollchrome/app-layout.md`: rule **4** and the steps table.
- `frontend/rollchrome/src/components/Filmstrip.vue`: built in UI-7, and reused here unchanged unless the
  Report says why.
- `frontend/rollchrome/src/views/Edit.vue`: the path field (`input[placeholder="/path/to/image.jpg"]`), how
  opening a photograph works (`openPreview`, the sidecar load, autosave and its status), the canvas column,
  and UI-2's layout.
- `crates/rollchrome-core/src/tools/develop_folder.rs`: `raw_names` and `thumbnail`, the RAW equivalents
  of what this step needs.
- `crates/rollchrome-desktop/src/commands/develop.rs`: `list_develop_folder` and `develop_thumbnail`, the
  pattern for the two new commands, including `resolve_input` (G6).
- How Develop's view limits concurrent thumbnail requests: check 1 of `check:develop` asserts no overlap.

### Why

The owner chose, on 2026-11-23, for Edit to work like Develop: **open a folder and move between its
photographs on a filmstrip**, not by typing one path at a time. Since UI-3, Edit also opens RAW files, so a
folder from the camera can be edited directly.

### What to build

**1. Core** (`crates/rollchrome-core`, new `tools/edit_folder.rs`; G1).
- `pub fn editable_names(folder: &Path) -> Result<Vec<EditableEntry>, Error>`.
  - **Which files:** JPEG (`jpg`, `jpeg`), TIFF (`tif`, `tiff`) and every `media::raw::RAW_EXTENSIONS`,
    case-insensitive. Files only, no recursion.
  - **Order:** sorted by name, byte order.
  - `EditableEntry { name: String, edited: bool }`, where `edited` is whether the photograph's `.photoedit`
    sidecar exists. Use `phototools_core::sidecar`'s naming, not a re-derivation.
  - `Error::Refused` when the folder is not a directory.
- `pub fn edit_thumbnail(path: &Path, max_edge: u32) -> Result<Vec<u8>, Error>`: an upright JPEG.
  - A RAW goes through `develop_folder::thumbnail`.
  - A JPEG or TIFF goes through `image_ops::decode_oriented`, then `downscale_to_max_edge`, then the same
    JPEG options `thumbnail` uses.
- Tests in that file:
  - `a_folder_lists_jpegs_tiffs_and_raws_and_nothing_else`: a temporary folder with `a.JPG`, `b.tif`,
    `c.RW2` (empty bytes are fine for listing), `d.txt`, `e.png` and a subfolder gives
    `["a.JPG", "b.tif", "c.RW2"]`.
  - `an_edited_photograph_is_marked`: a sidecar beside `a.JPG` gives `edited: true` for it alone.
  - `a_jpeg_thumbnail_is_upright_and_fits`: an oriented JPEG from the core fixtures, with orientation 6,
    gives a portrait thumbnail whose long edge equals `max_edge`.
  - `a_missing_folder_is_refused`.

**2. Desktop** (`crates/rollchrome-desktop`; transport only, G1).
- `list_edit_folder(folder: String) -> Vec<EditableEntryDto>` and
  `edit_thumbnail(folder: String, name: String) -> Response` (JPEG bytes, on `spawn_blocking`). Both resolve
  the folder through `resolve_input` (G6) and validate `name` as Develop's commands do.
- Register both. Add `listEditFolder` and `editThumbnail` to `src/api.ts`.
- Add both to `scripts/edit-harness/stub-api.ts`:
  - `/Volumes/Photos` lists `sample.jpg`, `second.jpg` and `third.tif`, with `second.jpg` marked edited;
  - thumbnails are small generated JPEGs;
  - the stand-in counts concurrent thumbnail calls, as Develop's does.

**3. Edit's screen.**
- **The field keeps its placeholder `/path/to/image.jpg`**, which every Edit check uses, and its label
  becomes **"Folder or photograph"**. On Enter, or a choice from the picker:
  - **a photograph** opens exactly as today, and its folder is listed in the filmstrip with that photograph
    active;
  - **a folder** is listed and its first photograph opens.
- The picker (`PathField`, with `overlay`) allows choosing a folder or a JPEG, TIFF or RAW file.
- **The filmstrip** (`Filmstrip.vue`) runs along the bottom of the canvas column, under the view toolbar,
  the canvas column's full width.
  - `items` are the folder's photographs, with `caption` set to **"edited"** when a sidecar exists.
  - `selection` is `[active]`; Edit selects one photograph at a time.
  - **Clicking a card, or ←/→ with focus in the strip, opens that photograph** through the same function
    Enter uses. Opening another photograph must first let a pending autosave finish, as changing the path
    does today. If it does not today, make it, and **say so in the Report**.
  - After a photograph is saved for the first time, its card's caption becomes "edited" without
    re-listing the folder.
- **Thumbnails load at most 2 at a time**, latest folder wins, the same discipline as Develop.
- **The canvas** shrinks to the space above the filmstrip. `.content` still never scrolls (UI-2's rule).

**4. A check: `check:edit-folder`** (`scripts/check-edit-folder.mjs`, on the whole-application harness;
added to `package.json`). At 1280×800 and 1024×700:
1. Fill the field with `/Volumes/Photos` and press Enter.
   - Three cards, in order, inside the viewport, in one row.
   - `sample.jpg` is active and opened: the stand-in's `openPreview` was called with
     `/Volumes/Photos/sample.jpg`.
   - `second.jpg`'s card shows "edited".
2. Click `third.tif`: `openPreview` was called with its path, and its card is `is-last-clicked`.
3. Focus the strip and press ArrowLeft: `second.jpg` opens.
4. Fill the field with `/Volumes/Photos/second.jpg`: the same three cards, `second.jpg` active.
5. Throughout, the stand-in saw at most 2 concurrent thumbnail calls, and `.content`'s `scrollHeight <=
   clientHeight + 1`.

Print "N assertions passed" and write `layout-proof/edit-folder-<width>.png`.

### Do not touch

- Every existing check, unchanged (G7). They type a photograph's path into the same field, so they must pass
  as they are. If one fails, report it with its output.
- Opening, rendering, saving and exporting a photograph: this step only adds a way to choose one.
- `Filmstrip.vue`'s interface. If Edit needs something it lacks, add it as an optional prop that Develop
  does not pass, and explain it in the Report.
- `docs/` other than this file's Report.

### Acceptance

- [ ] A folder opens in Edit, with its JPEGs, TIFFs and RAWs on a filmstrip. Click and ←/→ change the
      photograph, and edited ones are marked.
- [ ] Pasting a photograph's path still opens it, now with its folder alongside.
- [ ] `check:edit-folder` passes; every gate passes; the four core tests are added.

### Gates

The gate list in `CLAUDE.md` in full, plus `check:edit-layout`, `check:strip` and `check:edit-folder`.

### How to work: every step costs money

1. **Run every command in the foreground and wait for it to return.** Never start a command in the
   background and check on it, and never loop or sleep waiting for one.
2. **While working, run only** `cargo test -p rollchrome-core --lib edit_folder`, `typecheck`, `check:edit`
   and `check:edit-folder`.
3. **Run the full list under Gates once, at the end.**
4. **Read files in ranges.** Do not re-read a range to confirm an edit.
5. **Do not change approach silently.** If the same failure survives two attempts, stop, say what you tried
   in the Report, and commit what works.

### Report back

Under `## Report` in this file: what was done; **every** deviation, with the reason ("None" only if true);
how switching photographs waits for autosave; every gate's actual output, counts not "passed"; anything
unverified. Commit on `ui-8` with imperative subjects under 72 characters and no model name anywhere. Do not
merge.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*
