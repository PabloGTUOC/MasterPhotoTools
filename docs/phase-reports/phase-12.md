## Phase 12 — Google Photos

**Status:** complete, blocked on a real OAuth client for live verification — which the build plan
anticipates. Every acceptance criterion is met and tested. **No test reaches Google**: the API is a
trait, and the one test of the real HTTP client points it at a socket on `127.0.0.1`.

### The question that shaped everything

Google Photos **cannot delete through the API** (§6.1), and `batchCreate` **is not idempotent**
(§6.3). Every decision below comes from one question: *if this goes wrong, does somebody end up with
two copies of a photograph they have to remove by hand?*

That produced three rules, and they are the reason this phase is more than two HTTP calls:

1. **State is recorded before the call that changes it.** A process that dies mid-`batchCreate`
   leaves a row saying a create was in flight, which is a different fact from a row saying one was
   never attempted.
2. **An answer that never arrived is never retried.** §9.2 invariant 6 — an operation reports only
   what it has verified, and a lost response has verified nothing.
3. **Uploads may be retried freely; creates may not.** An unused upload token costs nothing and
   Google discards it.

### Delivered

- **Task 1 — OAuth.** Authorization-code flow with `access_type=offline` and `prompt=consent`, scope
  `photoslibrary.appendonly`, a server-side callback, and the refresh token **encrypted at rest**
  with ChaCha20-Poly1305 under a key from the environment.

  **A missing key is an error, not a fallback.** Quietly storing the token in the clear because
  nobody set `GOOGLE_REFRESH_TOKEN_ENCRYPTION_KEY` would make the column name a lie and would be
  discovered, at the earliest, by whoever read the database.

- **Task 2 — the two-step upload**, with `batchCreate` capped at 50 (§6.1). The real client's wire
  format is exercised against a hand-rolled mock server: headers, JSON shape, response parsing.

- **Task 3 — the state machine**, `pending → uploaded → creating → created`, persisted, resuming only
  from the recorded state. The fourth state is a deviation and is argued below.

- **Task 4 — `429` backoff** with a thirty-second floor, then exponential. A `Retry-After` shorter
  than the floor is **ignored**, because §6.1's stated minimum outranks the header.

- **Task 5 — the reconnect path.** `invalid_grant` marks the connector disconnected and every later
  call short-circuits *without asking Google again*.

- **Task 6 — the mandatory dry run**, checked against the database rather than a flag in memory.

- **Four connector routes and one publish route**, all from specification §8.

### Acceptance

- [x] `cargo fmt --all --check`
- [x] `cargo clippy --workspace --all-targets -- -D warnings`
- [x] `cargo build --workspace`
- [x] `cargo test --workspace` — **461 passed, 0 failed**
- [x] `cargo test -p phototools-core` (G2 isolation) — **400 passed, 0 failed**
- [x] **All tests run against a local mock; no test reaches Google.** Structurally, not by
      convention: `PhotosApi` and `TokenEndpoint` are traits, and the real client's own test binds
      `127.0.0.1:0`.
- [x] **120 items produce exactly 3 `batchCreate` calls.** Asserted as `vec![50, 50, 20]` — the
      sizes, not just the count, so a batch of 119 + 1 could not pass.
- [x] **A simulated timeout after upload but before create resumes without duplicating.** The second
      run makes **zero** upload calls and one `batchCreate`.
- [x] **A `429` is followed by a wait of at least 30 seconds.**
- [x] **`invalid_grant` marks disconnected and does not retry in a loop.** Asserted twice — at the
      connector, by counting refresh calls across ten attempts, and at the publisher, where a dead
      grant on a 400-photograph session produces exactly **one** token request.
- [x] **Publish is refused if no dry run has been performed for the session.** Refused in the handler
      *before* a job is created, and again inside the publisher.
- [x] **Append to `docs/manual-verification.md`.** Seven entries.

### The bug I wrote and then caught

