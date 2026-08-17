## Phase 10 — RAW to JPEG

**Status:** complete with gates. Rung 1 is done and tested and is the rung that will handle nearly
every real file; rung 2 only exists on macOS, and the *quality* of every rung needs a person.

### Delivered

- **Task 1 — embedded preview extraction, portable.** `media::raw` walks the TIFF/IFD structure of a
  RAW file, collects every `(offset, length)` pair that might be a JPEG, and slices out the largest.

  It follows all three places makers put previews: the `JPEGInterchangeFormat` pointer, single-strip
  JPEG data in a SubIFD (which is what DNG does), and the IFD chain, since IFD1 conventionally holds
  the thumbnail. **The bytes come out unchanged** — F14 calls the preview "effectively free to
  extract", and re-encoding the camera's own render would throw away the thing that makes rung 1 the
  preferred result.

  The walker is bounded against malformed and hostile files: a depth limit, a visited-offset set so
  an IFD pointing at itself terminates, and an entry count checked against the file's length before
  anything is read.

- **Task 2 — macOS ImageIO**, behind `#[cfg(target_os = "macos")]`, through `sips`. Away from macOS
  the rung reports "not applicable" rather than failing, which is precisely why rung 3 exists. A
  camera model ImageIO does not know is also a fallthrough, not an error — support is per-model and
  tied to the OS version.

- **Task 3 — `rawler` fallback**, portable, and the only rung available on the Linux server. Its
  `decode_file` is wrapped in `catch_unwind`, because it panics on some malformed input and a corrupt
  file on a card must not take the process down.

- **Task 4 — the ladder, with fallthrough that reports.** A rung returning `Ok(None)` means "not
  applicable"; an `Err` means it tried and failed. **Neither stops the ladder**, and both are
  collected, so a file nothing can handle produces `could not derive a JPEG from bogus.nef: embedded
  preview: not applicable; macOS ImageIO: not applicable; rawler: not applicable` rather than
  "conversion failed".

- **Task 5 — the output passes through F12 and F13.** `derive_batch` resizes a derivative that
  exceeds the megapixel ceiling and runs F13's quality ladder against the byte cap, because F12 says
  both thresholds "apply to both the JPEG path and the RAW-derived path". A derivative **already**
  within the ceiling is written unchanged rather than re-encoded — a re-encode there would cost
  quality for nothing.

  Metadata is copied from the RAW through **one `exiftool` process for the whole batch** (G4), and
  each result is read back before `metadata_verified` is set, per specification §9.2 invariant 6.

- **One server route and one Tauri command**, both jobs, both resolving through `Config::resolve`
  (G6).

### Acceptance

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — **352 passed, 0 failed**
- [x] `cargo test -p phototools-core` (G2 isolation) — **305 passed, 0 failed**
- [x] **Ladder order asserted: with a preview present, steps 2 and 3 are not reached.** Twice, two
      ways — see below.
- [x] **With no preview, the fallback runs and produces a valid JPEG.** The fall-through itself is
      asserted; producing a JPEG from a stub that carries no actual sensor data is not something any
      decoder can do, so this is verified as far as it can be here. See deviation 3.
- [x] **Capture date, camera and lens survive into the output.** The date is asserted to the second;
      the lens is read back through `exiftool` rather than through our own reader.
- [x] **Append to `docs/manual-verification.md`.** Five entries.

### How the ordering claim was made real

"Steps 2 and 3 are not reached" is a claim about what did *not* happen, and a test that only checks
which rung won cannot support it — an implementation that ran all three and preferred the first would
pass. So it is asserted twice:

1. **By counting calls.** The ladder is built over a `Rung` trait, and a unit test runs it with rungs
   that increment a counter. The assertion is `calls == 0` for rungs 2 and 3, which is the claim
   itself rather than a proxy for it.
2. **Against the real ladder.** An integration test converts a fixture that neither `sips` nor
   `rawler` could read, and gets a JPEG back. That only happens if rung 1 answered.

### Measurements

