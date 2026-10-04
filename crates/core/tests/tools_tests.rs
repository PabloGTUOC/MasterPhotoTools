//! Phase 3 acceptance tests — F1, F2, F3, F9.

mod fixtures;

use chrono::NaiveDateTime;
use fixtures::{tag, Fixtures, TakeoutVariant, TiffValue};
use phototools_core::jobs::InMemoryProgress;
use phototools_core::media::read_meta;
use phototools_core::tools::f1_dates::{
    self, DateRepairParams, DateRepairTool, DateStatus, FsTimeSource, RepairMode, ShiftDelta,
};
use phototools_core::tools::f2_takeout;
use phototools_core::tools::f3_rename::{BatchRenameParams, BatchRenamerTool, RenameOrder};
use phototools_core::tools::Tool;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;

fn dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S").unwrap()
}

/// A hash over every file's name and bytes in a directory tree.
///
/// Used to prove a `plan` changed nothing on disk.
fn hash_tree(root: &Path) -> String {
    let mut entries: Vec<_> = walk(root);
    entries.sort();

    let mut hasher = Sha256::new();
    for path in entries {
        hasher.update(
            path.strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .as_bytes(),
        );
        if let Ok(bytes) = fs::read(&path) {
            hasher.update(&bytes);
        }
    }
    hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn walk(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// F1 — scan
// ---------------------------------------------------------------------------

#[test]
fn f1_scan_classifies_missing_metadata() {
    let f = Fixtures::new();
    let dir = f.path().join("scan_missing");
    fs::create_dir(&dir).unwrap();
    let bare = f.jpeg_without_exif("bare.jpg", 40, 40);
    fs::rename(&bare, dir.join("bare.jpg")).unwrap();

    let results = f1_dates::scan_dates(&dir, false).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, DateStatus::MissingMetadata);
    assert_eq!(results[0].metadata_date, None);
}

#[test]
fn f1_scan_classifies_a_stale_metadata_date_as_a_mismatch() {
    let f = Fixtures::new();
    let dir = f.path().join("scan_mismatch");
    fs::create_dir(&dir).unwrap();
    // Written now, but claiming 2019 — the filesystem and metadata disagree.
    let old = f.jpeg_with_exif("old.jpg", 40, 40, "2019:01:01 00:00:00", "CAM");
    fs::rename(&old, dir.join("old.jpg")).unwrap();

    let results = f1_dates::scan_dates(&dir, false).unwrap();
    assert_eq!(results[0].status, DateStatus::Mismatch);
    assert_eq!(results[0].metadata_date, Some(dt("2019:01:01 00:00:00")));
    assert_eq!(results[0].tag.as_deref(), Some("EXIF:DateTimeOriginal"));
}

#[test]
fn f1_scan_reports_which_filesystem_timestamp_it_used() {
    let f = Fixtures::new();
    let dir = f.path().join("scan_fs");
    fs::create_dir(&dir).unwrap();
    let p = f.jpeg_without_exif("a.jpg", 20, 20);
    fs::rename(&p, dir.join("a.jpg")).unwrap();

    let results = f1_dates::scan_dates(&dir, false).unwrap();
    let source = results[0].fs_date_source.unwrap();

    // Never claim a birth time on a platform that has none to set.
    if f1_dates::birth_time_is_settable() {
        assert!(matches!(
            source,
            FsTimeSource::Created | FsTimeSource::Modified
        ));
    } else {
        assert_eq!(
            source,
            FsTimeSource::Modified,
            "Linux has no settable birth time, so the scan must say it used mtime"
        );
    }
}

#[test]
fn f1_scan_covers_every_extension_group_and_ignores_others() {
    let f = Fixtures::new();
    let dir = f.path().join("scan_ext");
    fs::create_dir(&dir).unwrap();

    for name in ["a.jpg", "b.TIFF", "c.cr2", "d.MOV", "e.heic"] {
        fs::write(dir.join(name), "x").unwrap();
    }
    fs::write(dir.join("notes.txt"), "x").unwrap();
    fs::write(dir.join("archive.zip"), "x").unwrap();

    let results = f1_dates::scan_dates(&dir, false).unwrap();
    assert_eq!(results.len(), 5, "only media files are reported");
}

#[test]
fn f1_scan_recurses_only_when_asked() {
    let f = Fixtures::new();
    let dir = f.path().join("scan_rec");
    let sub = dir.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(dir.join("top.jpg"), "x").unwrap();
    fs::write(sub.join("deep.jpg"), "x").unwrap();

    assert_eq!(f1_dates::scan_dates(&dir, false).unwrap().len(), 1);
    assert_eq!(f1_dates::scan_dates(&dir, true).unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// F1 — repair
// ---------------------------------------------------------------------------

#[test]
fn f1_manual_mode_forces_a_supplied_date_and_verifies_it() {
    let f = Fixtures::new();
    let path = f.jpeg_without_exif("manual.jpg", 40, 40);
    let wanted = dt("2022:07:08 09:10:11");

    let plan = DateRepairTool
        .plan(&DateRepairParams {
            paths: vec![path.clone()],
            mode: RepairMode::Manual(wanted),
            recursive: false,
        })
        .unwrap()
        .data;
    assert_eq!(plan.actions.len(), 1);

    let summary = DateRepairTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;

    assert!(summary.failures.is_empty());
    assert_eq!(summary.outcomes.len(), 1);
    assert!(summary.outcomes[0].metadata_verified);
    assert_eq!(read_meta(&path).unwrap().capture, Some(wanted));
}

#[test]
fn f1_auto_mode_copies_the_best_metadata_date_to_the_filesystem() {
    let f = Fixtures::new();
    let path = f.jpeg_with_exif("auto.jpg", 40, 40, "2023:04:05 06:07:08", "CAM");

    let plan = DateRepairTool
        .plan(&DateRepairParams {
            paths: vec![path.clone()],
            mode: RepairMode::Auto,
            recursive: false,
        })
        .unwrap()
        .data;
    assert_eq!(plan.actions[0].new_date, dt("2023:04:05 06:07:08"));

    let summary = DateRepairTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;
    assert!(summary.outcomes[0].metadata_verified);
    assert!(summary.outcomes[0].filesystem_verified);
}

/// **Phase 3 acceptance.** A fixture dated 2019 shifted by `+5:0:0 0:0:0` reads
/// back as 2024.
#[test]
fn f1_shift_moves_a_2019_fixture_to_2024() {
    let f = Fixtures::new();
    let path = f.jpeg_with_exif("shift.jpg", 40, 40, "2019:01:02 03:04:05", "CAM");

    let plan = DateRepairTool
        .plan(&DateRepairParams {
            paths: vec![path.clone()],
            mode: RepairMode::Shift("+5:0:0 0:0:0".into()),
            recursive: false,
        })
        .unwrap()
        .data;

    // The plan states the result before anything is written.
    assert_eq!(plan.actions[0].new_date, dt("2024:01:02 03:04:05"));

    let summary = DateRepairTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;

    assert!(summary.failures.is_empty());
    assert!(summary.outcomes[0].metadata_verified);
    assert_eq!(
        read_meta(&path).unwrap().capture,
        Some(dt("2024:01:02 03:04:05"))
    );
}

#[test]
fn f1_sidecar_mode_takes_its_date_from_a_takeout_json() {
    let f = Fixtures::new();
    // 2024-01-01 00:00:00 UTC.
    let media = f.takeout_pair("photo.jpg", 1_704_067_200, TakeoutVariant::Exact);

    let plan = DateRepairTool
        .plan(&DateRepairParams {
            paths: vec![media.clone()],
            mode: RepairMode::Sidecar,
            recursive: false,
        })
        .unwrap()
        .data;
    assert_eq!(plan.actions[0].new_date, dt("2024:01:01 00:00:00"));

    let summary = DateRepairTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;
    assert!(summary.outcomes[0].metadata_verified);
}

#[test]
fn f1_reports_files_it_could_not_resolve_rather_than_guessing() {
    let f = Fixtures::new();
    let bare = f.jpeg_without_exif("nodate.jpg", 20, 20);
    let missing = f.path().join("does-not-exist.jpg");

    let plan = DateRepairTool
        .plan(&DateRepairParams {
            paths: vec![bare, missing],
            mode: RepairMode::Auto,
            recursive: false,
        })
        .unwrap()
        .data;

    assert!(plan.actions.is_empty());
    assert_eq!(plan.skipped.len(), 2);
    assert!(plan.skipped.iter().any(|s| s.reason.contains("not found")));
    assert!(plan
        .skipped
        .iter()
        .any(|s| s.reason.contains("No metadata date")));
}

#[test]
fn f1_a_malformed_shift_delta_fails_the_plan_outright() {
    let f = Fixtures::new();
    let path = f.jpeg_with_exif("x.jpg", 20, 20, "2019:01:01 00:00:00", "CAM");

    let result = DateRepairTool.plan(&DateRepairParams {
        paths: vec![path],
        mode: RepairMode::Shift("next tuesday".into()),
        recursive: false,
    });
    assert!(
        result.is_err(),
        "a bad delta must fail the plan, not silently skip every file"
    );
}

#[test]
fn f1_reports_that_a_platform_without_a_settable_birth_time_only_moved_mtime() {
    let f = Fixtures::new();
    let path = f.jpeg_without_exif("note.jpg", 20, 20);

    let plan = DateRepairTool
        .plan(&DateRepairParams {
            paths: vec![path],
            mode: RepairMode::Manual(dt("2022:01:01 00:00:00")),
            recursive: false,
        })
        .unwrap()
        .data;
    let summary = DateRepairTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;

    if !f1_dates::birth_time_is_settable() {
        let note = summary.outcomes[0].note.as_deref().unwrap_or_default();
        assert!(
            note.contains("no settable creation time"),
            "§9.2 invariant 6: say what was not done. Got: {note:?}"
        );
    }
}

// ---------------------------------------------------------------------------
// F2 — Takeout sidecars
// ---------------------------------------------------------------------------

/// **Phase 3 acceptance.** All sidecar-naming variants resolve.
#[test]
fn f2_every_takeout_naming_variant_resolves() {
    let long_name = format!("{}.jpg", "a".repeat(60));

    let cases: Vec<(&str, String, TakeoutVariant)> = vec![
        ("exact", "photo.jpg".into(), TakeoutVariant::Exact),
        (
            "suffix on the sidecar",
            "dup(1).jpg".into(),
            TakeoutVariant::SuffixOnSidecar,
        ),
        (
            "suffix only on the media file",
            "only(1).jpg".into(),
            TakeoutVariant::SuffixOnMediaOnly,
        ),
        (
            "truncated long name",
            long_name,
            TakeoutVariant::Truncated { to: 46 },
        ),
    ];

    for (label, name, variant) in cases {
        let f = Fixtures::new();
        let media = f.takeout_pair(&name, 1_704_067_200, variant);
        let found = f2_takeout::sidecar_date(&media);
        assert_eq!(
            found,
            Some(dt("2024:01:01 00:00:00")),
            "variant {label:?} should resolve"
        );
    }
}

/// **Phase 3 acceptance.** A missing sidecar is reported, not fatal.
#[test]
fn f2_a_missing_sidecar_is_reported_not_fatal() {
    let f = Fixtures::new();
    let dir = f.path().join("takeout");
    fs::create_dir(&dir).unwrap();

    let with = f.takeout_pair("has.jpg", 1_704_067_200, TakeoutVariant::Exact);
    fs::rename(&with, dir.join("has.jpg")).unwrap();
    fs::rename(f.path().join("has.jpg.json"), dir.join("has.jpg.json")).unwrap();

    let without = f.jpeg_without_exif("lonely.jpg", 20, 20);
    fs::rename(&without, dir.join("lonely.jpg")).unwrap();

    let matches = f2_takeout::scan_sidecars(&dir, false).unwrap();
    assert_eq!(matches.len(), 2, "both files are reported");

    let lonely = matches
        .iter()
        .find(|m| m.media.ends_with("lonely.jpg"))
        .unwrap();
    assert!(lonely.sidecar.is_none());
    assert!(!lonely.is_resolved());

    let has = matches
        .iter()
        .find(|m| m.media.ends_with("has.jpg"))
        .unwrap();
    assert!(has.is_resolved());
}

#[test]
fn f2_scans_recursively_when_asked() {
    let f = Fixtures::new();
    let dir = f.path().join("tk");
    let sub = dir.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(dir.join("a.jpg"), "x").unwrap();
    fs::write(sub.join("b.jpg"), "x").unwrap();

    assert_eq!(f2_takeout::scan_sidecars(&dir, false).unwrap().len(), 1);
    assert_eq!(f2_takeout::scan_sidecars(&dir, true).unwrap().len(), 2);
}

// ---------------------------------------------------------------------------
// F3 — batch rename
// ---------------------------------------------------------------------------

/// **Phase 3 acceptance.** No file is ever overwritten.
#[test]
fn f3_a_collision_is_skipped_and_the_existing_file_is_untouched() {
    let f = Fixtures::new();
    let dir = f.path().join("rename");
    fs::create_dir(&dir).unwrap();

    for name in ["a.jpg", "b.jpg", "c.jpg"] {
        fs::write(dir.join(name), name).unwrap();
    }
    // Occupy the name the first renamed file would take.
    let occupied = dir.join("20240101-Trip-01.jpg");
    fs::write(&occupied, "PRECIOUS").unwrap();

    let plan = BatchRenamerTool
        .plan(&BatchRenameParams {
            paths: vec![dir.join("a.jpg"), dir.join("b.jpg"), dir.join("c.jpg")],
            date: Some("20240101".into()),
            subject: Some("Trip".into()),
            camera: None,
            film: None,
            order: RenameOrder::Numeric,
        })
        .unwrap()
        .data;

    assert_eq!(plan.skipped.len(), 1);
    assert!(plan.skipped[0].reason.contains("Would overwrite"));

    BatchRenamerTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap();

    assert_eq!(
        fs::read_to_string(&occupied).unwrap(),
        "PRECIOUS",
        "the existing file must survive untouched"
    );
    assert!(dir.join("20240101-Trip-02.jpg").exists());
    assert!(dir.join("20240101-Trip-03.jpg").exists());
}

/// A file listed twice must be renamed once. Giving it two sequence numbers
/// would leave the second rename with its source already moved away.
#[test]
fn f3_a_file_listed_twice_is_renamed_once() {
    let f = Fixtures::new();
    let dir = f.path().join("dup");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("x.jpg"), "x").unwrap();

    let plan = BatchRenamerTool
        .plan(&BatchRenameParams {
            paths: vec![dir.join("x.jpg"), dir.join("./x.jpg"), dir.join("x.jpg")],
            date: Some("202401".into()),
            subject: None,
            camera: None,
            film: None,
            order: RenameOrder::Numeric,
        })
        .unwrap()
        .data;

    assert_eq!(plan.actions.len(), 1, "one file, one rename");
    assert_eq!(plan.skipped.len(), 2);
    assert!(plan
        .skipped
        .iter()
        .all(|s| s.reason.contains("more than once")));

    let summary = BatchRenamerTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;
    assert_eq!(summary.renamed.len(), 1);
    assert!(summary.failures.is_empty(), "no orphaned second rename");
    assert!(dir.join("202401-01.jpg").exists());
}

#[test]
fn f3_numbers_are_zero_padded_to_at_least_two_digits() {
    let f = Fixtures::new();
    let dir = f.path().join("pad");
    fs::create_dir(&dir).unwrap();
    let paths: Vec<_> = (1..=3)
        .map(|i| {
            let p = dir.join(format!("img{i}.jpg"));
            fs::write(&p, "x").unwrap();
            p
        })
        .collect();

    let plan = BatchRenamerTool
        .plan(&BatchRenameParams {
            paths,
            date: None,
            subject: Some("Roll".into()),
            camera: None,
            film: None,
            order: RenameOrder::Numeric,
        })
        .unwrap()
        .data;

    let names: Vec<_> = plan
        .actions
        .iter()
        .map(|a| a.target.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, ["Roll-01.jpg", "Roll-02.jpg", "Roll-03.jpg"]);
}

#[test]
fn f3_capture_ordering_uses_metadata_not_just_file_times() {
    let f = Fixtures::new();
    let dir = f.path().join("cap");
    fs::create_dir(&dir).unwrap();

    // Created in one order, shot in the opposite order.
    for (name, capture) in [
        ("first_written.jpg", "2024:12:31 23:59:59"),
        ("second_written.jpg", "2024:01:01 00:00:00"),
    ] {
        let p = f.jpeg_with_exif(name, 32, 32, capture, "CAM");
        fs::rename(&p, dir.join(name)).unwrap();
    }

    let plan = BatchRenamerTool
        .plan(&BatchRenameParams {
            paths: vec![
                dir.join("first_written.jpg"),
                dir.join("second_written.jpg"),
            ],
            date: None,
            subject: Some("Roll".into()),
            camera: None,
            film: None,
            order: RenameOrder::Capture,
        })
        .unwrap()
        .data;

    // The January frame must be numbered 01 even though it was written second.
    let first = plan
        .actions
        .iter()
        .find(|a| a.target.file_name().unwrap() == "Roll-01.jpg")
        .unwrap();
    assert!(
        first.source.ends_with("second_written.jpg"),
        "capture order must come from metadata, not the filesystem"
    );
}

#[test]
fn f3_extensions_are_lowercased_and_preserved() {
    let f = Fixtures::new();
    let dir = f.path().join("ext");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("a.JPEG"), "x").unwrap();

    let plan = BatchRenamerTool
        .plan(&BatchRenameParams {
            paths: vec![dir.join("a.JPEG")],
            date: Some("202401".into()),
            subject: None,
            camera: None,
            film: None,
            order: RenameOrder::Numeric,
        })
        .unwrap()
        .data;

    assert!(plan.actions[0].target.ends_with("202401-01.jpeg"));
}

// ---------------------------------------------------------------------------
// The dry-run guarantee (acceptance, every tool)
// ---------------------------------------------------------------------------

/// **Phase 3 acceptance.** `plan` makes no filesystem modification, asserted by
/// hashing the directory before and after.
#[test]
fn planning_never_touches_the_filesystem() {
    let f = Fixtures::new();
    let dir = f.path().join("untouched");
    fs::create_dir(&dir).unwrap();

    let dated = f.jpeg_with_exif("a.jpg", 40, 40, "2019:05:06 07:08:09", "CAM");
    fs::rename(&dated, dir.join("a.jpg")).unwrap();
    let bare = f.jpeg_without_exif("b.jpg", 40, 40);
    fs::rename(&bare, dir.join("b.jpg")).unwrap();
    fs::write(
        dir.join("c.jpg.json"),
        r#"{"photoTakenTime":{"timestamp":"1704067200"}}"#,
    )
    .unwrap();
    fs::write(dir.join("c.jpg"), "x").unwrap();

    let before = hash_tree(&dir);
    let paths = vec![dir.join("a.jpg"), dir.join("b.jpg"), dir.join("c.jpg")];

    for mode in [
        RepairMode::Auto,
        RepairMode::Manual(dt("2020:01:01 00:00:00")),
        RepairMode::Shift("+1:0:0 0:0:0".into()),
        RepairMode::Sidecar,
    ] {
        let _ = DateRepairTool
            .plan(&DateRepairParams {
                paths: paths.clone(),
                mode,
                recursive: false,
            })
            .unwrap();
        assert_eq!(hash_tree(&dir), before, "F1 plan modified the directory");
    }

    let _ = BatchRenamerTool
        .plan(&BatchRenameParams {
            paths: paths.clone(),
            date: Some("20240101".into()),
            subject: Some("Trip".into()),
            camera: None,
            film: None,
            order: RenameOrder::Capture,
        })
        .unwrap();
    assert_eq!(hash_tree(&dir), before, "F3 plan modified the directory");

    let _ = f1_dates::scan_dates(&dir, true).unwrap();
    let _ = f2_takeout::scan_sidecars(&dir, true).unwrap();
    assert_eq!(hash_tree(&dir), before, "a scan modified the directory");
}

// ---------------------------------------------------------------------------
// Tag priority at file level
// ---------------------------------------------------------------------------

/// The positions of F1's order that a real file can actually carry.
///
/// `nom-exif` 3.6 does not surface `QuickTime:CreationDate`, `Keys:CreationDate`,
/// `XMP:CreateDate` or `QuickTime:ModifyDate` separately, so those positions are
/// covered by the exhaustive unit test in `media::meta` instead, and are listed
/// in `docs/manual-verification.md`.
#[test]
fn f1_tag_priority_holds_for_the_positions_a_file_can_carry() {
    let f = Fixtures::new();

    // Position 1 wins over position 2.
    let both = f.jpeg_with_tags(
        "both.jpg",
        40,
        40,
        &[],
        &[
            (
                tag::DATE_TIME_ORIGINAL,
                TiffValue::Ascii("2001:01:01 01:01:01".into()),
            ),
            (
                tag::CREATE_DATE,
                TiffValue::Ascii("2002:02:02 02:02:02".into()),
            ),
        ],
    );
    let meta = read_meta(&both).unwrap();
    assert_eq!(meta.capture, Some(dt("2001:01:01 01:01:01")));
    assert_eq!(meta.capture_source.unwrap().name(), "EXIF:DateTimeOriginal");

    // With position 1 absent, position 2 wins.
    let only_create = f.jpeg_with_tags(
        "create.jpg",
        40,
        40,
        &[],
        &[(
            tag::CREATE_DATE,
            TiffValue::Ascii("2002:02:02 02:02:02".into()),
        )],
    );
    let meta = read_meta(&only_create).unwrap();
    assert_eq!(meta.capture_source.unwrap().name(), "EXIF:CreateDate");

    // Position 4, from a QuickTime container.
    let mov = f.quicktime("clip.mov", 1_704_067_200);
    let meta = read_meta(&mov).unwrap();
    assert_eq!(meta.capture_source.unwrap().name(), "QuickTime:CreateDate");
}

#[test]
fn shift_deltas_round_trip_through_the_public_api() {
    let d = ShiftDelta::parse("+5:0:0 0:0:0").unwrap();
    assert_eq!(
        d.apply(dt("2019:01:02 03:04:05")),
        Some(dt("2024:01:02 03:04:05"))
    );
}

// ---------------------------------------------------------------------------
// §9.1 — the two performance targets that had no measurement
// ---------------------------------------------------------------------------
//
// Four targets are stated; a 400-shot card scan and a 24 MP resize were
// measured and these two were not. As with the resize benchmark, the figure is
// asserted only in a release build — the targets describe optimised code, and a
// debug number is evidence of nothing.

/// §9.1: a date scan of 500 library files in under five seconds.
#[test]
fn benchmark_a_date_scan_of_five_hundred_files() {
    use phototools_core::tools::f1_dates::scan_dates;

    let f = Fixtures::new();
    let root = f.path().join("library");
    fs::create_dir_all(&root).unwrap();

    // Small frames: this measures the metadata path, which §9.1's first rule
    // says must stay in-process, not the decoder.
    for i in 0..500 {
        f.jpeg_with_exif(
            &format!("library/IMG_{i:04}.jpg"),
            64,
            48,
            "2024:05:01 12:00:00",
            "PENTAX 17",
        );
    }

    let started = std::time::Instant::now();
    let results = scan_dates(&root, false).unwrap();
    let elapsed = started.elapsed();

    assert_eq!(results.len(), 500);
    println!(
        "500-file date scan: {elapsed:?}  [{} build]",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );

    #[cfg(not(debug_assertions))]
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "specification §9.1 target is 5 s, measured {elapsed:?}"
    );
}

