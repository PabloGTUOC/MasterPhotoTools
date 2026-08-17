# Gap analysis — repository vs. SPECIFICATION.md / BUILDPLAN.md / README.md

**Date:** 2026-08-17
**Commit audited:** `82533b7` ("Gemini first temp") — the only substantive commit in the repository.
**Method:** every source file read; `cargo build`, `cargo test`, `cargo clippy`, `cargo fmt` run locally; CI history checked on GitHub.

---

## 1. Headline

The repository is a **broad but shallow first pass**. Every phase of the build plan has been touched
at once, and none has been finished to its Definition of Done. The plan's core instruction — work
phases in order, one at a time, each gated by acceptance criteria — was not followed, so there is no
phase in the repository that can be called done.

| Measure | Status |
|---|---|
| CI | **Red.** The only run ever ([#1](https://github.com/PabloGTUOC/MasterPhotoTools/actions/runs/32025613711)) failed at the first step, `cargo fmt --all --check`. Every later step was skipped. |
| `cargo fmt --all --check` | **Fails** — 4 files need reformatting. |
| `cargo clippy --workspace --all-targets -- -D warnings` | **Fails** — 3 `redundant_closure` errors in `crates/core/src/config.rs`. |
| `cargo build --workspace` | **Fails on Linux** — `phototools-desktop` needs GTK/WebKit system libraries that CI never installs. |
| `cargo test -p phototools-core` | **Fails** — 3 of 16 tests fail (all needing `exiftool`, which CI never installs). |
| Requirements F1–F18 | 1 complete, 2 substantially complete, 7 partial, 8 missing or mocked. |
| Phases 0–14 | 0 complete, 8 partially started, 6 not started, 1 (Web) actively broken. |

### The three findings that matter most

1. **Nothing in the repository has ever passed its own gate.** CI has never been green. Two of the
   five commands the build plan calls non-negotiable fail on a clean checkout, and a third
   (`cargo build --workspace`) cannot pass on the Linux runner as configured.
2. **G6 — path confinement — is not wired into the API.** `Config::resolve` exists and is well
   tested, but no HTTP handler calls it. `crates/server/src/api.rs:67` even says so in a comment.
   As it stands, `POST /api/tools/f1/apply` will rewrite EXIF metadata on any path the server
   process can reach. This is the single most security-relevant gap in the system, and the build
   plan names it as such.
3. **G4 is violated in the derivation worker.** `WorkerPool::process_single`
   (`crates/core/src/derivation/worker.rs:93`) calls `ExifWriter::start()` *inside* a per-job
   function running under `rayon::into_par_iter` — one `exiftool` process per file, which is the
   exact thing the specification prohibits (§2.6) and the ground rules restate (G4).

---

## 2. Process gaps

The build plan's §0 and §4.1 define how work is to be delivered. None of it happened.

| Required | Actual |
|---|---|
| One branch per phase, `phase/NN-short-name` | All work on `main`, in one commit. |
| One PR per phase, titled `Phase NN — <name>` | No PRs. |
| A phase report (§10 template) posted per phase | None exist. |
| Phases worked **in order**, each gated by the previous phase's DoD | All 15 phases touched simultaneously; none finished. |
| G8 — new dependencies recorded with a reason in the phase report | 12 dependencies added beyond specification §2.6, unrecorded (below). |

Because there are no phase reports, the deviations, gates and measurements the plan asks for
(benchmark figures, macOS gating, blockers) are simply absent from the record.

---

## 3. Ground-rule violations (BUILDPLAN §1)

| Rule | Status | Evidence |
|---|---|---|
| **G1** No functionality in a binary crate | ⚠ Borderline | `crates/server/src/auth.rs` holds all Firebase verification logic. Phase 5 does place it in the server, so this is defensible — but it is ~180 lines of reusable logic sitting outside `core`. |
| **G2** `core` compiles and tests in isolation | ✗ Fails | `cargo test -p phototools-core` — 3 failures. |
| **G3** Never invoke `exiftool` to read | ✓ Holds | `read_meta` uses `nom-exif` in-process. |
| **G4** Never one `exiftool` per file | ✗ **Violated** | `crates/core/src/derivation/worker.rs:93` — `ExifWriter::start()` inside the parallel per-job closure. |
| **G5** Never write to a source card | ⚠ Not violated, not implemented | Nothing writes to the card, but the required copy-to-staging-and-verify step (spec F11 invariant, Phase 8 task 4) does not exist. |
| **G6** Every API path canonicalised against roots | ✗ **Violated** | No handler in `crates/server/src/api.rs` calls `Config::resolve`. |
| **G7** Never weaken a test | ✓ Holds | No `#[ignore]`, no deleted assertions. |
| **G8** No dependencies beyond spec §2.6 without a recorded reason | ✗ Violated | See §3.1. |
| **G9** Do not edit `SPECIFICATION.md` | ✓ Holds | Untouched. |
| **G10** No `todo!()` on a shipped path | ✗ **Violated** | `crates/core/src/media/meta.rs:165` — `ExifWriter::shift_dates` is `todo!("Shift dates logic")`. Eight server handlers return `501 NOT_IMPLEMENTED`. Four `publish` functions return hard-coded mock strings. |
| **G11** Do not invent scope | ✗ Violated | `crates/core/src/derivation/` is a module the specification's §2.5 layout does not contain. |

### 3.1 Dependency drift

**Added, not in specification §2.6, not recorded:**
`walkdir`, `sha2`, `regex`, `tiff`, `dirs`, `serde_json`, `lazy_static`, `ring`, `uuid`,
`tower-http`, `futures`, `tokio-stream`, `tracing`, `tracing-subscriber`, `thiserror` (v2 in server
vs v1 in core).

**Declared but never used** — dead weight in the build: `zune-jpeg`, `tiff` (the F8 comment admits
it was avoided), `ring`, `uuid`.

**Required by §2.6, entirely absent:** `rawler` (RAW decode), `notify` (filesystem watching),
`keyring` (macOS Keychain). All three gate requirements that are themselves missing (F14, F10,
F18-desktop).

---

## 4. Requirement coverage (F1–F18)

Legend: ✅ complete · 🟡 partial · ⭕ missing or mocked

### 🟡 F1 — Date scan and repair

*Present:* scan/classify walk, `OK`/`Mismatch`/`MissingMetadata`, all four repair modes reachable
through `plan`/`apply`, platform-branched filesystem time.

*Gaps:*
- **Six tags, not seven.** `get_best_date` (`media/meta.rs:101`) collapses the namespaced tags into
  six bare names. `EXIF:CreateDate` and `QuickTime:CreateDate` are one entry; `QuickTime:CreationDate`
  and `Keys:CreationDate` are matched by bare name only. The spec's priority order cannot be
  reproduced, and Phase 3's acceptance test ("for each of the seven tags…") cannot pass.
- **QuickTime-as-UTC is not implemented.** The spec calls this out specifically to prevent
  double-shifting.
- **Timezone handling is wrong for negative offsets.** `parse_date` (`meta.rs:96`) does
  `clean.replace('-', ":")` to accept `YYYY-MM-DD`. A value like `2024:05:01 12:00:00-05:00` becomes
  `2024:05:01 12:00:00:05:00`, which fails to parse rather than dropping the suffix.
- **Only `-AllDates` is written.** The spec requires `DateTimeOriginal`, `CreateDate`, `ModifyDate`
  *and* `AllDates` for images; `CreateDate`, `ModifyDate`, `MediaCreateDate`, `TrackCreateDate` for
  video; and `FileCreateDate`/`FileModifyDate` for both. `write_dates` (`meta.rs:150`) writes one tag.
- **Filesystem timestamps are never set** by `DateRepairTool::apply`.
- **`shift` mode does not use the `exiftool` shift path.** `ExifWriter::shift_dates` is `todo!()`;
  `RepairMode::Shift(i64)` carries seconds, which cannot express the spec's `+1:0:0 0:0:0`
  month/year deltas.
- **Nothing is verified after writing**, so §9.2 invariant 6 ("reports only what it has verified")
  cannot hold. `Summary` is `()`.
- No `rayon` parallelism, so the §9.1 target (500 files < 5 s) is untested and unlikely.

### 🟡 F2 — Google Takeout sidecar dates

*Present:* JSON parse, `photoTakenTime.timestamp`, exact match, a truncation attempt, one
`(1)`-suffix variant.

*Gaps:* no recursive/folder mode — `find_takeout_date` handles a single file only, and the spec
requires both. The truncation rule is a hard-coded guess at 46 characters. Only one of the several
`(1)`-placement permutations is covered by a test.

### 🟡 F3 — Batch rename

The most complete of the archive tools. Prefix assembly, sanitising, zero-padding, two-phase
plan/apply, and collision protection all match the spec, with a passing collision test.

*Gaps:*
- **`capture` ordering never reads metadata.** `f3_rename.rs:91` sorts by `get_fs_time` only. The
  spec orders by best metadata datetime *first*, falling back to modification time.
- Skipped files still consume a sequence index, leaving gaps in the numbering.

### 🟡 F4 — Half-frame film split

The largest single divergence from the specification. The spec defines a four-stage procedure with
seven named parameters; the implementation is a single darkest-column search.

*Missing entirely:* lab-border removal (stage 1), the `±window` refinement (stage 2), residual
dark-band trimming (stage 4), the frame-ratio guard, the landscape-half rotation, the
"more than 10% taller → trim from the bottom only" rule, and preview mode.

*Wrong:* the search margin is hard-coded to 0.35–0.65 instead of the specified `margin` = 0.20;
output is `{base}_1`/`{base}_2` preserving the source extension, not `{base}_A.jpg`/`{base}_B.jpg`;
saved via `DynamicImage::save` (default quality) instead of JPEG 95 with no chroma subsampling.

*Absent:* all seven configurable parameters — `threshold_dark`, `threshold_white`, `border_tol`,
`max_crop_pct`, `margin`, `window`, `ratio`.

*Also:* `plan()` calls `create_dir_all` (`f4_split.rs:33`), breaking the §7 contract that `plan`
never touches disk. F6, F7 and F8 do the same.

### 🟡 F5 — Contact sheet

*Present:* grid layout, aspect-preserving centred thumbnails, a red crossed box for unreadable
files that does not abort the sheet.

*Gaps:* no captions at all — and so no 30 px label strip, no `max(10, cell_size × 0.04)` font size,
no 28-character truncation. **The height formula is therefore wrong**: the code omits the
`label_height` term the spec's formula includes. No background colour option (hard-coded white,
so the inverting caption colour is moot), no sort-by option, EXIF orientation not honoured, and no
quality-95 encode.

### 🟡 F6 — Transform

*Gaps:* EXIF orientation is not applied first (`decode` does not read orientation at all). Rotation
is limited to 90/180/270 — any other angle is **silently ignored** rather than rotating with canvas
expansion. No `optimise` flag. `.heic`/`.heif` are accepted by the spec but the `image` crate cannot
decode them, and nothing filters or reports this.

### 🟡 F7 — Print border

*Gaps:* dark-edge trimming (stage 1) is missing entirely. **Canvas selection does not match the
spec** — it takes `long_edge`/`short_edge` as caller parameters, so no single pair can produce both
3000×3750 portrait and 3000×2400 landscape; Phase 4's acceptance test would fail. The margin is 5%
rather than a minimum 50 px. The corner radius is a caller parameter, not 2% of the image's short
side, and is not anti-aliased at 4× — the code comment concedes the anti-aliasing "could go here".
Saved at default quality rather than 95 with no subsampling.

### 🟡 F8 — TIFF to JPEG

**Multi-page TIFF is not implemented** — the core of the requirement. The code comment at
`f8_tiff.rs:91` states this outright and falls back to page 1 via `image::open`. The `tiff` crate is
a declared dependency and is never used. `{base}_p001.jpg` naming therefore does not exist. Alpha
flattening and the 2048 px cap are correct. Missing: 4:2:0 chroma subsampling, progressive encoding,
optimisation.

### ✅ F9 — Library browser

Matches the specification: confined to roots via `Config::resolve`, directories first then
case-insensitive alphabetical, `..` except at a root, unreadable entries skipped rather than fatal.
The one complete requirement in the repository.

### ⭕ F10 — Card detection

Not started. No `notify` dependency, no `/Volumes` watcher, no debounce, no `DCIM` test, no native
notification. Card fingerprinting exists in name only — `Fingerprint::generate`
(`ingest/fingerprint.rs:12`) **ignores its path argument and returns a constant hash of the literal
bytes `mock_fingerprint_data`**, with volume label `"MOCK_CARD"`. Every card therefore collapses to
the same `cards` row.

### 🟡 F11 — Card scan and shot pairing

*Present:* a directory walk, junk filtering, SHA-256 hashing, grouping by filename stem.

*Gaps:*
- **No EXIF is read during the scan.** Path, size and hash only — no dimensions, camera, or capture
  datetime. The spec makes reading dimensions from metadata (never by decoding) the central
  performance requirement of this requirement.
- **No candidate selection.** The JPEG-preferred-over-RAW rule that defines which asset gets
  published is absent; `CandidateShot` just holds every asset.
- **The scan hashes every file**, including 30 MB RAWs. On a 17 GB card that is minutes of I/O
  against a < 10 s target (§9.1). Nothing is parallelised — `rayon` is a dependency but is used only
  in `derivation`.
- Not restricted to the `DCIM` tree.
- **Simulated card mode (BUILDPLAN §6.3, marked "required") is not implemented** as a configuration
  option or flag — `Scanner::scan` takes a path, which is the right shape, but nothing above it
  exposes the mode.
- `group_assets` splits the stem at the *first* `.` (`grouping.rs:22`), so `2024.05.01-photo.jpg`
  groups under `2024` with every other such file.

### ⭕ F12 — Validation

Not started. No date check, no `max_age_days` comparison, no resolution check, no size check, no
`WARN`-vs-`FAIL` distinction, and no batch-median camera-clock detection. The `checks` table exists
in the schema and is never written to. `Thresholds` is defined with the correct defaults (90 days,
10 MP, 10 MB) and is never read by anything.

### ⭕ F13 — Remediation

Not started. None of the five remediation rows exist, there is no bulk-apply mechanism, and the
`95 → 88 → 82 → 75` quality ladder is absent.

**The EXIF-preserving resize the specification marks "Mandatory" does not exist.** There is no
helper that carries the metadata block forward and updates `PixelXDimension`/`PixelYDimension`, and
the dedicated round-trip test the spec names as mandatory (§9.4, Phase 2 acceptance) has not been
written. The nearest thing — `derivation::process_single` — shells out to `exiftool -TagsFromFile`
per file, which is both the G4 violation above and does not update the pixel-dimension tags.

### ⭕ F14 — RAW to JPEG

Not started. No embedded-preview extraction, no macOS ImageIO path, no `rawler` fallback, no ladder.
`rawler` is not a dependency.

### ⭕ F15 — Publish to Google Photos

Mocked, not implemented.

- `OAuth2Manager::get_bearer_token` returns the literal `"mock_access_token"`.
- `AlbumManager::resolve_album` returns `format!("mock_album_id_{}", name)`.
- `Uploader::create_media_item` returns the literal `"media_item_id_parsed"` — **the real response
  body is never parsed**, so even the live path cannot record a media item ID.
- No OAuth authorization-code flow, no `access_type=offline`, no server callback.
- **The refresh token is not encrypted at rest.** `set_oauth_token` writes the raw string into the
  column named `encrypted_refresh_token`; `test_publish_flow` asserts it reads back as plaintext.
- No batching — `batchCreate` is called with one item, never the required maximum of 50.
- No `pending → uploaded → created` state machine, no resume-from-recorded-state.
- No `429` backoff and no 30-second floor.
- No `invalid_grant` reconnect path.
- **No dry run**, which the specification makes mandatory before any publish because the API cannot
  delete.

### ⭕ F16 — Deduplication ledger

Not started. Hashes are computed and stored in `assets.sha256`, but nothing ever queries them. There
is no "has this been published" check, no manifest, and no pre-flight hash exchange. Re-ingesting the
same card would re-publish everything.

### 🟡 F17 — Jobs and progress

Types only. The `Progress` trait and a `JobState` struct exist, plus an in-memory no-op
implementation.

*Gaps:* **no job persistence** — the `jobs` table exists in the schema and `Ledger` has no method
that writes to it. The Phase 1 acceptance test ("a job written, the process simulated as restarted,
the job recovered") is instead a serde round-trip in memory (`jobs.rs:64`), which proves nothing
about restart survival. No job runner, no job IDs, no resume. The server's SSE endpoint creates a
throwaway broadcast channel per request (`api.rs:105`), emits one `"connected"` event and then
nothing — it is not connected to any work. And `f1_apply` (`api.rs:83`) runs the operation
synchronously inside the async handler, which both blocks the request until completion — the exact
thing F17 prohibits — and blocks a tokio worker thread.

### 🟡 F18 — Authentication

The strongest area of the codebase. RS256 verification, `iss`/`aud`/`exp`/`sub` checks, the UID
allow-list, a distinguishable reason code, a break-glass admin token, and six tests using a locally
generated keypair with no network — matching Phase 5's acceptance list closely.

*Gaps:*
- **The live path will not work.** Google's endpoint at `GOOGLE_CERTS_URL` returns X.509
  *certificates* (`-----BEGIN CERTIFICATE-----`). `auth.rs:101` passes those to
  `DecodingKey::from_rsa_pem`, which expects a public-key PEM and will reject them. The tests pass
  only because they inject a `PUBLIC KEY` into the cache directly, so this never surfaces.
- `not_authorized` returns **403**; specification §5.3 asks for a 401 with a distinguishable reason
  code so a client can tell "refresh and retry" from "not permitted".
- The allow-list is read from an `ALLOWED_UIDS` env var, not the `users` table in the data model.
  `ALLOWED_UIDS` is undocumented in `.env.example`.
- No sign-in on either front end; no macOS Keychain storage (`keyring` is not a dependency).

---

## 5. Server API — surface mismatch

The specification's §8 defines the API. Almost none of it exists, and what does exist is named
differently.

| Specified | Present |
|---|---|
| `POST /api/tools/dates/scan`, `/dates/fix` | `POST /api/tools/f1/plan`, `/f1/apply` — different names |
| `POST /api/tools/rename/plan`, `/rename/apply` | `POST /api/tools/f3/plan` → `501` |
| `/api/tools/split`, `/contact-sheet`, `/transform`, `/border`, `/tiff-to-jpeg` | `f4`–`f8` `/plan` → `501` |
| `GET /api/storage/ls?path=` | `POST /api/tools/f9/plan` → `501` |
| All six `/api/ingest/*` endpoints | ⭕ none |
| `GET /api/jobs/{id}` | ⭕ none |
| `GET /api/jobs/{id}/events` (SSE) | Present but not wired to any job |
| All four `/api/connectors/google/*` endpoints | ⭕ none |
| `GET /api/health` | ✅ present, unauthenticated, returns status + version |

Eight of the ten tool routes are `501` stubs. Only `f1` has a real implementation behind it.

---

## 6. Front ends

### Web (`frontend/web`) — does not build

- **`src/main.ts:7` imports `./components/F1Dates.vue`, which does not exist.**
- **`vue-router` and `lucide-vue-next` are imported** by `main.ts` and `App.vue` **but are not in
  `package.json`** and not in the lockfile.
- **There is no `typecheck` script**, so the build plan's mandatory
  `npm --prefix frontend/web run typecheck` (§4.2) cannot run at all.
- `LibraryBrowser.vue` is hard-coded mock data with a comment saying so — it never calls the API.
- `Dashboard.vue` is a placeholder card. The scaffold's `HelloWorld.vue` is still present.
- No Firebase sign-in, no token attachment, no refresh-on-expiry.
- No dry-run confirmation flow, no SSE progress, no cancel.
- The shared client (`frontend/shared/src/index.ts`) exposes only `f1_plan` and `f1_apply` — 2 of
  the ~10 tool operations — and `App.vue` does not import it, so the "no view imports `fetch`
  directly" acceptance criterion is untested.
- The layout is a 260 px fixed sidebar with a `max-width: 768px` override, which is desktop-first
  with a mobile fallback — the inverse of the "mobile-first, verified at 390 px" requirement.

### Desktop (`frontend/desktop`) — does not exist

The directory the specification's §2.7 requires is absent. `crates/desktop/tauri.conf.json` points
`frontendDist` at `../desktop/ui/dist`, a path that exists in neither layout.

`crates/desktop/src/main.rs` is 42 lines exposing two commands, `get_config` and `save_config`.
There is no `invoke` bridge to `core`'s tools, no card detection, no `reqwest` client for
server calls, no Firebase sign-in, no Keychain, and no graceful degradation when the server is
unreachable.

---

## 7. Test fixtures (BUILDPLAN §5)

The plan calls the fixture generator "not optional scaffolding — it is what makes the rest of the
plan verifiable". Three of the eight required generators exist.

| Generator | Status |
|---|---|
| `jpeg_with_exif` | 🟡 present, but **shells out to `exiftool`**, so it fails wherever `exiftool` is absent — including CI. This is what breaks 3 of the 16 tests. |
| `jpeg_without_exif` | ✅ |
| `card_tree` | 🟡 present, but writes 8-byte text files named `.JPG`/`.CR2` — not decodable images, and identical across shots. It cannot support the Phase 8 acceptance test (60 pairs / 30 JPEG-only / 10 RAW-only). |
| `half_frame_scan` | ⭕ missing — F4 has no test at all |
| `multipage_tiff` | ⭕ missing |
| `tiff_with_alpha` | ⭕ missing |
| `raw_stub_with_preview` | ⭕ missing |
| `takeout_pair` | ⭕ missing |

### Named acceptance tests that do not exist

- The EXIF-preservation round trip (spec §9.4, Phase 2) — **specification-mandatory**.
- The single-`exiftool`-process assertion (Phase 2 task 3). `test_exif_writer_single_process` is
  named for it but never counts processes — it writes 5 files and reads them back.
- Seven-tag priority tests (Phase 3).
- The `+5:0:0 0:0:0` shift test (Phase 3).
- Path-confinement *through the API* (Phase 5). The unit-level `test_resolve_g6` is good and covers
  all five required cases; the end-to-end route test does not exist.
- Every F4, F5 and F7 geometry assertion (Phase 4).
- Every Phase 8–13 acceptance test.
- The plan/apply "hash the directory before and after" assertion (Phase 3).

The 24 MP resize benchmark exists but **does not assert the 150 ms target** — it prints the duration
and asserts only that the width is 1000.

---

## 8. Repository structure and documentation

| Required | Status |
|---|---|
| `deploy/Dockerfile`, `deploy/docker-compose.yml` (spec §2.7, Phase 14) | ⭕ Directory does not exist. |
| `frontend/desktop/` (spec §2.7) | ⭕ Missing. |
| `docs/deployment.md` (Phase 14) | ⭕ Missing. |
| `docs/manual-verification.md` | ✅ Present and correctly populated for Phases 4, 7, 8, 10, 14. |
| `.env.example` listing **every** variable the system reads (Phase 0 task 5) | 🟡 `PORT` and `ALLOWED_UIDS` are read by the code and undocumented. Three Google OAuth variables are documented but read by nothing. |
| `core` module skeleton per spec §2.5 | 🟡 All seven present, plus an eighth (`derivation`) that the spec does not define. |
| `rust-toolchain.toml` pinning stable | ✅ |
| `clippy.toml` | 🟡 Present but empty (comment only). |

### README is stale and has a broken link

- **`README.md:32` still says "Specification only. No implementation yet."** — roughly 4,200 lines
  of code later.
- **`README.md:39` links `BUILD-PLAN.md`; the file is `BUILDPLAN.md`.** The link is dead.
- The README's document table omits `BUILDPLAN.md`'s actual name and `docs/`.

### Build artifacts are committed to the repository

There was **no root `.gitignore`**, and as a result **21,774 of the repository's 22,002 tracked
files were build output** — the entire `target/` directory, plus `frontend/shared/node_modules/`.
Source accounts for roughly 230 files; everything else was compiler and package-manager output.

This makes every `cargo build` dirty the working tree, makes diffs unreadable, and bloats every
clone. A root `.gitignore` has been added and those paths untracked as part of this audit.

### Stray file

`crates/core/test_nom_exif.rs` — a 4-line scratch file with an unused variable, outside any module
tree. It is not compiled, and should be deleted.

---

## 9. Phase status against the build plan

| # | Phase | Status | What blocks "done" |
|---|---|---|---|
| 0 | Repository scaffold | 🟡 | CI red on `fmt`; `cargo build --workspace` cannot pass on the Linux runner (no GTK/WebKit install step); `.env.example` incomplete; extra `derivation` module. |
| 1 | Core foundations | 🟡 | `Config::resolve` ✅ with all five tests. Ledger schema ✅. **Job persistence missing entirely**; the restart test is a serde round-trip. `todo!()` still present (Phase 2 was to remove it). |
| 2 | Media layer | 🟡 | Fixture generator 3/8 and `exiftool`-dependent. `read_meta` 6 tags not 7. No orientation handling. No quality ladder. **No EXIF-preserving re-encode.** `slices.rs` is a 4-line stub returning `0`. Benchmark unasserted. |
| 3 | Archive tools 1 | 🟡 | F1 write path incomplete and unverified; F2 no recursion; F3 capture ordering ignores metadata. |
| 4 | Archive tools 2 | 🟡 | F4 missing 3 of 4 stages; F5 missing captions and the height term; F7 missing trim, canvas rule and supersampling; F8 missing multi-page. No fixtures to test any of them. |
| 5 | Server, auth, jobs | 🟡 | Auth is strong but the cert-parsing path is broken for live use. API surface does not match §8. **G6 not wired.** No job system. Handlers block. |
| 6 | Web front end | ✗ **Broken** | Missing component, missing dependencies, no `typecheck` script — `npm run build` cannot succeed. |
| 7 | Desktop shell | ⭕ | Two config commands only. No `frontend/desktop`. macOS-gated and unverified. |
| 8 | Card detection and scan | 🟡 | F11 weak (no EXIF, no candidate selection, hashes everything). F10 absent. Fingerprint is a constant. No staging copy. |
| 9 | Validation and remediation | ⭕ | Not started. |
| 10 | RAW to JPEG | ⭕ | Not started. |
| 11 | Staging handoff and ledger | ⭕ | Not started. |
| 12 | Google Photos | ⭕ | Mock strings only. |
| 13 | Ingest UI | ⭕ | Not started. |
| 14 | Packaging and deployment | ⭕ | Not started. No `deploy/`. |

---

## 10. Suggested order of recovery

The build plan's own advice still applies: phases 0–4 have no external dependency and retire the
largest technical risk. Getting back onto it means going back to Phase 0 and moving forward one
phase at a time.

1. **Make CI green and honest.** Run `cargo fmt --all`; fix the three clippy errors; add the GTK/WebKit
   and `exiftool` install steps to `.github/workflows/ci.yml` (or exclude `desktop` from the Linux
   build and say so in the workflow). Until this is green, no phase can be assessed.
2. **Close G6 at the API boundary.** Route every path parameter in `crates/server/src/api.rs`
   through `Config::resolve` before it reaches a tool, and add the end-to-end traversal test Phase 5
   requires. This is the one gap with a security consequence today.
3. **Fix G4.** Hoist `ExifWriter::start()` out of `process_single` to one writer per batch.
4. **Finish Phase 2 properly** — the full fixture generator, the seven-tag order, the EXIF-preserving
   re-encode and its mandatory round-trip test. Everything in phases 3, 4, 9 and 10 is built on this
   layer, and the missing fixtures are why phases 3 and 4 cannot be verified.
5. **Then work phases 3 → 14 in order**, one branch and one phase report each, as §0 and §4.1
   describe.

Two smaller items worth doing immediately because they cost minutes: update `README.md`'s status
line and fix its `BUILD-PLAN.md` link, and delete `crates/core/test_nom_exif.rs`.
