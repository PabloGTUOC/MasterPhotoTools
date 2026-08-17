# PhotoTools — Build Plan

**Audience:** an autonomous development agent.
**Authority:** [`SPECIFICATION.md`](SPECIFICATION.md) defines *what* to build and
is the final word on behaviour. This document defines *in what order*, *to what
standard*, and *how to prove each piece works*.

If this document and the specification disagree, **the specification wins** —
stop and report the discrepancy rather than guessing.

---

## 0. How to use this document

1. Read `SPECIFICATION.md` in full before writing any code.
2. Work phases **in order**. A phase may not begin until the previous phase's
   Definition of Done is fully satisfied.
3. Within a phase, work tasks in order. Each task is small enough to be
   individually verifiable.
4. Run the phase's acceptance commands. They must pass, unmodified.
5. Commit, open a pull request for the phase, and report status using the
   template in §10.

**Do not** batch phases together, and do not begin a phase whose prerequisites
(§4) are unmet — stop and ask instead.

---

## 1. Ground rules

These are non-negotiable. Violating one is a defect regardless of whether tests
pass.

| # | Rule |
|---|---|
| G1 | **No functionality in a binary crate.** `server` and `desktop` contain only transport, platform integration and process lifecycle. Everything else lives in `core`. If you are about to write logic in a binary, move it. |
| G2 | **`core` must compile and pass its tests with no binary crate present.** `cargo test -p phototools-core` is run in CI with exactly that isolation. |
| G3 | **Never invoke `exiftool` to read metadata.** Reads are in-process. |
| G4 | **Never spawn one `exiftool` process per file.** Writes go through the single persistent driver built in Phase 2. |
| G5 | **Never write to a source SD card.** Copy, verify by hash, operate on the copy. |
| G6 | **Every path from an API or UI is canonicalised and checked against configured roots** before any filesystem access. |
| G7 | **Never weaken a test to make it pass.** If a test fails, fix the code or report the problem. Deleting, skipping or `#[ignore]`-ing a failing test is not a fix. |
| G8 | **Do not add dependencies beyond those listed in specification §2.6** without recording the reason in the phase report. |
| G9 | **Do not edit `SPECIFICATION.md`.** If it is wrong or ambiguous, stop and report. |
| G10 | **No `unimplemented!()`, `todo!()`, or silently swallowed errors on a shipped path.** A stub is acceptable only inside a phase, never at its Definition of Done. |
| G11 | **Do not invent scope.** Build what the specification describes. Ideas for more go in the phase report, not the code. |

---

## 2. When to stop and ask a human

Some work cannot be completed autonomously. On reaching one of these, complete
everything else in the phase, then stop and report precisely what is needed.

| Blocker | Needed from the human | Blocks |
|---|---|---|
| Firebase project | Project ID and web app config (public values) | Phase 5 integration, Phase 6 |
| Google Cloud OAuth client | Client ID and secret for a **Web application** client, redirect URI registered | Phase 12 integration |
| Google consent screen status | Confirmation it is published to production, not left in Testing (specification §6.2) | Phase 12 acceptance |
| macOS machine | Building and running anything Tauri, and all ImageIO code paths | Phases 7, 8, 10 (partial), 14 |
| Physical SD card | End-to-end ingest validation | Phase 8 acceptance (see §6.3 for the offline substitute) |
| NAS host | Deployment and SMB staging validation | Phase 14 |
| Real photographs | Visual quality judgement on F4, F7, F14 output | Phases 4, 10 acceptance |

**Do not fabricate credentials, and do not commit any secret.** Configuration is
read from environment variables; the repository holds only `.env.example`.

---

## 3. Environment

| Requirement | Version | Notes |
|---|---|---|
| Rust | stable, ≥ 1.80 | `rustfmt` and `clippy` components required |
| Node.js | ≥ 20 | For the front ends |
| `exiftool` | ≥ 12 | Metadata **writing** only |
| SQLite | bundled via `rusqlite` | No system dependency |
| Docker + buildx | current | Phase 14 |
| macOS | ≥ 14 | Only for the phases marked in §2 |