/// §9.1: a contact sheet from 200 images in under twenty seconds.
#[test]
fn benchmark_a_contact_sheet_from_two_hundred_images() {
    use phototools_core::tools::f5_contact::{ContactSheetParams, ContactSheetTool};

    let f = Fixtures::new();
    let root = f.path().join("shoot");
    fs::create_dir_all(&root).unwrap();

    // 800×600 is small for a photograph and large enough that the thumbnail is
    // real work rather than a memcpy.
    for i in 0..200 {
        f.jpeg_without_exif(&format!("shoot/frame_{i:03}.jpg"), 800, 600);
    }

    let out = f.path().join("sheet.jpg");
    let params = ContactSheetParams::new(vec![root], out.clone());

    let started = std::time::Instant::now();
    let plan = ContactSheetTool.plan(&params).unwrap().data;
    let summary = ContactSheetTool
        .apply(plan, &InMemoryProgress::default())
        .unwrap()
        .data;
    let elapsed = started.elapsed();

    assert_eq!(summary.cells, 200);
    println!(
        "200-image contact sheet: {elapsed:?}  [{} build]",
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        }
    );

    #[cfg(not(debug_assertions))]
    assert!(
        elapsed < std::time::Duration::from_secs(20),
        "specification §9.1 target is 20 s, measured {elapsed:?}"
    );
}

