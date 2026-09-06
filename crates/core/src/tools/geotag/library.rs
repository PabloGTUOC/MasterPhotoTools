//! The track library: importing a GPX file into the timeline, and the
//! disagreements that turns up.
//!
//! Importing is **a preview and a commit**, the shape every other tool here
//! uses. The preview diffs the file against the timeline and writes nothing;
//! the commit takes the decisions and applies them in one transaction.

use super::exif::{self, ExifPoint};
use super::gpx::{self, ParsedTrack, RejectedPoint};
use super::{metres_between, same_position, TrackPoint};
use crate::error::Error;
use crate::ledger::{Ledger, TrackConflictRecord, TrackRow};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

/// How many fixes the preview renders into EXIF's form.
///
/// Enough to see that the conversion is real and reads the way the file does,
/// short of turning the preview into the file itself.
const SAMPLE_POINTS: usize = 5;

/// A GPX file, read and parsed but not yet stored.
#[derive(Debug, Clone)]
pub struct TrackFile {
    /// The sha256 of the bytes. Two exports of the same afternoon are two
    /// files; the *same* export fed twice is one, and this is what knows.
    pub id: String,
    pub name: String,
    pub source_path: String,
    pub gpx: String,
    pub parsed: ParsedTrack,
}

/// Read and parse a `.gpx`.
///
/// The path is expected to have been resolved against the configured roots
/// already (G6); core tools take a path that has been through `Config`.
pub fn read_track(path: &Path) -> Result<TrackFile, Error> {
    let bytes = std::fs::read(path)?;
    // The same hex helper the ingest hashes use, so an id computed here and
    // one computed there are the same string for the same bytes.
    let id = crate::ingest::scanner::hex(&Sha256::digest(&bytes));
    let gpx = String::from_utf8(bytes)
        .map_err(|_| Error::Config("That file is not text, so it is not GPX".into()))?;
    let parsed = gpx::parse(&gpx)?;

    Ok(TrackFile {
        id,
        name: path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "track.gpx".into()),
        source_path: path.display().to_string(),
        gpx,
        parsed,
    })
}

/// One instant where a file and the library disagree.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointConflict {
    pub at: i64,
    /// What the timeline holds now, and which import put it there.
    pub existing: TrackPoint,
    pub existing_track_id: String,
    pub existing_track_name: String,
    /// What this file says instead.
    pub incoming: TrackPoint,
    /// How far apart the two are, over the ground.
    ///
    /// The number that says which fault this is: three metres is two apps
    /// disagreeing about one fix, two kilometres is a different device or an
    /// export with the wrong offset.
    pub metres: f64,
}

/// What an import would do, without doing it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackImportPreview {
    pub id: String,
    pub name: String,
    pub creator: Option<String>,
    /// When this exact file was first imported, if it has been before.
    pub already_imported_at: Option<i64>,
    /// Timed fixes the file holds.
    pub point_count: usize,
    /// Instants the timeline does not hold yet.
    pub new_points: usize,
    /// Instants it holds with the same position — nothing to do.
    pub identical_points: usize,
    pub conflicts: Vec<PointConflict>,
    /// Points in the file that cannot be used, and why.
    pub rejected: Vec<RejectedPoint>,
    pub first_fix: Option<i64>,
    pub last_fix: Option<i64>,
    /// The first few fixes in the form they will be written.
    pub sample: Vec<ExifPoint>,
}

/// Which side of a disagreement to keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Take {
    Existing,
    New,
}

/// The default for every conflict this import turns up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolution {
    KeepExisting,
    TakeNew,
}

impl Resolution {
    fn take(self) -> Take {
        match self {
            Resolution::KeepExisting => Take::Existing,
            Resolution::TakeNew => Take::New,
        }
    }
}

/// One instant decided individually, against the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Decision {
    pub at: i64,
    pub take: Take,
}

/// What an import did.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackImportResult {
    pub id: String,
    pub name: String,
    pub added: usize,
    pub identical: usize,
    pub kept_existing: usize,
    pub took_new: usize,
    /// Overrides naming an instant that is no longer in dispute.
    ///
    /// Reported rather than dropped: the library moved between the preview and
    /// the commit, and somebody who decided about a conflict deserves to know
    /// their decision was not needed.
    pub stale_overrides: Vec<i64>,
    pub rejected: Vec<RejectedPoint>,
    pub already_imported: bool,
}

