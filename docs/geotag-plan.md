# Geotag — development plan

> **Built.** Every step below is done and the gates pass; thirteen checks await a Mac (MV-15).
> Where the build diverged from this plan the text has been corrected in place and the reason is in
> [`phase-reports/geotag.md`](phase-reports/geotag.md) — the estimator, the offset read, the
> writer's method, and one query this plan did not foresee.

Join photographs to the places they were taken, using GPS tracks exported from a phone.

A photograph carries the moment it was made; a track carries where the phone was, moment by
moment. Neither knows about the other, and the only thing they share is time. This is the tool
that closes that join and writes the result into the file, so the location travels with the
photograph into Google Photos, Lightroom, or anywhere else it goes.

Steps have stable ids (`GT-4`), the same way [`manual-verification.md`](manual-verification.md)
numbers its checks, so one can be named in a commit or a conversation.

---

## The flow, as the screen presents it

Three panels, in the order the work actually happens.

**1 · Tracks.** Load a `.gpx`. It is parsed, rendered into the EXIF form it will eventually be
written in, and shown to you *before* anything is saved — new points, points the library already
holds, and any point where this file disagrees with what is already there. You decide on the
disagreements, and only then does it commit to the database.

**2 · Photographs.** Point at a folder. Every file is listed with what it already carries: a
capture date, a location, both, or neither. Nothing is written and nothing is matched yet — this
is the inventory, and it is the answer to "what am I actually missing?"

**3 · Match.** For the rows that have a date but no location, look the moment up in the library,
show what was found and how far in time it was, and write it.

---

## Status of this work against the specification

**`SPECIFICATION.md` does not mention GPS, geotagging, GPX or location anywhere.** There is no
F-number for this and none is invented: a fabricated `F19` would make the code look as though the
specification asked for something it never did.

Handled as the ground rules require:

| | |
|---|---|
| **G9** | The specification is not edited. |
| **G11** | The scope is the user's explicit request, recorded here before any code is written. |
| **G8** | No dependency is added. GPX is parsed in-crate; see [GT-1](#gt-1--the-gpx-reader). |

On completion this gets an entry in [`known-gaps.md`](known-gaps.md) under *Places the
implementation goes beyond the specification*, and a report in
[`phase-reports/`](phase-reports/).

## Decisions already taken

| | Decision | Consequence |
|---|---|---|
| Video | **Skipped**, with a reason on the row | QuickTime GPS is a different write (ISO 6709 into `UserData`) and belongs in its own change |
| RAW | **Written in place** | `exiftool` handles DNG/NEF/ARW/CR2; no sidecar, no second source of truth |
| Matching | **No interpolation at all — carry the last recorded position forward** | The tracker reports when, and only when, its owner moves. There are no missing points to fill in, and a computed coordinate would look exactly as trustworthy as a recorded one |
| Overlaps | **Surfaced, never resolved silently** | Every fix comes from one phone, so two files disagreeing about one instant is a fault, not a tie to break |

---

## The point library

### One position per instant

The library is a **single timeline**, not a pile of files. `at` — the instant, as Unix seconds
UTC — is the primary key of the points table, so "one position per instant" is enforced by the
schema rather than remembered by code. That is what makes the overlap rule expressible at all:
a second file offering a different position for a second already in the timeline is a primary key
collision with a decision attached.

It also removes something the earlier draft had: a silent tie-break between overlapping tracks.
There is no tie-break now. Two files that disagree produce a question, and the answer is yours.

### Migration 7

Appended to `MIGRATIONS` in `crates/core/src/ledger.rs` — never edited into an existing entry, or
databases in the field diverge from fresh ones.

```sql
CREATE TABLE IF NOT EXISTS tracks (
    id                 TEXT PRIMARY KEY,  -- sha256 of the file's bytes
    name               TEXT,              -- the file name, for a human to recognise
    source_path        TEXT,              -- where it was read from, for provenance
    creator            TEXT,              -- the GPX `creator` attribute, e.g. "OwnTracks"
    imported_at        INTEGER,           -- unix seconds
    point_count        INTEGER,           -- timed points in the file
    points_added       INTEGER,           -- how many entered the timeline
    points_identical   INTEGER,           -- already held, ignored
    points_conflicting INTEGER,           -- disagreed, settled by a decision
    first_fix          INTEGER,           -- unix seconds UTC; NULL if nothing was timed
    last_fix           INTEGER,
    min_lat REAL, min_lon REAL, max_lat REAL, max_lon REAL,
    gpx                TEXT               -- the file as fed, verbatim
);

-- The timeline. `at` is the rowid, so a lookup by time is as fast as SQLite gets
-- and the one-position-per-instant rule cannot be broken by a bug in the importer.
CREATE TABLE IF NOT EXISTS track_points (
    at       INTEGER PRIMARY KEY,  -- unix seconds UTC
    lat      REAL NOT NULL,
    lon      REAL NOT NULL,
    ele      REAL,                 -- NULL where the point carried no <ele>
    track_id TEXT NOT NULL         -- the import this value came from
);

-- Every disagreement and what was decided about it. These are not supposed to
-- happen; when one does, the decision is the only record of why the library says
-- something the file does not.
CREATE TABLE IF NOT EXISTS point_conflicts (
    at         INTEGER NOT NULL,
    track_id   TEXT NOT NULL,      -- the import that disagreed
    kept_lat REAL, kept_lon REAL, kept_ele REAL,
    other_lat REAL, other_lon REAL, other_ele REAL,
    metres     REAL,               -- how far apart the two positions are
    decision   TEXT,               -- 'kept-existing' or 'took-new'
    decided_at INTEGER,
    PRIMARY KEY (at, track_id)
);

CREATE INDEX IF NOT EXISTS idx_track_points_track_id ON track_points (track_id);
```

**Why the GPX text is kept.** The points are what the join queries; the text is what was actually
fed. Keeping it costs 5 KB for the sample track and buys two things a parsed table cannot:
provenance — the file can be handed back, byte for byte, to whoever asks where a coordinate came
from — and re-parsing, when the reader later learns a GPX element it ignored today.

**The track id is the content hash, so importing is idempotent.** Feed the same export twice and
the second is *reported as already imported*, with the date of the first. Two different files
holding the same afternoon are two rows, which is correct: they are two exports, and the timeline
resolves what they have in common.

### Import is a preview and a commit

The same shape as every other tool here: work out what would happen, show it, then do it.

`previewTrackImport` parses the file and diffs it against the timeline, writing nothing:

| Bucket | Meaning | Default |
|---|---|---|
| **New** | No point at that instant | Inserted |
| **Identical** | A point at that instant, same position | Ignored — nothing to do |
| **Conflicting** | A point at that instant, **different** position | Needs a decision |

"Identical" is *within a tolerance*, not bit equality: **1 × 10⁻⁶ degrees** (about 11 cm) in both
coordinates, and half a metre of elevation, or elevation absent on both. A re-export of the same
fix from the same app is byte-identical and lands here trivially; the tolerance is for an export
that rounded differently on its way out. Anything outside it is a conflict, deliberately — the
threshold decides what counts as the same reading, so it is written down rather than tuned until
the screen looks calm.

**Each conflict shows how far apart the two positions are, in metres.** That number is what tells
you which fault you have: three metres is two apps disagreeing about the same fix, two kilometres
is a different device, a wrong offset in the export, or a file from a day you did not think it
was.

`commitTrackImport` takes a default — *keep what the library holds*, or *take what the file says*
— and per-instant overrides for the rows you decided individually. It runs **in one transaction**:
either the whole import lands as you decided it, or none of it does. A half-applied import leaves
a timeline nobody chose.

The commit **re-reads the file and recomputes the diff**, rather than trusting the preview it was
handed — the library may have moved since, and what matters is what is true now. This is the same
reasoning that makes a publish rebuild its plan instead of carrying the dry run's. An override
for a conflict that no longer exists is **reported as stale**, not quietly dropped.

### Deleting a track

Deleting a track removes the points still attributed to it and says how many. Points that a later
file also contained are attributed to whichever import first contributed them, so deleting a
track can remove a position that another stored file also attests to. The GPX text of both is
kept, so re-importing restores it. This is a deliberate limit rather than a many-to-many table of
attestations: with every fix coming from one phone, the simpler model is worth more than the
completeness, and the recovery is one click.

---

## What "EXIF-compatible" means here, and what is actually stored

The transform into EXIF's form is **performed and shown at import** — it is in the preview table,
so "compatible" is something you can see rather than something you are asked to trust:

| From the GPX | Into EXIF |
|---|---|
| `lat="52.531549"` | `GPSLatitude 52.531549`, `GPSLatitudeRef N` |
| `lon="13.369192"` | `GPSLongitude 13.369192`, `GPSLongitudeRef E` |
| `<ele>36.40</ele>` | `GPSAltitude 36.40`, `GPSAltitudeRef 0` (above sea level) |
| `<time>2026-09-04T15:33:37Z</time>` | `GPSDateStamp 2026:09:04`, `GPSTimeStamp 15:33:37` |

**What is persisted is the canonical numeric form** — Unix seconds and decimal degrees — and the
EXIF rendering is a pure function over it, computed for display and again for the write. This is
one recommendation against the letter of the request, for one reason: everything done with a fix
is arithmetic — how far apart two of them are, which is nearer in time, whether two readings are
the same reading — and a degrees-minutes-seconds string has to be parsed back into a number for
any of it, coming back not quite the one that went in. Storing the numbers and rendering on demand
keeps the two forms from ever disagreeing, because there is only one of them.

---

## The hard problem: EXIF has no timezone

`MediaMeta.capture` is a `NaiveDateTime` and that is deliberate — EXIF `DateTimeOriginal` is
local wall-clock with no zone (`crates/core/src/media/meta.rs:122`). GPX time is UTC. **The join
cannot be computed without a UTC offset**, and an offset wrong by one hour puts every photograph
a few kilometres from where it was taken, plausibly and silently. That is the failure this design
is built around.

The offset is resolved in this order:

1. **`OffsetTimeOriginal` from the file.** Read in the entry walk `collect_exif` already makes,
   with `OffsetTimeDigitized` and `OffsetTime` as fallbacks in that order. (`nom-exif` does have a
   `find_tz_offset`, but on `IfdIter` rather than the `ExifIter` we hold; reading the tags in the
   fold is better placed anyway, because that function is pure over a tag list and the preference
   between the three is testable without a file.) Phones write it; most cameras do not. When it is
   there, it is the truth and nothing else is consulted.
2. **The offset set in the UI**, e.g. `+02:00` for the Berlin track in September.
3. **The estimate the tool offers.** Score each of the **thirty-eight offsets a clock is actually
   set to** by the *median* seconds between each photograph and its nearest fix, and propose the
   best.

   Real zones rather than a quarter-hour sweep, and that distinction is load-bearing: measured
   against the specimen track, a sweep returns **+01:45** for photographs taken at +02:00, because
   a track sampled every five minutes cannot separate the two — and no clock on earth is set to
   +01:45. The list keeps the five zones on a quarter or a half hour, so India, Nepal, Iran,
   Newfoundland and central Australia are not rounded away.

   Reported alongside the winner is the **range of offsets the evidence cannot separate from it**:
   two scores differing by less than the track's own spacing are not distinguishable *by that
   track*. `confident` is true only when one candidate survives and the median photograph is within
   half an hour of a fix — a weak win means the library does not cover these photographs, and
   saying so is more use than a confident wrong hour.

There is no silent default of UTC. A plan computed with an offset nobody confirmed says so on the
screen.

Separately, a **camera clock correction in seconds** handles a body whose clock has drifted. It
folds into the same arithmetic rather than being a second mechanism: photo instant =
capture − offset + correction.

## The inventory

Pointing at a folder produces a table, and writes nothing. Read-only work returns its rows rather
than a job id, for the reason `scanDates` does: the table *is* the answer, and handing back an id
threw it away.

| Column | From |
|---|---|
| File | |
| Capture | `MediaMeta.capture`, with which tag it came from |
| Offset | `OffsetTimeOriginal`, when the file carries one |
| Location | existing GPS, read in-process |
| Status | `Ok` · `NoLocation` · `NoDate` · `NoDateOrLocation` · `NotSupported` |

`NoDate` is the one that cannot be fixed here — with no capture time there is nothing to look up,
and the row says so and points at the Dates tab. Video is `NotSupported`, with the reason on the
row rather than an absence to be puzzled over.

## Matching

**Nothing is computed.** Every position written is one the phone recorded. The tracker reports
when — and only when — its owner moves, so the interval between two fixes is not missing data to
be filled in: it is somebody who had not moved yet. A line drawn across it passes through streets
nobody walked, and the coordinate it produces looks exactly as trustworthy as a real one.

For each photograph with a date and no location, converted to UTC, against the timeline:

| Situation | Result |
|---|---|
| A fix at that instant | Exact |
| Otherwise, the last fix **before** it, if within the age limit | Carried forward |
| Nothing before it — a photograph predating the track — and the first fix is within the limit | Nearest |
| Beyond the limit | Skipped: this track probably does not cover the photograph, and the reason says so |
| Already has GPS, and overwrite is off | Skipped, reason says so |

**The age limit is not about the fix going stale.** A movement tracker is right however long it
has been silent — the silence *is* the evidence that nobody moved. What the limit guards is the
other case: a photograph from a day this track does not cover, which would otherwise take the last
fix of a different trip and look exactly like a real answer.

It defaults to **twelve hours** — longer than any silence within a day the tracker was running, a
night at home included; short enough to refuse a frame from another journey. **Zero means no
limit**, the convention `max_megapixels` already uses. Every row reports the age of the fix it
used, so an answer carried forward for six hours says so.

`Nearest` remains a mode, for a tracker that samples continuously — where the interval really is
just a sampling rate and the nearer fix really is the better one. Against a movement-triggered
track it answers with where somebody went *next* whenever that fix happens to be nearer in time: a
fix twenty-nine minutes **after** a photograph can beat one three hours **before** it, and be
somewhere its subject never was. That is why it is not the default.

**Every matched row reports which method matched it and how many seconds away the fix was.** That
number is the honest measure of how much to trust the coordinate, so it belongs in the table, not
in a log.

### The join reads a slice, not a database

`Ledger` answers two questions — *the points between these two instants*, and *the fix either side
of that window* — and hands back a `Vec<TrackPoint>` sorted by time.

The second question is not an optimisation. The window is chosen from the photographs and the fixes
bracketing them can be any distance outside it, so without the neighbours a photograph inside the
overnight gap was refused with *"3 h 32 min after the last fix in the library"* when the library
holds fixes three hours later. A tool that has to refuse should at least refuse for the true reason. Everything after that is a pure function over a sorted slice,
which is what makes the matching testable without a database, a file, or a photograph. The window
is `[first capture − tolerance, last capture + tolerance]`, so a timeline holding a year of
five-minute fixes (~100 k points) loads only the part in play.

**A geotag job opens its own `Ledger`.** It reports progress, and `SinkProgress` writes through
the shared `Arc<Mutex<Ledger>>`; borrowing that one would deadlock against its own progress
update on the first photograph — the rule the publish path already follows
(`crates/server/src/api.rs:1570`).

## Writing

One new method on the persistent `ExifWriter` (G4) — `set_tags`, a single `-execute` per file
rather than one per tag. It takes the assignments already rendered, because `media` may not depend
on `tools` and a driver that knew what a GPS tag *means* would put the dependency the wrong way
round:

```
-GPSLatitude=<abs>   -GPSLatitudeRef=N|S
-GPSLongitude=<abs>  -GPSLongitudeRef=E|W
-GPSAltitude=<abs>   -GPSAltitudeRef=0|1        (0 above sea level, 1 below; omitted with no <ele>)
-GPSDateStamp=YYYY:MM:DD  -GPSTimeStamp=HH:MM:SS   (the fix's UTC, not the camera's local time)
-GPSMapDatum=WGS-84
-overwrite_original
```

Absolute value plus an explicit reference, because that is how the tags are defined; a signed
value with a mismatched ref is a coordinate in the wrong hemisphere.

**Then read back and verify**, the way `f1_dates::verify` does, and count *verified*, not
attempted. Two of the sixteen defects the Mac sessions found were tools reporting success having
done nothing; a geotag that claims to have written and has not is that same failure, and it is
invisible without the read-back.

Altitude is written as the track states it, including the one `<ele>0.00` in the sample — a
dropout. There is a switch to write no altitude at all. What there is not is a rule that quietly
discards suspicious elevations: inventing a correction is worse than recording what the phone
said.

A format `exiftool` refuses is a **failure on that row with the message it gave** (G10), not a
silent skip.

---

## Shape

```
crates/core/src/tools/geotag/gpx.rs      the GPX reader: text in, points out
crates/core/src/tools/geotag/exif.rs     the EXIF rendering of a point, and its tests
crates/core/src/tools/geotag/join.rs     offset resolution, matching, the estimator
crates/core/src/tools/geotag/library.rs  the import diff, and the commit
crates/core/src/tools/geotag/scan.rs     the inventory
crates/core/src/tools/geotag/tool.rs     GeotagTool — plan(), apply(), and preview()
crates/core/src/ledger.rs                migration 7, the timeline, the import diff
crates/core/src/media/meta.rs            read existing GPS, read the EXIF UTC offset,
                                         ExifWriter::write_gps
crates/server/src/api.rs                 7 routes
crates/desktop/src/commands.rs           7 commands
frontend/shared/src/client.ts            7 ApiClient methods and their types
frontend/shared/src/ui/views/Geotag.vue  the screen, rendered by both applications
frontend/{web,desktop}/src/main.ts       the route
frontend/{web,desktop}/src/App.vue       the tab
frontend/web/scripts/check-layout.mjs    /geotag added to ROUTES
```

`GeotagTool` implements the existing `Tool` trait, so `plan()` is a dry run that touches nothing
and `apply()` reports progress and runs as a job — the same shape as `DateRepairTool`, for the
same reasons.

### The API surface

Both transports can do all of this: it is local files and a local `exiftool` on either side. So
it goes on `ApiClient` and the view is shared — no method one transport has to throw for.

| Method | Route | Command |
|---|---|---|
| `previewTrackImport({ path })` → `TrackImportPreview` | `POST /api/tracks/preview` | `preview_track_import` |
| `commitTrackImport(request)` → `TrackImportResult` | `POST /api/tracks/import` | `import_track` |
| `tracks()` → `TrackRow[]` | `GET /api/tracks` | `list_tracks` |
| `deleteTrack(id)` → `{ points_removed }` | `DELETE /api/tracks/:id` | `delete_track` |
| `scanGeo({ path, recursive })` → `GeoScanRow[]` | `POST /api/tools/geotag/scan` | `scan_geo` |
| `planGeotag(request)` → `GeotagPreview` | `POST /api/tools/geotag/plan` | `plan_geotag` |
| `applyGeotag(request)` → job id | `POST /api/tools/geotag/apply` | `apply_geotag` |

```ts
TrackImportPreview {
  track: TrackRow;                     // parsed, not yet stored
  already_imported_at: number | null;  // this exact file has been seen before
  new_points: number;
  identical_points: number;
  conflicts: PointConflict[];          // { at, existing, incoming, metres, existing_track }
  sample: ExifPoint[];                 // the first rows, in the form they will be written
}

CommitTrackImportRequest {
  path: string;
  resolution: 'KeepExisting' | 'TakeNew';   // the default for every conflict
  overrides: { at: number; take: 'Existing' | 'New' }[];
}

GeotagRequest {
  paths: string[]; recursive: boolean;
  utc_offset_minutes: number | null;   // null = use the file's, or refuse
  clock_correction_seconds: number;
  mode: 'CarriedForward' | 'Nearest';
  max_edge_seconds: number;
  overwrite_existing: boolean; write_altitude: boolean;
}

GeotagPreview { plan: Plan<GeotagAction>; matched: number; unmatched: number;
                suggestion: OffsetSuggestion | null; }
```

`GeotagPreview` wraps `Plan<GeotagAction>` rather than replacing it, so the plan/apply contract
the other tools use is untouched and the suggestion has somewhere to live.

**Every path goes through `Config::resolve`, the `.gpx` included (G6).** Which means the track
file has to sit under a configured root: `docs/GPS/track.gpx` currently does not, so for testing
it moves under one or the project folder is added as a root. Worth knowing before the first run,
because the refusal otherwise reads like a permissions error.

### The screen

A new **Geotag** tab at `/geotag`, in `frontend/shared/src/ui/views/`. Built from `ToolPage`,
`PathListField` for the photographs and `PathField` for the track, so the folder pickers work
from the first commit.

**Tracks.** The library as a table — name, date range, points, imported. Adding one opens the
import preview: the counts, the EXIF rendering of the first few points, and, when there are any,
the conflicts. Conflicts are a table of their own — instant, what the library holds, what the
file says, **how far apart in metres**, and which to keep — with *keep all existing* and *take
all new* above it and a per-row override on each. Nothing is stored until that is answered.

**Photographs.** A folder, and the inventory table, with the counts summarised: *4 have
everything, 38 have a date and no location, 1 has neither.*

**Match.** The offset, the mode and the tolerances; then the preview table — file · capture
(local) · capture (UTC) · matched fix · seconds away · lat/lon · method — and apply, as a job
with `JobProgress`, reporting the verified count.

Styling is tokens only, radius ≤ 2 px, no glow on table type, controls at 16 px. `/geotag` joins
`check-layout.mjs`'s `ROUTES` so it is asserted at 390 px like every other route. Three tables on
one screen at 390 px is the layout risk here: each scrolls inside its own container, and the
panels collapse to one column.

**No map.** Tiles mean a CDN, and the desktop application has to work with the server off and the
machine offline (MV-7.3). If a picture is wanted later, a local SVG plot of the timeline with the
photographs marked on it is the version that stays offline.

One component change while here: `FolderPicker` gains an optional extension filter so a **file**
can be picked, not only a folder. The recurring cause behind the sixteen defects the Mac sessions
found was that the pickers made pointing at things the normal gesture while the tools had only
ever seen typed paths. A new tool shipping with a typed-only file field repeats that.

---

## Steps

Each ends somewhere the gates pass. The first four need no UI, no `exiftool` and no database.

### GT-1 — the GPX reader
`tools/geotag/gpx.rs`. A small explicit scanner over the text: `<trkpt>`, `<wpt>` and `<rtept>`,
attributes in either order, namespace prefixes (`<gpx:trkpt>`), nested `<ele>` and `<time>`,
several `<trkseg>` and `<trk>` concatenated. No `quick-xml`: the format is this simple, and a
dependency would need a G8 justification it cannot earn.
**Done when** the sample parses to 50 points with the right first and last fix, and malformed
input returns an error naming what it could not read.

### GT-2 — the EXIF rendering
`tools/geotag/exif.rs`. A point to its tags and back, with the hemisphere refs and the UTC
date/time stamps. Pure, and the same function the preview shows and the writer uses.
**Done when** a round trip through the rendering preserves the coordinate to EXIF's precision.

### GT-3 — metadata reads
`MediaMeta` gains the existing GPS fix and the EXIF UTC offset, both from the iterator
`collect_exif` already walks (`ExifIter::parse_gps`, `find_tz_offset`).
**Watch for:** `MediaMeta` derives `Eq`, which `f64` coordinates break. Drop to `PartialEq` after
checking nothing needs `Eq`.
**Done when** a phone photograph reports its coordinates and its offset, and a file with neither
still reports everything else it has.

### GT-4 — the join and the estimator
`tools/geotag/join.rs`, pure over a sorted slice. Offset resolution, the matching table above,
the median-gap estimator, and the metres-apart calculation the conflict table needs.
**Done when** the tests below pass. At this point the matching is complete and provable with no
UI, no database and no `exiftool`.

### GT-5 — the timeline
Migration 7, and the `Ledger` work: the import diff (new / identical / conflicting), the
transactional commit with its resolutions, the conflict audit rows, list, delete, and the
windowed points query.
**Done when** a fresh database and a migrated one have identical schemas; re-importing the sample
adds nothing and reports why; and a file altered in one coordinate produces exactly one conflict.

### GT-6 — the inventory
`tools/geotag/scan.rs`: the folder walk and the five statuses.
**Done when** a folder of mixed files reports each one correctly, video included.

### GT-7 — writing
`ExifWriter::write_gps`, one `-execute`, plus verify-by-read-back.
**Done when** a written file reads back the coordinate it was given.

### GT-8 — `GeotagTool`
`plan()` and `apply()` against the `Tool` trait, with the skip reasons from the matching table
and `summarise` for the closing line.
**Done when** a plan over a folder writes nothing — asserted on mtimes — and an apply reports
verified counts.

### GT-9 — transports
Seven routes, seven commands, every path through `Config::resolve`.
**Done when** both `check:transport` runs pass and the desktop typechecks against the same
`ApiClient`.

### GT-10 — the screen
`Geotag.vue`, the tab and route in both applications, `/geotag` in `check-layout.mjs`.
**Done when** `check:layout` passes with the new route included.

### GT-11 — the picker
`FolderPicker`'s optional file mode, `PathField` passing it through.
**Done when** existing callers are untouched and the track field can be picked.

### GT-12 — documents
The `known-gaps.md` entry, `phase-reports/geotag.md`, a new **MV-15** section, and the test counts
in `CLAUDE.md`'s gates block.

---

## Tests

All in `core`, so `cargo test -p phototools-core` proves the feature with no binary crate present
(G2). Names are sentences.

**The reader** — attributes in either order; a namespaced tag; a point with no `<ele>`; a point
with no `<time>`; several `<trkseg>`; points out of order; CRLF; an empty file; truncated XML.

**The rendering** — hemispheres north, south, east and west; a zero elevation; a below-sea-level
elevation taking `GPSAltitudeRef 1`; the date and time stamps in UTC, not local.

**The import diff** — the same file twice adds nothing; a file where one coordinate moved by a
metre is identical, by ten metres is a conflict; the metres figure is right for a known pair;
*keep existing* leaves the timeline untouched and still records the conflict; *take new* replaces
the point and records what it replaced; an override for a conflict that no longer exists is
reported as stale; a commit that fails part-way leaves the timeline as it was.

**The join** — an exact hit; *every answer is a fix the phone recorded*, asserted over every
second of a track rather than at chosen moments; *the answer is never a fix from later*, likewise;
a fix three hours old beating one twenty-nine minutes away, with `Nearest` shown taking the other
one; a photograph predating the track; the age limit refusing and zero accepting; existing GPS
with overwrite off and on.

**The offset** — a fixture built at `+02:00` is estimated at `+02:00`; a one-hour error is
visible in the score; a timeline covering none of the photographs yields no suggestion rather
than a confident wrong one.

**The inventory** — each of the five statuses from a file that genuinely has that shape.

**Planning writes nothing** — asserted on mtimes, not assumed.

**Applying** — against the stubbed writer (`ExifWriter::start_with`), asserting the tag set and
that one `-execute` covers one file. The claim, not a proxy for it.

## Gates

Everything in `CLAUDE.md`, unchanged, plus:

- `cargo test --workspace` and `cargo test -p phototools-core` — **both counts in `CLAUDE.md` go
  up and are updated to what they actually are**.
- `npm --prefix frontend/web run check:layout` with `/geotag` in `ROUTES`.
- MSRV 1.80 under clippy.

## MV-15 — what only a Mac and real photographs can settle

Proposed for [`manual-verification.md`](manual-verification.md) at GT-12:

| Id | Check |
|---|---|
| MV-15.1 | Import `track.gpx`: 50 points, first and last fix match the file, the EXIF sample reads correctly |
| MV-15.2 | Import it again: nothing added, reported as already imported |
| MV-15.3 | Import an overlapping export from the same phone: the shared instants are identical and ignored |
| MV-15.4 | Import a file altered in one coordinate: one conflict, with the right distance in metres, and the decision holds |
| MV-15.5 | Scan a folder: the counts of has-both, has-date-only and has-neither match what the files carry |
| MV-15.6 | Estimate the offset on Berlin photographs from 2–5 September: `+02:00` proposed |
| MV-15.7 | Preview at `+00:00`: visibly larger gaps, and the suggestion offers `+02:00` |
| MV-15.8 | A photograph inside the ten-hour overnight gap is refused, and says why |
| MV-15.9 | Apply to copies; macOS Preview and Photos show the location on a map |
| MV-15.10 | A photograph that already has GPS is untouched with the defaults |
| MV-15.11 | A NEF or ARW is written in place and reads back |
| MV-15.12 | A `.mov` is listed as `NotSupported`, with the reason |
| MV-15.13 | A geotagged photograph published to Google Photos shows its location there |

## Risks

| | |
|---|---|
| **The offset is wrong and nothing catches it** | The estimator, the seconds-away column, and no silent UTC default. The residual risk is a user who overrides all three. |
| **The identical-point tolerance is wrong for real exports** | 11 cm is tight enough that a genuine disagreement cannot hide under it, and MV-15.3 is the check that a real second export from the same phone does not produce a screenful of false conflicts. If it does, the number moves — and this document moves with it. |
| **Deleting a track removes a point another file also attests to** | Stated as a deliberate limit; the stored GPX makes it recoverable by re-import. |
| **`MediaMeta` losing `Eq`** | Checked at GT-3 before the change, not after. |
| **Writing into RAW** | `exiftool` is the same tool the rest of the application writes with, and MV-15.11 is a real file. A refusal is a reported failure, not a skip. |
| **G5 — never write to a source card** | This tool writes wherever it is pointed. Nothing in it distinguishes a card from a folder; that check lives in `Config` roots and is worth revisiting for every tool at once, not for this one alone. |
| **A large timeline** | 100 k points is 3 MB in memory and the query is windowed. A year of tracks is not a problem; ten years would want the matching to happen in SQL. |