// ---------------------------------------------------------------------------
// What a repair reports when it cannot confirm what it wrote
// ---------------------------------------------------------------------------

/// **A repair that wrote files must never report "nothing matched".**
///
/// Found on a deployed server: thirty-nine photographs were scanned, a manual
/// date was applied, and the answer was *"Nothing to do: nothing matched. If
/// the files are inside a subfolder, tick Include subfolders."* — advice that
/// was wrong about files that had been written. `verified_count` was passed as
/// "done", so an unconfirmed write counted as zero and the generic summary read
/// zero as "nothing was a candidate".
#[test]
fn files_written_but_unconfirmed_are_reported_as_such_not_as_nothing_matched() {
    use phototools_core::tools::f1_dates::{report, DateRepairOutcome, DateRepairSummary};

    let summary = DateRepairSummary {
        outcomes: (0..39)
            .map(|i| DateRepairOutcome {
                path: format!("/library/R1-02646-{i:04}.JPG").into(),
                intended: dt("2013:05:01 12:00:00"),
                metadata_verified: true,
                // The write went out; the read-back did not agree.
                filesystem_verified: false,
                note: Some("filesystem modification time did not read back as intended".into()),
            })
            .collect(),
        failures: Vec::new(),
    };

    let line = report(&summary, &[]);

    assert!(
        !line.contains("nothing matched"),
        "thirty-nine files were written; got: {line}"
    );
    assert!(
        !line.contains("subfolder"),
        "and the advice does not apply: {line}"
    );
    assert!(line.contains("39 written but not confirmed"), "got: {line}");
    assert!(
        line.contains("filesystem modification time"),
        "and it says why, so somebody can fix it: {line}"
    );
}

/// A failure without its reason is a number somebody can do nothing with.
#[test]
fn a_repair_that_failed_says_why_it_failed() {
    use phototools_core::tools::f1_dates::{report, DateRepairSummary};

    let summary = DateRepairSummary {
        outcomes: Vec::new(),
        failures: (0..39)
            .map(|i| {
                (
                    std::path::PathBuf::from(format!("/library/R1-02646-{i:04}.JPG")),
                    "/library/R1-02646-0000.JPG was not written: Error: Writing not permitted"
                        .to_string(),
                )
            })
            .collect(),
    };

    let line = report(&summary, &[]);
    assert!(line.contains("39 failed"), "got: {line}");
    assert!(
        line.contains("Writing not permitted"),
        "and says why, once: {line}"
    );
}

