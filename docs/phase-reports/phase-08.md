## Phase 8 — Card detection and scan

**Status:** complete with gates. F11 and the whole processing pipeline are done and tested on Linux;
F10's platform half — `/Volumes`, FSEvents, the macOS notification — needs a Mac and a card.

### Delivered

- **Task 1 — F11 in `core`, on simulated card mode.** `ingest::card::Card` is the whole of build
  plan §6.3: `Card::at(path)` accepts any directory, and **nothing below it can tell whether a real
  card was mounted**. `Origin` records which it was and *nothing branches on it* — a branch there
  would be exactly the coupling §6.3 forbids.

  `Card::media_root()` returns `DCIM` when there is one and the root otherwise, so §6.3's second
  reason — re-running ingest over a folder of already-copied files — works without a special case.

  The scan itself walks in parallel with `rayon` and records path, size, dimensions, camera, capture
  date and a content hash per file. It is sorted before it is returned, so a scan is reproducible
  rather than dependent on directory order.

- **Task 2 — card fingerprinting.** `Fingerprint::generate` previously ignored its path and returned
  a constant, so **every card in the ledger collapsed into one row**. It now hashes the sorted
  `(relative path, size, mtime)` tuples exactly as F10 specifies: sorted because directory order is
  not stable, relative because a card mounts at different absolute paths, size and mtime rather than
  content because hashing 64 GB to decide whether a card is new would cost more than the scan.

  Host junk (`.DS_Store`, AppleDouble files) is excluded, so a card that Finder merely looked at
  still fingerprints the same.

- **Task 3 — F10 detection in the desktop binary.** `crates/desktop/src/detection.rs`: an FSEvents
  watch on `/Volumes`, a debounce, the `DCIM` test, and a native notification. **Thin, as the plan
  requires** — it produces a path and hands it to `core`. The one piece of judgement it holds is the
  debounce, and that is a separate testable type rather than a sleep buried in a callback.

- **Task 4 — staging with hash verification (G5).** `stage_asset` copies through a `.partial` name
  and renames, so an interrupted copy never leaves a file that looks complete; then re-hashes the
  destination and compares it to the hash the scan computed. A mismatch deletes the copy rather than
  leaving something a later pass could mistake for good. Staged names are content hashes, so two
  cards that both hold `IMG_0001.JPG` do not collide and identical content deliberately coalesces.

- **Four Tauri commands** — `summarise_card`, `scan_card`, `stage_card`, `read_card` — each
  resolving its path through `Config::resolve` (G6) and delegating. Scanning and staging are jobs,
  so nothing blocks (F17).

### Acceptance

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — **283 passed, 0 failed**
- [x] `cargo test -p phototools-core` (G2 isolation) — **236 passed, 0 failed**
- [x] **`card_tree` with 100 shots — 60 RAW+JPEG, 30 JPEG-only, 10 RAW-only — groups into exactly
      100 shots with correct candidates.** 160 assets, 90 JPEG candidates, 10 marked for F14.
- [x] **No full decode occurs during scan.** See below.
- [x] **A read-only fixture directory scans successfully and is byte-identical afterwards** (G5).
- [x] **400-shot scan within 10 s.** Measured, with a caveat that matters — see Measurements.
- [~] **F10 end to end.** Every decision it makes is tested; that macOS raises the notification and
      that FSEvents reports `/Volumes` are not, and cannot be here.

### How the no-decode assertion was made real

An assertion that a scan "did not decode" is easy to write and easy to write uselessly. This one
uses a fixture, `jpeg_with_unreadable_pixels`, whose EXIF is intact and whose **frame header is
removed entirely** — so no decoder can determine even the image's size, while any metadata reader
finds the full tag set. The test first asserts `media::decode` fails on it, because without that the
test would pass on a file that simply happened to decode, and then asserts the scan reports
6000×4000 with no problem recorded.

The first attempt corrupted the entropy-coded scan data with a run of `0xFF` and **the test passed
when it should not have**: `0xFF` reads as JPEG fill, and decoders tolerate a truncated scan by
returning what they managed to reconstruct. That is recorded in the fixture's doc comment so nobody
re-derives it.

### Measurements

| | |
|---|---|
| 400-shot scan, fixture card | **44.6 ms** |
| Hash throughput, 100 × 8 MB | **684 MB/s** |
| Extrapolated 400 × 8 MB card | **≈ 4.7 s** |

**The 44.6 ms figure is not the real one and should not be quoted as one.** The fixture's JPEGs are
64×48, so the measurement shows the walk, grouping and metadata reads are not the bottleneck —
nothing more. On a real card the cost is dominated by hashing every byte, which F11 requires.

At the measured 684 MB/s on local disk, a 400-shot card of 8 MB frames is about 4.7 s, inside §9.1's
10 s budget. **On a real card reader it will not be.** A UHS-I reader sustains roughly 90 MB/s, which
puts the same card at around 35 s. Per build plan §11 this is reported rather than quietly relaxed:
the target is met on the machine the test runs on, and is expected to be missed on the hardware the
feature is for. **Task 4 of Phase 11 (pre-flight deduplication) is the fix** — sending hashes before
bytes means a card that has already been ingested is not re-read at all — and the honest measurement
is the one to take on a Mac with a real card, which is now in `docs/manual-verification.md`.

