//! Phase 9 acceptance: validation and remediation (F12, F13), on real files.

mod fixtures;

use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime};
use fixtures::Fixtures;
use phototools_core::config::Thresholds;
use phototools_core::ingest::scanner::scan_files;
use phototools_core::ingest::{
    apply_bulk, group_into_shots, plan_bulk, ActionKind, BulkRequest, CheckStatus, FailureClass,
    RemediationParams, Rule, Shot,
};
use phototools_core::jobs::InMemoryProgress;
use phototools_core::media::read_meta;
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

#[test]
fn a_24mp_frame_fails_and_resizing_brings_it_under_10mp_with_its_date_intact() {
    // The acceptance criterion, and the one the specification calls out:
    //
    // > Mandatory: resizing must preserve EXIF. Dropping EXIF at this step
    // > destroys the capture date that was just validated, and Google Photos
    // > would then file the photograph under its upload date.
    let f = Fixtures::new();
    let dir = f.path().join("card");
    std::fs::create_dir(&dir).unwrap();

    let big = f.jpeg_with_exif("big.jpg", 6000, 4000, "2024:05:30 14:22:11", "CANON EOS R6");
    std::fs::rename(&big, dir.join("IMG_0001.JPG")).unwrap();

    let shots = shots_in(&dir);
    // Stated rather than inherited: the default ceiling is now zero — none —
    // and this test is about the rule firing at ten.
    let thresholds = Thresholds {
        max_megapixels: 10,
        ..Thresholds::default()
    };
    let validation = phototools_core::ingest::validate(&shots, now(), &thresholds);

    // It fails the resolution check, as a 24 MP frame must against a 10 MP
    // ceiling.
    let resolution = validation.shots[0]
        .checks
        .iter()
        .find(|c| c.rule == Rule::Resolution)
        .unwrap();
    assert_eq!(resolution.status, CheckStatus::Fail);
    assert_eq!(resolution.failure, Some(FailureClass::TooManyPixels));

    // Resize the whole class — which here is one frame.
    let out = f.path().join("resized");
    let params = RemediationParams {
        shots: &shots,
        validation: &validation,
        thresholds: thresholds.clone(),
        request: BulkRequest {
            failure: FailureClass::TooManyPixels,
            action: ActionKind::Resize,
            date: None,
            output_dir: out.clone(),
        },
    };

    let plan = plan_bulk(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 1);

    let summary = apply_bulk(plan, &progress()).unwrap().data;
    assert_eq!(summary.rewritten.len(), 1, "{:?}", summary.failures);
    assert!(
        summary.exif_preserved,
        "F13 marks preserving EXIF mandatory"
    );

    // The result is under the ceiling.
    let written = &summary.rewritten[0];
    let meta = read_meta(written).unwrap();
    let megapixels = (meta.width as f64 * meta.height as f64) / 1_000_000.0;
    assert!(
        megapixels <= 10.0,
        "{}×{} is {megapixels:.1} MP, still over the ceiling",
        meta.width,
        meta.height
    );

    // And the capture date survived, which is the whole point.
    let capture = meta
        .capture
        .expect("the capture date must survive a resize");
    assert_eq!(capture.year(), 2024);
    assert_eq!(capture.month(), 5);
    assert_eq!(capture.day(), 30);
    assert_eq!(capture.format("%H:%M:%S").to_string(), "14:22:11");

    assert_eq!(
        meta.camera.as_deref(),
        Some("CANON EOS R6"),
        "the camera model survives too"
    );

    // PixelXDimension / PixelYDimension were updated, not left describing the
    // original — the specification names this explicitly.
    let decoded = phototools_core::media::decode(written).unwrap();
    assert_eq!(
        (meta.width, meta.height),
        (decoded.width(), decoded.height()),
        "metadata dimensions must match the pixels actually written"
    );

    // Aspect ratio held.
    let ratio = meta.width as f64 / meta.height as f64;
    assert!((ratio - 1.5).abs() < 0.01, "6000×4000 is 3:2, got {ratio}");
}

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
fn a_card_dated_2019_produces_one_bulk_shift_not_four_hundred_failures() {
    // The acceptance criterion. Real files, so the dates come from real EXIF.
    let f = Fixtures::new();
    let dir = f.path().join("card");
    std::fs::create_dir(&dir).unwrap();

    for i in 0..40 {
        let capture = format!("2019:03:01 {:02}:{:02}:00", 9 + i / 60, i % 60);
        let img = f.jpeg_with_exif(&format!("__{i}.jpg"), 800, 600, &capture, "CANON EOS R6");
        std::fs::rename(&img, dir.join(format!("IMG_{i:04}.JPG"))).unwrap();
    }

    let shots = shots_in(&dir);
    assert_eq!(shots.len(), 40);

    let validation = phototools_core::ingest::validate(&shots, now(), &Thresholds::default());

    let offset = validation
        .clock_offset
        .as_ref()
        .expect("a card entirely in 2019 with minutes of spread is a clock reset");
    assert_eq!(offset.affected, 40);
    assert_eq!(offset.spread_days, 0);

    // One class, one action — not forty decisions.
    let grouped = validation.by_failure();
    assert_eq!(grouped.len(), 1);
    assert_eq!(grouped[&FailureClass::DateOutOfRangeBatch].len(), 40);

    // And the correction it offers moves every frame into the present.
    let params = RemediationParams {
        shots: &shots,
        validation: &validation,
        thresholds: Thresholds::default(),
        request: BulkRequest {
            failure: FailureClass::DateOutOfRangeBatch,
            action: ActionKind::BulkShift,
            date: None,
            output_dir: f.path().join("unused"),
        },
    };
    let plan = plan_bulk(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 40, "one operation covers the card");
    for action in &plan.actions {
        assert!(action.new_date.unwrap().year() >= 2024);
    }
}

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

