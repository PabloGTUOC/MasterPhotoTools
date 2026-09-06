//! Geotagging, end to end: a real GPX file, real photographs, a real database
//! and a real `exiftool`.
//!
//! The unit tests prove the arithmetic. These prove the parts join up — which
//! is where this application has historically found its defects: tools that
//! passed every test they had and did nothing when pointed at a folder.

mod fixtures;

use fixtures::Fixtures;
use phototools_core::jobs::InMemoryProgress;
use phototools_core::ledger::Ledger;
use phototools_core::media::read_meta;
use phototools_core::tools::geotag::join::{Limits, Method, Mode};
use phototools_core::tools::geotag::library::{self, Resolution};
use phototools_core::tools::geotag::tool::{GeotagParams, GeotagTool};
use phototools_core::tools::Tool;
use std::path::{Path, PathBuf};

/// The specimen export, written where a test can point at it.
fn track_file(dir: &Path) -> PathBuf {
    let path = dir.join("track.gpx");
    std::fs::write(
        &path,
        include_str!("../src/tools/geotag/testdata/owntracks-sample.gpx"),
    )
    .unwrap();
    path
}

/// A database holding the specimen track.
fn library_with_track(dir: &Path) -> (Ledger, PathBuf) {
    let database = dir.join("ledger.sqlite3");
    let ledger = Ledger::open(&database).unwrap();
    let file = library::read_track(&track_file(dir)).unwrap();
    let result =
        library::commit_import(&ledger, &file, Resolution::KeepExisting, &[], 1_788_600_000)
            .unwrap();
    assert_eq!(result.added, 50, "the specimen holds fifty fixes");
    (ledger, database)
}

fn params(paths: Vec<PathBuf>, database: PathBuf) -> GeotagParams {
    GeotagParams {
        paths,
        recursive: false,
        database,
        // Berlin in September, which is what the specimen track was recorded in.
        utc_offset_minutes: Some(120),
        clock_correction_seconds: 0,
        limits: Limits::default(),
        overwrite_existing: false,
        write_altitude: true,
    }
}

#[test]
fn a_folder_of_photographs_is_located_from_a_track_and_reads_back_located() {
    let f = Fixtures::new();
    let (_ledger, database) = library_with_track(f.path());

    // Two frames from the afternoon of 4 September, on the camera's local
    // clock: 15:12:40 UTC and 17:26:03 UTC.
    let a = f.jpeg_with_exif("a.jpg", 40, 40, "2026:09:04 15:12:40", "CAM");
    let b = f.jpeg_with_exif("b.jpg", 40, 40, "2026:09:04 17:26:03", "CAM");

    let plan = GeotagTool
        .plan(&params(vec![a.clone(), b.clone()], database))
        .unwrap()
        .data;

    assert_eq!(plan.actions.len(), 2, "skipped: {:?}", plan.skipped);
    for action in &plan.actions {
        assert!(
            (52.5..52.55).contains(&action.point.lat),
            "{} landed at {}",
            action.name,
            action.point.lat
        );
        assert!(action.gap_seconds < 300);
    }

    let summary = GeotagTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;

    assert_eq!(summary.written, 2, "failures: {:?}", summary.failures);
    assert!(summary.unverified.is_empty());

    for path in [&a, &b] {
        let fix = read_meta(path)
            .unwrap()
            .gps
            .unwrap_or_else(|| panic!("{} should carry a position", path.display()));
        assert!((52.5..52.55).contains(&fix.lat));
        assert!((13.3..13.5).contains(&fix.lon));
    }
}

