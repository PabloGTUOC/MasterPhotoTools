# Timeline sync — one timeline, two machines

> **Built, and the gates pass**; MV-18 awaits both machines, and the report is in
> [`phase-reports/timeline-sync.md`](phase-reports/timeline-sync.md). One thing below was corrected
> in the building and the text says so where it happens: a received track keeps the date the machine
> that first imported it gave it, rather than being stamped on arrival.

The Mac and the NAS each hold a timeline, and today they are strangers: two `.gpx` files imported on
the Mac are invisible to the web application, and a point placed in the browser never reaches the
Mac. This is the plan for making each one carry what the other knows, **without merging the two
databases into one** — because the Mac has to keep working when the NAS is off.

---

## What syncs, and what deliberately does not

| | |
|---|---|
| **Syncs** | `tracks`, the fixes they contribute, and the conflict decisions recorded against them — the timeline, and nothing else |
| **Does not** | Cards, shots, sessions, publishes, jobs, settings. They are records of *what one machine did*, and copying them would be a lie about which machine did it |

Photographs are not touched. Sync moves the evidence about where somebody was; the Geotag tab is
what writes that into a file, on whichever machine holds the photographs.

---

## The three facts that make this tractable

1. **A track's id is the sha256 of its bytes.** The same `.gpx` imported on both machines already has
   the same id on both. Sync is therefore idempotent by construction, and "do they have the same
   tracks?" is a set comparison of ids, not a diff of a million rows.
2. **The GPX text is stored beside the parsed points** (`tracks.gpx`). A track is self-contained:
   send the text and the receiver can rebuild the fixes by the same reader that read it originally.
3. **`library::commit_import` already merges a track into a timeline** — new points in, instants
   already held reported as conflicts, decisions recorded. A received track goes in by that road.
   **No new merge rule is invented**, which is the whole reason this is a small feature rather than a
   distributed-systems problem.

So the unit of sync is a **track**, not a point; and the merge is the import that already exists.

---

## Topology: the Mac drives

The NAS cannot open a connection to a laptop that is usually asleep, on another network, or behind
somebody else's router. The Mac already knows where the server is (`ServerSettings`) and already
authenticates to it. So:

**The desktop pulls, then pushes.** The server is passive and answers questions. The web application
needs no sync button at all — by the time a browser opens, the NAS already has whatever the Mac
gave it.

This also settles what "when the desktop gets on" means: the desktop syncs when it starts and the
server answers, and whenever somebody presses the button.

---

## SY-1 — the diff, in `core`

A pure function, so it can be tested without either machine:

```rust
pub struct Inventory {
    pub tracks: Vec<TrackStamp>,      // id, imported_at, point_count
    pub deletions: Vec<Tombstone>,    // id, deleted_at
}

pub struct SyncPlan {
    pub pull: Vec<String>,            // ids to fetch from the server
    pub push: Vec<String>,            // ids to send to the server
    pub delete_here: Vec<String>,
    pub delete_there: Vec<String>,
}

pub fn plan(local: &Inventory, remote: &Inventory) -> SyncPlan;
```

Stateless: no "last synced at" cursor to be wrong, corrupted, or reset. Each side reports what it has
and what it has deleted, and the difference is the work. An inventory of a thousand tracks is a few
tens of kilobytes.

## SY-2 — deletions have to be remembered, or they come back

Additive sync resurrects: delete a bad track on the Mac, sync, and the NAS hands it back.

So a migration adds **`deleted_tracks (id TEXT PRIMARY KEY, deleted_at INTEGER)`**, written by
`delete_track` on both machines. The rule between the two sides:

- A tombstone **younger** than the other side's copy of the track deletes it there.
- A track imported **again after** a tombstone wins, and the tombstone is dropped. Somebody who
  re-imports a file they deleted last week means it.

Both timestamps come from the machine that acted, and the comparison is only ever "which of these
two happened later".

**Corrected in the building:** a received track keeps the `imported_at` the origin gave it rather
than the receiver's clock. Two machines holding different dates for one track can make a deletion
read as *older* than the copy it is meant to remove — a deletion that then silently never happens,
on a clock difference of seconds. Two tests failed on it before the change; see the report.

## SY-3 — four endpoints on the server, four commands on the desktop

| Endpoint | Answers |
|---|---|
| `GET /api/timeline/inventory` | Track stamps and tombstones. Cheap: no points, no GPX text |
| `GET /api/timeline/tracks/:id` | One track: its metadata, its GPX text, and the conflict decisions recorded against it |
| `POST /api/timeline/tracks` | Accept one track, by the same import road a file takes |
| `POST /api/timeline/deletions` | Accept tombstones |

