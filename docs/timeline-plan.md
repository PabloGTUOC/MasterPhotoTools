# Timeline — development plan

> **Stage A is built and the gates pass**; MV-17 awaits a browser, and the report is in
> [`phase-reports/timeline.md`](phase-reports/timeline.md). Where the build diverged from this plan
> the reason is there — provenance on the import rather than the point, and two timeline calls
> rather than one.
>
> **Stages B and C are not built.** Steps have stable ids (`TL-4`), the way
> [`geotag-plan.md`](geotag-plan.md) numbers its own, so one can be named in a commit or a
> conversation. It supersedes the narrower *Places* plan, which wrote a `.gpx` for Geotag to read
> back; the library is the timeline, so points can go straight into it.

See where you have been, and say where you were when nothing recorded it.

[Geotag](geotag-plan.md) already holds a **timeline**: one position per instant, in `track_points`,
built from imported `.gpx` files and matched against photographs on time. What it has never had is
a way to *look* at it. This tab is that window — a map and a date — plus the two things looking at
it immediately makes you want: **add a point where a day is blank**, and **import the location
history Google has been keeping all along**.

---

## What exists already, and is not rebuilt

| | |
|---|---|
| `track_points (at PRIMARY KEY, lat, lon, ele, track_id)` | One global timeline. A day is a range scan on the rowid — the query is already cheap |
| `Ledger::points_between(from, to)` | Exactly the day query this tab needs |
| `tracks` + `point_conflicts` | Provenance per import, and every disagreement kept rather than resolved silently |
| `geotag::join::match_at` | The matcher. **Untouched by this plan** |
| `TrackLibrary.vue`, `PathField`, `FolderPicker` | The import panel and the path controls |

The work is a map, three new queries, one importer, and one honest answer to what a hand-placed
point *is*.

---

## The honesty problem, which comes first

`join.rs` carries a promise, and it is why that module can be trusted:

> **Every position this tool writes was recorded by the phone.** Nothing is computed, averaged or
> drawn between two points.

A pin you place by hand is not a recorded fix — it is you asserting where you were. Usually right,
never an observation. Google's *place visits* are a third thing again: Google's inference from
signals it no longer shows you. If all three enter the library looking alike, that promise stops
being true and nothing in the data says so.

So the library gains **provenance per point**, and it is carried to every screen that shows one:

| Source | Written as | Reads as |
|---|---|---|
| A `.gpx` from a phone | `recorded` | *recorded* |
| A pin placed on this tab | `placed` | *placed by hand* |
| Google raw records | `recorded` | *recorded (Google)* |
| Google place visits | `inferred` | *inferred by Google* |

**Built as `tracks.source`, not `track_points.source`** — corrected here rather than left to
mislead. A point is attributed to the import that contributed it, so the column belongs to the
import and a join answers for the point: one row per import instead of one per fix, and a
million-point timeline that does not carry the word "recorded" a million times. Migration 10, in the
numbered style the ledger already uses, with existing rows keeping `recorded`, which is what they
are. `join.rs`'s promise is amended to the accurate one: *every position is one somebody recorded
or asserted; none is computed*. The arithmetic prohibition stands: a span becomes two real points,
one at each end, and nothing between them is interpolated.

---

## Delivered in three stages, each useful alone

Stage A is the tab the user asked for and stands on its own. B is the one that fills years of blank
days in one go. C is the one whose size is real and whose format I cannot yet verify.

---

# Stage A — the map, the day, and the pin

## TL-1 — three queries

In `Ledger`, beside `points_between`:

```rust
/// Day → how many fixes, for a coverage strip. Grouped in SQL; a year is 365 rows.
pub fn coverage(&self, from: i64, to: i64) -> SqlResult<Vec<DayCoverage>>;

/// The fixes in a window with the track they came from, for labels on a map.
pub fn points_with_source(&self, from: i64, to: i64) -> SqlResult<Vec<SourcedPoint>>;

/// The first and last instant the library holds anything for.
pub fn timeline_extent(&self) -> SqlResult<Option<(i64, i64)>>;
```

`at` is the rowid, so each is a range scan. `coverage` groups by `at / 86400`, which is UTC days —
and the day boundary is the user's, not UTC's, so the offset is applied to the query bounds rather
than to every row.

## TL-2 — reading the timeline, on both transports

One `ApiClient` method, implemented on the server **and** the desktop — a capability the type
system offers must exist on both:

```ts
timeline(request: { from: number; to: number }): Promise<{ points: SourcedPoint[]; coverage: DayCoverage[] }>;
```

