# RS-1 + RS-2 · Remove Google Photos publishing and the RAW tab

> Plan: [`rescope-plan.md`](../rescope-plan.md) steps RS-1 and RS-2, on one branch `rs-1`, because both are
> removals and touch the same files. Implemented by Claude while agy is out of credits, at the owner's
> instruction (2026-11-13). The review states that the reviewer was not independent.

## Brief

### Why
**The owner, 2026-11-13:**
- *"On the web, the current functionality to upload to google pictures is also to be deleted, makes no
  sense."*
- Asked about PhotoTools' RAW tab, now that Rollchrome's Develop does RAW properly, they chose: **remove it**.

**Two constraints on the location path:**
- **The owner, same day:** the phone's location engine *"is to remain only there"*, on the website, and the
  desktop *"can call the website one to ingest locations and apply them on the local machine"*.
- **WF-5 in [`workflow-plan.md`](../workflow-plan.md)** planned the same retirement, but it also removed the
  desktop's server connection. That predates timeline sync. **This step keeps the connection.**

### Remove

**Core (`crates/core`)**
- `publish/`: `api`, `auth`, `crypto`, `folder`, `publisher`, `mod`, and `pub mod publish`.
- `ingest::handoff`, `ingest::staging`, `ingest::remediation`, and their re-exports.
- The `Ledger` methods used only by those: the sessions, the publish log and the hand-off manifest.
- `Config::publishing_dir` and its environment variable. `Config::staging_dir` goes only if nothing that
  remains uses it.

**Server (`crates/server`)**
- Routes:
  - `/api/ingest/remediate`
  - `/api/ingest/derive`
  - `/api/ingest/sessions*`
  - `/api/publish/folder*`
  - `/api/connectors/google/*`
- Their handlers, the Google OAuth state and the token encryption key.
- Their tests.

**Desktop (`crates/desktop`)**
- Commands: `stage_card`, `hand_off_card`, `remediate`, `derive_raw` (RS-2) and `fill_publishing`.
- The hand-off's HTTP client in `server.rs`.

**Front ends**
- **Web:** `views/Publish.vue` and its route.
- **Shared:** `views/RawToJpeg.vue` and its route (RS-2).
- **Shared `client.ts` methods and types:** `remediate`, `deriveRaw`, `fillPublishing`, the folder-publish
  calls, the session publish calls and the Google connector calls.
- **Desktop `api.ts`:** the same methods.
- **Shared:** `BulkActions.vue`, which has had no caller since WF-1, and its part in the `check:ingest`
  harness.

**Tests:** `publish_tests.rs`, `handoff_tests.rs`, and the tests of the removed modules, routes and commands.
Each removed test file or group is listed in the report with the code it tested (G7: only tests of
removed code).

### Keep, untouched
- **Ingest:**
  - scan, validate (the sanity status) and deliver (card → folder, verified by hash, G5);
  - `ingest::derivation`, which validation and grouping use;
  - the server's `/api/ingest/scan` and `/validate`.
- **Takeout import** (`f2_takeout`).
- **The location path:**
  - the server's `/api/timeline/owntracks` receiver;
  - `/api/timeline/tracks`, `/deletions` and `/inventory`;
  - every `/api/tracks` route;
  - the desktop's `sync_timeline`, `server_status`, the server settings commands and `ServerConnection`,
    and its `session_client`.
- **Database:** the ledger's schema and migrations. **No migration drops a table:** a NAS database may hold
  rows, and old rows are simply no longer read.
- `SPECIFICATION.md` (G9). F15 and F16 no longer describe the system, which is recorded in `known-gaps.md`.

### Documents
- `deployment.md`: remove the Google and publishing environment variables.
- `publish-folder-plan.md` and `workflow-plan.md`: status withdrawn, with the date and reason. WF-5 is done
  by RS-1, except the server connection.
- `manual-verification.md`: MV-11, MV-12, MV-16 and the other publish checks are marked withdrawn, not
  deleted.
- `CLAUDE.md`: the screen list and the gate counts.

### Acceptance
- **Every gate passes** with counts reported. Every test that disappears belongs to removed code, listed by
  file.
- **The timeline sync tests and the OwnTracks tests pass with unchanged counts.**
- `grep -ri "google photos\|publishing_dir\|hand_off\|deriveRaw\|fillPublishing" crates frontend --include=*.rs
  --include=*.ts --include=*.vue` finds nothing but comments explaining what was removed.
- **Web `check:layout`:** the routes left are clean at 390 px, two fewer than before (Publish, RAW).
- **`check:ingest`** still passes.

## Report

Implemented by Claude on 2026-11-14.

**What was removed**
- **Core:**
  - `publish/` and `ingest::{handoff, staging, remediation}`;
  - 34 `Ledger` methods that only those called, and the `PublishRow` and `OAuthGrant` types;
  - `Config::{publishing_dir, staging_dir}` with their environment variables, load-time check and the
    two publishing-folder guards (`refuse_publishing_output`, `resolve_for_publishing`).
- **Server:** the remediate, derive, session, folder-publish and Google routes and handlers, and the Google
  connector unit tests. The remediation `actions`/`default_action` fields came off the validation
  response, since they described the removed bulk actions.