None specified for this phase. Extraction is a slice, so rung 1 is I/O-bound and its cost is reading
the file; the 12-file batch test completes in about 21 seconds in a debug build, dominated by
`exiftool` and by fixture generation rather than by the ladder.

### Gates

- **A Mac.** Rung 2 is compiled out on Linux and cannot be exercised at all here.
- **Real RAW files from each camera body.** Five items in `docs/manual-verification.md`, of which two
  matter most: whether the embedded preview on your bodies is genuinely full-resolution rather than
  screen-sized, and whether rung 3's colour is acceptable when it is reached at all.

### Deviations

1. **`sips` rather than `objc2` for rung 2.** F14 names both as acceptable. `sips` is a one-line
   subprocess; `objc2` bindings to ImageIO are a substantial amount of unsafe FFI that **cannot be
   exercised on this machine at all**. Neither can `sips`, but far less of it can be silently wrong.

   **This sits against specification §2.6**, which calls `exiftool` "the one permitted external
   binary". F14 explicitly permits `sips`, and is both more specific and later, so it wins — but the
   two statements do contradict each other and that is worth recording. If §2.6 is meant to be
   absolute, rung 2 has to become `objc2` bindings, which is a real piece of work and untestable
   until it runs on a Mac.

2. **`rawler` added to `crates/core`.** Named in specification §2.6. Recorded per G8. It is a large
   dependency tree — it brings in `jxl-oxide`, `libflate`, `multiversion` and others — and it roughly
   doubled the workspace's build footprint, which is worth knowing before the Docker image in Phase
   14.

3. **The "no preview" acceptance test asserts the fall-through, not a JPEG.** A RAW stub with no
   preview also has no sensor data — synthesising a real mosaic that `rawler` would decode means
   implementing a camera's raw format, which is a much larger piece of work than the rung it would
   test. What is asserted is that rung 1 correctly declines and the ladder moves on. **Rung 3 has
   never produced an image in a test**, and that is the honest statement; it is on the verification
   list.

4. **`derive_batch_with` exists alongside `derive_batch`** so a test can point at an `exiftool` shim,
   mirroring `ExifWriter::start_with`. The first version of the G4 test overrode `PATH` instead and
   **failed** — a test harness runs its tests as threads of one process, so another test's own
   `exiftool` call was counted by the shim. The current test passes the program explicitly and has no
   such race.

5. **CR3 is declined, deliberately.** `RAW_EXTENSIONS` omits it and a test asserts the omission. F14
   puts it out of scope because it is ISO-BMFF rather than TIFF-based; silently mis-handling it would
   be worse than saying no.

6. **Two new fixture generators**, `raw_with_metadata` and `raw_with_thumbnail_and_preview`. The
   existing `raw_stub_with_preview` carries a single preview and no dates, which cannot demonstrate
   either "the largest wins" or "the metadata was copied". The new metadata fixture's preview
   deliberately carries **no** EXIF, so a date found in the output can only have come from the copy
   step.

### Added to manual-verification.md

Five Phase 10 entries. The one the build plan names — colour quality per camera body — plus four that
came out of building it, the most important being whether your cameras' embedded previews are
full-resolution. A screen-sized preview would pass every test in this phase while producing
derivatives nobody would want to keep.

### Notes for the next phase

- **Phase 11 (F16) has everything it needs from here.** `DerivedShot` carries the output path,
  dimensions and byte count; what it does not yet carry is the derivative's content hash, which the
  pre-flight deduplication will want. Adding it is a line in `derive_one`.
- **`ingest::derivation::worker` is now the odd one out.** It predates this phase, produces 2000 px
  proxies, and nothing calls it. Phase 11 should either fold it into the staging path or delete it —
  it is the last piece of the original sketch still standing.
- **Rung 3's quality question affects where derivation should run.** If `rawler`'s output is not good
  enough to publish, then RAW-only shots must be derived on the Mac and the server route becomes a
  fallback nobody should use. That is a decision for after the manual check, not before it.