#[test]
fn a_repair_that_confirmed_everything_says_so_plainly() {
    use phototools_core::tools::f1_dates::{report, DateRepairOutcome, DateRepairSummary};

    let summary = DateRepairSummary {
        outcomes: vec![DateRepairOutcome {
            path: "/library/one.jpg".into(),
            intended: dt("2013:05:01 12:00:00"),
            metadata_verified: true,
            filesystem_verified: true,
            note: None,
        }],
        failures: Vec::new(),
    };

    assert_eq!(report(&summary, &[]), "1 redated and verified");
}

/// The generic advice is still right when genuinely nothing was a candidate.
#[test]
fn a_repair_with_no_outcomes_at_all_still_suggests_the_subfolder_box() {
    use phototools_core::tools::f1_dates::{report, DateRepairSummary};

    let line = report(&DateRepairSummary::default(), &[]);
    assert!(line.contains("nothing matched"), "got: {line}");
    assert!(line.contains("Include subfolders"), "got: {line}");
}

// ---------------------------------------------------------------------------
// ED-3 Image edit tools — sidecars, rename, card safety, export
// ---------------------------------------------------------------------------

#[test]
fn a_sidecar_is_named_after_the_whole_file_so_raw_and_jpeg_pairs_do_not_share_one() {
    use phototools_core::tools::edit::sidecar_path;

    let jpg_path = Path::new("/library/photos/IMG_0001.JPG");
    let raw_path = Path::new("/library/photos/IMG_0001.CR2");

    let jpg_sidecar = sidecar_path(jpg_path);
    let raw_sidecar = sidecar_path(raw_path);

    assert_eq!(
        jpg_sidecar,
        Path::new("/library/photos/IMG_0001.JPG.photoedit")
    );
    assert_eq!(
        raw_sidecar,
        Path::new("/library/photos/IMG_0001.CR2.photoedit")
    );
    assert_ne!(
        jpg_sidecar, raw_sidecar,
        "RAW and JPEG pairs side-by-side must never share a sidecar"
    );
}

#[test]
fn a_sidecar_round_trips_and_a_future_version_is_refused() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::{load_recipe, save_recipe, CURRENT_RECIPE_VERSION};

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("photo.jpg", 100, 100);

    let recipe = AdjustmentRecipe {
        exposure: 0.75,
        temperature: 0.1,
        tint: -0.2,
        highlights: -0.5,
        shadows: 0.3,
        contrast: 0.15,
        saturation: 0.05,
        vibrance: -0.1,
        lut: Some(phototools_core::media::edit::LutRef {
            name: "creative.cube".into(),
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        }),
        lut_intensity: 0.8,
        ..AdjustmentRecipe::default()
    };

    let sidecar = save_recipe(&img, &recipe).unwrap();
    assert!(sidecar.exists());

    let loaded = load_recipe(&sidecar).unwrap();
    assert_eq!(loaded.exposure, 0.75);
    assert_eq!(loaded.temperature, 0.1);
    assert_eq!(
        loaded.lut.as_ref().map(|l| l.name.as_str()),
        Some("creative.cube")
    );
    assert_eq!(loaded.lut_intensity, 0.8);
    assert!(!loaded.source_sha256.is_empty());

    // Future version refused
    let future_json = serde_json::json!({
        "version": CURRENT_RECIPE_VERSION + 1,
        "source_sha256": loaded.source_sha256,
        "exposure": 1.0
    });
    fs::write(&sidecar, future_json.to_string()).unwrap();

    let err = load_recipe(&sidecar).unwrap_err();
    assert!(
        err.to_string().contains("unsupported sidecar version"),
        "expected refusal of future version, got: {err}"
    );
}

#[test]
fn a_v1_sidecar_loads_into_v2_with_default_identities() {
    use phototools_core::tools::edit::load_recipe;

    let f = Fixtures::new();
    let sidecar = f.path().join("test_v1.jpg.photoedit");
    let v1_json = serde_json::json!({
        "version": 1,
        "source_sha256": "abcdef123456",
        "exposure": 0.75,
        "temperature": -15.0,
        "tint": 10.0,
        "highlights": -25.0,
        "shadows": 20.0,
        "contrast": 15.0,
        "saturation": 5.0,
        "vibrance": -5.0,
        "lut": null,
        "lut_intensity": 1.0
    });
    fs::write(&sidecar, v1_json.to_string()).unwrap();

    let loaded = load_recipe(&sidecar).expect("v1 sidecar must load cleanly into v2");
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.source_sha256, "abcdef123456");
    assert_eq!(loaded.exposure, 0.75);
    assert_eq!(loaded.temperature, -15.0);
    assert_eq!(loaded.tint, 10.0);
    assert_eq!(loaded.highlights, -25.0);
    assert_eq!(loaded.shadows, 20.0);
    assert_eq!(loaded.contrast, 15.0);
    assert_eq!(loaded.saturation, 5.0);
    assert_eq!(loaded.vibrance, -5.0);
    // New v2 fields must have default identity values
    assert_eq!(loaded.whites, 0.0);
    assert_eq!(loaded.blacks, 0.0);
    assert_eq!(loaded.brightness, 0.0);
    assert_eq!(loaded.hue, 0.0);
}

#[test]
fn a_recipe_version_four_is_refused_as_unsupported() {
    use phototools_core::tools::edit::load_recipe;

    // Version 3 (masks, ED-19) is current; the first version this build does not know is 4.
    let f = Fixtures::new();
    let sidecar = f.path().join("version4.jpg.photoedit");
    let v4_json = serde_json::json!({
        "version": 4,
        "source_sha256": "abcdef123456",
        "exposure": 1.0
    });
    fs::write(&sidecar, v4_json.to_string()).unwrap();

    let err = load_recipe(&sidecar).expect_err("version 4 must be refused");
    assert!(
        err.to_string().contains("unsupported sidecar version 4"),
        "expected unsupported version error, got: {err}"
    );
}

#[test]
fn a_v1_sidecar_saved_again_is_written_as_the_current_version() {
    use phototools_core::tools::edit::{load_recipe, save_recipe};

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("v1_to_v2.jpg", 100, 100);
    let sidecar = phototools_core::tools::edit::sidecar_path(&img);

    let v1_json = serde_json::json!({
        "version": 1,
        "source_sha256": "original_sha",
        "exposure": 0.5,
        "temperature": 12.0,
        "tint": -8.0,
        "highlights": 30.0,
        "shadows": -20.0,
        "contrast": 10.0,
        "saturation": 8.0,
        "vibrance": 4.0,
        "lut": null,
        "lut_intensity": 1.0
    });
    fs::write(&sidecar, v1_json.to_string()).unwrap();

    // Load v1 recipe
    let loaded = load_recipe(&sidecar).expect("v1 sidecar must load");
    assert_eq!(loaded.version, 1);
    assert_eq!(loaded.exposure, 0.5);

    // Save recipe again
    let written = save_recipe(&img, &loaded).expect("re-saving v1 must succeed");
    assert_eq!(written, sidecar);

    // Read the file: must now have the current version and preserved original values
    let file_content = fs::read_to_string(&sidecar).unwrap();
    let saved_val: serde_json::Value = serde_json::from_str(&file_content).unwrap();
    assert_eq!(
        saved_val["version"],
        phototools_core::tools::CURRENT_RECIPE_VERSION
    );
    assert_eq!(saved_val["version"], 3);
    assert_eq!(saved_val["exposure"], 0.5);
    assert_eq!(saved_val["temperature"], 12.0);
    assert_eq!(saved_val["tint"], -8.0);
    assert_eq!(saved_val["highlights"], 30.0);
    assert_eq!(saved_val["shadows"], -20.0);
    assert_eq!(saved_val["contrast"], 10.0);
    assert_eq!(saved_val["saturation"], 8.0);
    assert_eq!(saved_val["vibrance"], 4.0);
}

#[test]
fn a_sidecar_is_written_atomically() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::{save_recipe, sidecar_path};

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("atomic.jpg", 100, 100);
    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..AdjustmentRecipe::default()
    };

    let written = save_recipe(&img, &recipe).unwrap();
    assert_eq!(written, sidecar_path(&img));
    assert!(written.exists());

    // Ensure no leftover temporary files in directory
    let parent = img.parent().unwrap();
    for entry in fs::read_dir(parent).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        assert!(
            !name.starts_with(".photoedit_tmp"),
            "temporary file left behind: {name}"
        );
    }
}