### A specification question, and what I did about it

**F10's card identity cannot support F10's own notification text, and I had to choose.** F10 says a
card is identified by "its volume label plus a fingerprint computed over the sorted `(relative path,
size, modification time)` tuples of its contents". It also says the notification reads
`EOS_DIGITAL — 412 new shots. Review?`.

Those conflict. The fingerprint covers the card's contents, so **it changes the moment another frame
is shot** — which is the normal case for a card between insertions. Under a literal reading, a card
returning with 40 new frames is a card never seen before, and all 412 shots are "new" every time.
The identity scheme recognises only a card reinserted *completely unchanged*, which is the case
where there is nothing to announce.

I kept the specified identity and added a second key rather than replacing anything:

- **`card_id` is exactly what F10 specifies** — label plus content fingerprint — and remains the
  `cards` table's key. Each row is one observed state of a card, which is a useful record.
- **Shots are keyed by the card's label**, which survives shooting more frames. A new `card_scope`
  column (migration 3) carries it, and `shot_stems` looks up by it.

So a reinserted card with 40 new frames announces 40, and a reinserted card with nothing new
announces nothing at all — which is what recognising cards is *for*, and what the notification text
implies. **Per build plan §11 I am reporting rather than assuming**: if the intended reading is the
literal one, revert to keying shots on `card_id` and accept that the count always equals the whole
card. My recommendation is the implemented behaviour, because the alternative makes the notification
useless in the common case.

The weakness of keying on the label: a card that is reformatted and reuses `IMG_0001` will look
already-seen. Phase 11's content-hash deduplication is the real answer, and this is a detection-time
estimate that costs no file reads — it is deliberately not the thing that decides what gets
published.

### Gates

- **A Mac with a card reader.** Seven items now in `docs/manual-verification.md`, including two that
  could silently misbehave rather than fail loudly: whether 1.5 s of debounce is enough for a real
  reader, and whether `/Volumes` is watchable at all.
- **`tauri-plugin-notification` permission.** macOS asks the user; a denied prompt means detection
  works and says nothing.

### Deviations

1. **Two new dependencies, both named in specification §2.6 or implied by it.** `notify` (§2.6 names
   it for filesystem watching) and `tauri-plugin-notification` (F10 requires a native notification;
   §2.6 does not name a mechanism). Recorded per G8.
2. **`crates/desktop/capabilities/default.json` did not exist and now does.** Tauri v2 gates the
   webview's access behind capabilities, and there were none — which is a latent Phase 7 bug rather
   than anything Phase 8 introduced. It grants `core:default` and `notification:default`. **Only a
   Mac can confirm this is right**, and it is on the verification list.
3. **The old `ingest` API is gone**, not deprecated. `Scanner::scan`, `group_assets`, `CandidateShot`,
   `CandidateAsset` and `ingest_card` are replaced by `scan_files`, `group_into_shots`, `Shot`,
   `ScannedAsset`, `scan_card` and `record_scan`. The old shapes carried no metadata, no asset kind
   and no candidate, so F11 could not have been built on them.
4. **Ledger migration 3 adds `shots.card_scope`.** Forward-only and additive, per the migration
   rules. See the specification question above for why it exists.
5. **`JobRunner::ledger()` added to core.** Detection reads the ledger on every mount while jobs may
   be writing to it; two connections to one SQLite file would contend for the write lock, so the
   handle is shared rather than reopened.
6. **`Ledger::count(table)` interpolates a table name.** SQLite cannot bind an identifier. It asserts
   the argument is a bare identifier, and is only ever called with literals from this crate.

### Added to manual-verification.md

Seven Phase 8 entries, replacing the single line that said "requires a physical SD card". Two of them
are for behaviour that would fail *silently* rather than loudly — a debounce too short for a real
reader, and `/Volumes` not being watchable — which are the ones worth doing first.

### Notes for the next phase

- **Phase 9 has everything it needs.** `ScannedAsset` carries `capture`, `megapixels()` and `bytes`,
  which are the three checks; `Shot::capture()` is the per-shot date the batch-median detection works
  over.
- **`dimensions_unknown()` is a real state, not a rounding of zero.** A file whose metadata carries
  no dimensions records 0×0 deliberately, because resolving it would mean decoding. Phase 9's
  resolution check must treat it as its own outcome rather than as a 0 MP failure.
- **The `derived` table and `WorkerPool` already exist** from an earlier phase and now compile
  against the new types, but the derivation path is not part of Phase 8's acceptance and has not been
  re-verified against real RAW input. Phase 10 owns it.
- **Phase 11 is where the performance caveat gets resolved.** Pre-flight deduplication means an
  already-ingested card is never re-read, which matters more than any constant-factor improvement to
  the hash.