The desktop's side is `reqwest` in `crates/desktop/src/server.rs`, where its other HTTP already lives
(§8 — the webview never makes these calls). Authentication is the existing bearer token; when
Firebase sign-in reaches the desktop, nothing here changes.

**One track is one request and one transaction.** A sync interrupted halfway leaves both sides
consistent and the next run finishes the job.

## SY-4 — what a received track does on arrival

`library::commit_import`, with the **decisions the origin recorded** passed as overrides.

That last part is what makes the two sides converge rather than merely overlap: where a person chose
to keep an existing fix over an imported one, that choice travels with the track instead of being
re-decided by whichever machine happens to import second. Where no choice was made, both sides apply
the same default — keep what is held, record the conflict — and reach the same answer because the
diff is deterministic.

**`source` travels too**: a point placed by hand on the Mac arrives on the NAS still saying *placed
by hand*, and Google's inferences (Stage B of [`timeline-plan.md`](timeline-plan.md)) will arrive
still saying *inferred*. A sync that laundered provenance would undo the point of having it.

`source_path` is kept as the origin wrote it and shown as what it is: the path **on the machine that
imported it**. `/Users/pablo/Desktop/TestIn/GEO/track.gpx` is not a path on the NAS and must not
pretend to be.

## SY-5 — when it runs, and what it says

- **On startup**, if the server answers its health probe within the existing three-second timeout.
  Never blocking: the application opens whether or not the NAS is there.
- **On a button**, in the desktop's sidebar rather than inside the Timeline tab: the sync is the
  application's business with the server, not one screen's, and its last result belongs where it can
  be read from any of them.
- **Never automatically on the web side.** The server is passive.

Afterwards, one line in plain words: *pulled 2 tracks, sent 1, 3 conflicts recorded, nothing
deleted*. A sync that says "done" and moved nothing is indistinguishable from a sync that failed
silently, which is the failure this project has met before.

Not built: a background timer, a watcher, or retries. The startup attempt and the button cover the
case, and a silent retry loop against a NAS that is off is how logs fill up.

## SY-6 — the checks a person has to run

New **MV-18** items:

| | |
|---|---|
| MV-18.1 | A `.gpx` imported on the Mac appears in the web Timeline after a sync, with the same fixes and the same *recorded* label |
| MV-18.2 | A point placed in the browser reaches the Mac's Timeline, still labelled *placed by hand* |
| MV-18.3 | Deleting a track on one side removes it on the other, and does not come back on the next sync |
| MV-18.4 | Re-importing a file deleted last week keeps it: the tombstone loses to the newer import |
| MV-18.5 | Two tracks that disagree about an instant produce the **same** kept fix and the same conflict record on both machines |
| MV-18.6 | The NAS being off is a sentence, not an error, and the application works exactly as before |
| MV-18.7 | A sync interrupted midway (network pulled) leaves both sides consistent and finishes on the next run |

---

## Considered and rejected

**One library, over HTTP: the desktop asks the NAS for everything.** Simplest to describe and wrong
here — it breaks the case the desktop exists for. §2.3 puts the card reader on the Mac, MV-7.3
expects the application to work with the server off, and a laptop on a train would lose the Geotag
tab entirely.

**Replicating the SQLite file** (rsync, Litestream, a shared volume). The ledger holds rows that
belong to one machine — cards, staging paths, jobs — and file-level replication has no merge: the
last writer wins and the other machine's afternoon is gone. The database is shared *state*, not a
shared *file*.

**Syncing points rather than tracks.** It would work, and it throws away the two things that make
this cheap: the content-hash identity, and an import path that already knows how to merge. It also
loses the GPX text, which is the provenance somebody will one day want to see.

---

## Known limits, said now rather than discovered later

| | |
|---|---|
| **A track with no stored text cannot be pushed** | Stage C of the timeline plan caps `tracks.gpx` for enormous imports. Such a track syncs its *stamp* and is reported as "too large to send", not skipped silently |
| **Conflicts are recorded per machine** | Both sides reach the same timeline; the audit rows read "decided here" or "decided there" and the id of the origin |
| **No three-way merge** | Two machines, one direction of travel each way. A third machine would work, but nothing is designed for it |
| **The timeline only** | Photographs, cards and publishes stay where they were made |