#[test]
fn f3_rename_carries_companion_photoedit_sidecar() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::{save_recipe, sidecar_path};

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("IMG_0001.jpg", 100, 100);
    let recipe = AdjustmentRecipe {
        exposure: 1.25,
        ..AdjustmentRecipe::default()
    };
    let sidecar = save_recipe(&img, &recipe).unwrap();
    assert!(sidecar.exists());
    let original_bytes = fs::read(&sidecar).unwrap();

    let plan = BatchRenamerTool
        .plan(&BatchRenameParams {
            paths: vec![img.clone()],
            date: Some("20240101".into()),
            subject: Some("Trip".into()),
            camera: None,
            film: None,
            order: RenameOrder::Numeric,
        })
        .unwrap()
        .data;

    assert_eq!(plan.actions.len(), 1);
    let target_photo = plan.actions[0].target.clone();
    let target_sidecar = sidecar_path(&target_photo);

    let summary = BatchRenamerTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;

    assert_eq!(summary.renamed.len(), 1);
    assert!(summary.failures.is_empty());
    assert!(!img.exists(), "original photo was renamed");
    assert!(!sidecar.exists(), "original sidecar was renamed");
    assert!(target_photo.exists(), "target photo exists");
    assert!(target_sidecar.exists(), "target sidecar exists");
    assert_eq!(
        fs::read(&target_sidecar).unwrap(),
        original_bytes,
        "sidecar content preserved intact"
    );
}

/// A version 3 sidecar, with an automatic mask's stored pixels and brush strokes, follows its
/// photograph through Rename byte for byte and still loads with every mask (ED-24).
#[test]
fn f3_rename_carries_masks_in_the_sidecar() {
    use phototools_core::media::edit::{
        AdjustmentRecipe, AutoTarget, LocalAdjustments, Mask, MaskKind, StoredRaster, Stroke,
    };
    use phototools_core::tools::edit::{load_recipe, save_recipe, sidecar_path};

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("IMG_0002.jpg", 120, 80);
    // A mask the size the models store, with detail, so the sidecar is as large as in use.
    let (w, h) = (1024u32, 683u32);
    let data: Vec<u8> = (0..w * h)
        .map(|i| (((i % w) * 7 + (i / w) * 13) % 256) as u8)
        .collect();
    let auto = Mask {
        id: "subject".into(),
        name: "Subject 1".into(),
        kind: MaskKind::Auto {
            target: AutoTarget::Subject,
            mask: StoredRaster::encode(w, h, &data, "sha").unwrap(),
        },
        invert: false,
        opacity: 0.8,
        enabled: true,
        adjustments: LocalAdjustments {
            exposure: 0.4,
            ..Default::default()
        },
        strokes: vec![Stroke {
            points: (0..120).map(|i| [0.2 + i as f32 * 0.005, 0.5]).collect(),
            radius: 0.03,
            feather: 0.5,
            flow: 1.0,
            erase: true,
        }],
    };
    let recipe = AdjustmentRecipe {
        masks: vec![auto],
        ..AdjustmentRecipe::default()
    };
    let sidecar = save_recipe(&img, &recipe).unwrap();
    let original_bytes = fs::read(&sidecar).unwrap();
    assert!(
        original_bytes.len() > 50_000,
        "a realistic sidecar: {} bytes",
        original_bytes.len()
    );

    let plan = BatchRenamerTool
        .plan(&BatchRenameParams {
            paths: vec![img.clone()],
            date: Some("20240101".into()),
            subject: Some("Masks".into()),
            camera: None,
            film: None,
            order: RenameOrder::Numeric,
        })
        .unwrap()
        .data;
    let target_sidecar = sidecar_path(&plan.actions[0].target);
    let summary = BatchRenamerTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;

    assert!(summary.failures.is_empty());
    assert_eq!(fs::read(&target_sidecar).unwrap(), original_bytes);
    let loaded = load_recipe(&target_sidecar).unwrap();
    assert_eq!(loaded.version, 3);
    assert_eq!(loaded.masks, recipe.masks);
    let MaskKind::Auto { mask, .. } = &loaded.masks[0].kind else {
        panic!("the automatic mask should load as one");
    };
    assert_eq!(mask.decode().unwrap().data, data);
}

#[test]
fn f3_rename_plans_a_conflict_when_the_sidecar_target_exists() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::{save_recipe, sidecar_path};

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("IMG_0001.jpg", 100, 100);
    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..AdjustmentRecipe::default()
    };
    save_recipe(&img, &recipe).unwrap();

    // Plant the target sidecar already on disk
    let target = f.path().join("20240101-Trip-01.jpg");
    let target_sidecar = sidecar_path(&target);
    fs::write(&target_sidecar, b"existing sidecar").unwrap();

    let plan = BatchRenamerTool
        .plan(&BatchRenameParams {
            paths: vec![img.clone()],
            date: Some("20240101".into()),
            subject: Some("Trip".into()),
            camera: None,
            film: None,
            order: RenameOrder::Numeric,
        })
        .unwrap()
        .data;

    assert!(
        plan.actions.is_empty(),
        "action must be skipped due to target sidecar conflict"
    );
    assert_eq!(plan.skipped.len(), 1);
    assert!(
        plan.skipped[0]
            .reason
            .contains("Would overwrite an existing sidecar"),
        "reason was: {}",
        plan.skipped[0].reason
    );
}

#[test]
fn f3_rename_reports_a_sidecar_that_could_not_follow_its_photo() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::save_recipe;

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("IMG_0001.jpg", 100, 100);
    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..AdjustmentRecipe::default()
    };
    let sidecar = save_recipe(&img, &recipe).unwrap();

    let plan = BatchRenamerTool
        .plan(&BatchRenameParams {
            paths: vec![img.clone()],
            date: Some("20240101".into()),
            subject: Some("Trip".into()),
            camera: None,
            film: None,
            order: RenameOrder::Numeric,
        })
        .unwrap()
        .data;

    // Simulate companion sidecar vanishing between plan and apply
    fs::remove_file(&sidecar).unwrap();

    let summary = BatchRenamerTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;

    // Photo was renamed in step 1, but step 2 reported the vanished sidecar against the photo
    assert_eq!(summary.renamed.len(), 1);
    assert_eq!(summary.failures.len(), 1);
    assert_eq!(summary.failures[0].0, img);
    assert!(
        summary.failures[0].1.contains("vanished"),
        "failure message must mention sidecar vanished: {}",
        summary.failures[0].1
    );
}

#[test]
fn editor_refuses_to_save_a_sidecar_for_a_nonexistent_image() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::save_recipe;

    let nonexistent = Path::new("/definitely/not/a/real/path/IMG_9999.JPG");
    let recipe = AdjustmentRecipe::default();
    let err = save_recipe(nonexistent, &recipe).unwrap_err();
    assert!(
        err.to_string().contains("does not exist"),
        "expected nonexistent image refusal, got: {err}"
    );
}

#[test]
fn editor_refuses_to_write_a_sidecar_on_a_card_volume() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::save_recipe_with_resolver;

    let temp = tempfile::tempdir().unwrap();
    let card_root = temp.path().join("card");
    let dcim = card_root.join("DCIM");
    fs::create_dir_all(&dcim).unwrap();
    let img = card_root.join("DCIM/100EOS5D/IMG_0001.JPG");
    fs::create_dir_all(img.parent().unwrap()).unwrap();
    fs::write(&img, b"fake jpg").unwrap();

    let recipe = AdjustmentRecipe::default();
    let resolver = |_p: &Path| Some(card_root.clone());
    let err = save_recipe_with_resolver(&img, &recipe, resolver).unwrap_err();

    assert!(
        err.to_string().contains(
            "Card media is read-only (G5). Copy files to a working folder to save edits."
        ),
        "expected card read-only refusal message, got: {err}"
    );
}

#[test]
fn a_mounted_share_without_dcim_is_writable() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::{is_card_volume_with_resolver, save_recipe_with_resolver};

    let temp = tempfile::tempdir().unwrap();
    let share_root = temp.path().join("share");
    fs::create_dir_all(&share_root).unwrap();
    let img = share_root.join("photos/IMG_0001.JPG");
    fs::create_dir_all(img.parent().unwrap()).unwrap();
    fs::write(&img, b"fake jpg").unwrap();

    let resolver = |_p: &Path| Some(share_root.clone());
    assert!(!is_card_volume_with_resolver(&img, resolver));

    let recipe = AdjustmentRecipe::default();
    let sidecar = save_recipe_with_resolver(&img, &recipe, resolver).unwrap();
    assert!(sidecar.exists());
}

#[test]
fn a_library_folder_containing_a_copied_dcim_is_not_a_card() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::{is_card_volume, save_recipe};

    let temp = tempfile::tempdir().unwrap();
    let library = temp.path().join("MyPhotoLibrary");
    let copied_dcim = library.join("DCIM").join("100CANON");
    fs::create_dir_all(&copied_dcim).unwrap();
    let img = copied_dcim.join("IMG_0001.JPG");
    fs::write(&img, b"fake jpg").unwrap();

    // With the real volume root detection (walk up to mount point):
    assert!(
        !is_card_volume(&img),
        "A copied DCIM folder inside a library is not at the mount root, so it must not be treated as a card"
    );

    let recipe = AdjustmentRecipe::default();
    let sidecar = save_recipe(&img, &recipe).unwrap();
    assert!(sidecar.exists());
}

