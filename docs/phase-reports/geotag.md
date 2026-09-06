# Geotag — joining photographs to a phone's GPS track

**Status:** built, gates pass. Thirteen checks await a Mac and real photographs (MV-15).

Not a numbered phase. `BUILDPLAN.md`'s fifteen phases were closed by Phase 14; this is work the
user asked for afterwards, planned in [`geotag-plan.md`](../geotag-plan.md) before any code was
written and reported here on the same terms as a phase.

---

## Standing against the specification

**`SPECIFICATION.md` mentions neither GPS, geotagging, GPX nor location.** There is no F-number for
this and none was invented: a fabricated `F19` would make the code look as though the specification
had asked for something it never did. The specification was not edited (G9); the deviation is
recorded in [`known-gaps.md`](../known-gaps.md) under *Places the implementation goes beyond the
specification*.

**No dependency was added (G8).** GPX is read by a hand-written scanner in
`tools/geotag/gpx.rs` — the format is a handful of elements with two attributes that matter, and
`quick-xml` would need a justification that "we needed to find `lat` and `lon`" cannot earn.

## Delivered

| | |
|---|---|
| `tools/geotag/gpx.rs` | The reader. `<trkpt>`, `<wpt>`, `<rtept>`, namespace prefixes, comments, CDATA, attributes in either order. A malformed *point* is a rejection with a reason; only a broken *document* is an error. |
| `tools/geotag/exif.rs` | One rendering of a fix into EXIF's tags, used by the import preview, the inventory and the writer alike. |
| `tools/geotag/join.rs` | The matching — exact, carried forward, nearest — and the offset estimator. Pure over a sorted slice, and with no arithmetic on coordinates in it. |
| `tools/geotag/library.rs` | Import as a preview and a commit, with the three buckets and the conflict decisions. |
| `tools/geotag/scan.rs` | The inventory: what a folder already carries. |
| `tools/geotag/tool.rs` | `GeotagTool` against the existing `Tool` trait, plus `preview` for the screen. |
| `ledger.rs` migration 7 | `tracks`, `track_points`, `point_conflicts`. |
| `media/meta.rs` | Reads an existing GPS fix and the EXIF UTC offset; `ExifWriter::set_tags`. |
| `crates/server/src/api.rs` | Seven routes. |
| `crates/desktop/src/commands.rs` | Seven commands. |
| `frontend/shared` | `ApiClient` gains seven methods; `Geotag.vue`; `TrackLibrary.vue`; `FolderPicker` can select files. |

## The decisions that shaped it

### One position per instant, enforced by the schema

`track_points.at` is the primary key. Every fix comes from one phone, so a second file offering a
different position for a second already held is a **fault**, not a tie to break — and expressing it
as a key collision is what makes it impossible to resolve silently by whichever import ran last.

The first draft of the plan had a silent tie-break ("nearest, then most recently imported"). The
user rejected it before any code existed: disagreement is a question, and the answer is theirs.

### Importing is a preview and a commit

The shape every other tool here uses. The preview diffs the file against the timeline into **new**,
**identical** and **conflicting** and writes nothing; the commit takes a default plus per-instant
overrides and applies them in one transaction.

The commit **recomputes the diff** rather than trusting the preview it was handed — the library may
have moved since, and what matters is what is true now. This is the same reasoning that stops a
publish trusting its own dry run. An override naming an instant that is no longer in dispute is
**reported as stale**, not quietly dropped.

### "Identical" is a tolerance, and a conflict carries a distance

A millionth of a degree, about 11 cm, and half a metre of elevation. Two exports of one fix from
one phone are usually byte-identical; the tolerance is for an export that rounded differently on
its way out. Bit equality would have turned a re-export into a screenful of false conflicts.

Every conflict reports **how far apart the two positions are, in metres**. That number says which
fault it is: three metres is two apps disagreeing about one reading, two kilometres is a different
device or an export with the wrong offset.

### The numbers are stored; the EXIF form is rendered

The user asked for the fixes to be converted into EXIF's form and saved that way. They are
converted — the conversion is *shown* in the import preview, so "compatible" is visible rather than
promised — but what is persisted is Unix seconds and decimal degrees.

Everything done with a fix is arithmetic — the distance between two of them, which is nearer in
time, whether two readings are the same reading — and a degrees-minutes-seconds string has to be
parsed back into a number for any of it, coming back not quite the one that went in.
One stored form, rendered on demand by one function, is what stops the two ever disagreeing. The
rendering keeps seven decimal places — about a centimetre, an order of magnitude finer than the
tolerance that decides whether two readings are the same reading.