#[test]
fn a_plan_writes_nothing() {
    let f = Fixtures::new();
    let (_ledger, database) = library_with_track(f.path());
    let path = f.jpeg_with_exif("a.jpg", 40, 40, "2026:09:04 15:12:40", "CAM");

    let before = std::fs::metadata(&path).unwrap().modified().unwrap();
    let plan = GeotagTool
        .plan(&params(vec![path.clone()], database))
        .unwrap()
        .data;

    assert_eq!(plan.actions.len(), 1);
    assert_eq!(
        std::fs::metadata(&path).unwrap().modified().unwrap(),
        before,
        "a dry run must not touch the file it is describing"
    );
    assert_eq!(read_meta(&path).unwrap().gps, None);
}

#[test]
fn a_photograph_taken_overnight_is_refused_while_a_ceiling_stands() {
    // The specimen has no fixes between 20:27 on the 2nd and 06:47 on the 3rd.
    // 02:00 local is 00:00 UTC, three and a half hours after the last one.
    let f = Fixtures::new();
    let (_ledger, database) = library_with_track(f.path());
    let path = f.jpeg_with_exif("night.jpg", 40, 40, "2026:09:03 02:00:00", "CAM");

    // A ceiling of half an hour, set deliberately: the default is twelve, and
    // twelve accepts this night quite correctly.
    let mut short_ceiling = params(vec![path.clone()], database.clone());
    short_ceiling.limits.max_edge_seconds = 30 * 60;

    let plan = GeotagTool.plan(&short_ceiling).unwrap().data;

    assert_eq!(plan.actions, vec![]);
    assert_eq!(plan.skipped.len(), 1);
    assert!(
        plan.skipped[0].reason.contains("recorded nothing for"),
        "got {:?}",
        plan.skipped[0].reason
    );
    assert_eq!(read_meta(&path).unwrap().gps, None);
}

#[test]
fn an_overnight_photograph_takes_where_the_phone_last_was() {
    // The phone was not moving; it was at home. The last fix of the evening is
    // where its owner was all night, and that is the answer — reported with the
    // age of the fix, not disguised as a fresh one.
    let f = Fixtures::new();
    let (_ledger, database) = library_with_track(f.path());
    let path = f.jpeg_with_exif("night.jpg", 40, 40, "2026:09:03 02:00:00", "CAM");

    // The defaults, unchanged: twelve hours covers a night at home.
    let plan = GeotagTool
        .plan(&params(vec![path.clone()], database))
        .unwrap()
        .data;
    assert_eq!(plan.actions.len(), 1, "skipped: {:?}", plan.skipped);

    let action = &plan.actions[0];
    assert_eq!(action.method, Method::CarriedForward);
    // 52.531469 / 13.369442 is the 20:27:33 fix, verbatim from the file.
    assert_eq!(action.point.lat, 52.531469);
    assert_eq!(action.point.lon, 13.369442);
    assert!(
        (12000..13000).contains(&action.gap_seconds),
        "about three and a half hours old, got {}",
        action.gap_seconds
    );
}

#[test]
fn a_photograph_that_already_knows_where_it_was_is_left_alone() {
    let f = Fixtures::new();
    let (_ledger, database) = library_with_track(f.path());
    let path = f.jpeg_with_exif("a.jpg", 40, 40, "2026:09:04 15:12:40", "CAM");

    // Give it a position of its own, a long way from the track.
    let mut writer = phototools_core::media::ExifWriter::start().unwrap();
    writer
        .set_tags(
            &path,
            &[
                "-GPSLatitude=48.858400".to_string(),
                "-GPSLatitudeRef=N".to_string(),
                "-GPSLongitude=2.294500".to_string(),
                "-GPSLongitudeRef=E".to_string(),
            ],
        )
        .unwrap();
    writer.close().unwrap();

    let plan = GeotagTool
        .plan(&params(vec![path.clone()], database.clone()))
        .unwrap()
        .data;
    assert_eq!(plan.actions, vec![]);
    assert!(plan.skipped[0]
        .reason
        .contains("already carries a position"));

    // Still in Paris, not moved to Berlin.
    let fix = read_meta(&path).unwrap().gps.unwrap();
    assert!((fix.lat - 48.8584).abs() < 1e-5);

    // And asking for it explicitly does move it.
    let mut overwriting = params(vec![path.clone()], database);
    overwriting.overwrite_existing = true;
    let plan = GeotagTool.plan(&overwriting).unwrap().data;
    assert_eq!(plan.actions.len(), 1);
    assert!(
        plan.actions[0].replaces.is_some(),
        "the plan should show what it is writing over"
    );

    let summary = GeotagTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;
    assert_eq!(summary.written, 1);
    let fix = read_meta(&path).unwrap().gps.unwrap();
    assert!((fix.lat - 52.5).abs() < 0.1, "moved to {}", fix.lat);
}