#[test]
fn bulk_apply_over_fifty_shots_sharing_a_failure_is_one_operation() {
    // The acceptance criterion. The ceiling is lowered rather than the fixtures
    // enlarged, so this stays a test of the bulk path and not of the encoder.
    let f = Fixtures::new();
    let dir = f.path().join("card");
    std::fs::create_dir(&dir).unwrap();

    for i in 0..50 {
        let img = f.jpeg_with_exif(
            &format!("__{i}.jpg"),
            1200,
            900,
            "2024:05:30 12:00:00",
            "CANON EOS R6",
        );
        std::fs::rename(&img, dir.join(format!("IMG_{i:04}.JPG"))).unwrap();
    }

    let thresholds = Thresholds {
        max_megapixels: 1,
        ..Thresholds::default()
    };

    let shots = shots_in(&dir);
    let validation = phototools_core::ingest::validate(&shots, now(), &thresholds);
    assert_eq!(validation.failing(), 50, "1.08 MP is over a 1 MP ceiling");

    let out = f.path().join("resized");
    let params = RemediationParams {
        shots: &shots,
        validation: &validation,
        thresholds: thresholds.clone(),
        request: BulkRequest {
            failure: FailureClass::TooManyPixels,
            action: ActionKind::Resize,
            date: None,
            output_dir: out.clone(),
        },
    };

    let plan = plan_bulk(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 50, "one plan, fifty shots");

    let summary = apply_bulk(plan, &progress()).unwrap().data;
    assert_eq!(summary.rewritten.len(), 50, "{:?}", summary.failures);
    assert!(summary.failures.is_empty());
    assert!(summary.exif_preserved, "all fifty kept their metadata");

    // Every one is genuinely under the ceiling, and every one kept its date.
    for written in &summary.rewritten {
        let meta = read_meta(written).unwrap();
        assert!(
            (meta.width as u64 * meta.height as u64) <= 1_000_000,
            "{}×{} is still over",
            meta.width,
            meta.height
        );
        assert!(
            meta.capture.is_some(),
            "{} lost its date",
            written.display()
        );
    }
}

// ---------------------------------------------------------------------------
// Size, and the quality ladder
// ---------------------------------------------------------------------------