### No silent UTC

EXIF capture times are local wall-clock with no zone. The offset is resolved in one order: the
camera's own `OffsetTimeOriginal`, then what was set on the screen. A file with neither is
**skipped with a reason** rather than read as UTC, which would move every photograph by whatever
the offset really was. Each row records which source its offset came from, so a folder holding a
phone's photographs beside a camera's shows that the two were treated differently.

### Nothing is computed: the position is always one the phone recorded

The plan proposed interpolating between the fixes either side, under a ceiling. **The user removed
it during testing, and was right to.** Their tracker reports when — and only when — they move, so
the interval between two fixes is not missing data: it is somebody who had not moved yet. A line
drawn across it goes through streets they never walked, and produces a coordinate that looks
exactly as trustworthy as a recorded one.

So `match_at` has no arithmetic on coordinates in it at all. It answers with a fix at that second,
or the last fix **before** the photograph, or — for a photograph predating the track, where there
is nothing to carry — the first fix. A test asserts the guarantee over every second of a track
rather than at chosen moments, and a second asserts the answer is never a fix from later.

The route there is recorded because the intermediate step was wrong in an instructive way. Told
that gaps meant *stationary*, the first fix was to carry the last position forward while keeping
interpolation under a ceiling, and to measure that ceiling from the specimen file: its intervals
fall into two populations that do not overlap — 35 of 49 under 9m11s, the phone reporting while
its owner walked, and 14 over 10m17s running to 21 hours. Ten minutes sat in the empty band
between them. That was a better ceiling than the thirty minutes originally proposed, and it was
still a ceiling on a thing that should not have been happening.

What survives is the **age limit** — and what it means changed with it. It is not about a fix
going stale: a movement tracker is right however long it stays quiet, because the silence is the
evidence. It guards the other case, a photograph from a day the track does not cover, which would
otherwise take the last fix of a different trip and look exactly like a real answer. Half an hour
was the wrong shape of number for that job — it refused the café afternoon the rule exists to
answer — so the default is **twelve hours**: longer than any silence within a running day, short
enough to refuse another journey. **Zero means no limit.** Every row reports the age of the fix it
used, which is the whole safeguard: an answer carried forward for six hours has to look like one.

## Deviations from the plan, and why

### `ExifIter::find_tz_offset()` does not exist

The plan cited it for reading the EXIF UTC offset. It is a method on `nom-exif`'s **`IfdIter`**, not
on `ExifIter`, and is unreachable from the iterator `read_meta` holds. The offset tags are read in
the entry walk instead, in `collect_exif` — which is better placed anyway: that function is a pure
fold over tags, so the preference between `OffsetTimeOriginal`, `OffsetTimeDigitized` and
`OffsetTime` is testable from a tag list with no file behind it.

### The estimator scores real time zones, not a quarter-hour sweep

The plan proposed sweeping −12:00 to +14:00 in quarter-hour steps. Measured against the actual
sample track, that sweep returns **+01:45** for photographs taken at +02:00: with a track sampled
every five minutes the two scores differ by less than its own spacing, and no clock on earth is set
to +01:45.

The candidates are now the **thirty-eight offsets a clock is actually set to** — a camera's clock
is set to a *zone* — which includes the five on a quarter or a half hour, so India, Nepal, Iran,
Newfoundland and central Australia are not rounded away. Alongside the winner the estimator reports
the **range of offsets the evidence cannot separate from it**, and `confident` is true only when one
candidate survives *and* the median photograph is within half an hour of a fix. On the specimen
track with seven photographs it returns +02:00, confidently; a week away from the track it returns
something and says it is not confident.

### `ExifWriter::set_tags`, not `write_gps`

`media` may not depend on `tools`, and a driver method that knew what a GPS tag *means* would put
the dependency the wrong way round. The writer takes rendered assignments and applies them in one
`-execute`; what the tags mean belongs to the tool that decided to write them. Nine tags per file
through `set_tag` would have cost nine processes a file, which is the whole of G4.

### `Ledger::points_around`, which the plan did not foresee

Found by the end-to-end test. The timeline is read in a window chosen from the photographs, and the
fixes bracketing them can be **any** distance outside it — an overnight leaves ten hours. Without
the neighbours, a photograph inside that gap was refused with *"3 h 32 min after the last fix in the
library"* when the library holds fixes three hours later. The refusal was false, and the tool would
have taught somebody the wrong thing about their own track. Two indexed queries fixed it.

