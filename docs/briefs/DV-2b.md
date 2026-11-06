# `DV-2b` · Develop a folder: the plan, the outputs, the metadata, the commands

> **Status:** briefed
> **Plan:** [`docs/develop-plan.md`](../develop-plan.md), step `DV-2`, second half · **Branch:** `dv-2b`, cut from `main` after this brief

One file per piece of work, three sections, filled in turn: the architect briefs, the implementer
reports, the architect reviews. If changes are requested, the implementer reports again under a new
heading and the architect reviews again — nothing is deleted, so the file is the whole history.

---

## Brief

*Written by the architect. **Self-contained**: the implementer has none of the architect's context,
so anything not written here does not exist for it.*

### Read first

- `CLAUDE.md`: **Ground rules** (G1–G11), **Gates**, *Conventions that are load-bearing*.
- `docs/develop-plan.md`: *What it does*, *Decisions* 2 and 3, *The pipeline* step 6, and *Things
  that will catch you*, especially "A RAW's tags describe a RAW" and "Same name, different folder".
- `docs/rollchrome-plan.md`: rules **R1** (no *new* exiftool path) and **R2** (no path-based API in
  the core).
- `docs/briefs/DV-1.md` and `docs/briefs/DV-2a.md`: `develop`, `render`, `OutputFormat`, and the
  handmade DNG writer in `crates/rollchrome-core/tests/support/dng.rs`.
- **The pattern to follow:**
  - `crates/rollchrome-core/src/tools/bulk_edit.rs` (Batch Grade), for sequential processing,
    `Progress`, cancellation and one exiftool process per batch;
  - `crates/rollchrome-desktop/src/commands/bulk_edit.rs`, for a plan command and an apply
    command that spawns a job;
  - their tests, `crates/rollchrome-core/tests/bulk_edit_tests.rs` and
    `crates/rollchrome-desktop/tests/bulk_edit_desktop.rs`, including how Batch Grade proves
    there is one exiftool process (G4).

### Goal

DV-2a turns a developed RAW into a file's bytes. This half does the rest of Develop's function
(develop-plan, *What it does*):

1. a folder of RAW files, and for each photograph a look and an exposure offset, give a **plan**;
2. executing the plan gives the **outputs** and a report, every skipped file with its reason
   (G10).

There is no screen. The function is exercised by tests and by two Tauri commands.

The rules this step exists to honour (develop-plan, decision 2):

- An output keeps its original's name with the new extension, in a **`Developed`** folder beside
  the RAW files: `DSCF1234.RAF` → `Developed/DSCF1234.jpg` or `.tif`.
- It **never overwrites anything**. A name already taken is skipped with its reason, **never
  renamed**. So not Edit's `exclusive_create_target`, which adds `_1`.
- **Its metadata is the original's, in full**, with three corrections, because some of a RAW's
  tags are true of the RAW and false of the output.
- **A photograph given no look is left alone.**

**R2 and G1 together decide the shape.** The planning and the execution live in `rollchrome-core`
(G1: no logic in the binary). They see the folder only through a small **store** trait, so they
take names and bytes, not paths (R2). One implementation of the store is the filesystem, and it
also lives in `rollchrome-core`, as an adapter; a future iPad one would be another. The desktop
command only resolves the folder against the roots (G6) and starts the job.

**Metadata goes through `ExifWriter`**, the persistent exiftool driver that `carry_metadata` uses.
That is not a new exiftool path (R1): it is the same driver, so RC-4 replaces it once for
everything. One process per batch (G4).

### Change

