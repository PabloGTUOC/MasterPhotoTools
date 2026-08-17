## Phase 7 — Desktop shell

**Status:** complete with gates — everything is written, compiled and tested on Linux; the macOS
build, launch and Keychain round trip are human steps, recorded in
`docs/manual-verification.md`.

### Delivered

- **Task 1 — Tauri v2 scaffold and `frontend/desktop`.** The directory specification §2.7 requires
  did not exist. It does now, and `crates/desktop/tauri.conf.json` pointed `frontendDist` at
  `../desktop/ui/dist`, a path that existed in neither layout; it now points at the real build.

  The desktop reuses `frontend/shared` and **the same view files as the web build** — `Library`,
  `Dates`, `Rename`, `ImageTool`, `ContactSheet`, `Transform`. That is the point of the shared
  interface: views are written once. (They were copies when this phase landed; `phase-07a` moved
  them into `frontend/shared/src/ui`, so they are now literally the same files.)

- **Task 2 — the command layer.** Seventeen commands, each of which parses, resolves paths against
  the configured roots, and delegates. No functionality lives in the binary (G1). **G6 holds on the
  desktop as well as the server**: inputs through `Config::resolve`, outputs through
  `resolve_for_create`, and an access-denied error is rendered as a sentence a person can act on
  rather than a path they were not allowed to see.

  The crate is now a lib plus a thin binary, so the command layer is reachable from tests.

- **Task 3 — the Tauri `ApiClient`.** `frontend/desktop/src/api.ts` implements the same interface
  the web build implements over HTTP. **It is the only file that differs between the two front
  ends.** Job progress arrives as Tauri events rather than SSE, which is F17's other half.

- **Task 4 — server settings, and HTTP from the Rust side.** `server.rs` holds a `reqwest` client
  and the base URL. **Nothing in the webview issues an HTTP request** — specification §8's note,
  which avoids CORS, mixed content and certificates on a local network. The transport check enforces
  it: `check:transport` allows only `src/api.ts`, and that file calls `invoke` and nothing else.

- **Task 5 — Keychain.** `credentials.rs` stores the Firebase refresh token via `keyring`, which on
  macOS is the Keychain (§5.2). A missing entry reads as `None` rather than an error, because
  "nobody has signed in yet" is not a failure.

- **Task 6 — graceful degradation.** `ServerConnection::status()` **never returns an error**: an
  unreachable NAS is a state the UI shows, not a failure that breaks the app. The shell polls it,
  shows a red or green indicator with the reason, and a banner explaining that server-owned work is
  unavailable while local tools keep working.

- **G1 fix, found while building this.** The server's `JobManager` did the running, persistence and
  recovery; the desktop needs all three. Writing a second copy would have been a plain G1 violation
  ("if `server` and `desktop` both need it, it belongs in `core`"). The runner now lives in
  `core::jobs::JobRunner` with a pluggable `JobEventSink`; the server supplies a broadcast sink for
  SSE and the desktop a sink that emits Tauri events. `JobManager` is now a thin adapter and its
  tests are unchanged and still pass.

  That refactor also removed a race I had introduced: the sink looks a job up by id, so the id must
  exist before the job can emit. `JobRunner::spawn_with_id` lets the caller register its listener
  first, which closes the window in which early updates had nowhere to go.

### Acceptance

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — 213 passed, 0 failed
- [x] `cargo test -p phototools-core` (G2 isolation) — 177 passed, 0 failed
- [x] `npm --prefix frontend/desktop run typecheck` clean
- [x] `npm --prefix frontend/desktop run build` clean — 117 kB, 44 kB gzipped
- [x] `npm --prefix frontend/desktop run check:transport` — nothing but `api.ts` reaches out
- [~] **The app launches and runs an F1 date scan on a local folder.** The scan is tested headlessly
      (`a_date_scan_runs_locally_with_no_server_present`); launching a window needs a Mac.
- [~] **With the server stopped, the app still starts and local tools work.**
      `local_jobs_run_while_the_server_is_unreachable` proves the behaviour against a dead port;
      that the *window* opens needs a Mac.
- [ ] **Human step: confirm the build runs on macOS.** Outstanding, and now written up with what
      specifically to check.

### Measurements

Desktop bundle 117.55 kB raw, 44.04 kB gzipped — well under the web build's 280 kB, because the
desktop needs no Firebase web SDK and no HTTP client in the webview.

### Gates

- **A Mac.** Five items in `docs/manual-verification.md`: the build, the `invoke` round trip, the
  offline indicator, the Keychain round trip, and Tauri job events including the interrupted-job
  case.
- **Firebase configuration**, shared with Phase 6.

### Deviations

1. **Desktop sign-in is not wired to Firebase yet.** `credentials.rs` stores and reads the refresh
   token, and the command layer is ready, but there is no desktop sign-in flow. Firebase's web SDK
   assumes a browser redirect or popup, which inside a Tauri webview needs either the deep-link
   plugin or a system-browser flow, and choosing between those without a Firebase project to test
   against would be guessing. **This is the one task-5 item genuinely not done**, and it is blocked
   on the same gate as Phase 6.
2. **Two npm dependencies for the desktop front end:** `@tauri-apps/api` (the `invoke` bridge) and
   `vue-router`. Recorded per G8. The desktop needs no `firebase` package until item 1 is settled.
3. **`keyring` added to the desktop crate.** It is named in specification §2.6, so this is the
   dependency arriving rather than drifting.
4. **The desktop router uses hash history**, not path history: the app is served from a file URL
   with no server to fall back on.
5. ~~**The views were copied, not shared through a package.**~~ **Resolved** — see
   `phase-07a-view-dedup.md`. The ten shared files now live in `frontend/shared/src/ui` and both
   front ends compile them from there, which is what §2.7 describes.

### Added to manual-verification.md

Five Phase 7 entries, replacing the single line that said only "requires macOS".

### Notes for the next phase

- **Phase 8 is card detection and scan**, and `notify` is still not a dependency — specification
  §2.6 names it for filesystem watching and F10 needs it.
- **Build plan §6.3 requires simulated card mode**, and it does not exist. The scanner takes a path,
  which is the right shape, but nothing exposes "treat this directory as a card". Phase 8 should add
  it early, since Phases 8–13 are otherwise untestable without hardware.
- **`Fingerprint::generate` still ignores its path and returns a constant**, so every card collapses
  to one `cards` row. That is Phase 8 task 2.
- **The view duplication in deviation 5 is resolved** (`phase-07a`), so Phase 13 can add ingest views
  to `frontend/shared/src/ui` and have them typechecked against both transports for free.