#[test]
fn export_edited_image_appends_edit_suffix_and_never_overwrites() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::export_edited_image;

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("flower.jpg", 50, 50);
    let out_dir = f.path().join("exports");
    fs::create_dir_all(&out_dir).unwrap();

    let recipe = AdjustmentRecipe {
        exposure: 0.2,
        ..Default::default()
    };

    let res1 = export_edited_image(&img, &recipe, None, &out_dir).unwrap();
    assert_eq!(res1.path, out_dir.join("flower_edit.jpg"));
    assert!(res1.path.exists());

    let res2 = export_edited_image(&img, &recipe, None, &out_dir).unwrap();
    assert_eq!(res2.path, out_dir.join("flower_edit_1.jpg"));
    assert!(res2.path.exists());

    let res3 = export_edited_image(&img, &recipe, None, &out_dir).unwrap();
    assert_eq!(res3.path, out_dir.join("flower_edit_2.jpg"));
    assert!(res3.path.exists());

    // Verify earlier exports were not overwritten
    assert!(res1.path.exists());
    assert!(res2.path.exists());
}

#[test]
fn exporting_a_recipe_whose_lut_is_missing_is_refused() {
    use phototools_core::media::edit::{AdjustmentRecipe, LutRef};
    use phototools_core::tools::edit::export_edited_image;

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("photo.jpg", 50, 50);
    let out_dir = f.path().join("exports_lut");

    let recipe = AdjustmentRecipe {
        lut: Some(LutRef {
            name: "missing.cube".into(),
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
        }),
        lut_intensity: 1.0,
        ..AdjustmentRecipe::default()
    };

    let err = export_edited_image(&img, &recipe, None, &out_dir).unwrap_err();
    assert!(
        err.to_string().contains("no LUT was provided"),
        "expected missing LUT refusal, got: {err}"
    );
}

#[test]
fn a_failed_export_leaves_no_file_behind() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::export_edited_image;

    let f = Fixtures::new();
    let bad_img = f.path().join("corrupted.jpg");
    fs::write(&bad_img, b"not a real jpeg").unwrap();
    let out_dir = f.path().join("exports_clean");
    fs::create_dir_all(&out_dir).unwrap();

    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..AdjustmentRecipe::default()
    };

    let err = export_edited_image(&bad_img, &recipe, None, &out_dir);
    assert!(err.is_err(), "corrupted image export must fail");

    let entries: Vec<_> = fs::read_dir(&out_dir).unwrap().flatten().collect();
    assert!(
        entries.is_empty(),
        "failed export must leave no file behind, found: {entries:?}"
    );
}

#[test]
fn export_refuses_a_destination_on_a_card() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::export_edited_image_with_resolver;

    let temp = tempfile::tempdir().unwrap();
    let card_root = temp.path().join("card");
    let dcim = card_root.join("DCIM");
    fs::create_dir_all(&dcim).unwrap();
    let card_out = card_root.join("DCIM/exports");

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("flower.jpg", 50, 50);
    let recipe = AdjustmentRecipe::default();
    let resolver = |_p: &Path| Some(card_root.clone());

    let err =
        export_edited_image_with_resolver(&img, &recipe, None, &card_out, resolver).unwrap_err();

    assert!(
        err.to_string()
            .contains("Cannot export to a card volume: card media is read-only (G5)"),
        "expected card refusal, got: {err}"
    );
}

#[test]
fn exporting_an_untouched_jpeg_preserves_source_bytes_without_reencoding() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::export_edited_image;

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("original.jpg", 100, 100);
    let original_bytes = fs::read(&img).unwrap();
    let out_dir = f.path().join("exports");

    let recipe = AdjustmentRecipe::default();
    let res = export_edited_image(&img, &recipe, None, &out_dir).unwrap();
    let exported_bytes = fs::read(&res.path).unwrap();

    assert_eq!(
        exported_bytes, original_bytes,
        "Untouched image must be stream-copied byte-for-byte without re-encoding"
    );
}

#[test]
fn an_exported_jpeg_keeps_its_orientation_tag_and_capture_date() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::media::meta::Orientation;
    use phototools_core::tools::edit::export_edited_image;

    let f = Fixtures::new();
    let img = f.jpeg_with_tags(
        "portrait.jpg",
        100,
        200,
        &[(tag::ORIENTATION, TiffValue::Short(6))],
        &[(
            tag::DATE_TIME_ORIGINAL,
            TiffValue::Ascii("2024:05:15 12:00:00".into()),
        )],
    );
    let meta_before = read_meta(&img).unwrap();
    assert_eq!(meta_before.orientation, Orientation::Rotate90);
    assert!(meta_before.capture.is_some());

    let out_dir = f.path().join("exports");
    // Non-identity recipe triggers decode -> apply -> encode
    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..AdjustmentRecipe::default()
    };

    let res = export_edited_image(&img, &recipe, None, &out_dir).unwrap();
    assert!(res.path.exists());

    let meta_after = read_meta(&res.path).unwrap();
    assert_eq!(
        meta_after.orientation, meta_before.orientation,
        "Orientation tag must survive with unrotated pixels"
    );
    assert_eq!(
        meta_after.capture, meta_before.capture,
        "Capture date must survive into export"
    );
}

#[test]
fn a_16_bit_tiff_exports_as_a_16_bit_tiff() {
    use phototools_core::media::edit::{decode_image, AdjustmentRecipe, ImageBuffer};
    use phototools_core::tools::edit::export_edited_image;

    let f = Fixtures::new();
    let mut img16 = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::new(20, 20);
    for pixel in img16.pixels_mut() {
        *pixel = image::Rgb([10000u16, 20000u16, 30000u16]);
    }
    let tiff_path = f.path().join("test16.tiff");
    let dyn_img = image::DynamicImage::ImageRgb16(img16);
    dyn_img
        .save_with_format(&tiff_path, image::ImageFormat::Tiff)
        .unwrap();

    let out_dir = f.path().join("exports");
    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..AdjustmentRecipe::default()
    };

    let res = export_edited_image(&tiff_path, &recipe, None, &out_dir).unwrap();
    assert!(res.path.exists());
    assert_eq!(res.path.extension().and_then(|s| s.to_str()), Some("tiff"));

    let decoded = decode_image(&res.path).unwrap();
    match decoded {
        ImageBuffer::Rgb16 {
            width,
            height,
            data,
        } => {
            assert_eq!(width, 20);
            assert_eq!(height, 20);
            assert_eq!(data.len(), 20 * 20 * 3);
        }
        ImageBuffer::Rgb8 { .. } => {
            panic!("16-bit TIFF exported as 8-bit!");
        }
    }
}

// ---------------------------------------------------------------------------
// ED-5: Bulk LUT tool (crates/core/src/tools/lut.rs)
// ---------------------------------------------------------------------------

fn spawn_counting_shim(f: &Fixtures) -> (std::path::PathBuf, std::path::PathBuf) {
    let log = f.path().join("spawns.log");
    let shim = f.path().join("exiftool-shim");
    let real = std::process::Command::new("which")
        .arg("exiftool")
        .output()
        .unwrap()
        .stdout;
    let real = String::from_utf8(real).unwrap().trim().to_string();

    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\necho spawn >> {}\nexec {} \"$@\"\n",
            log.display(),
            real
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    (shim, log)
}

fn create_test_cube(dir: &Path) -> std::path::PathBuf {
    let p = dir.join("test.cube");
    let content = "\
TITLE \"Test Cube\"
LUT_3D_SIZE 2
0.0 0.0 0.0
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
    fs::write(&p, content).unwrap();
    p
}

#[test]
fn bulk_lut_over_many_files_starts_exactly_one_exiftool() {
    use phototools_core::tools::lut::{BulkLutParams, BulkLutTool};

    let f = Fixtures::new();
    let (shim, log) = spawn_counting_shim(&f);

    let mut inputs = Vec::new();
    for i in 0..6 {
        inputs.push(f.jpeg_with_exif(
            &format!("shot{i}.jpg"),
            40,
            40,
            &format!("2024:05:1{i} 10:00:00"),
            "CAM1",
        ));
    }

    let lut_path = create_test_cube(f.path());
    let out_dir = f.path().join("bulk_out");
    let params = BulkLutParams::new(inputs, lut_path, 1.0, out_dir);

    let plan = BulkLutTool.plan(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 6);

    let summary = BulkLutTool
        .apply_with(
            plan,
            &InMemoryProgress::default(),
            Some(&shim.to_string_lossy()),
        )
        .unwrap()
        .data;

    assert_eq!(summary.written.len(), 6);
    assert_eq!(summary.failures.len(), 0);

    let spawns = std::fs::read_to_string(&log).unwrap().lines().count();
    assert_eq!(
        spawns, 1,
        "6 files processed by BulkLutTool must start exactly one exiftool, not {spawns}"
    );
}