**1. `crates/rollchrome-core/src/tools/develop.rs`**, declared in `tools/mod.rs`: the function.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevelopItem { pub name: String, pub look: Option<String>, pub exposure_ev: f32 }
// `name`: a file name in the folder, no directory. `look`: a LUT in the library, by name or sha256.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevelopAction { pub source: String, pub output: String, pub look: String, pub exposure_ev: f32 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevelopPlan { pub format: OutputFormat, pub actions: Vec<DevelopAction>, pub skipped: Vec<Skip> }

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DevelopSummary {
    pub developed: Vec<String>,          // output names written
    pub skipped: Vec<Skip>,              // the plan's, plus anything skipped while running
    pub failures: Vec<Skip>,             // could not be developed or written, with the reason
    pub metadata_skipped: Vec<Skip>,     // written, but the metadata could not be carried
}

pub trait DevelopStore {
    fn existing_outputs(&self) -> Result<BTreeSet<String>, Error>;   // names already in Developed/, lowercased
    fn read(&self, name: &str) -> Result<Vec<u8>, Error>;
    fn look(&self, look: &str) -> Result<Lut, Error>;
    /// Writes `bytes` as a new file. Must fail with `Error::Refused` naming the file if it already exists.
    fn create_new(&mut self, output: &str, bytes: &[u8]) -> Result<(), Error>;
    fn carry_metadata(&mut self, source: &str, output: &str, width: u32, height: u32, format: OutputFormat) -> Result<(), Error>;
    fn finish(&mut self) {}  // e.g. close the exiftool process
}

pub fn plan_develop(items: &[DevelopItem], format: OutputFormat, existing_outputs: &BTreeSet<String>) -> DevelopPlan;
pub fn execute_develop(plan: DevelopPlan, store: &mut dyn DevelopStore, progress: &dyn Progress) -> DevelopSummary;
```

`OutputFormat` needs `Serialize`/`Deserialize`. Add them in `render.rs` with
`#[serde(tag = "kind", rename_all = "snake_case")]`: that one change to DV-2a's code is permitted.
`Skip` and `Progress` are phototools-core's (`tools::Skip`, `jobs::Progress`). `Lut` is
`media::edit::lut::Lut`.

**`plan_develop`** is pure, with no I/O. Handle items in **name order** (byte order of `name`),
whatever order they arrive in. Each item ends in exactly one of these:

| Case | Result |
|---|---|
| `look` is `None` | skipped: `"no look chosen"` |
| extension not in `phototools_core::media::raw::RAW_EXTENSIONS` (case-insensitive) | skipped: `"not a RAW file Develop can read"` |
| `exposure_ev` not finite or outside −4.0..=4.0 | skipped: `"exposure must be between −4 and +4 EV"` |
| `name` contains `/` or `\`, or is empty | skipped: `"not a file name"` |
| output (`<stem>.jpg` or `<stem>.tif`, stem as written) already in `existing_outputs` (compare lowercased: macOS is case-insensitive) | skipped: `"Developed/<output> already exists"` |
| an earlier item already planned the same output (lowercased) | skipped: `"<earlier source> already develops to Developed/<output>"` |
| otherwise | an action |

**`execute_develop`**: sequentially, one photograph at a time, never several decoded at once,
which is the memory reason Batch Grade gives. Each look is resolved once through `store.look`,
cached by its string, and its failure fails that look's photographs with the reason. For each
action:

1. check `progress.cancelled()`; if so, stop;
2. `progress.report(done, total, source)`;
3. `store.read` → `develop(bytes, &DevelopOptions { exposure_ev })` → `render(&developed, Some(&lut), plan.format)`;
4. `store.create_new(output, bytes)`. A refusal there, because something appeared since the plan,
   goes in `skipped` with its reason. Any other error, at this step or earlier, goes in
   `failures` with its reason. Either way, continue with the next photograph;
5. after a successful write, `store.carry_metadata(source, output, rendered.width, rendered.height, format)`.
   If that fails, record it in `metadata_skipped` and **keep the file**: it is developed, and the
   report says what it lacks.

Finally `store.finish()` and `progress.report(total, total, "done")`.

**2. `crates/rollchrome-core/src/tools/develop_folder.rs`**, declared in `tools/mod.rs`: the
filesystem adapter, the one place in this step with paths.

```rust
pub struct FolderStore { /* folder: PathBuf, lut_dir: PathBuf, exiftool program, writer: Option<ExifWriter> */ }
impl FolderStore {
    pub fn new(folder: PathBuf, lut_dir: PathBuf) -> Self;
    pub fn with_exiftool(folder: PathBuf, lut_dir: PathBuf, program: Option<String>) -> Self; // for the G4 test, as Batch Grade does
    pub fn output_dir(&self) -> PathBuf;   // folder.join("Developed")
}
impl DevelopStore for FolderStore { … }
```

- `existing_outputs`: the lowercased file names in `Developed/`, or an empty set if it does not
  exist yet.
- `read`: `folder.join(name)`.
- `look`: `tools::lut_library::find_lut_by_name_or_sha256(&lut_dir, look)`. Not found →
  `Error::Refused("the look '<look>' is not in the LUT library")`.
- `create_new`: create `Developed/` if missing, then `OpenOptions::new().write(true).create_new(true)`.
  `AlreadyExists` → `Error::Refused("Developed/<output> already exists")`. If writing fails after
  the file was created, remove the partial file before returning the error.
- `carry_metadata`: start the `ExifWriter` on first use (`start`, or `start_with(program, 60 s)`)
  and keep it for the batch (G4). Then:
  1. `copy_metadata_to_derivative(source, output, width, height, upright: false)`. The pixels
     are as stored (DV-1 reports orientation and does not apply it), so the original's
     `Orientation` tag stays true.
  2. One `set_tags` call with the corrections:
     - **JPEG:** `-ExifIFD:ColorSpace#=1` (sRGB).
     - **TIFF:** `-ExifIFD:ColorSpace#=65535` (Uncalibrated) and `-InteropIFD:InteropIndex=`
       (deleted). Software that believes the tag over the profile would otherwise show ProPhoto
       as sRGB.
     - **Both:** delete the tags that make a DNG a DNG, *if test 16 shows they come across*:
       `-IFD0:DNGVersion=`, `-IFD0:DNGBackwardVersion=`, plus any other DNG tag the test finds,
       listed in the Report. If `-all:all` already leaves them out, add nothing and say so.

  A comment states why each correction exists.
- `finish`: close the writer.

**3. Tauri commands** in a new `crates/rollchrome-desktop/src/commands/develop.rs`, registered in
`main.rs`. Transport only (G1); follow `commands/bulk_edit.rs`.

- `plan_develop(folder: String, items: Vec<DevelopItem>, format: OutputFormat) -> CommandResult<DevelopPlan>`:
  1. `resolve_input` on the folder (G6);
  2. **`resolve_output` on `<folder>/Developed`**, so that a publishing folder is refused exactly
     as for every other output (MV-16.7);
  3. `FolderStore::existing_outputs`, then `plan_develop`.
- `apply_develop(folder: String, items: Vec<DevelopItem>, format: OutputFormat) -> CommandResult<String>`:
  the same resolution, then `state.jobs.spawn("develop", total, …)`. The job plans again (the folder
  may have changed), executes with a `FolderStore` for that folder and
  `rollchrome_core::config::lut_dir(&config)`, and returns
  `phototools_core::tools::summarise(developed.len(), "developed", failures.len(), &[skipped, metadata_skipped].concat(), RAW_EXTENSIONS)`,
  or whatever `summarise` takes; match Batch Grade's call.
- Each command has an `_impl` function taking `&AppState`, so it can be tested, as the existing
  commands do.

No `frontend/` change: there is no screen yet, so `api.ts` gains nothing.

**4. Tests: exactly these 21.** Core unit tests 1–12 go in `tools/develop.rs`, with an in-memory
store written in the test module (a `HashMap` of names to bytes, and a set of outputs). Core
integration tests 13–19 go in a new `crates/rollchrome-core/tests/develop_folder_tests.rs`, using
the handmade DNG writer, a temp dir and exiftool. Desktop tests 20–21 go in a new
`crates/rollchrome-desktop/tests/develop_desktop.rs`.

