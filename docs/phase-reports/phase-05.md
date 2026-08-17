## Phase 5 — Server, authentication, jobs

**Status:** complete, with live Firebase sign-in as the only gate.

**G6 is now wired at the API boundary.** That was the highest-severity open item in
`docs/gap-analysis.md` and it is closed.

### Delivered

- **Task 1 — axum skeleton.** The server is now a **library plus a thin binary** rather than a bin
  target only. An integration test cannot import from a binary, so the previous shape made it
  impossible to test the router as a client meets it. `GET /api/health` remains the one
  unauthenticated route.

- **Task 2 — Firebase ID token verification.** The audit found this module the strongest in the
  repository, but **its live path could not have worked**: it fetched Google's *x509* endpoint, which
  serves PEM **certificates**, and passed them to `DecodingKey::from_rsa_pem`, which expects a public
  key. The tests passed only because they injected a public key straight into the cache, so the bug
  was invisible.

  Now it uses Google's **JWK** endpoint and `DecodingKey::from_rsa_components`, which needs no X.509
  parsing at all. The `KeyStore` is a real cache with two constructors — `google()` refreshes from
  the network, `offline()` never touches it — so the tests exercise the same code path the server
  runs rather than a special case.

  Also found while testing: **`jsonwebtoken` defaults to 60 seconds of leeway on `exp`**, which was
  being inherited silently. A token 60 seconds past expiry was accepted. Leeway is now explicit at
  10 seconds, documented as clock-skew tolerance rather than a grace period, and pinned with a
  compile-time assertion so a later increase cannot pass unnoticed.

- **Task 3 — distinguishable reason codes.** Every rejection is a 401 carrying a `code`. A client
  branches on `token_expired` to refresh and retry; anything else means stop.
  `AuthError::is_retryable()` names that distinction rather than leaving each caller to rediscover
  it. Previously `not_authorized` returned 403, which a client could not tell from a path rejection.

- **Task 4 — jobs and SSE.** Built on Phase 1's persistence. `JobManager::spawn` writes the job row
  **before** any work starts, then runs the closure on `spawn_blocking` — the archive tools are
  synchronous and CPU-bound, and running them on an async worker would stall the runtime for every
  other request. Progress is written to the ledger *and* broadcast to subscribers.

  Handlers return **202 with a job id** and never wait. The previous `f1_apply` ran the whole
  operation inside the async handler.

  The SSE stream ends with an explicit `terminal` event, so a client knows the job is done rather
  than waiting on a connection that will never speak again. A job that has *already* finished when
  the client connects gets one replayed terminal event instead of an empty stream.

  `main` calls `recover()` at startup, so a job orphaned by an unclean shutdown is marked interrupted
  and logged rather than silently disappearing (F17).

- **Task 5 — F1–F9 wired, at the specification's routes.** The old surface was
  `/api/tools/f1/plan` with eight `501` stubs. It is now §8's:

  | Route | |
  |---|---|
  | `POST /api/tools/dates/scan`, `/dates/fix` | F1, jobs |
  | `POST /api/tools/rename/plan` | F3 dry run, synchronous — it writes nothing |
  | `POST /api/tools/rename/apply` | F3, job |
  | `POST /api/tools/split`, `/contact-sheet`, `/transform`, `/border`, `/tiff-to-jpeg` | F4–F8, jobs |
  | `GET /api/storage/ls?path=` | F9 |
  | `GET /api/jobs/{id}`, `/{id}/events` | state and SSE |

  Handlers parse, resolve, delegate — nothing else (G1).

- **Task 6 — break-glass admin token.** Compared in constant time, and off unless `ADMIN_TOKEN` is
  set.

- **G6, closed.** Every input path goes through `Config::resolve` and every output path through
  `Config::resolve_for_create` (Phase 4). A rejection is a 403 with `path_not_allowed` and a message
  that does not reveal whether the path exists outside the roots.

