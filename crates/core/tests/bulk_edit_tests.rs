//! ED-18 bulk preset tests that mirror the bulk_lut test suite.

mod fixtures;

use chrono::NaiveDateTime;
use fixtures::Fixtures;
use phototools_core::jobs::{InMemoryProgress, Progress};
use phototools_core::media::edit::AdjustmentRecipe;
use phototools_core::media::{read_meta, ExifWriter};
use phototools_core::tools::bulk_edit::{BulkEditParams, BulkEditTool};
use phototools_core::tools::geotag::{exif, TrackPoint};
use phototools_core::tools::Tool;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

fn dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S").unwrap()
}

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

#[test]
fn bulk_preset_refuses_when_recipe_changed_since_reviewed_plan() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::bulk_edit::{BulkEditParams, BulkEditTool};
    use phototools_core::tools::Tool;

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("photo.jpg", 40, 40);
    let out_dir = f.path().join("out");

    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..Default::default()
    };
    let params = BulkEditParams::new(
        vec![img.clone()],
        recipe.clone(),
        f.path().join("luts"),
        out_dir.clone(),
    );
    let plan = BulkEditTool.plan(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 1);

    let changed = AdjustmentRecipe {
        exposure: 0.6,
        ..Default::default()
    };
    let wrong_hash = phototools_core::tools::bulk_edit::recipe_sha256(&changed);

    let err = BulkEditTool
        .apply_with(plan, &wrong_hash, &InMemoryProgress::default(), None)
        .unwrap_err();
    assert!(
        err.to_string().contains("recipe changed since the dry run"),
        "expected recipe-lock refusal, got: {err}"
    );
    assert!(
        !out_dir.exists(),
        "no file should be written when the recipe lock fails"
    );
}

#[test]
fn recipe_hash_ignores_the_photograph_a_recipe_was_made_on() {
    use phototools_core::media::edit::AdjustmentRecipe;
    use phototools_core::tools::bulk_edit::recipe_sha256;

    let a = AdjustmentRecipe {
        exposure: 0.5,
        source_sha256: "aaa".into(),
        ..Default::default()
    };

    let b = AdjustmentRecipe {
        exposure: 0.5,
        source_sha256: "bbb".into(),
        ..Default::default()
    };

    assert_eq!(
        recipe_sha256(&a),
        recipe_sha256(&b),
        "recipes that differ only in source_sha256 must lock to the same hash"
    );
}

#[test]
fn bulk_preset_applies_full_recipe_identically_to_single_export() {
    use phototools_core::media::edit::{decode_image, AdjustmentRecipe, ImageBuffer};
    use phototools_core::tools::bulk_edit::{BulkEditParams, BulkEditTool};
    use phototools_core::tools::edit::export_edited_image;
    use phototools_core::tools::Tool;

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("photo.jpg", 40, 40);

    let recipe = AdjustmentRecipe {
        exposure: 0.25,
        contrast: 0.15,
        saturation: -0.1,
        shadows: 0.2,
        highlights: -0.15,
        ..Default::default()
    };

    let single_out = f.path().join("single_out");
    let single = export_edited_image(&img, &recipe, None, &single_out).unwrap();

    let bulk_out = f.path().join("bulk_out");
    let params = BulkEditParams::new(
        vec![img.clone()],
        recipe,
        f.path().join("luts"),
        bulk_out.clone(),
    );
    let plan = BulkEditTool.plan(&params).unwrap().data;
    let summary = BulkEditTool
        .apply(plan, &InMemoryProgress::default())
        .unwrap()
        .data;
    assert_eq!(summary.written.len(), 1);

    let bulk_path = &summary.written[0];

    let single_buf = decode_image(&single.path).unwrap();
    let bulk_buf = decode_image(bulk_path).unwrap();

    match (single_buf, bulk_buf) {
        (
            ImageBuffer::Rgb8 {
                width: w1,
                height: h1,
                data: d1,
            },
            ImageBuffer::Rgb8 {
                width: w2,
                height: h2,
                data: d2,
            },
        ) => {
            assert_eq!(w1, w2);
            assert_eq!(h1, h2);
            assert_eq!(d1, d2, "bulk export pixels must equal single export pixels");
        }
        _ => panic!("both outputs must decode as Rgb8"),
    }
}

