# Timeline — the track library on a map, and points placed by hand

**Status:** Stage A built, gates pass. Nine checks await a browser and a library with fixes in it
(MV-17). Stages B and C — importing Google's location history — are **not built**.

Not a numbered phase. Planned in [`timeline-plan.md`](../timeline-plan.md) before any code was
written, at the user's request, and reported here on the same terms as a phase.

---

## Standing against the specification

`SPECIFICATION.md` mentions neither maps nor location, so this stands exactly where Geotag stands:
no F-number invented, the specification not edited (G9), the scope the user's explicit request
recorded before any code (G11), and an entry in [`known-gaps.md`](../known-gaps.md) under *Places
the implementation goes beyond the specification*.

**One dependency was added (G8): `leaflet`**, ~40 KB gzipped, MIT, a peer dependency of the shared
package and a dependency of both applications, deduped the way `vue` is. The justification the GPX
reader could not earn — "we needed to find `lat` and `lon`" — a map does earn: tile arithmetic,
projection, drag, pinch-zoom and marker layers are not a weekend's work, and a hand-rolled version
would be worse in every way that matters to somebody trying to point at a square in Berlin.

**It is bundled, never fetched from a CDN**, for the reason `fonts.css` gives: the desktop
application is offline exactly when somebody is travelling with a card reader, and a CDN that fails
does so silently.

## What it does

| | |
|---|---|
| A month strip | One cell per day, lit where the library holds fixes. **The unlit ones are the point** — those are the days a roll of film has nothing to match against |
| A day on a map | The day's fixes in time order, each labelled with its time, its source and the import it came from |
| A pin | Click or type a coordinate, give it a date, a span and a UTC offset; it enters the library as two points |
| Export | The day as a `.gpx`, for anything else that reads one |

Both applications carry the tab: the view is shared, and each transport implements the same four
`ApiClient` methods against its own timeline — the Mac's and the NAS's.

## The decision this feature turned on

`join.rs` promised:

> **Every position this tool writes was recorded by the phone.** Nothing is computed, averaged or
> drawn between two points.

A pin is not a recording. It is the photographer asserting where they were — usually right, never an
observation — and storing it as though a phone had reported it would have made that promise quietly
false, with nothing in the data to say so.

So **provenance is stored per import and shown wherever a position is shown**: migration 10 adds
`tracks.source`, and a point's kind comes from the import that contributed it — `recorded`,
`placed`, or `inferred` for Google's place visits when Stage B lands. `join.rs`'s doc comment is
amended to the true version: every position is one somebody recorded or asserted, and none is
computed.

**The arithmetic prohibition is untouched.** A span is written as two real points, one at each end;
`CarriedForward` reads the silence between them as somebody who had not moved, exactly as it does
for a phone that sat still. The map draws a dashed line between fixes and the legend says it was
drawn, not travelled.

## Deviations from the plan

| Plan | Built | Why |
|---|---|---|
| `source` on `track_points` | `source` on `tracks` | A point is attributed to the import that contributed it, so the column belongs to the import and a join answers for the point. One row per import instead of one per fix — a million-point timeline does not carry the word "recorded" a million times |
| One `timeline` call per view | Two, one for the strip and one for the day | `include_points: false` answers the month with counts. One call for both would carry a month of positions across to draw thirty dots |
| Place-name search | **Not built**, and not in this plan either | It was in the superseded *Places* plan, over Nominatim. The tab works by clicking the map and by typing a coordinate, and search would be the only part of this application that sends anything typed to a third party — worth deciding deliberately rather than carrying in on the back of a map |
| Leaflet's attribution control | Rendered below the map instead | Its link is 13px in a corner. The credit is not optional and the 40px rule is not either, so it moved to where both can hold |

## What the gates caught

`check:layout` refused the first version of the tab three times over, which is what it is for:

1. **An unstyled `input[type="date"]` is 21px tall.** No view had used one before, so the design
   language had no rule for it. Fixed in `components.css` — date, time and datetime-local now read
   as terminal fields like every other control — rather than in this one view.
2. **Leaflet's zoom buttons are 30px.** Restyled to 40px, in the application's own colours.
3. **Leaflet's tiles report as overflowing the viewport.** They do not: they are clipped by the map's
   own `overflow: hidden` and cannot scroll the page. The check's ancestor walk excluded
   `auto`/`scroll` and not `hidden`, so it was imprecise rather than strict — the walk now includes
   it, and the `documentWidth` assertion, which is the claim itself, is untouched.

## Tests

Thirty-one new, all in `core` and so all running with no binary crate present (G2):

- `gpx::write` round-trips through `gpx::parse` unchanged, escapes markup in a name, and omits an
  altitude nobody knew rather than writing zero.
- `place::build` — a span is two points and never a third, an instant is one, the same placement
  twice is one import, stops out of order are the same import, and each refusal (a coordinate off
  the planet, a span that ends before it starts, two stops claiming one instant) is refused by
  message.
- `Ledger` — a fix carries the kind of claim its import made, an unknown word reads as `recorded`
  rather than failing the read, coverage counts days and omits the empty ones, and it groups by the
  day the person was living rather than the day UTC was having.

Workspace 709 → **731**, core 624 → **646**.

## Not built, and deliberately

Stages B and C of the plan: Google place visits and raw records. The reader is one file behind a
shape detector, and **the shape is not yet known** — Google changed the export in 2024 and which
one an account produces cannot be determined from here. The plan says so, and says that one real
export's file list is what unblocks it.
