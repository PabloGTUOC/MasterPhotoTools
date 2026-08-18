## Phase 11 — Staging handoff and ledger

**Status:** complete with one gate. Every acceptance criterion is met and tested, at both the
protocol level and over real HTTP. The gate is not code: the desktop has no Firebase sign-in, so the
handoff authenticates with the specification's break-glass token, which somebody has to set.

### Delivered

- **Task 1 — the manifest.** Per shot: stem, two content hashes, staged file name, dimensions, byte
  count and capture date. It carries **no local paths** — it is written on the Mac and read on the
  NAS, where `/Users/pablo/…` means nothing and leaks the photographer's directory layout for
  nothing. A test asserts the wire format contains no path from the machine that built it.

- **Task 2 — pre-flight deduplication.** `POST /api/ingest/sessions` takes the manifest and answers
  before a single photograph moves. It reads no image data at all: one indexed query for the whole
  manifest plus a `stat` per entry, so a 400-frame card is answered in milliseconds. That is what
  makes asking first worth doing.

  The ordering claim is asserted directly rather than inferred from the end state — see below.

- **Task 3 — the staging writer and arrival verification.** The desktop copies through
  `<name>.partial` and renames, the same discipline `ingest::staging` already uses for the card copy
  and for the same reason: the server reads this directory, and a partially written file under its
  final name would be read as an arrival. On arrival every file is hashed against the manifest, and
  a mismatch produces a **recopy request, not a failure** — specification §2.3 says an interrupted
  copy "is simply recopied", so that is what it does, for up to three rounds.

- **Task 4 — the ledger.** A `published` table keyed by SHA-256, written when a photograph reaches
  Google Photos. Phase 12 is what will call it for real; the tests call it directly, which is what
  "already processed" means for a phase that cannot publish yet.

- **One `SessionClient` trait as the seam.** The whole protocol — what to copy, what a mismatch
  means, how many rounds — is in `core`. The desktop supplies two HTTP calls and nothing else (G1).
  That is what lets the entire exchange, recopy rounds included, be tested without an HTTP server.

- **Three routes, all from specification §8**, plus one Tauri command.

### Acceptance

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — **406 passed, 0 failed**
- [x] `cargo test -p phototools-core` (G2 isolation) — **351 passed, 0 failed**
- [x] **Ingesting the same fixture card twice transfers zero bytes the second time and publishes
      nothing.** Twice: once at protocol level, once end-to-end from a real fixture card through
      scan, pairing and rescan.
- [x] **A truncated staged file is detected by hash mismatch and recopied.** Also twice — in `core`
      against the driver, and over HTTP against the real routes.
- [x] **The manifest round-trips through JSON without loss**, with `capture` asserted by name.
- [x] **Append to `docs/manual-verification.md`.** Five entries.

### Two hashes, and why one will not do

The build plan says the manifest carries "the content hash". It carries two, because one hash cannot
do both jobs and each choice breaks the other:

- **`source_sha256`** — the hash of the file on the card. This is the deduplication key, and
  specification §7 names it: *"`assets.sha256` is the authoritative deduplication key"*, where
  `assets` are the card's files. The camera's bytes are never rewritten, so they hash the same on
  every ingest of that card forever.
- **`derived_sha256`** — the hash of the file that crosses the network. It is the only thing that
  can tell a truncated copy from a whole one on arrival.

Deduplicating on the derived hash would make F16 depend on every encoder in the pipeline being
byte-deterministic, and would make **every already-published photograph look new the moment
`MAX_MEGAPIXELS` changed**. Verifying with the source hash is simply impossible — the server never
sees the card. A test pins the resized case specifically.

### Three dispositions, and why two will not do

The build plan describes the reply as "which are new". A binary new/known split has to decide what
"known" means, and both readings lose photographs or bytes:

| "known" means | What breaks |
|---|---|
| *published* | A photograph whose bytes are already on the NAS is copied a second time for nothing. |
| *seen* | A photograph that was staged but whose publish **failed** is never retried. It is silently lost — the exact failure F16 exists to prevent, arriving from the other direction. |

So the reply is `send` / `already_staged` / `already_published`, which separates "do not send the
bytes" from "do not publish it" and answers both. `already_staged` is the interrupted-publish case,
and it has its own test at both levels.

### How the ordering claim was made real

"The manifest goes before the bytes" is F16's actual requirement, and it is a claim about sequence
that no test of the end state can support — an implementation that copied everything and *then*
asked would leave identical files on disk. So the staging directory is inspected **from inside
`open_session`**, at the one moment where "before" and "after" are distinguishable, and the
assertion is that it was empty when the manifest arrived.

### Security

`file_name` is the one field in this protocol where a client names a file the server will write to,
and the server joins it onto the staging directory. A manifest naming `../../etc/cron.d/pwn` is
refused with `403` before anything is read — the same concern G6 addresses for request paths,
at the one place G6 itself does not reach. Tested over HTTP, and at the unit level against five
shapes of hostile name.

The session id is minted by the server rather than taken from the manifest. A client that named its
own session could name one another client is using, and the second `ready` would then verify against
the first one's agreement.

### Measurements

None specified for this phase, and none of the interesting numbers can be taken here — the whole
point of the medium is that it is a network share, and every test writes to a local temporary
directory. What *is* known: `decide` reads no image data, so answering a manifest is one indexed
query plus a `stat` per entry; verification is bounded by disk read speed, which is why it is a job.