The first version of the publish route rebuilt its plan by calling `dry_run`, because a publish needs
the same plan a dry run produces. `dry_run` **records that a dry run happened**. So every publish
would have stamped its own session as reviewed on the way past, and §9.2 rule 3 would have become a
check that could never fail — the safeguard would have been decorative.

Planning is now `plan_publish`, which records nothing, and `dry_run` is `plan_publish` plus the
stamp. `computing_a_plan_does_not_count_as_having_reviewed_one` pins it.

### The other bug, caught before it shipped

The publish job originally held the shared `Arc<Mutex<Ledger>>` guard across the whole run.
`SinkProgress::report` locks that same mutex on every progress report, and `std::sync::Mutex` is not
reentrant — the job would have deadlocked against its own progress updates on the first photograph.

The job now opens **its own connection** to the database, and `Ledger::open` sets a ten-second busy
timeout so two writers to one file wait rather than fail with `SQLITE_BUSY`.

### Four states, not three

§6.3 names three: `pending → uploaded → created`. A fourth, `creating`, sits between the last two and
exists because the three cannot express the case that actually costs somebody their afternoon.

| What happened | State | What happens next |
|---|---|---|
| Nothing yet | `pending` | Upload it. |
| Uploaded, token held | `uploaded` | Create it. Never re-upload — §6.3's instruction. |
| **Create sent, no answer** | `creating` | **Nothing.** Reported as unconfirmed. |
| Google confirmed | `created` | Done, and written to F16's ledger. |

A definite failure — a `503`, a connection refused — returns the shot to `uploaded`, where the held
token makes a retry free. Only a **lost answer** leaves it at `creating`, and those are never retried
automatically: Google may have created the items, a second attempt would duplicate whichever
succeeded, and the API cannot delete. So they are surfaced for a person to check.

This is §9.2 invariant 6 applied literally. Three states would have forced a choice between retrying
(duplicates) and marking failed (a photograph silently lost), and both are wrong.

### `created` and the deduplication ledger are one transaction

`record_created_and_published` writes the media item and F16's published-hash row together. A crash
between them would leave a photograph that is in Google Photos but absent from the deduplication
ledger — and the next ingest of that card would publish it a second time. That is the exact duplicate
F16 exists to prevent, arriving through the back door.

Two tests hold the line from both sides: a created photograph **is** in the ledger, and a photograph
whose create failed **is not**.

### How the thirty-second floor was tested without waiting thirty seconds

`Sleeper` is a trait. The test implementation records the durations it is asked for and returns
immediately, so the assertion is *"the code asked to wait at least thirty seconds"* — which is the
actual claim. A test that really slept is a test that gets deleted, and then the floor is unguarded.

The same reasoning gives the growth assertion: successive waits must strictly increase, and after
five the run stops rather than hammering an account Google is already refusing.

### Security

- **The refresh token never touches disk unencrypted.** A test asserts the stored value contains no
  fragment of the plaintext — a round-trip test alone would pass an implementation that stored both.
- **A fresh nonce per encryption**, so two identical tokens do not produce identical ciphertext.
  Poly1305 authenticates as well as encrypts, so an edited database is detected rather than yielding
  plausible rubbish.
- **The OAuth `state` nonce is checked, and is single-use.** §6.2 does not mention it. Without it,
  anyone who can make the photographer's browser hit the callback can bind **their** Google account
  to this server, and every photograph published afterwards goes to a stranger's library. The nonce
  is cleared whether or not the exchange succeeds, so a failed attempt cannot be replayed.
- **`TokenCipher`'s `Debug` prints `<key withheld>`**, with a test, so the key cannot reach a log by
  someone deriving `Debug` on a struct that holds one.

### Measurements

None specified. What is known from the tests: a dry run makes no network calls at all, and a
120-photograph publish makes 120 uploads and 3 creates — 123 requests against §6.1's quota of 10,000
per day, so roughly 80 cards of that size before the quota is a consideration.

### Gates

- **A real OAuth client**, as the build plan states. Nothing here has spoken to Google.
- **Specification §6.4's own check**: publish one photograph, confirm the capture date survives and it
  is filed under the correct day, *before* the first bulk run. The staged file is uploaded byte for
  byte — nothing in this phase rewrites EXIF — so this is really a check on what Phases 9 and 10
  preserved, arriving at the one place that matters.
