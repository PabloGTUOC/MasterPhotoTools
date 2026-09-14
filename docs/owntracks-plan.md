# OwnTracks — the phone reports where it was, and the timeline keeps it

> **Built, and the gates pass**; MV-19 awaits a phone, and the report is in
> [`phase-reports/owntracks.md`](phase-reports/owntracks.md). Steps have stable ids (`OT-3`).

Today a position reaches the library because somebody exported a `.gpx` and imported it. This is the
plan for the phone doing it itself: OwnTracks posts each fix to the server as it happens, and at the
end of the day those fixes become that day's track — an ordinary track, in the ordinary timeline,
which the desktop then picks up on its next sync.

**No MQTT broker.** The phone's configuration is unconnected placeholders (`host`, `user`, `pass`)
and nothing consumes its messages, so there is no broker to preserve and none to run. OwnTracks'
HTTP mode posts straight to a URL, and one already exists: the Cloudflare tunnel that serves the web
application. One endpoint, one credential, no new container, no port forwarding.

---

## What arrives, and what is kept

OwnTracks posts a small JSON object per fix. The fields that matter here:

| Field | Used as |
|---|---|
| `tst` | **The instant**, Unix seconds UTC. The time the fix was *taken*, never the time it arrived |
| `lat`, `lon` | The position |
| `alt` | `ele`, when present |
| `tid` | Which device, kept for the track's name |

Everything else — battery, velocity, connection, geofence transitions, waypoints, the last-will
message — is read past. The endpoint accepts a payload whose `_type` is not `location` and answers
politely without storing anything: refusing would make the phone retry forever for a message we were
never going to keep.

**`tst`, not arrival time**, is what makes a week's catch-up work. A phone that was in a tunnel, or
abroad with data off, queues its fixes and posts them all at once; each one still knows when it
happened, so each lands on the right day.

---

## OT-1 — one endpoint, which can do exactly one thing

`POST /api/timeline/owntracks`, and nothing else on it.

It is the **second route that Firebase does not guard** (`/api/health` is the first), because a
phone has no Firebase session. So its own credential, and a deliberately tiny blast radius:

- **A device username and token**, sent as HTTP Basic — the two fields OwnTracks already offers.
  `OWNTRACKS_USER` and `OWNTRACKS_TOKEN` in the server's environment; **unset means the route
  refuses everything**, the way an unset `PUBLISHING_DIR` refuses all publishing. There is no
  default credential and no "allow if empty".
- **Compared in constant time.** A token checked with `==` leaks its length and prefix to anybody
  patient.
- **A body cap** (64 KB) and only the six fields above read out of it.
- **It cannot read.** No query, no listing, no path, no filesystem. The worst a stolen token buys is
  writing false positions into the timeline — which is why the token is long, and why the next
  section keeps arrivals apart from the library until a day closes.
- Answers `[]`, which is what OwnTracks expects and means "no commands for you".

## OT-2 — arrivals land in a staging table, not in the timeline

Migration 12: `device_fixes (at INTEGER PRIMARY KEY, lat, lon, ele, device TEXT, received_at INTEGER)`.

Two reasons this is not written straight into `track_points`:

1. **A track is a document.** Every position in the library came from a file whose text is stored
   beside it, and that is what lets somebody ask where a coordinate came from. A fix that arrives by
   HTTP has no document until the day it belongs to is complete.
2. **`INSERT OR IGNORE` on the instant makes retries free.** A phone that posts, loses the reply and
   posts again writes the same row twice with no consequence.

Staged rows are deleted once the day they belong to has been rolled up and stored — at which point
they exist in the timeline with a document behind them.

## OT-3 — at the end of the day, the day becomes a track

A day is closed **two hours after local midnight**, which is the grace period for a phone that was
asleep. Closing it means: take that day's staged fixes, write them as a GPX document, import it by
`library::commit_import` like any file, and delete the staged rows.