#[test]
fn a_photograph_with_no_offset_and_none_set_is_refused_rather_than_read_as_utc() {
    let f = Fixtures::new();
    let (_ledger, database) = library_with_track(f.path());
    let path = f.jpeg_with_exif("a.jpg", 40, 40, "2026:09:04 15:12:40", "CAM");

    let mut none_set = params(vec![path], database);
    none_set.utc_offset_minutes = None;

    let plan = GeotagTool.plan(&none_set).unwrap().data;
    assert_eq!(plan.actions, vec![]);
    assert!(
        plan.skipped[0].reason.contains("no UTC offset"),
        "got {:?}",
        plan.skipped[0].reason
    );
}

#[test]
fn a_video_and_an_undated_frame_are_listed_as_declined_not_omitted() {
    let f = Fixtures::new();
    let (_ledger, database) = library_with_track(f.path());
    let good = f.jpeg_with_exif("a.jpg", 40, 40, "2026:09:04 15:12:40", "CAM");
    f.jpeg_without_exif("undated.jpg", 40, 40);
    f.quicktime("clip.mov", 1_788_536_017);

    let plan = GeotagTool
        .plan(&params(
            vec![good.parent().unwrap().to_path_buf()],
            database,
        ))
        .unwrap()
        .data;

    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.skipped.len(), 2);
    assert!(plan
        .skipped
        .iter()
        .any(|s| s.reason.contains("no capture date")));
    assert!(plan.skipped.iter().any(|s| s.reason.contains("video")));
}

#[test]
fn nearest_mode_takes_the_closer_fix_from_either_side() {
    let f = Fixtures::new();
    let (_ledger, database) = library_with_track(f.path());
    let path = f.jpeg_with_exif("a.jpg", 40, 40, "2026:09:04 15:12:40", "CAM");

    let mut nearest = params(vec![path], database);
    nearest.limits.mode = Mode::Nearest;

    let plan = GeotagTool.plan(&nearest).unwrap().data;
    assert_eq!(plan.actions[0].method, Method::Nearest);

    // 52.528909 / 13.378771 is a fix in the file, verbatim.
    let point = plan.actions[0].point;
    assert_eq!(point.lat, 52.528909);
    assert_eq!(point.lon, 13.378771);
}

#[test]
fn the_offset_can_be_read_off_the_photographs_themselves() {
    use phototools_core::tools::geotag::join::estimate_offset;

    let f = Fixtures::new();
    let (ledger, _database) = library_with_track(f.path());

    // Seven frames from the afternoon of the 4th and the morning of the 5th,
    // on a camera set to Berlin time.
    let captures: Vec<_> = [
        "2026:09:04 15:12:40",
        "2026:09:04 15:28:55",
        "2026:09:04 17:15:20",
        "2026:09:04 17:26:03",
        "2026:09:05 12:21:47",
        "2026:09:05 12:52:10",
        "2026:09:05 13:06:30",
    ]
    .iter()
    .map(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S").unwrap())
    .collect();

    let points = ledger.points_between(0, i64::MAX).unwrap();
    let suggestion = estimate_offset(&points, &captures).unwrap();

    assert_eq!(suggestion.minutes, 120, "{suggestion:?}");
    assert!(suggestion.confident, "{suggestion:?}");
}