#[test]
fn an_oversized_file_is_re_encoded_down_and_keeps_its_date() {
    let f = Fixtures::new();
    let dir = f.path().join("card");
    std::fs::create_dir(&dir).unwrap();

    let img = f.jpeg_with_exif(
        "__big.jpg",
        2000,
        1500,
        "2024:05:30 08:15:00",
        "CANON EOS R6",
    );
    std::fs::rename(&img, dir.join("IMG_0001.JPG")).unwrap();

    let actual = std::fs::metadata(dir.join("IMG_0001.JPG")).unwrap().len();
    let thresholds = Thresholds {
        // Below what the fixture actually is, so the size rule trips.
        max_output_bytes: actual - 1,
        ..Thresholds::default()
    };

    let shots = shots_in(&dir);
    let validation = phototools_core::ingest::validate(&shots, now(), &thresholds);

    let size = validation.shots[0]
        .checks
        .iter()
        .find(|c| c.rule == Rule::Size)
        .unwrap();
    assert_eq!(size.failure, Some(FailureClass::TooLarge));

    let out = f.path().join("smaller");
    let params = RemediationParams {
        shots: &shots,
        validation: &validation,
        thresholds: thresholds.clone(),
        request: BulkRequest {
            failure: FailureClass::TooLarge,
            action: ActionKind::ReencodeLower,
            date: None,
            output_dir: out,
        },
    };

    let plan = plan_bulk(&params).unwrap().data;
    assert_eq!(
        plan.actions[0].max_bytes,
        Some(thresholds.max_output_bytes),
        "the plan must carry the cap, or the quality ladder has nothing to aim at"
    );

    let summary = apply_bulk(plan, &progress()).unwrap().data;

    assert_eq!(summary.rewritten.len(), 1, "{:?}", summary.failures);
    assert!(summary.exif_preserved);
    assert!(read_meta(&summary.rewritten[0]).unwrap().capture.is_some());

    // F13: "stepping quality down 95 → 88 → 82 → 75 until the byte cap is
    // satisfied". The output must actually be smaller than the cap that failed.
    let written = std::fs::metadata(&summary.rewritten[0]).unwrap().len();
    assert!(
        written <= thresholds.max_output_bytes,
        "re-encoding produced {written} bytes against a {} byte cap",
        thresholds.max_output_bytes
    );
    assert!(
        summary.still_too_large.is_empty(),
        "the ladder met the cap, so nothing should be reported as still too large"
    );
}

#[test]
fn a_file_that_cannot_meet_the_cap_is_reported_rather_than_claimed_fixed() {
    // The bottom of the quality ladder is 75; a cap of a few hundred bytes
    // cannot be met by any real photograph. The file is still written — a
    // smaller file is better than none — but it is reported.
    let f = Fixtures::new();
    let dir = f.path().join("card");
    std::fs::create_dir(&dir).unwrap();

    let img = f.jpeg_with_exif(
        "__big.jpg",
        1600,
        1200,
        "2024:05:30 08:15:00",
        "CANON EOS R6",
    );
    std::fs::rename(&img, dir.join("IMG_0001.JPG")).unwrap();

    let thresholds = Thresholds {
        max_output_bytes: 512,
        ..Thresholds::default()
    };

    let shots = shots_in(&dir);
    let validation = phototools_core::ingest::validate(&shots, now(), &thresholds);

    let params = RemediationParams {
        shots: &shots,
        validation: &validation,
        thresholds: thresholds.clone(),
        request: BulkRequest {
            failure: FailureClass::TooLarge,
            action: ActionKind::ReencodeLower,
            date: None,
            output_dir: f.path().join("smaller"),
        },
    };

    let plan = plan_bulk(&params).unwrap().data;
    let summary = apply_bulk(plan, &progress()).unwrap().data;

    assert_eq!(summary.rewritten.len(), 1);
    assert_eq!(
        summary.still_too_large.len(),
        1,
        "512 bytes is unreachable; that must be said, not glossed over"
    );
}

// ---------------------------------------------------------------------------
// The dry-run guarantee
// ---------------------------------------------------------------------------

