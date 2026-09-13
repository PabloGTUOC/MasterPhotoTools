# Timeline sync — one timeline, two machines

**Status:** built, gates pass. Eight checks await both machines (MV-18).

Not a numbered phase. Planned in [`timeline-sync-plan.md`](../timeline-sync-plan.md) at the user's
request and reported here on the same terms as a phase.

---

## What it does, and what it refuses to do

The Mac and the NAS each hold a library; neither is the authority; both must work when the other is
unreachable. Sync carries **geopositions only** — tracks, the fixes they contribute, the decisions
recorded about instants in dispute, and deletions.

Cards, shots, sessions, publishes, jobs and settings do not move. They are records of *what one
machine did*, and copying them would misrepresent which machine did it. A test asserts the shape of
an inventory for exactly that reason: it is `tracks` and `deletions`, and nothing else.

## Why it is small

Three properties of the existing design did the work:

| | |
|---|---|
| A track's id is the **sha256 of its bytes** | Comparing libraries is comparing two lists of ids. Sync is idempotent by construction |
| The **GPX text is stored** beside the parsed points | A track is self-contained: send the text, and the other side rebuilds the fixes with the same reader |
| `library::commit_import` **already merges** a track into a timeline | A received track goes in by that road. **No new merge rule was invented** |

So the unit of sync is a track, never a point, and the hard part — what happens when two libraries
disagree about one second — was already solved and already tested.

## The shape

| | |
|---|---|
| `geotag::sync` (core) | The diff, the remote as a trait, and the sequence. No clock, no filesystem, no network |
| `Ledger` | `track_stamps`, `track_text`, `tombstones`; `delete_track` now records a tombstone in the same transaction as the deletion |
| Migration 11 | `deleted_tracks (id, deleted_at)` |
| Server | Four routes: inventory, fetch one, accept one, accept deletions. Passive throughout |
| Desktop | `TimelineRemote` over `reqwest` (§8 — HTTP stays on the Rust side), one Tauri command, a sidebar button and an automatic run at startup |

**The desktop drives**, because a NAS cannot open a connection to a laptop that is asleep or on
another network. The web application has no sync button and should not have one.

`TimelineRemote` is a trait for the same reason `ingest::SessionClient` is: the sequence is worth
testing with neither machine present, and the HTTP belongs in the binary (G1).

## What the tests caught

**A received track must keep the date the origin gave it.** The first version stamped the receiving
machine's clock on arrival. Two tests failed — a deletion made after a sync did not propagate — and
the cause is worth writing down: `imported_at` is what a tombstone is compared against, so two
machines holding different dates for one track can make a deletion read as *older* than the copy it
is meant to remove. That deletion then silently never happens, and a clock difference of seconds is
enough to cause it. One date, set once, by the machine that first imported the file.

That is a bug that would have been very hard to find in the field: it fails rarely, silently, and
only for people whose clocks differ.

## Tests

Twenty-six new, 731 → **757** in the workspace and 646 → **669** in core.

- **`sync::plan`** — eleven cases: only here, only there, both, a deletion that travels either way, a
  re-import that beats an older deletion, a deletion both sides already applied, a track whose text
  was never stored (reported, not skipped), and a stable order so two runs read the same.
- **`sync::run` against two real `Ledger`s** — the fake remote is a whole second library, so what is
  asserted is what two timelines actually end up holding: each machine gains what the other had, a
  placed point keeps its provenance across, a second sync moves nothing, deletions travel in both
  directions and stay travelled, a re-import brings a track back on both, and two machines that
  disagree about one instant still hold one position for it.
- **The server's four routes** end to end over HTTP: a track handed over is held afterwards, with
  its provenance and its origin's date; it can be fetched back; a deletion removes it and is
  remembered; every route refuses an unauthenticated request (§5.3); and an inventory carries the
  timeline and nothing else.

## Deviations from the plan

| Plan | Built | Why |
|---|---|---|
| The receiver dates a track when it learns of it | The origin's date is preserved | See above — the plan's version silently loses deletions when clocks differ |
| "Pull, push, then deletions" | As planned, and deletions carry the tombstone's own date rather than "now" | The moment somebody deleted a track is what a later re-import has to beat |

## Not built, deliberately

A background timer, automatic retries, and any notion of more than two machines. Startup plus a
button covers the case, and a silent retry loop against a NAS that is off is how logs fill up with
nothing.