- Named `OwnTracks 2026-09-13`, creator `OwnTracks`, `source_path` `(reported by <tid>)`.
- `source = recorded`: a device recorded these. It is the one kind of claim this road can make.
- **The same day rolled up twice is one track, not two** — the same fixes produce the same text,
  which produces the same hash, which is the same id. The second import adds nothing.
- **A fix that arrives after its day was closed** produces a second, amended track for that day. It
  merges by instant like any other import, and the conflict rules apply if it disagrees with a
  position already held.

**Which day a fix belongs to is a local question**, so the boundary uses a configured offset
(`TIMELINE_OFFSET_MINUTES`, defaulting to UTC) — the same problem, and the same answer, as the
Timeline tab's coverage strip. An evening in Berlin is the same day as that morning, and UTC
disagrees for two hours of it.

## OT-4 — what runs the clock

An hourly task in the server binary: for each day that has staged fixes and is past its grace
period, roll it up. Lifecycle belongs to the binary (G1); the rollup itself is a `core` function
taking a ledger, a day and an offset, so it is tested without a server.

Hourly rather than at a fixed midnight: a NAS that was off at 02:00 must still close yesterday when
it comes back, and a task that only fires at one moment quietly never does.

## OT-5 — what the phone is set to

In OwnTracks: **Mode → HTTP**, then

| Setting | Value |
|---|---|
| URL | `https://phototools.opinasdeque.es/api/timeline/owntracks` |
| Authentication | on, with the user and token from the server's environment |
| Encryption key | **empty** — the connection is already HTTPS, and an encrypted payload would need decrypting at the other end for no gain |
| Monitoring | **significant changes**, as it is now: a handful of fixes a day, reported when you actually go somewhere, which is what dating a roll of film needs |

`mode: 0` in the configuration seen today is MQTT; HTTP is `mode: 3` and adds a `url`. Nothing else
consumes the phone's messages, so nothing breaks in the switch.

## OT-6 — what this changes about the phone, said plainly

The phone will report its position to the NAS continuously, in the background, for as long as this
is switched on. It is the user's own server and no fix leaves it — but that is a real change in what
a phone does, and it belongs in the documentation rather than in a footnote.

Turning it off is turning off HTTP mode in OwnTracks; the positions already stored stay.

## OT-7 — accuracy, and what is not thrown away

OwnTracks reports `acc`, the radius it believes the fix is good to — sometimes kilometres, indoors
or on a train.

**Nothing is dropped for being inaccurate.** A poor fix is still where the phone believed it was,
and the tool that matches photographs already shows the age of a fix rather than pretending to
certainty. A filter would be a different feature, with a threshold nobody can choose well in
advance; if it turns out to matter, the number belongs on screen, not buried in an importer.

## OT-8 — the checks a person has to run

New **MV-19** items:

| | |
|---|---|
| MV-19.1 | With the phone configured, walking to a different place produces a fix: the staged count rises and the web Timeline shows it after the rollup |
| MV-19.2 | A request without the token, or with a wrong one, is refused — and the refusal says nothing about which part was wrong |
| MV-19.3 | Aeroplane mode for a day, then reconnecting: every queued fix lands on **the day it happened**, not the day it arrived |
| MV-19.4 | A day rolled up twice produces one track, not two |
| MV-19.5 | The day boundary follows the configured offset: an 23:30 fix in Berlin belongs to that day, not the next |
| MV-19.6 | The day's track reaches the Mac on the next desktop sync, labelled *recorded* |
| MV-19.7 | With `OWNTRACKS_TOKEN` unset the route refuses everything, and the rest of the server is unaffected |

---

## What this does not do

- **It does not replace `.gpx` import.** A camera-side tracker, a borrowed phone, an old export —
  all still import as files.
- **It does not track anybody else.** One device, one credential. A second phone would need its own,
  and the question of whose timeline it belongs to is not answered here.
- **It does not report live position anywhere.** The library is a record of where somebody has been,
  read by the tools that date and place photographs. There is no map that says where you are now.
