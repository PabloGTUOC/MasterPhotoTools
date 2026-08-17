## Phase 1 — Core foundations

**Status:** complete

Phase 0 closed green on CI ([run #2](https://github.com/PabloGTUOC/MasterPhotoTools/actions/runs/32028486675)),
so this phase was eligible to start.

### Delivered

Three of the six tasks were already satisfied and were verified rather than rewritten. Three had
real gaps.

- **Task 1 — `config`.** `Config` and `Thresholds` existed with the correct default values, but
  `from_env` ignored the thresholds entirely: it always returned `Thresholds::default()`, so the
  documented defaults were the *only* obtainable values. Added `Thresholds::from_env`, reading
  `MAX_AGE_DAYS`, `MAX_MEGAPIXELS` and `MAX_OUTPUT_BYTES`, with the defaults exported as named
  constants and documented in a doc-comment table. A variable that is set but unparseable is now a
  startup **error**, not a silent fallback — a typo in `MAX_MEGAPIXELS` must not quietly restore
  10 MP and let oversized frames through (§9.2 invariant 6).

  Also hardened root loading. `from_env` previously did
  `canonicalize().unwrap_or_else(|_| PathBuf::from(s))`, keeping the raw string when
  canonicalisation failed. A root that is relative, or is itself a symlink, makes `resolve`'s prefix
  check meaningless. Unresolvable roots are now rejected at load.

- **Task 2 — `Config::resolve` (G6).** Already correct, with all five required cases covered.
  Verified, not changed. Added one test: a sibling directory whose name shares a prefix with a root
  (`/photos-private` against root `/photos`) must be rejected. It already is — `Path::starts_with`
  is component-wise — but the test pins that against a future refactor to string prefixes, which is
  the classic way this check gets silently broken.

- **Task 3 — `Error`.** Already a single crate-wide `thiserror` enum, and `anyhow` is not a
  dependency of `core`. Verified, unchanged.

- **Task 4 — `ledger`.** Two real gaps.

  *No migration system existed.* The schema was one `CREATE TABLE IF NOT EXISTS` batch with no
  version tracking, so there was no way to evolve it. Replaced with a forward-only `MIGRATIONS`
  array gated on `PRAGMA user_version`: each entry applies once, in order. Migration 1 is the §7
  schema verbatim; migration 2 adds indexes on `assets.sha256` (the F16 deduplication key),
  `assets.shot_id`, `shots.card_id` and `jobs.state`.

  *The round-trip test covered 2 of 10 tables.* `users` and `settings` were genuinely round-tripped;
  the other eight had a bare `INSERT INTO t (id) VALUES (...)` that was never read back — it proved
  the table existed and nothing more. Replaced with a test that writes and reads back **every column
  of all ten tables**. Making that possible required additive API: `add_user`, `upsert_card`
  (carrying `volume_label`), `set_shot_candidate`, `upsert_asset` (all ten columns — `kind`,
  `capture_datetime`, `width`, `height`, `camera` were previously unwritable), `add_check`,
  `set_setting`/`get_setting`. Existing signatures were kept as wrappers so no caller broke.

- **Task 5 — `jobs`.** The `Progress` trait and an in-memory implementation existed. **Job state
  persistence did not exist at all** — the `jobs` table had no writer, and the "survives a restart"
  test was a serde round-trip in memory, which proves nothing about restart.

  Added `JobStatus` (`pending`/`running`/`completed`/`failed`/`interrupted`) and `Job`, the
  persisted row, plus ledger operations: `insert_job`, `update_job_progress`, `finish_job`,
  `get_job`, `jobs_with_status`, and `recover_interrupted_jobs`. The last is the F17 requirement:
  called at startup, it takes every job still `pending` or `running` — necessarily orphaned by a
  dead process — marks it `interrupted` with an explanatory error, and returns it. Progress is
  preserved on the record so a resumable job kind can pick up where it stopped. Nothing disappears
  silently.

- **Task 6 — §7 contracts.** All present and verified: `Config`/`Config::resolve`, `MediaMeta`/
  `read_meta`/`ExifWriter`, `Progress`, `ToolResult`/`Outcome`, `Plan`/`Tool`. No new `todo!()`
  bodies were added; the one that remains (`ExifWriter::shift_dates`) is Phase 2's to remove.

Also updated `.env.example` with the three new threshold variables, keeping Phase 0 task 5 true.

### Acceptance

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — 34 passed, 0 failed
- [x] `cargo test -p phototools-core` (G2 isolation) — 28 passed, 0 failed
- [x] **`Config::resolve` covers all five required cases** — inside a root, outside, `..` traversal,
      absolute path outside, symlink pointing outside. All five reject correctly.
- [x] **Ledger round-trip test per table** — `every_table_round_trips` covers all ten, every column.
- [x] **A job written, the process simulated as restarted, the job recovered** —
      `a_job_survives_a_restart_and_is_recovered`. The restart is simulated by dropping the `Ledger`,
      closing the SQLite connection entirely, and opening a fresh one from the same file; nothing is
      carried in memory. Asserts the record survived with progress intact, that recovery marks it
      interrupted with an error and a `finished_at`, that a job which had completed cleanly is left
      alone, and that recovery is idempotent.

Core unit tests went from 3 to 15.

### Measurements

Phase 1 specifies no benchmarks.

### Gates

None. This phase has no external dependency.

### Deviations

1. **`JobState` was kept alongside the new `Job`.** `JobState` is the SSE progress DTO and is
   imported by `crates/server/src/api.rs`. Changing it would have meant editing the server, which
   Phase 1's "no module outside `config`/`ledger`/`jobs` has been touched" forbids. `Job` is the
   persisted row; `JobState` remains the wire snapshot. Phase 5 should collapse the two when it
   builds the real job runner — noted in a doc comment on both types.
2. **Ledger API additions are additive rather than replacing the old signatures.** `add_card` and
   `add_asset` are now thin wrappers over `upsert_card`/`upsert_asset`, because changing their
   signatures would have required editing `ingest`, again outside this phase's allowed set.

The "Done when" constraint holds exactly: `git diff --stat` for this phase shows three files —
`crates/core/src/config.rs`, `crates/core/src/jobs.rs`, `crates/core/src/ledger.rs`.

### Added to manual-verification.md

Nothing. Phase 1 produces no output requiring human judgement.

### Notes for the next phase

- **Phase 2 must remove the last `todo!()`**: `ExifWriter::shift_dates` at
  `crates/core/src/media/meta.rs:165`. Task 6 permitted `todo!()` in Phase 1 only.
- **G4 is violated and Phase 2 task 3 owns the fix.** `ingest/derivation/worker.rs` calls
  `ExifWriter::start()` inside the per-job rayon closure — one `exiftool` process per file. The
  Phase 2 acceptance test ("writing 50 files spawns exactly one process") will fail until this is
  hoisted, and note that the *existing* `test_exif_writer_single_process` does not actually count
  processes despite its name.
- **`Config::resolve` requires the path to already exist**, because `canonicalize` fails otherwise.
  Every tool that writes to a new `out_dir` (F4, F6, F7, F8) will need a create-path variant that
  canonicalises the parent and checks that. Left undone deliberately — inventing it here would be
  Phase 3/4 scope (G11) — but it is a known blocker for wiring G6 into those tools.
- **The jobs plumbing is ready for Phase 5.** `recover_interrupted_jobs` should be called once at
  server startup, and `update_job_progress` is what `ApiProgress::report` should write to instead of
  its current fire-and-forget broadcast.
- `assets.sha256` now has an index, so the F16 deduplication lookup will not table-scan.