- **Desktop:**
  - `stage_card`, `remediate`, `hand_off_card`, `derive_raw` and `fill_publishing`;
  - the session half of the server client (`open_session`, `mark_ready`, the job polling).
  - **Kept:** what remains of the client, the one timeline sync uses, renamed `HandoffClient` →
    `ServerClient`, together with `ServerConnection`, the settings, `server_status` and `sync_timeline`.
- **Front ends:**
  - `Publish.vue`, `RawToJpeg.vue`, `BulkActions.vue`, and the `/publish` and `/raw-to-jpeg` routes;
  - the dashboard tile;
  - the client methods and types;
  - the desktop client methods.
- **Checks:** `check:ingest` parts (2) and (3) and their harness. `check:layout`'s list loses the two routes.
- **Deploy:** the publishing volume, `PUBLISHING_PATH`/`PUBLISHING_DIR`, `STAGING_DIR` and the four Google
  variables from Compose, the Dockerfile, `deploy.sh` (which still parses) and `deploy.env.example`.
  `ADMIN_TOKEN` is kept: it is what sync authenticates with.
- **Documents:**
  - `deployment.md`;
  - `known-gaps.md` (a new G9 entry: F13, F14, F15 and F16 no longer describe the system; dated closing
    notes on the affected entries);
  - `publish-folder-plan.md` (withdrawn);
  - `workflow-plan.md` (WF-5 done);
  - `manual-verification.md` (Phases 9 to 12 and folder publishing marked withdrawn, not deleted);
  - `CLAUDE.md`;
  - the front-end README.

**Kept, as the brief required:**
- Ingest's scan, validate and deliver, and `ingest::derivation`.
- Takeout import.
- The phone receiver and every sync and tracks route.
- The schema: no migration drops a table.

**Tests: 1349 before, 1153 after, 196 removed, 0 added.** Each removed test was diffed by name
(`cargo test --workspace -- --list`, before and after):
- Unit tests in removed modules: `ingest::handoff` 39, `ingest::remediation` 13, `ingest::staging` 8,
  `publish` 43.
- `config::tests`: 18 tests of the publishing folder and its guards.
- The server's `api::connector_tests`: 3.
- The `ledger::tests` test of the removed `add_card`: 1.
- Integration tests:
  - `publish_tests.rs` and `handoff_tests.rs`;
  - 18 server tests of the hand-off and Google routes;
  - 8 remediation tests in `validation_tests.rs`;
  - the staging-only `staged_copies_are_verified…`;
  - the two Rollchrome tests of the publishing-folder guard.

**Tests kept, and changed, with the reason:**
1. **`a_read_only_card_scans_and_is_byte_identical_afterwards`** (the G5 test). Its copy step called the
   removed `stage_all`. It now calls `deliver_all`, which is now the only copy off a card. The read-only
   card and the byte-for-byte assertion are unchanged.
2. **`a_database_at_any_version_catches_up_to_a_fresh_one`.** Its one `open_folder_session` call became the
   same `INSERT` in SQL. The table stays, so the migration still has to accept a row.
3. **`every_table_round_trips`.**
   - Its `add_shot` setup became the same `INSERT`.
   - Its sections for `checks`, `derived`, `publishes` and `oauth` were removed, because their API is
     gone. Those tables stay, and test 2 covers that the migrations create them.
   - Every other section is unchanged.

   These are the only changes to assertions of surviving tests, and they are listed here for the reviewer.

**Gates** (`gate-rs1.log`)

| Gate | Result |
|---|---|
| fmt / clippy / build | rc 0 |
| `--workspace` | **1148 passed**, 0 failed, 5 ignored (= the 1153 listed) |
| `phototools-core` | **543** |
| `rollchrome-core` | 469 |
| `--features film` | 490 passed, 3 ignored |
| `phototools-desktop` | 26 |
| `rollchrome-desktop` | **28** (the two publishing-folder guard tests) |
| `rollchrome-cli` | 8 |
| Every Rollchrome `check:*` | pass, `check:develop` 29/29 |
| Web and desktop typecheck, build, `check:transport` | pass |
| Web `check:layout` | **10 routes** clean at 390 px |
| Web `check:ingest` | 400 shots: 27.6 ms filter, 11.4 ms a scrolled frame |

**The location path:** none of the 196 removed tests mentions sync, timeline or OwnTracks. All the timeline
and sync tests are in the after list and pass.

**Unverified:**
- the NAS deployment with the new Compose file;
- a real sync from the Mac after the rename.

That is MV-18, unchanged in meaning.

## Review

**Approved**, with the caveat that **the reviewer wrote the code**, so this is not an independent review.

**What was checked**
- **Gates:** run on the branch.
- **G7:** the test diff accounts for every disappeared test against removed code, and the three edits to
  kept tests are stated with their reasons.
- **The location path:** routes, commands and the client survive, and their tests run.
- **G9:** `SPECIFICATION.md` is untouched, and the difference is recorded in `known-gaps.md`.
- **G11:** the one rename (`ServerClient`) is in the brief's spirit: the old name described a removed
  feature.
- **Old configuration still loads:** `Config` does not deny unknown fields, so a `config.json` with the old
  keys is accepted.