The agent is assumed to run on Linux. **Phases marked `macOS-gated` below can be
written and unit-tested on Linux but require a Mac to build and verify.** Write
the code, mark the gate in the phase report, and continue.

---

## 4. Conventions

### 4.1 Branches and commits

- One branch per phase: `phase/NN-short-name` (e.g. `phase/03-media-layer`).
- One pull request per phase, titled `Phase NN — <name>`.
- Commit messages: imperative mood, subject under 72 characters, body
  explaining *why*. Reference the functional requirement where relevant
  (`F4`, `G6`).
- Do not merge your own phase PR. Leave it for review.

### 4.2 Commands that must pass at every Definition of Done

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo build --workspace
cargo test --workspace
cargo test -p phototools-core          # G2: core in isolation
```

Front-end phases additionally:

```bash
npm --prefix frontend/web run build
npm --prefix frontend/web run typecheck
```

### 4.3 Testing standards

- Every public function in `core` has at least one test.
- Every functional requirement (F1–F18) has at least one test named after it,
  e.g. `f4_split_finds_divider_in_centre_band`.
- Image-producing operations are tested on **generated fixtures** (§5), asserting
  measurable properties — dimensions, aspect ratio, mean luminance of a region,
  preserved metadata — never by eyeballing.
- Tests must not reach the network. The Google Photos client is tested against a
  local mock.

---

## 5. Test fixtures

The agent has no photographs. **Phase 2 Task 2.1 builds a fixture generator**,
and everything afterwards depends on it. This is not optional scaffolding — it
is what makes the rest of the plan verifiable.

`crates/core/tests/fixtures/mod.rs` provides:

| Generator | Produces |
|---|---|
| `jpeg_with_exif(w, h, capture, camera)` | A JPEG of known size with a known capture date and camera |
| `jpeg_without_exif(w, h)` | A JPEG with no metadata |
| `half_frame_scan(...)` | A synthetic two-up scan: white lab border, two distinct coloured panels, a dark divider column at a known x, and dark bands of known width — so F4's divider detection and trimming can be asserted exactly |
| `multipage_tiff(pages, w, h)` | A multi-page TIFF for F8 |
| `tiff_with_alpha(w, h)` | For the flatten path |
| `raw_stub_with_preview(w, h)` | A minimal TIFF/IFD structure carrying an embedded JPEG preview, so F14's preferred path is testable without a camera file |
| `takeout_pair(name, timestamp)` | A media file plus its Google Takeout JSON sidecar, including the truncation and `(1)`-suffix variants |
| `card_tree(shots)` | A `DCIM` directory tree with configurable RAW+JPEG pairing, for F11 |

Fixtures are generated at test time into a temp directory, never committed.

**Where a generator cannot faithfully reproduce reality** — genuine RAW files
from a specific camera body, real half-frame scans — write the test against the
synthetic case, and add an entry to `docs/manual-verification.md` describing what
a human must check with real files.

---

## 6. Platform gating and the simulated card

### 6.1 macOS-gated work

Phases 7, 8, 10 (ImageIO path) and 14 include code that cannot run on Linux.
Structure it so this is not blocking:

- Platform-specific code sits behind `#[cfg(target_os = "macos")]` with a
  non-macOS fallback that returns a clear "unsupported on this platform" error.
- The **logic** around it lives in `core` and is tested on any platform.
- CI compiles for Linux; the macOS build is a human step recorded in the phase
  report.

### 6.2 The RAW decode ladder and platform

Specification §4/F14 defines the ladder as embedded preview → macOS ImageIO →
`rawler`. Steps 1 and 3 are portable and must be fully tested on Linux. Step 2 is
macOS-gated and slots between them at runtime.