#[test]
fn planning_a_bulk_remediation_leaves_the_card_untouched() {
    // Build plan §7: plan never touches disk. Asserted by hashing the tree.
    let f = Fixtures::new();
    let dir = f.path().join("card");
    std::fs::create_dir(&dir).unwrap();

    for i in 0..5 {
        let img = f.jpeg_with_exif(
            &format!("__{i}.jpg"),
            2000,
            1500,
            "2024:05:30 12:00:00",
            "CANON EOS R6",
        );
        std::fs::rename(&img, dir.join(format!("IMG_{i:04}.JPG"))).unwrap();
    }

    let before = snapshot(&dir);

    let shots = shots_in(&dir);
    let validation = phototools_core::ingest::validate(&shots, now(), &Thresholds::default());
    let out = f.path().join("out");

    for action in [
        ActionKind::Resize,
        ActionKind::PublishAnyway,
        ActionKind::Skip,
    ] {
        let params = RemediationParams {
            shots: &shots,
            validation: &validation,
            thresholds: Thresholds::default(),
            request: BulkRequest {
                failure: FailureClass::TooManyPixels,
                action,
                date: None,
                output_dir: out.clone(),
            },
        };
        plan_bulk(&params).unwrap();
    }

    assert_eq!(snapshot(&dir), before, "plan must not modify the card");
    assert!(!out.exists(), "plan must not create its output directory");
}

fn snapshot(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for entry in walkdir::WalkDir::new(root).sort_by_file_name() {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            out.push((
                entry.file_name().to_string_lossy().to_string(),
                std::fs::read(entry.path()).unwrap(),
            ));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Undated frames
// ---------------------------------------------------------------------------

#[test]
fn an_undated_frame_can_take_the_batch_median() {
    let f = Fixtures::new();
    let dir = f.path().join("card");
    std::fs::create_dir(&dir).unwrap();

    for (i, day) in [29u32, 30, 31].iter().enumerate() {
        let img = f.jpeg_with_exif(
            &format!("__{i}.jpg"),
            800,
            600,
            &format!("2024:05:{day} 12:00:00"),
            "CANON EOS R6",
        );
        std::fs::rename(&img, dir.join(format!("IMG_{i:04}.JPG"))).unwrap();
    }
    let undated = f.jpeg_without_exif("__undated.jpg", 800, 600);
    std::fs::rename(&undated, dir.join("IMG_9999.JPG")).unwrap();

    let shots = shots_in(&dir);
    let validation = phototools_core::ingest::validate(&shots, now(), &Thresholds::default());

    let grouped = validation.by_failure();
    assert_eq!(grouped[&FailureClass::NoDate].len(), 1);

    let params = RemediationParams {
        shots: &shots,
        validation: &validation,
        thresholds: Thresholds::default(),
        request: BulkRequest {
            failure: FailureClass::NoDate,
            action: ActionKind::DeriveFromBatchMedian,
            date: None,
            output_dir: f.path().join("unused"),
        },
    };

    let plan = plan_bulk(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 1);

    let derived = plan.actions[0].new_date.expect("the median");
    assert_eq!(derived.day(), 30, "the middle of 29, 30, 31");
}

#[test]
fn an_undated_frame_can_fall_back_to_its_modification_time() {
    let f = Fixtures::new();
    let dir = f.path().join("card");
    std::fs::create_dir(&dir).unwrap();

    let undated = f.jpeg_without_exif("__undated.jpg", 800, 600);
    std::fs::rename(&undated, dir.join("IMG_0001.JPG")).unwrap();

    let shots = shots_in(&dir);
    let validation = phototools_core::ingest::validate(&shots, now(), &Thresholds::default());

    let params = RemediationParams {
        shots: &shots,
        validation: &validation,
        thresholds: Thresholds::default(),
        request: BulkRequest {
            failure: FailureClass::NoDate,
            action: ActionKind::UseFileModificationTime,
            date: None,
            output_dir: f.path().join("unused"),
        },
    };

    let plan = plan_bulk(&params).unwrap().data;
    let date = plan.actions[0].new_date.expect("a modification time");

    // The fixture was written moments ago, so the date is close to the present.
    let age = (chrono::Utc::now().naive_utc() - date).num_seconds().abs();
    assert!(age < 3600, "the file was just written, got {date}");
    assert!(date > now() - Duration::days(365 * 10));
}