#[test]
fn bulk_lut_preserves_capture_date_and_gps_on_all_outputs() {
    use phototools_core::media::ExifWriter;
    use phototools_core::tools::geotag::{exif, TrackPoint};
    use phototools_core::tools::lut::{BulkLutParams, BulkLutTool};

    let f = Fixtures::new();
    let mut inputs = Vec::new();
    for i in 0..3 {
        let day = i + 1;
        let path = f.jpeg_with_exif(
            &format!("geo{i}.jpg"),
            50,
            50,
            &format!("2024:06:0{day} 14:00:00"),
            "GPSCAM",
        );
        let fix = TrackPoint {
            at: 1_767_259_800 + i as i64 * 60,
            lat: 52.531549 + i as f64 * 0.01,
            lon: 3.460808 + i as f64 * 0.01,
            ele: Some(36.4),
        };
        let mut writer = ExifWriter::start().unwrap();
        writer
            .set_tags(&path, &exif::render(&fix, true).args())
            .unwrap();
        writer.close().unwrap();
        inputs.push(path);
    }

    let lut_path = create_test_cube(f.path());
    let out_dir = f.path().join("geo_out");
    let params = BulkLutParams::new(inputs, lut_path, 0.8, out_dir);

    let plan = BulkLutTool.plan(&params).unwrap().data;
    let summary = BulkLutTool
        .apply(plan, &InMemoryProgress::default())
        .unwrap()
        .data;

    assert_eq!(summary.written.len(), 3);
    for (i, out_path) in summary.written.iter().enumerate() {
        let day = i + 1;
        let meta = read_meta(out_path).unwrap();
        assert_eq!(
            meta.capture,
            Some(dt(&format!("2024:06:0{day} 14:00:00"))),
            "capture date must be preserved on output {i}"
        );
        assert_eq!(meta.camera.as_deref(), Some("GPSCAM"));
        let fix = meta.gps.expect("GPS must survive into bulk LUT output");
        assert!((fix.lat - (52.531549 + i as f64 * 0.01)).abs() < 1e-4);
        assert!((fix.lon - (3.460808 + i as f64 * 0.01)).abs() < 1e-4);
    }
}

#[test]
fn bulk_lut_summary_reports_processed_skipped_and_failed() {
    use phototools_core::tools::lut::{BulkLutParams, BulkLutTool};

    let f = Fixtures::new();
    let good1 = f.jpeg_without_exif("good1.jpg", 30, 30);
    let good2 = f.jpeg_without_exif("good2.jpg", 30, 30);
    let corrupt = f.path().join("corrupt.jpg");
    fs::write(&corrupt, b"not a valid JPEG image file content at all").unwrap();

    let lut_path = create_test_cube(f.path());
    let out_dir = f.path().join("summary_out");
    let params = BulkLutParams::new(vec![good1, corrupt.clone(), good2], lut_path, 0.5, out_dir);

    let plan = BulkLutTool.plan(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 3);

    let summary = BulkLutTool
        .apply(plan, &InMemoryProgress::default())
        .unwrap()
        .data;

    assert_eq!(summary.written.len(), 2, "2 good files must be written");
    assert_eq!(summary.failures.len(), 1, "1 corrupt file must be reported");
    assert_eq!(summary.failures[0].0, corrupt);
    assert!(
        !summary.failures[0].1.is_empty(),
        "failure must include a reason"
    );
}

#[test]
fn bulk_lut_never_overwrites_and_names_outputs_with_lut_suffix() {
    use phototools_core::tools::lut::{BulkLutParams, BulkLutTool};

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("photo.jpg", 40, 40);
    let lut_path = create_test_cube(f.path());
    let out_dir = f.path().join("mono_out");

    let params = BulkLutParams::new(vec![img.clone()], lut_path.clone(), 1.0, out_dir.clone());

    // First apply -> photo_lut.jpg
    let plan1 = BulkLutTool.plan(&params).unwrap().data;
    let summary1 = BulkLutTool
        .apply(plan1, &InMemoryProgress::default())
        .unwrap()
        .data;
    assert_eq!(summary1.written.len(), 1);
    assert_eq!(summary1.written[0].file_name().unwrap(), "photo_lut.jpg");

    // Second apply -> photo_lut_1.jpg
    let plan2 = BulkLutTool.plan(&params).unwrap().data;
    let summary2 = BulkLutTool
        .apply(plan2, &InMemoryProgress::default())
        .unwrap()
        .data;
    assert_eq!(summary2.written.len(), 1);
    assert_eq!(summary2.written[0].file_name().unwrap(), "photo_lut_1.jpg");

    // Third apply -> photo_lut_2.jpg
    let plan3 = BulkLutTool.plan(&params).unwrap().data;
    let summary3 = BulkLutTool
        .apply(plan3, &InMemoryProgress::default())
        .unwrap()
        .data;
    assert_eq!(summary3.written.len(), 1);
    assert_eq!(summary3.written[0].file_name().unwrap(), "photo_lut_2.jpg");

    // All three files exist concurrently and were not overwritten
    assert!(out_dir.join("photo_lut.jpg").exists());
    assert!(out_dir.join("photo_lut_1.jpg").exists());
    assert!(out_dir.join("photo_lut_2.jpg").exists());
}

#[test]
fn bulk_lut_refuses_an_output_directory_on_a_card() {
    use phototools_core::tools::lut::{BulkLutParams, BulkLutTool};

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("photo.jpg", 40, 40);
    let lut_path = create_test_cube(f.path());

    let temp = tempfile::tempdir().unwrap();
    let card_root = temp.path().join("card");
    let dcim = card_root.join("DCIM");
    fs::create_dir_all(&dcim).unwrap();
    let card_out = card_root.join("DCIM/exports");

    let params = BulkLutParams::new(vec![img], lut_path, 1.0, card_out);
    let resolver = |_p: &Path| Some(card_root.clone());

    let err = BulkLutTool
        .plan_with_resolver(&params, resolver)
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("Cannot output to a card volume: card media is read-only (G5)"),
        "expected card refusal, got: {err}"
    );
}

#[test]
fn bulk_lut_cancelled_midway_leaves_no_partial_file_and_reports_what_it_wrote() {
    use phototools_core::jobs::Progress;
    use phototools_core::tools::lut::{BulkLutParams, BulkLutTool};
    use std::sync::atomic::{AtomicUsize, Ordering};

    let f = Fixtures::new();
    let img1 = f.jpeg_without_exif("shot1.jpg", 40, 40);
    let img2 = f.jpeg_without_exif("shot2.jpg", 40, 40);
    let img3 = f.jpeg_without_exif("shot3.jpg", 40, 40);
    let lut_path = create_test_cube(f.path());
    let out_dir = f.path().join("cancel_out");

    struct CancelAfterFirst {
        reports: AtomicUsize,
    }
    impl Progress for CancelAfterFirst {
        fn report(&self, _done: u64, _total: u64, _message: &str) {
            self.reports.fetch_add(1, Ordering::SeqCst);
        }
        fn cancelled(&self) -> bool {
            // Turns true after its first report
            self.reports.load(Ordering::SeqCst) >= 1
        }
    }

    let params = BulkLutParams::new(vec![img1, img2, img3], lut_path, 1.0, out_dir.clone());
    let plan = BulkLutTool.plan(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 3);

    let progress = CancelAfterFirst {
        reports: AtomicUsize::new(0),
    };
    let summary = BulkLutTool.apply(plan, &progress).unwrap().data;

    assert_eq!(
        summary.written.len(),
        1,
        "only the first file should have been written before cancellation"
    );
    assert!(out_dir.join("shot1_lut.jpg").exists());
    assert!(!out_dir.join("shot2_lut.jpg").exists());
    assert!(!out_dir.join("shot3_lut.jpg").exists());

    // Assert no partial files or temp files in out_dir
    let written_files: Vec<_> = std::fs::read_dir(&out_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(written_files, vec!["shot1_lut.jpg"]);

    // Summary line reports exactly what was written
    let summary_line = phototools_core::tools::summarise(
        summary.written.len(),
        "graded",
        summary.failures.len(),
        &summary.skipped,
        &phototools_core::tools::lut::ACCEPTED,
    );
    assert_eq!(summary_line, "1 graded, 0 failed");
}

#[test]
fn bulk_lut_skips_sidecars_and_hidden_files() {
    use phototools_core::tools::lut::{BulkLutParams, BulkLutTool};

    let f = Fixtures::new();
    let in_dir = f.path().join("mix_input");
    fs::create_dir_all(&in_dir).unwrap();

    let valid = in_dir.join("photo.jpg");
    let img_src = f.jpeg_without_exif("valid_src.jpg", 40, 40);
    fs::copy(&img_src, &valid).unwrap();

    let sidecar = in_dir.join("photo.jpg.photoedit");
    fs::write(&sidecar, b"{\"version\":1}").unwrap();

    let hidden_img = in_dir.join(".hidden.jpg");
    fs::copy(&img_src, &hidden_img).unwrap();

    let ds_store = in_dir.join(".DS_Store");
    fs::write(&ds_store, b"fake ds store").unwrap();

    let lut_path = create_test_cube(f.path());
    let out_dir = f.path().join("mix_out");

    let params = BulkLutParams::new(vec![in_dir], lut_path, 1.0, out_dir.clone());
    let plan = BulkLutTool.plan(&params).unwrap().data;

    assert_eq!(
        plan.actions.len(),
        1,
        "only valid photo.jpg should become an action, sidecars and hidden files skipped"
    );
    assert_eq!(plan.actions[0].source, valid);

    let summary = BulkLutTool
        .apply(plan, &InMemoryProgress::default())
        .unwrap()
        .data;

    assert_eq!(summary.written.len(), 1);
    assert_eq!(summary.written[0], out_dir.join("photo_lut.jpg"));
    assert!(!out_dir.join(".hidden_lut.jpg").exists());
    assert!(!out_dir.join(".DS_Store_lut.jpg").exists());
}

// ---------------------------------------------------------------------------
// ED-18: Bulk preset / recipe tool (crates/core/src/tools/bulk_edit.rs)
// ---------------------------------------------------------------------------

// (moved to bulk_edit_tests.rs — all nine ED-18 tests live in one file)

#[test]
fn sample_frames_spreads_across_the_folder_rather_than_taking_the_first() {
    use phototools_core::tools::lut::{sample_frames, BulkLutAction};
    use phototools_core::tools::Plan;
    use std::path::PathBuf;

    let actions: Vec<_> = (0..10)
        .map(|i| BulkLutAction {
            source: PathBuf::from(format!("/photos/IMG_{i:04}.jpg")),
            out_dir: PathBuf::from("/out"),
            lut_path: PathBuf::from("/luts/test.cube"),
            lut_sha256: "test_sha256".to_string(),
            intensity: 1.0,
        })
        .collect();

    let plan = Plan {
        actions,
        skipped: Vec::new(),
    };

    let samples = sample_frames(&plan, 3);
    assert_eq!(samples.len(), 3);
    assert_eq!(
        samples[0],
        PathBuf::from("/photos/IMG_0000.jpg"),
        "first sample must be the first frame"
    );
    assert_eq!(
        samples[1],
        PathBuf::from("/photos/IMG_0004.jpg"),
        "middle sample must be midway through the folder"
    );
    assert_eq!(
        samples[2],
        PathBuf::from("/photos/IMG_0009.jpg"),
        "last sample must be the last frame"
    );

    // Proves it does NOT take the first 3
    let first_three: Vec<_> = plan
        .actions
        .iter()
        .take(3)
        .map(|a| a.source.clone())
        .collect();
    assert_ne!(
        samples, first_three,
        "sample_frames must spread evenly across the folder, not just take the first N"
    );
}

#[test]
fn export_still_carries_metadata_after_the_split() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::edit::export_edited_image;

    let f = Fixtures::new();
    let img = f.jpeg_with_exif("source.jpg", 60, 60, "2025:07:08 12:00:00", "LEICA_M11");
    let out_dir = f.path().join("split_guard_out");

    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..AdjustmentRecipe::default()
    };

    let res = export_edited_image(&img, &recipe, None, &out_dir).unwrap();
    assert!(res.path.exists());

    let meta = read_meta(&res.path).unwrap();
    assert_eq!(
        meta.capture,
        Some(dt("2025:07:08 12:00:00")),
        "Capture date must survive into export after render_and_write split"
    );
    assert_eq!(
        meta.camera.as_deref(),
        Some("LEICA_M11"),
        "Camera tag must survive into export after render_and_write split"
    );
}