### Gates

- **`ADMIN_TOKEN` must be set for the desktop to authenticate.** Everything else is code; this is
  configuration, and without it every handoff request is a `401`. See deviation 4.
- **A NAS and a real SMB share.** Three of the five manual-verification entries are about SMB
  semantics that a local temporary directory cannot exercise — chiefly whether `rename` is atomic
  enough on the share, since the `.partial` discipline depends on it.

### Deviations

1. **Two hashes rather than one**, and **three dispositions rather than two.** Both argued above.
   Both are departures from the build plan's wording, and both exist because the literal reading
   loses photographs.

2. **`published` and `sessions` tables are not in specification §7.** §7 lists `publishes`, but that
   is keyed by `shot_id` and holds Phase 12's state machine; a key that is a shot on one particular
   card cannot answer "have I published this photograph before?" for a card that has been
   reformatted since. F16 asks for something different in as many words — *"the server maintains a
   SHA-256 ledger of every file it has published"* — so the table exists. **§7 is incomplete
   relative to F16 and §8**, which is recorded here rather than fixed by editing the specification
   (G9). §7 also lists no session table while §8 has `/api/ingest/sessions/{id}/…`.

3. **`ready` is a job, and the report is served from `GET .../shots`.** F17 forbids blocking a
   request until work completes, and verification hashes gigabytes off the NAS's own disks. So
   `ready` returns `202` and a job id, the job writes the report onto the session row, and §8's
   existing `GET /api/ingest/sessions/{id}/shots` — *"results with per-check status"* — serves it.
   No endpoint was invented. Phase 13 adds F12's validation checks to the same rows, which is why
   the shape has room for more than one verdict per shot.

   That response also carries the raw `ArrivalReport` alongside the per-shot rows. It is the same
   facts twice, deliberately: the review grid wants rows, the desktop wants an exact list of files
   to recopy, and re-deriving that list by parsing display strings would put a parser between two
   halves of one protocol.

4. **The desktop authenticates with §5.3's administrative token.** `ServerSettings` gained an
   optional `auth_token`. The desktop has a Keychain (`credentials.rs`) but **no Firebase sign-in
   flow**, so there is no ID token for it to send; §5.3 provides for exactly this case — "a
   configured local administrative token provides a documented break-glass path" — and a machine on
   the local network talking to a NAS is the case it describes. When the desktop's sign-in is wired,
   the same field carries the ID token and nothing else changes.

5. **A poll loop rather than `notify` for the "watcher".** The build plan says "watcher on the
   server". What is built is a polling verification pass, triggered by `ready`, and the reason is
   the deployment: inotify watches the local VFS, and whether it fires here depends on a topology
   Phase 14 has not decided yet. If the container bind-mounts a directory that `smbd` writes to
   locally, inotify fires; if the container is itself a CIFS **client**, it does not. A poll works
   in both, needs no dependency, and is about twelve lines. Adding a dependency that silently does
   nothing in half the deployments would be worse than not adding it.

6. **`reqwest`'s `blocking` feature added to the desktop.** Not a new dependency — `reqwest` is
   already there and is named in §2.6 — but a new feature, recorded per G8's spirit. The handoff
   runs inside a job on its own `std::thread`, where a synchronous client is the honest shape; the
   async client stays for the health probe, which is called from Tauri's async command context.

7. **`ingest::derivation::worker` is deleted**, along with `derivation_tests.rs`. This is the loose
   end Phase 10's report flagged. Nothing called it — no route, no command — and it was actively
   dangerous to leave: it hard-coded a 2000 px long edge and quality 95, **ignoring both F12's
   configurable megapixel ceiling and F13's byte cap**, so anything that wired it up would have
   silently produced non-compliant output. `derive_batch` supersedes it and honours both thresholds.

   Its test asserted that a 4000×3000 JPEG downscales with camera and capture date intact; that
   behaviour is covered by `validation_tests.rs::a_24mp_frame_fails_and_resizing_brings_it_under_10mp_with_its_date_intact`
   from Phase 9, which asserts the same facts against the *configured* threshold rather than a
   hard-coded number. **This is not G7**: the test was not failing and was not weakened — the code
   it covered was removed, and its coverage was checked to exist elsewhere before deleting it.

### Notes for the next phase

- **Phase 12 has one call to make and one invariant to keep.** `record_published(source_sha256, …)`
  is the call; the invariant is that it happens **after** Google Photos confirms, never before. A
  row written early is a photograph that is never published and never retried, which is the failure
  mode `already_staged` exists to prevent — writing the ledger optimistically would reintroduce it
  through the front door.
- **`media_item_id` is already nullable** for Phase 12's `pending → uploaded → created` machine: a
  row with no media item is a photograph whose bytes are up but whose item is not yet made.
- **`MAX_RECOPY_ROUNDS` is three and arbitrary.** It is a bounded loop rather than a patient one on
  purpose, but nobody has watched a real share fail. If SMB write caching turns out to need a beat
  before a file settles, the fix is a short wait before re-verifying, not a bigger number.
- **Nothing cleans the staging directory.** Files stay after publishing, which is what makes
  `already_staged` work across restarts — but a NAS accumulating every card ever ingested is not a
  plan. Deletion needs the ledger to know a file is published *and* that nothing else references it;
  that is a decision about retention, not a bug, and it belongs to whoever owns Phase 14's
  deployment.
