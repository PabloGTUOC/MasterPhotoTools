# Known gaps

Things that are open in the code, as opposed to things awaiting a human — those are in
[`manual-verification.md`](manual-verification.md).

Nothing here is a `todo!()` or a silently swallowed error on a shipped path (G10). Each is a real
limitation, recorded where it was found rather than left to be rediscovered.

---

## Missed acceptance criteria

### §9.1 — 24 MP resize and encode, 150 ms

**Target 150 ms. Actual 203 ms.** Reported rather than relaxed.

Was 463 ms with the `image` crate's encoder; the mozjpeg change
([`phase-reports/encoder-change.md`](phase-reports/encoder-change.md)) took 260 ms out of it and
still misses by 53 ms. The remaining cost is split between decode and resize rather than encode, so
closing it means attacking a different part of the pipeline than the one already replaced.

### F1 — four of seven date tags cannot be read from a file

`nom-exif` 3.6 exposes `EXIF:DateTimeOriginal`, `EXIF:CreateDate` and a single collapsed QuickTime
`CreateDate`. It does **not** separately surface `QuickTime:CreationDate`, `Keys:CreationDate`,
`XMP:CreateDate` or `QuickTime:ModifyDate`.

F1's preference order is implemented and unit-tested at all seven positions; positions 3, 5, 6 and 7
simply cannot be populated from a real file. Whether that matters in practice is **MV-2.2** — if the
resolved date is right anyway, this stays a limitation; if it is not, it becomes a defect.

---

## Seams that work but grate

### No session discovery

Specification §8 lists no endpoint that enumerates ingest sessions, so after a handoff the desktop
reports the session id in its handover panel and **a person copies the UUID into the web publish
screen**.

It works. It will grate on the fortieth card. The fix is a `GET /api/ingest/sessions` returning
recent sessions with their state — a small endpoint, but one the specification does not have, so
adding it is a decision rather than an omission (G11). See **MV-13.4**.

### Nothing cleans the staging directory

Staged derivatives stay on the NAS after publishing. That is deliberate — it is what lets
`already_staged` work across restarts, so an interrupted publish does not re-transfer gigabytes — but
a NAS accumulating every card ever ingested is not a plan.

Deleting safely needs the ledger to know a file is published *and* that nothing else references it.
That is a retention policy, not a bug, and it belongs with Phase 14's deployment decisions.

### The desktop has no Firebase sign-in

`crates/desktop/src/credentials.rs` stores a refresh token in the macOS Keychain, but nothing
exchanges it for an ID token. The handoff therefore authenticates with §5.3's documented break-glass
`ADMIN_TOKEN`, carried in `ServerSettings.auth_token`.

This is the one thing standing between the handoff and working end to end, and it is configuration
rather than code — see **MV-11.1**. When sign-in is wired, the same field carries the ID token and
nothing else changes.

---

## Deliberate limits

### CR3 is declined

F14 puts Canon's CR3 out of scope: it is an ISO-BMFF container rather than a TIFF-based one and needs
separate handling. `RAW_EXTENSIONS` omits it and a test asserts the omission. A CR3 on a card is
reported as underivable — **MV-10.5** confirms that reads as a clear message rather than a crash.

### Rung 3 has never produced an image in a test

A RAW stub with no embedded preview also has no sensor data, and synthesising a real mosaic means
implementing a camera's raw format — a much larger piece of work than the rung it would test. What
*is* asserted is that rung 1 correctly declines and the ladder moves on. **MV-10.4** is the judgement
about whether rung 3's output is usable at all.

### Rung 2 is compiled out on Linux

`sips` is invoked only under `#[cfg(target_os = "macos")]`. Away from macOS the rung reports "not
applicable" rather than failing, which is why rung 3 exists. **MV-10.3**.

---

## Places the specification is incomplete or contradicts itself

Recorded rather than resolved by editing it (G9).

### §7 lists neither the `published` nor the `sessions` table

F16 asks for "a SHA-256 ledger of every file it has published" in as many words, and §8 has session
URLs — but §7's data model has neither table. §7 does list `publishes`, which is keyed by `shot_id`
and holds Phase 12's state machine; a key that is a shot on one particular card cannot answer *"have
I published this photograph before?"* for a card that has been reformatted since.

Both tables exist, added in migrations 4 and 5.

### §2.6 and F14 disagree about `sips`

§2.6 calls `exiftool` "the one permitted external binary". F14 explicitly permits `sips` for the
macOS ImageIO rung. Taken as F14 winning, being both more specific and later — but if §2.6 is meant
absolutely, rung 2 has to become `objc2` FFI bindings, which is real work and untestable until it
runs on a Mac.

### §6.3's three publish states cannot express "sent, no answer"

`pending → uploaded → created` has no room for a `batchCreate` that left and whose response never
came back. A fourth state, `creating`, was added: the row is written **before** the call, so a
process that dies mid-flight leaves evidence.

Three states would have forced a choice between retrying (duplicating any photograph Google did
create, which the API cannot delete) and marking it failed (losing one silently). Both are wrong, so
the shots sit as unconfirmed for a person to check — §9.2 invariant 6 applied literally.

---

## Not yet built

**Phase 14 — packaging and deployment.** Multi-stage `Dockerfile` on `distroless/cc` with
`cargo-chef`, `docker buildx` for `linux/amd64` and `linux/arm64`, `docker-compose.yml` with a health
check, the `.dmg` bundle, and `docs/deployment.md`.

One thing to know going in: a full debug `target/` reaches around **20 GB**, most of it `rawler` and
rustls' `aws-lc-sys`. Without `cargo-chef` caching configured from the first commit, every image
build recompiles that tree.
