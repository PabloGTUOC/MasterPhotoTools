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