### Acceptance

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — 205 passed, 0 failed
- [x] `cargo test -p phototools-core` (G2 isolation) — 177 passed, 0 failed
- [x] **Token tests use a locally generated RSA keypair with the public key injected, no network.**
      All five required cases, plus an unknown signing key, a malformed token, an empty allow-list,
      and the leeway boundary.
- [x] **Valid signature but UID not on the allow-list is rejected** —
      `a_perfectly_valid_token_from_an_uninvited_account_is_rejected`. Firebase will authenticate any
      Google account in existence; this is the check that actually protects the library.
- [x] **Every `/api/tools/*` route returns 401 without a token.** The test enumerates all nine POST
      routes plus the three GETs.
- [x] **A path-traversal attempt through the API is rejected.** Four attempts — `..` out of the root,
      an absolute path to a real directory outside it, `/etc`, and a walk above the root — each
      403 with `path_not_allowed`. The test then lists the root successfully, proving the refusals
      are not a blanket failure. A separate test covers an *output* directory outside the roots.
- [x] **An SSE client receives progress events and a terminal event.** Against a real server on a
      real socket: 202 with a job id, the job row readable immediately, `text/event-stream` with
      `event: terminal`, and the persisted job reading `completed` afterwards.

Server tests went from 6 to 28 (17 unit, 11 integration).

### Measurements

Phase 5 specifies no benchmarks. `spawning_returns_immediately_and_the_job_exists_at_once` asserts
the F17 property directly: a job whose work sleeps 400 ms returns its id in under 150 ms.

### Gates

- **A real Firebase project, for live sign-in only.** All verification logic is tested offline. The
  JWK fetch path itself is the one piece no test exercises — it needs the network by definition —
  so the first live sign-in is the moment to confirm it.
- Ingest (`/api/ingest/*`) and Google Photos connector (`/api/connectors/google/*`) routes from §8
  are **deliberately absent**: they belong to Phases 11–13 and inventing them here would be G11.

### Deviations

1. **`not_authorized` returns 401, not 403.** 403 is the more RESTful choice for
   authenticated-but-forbidden, and it is what the code did before. Build plan Phase 5 task 3 asks
   for "a 401 carrying a distinguishable reason code so a client can tell 'token expired, refresh and
   retry' from 'not authorised'", which reads as both being 401 with the code carrying the
   difference. Followed the plan; flagging it because it is a defensible disagreement.
2. **The server became a lib + bin.** Not in the plan, but the acceptance criteria require driving
   the router from tests, which a bin-only crate cannot support.
3. **`ring` and `lazy_static` removed**, both unused. `async-stream` added for the SSE generator —
   a transport concern in a binary crate, recorded here per G8.
4. **Job results are a summary string, not structured data.** Enough for a progress UI and honest
   about what happened; a view that needs the full scan table will want a richer result, which is
   Phase 6's problem to raise.

### Added to manual-verification.md

Nothing new. The Firebase gate is already recorded under Phase 5's blocker in the build plan.

### Notes for the next phase

- **Phase 6 has a real API to build against**, and the shared client's `Shift` type was already
  corrected in Phase 3.
- **The front end must branch on the reason code, not the status.** Every rejection is a 401;
  `token_expired` means refresh and retry, anything else means stop. That is exactly the behaviour
  §5.3 asks the front ends for.
- **`GET /api/jobs/{id}/events` replays a terminal event for an already-finished job**, so a client
  that connects late still learns the outcome instead of hanging.
- **The web front end does not currently build** — `main.ts` imports a missing `F1Dates.vue`, and
  `vue-router` and `lucide-vue-next` are imported but absent from `package.json`. There is also no
  `typecheck` script, which build plan §4.2 requires for front-end phases. That is Phase 6's first
  job, before anything else.
- **Disk pressure is real in this environment.** `target/` reached 28 GB and the linker failed with a
  bus error mid-phase. `CARGO_INCREMENTAL=0` and periodically clearing `target/debug/incremental`
  keeps it manageable.