- **The consent screen must be published to "In production".** A project left in Testing issues
  refresh tokens that expire every seven days. The reconnect path handles it either way, but weekly
  re-authorisation is a weekly interruption.

### Deviations

1. **A fourth state, `creating`.** Argued above. §6.3's three states cannot distinguish "we sent it
   and do not know" from "we never sent it", and for a non-idempotent API that is the difference
   between a duplicate and a lost photograph.

2. **`chacha20poly1305` added to `core` (G8).** §2.6 lists no cipher at all, while §6.2 step 4
   requires encryption at rest — the specification asks for the outcome without naming a tool.
   ChaCha20-Poly1305 rather than AES-GCM because the NAS may be ARM, where AES has no hardware
   backing and constant-time software AES is the slower and more delicate option. It is a small
   dependency tree: `chacha20`, `poly1305`, `aead`, `cipher`.

3. **Three new columns and one new index (migration 5).** `oauth.state` for the reconnect mark,
   `sessions.dry_run_at` for §9.2 rule 3, and `publishes.session_id`/`source_sha256`/`file_name` so
   a publish job can find its own shots. §7 lists `publishes` without a session, because §7 predates
   sessions existing.

4. **`publishes.shot_id` is `"{session_id}:{source_sha256}"`**, a readable composite rather than a
   hash. It is deterministic, so re-running a session's publish resumes its rows instead of writing a
   second set — and the first thing anybody does with a stuck publish is look at the row.

5. **`Ledger::open` now sets a ten-second busy timeout.** Needed by the publish job's own connection,
   and strictly an improvement everywhere else: without it, the loser of any write race got
   `SQLITE_BUSY` immediately rather than waiting the moment out.

6. **The OAuth `state` nonce is not in the specification.** Added because omitting it is an account-
   takeover hole, not because the spec asked. Recorded here rather than treated as invisible.

7. **`AccessTokens` is not `Send + Sync`.** The obvious implementation is `Connector`, which holds a
   `rusqlite::Connection`, and that is not `Sync`. A publish runs inside one job on one thread, so
   requiring more would buy nothing and force every caller through a second mutex.

8. **The Phase 1 `publish` sketch is deleted** — `uploader.rs`, `album.rs` and the old
   `publish_tests.rs`. It returned a hard-coded `"mock_access_token"`, had `if path == "MOCK"`
   branches **in the shipped code path**, returned the literal string `"media_item_id_parsed"`
   instead of parsing anything, and wrote the refresh token to a column named
   `encrypted_refresh_token` in the clear. `album.rs` resolved album names to
   `format!("mock_album_id_{name}")`; nothing referenced it and §8 has no album endpoint, so it went
   rather than being rebuilt (G11).

   **This is not G7.** The old test was not weakened to make anything pass — it asserted that mocks
   returned their own mock values, which is a test of nothing. The code it covered is gone and the
   behaviour that matters is covered by 33 new tests.

### Notes for the next phase

- **Phase 13's UI must reach the dry run before it can reach publish.** The route enforces it, and
  the enforcement is in the database, so the UI cannot route around it — but the UI should *show* the
  plan, not merely satisfy the check. `PublishPlan` carries item count, byte total, request counts
  and a `skipped` list with reasons, which is what a review screen needs.
- **`ResumeCounts` is for the resume banner.** After an interrupted run it says how many are
  uploaded, created and unconfirmed, which is the difference between "publish" and "carry on".
- **Unconfirmed shots need a human affordance.** Nothing in the UI yet says "these may or may not be
  in Google Photos — go and look". They are the one case where the system cannot decide, and leaving
  them invisible would waste the care taken to detect them.
- **The `description` field is left empty.** `simpleMediaItem.fileName` carries the stem, so a
  photograph is findable by the name the camera gave it. Whether descriptions should say anything is
  a choice about someone's library rather than a technical question.