| # | Test | Asserts |
|---|---|---|
| 1 | `each_raw_with_a_look_is_planned_to_its_own_name` | `DSCF1234.RAF` → `DSCF1234.jpg` (JPEG), `.tif` (TIFF); look and exposure carried |
| 2 | `a_photograph_given_no_look_is_left_alone` | skipped, reason `no look chosen`, no action |
| 3 | `a_name_already_developed_is_skipped_never_renamed` | `existing` holds `dscf1234.jpg`: `DSCF1234.RAF` skipped with `Developed/DSCF1234.jpg already exists`; no output name anywhere contains `_1` |
| 4 | `two_raws_sharing_a_name_develop_only_the_first` | `IMG_0001.DNG` and `IMG_0001.CR2`, given in either order: `IMG_0001.CR2` is planned (name order), `IMG_0001.DNG` is skipped naming it |
| 5 | `a_file_that_is_not_a_raw_is_skipped_with_its_reason` | `DSCF1234.JPG`, `notes.txt` |
| 6 | `an_exposure_outside_four_stops_is_skipped` | ±4.5 and NaN skipped; ±4.0 planned |
| 7 | `a_name_with_a_directory_is_skipped` | `../x.RAF`, `a/b.RAF`, empty |
| 8 | `executing_writes_each_output_once_and_carries_its_metadata` | 2 actions → 2 `create_new`, then 2 `carry_metadata` with matching names, dimensions and format; `finish` called once |
| 9 | `an_output_that_appeared_after_planning_is_skipped_not_overwritten` | the store refuses `create_new` for one: it is in `skipped`, the other is developed, and nothing is written twice |
| 10 | `a_raw_that_cannot_be_developed_fails_alone` | one item's bytes are garbage: it is in `failures` with a reason; the next is developed |
| 11 | `cancelling_stops_before_the_next_photograph` | the progress reports cancelled after the first: 1 developed, the rest neither written nor failed |
| 12 | `a_look_that_left_the_library_fails_its_photographs_with_the_reason` | `store.look` errors for one look: its photographs fail with the reason, and `look` is called once for it (cached) |
| 13 | `a_developed_jpeg_carries_the_originals_date_camera_and_position` | set DateTimeOriginal, Make, Model and a GPS position on the handmade DNG first, with `ExifWriter::set_tags` in the test's setup. After a JPEG run, `phototools_core::media::meta::read_meta` on the output returns that capture, camera and position |
| 14 | `a_developed_jpeg_says_srgb` | the source has ColorSpace = 65535 set in setup; the output's ColorSpace reads 1 |
| 15 | `a_developed_tiff_says_uncalibrated_and_has_no_interop_index` | the source has ColorSpace = 1 and InteropIndex `R98` set in setup; the TIFF's ColorSpace reads 65535, and InteropIndex is absent |
| 16 | `a_developed_tiff_is_not_taken_for_a_dng` | the TIFF has no DNGVersion (50706) and no DNGBackwardVersion (50707), read with the `tiff` crate |
| 17 | `the_tiff_keeps_its_own_profile_after_the_metadata_copy` | tag 34675 after the run equals `prophoto_icc_profile()` byte for byte |
| 18 | `outputs_go_beside_the_raws_in_developed_and_nothing_is_overwritten` | `Developed/` did not exist and now holds the output. A second run with `Developed/DSCF0002.jpg` pre-filled with known bytes leaves those bytes unchanged and reports the skip |
| 19 | `one_exiftool_process_carries_the_whole_batch` | three photographs, with an exiftool shim counting starts, exactly as Batch Grade's G4 test does: **1** start |
| 20 | `plan_develop_refuses_a_folder_outside_the_roots` | G6 refusal, through `plan_develop_impl` |
| 21 | `plan_develop_plans_a_folder_inside_the_roots` | a temp root with two handmade DNGs and a text file: two actions, one skip |

