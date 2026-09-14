//! The phone reporting where it was, and the day that becomes a track.
//!
//! **Beyond the specification**, like the rest of `geotag`; the reasoning is in
//! `docs/owntracks-plan.md`.
//!
//! OwnTracks posts one small JSON object per fix. Those arrive through the day
//! and wait in `device_fixes`; once a day is complete it is written as a GPX
//! document and imported like any file, which is what gives every position in
//! the library a document behind it.
//!
//! **The instant is `tst`, the moment the fix was taken — never when it
//! arrived.** A phone in a tunnel, or abroad with data off, queues its fixes
//! and posts them all at once; each still knows when it happened, so each lands
//! on the day it belongs to.

use super::gpx;
use super::library::{self, TrackFile};
use super::{PointSource, TrackPoint};
use crate::error::Error;
use crate::ledger::Ledger;
use serde::{Deserialize, Serialize};

/// How long after a day ends before it is written up, in seconds.
///
/// Two hours: a phone that was asleep at midnight, or a laptop lid closed over
/// the boundary, still gets its last fixes into the right day. Longer would
/// delay a day somebody wants to geotag that morning; shorter starts splitting
/// evenings across two tracks.
pub const GRACE_SECONDS: i64 = 2 * 3600;

/// One message as OwnTracks sends it.
///
/// Only the fields a timeline needs are named. Battery, velocity, connection,
/// the geofence a phone crossed and the accuracy it claims are read past —
/// `serde` ignores what is not asked for, which is also what makes this
/// tolerant of OwnTracks adding fields.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Report {
    /// `location`, `transition`, `lwt`, `waypoints`, `card`…
    #[serde(rename = "_type")]
    pub kind: String,
    /// Unix seconds, UTC — when the fix was taken.
    pub tst: Option<i64>,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    /// Metres above sea level, where the phone had one.
    pub alt: Option<f64>,
    /// The device's two-letter tracker id.
    pub tid: Option<String>,
}

/// What a payload turned out to be.
#[derive(Debug, Clone, PartialEq)]
pub enum Accepted {
    /// A fix, ready to store.
    Fix {
        point: TrackPoint,
        device: Option<String>,
    },
    /// A message this application has no use for, and says so without
    /// complaining: refusing would have the phone retry forever for something
    /// that was never going to be kept.
    Ignored { kind: String },
}

/// Read one message.
///
/// A `location` without a position or without a time is **not** a fix, and is
/// refused rather than stored at the epoch or at the equator.
pub fn read(report: &Report) -> Result<Accepted, Error> {
    if report.kind != "location" {
        return Ok(Accepted::Ignored {
            kind: report.kind.clone(),
        });
    }

    let (Some(at), Some(lat), Some(lon)) = (report.tst, report.lat, report.lon) else {
        return Err(Error::Config(
            "a location report needs tst, lat and lon".into(),
        ));
    };
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
        return Err(Error::Config(format!(
            "{lat:.5}, {lon:.5} is not a place on Earth"
        )));
    }

    Ok(Accepted::Fix {
        point: TrackPoint {
            at,
            lat,
            lon,
            // Absent stays absent: `ele = None` means "no altitude was
            // recorded", which is not the same claim as zero.
            ele: report.alt,
        },
        device: report.tid.clone().filter(|t| !t.trim().is_empty()),
    })
}

/// The start of the local day an instant falls in, as Unix seconds UTC.
///
/// Which day a fix belongs to is a local question. An evening in Berlin is the
/// same day as that morning, and UTC disagrees with that for two hours of it.
pub fn day_start(at: i64, offset_seconds: i64) -> i64 {
    let local = at + offset_seconds;
    (local - local.rem_euclid(86_400)) - offset_seconds
}

/// The local days that have staged fixes and are complete enough to write up.
///
/// A day is due once it has ended and the grace period has passed. Days are
/// returned oldest first, so a week of catch-up is written up in order.
pub fn days_due(
    ledger: &Ledger,
    now: i64,
    offset_seconds: i64,
    grace_seconds: i64,
) -> Result<Vec<i64>, Error> {
    let Some((first, last)) = ledger.device_fix_extent()? else {
        return Ok(Vec::new());
    };

    let mut due = Vec::new();
    let mut day = day_start(first, offset_seconds);
    let last_day = day_start(last, offset_seconds);
    while day <= last_day {
        // The day has ended, and the grace period after it has passed.
        if day + 86_400 + grace_seconds <= now {
            due.push(day);
        }
        day += 86_400;
    }
    Ok(due)
}

/// What writing up one day did.
#[derive(Debug, Clone, PartialEq)]
pub struct RolledUp {
    pub day: i64,
    pub fixes: usize,
    pub track_id: String,
    pub name: String,
    /// Points this added to the timeline. Fewer than `fixes` where the library
    /// already held some of them.
    pub added: usize,
}

