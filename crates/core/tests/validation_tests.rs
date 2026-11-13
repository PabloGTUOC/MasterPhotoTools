//! Phase 9 acceptance: validation and remediation (F12, F13), on real files.

mod fixtures;

use chrono::{NaiveDate, NaiveDateTime};
use fixtures::Fixtures;
use phototools_core::config::Thresholds;
use phototools_core::ingest::scanner::scan_files;
use phototools_core::ingest::{group_into_shots, CheckStatus, Rule, Shot};
use phototools_core::jobs::InMemoryProgress;
use std::path::Path;

fn now() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2024, 6, 1)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
}

fn progress() -> InMemoryProgress {
    InMemoryProgress::new()
}

/// Scan a directory and pair it into shots, the way the real pipeline does.
fn shots_in(dir: &Path) -> Vec<Shot> {
    group_into_shots(scan_files(dir, &progress()).unwrap().assets)
}

// ---------------------------------------------------------------------------
// The mandatory round trip: resize preserves EXIF
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// The megapixel boundary, on real files
// ---------------------------------------------------------------------------

#[test]
fn ten_megapixels_exactly_passes_and_ten_point_one_fails() {
    // 4000×2500 is 10,000,000 pixels exactly; 4040×2500 is 10,100,000.
    let f = Fixtures::new();
    // The boundary this test is about is ten megapixels, so it sets ten. The
    // default is now zero — no ceiling — which would make every case pass and
    // the test meaningless.
    let thresholds = Thresholds {
        max_megapixels: 10,
        ..Thresholds::default()
    };

    for (name, w, h, expected) in [
        ("exact", 4000u32, 2500u32, CheckStatus::Pass),
        ("over", 4040, 2500, CheckStatus::Fail),
    ] {
        let dir = f.path().join(name);
        std::fs::create_dir(&dir).unwrap();
        let img = f.jpeg_with_exif(
            &format!("__{name}.jpg"),
            w,
            h,
            "2024:05:30 12:00:00",
            "CANON EOS R6",
        );
        std::fs::rename(&img, dir.join("IMG_0001.JPG")).unwrap();

        let shots = shots_in(&dir);
        let validation = phototools_core::ingest::validate(&shots, now(), &thresholds);
        let check = validation.shots[0]
            .checks
            .iter()
            .find(|c| c.rule == Rule::Resolution)
            .unwrap();

        assert_eq!(
            check.status, expected,
            "{w}×{h} should be {expected:?}: {}",
            check.detail
        );
    }
}

// ---------------------------------------------------------------------------
// One bulk suggestion, not four hundred failures
// ---------------------------------------------------------------------------

#[test]
fn a_stray_frame_from_an_earlier_shoot_warns_rather_than_failing_the_card() {
    // F12: "Frames left over from an earlier shoot on the same card are
    // legitimate." The card is recent; one frame is 60 days old, inside the
    // 90-day limit but far from the median.
    let f = Fixtures::new();
    let dir = f.path().join("card");
    std::fs::create_dir(&dir).unwrap();

    for i in 0..5 {
        let img = f.jpeg_with_exif(
            &format!("__recent{i}.jpg"),
            800,
            600,
            "2024:05:30 12:00:00",
            "CANON EOS R6",
        );
        std::fs::rename(&img, dir.join(format!("IMG_{i:04}.JPG"))).unwrap();
    }
    let stray = f.jpeg_with_exif(
        "__stray.jpg",
        800,
        600,
        "2024:04:01 12:00:00",
        "CANON EOS R6",
    );
    std::fs::rename(&stray, dir.join("IMG_9999.JPG")).unwrap();

    let shots = shots_in(&dir);
    let validation = phototools_core::ingest::validate(&shots, now(), &Thresholds::default());

    assert!(validation.clock_offset.is_none(), "the card is not old");
    assert_eq!(validation.failing(), 0, "a stray frame is not a failure");

    let stray = validation
        .shots
        .iter()
        .find(|s| s.stem == "IMG_9999")
        .unwrap();
    assert_eq!(stray.status(), CheckStatus::Warn);
}

// ---------------------------------------------------------------------------
// Bulk apply over fifty shots
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Size, and the quality ladder
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// The dry-run guarantee
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Undated frames
// ---------------------------------------------------------------------------