### 6.3 Simulated card mode — required

**Build ingest so that any directory can be treated as a card.** A configuration
option or CLI flag points the scan at an arbitrary path, and the pipeline behaves
exactly as if a card were mounted there.

This is required for three reasons: it makes Phases 8–13 testable without
hardware, it lets a human re-run ingest over a folder of already-copied files,
and it keeps card *detection* (a thin platform concern) cleanly separate from
card *processing* (the actual work).

Detection (F10) produces a path. Everything downstream takes a path. They must
not be coupled beyond that.

---

## 7. Cross-phase contracts

Define these in Phase 1 so later phases build against stable shapes. Signatures
are indicative — adjust for idiom, but keep the boundaries.

```rust
// core::config
pub struct Config {
    pub roots: Vec<PathBuf>,          // G6 — allowed filesystem roots
    pub staging_dir: PathBuf,
    pub thresholds: Thresholds,       // max_age_days, max_megapixels, max_output_bytes
    pub database: PathBuf,
}
impl Config {
    /// G6. Canonicalise and reject anything outside `roots`.
    pub fn resolve(&self, requested: &Path) -> Result<PathBuf, Error>;
}

// core::media
pub struct MediaMeta {
    pub width: u32,
    pub height: u32,
    pub capture: Option<NaiveDateTime>,
    pub capture_source: Option<TagSource>,   // which of the 7 tags supplied it
    pub camera: Option<String>,
    pub orientation: Orientation,
}
/// G3 — in-process, never a subprocess.
pub fn read_meta(path: &Path) -> Result<MediaMeta, Error>;

/// G4 — one long-lived process for the whole batch.
pub struct ExifWriter { /* -stay_open pipe */ }
impl ExifWriter {
    pub fn start() -> Result<Self, Error>;
    pub fn write_dates(&mut self, path: &Path, set: &DateSet) -> Result<(), Error>;
    pub fn shift_dates(&mut self, path: &Path, delta: &str) -> Result<(), Error>;
}

// core::jobs
pub trait Progress: Send + Sync {
    fn report(&self, done: u64, total: u64, message: &str);
    fn cancelled(&self) -> bool;
}

/// Every long operation takes a Progress and returns a summary. F17.
pub type ToolResult<T> = Result<Outcome<T>, Error>;

// core::tools — every tool follows this shape
pub struct Plan<T> { pub actions: Vec<T>, pub skipped: Vec<Skip> }
pub trait Tool {
    type Params; type Action; type Summary;
    /// Dry run. Never touches disk. Specification principle 5.
    fn plan(&self, p: &Self::Params) -> ToolResult<Plan<Self::Action>>;
    fn apply(&self, plan: Plan<Self::Action>, progress: &dyn Progress) -> ToolResult<Self::Summary>;
}
```

**The `plan` / `apply` split is mandatory for every tool that writes.** It is how
the dry-run guarantee is delivered uniformly rather than tool by tool.

---

## 8. Phases

Effort figures are focused working days. `macOS-gated` marks phases needing a Mac
to complete verification.

---

### Phase 0 — Repository scaffold · 1 d

**Goal.** A workspace that builds, tests and lints, containing no functionality.

**Tasks**

1. Cargo workspace at the root with members `crates/core`, `crates/server`,
   `crates/desktop`.
2. `crates/core` with the module skeleton from specification §2.5 — `media`,
   `tools`, `ingest`, `publish`, `ledger`, `jobs`, `config` — each an empty
   module with a doc comment stating its responsibility.
3. `rust-toolchain.toml` pinning stable; `rustfmt.toml`; `clippy.toml`.
4. GitHub Actions workflow running the §4.2 command set on push, including the
   isolated `cargo test -p phototools-core`.
5. `.env.example` listing every environment variable the system will read, each
   with a comment. No values.
6. `docs/manual-verification.md` — empty with a heading, to be appended to.

**Acceptance**

