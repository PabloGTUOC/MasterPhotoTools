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

*(to be filled)*

## Review

*(to be filled)*