A window is required, never "everything": a decade of five-minute fixes is a million rows and no
map can draw them. The screen asks for a day, or a month when showing the strip.

## TL-3 — the screen

`frontend/shared/src/ui/views/Timeline.vue`, in `sharedToolRoutes` before Geotag.

- **A coverage strip** across a month: one cell per day, a dot where the library holds fixes, empty
  where it holds none. The empty ones are the point of this feature — they are where a roll of film
  has nothing to match against.
- **A map** of the chosen day: the fixes in time order, each labelled with its time and its source,
  the path between them drawn as a dashed line that says plainly it is *drawn between fixes, not
  travelled*.
- **A day with nothing** gets the pin flow rather than an empty state: the map opens where the last
  known fix was, not in the Atlantic.
- Every control clears 40px and the screen is clean at 390px, or `check:layout` fails. The map gets
  its own `overflow` container so the page never scrolls sideways.

## TL-4 — the map component

`frontend/shared/src/ui/components/MapView.vue` — transport-free, props in, events out: points and
a selected position in, `update:position` out. Testable without a network.

- **Leaflet, bundled** (G8: ~40 KB gzipped, MIT, recorded in the phase report). Not from a CDN, for
  the reason the fonts are self-hosted: the Mac is offline exactly when somebody is travelling.
- Tiles from OpenStreetMap, attributed, overridable by `VITE_TILE_URL`. No key, no billing, and no
  terms forbidding what we do with a coordinate — which Google's tiles do have.
- Dark, to match the rest, by a CSS filter on the tile layer, behind one class that can be removed.
- Tiles that will not load are a state, not an error: a grey map, a notice, and every coordinate
  field still working.

## TL-5 — placing a point

Click the map, or type a coordinate. Give it a date, a **from**, a **to**, and the UTC offset that
applied there and then — defaulting to the Mac's, with the resulting UTC shown as you type:

```
14:00 on 1 May 2013, UTC+02:00  →  2013-05-01T12:00:00Z
```

A span writes **two** points, at `from` and `to`, at the same coordinates, `source = placed`. An
instant writes one. `CarriedForward` then does the rest, exactly as it does for a phone that sat
still. Altitude is never written: we do not know it, and `ele = None` already means *unrecorded*
rather than zero.

Saved through `commitPlacedPoints`, which reuses `library::commit_import` so a pin meets the same
conflict rules as a file: an instant already held is put to the user, never overwritten silently.

The timezone is typed twice — here and in Geotag — and that is this design's one remaining way to
move every photograph by an hour. Both screens say so. Reading the offset off the timeline is a
later step, not this one.

## TL-6 — GPX export, because a library that cannot be got out of is a trap

`geotag::gpx::write(points) -> String`, beside `parse` so the two cannot drift, and a **Export this
day** control. Round-trip is the test: written, re-parsed, same points, and `creator` says
PhotoTools.

This also covers the original request — a `.gpx` to hand to anything else — without making it the
route into our own library.

---

# Stage B — Google place visits

Google's Timeline holds two different things, and the smaller, better one comes first.

A **place visit** is a name, a coordinate and a span: *Alexanderplatz, 14:02–17:40*. That is exactly
the shape of TL-5's pin, arrives with the name already on it, and a year of them is a few thousand
rows rather than millions. For a library of old photographs it answers the actual question —
*which city was I in that week* — and it is what fills the blank days in the strip.

## TL-7 — the reader

`crates/core/src/tools/geotag/timeline.rs`, taking a path and producing the same `PlacedStop` list
TL-5 builds by hand, `source = inferred`:

- Each visit becomes two points, at the start and end of the span, named for the place.
- **Nothing is invented between visits.** The gap between leaving one and arriving at another is a
  gap, and `CarriedForward` reads it as *still at the last place*, which is what the module already
  believes about silence.
- A visit whose span is absurd — negative, or longer than a fortnight — is rejected with its index
  and reason, the way `gpx::parse` rejects a point rather than failing the file.

## TL-8 — importing it

The same three-step flow the `.gpx` import already has, because it is the right one: read, show
what would change, commit. `preview_import` and `commit_import` are reused whole; only the reader
in front of them is new. One import is one row in `tracks`, named for the file, `creator` saying
Google.

---

# Stage C — Google raw records

## TL-9 — what this actually is

`Records.json` from a Takeout is the phone's own fixes: a point every minute or two, for as long as
the account has had Timeline on. **Hundreds of megabytes and millions of points is normal.** That
changes three things and nothing else:

| | |
|---|---|
| Reading | Streamed, not `serde_json::from_str` on a 300 MB string. Progress is reported per megabyte through the existing job system (F17) |
| Storing | Millions of rows is nothing for SQLite, but `tracks.gpx` — which keeps the source text for provenance — is not holding 300 MB. Above a cap (5 MB) the path and hash are kept and the text is not, and the row says which |
| Conflicts | A year of Google fixes against a fortnight of phone `.gpx` will collide on thousands of instants. Point-by-point resolution is the wrong instrument at that scale |

## TL-10 — conflicts, in bulk

The import preview reports **counts and a sample**, not ten thousand rows, and offers three
policies: *keep what the library holds*, *take the imported fix*, or *keep mine where they exist and
add the rest* — the default, and the only one that cannot lose a fix somebody already decided on.
Whatever is chosen is recorded per point in `point_conflicts` exactly as today, so the decision
remains inspectable afterwards.

## TL-11 — thinning, offered and never silent

One fix a minute for a decade is a great deal of data to carry for photographs that need a position
to the nearest few minutes. An optional **thin to one fix every N seconds** (default off) states
what it did on the row: *1,240,880 read, 186,112 kept, one per 300 s*. Thinning drops observations
— so it is offered, reported, and never the default.

---

## What this is not

- **Not a route drawer.** A line between two fixes is drawn on the map and never written to a file
  or a photograph.
- **Not a reverse geocoder.** Naming the place a photograph was taken is a different feature.
- **Not a live tracker.** Nothing here talks to a phone.
- **Not Google Maps.** Its tiles need a key and a billing account, and its terms do not allow taking
  coordinates out of Google Maps into files we write. Importing *your own* Timeline export is a
  different matter: that is your data, handed to you by Google for this purpose.

---

## What I cannot verify from here, and will not guess

**Google's export format has changed, and I do not know which one your account produces.** Timeline
moved on-device in 2024; some accounts export `Records.json` and `Semantic Location History/`
monthly files from Takeout, others a single `Timeline.json` from the phone, with coordinates as
`"geo:52.52,13.41"` strings and a different tree. Writing a reader for the wrong one is a wasted
stage.

**Before TL-7 is written, one real export is needed** — the file list is enough to start, a small
sample file to finish. The reader then detects the shape from its top-level keys and refuses an
unknown one by name, rather than half-reading it into a timeline that quietly claims you spent 1970
at the equator.

---

## Risks

| Risk | What it costs, and what it changes |
|---|---|
| **The offset is typed twice**, here and in Geotag | Every photograph moves by the difference. Both screens say so; reading it off the timeline is a later step |
| **Google's format changes again** | The reader is one file behind a shape detector, and an unknown shape is refused by name rather than misread |
| **A timeline import dwarfs the ledger** | Millions of rows are fine; the source text is not kept above 5 MB, and the row says so |
| **An inferred visit is not a fix** | Carried as `source = inferred` and shown that way everywhere a point is shown |
| **`track_points` gains a column** | A ledger migration, in the numbered style already there. Existing rows are `recorded`, which is what they are |
| **The map is the first component with a network dependency of its own** | It degrades to a grey square, and every field still works — MV-17.4 |
| **Leaflet** (G8) | One dependency, bundled, with the reason in the phase report |

---

## The checks a person has to run

New **MV-17** items, each with what passing looks like:

| | |
|---|---|
| MV-17.1 | A day with a `.gpx` imported shows its fixes in time order, labelled *recorded* |
| MV-17.2 | The month strip marks the days that hold fixes and leaves the rest empty |
| MV-17.3 | A pin at Alexanderplatz, 1 May 2013 14:00–18:00 at UTC+02:00, stores two points at `12:00Z` and `16:00Z`, labelled *placed by hand* |
| MV-17.4 | Aeroplane mode: no tiles, a notice, and a typed coordinate still stores the same two points |
| MV-17.5 | A frame from that afternoon takes the position in Geotag; one from the morning is offered it with the gap shown |
| MV-17.6 | Placing a point on an instant the library already holds is put to the user, not written silently |
| MV-17.7 | Exporting that day and re-importing the file changes nothing — every point is identical |
| MV-17.8 | A Google place-visit import shows the place names and imports as *inferred by Google* |
| MV-17.9 | A raw-records import of a real export finishes, reports its counts, and the ledger is still under a sensible size |
| MV-17.10 | The screen is clean at 390px and the map is usable with one thumb |