#[test]
fn bulk_preset_cancellation_cleans_up_and_reports_accurate_counts() {
    let f = Fixtures::new();
    let img1 = f.jpeg_without_exif("shot1.jpg", 40, 40);
    let img2 = f.jpeg_without_exif("shot2.jpg", 40, 40);
    let img3 = f.jpeg_without_exif("shot3.jpg", 40, 40);
    let out_dir = f.path().join("cancel_out");

    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..Default::default()
    };
    let params = BulkEditParams::new(
        vec![img1, img2, img3],
        recipe,
        f.path().join("luts"),
        out_dir.clone(),
    );

    let plan = BulkEditTool.plan(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 3);

    struct CancelAfterFirst {
        reports: AtomicUsize,
    }
    impl Progress for CancelAfterFirst {
        fn report(&self, _done: u64, _total: u64, _message: &str) {
            self.reports.fetch_add(1, Ordering::SeqCst);
        }
        fn cancelled(&self) -> bool {
            self.reports.load(Ordering::SeqCst) >= 1
        }
    }

    let progress = CancelAfterFirst {
        reports: AtomicUsize::new(0),
    };
    let summary = BulkEditTool.apply(plan, &progress).unwrap().data;

    assert_eq!(
        summary.written.len(),
        1,
        "only the first file should have been written before cancellation"
    );
    assert!(out_dir.join("shot1_edit.jpg").exists());
    assert!(!out_dir.join("shot2_edit.jpg").exists());
    assert!(!out_dir.join("shot3_edit.jpg").exists());

    let written_files: Vec<_> = std::fs::read_dir(&out_dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(written_files, vec!["shot1_edit.jpg"]);

    let summary_line = phototools_core::tools::summarise(
        summary.written.len(),
        "edited",
        summary.failures.len(),
        &summary.skipped,
        &phototools_core::tools::lut::ACCEPTED,
    );
    assert_eq!(summary_line, "1 edited, 0 failed");
}

#[test]
fn bulk_preset_never_overwrites_existing_files() {
    let f = Fixtures::new();
    let img = f.jpeg_without_exif("photo.jpg", 40, 40);
    let out_dir = f.path().join("mono_out");

    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..Default::default()
    };
    let params = BulkEditParams::new(
        vec![img.clone()],
        recipe.clone(),
        f.path().join("luts"),
        out_dir.clone(),
    );

    let plan1 = BulkEditTool.plan(&params).unwrap().data;
    let summary1 = BulkEditTool
        .apply(plan1, &InMemoryProgress::default())
        .unwrap()
        .data;
    assert_eq!(summary1.written.len(), 1);
    assert_eq!(summary1.written[0].file_name().unwrap(), "photo_edit.jpg");

    let plan2 = BulkEditTool.plan(&params).unwrap().data;
    let summary2 = BulkEditTool
        .apply(plan2, &InMemoryProgress::default())
        .unwrap()
        .data;
    assert_eq!(summary2.written.len(), 1);
    assert_eq!(summary2.written[0].file_name().unwrap(), "photo_edit_1.jpg");

    let plan3 = BulkEditTool.plan(&params).unwrap().data;
    let summary3 = BulkEditTool
        .apply(plan3, &InMemoryProgress::default())
        .unwrap()
        .data;
    assert_eq!(summary3.written.len(), 1);
    assert_eq!(summary3.written[0].file_name().unwrap(), "photo_edit_2.jpg");

    assert!(out_dir.join("photo_edit.jpg").exists());
    assert!(out_dir.join("photo_edit_1.jpg").exists());
    assert!(out_dir.join("photo_edit_2.jpg").exists());
}

#[test]
fn bulk_preset_preserves_capture_date_and_gps_on_all_outputs() {
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

    let recipe = AdjustmentRecipe {
        exposure: 0.25,
        ..Default::default()
    };
    let out_dir = f.path().join("geo_out");
    let params = BulkEditParams::new(inputs.clone(), recipe, f.path().join("luts"), out_dir);

    let plan = BulkEditTool.plan(&params).unwrap().data;
    let summary = BulkEditTool
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
        let fix = meta.gps.expect("GPS must survive into bulk edit output");
        assert!((fix.lat - (52.531549 + i as f64 * 0.01)).abs() < 1e-4);
        assert!((fix.lon - (3.460808 + i as f64 * 0.01)).abs() < 1e-4);
    }
}