### `MediaMeta` no longer derives `Eq`

Coordinates are floats. Nothing needed `Eq`; the type now carries a comment saying why it is
absent, and that whether two fixes are "the same" is a question about tolerance answered by
`tools::geotag::same_position`.

## Defects found while building this

| | |
|---|---|
| **A false refusal** | See `points_around` above. Found by the first end-to-end test, not by any unit test — the window is a property of the seam between the ledger and the matcher. |
| **`npm --prefix frontend/shared run build` was already failing on `main`** | `client.ts` imported `ImageToolRequest` and never used it; `tsc` rejects it under `noUnusedLocals`. A gate in `CLAUDE.md` that does not pass on a clean checkout. Removed the import. |
| **A test constant, not the code** | The first EXIF rendering test asserted a date a day out because the epoch in the fixture was hand-written. The renderer was right. |

## Acceptance

| Claim | How it is held up |
|---|---|
| The specimen export reads as 50 fixes with the right first and last | `the_sample_track_reads_as_fifty_fixes`, against a frozen copy of the real file |
| Every answer is a fix the phone recorded, and never one from later | `every_answer_is_a_fix_the_phone_recorded`, `the_answer_is_never_a_fix_from_later` — both over every second of a track |
| A fix three hours old beats one twenty-nine minutes away | `a_fix_three_hours_old_beats_one_twenty_nine_minutes_away`, with `Nearest` shown taking the other one |
| An overnight photograph takes where the phone last was, with the age reported | `with_no_ceiling_an_overnight_photograph_takes_where_the_phone_last_was`, end to end against the specimen |
| One minute past the last fix still gets that fix | `a_photograph_at_the_near_edge_of_a_wide_gap_takes_the_fix_beside_it` |
| The same export twice adds nothing | `the_same_export_fed_twice_adds_nothing_the_second_time` |
| A reading that moved 11 cm is the same reading; 10 m is a disagreement | `a_fix_that_moved_a_hand_span_is_the_same_fix`, `a_fix_that_moved_a_street_away_is_a_disagreement_with_a_distance_on_it` |
| A decision that is no longer needed is reported | `a_decision_about_an_instant_that_is_no_longer_in_dispute_is_reported` |
| The commit works from the library as it is now | `the_commit_works_from_the_library_as_it_is_now_not_as_the_preview_found_it` |
| A migrated database and a fresh one have the same schema | `a_database_from_before_the_track_library_ends_up_with_the_same_schema_as_a_fresh_one`, comparing all of `sqlite_master` |
| A position written reads back as that position | `a_position_written_into_a_photograph_reads_back_as_that_position`, real `exiftool`, real JPEG |
| Fifty files, one process | `writing_a_position_into_fifty_files_spawns_one_exiftool` (G4) |
| A plan writes nothing | `a_plan_writes_nothing`, asserted on the modification time |
| A photograph that already knows where it was is left alone | `a_photograph_that_already_knows_where_it_was_is_left_alone` |
| The offset can be read off the photographs | `the_offset_can_be_read_off_the_photographs_themselves`, against the specimen track |

## Gates

| | Before | After |
|---|---|---|
| `cargo test --workspace` | 504 | **628** |
| `cargo test -p phototools-core` (G2) | 423 | **547** |
| `check:layout` | 9 routes | **10 routes**, `/geotag` included |

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -D warnings`, both front-end
typechecks, both `check:transport` runs and `check:ingest` all pass.

## Ideas not acted on (G11)

- **Video.** QuickTime GPS is ISO 6709 in `UserData` and a different write. Videos are listed as
  `NotSupported` with the reason on the row.
- **A map.** Tiles mean a CDN, and the desktop application has to work with the machine offline
  (MV-7.3). A local SVG plot of the timeline with the photographs marked on it would stay offline,
  if a picture is ever wanted.
- **Other track formats.** Google Takeout's location history is JSON and would be a second reader.
- **Attestation.** Deleting a track removes the fixes still attributed to it, which can take a
  position another stored file also contains. Re-importing that file restores it, and the GPX text
  is kept so it can be. A `point_sources` table would make this exact; with every fix coming from
  one phone it is not worth the second table.
- **G5 and this tool.** It writes wherever it is pointed, and nothing in it distinguishes a card
  from a folder. That check lives in the configured roots and is worth revisiting for every tool at
  once, not for this one alone.