impl TrackImportResult {
    /// The closing line, for a screen and for a job's summary.
    pub fn describe(&self) -> String {
        if self.added == 0 && self.took_new == 0 {
            return format!(
                "Nothing new: all {} fixes in {} were already in the library",
                self.identical, self.name
            );
        }
        let mut line = format!("{} fixes added from {}", self.added, self.name);
        if self.identical > 0 {
            line.push_str(&format!(", {} already known", self.identical));
        }
        if self.kept_existing + self.took_new > 0 {
            line.push_str(&format!(
                ", {} disagreement(s) settled ({} kept, {} replaced)",
                self.kept_existing + self.took_new,
                self.kept_existing,
                self.took_new
            ));
        }
        line
    }
}

/// The three buckets, computed against the timeline as it stands.
struct Diff {
    new: Vec<TrackPoint>,
    identical: usize,
    conflicts: Vec<PointConflict>,
}

fn diff(ledger: &Ledger, file: &TrackFile) -> Result<Diff, Error> {
    let instants: Vec<i64> = file.parsed.points.iter().map(|p| p.at).collect();
    let held = ledger.points_at(&instants)?;

    // Track names are looked up once each rather than per conflict: a file that
    // disagrees about one instant usually disagrees about many, and they all
    // came from the same handful of imports.
    let mut names: BTreeMap<String, String> = BTreeMap::new();

    let mut result = Diff {
        new: Vec::new(),
        identical: 0,
        conflicts: Vec::new(),
    };

    for incoming in &file.parsed.points {
        match held.get(&incoming.at) {
            None => result.new.push(*incoming),
            Some((existing, track_id)) => {
                if same_position(existing, incoming) {
                    result.identical += 1;
                    continue;
                }
                if !names.contains_key(track_id) {
                    let name = ledger
                        .track(track_id)?
                        .map(|t| t.name)
                        .unwrap_or_else(|| track_id.clone());
                    names.insert(track_id.clone(), name);
                }
                result.conflicts.push(PointConflict {
                    at: incoming.at,
                    existing: *existing,
                    existing_track_id: track_id.clone(),
                    existing_track_name: names[track_id].clone(),
                    incoming: *incoming,
                    metres: metres_between(existing, incoming),
                });
            }
        }
    }

    Ok(result)
}

/// What importing this file would do. Writes nothing.
pub fn preview_import(ledger: &Ledger, file: &TrackFile) -> Result<TrackImportPreview, Error> {
    let diff = diff(ledger, file)?;

    Ok(TrackImportPreview {
        id: file.id.clone(),
        name: file.name.clone(),
        creator: file.parsed.creator.clone(),
        already_imported_at: ledger.track(&file.id)?.map(|t| t.imported_at),
        point_count: file.parsed.points.len(),
        new_points: diff.new.len(),
        identical_points: diff.identical,
        conflicts: diff.conflicts,
        rejected: file.parsed.rejected.clone(),
        first_fix: file.parsed.first_fix(),
        last_fix: file.parsed.last_fix(),
        sample: file
            .parsed
            .points
            .iter()
            .take(SAMPLE_POINTS)
            .map(|p| exif::render(p, true))
            .collect(),
    })
}