#[test]
fn a_malformed_lut_is_refused_at_plan_time_not_after_running() {
    use phototools_core::tools::lut::{BulkLutParams, BulkLutTool};

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("photo.jpg", 30, 30);
    let broken_lut = f.path().join("broken.cube");
    fs::write(&broken_lut, b"TITLE \"Broken\"\nLUT_3D_SIZE 2\n0.0 0.0\n").unwrap();

    let out_dir = f.path().join("out");
    let params = BulkLutParams::new(vec![img], broken_lut, 1.0, out_dir);

    let err = BulkLutTool.plan(&params).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains(":3:"),
        "error must cite parser's line-numbered failure at plan time; got: {msg}"
    );
}

#[test]
fn a_lut_changed_after_the_dry_run_refuses_the_run() {
    use phototools_core::tools::lut::{BulkLutParams, BulkLutTool};

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("photo.jpg", 30, 30);
    let lut_path = create_test_cube(f.path());
    let out_dir = f.path().join("changed_out");

    let params = BulkLutParams::new(vec![img], lut_path.clone(), 1.0, out_dir);
    let plan = BulkLutTool.plan(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 1);

    // Modify the LUT file on disk after the plan / dry run has been produced
    let modified_content = "\
TITLE \"Modified Cube\"
LUT_3D_SIZE 2
0.1 0.1 0.1
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
    fs::write(&lut_path, modified_content).unwrap();

    let err = BulkLutTool
        .apply(plan, &InMemoryProgress::default())
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("changed on disk since the dry run"),
        "must refuse execution when LUT changes after dry run; got: {msg}"
    );
}

#[test]
fn import_lut_refuses_an_unparseable_file_and_never_overwrites_a_different_lut() {
    use phototools_core::tools::lut_library::{import_lut, list_luts};

    let f = Fixtures::new();
    let library_dir = f.path().join("luts");

    // 1. Unparseable file: refused at import time
    let broken = f.path().join("corrupted.cube");
    fs::write(&broken, b"LUT_3D_SIZE 2\n0.0 0.0\n").unwrap();
    let err = import_lut(&library_dir, &broken).unwrap_err();
    assert!(
        err.to_string().contains(":2:") || err.to_string().contains("corrupted.cube"),
        "broken file must be refused at import time: {err}"
    );

    // 2. Valid LUT: imported successfully
    let valid_a = create_test_cube(f.path()); // "test.cube"
    let entry_a = import_lut(&library_dir, &valid_a).unwrap();
    assert_eq!(entry_a.name, "test.cube");
    assert_eq!(entry_a.format, "cube");

    // Check it's in list_luts
    let list = list_luts(&library_dir).unwrap();
    assert_eq!(list.luts.len(), 1);
    assert_eq!(list.luts[0].name, "test.cube");

    // 3. Different LUT with the same file name: must refuse to overwrite
    let other_dir = f.path().join("other");
    fs::create_dir_all(&other_dir).unwrap();
    let different_lut = other_dir.join("test.cube");
    let different_content = "\
TITLE \"Different\"
LUT_3D_SIZE 2
0.5 0.5 0.5
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
    fs::write(&different_lut, different_content).unwrap();

    let err = import_lut(&library_dir, &different_lut).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("different LUT with that name already exists"),
        "importing different LUT under same name must be refused; got: {msg}"
    );

    // Verify existing file in library is unchanged
    let re_read = list_luts(&library_dir).unwrap();
    assert_eq!(re_read.luts[0].sha256, entry_a.sha256);
}

#[test]
fn a_recipe_whose_lut_left_the_library_is_refused_by_name() {
    use phototools_core::media::edit::pipeline::LutRef;
    use phototools_core::tools::lut_library::resolve_recipe_lut;

    let f = Fixtures::new();
    let library_dir = f.path().join("luts");
    fs::create_dir_all(&library_dir).unwrap();

    let lut_ref = LutRef {
        name: "VintageWarm.cube".into(),
        sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into(),
    };

    let err = resolve_recipe_lut(&library_dir, &lut_ref).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("VintageWarm.cube"),
        "error message must name the missing LUT; got: {msg}"
    );
    assert!(
        msg.contains("no longer in the library"),
        "error message must state it is no longer in the library; got: {msg}"
    );
}

#[test]
fn a_recipe_with_only_round_two_adjustments_is_rendered_not_byte_copied() {
    use phototools_core::media::edit::{AdjustmentRecipe, CurvePoint, HslAdjustments, ToneCurves};
    use phototools_core::tools::edit::export_edited_image;

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("original.jpg", 100, 100);
    let original_bytes = fs::read(&img).unwrap();
    let out_dir = f.path().join("exports");

    // 1. Curves-only recipe
    let curves_recipe = AdjustmentRecipe {
        curves: Some(ToneCurves {
            luma: vec![
                CurvePoint::new(0.0, 0.0),
                CurvePoint::new(0.5, 0.6),
                CurvePoint::new(1.0, 1.0),
            ],
            red: vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)],
            green: vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)],
            blue: vec![CurvePoint::new(0.0, 0.0), CurvePoint::new(1.0, 1.0)],
        }),
        ..Default::default()
    };
    assert!(!curves_recipe.is_identity());
    let res_curves = export_edited_image(&img, &curves_recipe, None, &out_dir).unwrap();
    let curves_bytes = fs::read(&res_curves.path).unwrap();
    assert_ne!(
        curves_bytes, original_bytes,
        "Curves-only recipe must be rendered, not byte-copied"
    );

    // 2. HSL-only recipe
    let mut hsl = HslAdjustments::default();
    hsl.red.saturation = 50.0;
    let hsl_recipe = AdjustmentRecipe {
        hsl: Some(hsl),
        ..Default::default()
    };
    assert!(!hsl_recipe.is_identity());
    let res_hsl = export_edited_image(&img, &hsl_recipe, None, &out_dir).unwrap();
    let hsl_bytes = fs::read(&res_hsl.path).unwrap();
    assert_ne!(
        hsl_bytes, original_bytes,
        "HSL-only recipe must be rendered, not byte-copied"
    );

    // 3. LUT with 0% intensity is identity and byte-copied
    let zero_lut_recipe = AdjustmentRecipe {
        lut: Some(phototools_core::media::edit::pipeline::LutRef {
            name: "test.cube".into(),
            sha256: "dummy".into(),
        }),
        lut_intensity: 0.0,
        ..Default::default()
    };
    assert!(
        zero_lut_recipe.is_identity(),
        "Recipe with 0% intensity LUT must be considered identity"
    );
}
