# Known gaps

Things that are open in the code, as opposed to things awaiting a human — those are in
[`manual-verification.md`](manual-verification.md).

Nothing here is a `todo!()` or a silently swallowed error on a shipped path (G10). Each is a real
limitation, recorded where it was found rather than left to be rediscovered.

---

## Performance, measured

### §9.1 — the four performance targets, per machine

Re-measured 2026-08-20 in a release build on an Apple Silicon Mac. All four are met **there**;
that is a statement about a machine, not a verdict about the project.

| Target | Budget | Measured |
|---|---|---|
| 400-frame card scan | < 10 s | **32 ms** |
| Date scan of 500 library files | < 5 s | **8.4 ms** |
| Contact sheet from 200 images | < 20 s | **749 ms** |
| Resize and encode one 24 MP JPEG | < 150 ms | **97.8 ms** |

Two of those had never been measured. §9.1 states four targets and only the card scan and the
resize had benchmarks; the date scan and the contact sheet were added when this was re-measured.
All four assert only in a release build and print in debug — the targets describe optimised code,
and a debug figure is evidence of nothing.

**This entry previously read "Target 150 ms. Actual 203 ms", reported as a missed criterion.** That
figure was honestly taken, but on the Linux container the earlier phases were built on. It is 97.8
ms on the Mac the ingest actually runs on. Nothing was optimised to achieve that — the hardware
differs. If the number matters for the server, it needs measuring on the NAS.

The history is worth keeping: it was 463 ms with the `image` crate's encoder, and the mozjpeg
change ([`phase-reports/encoder-change.md`](phase-reports/encoder-change.md)) took 260 ms out of it.

The card scan's 32 ms is on 64×48 fixtures rather than camera files. **MV-8.7** exists to get the
real figure and warns that a slow reader may exceed the ten seconds.

---

## Missed acceptance criteria

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

### The application icons are placeholders

`crates/desktop/icons/icon.icns` and `icon.ico` are **0-byte files** and `icon.png` is a single
pixel. Referencing the empty `.icns` aborts the application at launch on macOS — Tauri sets the
dock icon through `NSImage::initWithData(..).expect("creating icon")`, and a panic there cannot
unwind — so `bundle.icon` lists only the three valid PNGs, which are blank but well-formed.

That is enough to run and to bundle, and not enough to ship: a `.dmg` wants a real `.icns`, and the
window and dock currently show nothing. Closing it needs a square source PNG — 1024×1024 — after
which `cargo tauri icon <file>` regenerates the whole set. What the icon should *look* like is a
decision about the product rather than a packaging task (G11), so it is recorded rather than
invented. Blocks **MV-14.1**.

---

## Places the implementation goes beyond the specification

Recorded because a reader comparing the two should not have to work out which
of them moved.

### F7's canvas is configurable, where the specification fixes it

§F7 describes "a fixed white canvas": 3000 px wide, a 50 px minimum margin, a
2% corner radius. The word doing the work is *fixed* — the reason for fixing it
is that a set of prints then looks like a set, and every post is framed
identically.

`BorderStyle` now carries the canvas colour, width, margin and radius, and the
UI offers all four. **The defaults are the specification's values**, so an
untouched run produces exactly what §F7 describes, and a test asserts that.
What the specification guaranteed, the operator now has to choose to keep.

Requested deliberately. One implementation detail worth recording: the rounded
corners are blended against the canvas colour rather than against white, which
the fixed version could hardcode. A dark canvas blended against white shows a
pale fringe around every photograph, and that is the only place the change is
visible if it is done carelessly — a test covers it.

### Dependencies beyond §2.6

G8 asks for the reason, not just the addition.

| Crate | Where | Why |
|---|---|---|
| `base64` | `server`, `desktop` | F4's preview carries three images across a process boundary — an HTTP response or a Tauri command result — and both front ends put them straight into an `<img src>`. Data URLs avoid a second round trip per image, and a preview that needed three more requests for something looked at once would be worse. |
| `image` | `server`, dev only | One test builds a two-panel fixture with a known divider so the preview route can be asserted against a real image rather than a fabricated JSON body. |
| `tauri-plugin-autostart` | `desktop` | Launch at login (build plan Phase 14). §2.6 names no mechanism and the macOS one is a LaunchAgent. |