/// Write one local day's staged fixes up as a track.
///
/// The document is the point: it is what every other position in the library
/// has behind it, and what lets this day be exported, re-imported or synced to
/// the other machine like anything else.
///
/// **Running this twice for one day is not two tracks.** The same fixes produce
/// the same text, which hashes to the same id, and the second import adds
/// nothing. A fix that arrives after its day was written up produces a second,
/// amended track for that day, which merges by instant like any import.
pub fn roll_up(
    ledger: &Ledger,
    day: i64,
    offset_seconds: i64,
    now: i64,
) -> Result<Option<RolledUp>, Error> {
    let from = day;
    let to = day + 86_399;

    let points = ledger.device_fixes_between(from, to)?;
    if points.is_empty() {
        return Ok(None);
    }

    let device = ledger.device_in(from, to)?;
    let label = local_date(day, offset_seconds);
    let name = format!("OwnTracks {label}");
    let source_path = match &device {
        Some(tid) => format!("(reported by {tid})"),
        None => "(reported by a device)".to_string(),
    };

    let text = gpx::write(&name, "OwnTracks", &points)?;
    // `Recorded`: a device recorded these. It is the one kind of claim this
    // road can make, and the only one it is allowed to make.
    let file = TrackFile::from_text(&name, &source_path, PointSource::Recorded, &text)?;
    let result =
        library::commit_import(ledger, &file, library::Resolution::KeepExisting, &[], now)?;

    // Only after the import has committed: a crash between the two leaves the
    // fixes staged, and the next run writes them up again to the same id.
    ledger.clear_device_fixes(from, to)?;

    Ok(Some(RolledUp {
        day,
        fixes: points.len(),
        track_id: result.id,
        name,
        added: result.added,
    }))
}

/// Write up every day that is due. Returns what each one did.
pub fn roll_up_due(
    ledger: &Ledger,
    now: i64,
    offset_seconds: i64,
    grace_seconds: i64,
) -> Result<Vec<RolledUp>, Error> {
    let mut done = Vec::new();
    for day in days_due(ledger, now, offset_seconds, grace_seconds)? {
        if let Some(rolled) = roll_up(ledger, day, offset_seconds, now)? {
            done.push(rolled);
        }
    }
    Ok(done)
}