```bash
cargo build --workspace && cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

**Done when** CI is green on a pull request and the workspace contains no
business logic.

---

### Phase 1 — Core foundations · 2 d

**Goal.** Configuration, errors, persistence and the contracts in §7.

**Tasks**

1. `config` — `Config`, `Thresholds`, loading from environment with documented
   defaults (`max_age_days = 90`, `max_megapixels = 10`,
   `max_output_bytes = 10 MB`).
2. **`Config::resolve` implementing G6.** Canonicalise, resolve symlinks, reject
   anything outside `roots`.
3. `Error` as a single crate-wide enum with `thiserror`. No `anyhow` in library
   code.
4. `ledger` — SQLite schema exactly as specification §7, with forward
   migrations. Open, migrate, and round-trip every table.
5. `jobs` — the `Progress` trait, an in-memory implementation for tests, and job
   state persistence. Jobs must survive process restart (F17).
6. Define the §7 contracts as traits and types, with `todo!()` bodies — the only
   phase where that is permitted, and Phase 2 removes them.

**Acceptance**

- `Config::resolve` tests cover: a path inside a root, a path outside, `..`
  traversal, an absolute path outside, and a symlink pointing outside. **All
  five must reject correctly** — this is G6 and the most security-relevant code
  in the system.
- Ledger round-trip test per table.
- A job written, the process simulated as restarted, the job recovered.

**Done when** the above pass and no module outside `config`/`ledger`/`jobs` has
been touched.

---

### Phase 2 — Media layer · 4–5 d

**Goal.** Everything that touches image bytes or metadata. Nothing above this
layer may read or write images directly.

**Tasks**

1. **The fixture generator (§5).** Do this first; the rest of the phase is
   tested with it.
2. `read_meta` — in-process EXIF (G3). Implement the seven-tag preference order
   from specification F1 exactly, including normalisation, the
   `0000:00:00 00:00:00` sentinel, timezone stripping, `YYYY-MM-DD` acceptance,
   and QuickTime-as-UTC.
3. `ExifWriter` — the `-stay_open` persistent driver (G4). Handshake, command
   framing, `{ready}` sentinel handling, timeout, and clean shutdown. **A test
   must assert that writing 50 files spawns exactly one process.**
4. Decode and encode for JPEG, PNG, TIFF; EXIF orientation applied on load.
5. Resize via `fast_image_resize`, downscale-only helper, and the
   quality-stepping encoder (`95 → 88 → 82 → 75`) used by F13.
6. **EXIF-preserving re-encode** — carry the metadata block forward and update
   `PixelXDimension`/`PixelYDimension`. Specification F13 marks this mandatory.
7. Slice-based primitives for edge scanning, dark-band detection and column
   profiling (specification §9.1 rule 3). These are shared by F4 and F7 — write
   them once here.

**Acceptance**

- Single-process assertion for `ExifWriter` (task 3).
- A round-trip test: generate a JPEG with a known capture date, resize it, read
  the metadata back, assert the date and camera survived and the pixel
  dimensions were updated. **This test is named in the specification as
  mandatory; it must exist and pass.**
- Tag-priority tests: a file with several date tags resolves to the correct one
  for every position in the order.
- Benchmark: resize and encode one 24 MP JPEG in under 150 ms
  (specification §9.1). Record the measured figure in the phase report.

**Done when** the acceptance items pass and no `todo!()` remains in `media`.

---

### Phase 3 — Archive tools, part 1 · 5–6 d

**Goal.** F1, F2, F3, F9 — the metadata and filesystem tools.

**Tasks**

1. **F9 library browser** first — small, and it exercises G6 end to end.
2. **F1 date scan** — walk, classify `OK` / `Mismatch` / `Missing Metadata`,
   handling every extension group in the specification.
3. **F1 repair** — all four modes (`auto`, `manual`, `shift`, `sidecar`) through
   `plan`/`apply`.
4. **Platform-dependent filesystem timestamps.** macOS and BSD expose a settable
   creation time; Linux does not. Branch on platform, compare against
   modification time where no creation time exists, and **never report an
   outcome that was not verified** (specification §9.2 invariant 6). This is a
   named requirement, not an edge case.
5. **F2 Takeout sidecars** — including both filename quirks: truncation, and
   `(1)`-style suffixes on either the media file or the sidecar.
6. **F3 rename** — prefix assembly with the sanitising rules, both orderings,
   zero-padding, and the two-phase plan/apply. Duplicate targets within a batch
   and collisions on disk are skipped and reported, **never overwritten**.

**Acceptance**

- F1: for each of the seven tags, a fixture where that tag is the highest
  available resolves to it.
- F1 `shift`: a fixture dated 2019 shifted by `+5:0:0 0:0:0` reads back as 2024.
- F2: all sidecar-naming variants resolve; a missing sidecar is reported, not
  fatal.
- F3: a collision test proving no file is ever overwritten.
- Every tool: `plan` makes no filesystem modification. Assert by hashing the
  directory before and after.

---

### Phase 4 — Archive tools, part 2 · 5–6 d

**Goal.** F4, F5, F6, F7, F8 — the image-producing tools.

**Tasks**

1. **F6 transform** — simplest; establishes the pattern.
2. **F8 TIFF to JPEG** — multi-page naming, alpha flattening, 2048 px cap.
3. **F5 contact sheet** — grid maths exactly as specified, caption truncation at
   28 characters, and the **red crossed box for an unreadable file, which must
   never abort the sheet**.
4. **F7 print border** — dark-edge trim, canvas selection (4:5 portrait, 5:4
   landscape, 3000 px long side), fit inside the 50 px margin, rounded corners at
   4× supersampling.
5. **F4 half-frame split** — the four-stage procedure. Use the Phase 2 slice
   primitives.

**Acceptance**

- F5: a sheet from 9 fixtures where the 5th is corrupt — assert the output
  dimensions match the formula and that the 5th cell contains red pixels.
- F7: portrait input yields exactly 3000×3750; landscape yields 3000×2400;
  the image is centred with at least 50 px of white on every side.
- F4: on `half_frame_scan` with a divider planted at a known column, assert the
  detected split is within a small tolerance of it, both halves are portrait, and
  both are within 10% of ratio 24/17.
- Append to `docs/manual-verification.md`: F4 and F7 need a human to judge real
  scans.

---

### Phase 5 — Server, authentication, jobs · 4–5 d

**Goal.** `phototools-server` exposing specification §8 for the archive tools.

**Tasks**

1. axum skeleton, layered config, graceful shutdown, `GET /api/health`
   unauthenticated returning status and version.
2. **Firebase ID token verification** (specification §5.2): fetch and cache
   Google's signing certificates, verify RS256, check `iss`, `aud`, `exp`,
   `sub` — then check `sub` against the **UID allow-list**.
3. Return a `401` carrying a **distinguishable reason code** so a client can tell
   "token expired, refresh and retry" from "not authorised" (specification §5.3).
4. Job endpoints and the SSE progress stream. **No handler may block until an
   operation completes** (F17).
5. Wire F1–F9 as handlers. Handlers only parse, resolve paths via
   `Config::resolve`, and delegate (G1).
6. Local break-glass admin token from environment, for when Firebase is
   unreachable (specification §5.3).

**Acceptance**

- Token tests use a **locally generated RSA keypair**, injecting the public key
  so no network is needed: valid token accepted; expired rejected; wrong `aud`
  rejected; wrong `iss` rejected; **valid signature but UID not on the allow-list
  rejected**. That last case is the one that actually protects the library.
- Every `/api/tools/*` route returns 401 without a token.
- A path-traversal attempt through the API is rejected (G6 end to end).
- An SSE client receives progress events and a terminal event.

**Blocked on** a real Firebase project only for live sign-in; all verification
logic is testable offline.

---

### Phase 6 — Web front end · 5–6 d

**Goal.** Mobile-first UI over F1–F9.

**Tasks**

1. Vite + Vue 3 in `frontend/web`, with `frontend/shared` for components and the
   API client.
2. **The shared API client exposes one interface** with an HTTP implementation
   here. Phase 7 adds the Tauri implementation. Views import the interface, never
   a transport.
3. Firebase web sign-in; attach the ID token to every request; refresh
   transparently on the expiry reason code.
4. A view per tool. **Every destructive action shows the dry-run plan and
   requires confirmation** before apply.
5. Job progress via SSE with cancel.
6. Library browser with breadcrumbs, usable one-handed on a phone.

**Acceptance**

- `npm run build` and `npm run typecheck` clean.
- Layout verified at 390 px width.
- No view imports `fetch` directly — all traffic goes through the shared client.

---

### Phase 7 — Desktop shell · 3–4 d · **macOS-gated**

**Goal.** `phototools-desktop` running, with the Vue UI and the `invoke` bridge.

**Tasks**

1. Tauri v2 scaffold, `frontend/desktop` reusing `frontend/shared`.
2. Tauri command layer delegating to `core` (G1). Commands parse and delegate,
   nothing more.
3. The Tauri implementation of the shared API client interface.
4. Server connection settings; **HTTP calls to the server made from the Rust
   side with `reqwest`, never from the webview** (specification §8) — this avoids
   CORS, mixed content and certificate handling entirely.
5. Firebase sign-in on desktop; refresh token in the macOS Keychain.
6. Graceful degradation when the server is unreachable: server-backed features
   disable with a clear indicator; nothing local breaks.

**Acceptance**

- The app launches and runs an F1 date scan on a local folder.
- With the server stopped, the app still starts and local tools work.
- Human step: confirm the build runs on macOS; record in the phase report.

---

### Phase 8 — Card detection and scan · 3–4 d · **macOS-gated (detection only)**

**Goal.** F10 and F11.

**Tasks**

1. **F11 first, in `core`, using simulated card mode (§6.3)** — parallel walk,
   in-process metadata, grouping by filename stem, candidate selection, content
   hashing. Fully testable on Linux.
2. Card fingerprinting: volume label plus a hash over sorted
   `(relative path, size, mtime)`.
3. **F10 detection in the desktop binary** — `/Volumes` watching, debounce,
   `DCIM` check, native notification. Thin: it produces a path and hands it to
   the `core` pipeline (§6.3).
4. **Staging copy with hash verification, never writing to the card** (G5).

**Acceptance**

- `card_tree` fixture with 100 shots — 60 RAW+JPEG pairs, 30 JPEG-only, 10
  RAW-only — groups into exactly 100 shots with correct candidates.
- Dimensions are read from metadata: assert no full decode occurs during scan
  (specification F11 — decoding would turn a two-second scan into a two-minute
  one).
- A read-only fixture directory is scanned successfully and is byte-identical
  afterwards (G5).
- Performance: 400-shot fixture scans in under 10 seconds (§9.1). Record the
  figure.

---

### Phase 9 — Validation and remediation · 4–5 d

**Goal.** F12 and F13.

**Tasks**

1. The three checks — date, resolution, size — each independently testable.
2. **Batch median clock detection**: median capture date across the card; if the
   median is out of range but the spread is under 30 days, surface a single bulk
   correction of `now − median` rather than one prompt per file.
3. `WARN` versus `FAIL` for in-range-but-far-from-median.
4. Remediation actions from the F13 table, **each available as a bulk apply
   across all shots sharing a failure**.
5. Resize using the Phase 2 EXIF-preserving path.

**Acceptance**

- A fixture card where every frame is dated 2019 with a tight spread produces
  **one** bulk-shift suggestion, not 400 individual failures.
- A 24 MP fixture fails the resolution check, and resizing brings it under 10 MP
  **with the capture date intact**.
- A fixture at 10.0 MP exactly passes; 10.1 MP fails. Assert the boundary.
- Bulk apply over 50 shots sharing a failure completes as one operation.

---

### Phase 10 — RAW to JPEG · 2–3 d · **macOS-gated (step 2 only)**

**Goal.** F14.

**Tasks**

1. **Embedded preview extraction** — walk the IFD structure, locate the largest
   preview, slice it out. Portable; test on Linux with `raw_stub_with_preview`.
2. **macOS ImageIO path** behind `#[cfg(target_os = "macos")]`, via `sips` or
   `objc2`.
3. **`rawler` fallback**, portable.
4. The ladder with correct fallthrough, and metadata copied from the RAW into
   the output.
5. Output passes through Phase 9 validation and resize.

**Acceptance**

- Ladder order asserted: with a preview present, steps 2 and 3 are not reached.
- With no preview, the fallback runs and produces a valid JPEG.
- Capture date, camera and lens survive into the output.
- Append to `docs/manual-verification.md`: colour quality on real RAW files from
  each camera body needs human judgement.

---

### Phase 11 — Staging handoff and ledger · 3–4 d

**Goal.** F16 and the desktop-to-server handoff.

**Tasks**

1. Manifest format: per shot, the content hash, derived file name, dimensions
   and capture date.
2. **Pre-flight deduplication** — the desktop sends hashes *before* copying any
   bytes; the server replies with which are new. Known duplicates are never
   transferred.
3. Staging directory writer on the desktop; watcher on the server; hash
   verification on arrival, with mismatches recopied rather than failed.
4. Ledger writes recording every published hash.

**Acceptance**

- Ingesting the same fixture card twice transfers zero bytes the second time and
  publishes nothing.
- A truncated staged file is detected by hash mismatch and recopied.
- The manifest round-trips through JSON without loss.

---

### Phase 12 — Google Photos · 5–7 d

**Goal.** F15.

**Tasks**

1. OAuth: authorization-code flow with `access_type=offline` and
   `prompt=consent`, scope `photoslibrary.appendonly`, server-side callback,
   refresh token **encrypted at rest** with a key from environment.
2. Two-step upload — `POST /v1/uploads` for the token, then
   `mediaItems:batchCreate` in batches of **at most 50**.
3. **The `pending → uploaded → created` state machine** persisted, with retry
   resuming only from the recorded state. `batchCreate` is not idempotent; a
   naive retry duplicates photographs.
4. `429` backoff with a **30-second floor**, then exponential.
5. **Reconnect path**: catch `invalid_grant`, mark the connector disconnected,
   surface a re-authorise prompt. Do not fail silently — a project left in
   Testing status expires refresh tokens every seven days.
6. **Dry run, mandatory before any publish.** The API cannot delete, so a
   mistaken bulk publish is manual cleanup by hand.

**Acceptance**

- All tests run against a local mock; no test reaches Google.
- 120 items produce exactly 3 `batchCreate` calls.
- A simulated timeout after upload but before create resumes without
  duplicating.
- A `429` response is followed by a wait of at least 30 seconds.
- `invalid_grant` marks disconnected and does not retry in a loop.
- **Publish is refused if no dry run has been performed for the session.**

**Blocked on** a real OAuth client for live verification. Before the first real
bulk run, publish one photograph and confirm the capture date survives and it is
filed under the correct day (specification §6.4).

---

### Phase 13 — Ingest UI · 4 d

**Goal.** The card review experience.

**Tasks**

1. Review grid: one row per shot with pairing, capture date, megapixels, size
   and a status chip per check.
2. Filter by failure class; bulk action bar.
3. **Auto-resize on by default.** A 10 MP ceiling means a 24–45 MP camera fails
   the resolution check on virtually every frame — resizing is the normal path,
   so the UI is built for bulk approval, not four hundred prompts.
4. Mandatory dry-run preview before publish.
5. Live progress; clear indication when the desktop's work is finished and the
   server has taken over.

**Acceptance**

- A 400-shot session renders and stays responsive.
- Bulk-approving all resizes is one action.
- Publish is unreachable in the UI until a dry run has been reviewed.

---

### Phase 14 — Packaging and deployment · 2–3 d · **macOS-gated**

**Goal.** Shippable artefacts.

**Tasks**

1. Multi-stage `Dockerfile` on `distroless/cc`, with **`cargo-chef` dependency
   caching configured from the start** — without it every build recompiles the
   whole dependency tree.
2. `docker buildx` multi-architecture build for `linux/amd64` and `linux/arm64`.
3. `docker-compose.yml` with volume mounts, environment and a health check.
4. `.dmg` bundle, application icon, launch at login.
5. `docs/deployment.md` — server deployment, desktop install, Firebase setup,
   Google OAuth client setup, and every environment variable.

**Acceptance**

- The image builds for both architectures and the container passes its health
  check.
- The `.dmg` installs and launches on macOS (human step).
- A second person can follow `docs/deployment.md` without asking questions.

---

## 9. Sequencing summary

| Phase | Name | Days | Gate |
|---|---|---|---|
| 0 | Repository scaffold | 1 | — |
| 1 | Core foundations | 2 | — |
| 2 | Media layer | 4–5 | — |
| 3 | Archive tools, part 1 | 5–6 | — |
| 4 | Archive tools, part 2 | 5–6 | — |
| 5 | Server, auth, jobs | 4–5 | Firebase project for live sign-in |
| 6 | Web front end | 5–6 | Firebase config |
| 7 | Desktop shell | 3–4 | **macOS** |
| 8 | Card detection and scan | 3–4 | **macOS** for detection |
| 9 | Validation and remediation | 4–5 | — |
| 10 | RAW to JPEG | 2–3 | **macOS** for ImageIO |
| 11 | Staging handoff and ledger | 3–4 | — |
| 12 | Google Photos | 5–7 | OAuth client for live run |
| 13 | Ingest UI | 4 | — |
| 14 | Packaging and deployment | 2–3 | **macOS**, NAS |

**Total ≈ 52–65 days.**

**Phases 0–4 have no external dependency at all.** They deliver a fully tested
core with every archive tool and no UI — the largest technical risk, retired
first. Begin there and do not wait on any human input.

---

## 10. Phase report template

Post this on the phase pull request.

```markdown
## Phase NN — <name>

**Status:** complete | blocked | complete with gates

### Delivered
- <requirement id>: <what was built>

### Acceptance
- [ ] cargo fmt --all --check
- [ ] cargo clippy --workspace --all-targets -- -D warnings
- [ ] cargo build --workspace
- [ ] cargo test --workspace
- [ ] cargo test -p phototools-core   (G2 isolation)
- [ ] <phase-specific criteria, one line each>

### Measurements
<benchmark figures where the phase specifies them>

### Gates
<anything needing a human: macOS build, credentials, real files>

### Deviations
<anything built differently from this plan, and why. Empty is the expected answer.>

### Added to manual-verification.md
<what a human still needs to check by eye>

### Notes for the next phase
<contracts introduced, decisions made, traps found>
```

---

## 11. Failure protocol

| Situation | Action |
|---|---|
| A test fails and the cause is unclear | Investigate. Do not modify the test. Report if still unresolved. |
| The specification is ambiguous | Stop. Report the ambiguity with the options and a recommendation. Do not pick one silently. |
| A dependency does not do what was assumed | Report before substituting. G8. |
| A performance target is missed | Report the measured figure. Do not silently relax the target. |
| A phase needs something from §2 | Complete everything else, then report precisely what is needed. |
| You are tempted to put logic in a binary crate | You have found a missing `core` abstraction. Add it to `core`. G1. |