All three were already in `Cargo.lock` through existing transitive dependencies, so none adds a
tree that was not being compiled.

### F5 has a film-strip layout the specification does not describe

§F5 asks for a grid proof sheet: uniform cells, filename captions, a
configurable column count. That is still what `ContactSheetParams` defaults to
and what every F5 test asserts.

Added beside it is a `Filmstrip` style — frames laid out as cut strips of 35mm
with rebate, perforations and frame numbers printed on the edge, five frames to
a strip. The geometry is the real film's, scaled from the frame width: 36×24 mm
image, 5.5 mm rebate, 2 mm between frames, perforations 2.79×1.98 mm on a
4.75 mm pitch. Strips are cut to the frames they hold, so a roll of four does
not leave a fifth slot of bare film.

**The desktop and web UI default to it**, which is the part that departs from
the specification rather than merely extending it — the underlying tool still
defaults to `Grid`. Requested deliberately; recorded here so the divergence is
visible rather than discovered.

Two decisions inside it worth knowing. A portrait frame is fitted whole into
the 3:2 gate with film base either side rather than cropped to fill, because a
proof sheet that crops is one you cannot judge a frame from. And the
perforations run at a fixed pitch independent of where the frames fall, which
is how film is manufactured and why they do not line up with the frame edges.

---

## Places the specification is incomplete or contradicts itself

Recorded rather than resolved by editing it (G9).

### §9.3's `distroless/cc` base cannot run §2.6's `exiftool`

§9.3 specifies a `distroless/cc` base for the server image. §2.6 makes `exiftool` the one permitted
external binary and requires it for every metadata **write**, and two shipped server routes perform
one — `POST /api/tools/dates/fix` (F1) and `POST /api/ingest/derive` (F14).

`exiftool` is a Perl program. `distroless/cc` carries a C runtime and no interpreter, so on that
base both routes fail at their first write: a capability the API offers and the image cannot honour.

The image is `debian:bookworm-slim` instead, recorded in
[`phase-reports/phase-14.md`](phase-reports/phase-14.md). The alternative was withdrawing two
specified routes from the server build, which is a larger decision than a base image.

### §2.2 has the server serving the web UI, and no phase implemented it

"Web UI | Vue 3 | **Served by `phototools-server`**" (§2.2), but the router had no static route, and
`tower-http`'s `fs` feature was enabled in `crates/server/Cargo.toml` and unused — so the front end
existed, was built by CI, and had nothing to serve it outside Vite's development server.

Closed in Phase 14 rather than left: the deployed container is the first place it would have
mattered, and it would have read as "the deployment is broken" rather than as an unimplemented line
of the specification.

### F7 trims a pixel from every edge, trimmed or not

§F7's step 1 reads "up to a maximum of 40 px, plus a 1 px safety inset".
`trim_dark_edges` applies that inset to all four sides unconditionally, in the
`Bounds` it returns — so a photograph with no dark edge anywhere still loses one
pixel from each side. A test pins it deliberately (`bounds.right == 59` on a
60-wide buffer with nothing dark on the right), so this is somebody's reading of
the wording rather than an oversight.

It was invisible while the canvas was fixed, because the photograph is rescaled
to fit regardless. It is visible now: `ImagePlusMargin` promises the photograph
untouched, and with the trim on the output is two pixels smaller in each
dimension than the arithmetic says.

Both readings of "plus a 1 px safety inset" are defensible — a safety margin
around a trim that happened, or an unconditional inset. Left as it is, and the
inset can be avoided by turning the trim off. Deciding it belongs with whoever
owns the specification (G9).

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

Nothing. Phases 0–14 are built.

What remains is [`manual-verification.md`](manual-verification.md) — the checks
needing a Mac, a camera, a NAS, a Google account or somebody's judgement.