/// `2026-09-13` in the offset the days are counted in.
fn local_date(day: i64, offset_seconds: i64) -> String {
    chrono::DateTime::from_timestamp(day + offset_seconds, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| day.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BERLIN: i64 = 2 * 3600;

    fn location(tst: i64, lat: f64, lon: f64) -> Report {
        Report {
            kind: "location".into(),
            tst: Some(tst),
            lat: Some(lat),
            lon: Some(lon),
            alt: None,
            tid: Some("pg".into()),
        }
    }

    fn stage(ledger: &Ledger, report: &Report, received_at: i64) {
        match read(report).unwrap() {
            Accepted::Fix { point, device } => {
                ledger
                    .record_device_fix(&point, device.as_deref(), received_at)
                    .unwrap();
            }
            Accepted::Ignored { .. } => panic!("expected a fix"),
        }
    }

    #[test]
    fn a_location_report_is_read_as_a_fix() {
        let report = location(1_757_000_000, 52.5, 13.4);
        match read(&report).unwrap() {
            Accepted::Fix { point, device } => {
                assert_eq!(point.at, 1_757_000_000);
                assert_eq!(device.as_deref(), Some("pg"));
                assert_eq!(point.ele, None, "no altitude was reported");
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn anything_that_is_not_a_location_is_ignored_rather_than_refused() {
        // A geofence crossing, a last-will message, a waypoint list: the phone
        // must not be made to retry forever for a message we will never keep.
        for kind in ["transition", "lwt", "waypoints", "card"] {
            let report = Report {
                kind: kind.into(),
                ..location(1, 0.0, 0.0)
            };
            assert_eq!(
                read(&report).unwrap(),
                Accepted::Ignored { kind: kind.into() }
            );
        }
    }

    #[test]
    fn a_location_without_a_time_or_a_place_is_refused() {
        let mut timeless = location(1, 52.5, 13.4);
        timeless.tst = None;
        assert!(read(&timeless).is_err(), "no instant, so no fix");

        let mut placeless = location(1, 52.5, 13.4);
        placeless.lat = None;
        assert!(read(&placeless).is_err());
    }

    #[test]
    fn a_coordinate_off_the_planet_is_refused() {
        let report = location(1, 91.0, 13.4);
        assert!(read(&report).is_err());
    }

    #[test]
    fn the_day_a_fix_belongs_to_is_the_local_one() {
        // 2026-09-13 23:30 in Berlin is 21:30Z — the same day there, and the
        // next day in UTC.
        let late_evening = 1_789_335_000;
        assert_eq!(
            local_date(day_start(late_evening, BERLIN), BERLIN),
            "2026-09-13"
        );
        assert_eq!(local_date(day_start(late_evening, 0), 0), "2026-09-13");

        // 00:30 in Berlin is 22:30Z the day before.
        let small_hours = 1_789_338_600;
        assert_eq!(
            local_date(day_start(small_hours, BERLIN), BERLIN),
            "2026-09-14"
        );
        assert_eq!(
            local_date(day_start(small_hours, 0), 0),
            "2026-09-13",
            "which is exactly why the offset is applied"
        );
    }

    #[test]
    fn a_day_is_written_up_once_it_has_ended_and_the_grace_has_passed() {
        let ledger = Ledger::open_in_memory().unwrap();
        let noon = 1_789_300_800; // 2026-09-13 12:00Z
        stage(&ledger, &location(noon, 52.5, 13.4), noon);

        let day = day_start(noon, BERLIN);
        let ends = day + 86_400;

        assert!(days_due(&ledger, ends - 1, BERLIN, GRACE_SECONDS)
            .unwrap()
            .is_empty());
        assert!(
            days_due(&ledger, ends + 60, BERLIN, GRACE_SECONDS)
                .unwrap()
                .is_empty(),
            "still inside the grace period"
        );
        assert_eq!(
            days_due(&ledger, ends + GRACE_SECONDS, BERLIN, GRACE_SECONDS).unwrap(),
            vec![day]
        );
    }

    #[test]
    fn a_days_fixes_become_one_track_with_a_document_behind_it() {
        let ledger = Ledger::open_in_memory().unwrap();
        let noon = 1_789_300_800;
        stage(&ledger, &location(noon, 52.5, 13.4), noon);
        stage(&ledger, &location(noon + 3_600, 52.6, 13.5), noon);

        let day = day_start(noon, BERLIN);
        let rolled = roll_up(&ledger, day, BERLIN, noon + 90_000)
            .unwrap()
            .expect("a day with fixes");

        assert_eq!(rolled.fixes, 2);
        assert_eq!(rolled.added, 2);
        assert_eq!(rolled.name, "OwnTracks 2026-09-13");

        let track = ledger.track(&rolled.track_id).unwrap().unwrap();
        assert_eq!(track.source, PointSource::Recorded);
        assert_eq!(track.source_path, "(reported by pg)");
        assert!(
            ledger.track_text(&rolled.track_id).unwrap().is_some(),
            "every position in the library has a document behind it"
        );
        assert_eq!(ledger.points_between(0, i64::MAX).unwrap().len(), 2);
        assert!(
            ledger.device_fixes_between(0, i64::MAX).unwrap().is_empty(),
            "staged fixes are forgotten once they are a track"
        );
    }

    #[test]
    fn writing_up_the_same_day_twice_is_one_track() {
        let ledger = Ledger::open_in_memory().unwrap();
        let noon = 1_789_300_800;
        stage(&ledger, &location(noon, 52.5, 13.4), noon);
        let day = day_start(noon, BERLIN);

        let first = roll_up(&ledger, day, BERLIN, noon + 90_000)
            .unwrap()
            .unwrap();
        // Nothing staged now, so the second is a no-op rather than a new track.
        assert!(roll_up(&ledger, day, BERLIN, noon + 90_001)
            .unwrap()
            .is_none());

        // And staging the very same fix again reproduces the same id.
        stage(&ledger, &location(noon, 52.5, 13.4), noon);
        let again = roll_up(&ledger, day, BERLIN, noon + 90_002)
            .unwrap()
            .unwrap();
        assert_eq!(again.track_id, first.track_id);
        assert_eq!(again.added, 0, "the library already held it");
        assert_eq!(ledger.tracks().unwrap().len(), 1);
    }

    #[test]
    fn a_fix_that_arrives_late_lands_on_the_day_it_happened() {
        let ledger = Ledger::open_in_memory().unwrap();
        let sunday = 1_789_300_800; // 2026-09-13 12:00Z
        let wednesday = sunday + 3 * 86_400;

        // Reported on Wednesday, taken on Sunday: the phone was in a tunnel.
        stage(&ledger, &location(sunday, 52.5, 13.4), wednesday);
        stage(&ledger, &location(wednesday, 48.85, 2.35), wednesday);

        let rolled = roll_up_due(&ledger, wednesday + 2 * 86_400, BERLIN, GRACE_SECONDS).unwrap();

        assert_eq!(rolled.len(), 2, "two days, in order");
        assert_eq!(rolled[0].name, "OwnTracks 2026-09-13");
        assert_eq!(rolled[1].name, "OwnTracks 2026-09-16");
    }

    #[test]
    fn a_repeated_post_of_one_fix_stores_it_once() {
        let ledger = Ledger::open_in_memory().unwrap();
        let noon = 1_789_300_800;
        let report = location(noon, 52.5, 13.4);

        stage(&ledger, &report, noon);
        stage(&ledger, &report, noon + 5);

        assert_eq!(ledger.device_fixes_between(0, i64::MAX).unwrap().len(), 1);
    }

    #[test]
    fn a_day_with_nothing_staged_produces_no_track() {
        let ledger = Ledger::open_in_memory().unwrap();
        assert!(roll_up(&ledger, 0, BERLIN, 100_000).unwrap().is_none());
        assert_eq!(ledger.tracks().unwrap().len(), 0);
    }
}