#[test]
fn bulk_preset_refuses_a_recipe_that_changes_nothing() {
    let f = Fixtures::new();
    let img = f.jpeg_without_exif("photo.jpg", 40, 40);
    let out_dir = f.path().join("out");

    let recipe = AdjustmentRecipe::default();
    let params = BulkEditParams::new(vec![img], recipe, f.path().join("luts"), out_dir);

    let err = BulkEditTool.plan(&params).unwrap_err();
    assert!(
        err.to_string().contains("Recipe does not change anything"),
        "expected identity refusal, got: {err}"
    );
}

#[test]
fn bulk_preset_refuses_an_output_directory_on_a_card() {
    let f = Fixtures::new();
    let img = f.jpeg_without_exif("photo.jpg", 40, 40);

    let temp = tempfile::tempdir().unwrap();
    let card_root = temp.path().join("card");
    let dcim = card_root.join("DCIM");
    fs::create_dir_all(&dcim).unwrap();
    let card_out = card_root.join("DCIM/exports");

    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..Default::default()
    };
    let params = BulkEditParams::new(vec![img], recipe, f.path().join("luts"), card_out);
    let resolver = |_p: &Path| Some(card_root.clone());

    let err = BulkEditTool
        .plan_with_resolver(&params, resolver)
        .unwrap_err();

    assert!(
        err.to_string()
            .contains("Cannot output to a card volume: card media is read-only (G5)"),
        "expected card refusal, got: {err}"
    );
}

#[test]
fn bulk_preset_over_many_files_starts_exactly_one_exiftool() {
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

    let recipe = AdjustmentRecipe {
        exposure: 0.25,
        ..Default::default()
    };
    let out_dir = f.path().join("bulk_out");
    let params = BulkEditParams::new(inputs, recipe, f.path().join("luts"), out_dir);

    let plan = BulkEditTool.plan(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 6);

    let summary = BulkEditTool
        .apply_with(
            plan,
            &phototools_core::tools::bulk_edit::recipe_sha256(&params.recipe),
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
        "6 files processed by BulkEditTool must start exactly one exiftool, not {spawns}"
    );
}

/// A mask is placed on one photograph's content, so a batch never applies one (ED-19): a
/// recipe with masks plans and locks exactly as the same recipe without them, and a recipe
/// whose only change is a mask is refused as changing nothing.
#[test]
fn masks_are_never_applied_by_batch() {
    use phototools_core::media::edit::{LocalAdjustments, Mask, MaskKind};
    use phototools_core::tools::bulk_edit::recipe_sha256;

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("photo.jpg", 40, 40);
    let out_dir = f.path().join("out");

    let sky = Mask {
        id: "sky".into(),
        name: "Sky".into(),
        kind: MaskKind::Linear {
            start: [0.5, 0.0],
            end: [0.5, 0.5],
        },
        invert: false,
        opacity: 1.0,
        enabled: true,
        adjustments: LocalAdjustments {
            exposure: -1.0,
            ..Default::default()
        },
    };

    let plain = AdjustmentRecipe {
        exposure: 0.5,
        ..Default::default()
    };
    let with_mask = AdjustmentRecipe {
        masks: vec![sky.clone()],
        ..plain.clone()
    };
    assert_eq!(recipe_sha256(&with_mask), recipe_sha256(&plain));

    let plan = BulkEditTool
        .plan(&BulkEditParams::new(
            vec![img.clone()],
            with_mask,
            f.path().join("luts"),
            out_dir.clone(),
        ))
        .unwrap()
        .data;
    assert!(plan.actions.iter().all(|a| a.recipe.masks.is_empty()));
    assert_eq!(plan.actions[0].recipe_sha256, recipe_sha256(&plain));

    let only_mask = AdjustmentRecipe {
        masks: vec![sky],
        ..Default::default()
    };
    let err = BulkEditTool
        .plan(&BulkEditParams::new(
            vec![img],
            only_mask,
            f.path().join("luts"),
            out_dir,
        ))
        .unwrap_err();
    assert!(
        err.to_string().contains("does not change anything"),
        "got: {err}"
    );
}
