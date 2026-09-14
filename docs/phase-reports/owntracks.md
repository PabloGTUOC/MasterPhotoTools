# OwnTracks — the phone reports its own positions

**Status:** built, gates pass. Seven checks await a phone (MV-19).

Not a numbered phase. Planned in [`owntracks-plan.md`](../owntracks-plan.md) at the user's request
and reported here on the same terms as a phase.

---

## What it does

OwnTracks posts each fix to `POST /api/timeline/owntracks`. Arrivals wait in `device_fixes`
(migration 12); two hours after a local day ends, that day's fixes are written as a GPX document and
imported like any file — `OwnTracks 2026-09-13`, creator `OwnTracks`, `source = recorded`. The
desktop picks it up on its next sync, so positions reach both machines with nobody exporting
anything.

**No MQTT broker.** The phone's configuration was unconnected placeholders and nothing consumed its
messages, so there was no broker to preserve and none to run. HTTP mode posts to a URL that already
exists — the Cloudflare tunnel serving the web application. No new container, no port forwarding, no
dependency (G8).

## The route, and why its blast radius is small

It is the **second route Firebase does not guard**, `/api/health` being the first, because a phone
has no Firebase session. Everything about it is shaped by being reachable from the public internet:

| | |
|---|---|
| Credential | `OWNTRACKS_USER` + `OWNTRACKS_TOKEN`, HTTP Basic, **compared in constant time** — a token checked with `==` leaks its length and prefix to anybody patient |
| Unconfigured | **Refuses everything**, `404`. No default credential, no "allow when empty" |
| Body | Capped at 64 KB; six fields read; `serde` ignores the rest |
| Reach | It cannot read. No path, no query, no listing, no filesystem |
| Worst case | A stolen token writes false positions into the timeline — which is why arrivals stage rather than joining the library directly |

A Firebase token is refused here, and this credential is refused everywhere else. Both directions are
asserted, because "the new route accidentally accepts the old key" is exactly the kind of thing that
passes a happy-path test.

## Decisions

**Arrivals stage; they do not join the timeline.** Every position in the library has a document
behind it — that is what lets somebody ask where a coordinate came from — and a fix that arrives
over HTTP has none until its day is complete. `at` as the primary key also makes a retry free: a
phone that posts, loses the reply and posts again writes the same row twice with no consequence.

**The instant is `tst`, when the fix was taken.** Never arrival time. A week queued abroad lands on
the days it happened, which a test asserts with a fix reported three days late.

**Rolling up twice is one track.** The same fixes produce the same text, which hashes to the same id.
A fix that arrives after its day was written up makes a second, amended track for that day, merging
by instant like any import.

**Which day a fix belongs to is local**, so the boundary follows `TIMELINE_OFFSET_MINUTES`. An
evening in Berlin is the same day as that morning; UTC disagrees for two hours of it, and a test
pins both readings.

**Nothing is dropped for being inaccurate.** A poor fix is still where the phone believed it was, and
a threshold nobody can choose in advance belongs on a screen rather than in an importer.

**Hourly, not at midnight.** A NAS that was off at 02:00 must still write up yesterday when it comes
back, and a task that fires at one moment quietly never does. Writing up a day already written up is
a no-op, so running often costs nothing.

## Tests

Seventeen new, 757 → **774** in the workspace and 669 → **680** in core.

- **`owntracks::read`** — a location becomes a fix; a transition, last-will, waypoint list or card is
  ignored rather than refused (refusing would have the phone retry forever); a location with no time
  or no place is refused rather than stored at the epoch; a coordinate off the planet is refused.
- **The day boundary** — 23:30 in Berlin belongs to that day and not the next, in both the offset
  reading and the UTC one, so the difference is pinned rather than assumed.
- **Rollup** — a day becomes one track with a document behind it and the staged rows are forgotten;
  running it twice is one track; a fix taken on Sunday and reported on Wednesday lands on Sunday; a
  repeated post stores one fix; a day with nothing staged produces no track.
- **The credential** — the right pair is accepted; a wrong token, a wrong user, a longer token, a
  truncated one, a bearer token, malformed base64, and no header at all are refused; a half-set
  credential is no credential.
- **The route** — refuses without the credential and with a Firebase token; `404` when unconfigured;
  stores a reported position, answers `[]`, accepts a non-location without storing it, and the day
  then becomes a track with its text kept.

## One thing fixed on the way, which was not this feature

`EXIFTOOL_PATH` is read by every metadata write, and a test set it process-wide to prove a wrong path
is reported rather than ignored. The tests in that binary run in parallel, so roughly one run in ten
failed in whichever *other* test happened to be writing at that moment — twice during this work,
with a message about `/nowhere/exiftool` that had nothing to do with the test that failed.

`media::meta::exiftool_program_with` now takes the configured value as an argument, and
`exiftool_program` reads the environment and delegates. The test asserts the same two claims with no
global state. A flaky suite is worse than the bug it resembles: it teaches people that a red run
means nothing.

## Not built, deliberately

Decrypting OwnTracks' end-to-end encrypted payloads — the connection is already HTTPS and the key
would have to live on the server, which is more moving parts for no gain. More than one device: one
credential, one timeline, and whose timeline a second phone belongs to is a question this does not
answer. And nothing that shows where somebody is *now*: this is a record of where they have been,
read by the tools that date and place photographs.