/// Import the file, applying the decisions.
///
/// **The diff is recomputed here rather than carried from the preview.** The
/// library may have moved since — another import, a deletion — and what matters
/// is what is true now. The same reasoning stops a publish trusting its own dry
/// run.
pub fn commit_import(
    ledger: &Ledger,
    file: &TrackFile,
    resolution: Resolution,
    overrides: &[Decision],
    now: i64,
) -> Result<TrackImportResult, Error> {
    let diff = diff(ledger, file)?;
    let decided: BTreeMap<i64, Take> = overrides.iter().map(|d| (d.at, d.take)).collect();

    let mut to_write = diff.new.clone();
    let mut conflict_records = Vec::new();
    let mut kept_existing = 0usize;
    let mut took_new = 0usize;

    for conflict in &diff.conflicts {
        let take = decided
            .get(&conflict.at)
            .copied()
            .unwrap_or_else(|| resolution.take());
        let (kept, other) = match take {
            Take::New => {
                took_new += 1;
                to_write.push(conflict.incoming);
                (conflict.incoming, conflict.existing)
            }
            Take::Existing => {
                kept_existing += 1;
                (conflict.existing, conflict.incoming)
            }
        };
        conflict_records.push(TrackConflictRecord {
            at: conflict.at,
            kept,
            other,
            metres: conflict.metres,
            decision: match take {
                Take::New => "took-new".into(),
                Take::Existing => "kept-existing".into(),
            },
        });
    }

    let in_dispute: Vec<i64> = diff.conflicts.iter().map(|c| c.at).collect();
    let stale_overrides: Vec<i64> = decided
        .keys()
        .copied()
        .filter(|at| !in_dispute.contains(at))
        .collect();

    let existing = ledger.track(&file.id)?;
    let bounds = file.parsed.bounds();
    let row = TrackRow {
        id: file.id.clone(),
        name: file.name.clone(),
        source_path: file.source_path.clone(),
        creator: file.parsed.creator.clone(),
        // When the file was *first* seen. A re-import restores whatever has
        // since been deleted, and dating that as a fresh import would lose the
        // one date somebody might reasonably look for.
        imported_at: existing.as_ref().map(|t| t.imported_at).unwrap_or(now),
        point_count: file.parsed.points.len() as i64,
        points_added: to_write.len() as i64,
        points_identical: diff.identical as i64,
        points_conflicting: diff.conflicts.len() as i64,
        first_fix: file.parsed.first_fix(),
        last_fix: file.parsed.last_fix(),
        bounds,
    };

    ledger.record_track_import(&row, &file.gpx, &to_write, &conflict_records)?;

    Ok(TrackImportResult {
        id: file.id.clone(),
        name: file.name.clone(),
        added: diff.new.len(),
        identical: diff.identical,
        kept_existing,
        took_new,
        stale_overrides,
        rejected: file.parsed.rejected.clone(),
        already_imported: existing.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A document holding exactly these fixes.
    fn document(points: &[(&str, f64, f64)]) -> String {
        let body: String = points
            .iter()
            .map(|(time, lat, lon)| {
                format!(
                    "<trkpt lat=\"{lat}\" lon=\"{lon}\"><ele>36.4</ele>\
                     <time>{time}</time></trkpt>\n"
                )
            })
            .collect();
        format!("<gpx creator=\"OwnTracks\"><trk><trkseg>\n{body}</trkseg></trk></gpx>")
    }

    /// A parsed file with a given name, without touching a disk.
    fn file(name: &str, points: &[(&str, f64, f64)]) -> TrackFile {
        let gpx = document(points);
        let id = crate::ingest::scanner::hex(&Sha256::digest(gpx.as_bytes()));
        TrackFile {
            id,
            name: name.into(),
            source_path: format!("/tracks/{name}"),
            parsed: gpx::parse(&gpx).unwrap(),
            gpx,
        }
    }

    const MONDAY: [(&str, f64, f64); 3] = [
        ("2026-09-02T19:40:44Z", 52.509998, 13.419901),
        ("2026-09-02T19:45:50Z", 52.516569, 13.402709),
        ("2026-09-02T19:50:56Z", 52.517415, 13.383984),
    ];

    fn ledger() -> Ledger {
        Ledger::open_in_memory().unwrap()
    }

    fn import(ledger: &Ledger, file: &TrackFile) -> TrackImportResult {
        commit_import(ledger, file, Resolution::KeepExisting, &[], 1_788_600_000).unwrap()
    }

    #[test]
    fn a_first_import_adds_every_fix_the_file_holds() {
        let ledger = ledger();
        let track = file("monday.gpx", &MONDAY);

        let preview = preview_import(&ledger, &track).unwrap();
        assert_eq!(preview.new_points, 3);
        assert_eq!(preview.identical_points, 0);
        assert_eq!(preview.conflicts, vec![]);
        assert_eq!(preview.already_imported_at, None);
        assert_eq!(preview.creator.as_deref(), Some("OwnTracks"));

        let result = import(&ledger, &track);
        assert_eq!(result.added, 3);
        assert_eq!(ledger.points_between(0, i64::MAX).unwrap().len(), 3);
    }

    #[test]
    fn a_preview_writes_nothing() {
        let ledger = ledger();
        let track = file("monday.gpx", &MONDAY);
        preview_import(&ledger, &track).unwrap();
        preview_import(&ledger, &track).unwrap();

        assert_eq!(ledger.points_between(0, i64::MAX).unwrap().len(), 0);
        assert_eq!(ledger.tracks().unwrap().len(), 0);
    }

    #[test]
    fn the_same_export_fed_twice_adds_nothing_the_second_time() {
        let ledger = ledger();
        let track = file("monday.gpx", &MONDAY);
        import(&ledger, &track);

        let preview = preview_import(&ledger, &track).unwrap();
        assert_eq!(preview.new_points, 0);
        assert_eq!(preview.identical_points, 3);
        assert_eq!(preview.already_imported_at, Some(1_788_600_000));

        let again = import(&ledger, &track);
        assert_eq!(again.added, 0);
        assert_eq!(again.identical, 3);
        assert!(again.already_imported);
        assert_eq!(ledger.points_between(0, i64::MAX).unwrap().len(), 3);
        assert_eq!(ledger.tracks().unwrap().len(), 1);
    }

    #[test]
    fn a_second_export_overlapping_the_first_contributes_only_what_is_new() {
        let ledger = ledger();
        import(&ledger, &file("monday.gpx", &MONDAY));

        // The same afternoon exported again, with one more fix on the end.
        let mut longer = MONDAY.to_vec();
        longer.push(("2026-09-02T19:55:57Z", 52.525582, 13.373768));
        let preview = preview_import(&ledger, &file("monday-again.gpx", &longer)).unwrap();

        assert_eq!(preview.identical_points, 3);
        assert_eq!(preview.new_points, 1);
        assert_eq!(preview.conflicts, vec![]);
    }

    #[test]
    fn a_fix_that_moved_by_a_hand_span_is_the_same_fix() {
        let ledger = ledger();
        import(&ledger, &file("monday.gpx", &MONDAY));

        // A re-export that rounded in the seventh decimal.
        let rounded = [
            ("2026-09-02T19:40:44Z", 52.5099981, 13.4199011),
            ("2026-09-02T19:45:50Z", 52.516569, 13.402709),
            ("2026-09-02T19:50:56Z", 52.517415, 13.383984),
        ];
        let preview = preview_import(&ledger, &file("rounded.gpx", &rounded)).unwrap();
        assert_eq!(preview.identical_points, 3);
        assert_eq!(preview.conflicts, vec![]);
    }

    #[test]
    fn a_fix_that_moved_a_street_away_is_a_disagreement_with_a_distance_on_it() {
        let ledger = ledger();
        import(&ledger, &file("monday.gpx", &MONDAY));

        let moved = [
            ("2026-09-02T19:40:44Z", 52.509998, 13.419901),
            ("2026-09-02T19:45:50Z", 52.517569, 13.402709), // 111 m north
            ("2026-09-02T19:50:56Z", 52.517415, 13.383984),
        ];
        let preview = preview_import(&ledger, &file("moved.gpx", &moved)).unwrap();

        assert_eq!(preview.identical_points, 2);
        assert_eq!(preview.new_points, 0);
        assert_eq!(preview.conflicts.len(), 1);

        let conflict = &preview.conflicts[0];
        assert_eq!(conflict.existing_track_name, "monday.gpx");
        assert!(
            (110.0..113.0).contains(&conflict.metres),
            "expected about 111 m, got {}",
            conflict.metres
        );
    }

    #[test]
    fn keeping_what_the_library_holds_leaves_the_timeline_alone_and_still_records_the_dispute() {
        let ledger = ledger();
        import(&ledger, &file("monday.gpx", &MONDAY));

        let moved = file(
            "moved.gpx",
            &[("2026-09-02T19:45:50Z", 52.517569, 13.402709)],
        );
        let result = commit_import(
            &ledger,
            &moved,
            Resolution::KeepExisting,
            &[],
            1_788_600_001,
        )
        .unwrap();

        assert_eq!(result.kept_existing, 1);
        assert_eq!(result.took_new, 0);

        let held = ledger.points_between(0, i64::MAX).unwrap();
        let disputed = held.iter().find(|p| p.at == 1_788_378_350).unwrap();
        assert_eq!(
            disputed.lat, 52.516569,
            "the timeline should not have moved"
        );

        // The decision is on the record: without it, the library says something
        // one of its own stored files does not, and nothing says why.
        let recorded = ledger.conflicts_for_track(&moved.id).unwrap();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].decision, "kept-existing");
        assert_eq!(recorded[0].kept.lat, 52.516569);
        assert_eq!(recorded[0].other.lat, 52.517569);
    }

    #[test]
    fn taking_the_new_reading_replaces_the_point_and_records_what_it_replaced() {
        let ledger = ledger();
        import(&ledger, &file("monday.gpx", &MONDAY));

        let moved = file(
            "moved.gpx",
            &[("2026-09-02T19:45:50Z", 52.517569, 13.402709)],
        );
        let result =
            commit_import(&ledger, &moved, Resolution::TakeNew, &[], 1_788_600_001).unwrap();

        assert_eq!(result.took_new, 1);
        let held = ledger.points_between(0, i64::MAX).unwrap();
        let disputed = held.iter().find(|p| p.at == 1_788_378_350).unwrap();
        assert_eq!(disputed.lat, 52.517569);

        let recorded = ledger.conflicts_for_track(&moved.id).unwrap();
        assert_eq!(recorded[0].decision, "took-new");
        assert_eq!(recorded[0].kept.lat, 52.517569);
        assert_eq!(recorded[0].other.lat, 52.516569);
    }

    #[test]
    fn one_instant_can_be_decided_against_the_default() {
        let ledger = ledger();
        import(&ledger, &file("monday.gpx", &MONDAY));

        let moved = file(
            "moved.gpx",
            &[
                ("2026-09-02T19:40:44Z", 52.519998, 13.419901),
                ("2026-09-02T19:45:50Z", 52.517569, 13.402709),
            ],
        );
        let result = commit_import(
            &ledger,
            &moved,
            Resolution::KeepExisting,
            &[Decision {
                at: 1_788_378_044,
                take: Take::New,
            }],
            1_788_600_001,
        )
        .unwrap();

        assert_eq!(result.took_new, 1);
        assert_eq!(result.kept_existing, 1);

        let held = ledger.points_between(0, i64::MAX).unwrap();
        assert_eq!(
            held.iter().find(|p| p.at == 1_788_378_044).unwrap().lat,
            52.519998,
            "the overridden instant should have taken the new reading"
        );
        assert_eq!(
            held.iter().find(|p| p.at == 1_788_378_350).unwrap().lat,
            52.516569,
            "the rest should have followed the default"
        );
    }

    #[test]
    fn a_decision_about_an_instant_that_is_no_longer_in_dispute_is_reported() {
        // The library moved between the preview and the commit. Dropping the
        // decision silently would leave somebody believing they had settled
        // something.
        let ledger = ledger();
        let track = file("monday.gpx", &MONDAY);

        let result = commit_import(
            &ledger,
            &track,
            Resolution::KeepExisting,
            &[Decision {
                at: 1_788_378_044,
                take: Take::New,
            }],
            1_788_600_000,
        )
        .unwrap();

        assert_eq!(result.stale_overrides, vec![1_788_378_044]);
    }

    #[test]
    fn the_commit_works_from_the_library_as_it_is_now_not_as_the_preview_found_it() {
        let ledger = ledger();
        let moved = file(
            "moved.gpx",
            &[("2026-09-02T19:45:50Z", 52.517569, 13.402709)],
        );

        // Previewed against an empty library: one new point, no conflict.
        let preview = preview_import(&ledger, &moved).unwrap();
        assert_eq!(preview.new_points, 1);
        assert_eq!(preview.conflicts.len(), 0);

        // Somebody else imports the Monday track in between.
        import(&ledger, &file("monday.gpx", &MONDAY));

        // The commit sees the conflict the preview could not have known about,
        // and applies the default rather than writing over it.
        let result = commit_import(
            &ledger,
            &moved,
            Resolution::KeepExisting,
            &[],
            1_788_600_002,
        )
        .unwrap();
        assert_eq!(result.added, 0);
        assert_eq!(result.kept_existing, 1);
        assert_eq!(
            ledger
                .points_between(0, i64::MAX)
                .unwrap()
                .iter()
                .find(|p| p.at == 1_788_378_350)
                .unwrap()
                .lat,
            52.516569
        );
    }

    #[test]
    fn a_track_row_records_what_the_import_did() {
        let ledger = ledger();
        let track = file("monday.gpx", &MONDAY);
        import(&ledger, &track);

        let rows = ledger.tracks().unwrap();
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.name, "monday.gpx");
        assert_eq!(row.point_count, 3);
        assert_eq!(row.points_added, 3);
        assert_eq!(row.first_fix, Some(1_788_378_044));
        assert_eq!(row.last_fix, Some(1_788_378_656));
        assert_eq!(row.creator.as_deref(), Some("OwnTracks"));
        let (min_lat, min_lon, max_lat, max_lon) = row.bounds.unwrap();
        assert!(min_lat < max_lat && min_lon < max_lon);
    }

    #[test]
    fn deleting_a_track_takes_its_fixes_with_it() {
        let ledger = ledger();
        let track = file("monday.gpx", &MONDAY);
        import(&ledger, &track);

        let removed = ledger.delete_track(&track.id).unwrap();
        assert_eq!(removed, 3);
        assert_eq!(ledger.points_between(0, i64::MAX).unwrap().len(), 0);
        assert_eq!(ledger.tracks().unwrap().len(), 0);
    }

    #[test]
    fn re_importing_a_stored_file_restores_fixes_a_deletion_took_with_it() {
        // The deliberate limit made recoverable: a point another file also
        // attested to goes when its first contributor does, and feeding that
        // other file again brings it back.
        let ledger = ledger();
        let monday = file("monday.gpx", &MONDAY);
        let again = file("monday-again.gpx", &MONDAY);
        import(&ledger, &monday);
        import(&ledger, &again); // adds nothing; every fix is already held

        ledger.delete_track(&monday.id).unwrap();
        assert_eq!(ledger.points_between(0, i64::MAX).unwrap().len(), 0);

        let restored = import(&ledger, &again);
        assert_eq!(restored.added, 3);
        assert_eq!(ledger.points_between(0, i64::MAX).unwrap().len(), 3);
    }

    #[test]
    fn a_window_returns_only_the_window() {
        let ledger = ledger();
        import(&ledger, &file("monday.gpx", &MONDAY));

        let window = ledger.points_between(1_788_378_044, 1_788_378_350).unwrap();
        assert_eq!(window.len(), 2);
        assert!(window.windows(2).all(|w| w[0].at < w[1].at));
    }

    #[test]
    fn the_preview_shows_the_fixes_in_the_form_they_will_be_written() {
        let ledger = ledger();
        let preview = preview_import(&ledger, &file("monday.gpx", &MONDAY)).unwrap();

        assert_eq!(preview.sample.len(), 3);
        assert_eq!(preview.sample[0].latitude_ref, "N");
        assert_eq!(preview.sample[0].date_stamp, "2026:09:02");
        assert_eq!(preview.sample[0].time_stamp, "19:40:44");
    }

    #[test]
    fn points_the_file_cannot_offer_are_carried_through_to_the_result() {
        let ledger = ledger();
        let gpx = "<gpx creator=\"OwnTracks\"><trk><trkseg>\
                   <trkpt lat=\"52.5\" lon=\"13.4\"><time>2026-09-02T19:40:44Z</time></trkpt>\
                   <trkpt lat=\"52.6\" lon=\"13.5\"></trkpt>\
                   </trkseg></trk></gpx>";
        let track = TrackFile {
            id: "abc".into(),
            name: "partial.gpx".into(),
            source_path: "/tracks/partial.gpx".into(),
            parsed: gpx::parse(gpx).unwrap(),
            gpx: gpx.into(),
        };

        let result = import(&ledger, &track);
        assert_eq!(result.added, 1);
        assert_eq!(result.rejected.len(), 1);
    }
}