**How tests 14–16 read tags:** ColorSpace and InteropIndex through `nom-exif` (G3), added as a
**dev-dependency** of `rollchrome-core` at `phototools-core`'s version (`3.6.2`, recorded under
G8). DNG tags and the ICC profile through `tiff`, since they live in IFD0 and `nom-exif` may not
expose them. **No test reads metadata with exiftool** (G3). If `nom-exif` cannot read InteropIndex,
read it with `tiff` from the Interop IFD and say so.

Tests that need exiftool follow whatever Batch Grade's tests do when it is absent. CI installs it.

### Do not touch

- `crates/core/` (PhotoTools): no new function there; use `ExifWriter`, `RAW_EXTENSIONS`, `Skip`,
  `Progress` and `summarise` as they are.
- `media/develop/`, except the serde derive on `OutputFormat`. DV-1's and DV-2a's tests and
  thresholds stay as they are (G7).
- `tools/bulk_edit.rs`, `tools/edit.rs`, every front end, CI, `SPECIFICATION.md` (G9), `CLAUDE.md`,
  and every document except the Report section of this file.
- Out of scope (G11): a screen; `api.ts`; recursion into subfolders; sidecar `.xmp` files; reading
  `.photoedit` sidecars; parallel developing; a cache; a stale-plan hash like Batch Grade's (the job
  re-plans instead, so a stale plan cannot overwrite).

### Acceptance

- [ ] **Baseline first**, on `dv-2b` as cut: workspace **1045**, `phototools-core` **713**,
      `rollchrome-core` **222**, `phototools-desktop` **26**, `rollchrome-desktop` **13**,
      `phototools-server` **71**. If any differs, stop and report.
- [ ] **After:** `rollchrome-core` **241** (222 + 19), `rollchrome-desktop` **15** (13 + 2),
      workspace **1066** (1045 + 21), all others unchanged; 0 failed, 0 ignored. Paste the
      `test result:` lines.
- [ ] `git grep -n "Path" -- crates/rollchrome-core/src/tools/develop.rs` finds nothing (R2: the
      paths are in `develop_folder.rs` only).
- [ ] No new `std::process::Command` and no new exiftool invocation outside `ExifWriter`:
      `git diff main -- crates | grep -n "Command::new"` finds nothing (R1).
- [ ] `git diff main --stat -- crates/core crates/server frontend .github SPECIFICATION.md CLAUDE.md`
      is empty. `Cargo.lock` changes only in `rollchrome-core`'s and `rollchrome-desktop`'s
      dependency lists, if at all.
- [ ] No `unwrap()`, `expect(`, `todo!`, `unimplemented!` or `panic!` outside `#[cfg(test)]` and
      `tests/` (G10).
- [ ] The Report says: whether DNG tags came across (test 16) and which corrections were needed;
      how InteropIndex was read; and anything exiftool did that surprised you.

### Gates

```sh
cargo fmt --all --check                                  # no output
cargo clippy --workspace --all-targets -- -D warnings    # no warnings
cargo build --workspace                                  # ok
cargo test --workspace                                   # 1066 passed
cargo test -p phototools-core                            # 713 passed — G2
cargo test -p rollchrome-core                            # 241 passed — G2
cargo test -p phototools-desktop                         # 26 passed
cargo test -p rollchrome-desktop                         # 15 passed
npm --prefix frontend/rollchrome run typecheck           # untouched, still clean
```

If `cargo` is not on `PATH`, run `. "$HOME/.cargo/env"` first.

### Report back

In **Report** below: what you did; any deviation, and why; the actual `test result:` lines of every
gate; everything the last Acceptance item lists; and what you could not verify, said plainly. Do
not tick the acceptance boxes above; the review does that.

---

## Report

*Written by the implementer.*

---

## Review

*Written by the architect, after re-running the gates on the branch.*

**Approved** — or **Changes requested:**

1. <specific, actionable, one change per item>
