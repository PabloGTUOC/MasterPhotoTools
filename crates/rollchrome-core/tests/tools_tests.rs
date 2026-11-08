//! Editing tool acceptance tests moved from `phototools-core` in RC-1.

#[path = "../../core/tests/fixtures/mod.rs"]
mod fixtures;

use chrono::NaiveDateTime;
use fixtures::{tag, Fixtures, TiffValue};
use phototools_core::jobs::InMemoryProgress;
use phototools_core::media::read_meta;
use phototools_core::tools::f3_rename::{BatchRenameParams, BatchRenamerTool, RenameOrder};
use phototools_core::tools::Tool;
use std::fs;
use std::path::Path;

fn dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S").unwrap()
}

#[test]
fn a_sidecar_round_trips_and_a_future_version_is_refused() {
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::{load_recipe, save_recipe, CURRENT_RECIPE_VERSION};

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
        lut: Some(rollchrome_core::media::edit::LutRef {
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
    use rollchrome_core::tools::edit::load_recipe;

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
    use rollchrome_core::tools::edit::load_recipe;

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
    use rollchrome_core::tools::edit::{load_recipe, save_recipe};

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("v1_to_v2.jpg", 100, 100);
    let sidecar = rollchrome_core::tools::edit::sidecar_path(&img);

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
        rollchrome_core::tools::CURRENT_RECIPE_VERSION
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
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::{save_recipe, sidecar_path};

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

/// A version 3 sidecar, with an automatic mask's stored pixels and brush strokes, follows its
/// photograph through Rename byte for byte and still loads with every mask (ED-24).
#[test]
fn f3_rename_carries_masks_in_the_sidecar() {
    use rollchrome_core::media::edit::{
        AdjustmentRecipe, AutoTarget, LocalAdjustments, Mask, MaskKind, StoredRaster, Stroke,
    };
    use rollchrome_core::tools::edit::{load_recipe, save_recipe, sidecar_path};

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
        components: Vec::new(),
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
fn editor_refuses_to_save_a_sidecar_for_a_nonexistent_image() {
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::save_recipe;

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
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::save_recipe_with_resolver;

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
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::{is_card_volume_with_resolver, save_recipe_with_resolver};

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
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::{is_card_volume, save_recipe};

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
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::export_edited_image;

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
    use rollchrome_core::media::edit::{AdjustmentRecipe, LutRef};
    use rollchrome_core::tools::edit::export_edited_image;

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
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::export_edited_image;

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
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::export_edited_image_with_resolver;

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
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::export_edited_image;

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
    use phototools_core::media::meta::Orientation;
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::export_edited_image;

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
    use rollchrome_core::media::edit::{decode_image, AdjustmentRecipe, ImageBuffer};
    use rollchrome_core::tools::edit::export_edited_image;

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
    use rollchrome_core::tools::lut::{BulkLutParams, BulkLutTool};

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
    use rollchrome_core::tools::lut::{BulkLutParams, BulkLutTool};

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
    use rollchrome_core::tools::lut::{BulkLutParams, BulkLutTool};

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
    use rollchrome_core::tools::lut::{BulkLutParams, BulkLutTool};

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
    use rollchrome_core::tools::lut::{BulkLutParams, BulkLutTool};

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
    use rollchrome_core::tools::lut::{BulkLutParams, BulkLutTool};
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
        &rollchrome_core::tools::lut::ACCEPTED,
    );
    assert_eq!(summary_line, "1 graded, 0 failed");
}

#[test]
fn bulk_lut_skips_sidecars_and_hidden_files() {
    use rollchrome_core::tools::lut::{BulkLutParams, BulkLutTool};

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
    use phototools_core::tools::Plan;
    use rollchrome_core::tools::lut::{sample_frames, BulkLutAction};
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
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::export_edited_image;

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
    use rollchrome_core::tools::lut::{BulkLutParams, BulkLutTool};

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
    use rollchrome_core::tools::lut::{BulkLutParams, BulkLutTool};

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
    use rollchrome_core::tools::lut_library::{import_lut, list_luts};

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
    use rollchrome_core::media::edit::pipeline::LutRef;
    use rollchrome_core::tools::lut_library::resolve_recipe_lut;

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
    use rollchrome_core::media::edit::{AdjustmentRecipe, CurvePoint, HslAdjustments, ToneCurves};
    use rollchrome_core::tools::edit::export_edited_image;

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
        lut: Some(rollchrome_core::media::edit::pipeline::LutRef {
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

#[test]
fn an_edit_export_of_a_raw_file_is_refused_and_writes_nothing() {
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::export_edited_image;

    let f = Fixtures::new();
    let raw_path = f.path().join("camera_raw.nef");
    fs::write(&raw_path, b"dummy raw data").unwrap();

    let out_dir = f.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();

    // 1. Identity recipe
    let identity_recipe = AdjustmentRecipe::default();
    let err = export_edited_image(&raw_path, &identity_recipe, None, &out_dir).unwrap_err();
    match err {
        phototools_core::error::Error::Refused(msg) => {
            assert_eq!(
                msg,
                "RAW files are developed in the Develop tab. Open the JPEG or TIFF it writes."
            );
        }
        other => panic!("expected Error::Refused, got {other:?}"),
    }
    assert_eq!(
        fs::read_dir(&out_dir).unwrap().count(),
        0,
        "out_dir must remain empty after refused RAW export"
    );

    // 2. Non-identity recipe
    let non_identity_recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..Default::default()
    };
    let err = export_edited_image(&raw_path, &non_identity_recipe, None, &out_dir).unwrap_err();
    match err {
        phototools_core::error::Error::Refused(msg) => {
            assert_eq!(
                msg,
                "RAW files are developed in the Develop tab. Open the JPEG or TIFF it writes."
            );
        }
        other => panic!("expected Error::Refused, got {other:?}"),
    }
    assert_eq!(
        fs::read_dir(&out_dir).unwrap().count(),
        0,
        "out_dir must remain empty after refused RAW export"
    );
}

#[test]
fn a_batch_given_a_raw_file_skips_it_and_says_so() {
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::bulk_edit::{BulkEditParams, BulkEditTool};
    use rollchrome_core::tools::lut::{BulkLutParams, BulkLutTool};

    let f = Fixtures::new();
    let raw_file = f.path().join("camera_raw.nef");
    fs::write(&raw_file, b"dummy raw data").unwrap();

    let out_dir = f.path().join("out");
    fs::create_dir_all(&out_dir).unwrap();

    // 1. BulkLutTool
    let lut_path = create_test_cube(f.path());
    let lut_params = BulkLutParams::new(vec![raw_file.clone()], lut_path, 1.0, out_dir.clone());
    let lut_plan = BulkLutTool.plan(&lut_params).unwrap().data;
    assert_eq!(lut_plan.actions.len(), 0);
    assert_eq!(lut_plan.skipped.len(), 1);
    assert_eq!(lut_plan.skipped[0].file, raw_file.to_string_lossy());
    assert_eq!(
        lut_plan.skipped[0].reason,
        "Not a file type this tool accepts"
    );

    // 2. BulkEditTool
    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..Default::default()
    };
    let edit_params = BulkEditParams::new(
        vec![raw_file.clone()],
        recipe,
        f.path().join("luts"),
        out_dir.clone(),
    );
    let edit_plan = BulkEditTool.plan(&edit_params).unwrap().data;
    assert_eq!(edit_plan.actions.len(), 0);
    assert_eq!(edit_plan.skipped.len(), 1);
    assert_eq!(edit_plan.skipped[0].file, raw_file.to_string_lossy());
    assert_eq!(
        edit_plan.skipped[0].reason,
        "Not a file type this tool accepts"
    );
}

// ---------------------------------------------------------------------------
// RP-5 Snapshots & History tests
// ---------------------------------------------------------------------------

#[test]
fn a_snapshot_survives_a_save_and_a_load() {
    use rollchrome_core::media::edit::{AdjustmentRecipe, Snapshot};
    use rollchrome_core::tools::edit::{load_recipe, save_recipe, sidecar_path};

    let f = Fixtures::new();
    let img_path = f.path().join("photo.jpg");
    fs::write(&img_path, b"fake jpeg image data").unwrap();

    let snap_recipe = AdjustmentRecipe {
        exposure: 1.2,
        temperature: 25.0,
        ..Default::default()
    };
    let recipe = AdjustmentRecipe {
        exposure: -0.5,
        snapshots: vec![Snapshot {
            name: "Warm sunset".into(),
            recipe: snap_recipe,
        }],
        ..Default::default()
    };

    save_recipe(&img_path, &recipe).expect("save recipe with snapshot");
    let sidecar = sidecar_path(&img_path);
    assert!(sidecar.exists());

    let loaded = load_recipe(&sidecar).expect("load saved recipe");
    assert_eq!(loaded.exposure, -0.5);
    assert_eq!(loaded.snapshots.len(), 1);
    assert_eq!(loaded.snapshots[0].name, "Warm sunset");
    assert_eq!(loaded.snapshots[0].recipe.exposure, 1.2);
    assert_eq!(loaded.snapshots[0].recipe.temperature, 25.0);
}

#[test]
fn a_saved_snapshot_never_holds_snapshots_of_its_own() {
    use rollchrome_core::media::edit::{AdjustmentRecipe, Snapshot};
    use rollchrome_core::tools::edit::{load_recipe, save_recipe, sidecar_path};

    let f = Fixtures::new();
    let img_path = f.path().join("photo2.jpg");
    fs::write(&img_path, b"fake jpeg image data").unwrap();

    let nested_snap = Snapshot {
        name: "Nested".into(),
        recipe: AdjustmentRecipe::default(),
    };
    let snap_with_nested = Snapshot {
        name: "Top-level".into(),
        recipe: AdjustmentRecipe {
            exposure: 0.75,
            snapshots: vec![nested_snap],
            ..Default::default()
        },
    };
    let recipe = AdjustmentRecipe {
        snapshots: vec![snap_with_nested],
        ..Default::default()
    };

    save_recipe(&img_path, &recipe).expect("save recipe");
    let loaded = load_recipe(&sidecar_path(&img_path)).expect("load recipe");
    assert_eq!(loaded.snapshots.len(), 1);
    assert!(
        loaded.snapshots[0].recipe.snapshots.is_empty(),
        "a saved snapshot's recipe must have its snapshots cleared"
    );
}

#[test]
fn a_preset_saved_from_a_recipe_with_snapshots_carries_none() {
    use rollchrome_core::media::edit::{AdjustmentRecipe, Snapshot};
    use rollchrome_core::tools::presets::{load_preset, save_preset};

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();

    let recipe = AdjustmentRecipe {
        exposure: 0.8,
        snapshots: vec![Snapshot {
            name: "Should be dropped".into(),
            recipe: AdjustmentRecipe::default(),
        }],
        ..Default::default()
    };

    save_preset(dir, "clean_preset", &recipe, false).expect("save preset");
    let loaded = load_preset(dir, "clean_preset").expect("load preset");
    assert_eq!(loaded.exposure, 0.8);
    assert!(
        loaded.snapshots.is_empty(),
        "a preset must carry no snapshots"
    );
}

#[test]
fn a_recipe_without_snapshots_is_written_without_the_field() {
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::{save_recipe, sidecar_path};

    let f = Fixtures::new();
    let img_path = f.path().join("no_snaps.jpg");
    fs::write(&img_path, b"fake jpeg image data").unwrap();

    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        snapshots: Vec::new(),
        ..Default::default()
    };

    save_recipe(&img_path, &recipe).expect("save recipe");
    let content = fs::read_to_string(sidecar_path(&img_path)).unwrap();
    assert!(
        !content.contains("\"snapshots\""),
        "recipe without snapshots must not serialize the snapshots field"
    );
}

#[test]
fn a_recipe_with_only_snapshots_is_still_identity() {
    use rollchrome_core::media::edit::{AdjustmentRecipe, Snapshot};

    let recipe = AdjustmentRecipe {
        snapshots: vec![Snapshot {
            name: "Snapshot 1".into(),
            recipe: AdjustmentRecipe {
                exposure: 2.5,
                contrast: 50.0,
                ..Default::default()
            },
        }],
        ..Default::default()
    };

    assert!(
        recipe.is_identity(),
        "a recipe with only snapshots and no edits must be identity"
    );
}

#[test]
fn a_version_survives_a_save_and_a_load() {
    use rollchrome_core::media::edit::{AdjustmentRecipe, Version};
    use rollchrome_core::tools::edit::{load_recipe, save_recipe, sidecar_path};

    let f = Fixtures::new();
    let img_path = f.path().join("photo_versions.jpg");
    fs::write(&img_path, b"fake jpeg image data").unwrap();

    let colour_recipe = AdjustmentRecipe {
        exposure: 0.5,
        saturation: 20.0,
        ..Default::default()
    };
    let recipe = AdjustmentRecipe {
        exposure: -0.2,
        version_name: Some("B&W".into()),
        versions: vec![Version {
            name: "Colour".into(),
            recipe: colour_recipe,
        }],
        ..Default::default()
    };

    save_recipe(&img_path, &recipe).expect("save recipe with version");
    let sidecar = sidecar_path(&img_path);
    assert!(sidecar.exists());

    let loaded = load_recipe(&sidecar).expect("load saved recipe");
    assert_eq!(loaded.exposure, -0.2);
    assert_eq!(loaded.version_name.as_deref(), Some("B&W"));
    assert_eq!(loaded.versions.len(), 1);
    assert_eq!(loaded.versions[0].name, "Colour");
    assert_eq!(loaded.versions[0].recipe.exposure, 0.5);
    assert_eq!(loaded.versions[0].recipe.saturation, 20.0);
}

#[test]
fn a_saved_version_never_holds_versions_or_snapshots_of_its_own() {
    use rollchrome_core::media::edit::{AdjustmentRecipe, Snapshot, Version};
    use rollchrome_core::tools::edit::{load_recipe, save_recipe, sidecar_path};

    let f = Fixtures::new();
    let img_path = f.path().join("photo_nested_versions.jpg");
    fs::write(&img_path, b"fake jpeg image data").unwrap();

    let inner_version = Version {
        name: "Inner".into(),
        recipe: AdjustmentRecipe::default(),
    };
    let inner_snapshot = Snapshot {
        name: "InnerSnap".into(),
        recipe: AdjustmentRecipe::default(),
    };
    let version_with_nested = Version {
        name: "TopVersion".into(),
        recipe: AdjustmentRecipe {
            exposure: 0.75,
            version_name: Some("InnerName".into()),
            versions: vec![inner_version],
            snapshots: vec![inner_snapshot],
            ..Default::default()
        },
    };

    let recipe = AdjustmentRecipe {
        exposure: 0.1,
        versions: vec![version_with_nested],
        ..Default::default()
    };

    save_recipe(&img_path, &recipe).expect("save recipe");
    let loaded = load_recipe(&sidecar_path(&img_path)).expect("load recipe");
    assert_eq!(loaded.versions.len(), 1);
    assert!(
        loaded.versions[0].recipe.versions.is_empty(),
        "a saved version's recipe must have its versions cleared"
    );
    assert!(
        loaded.versions[0].recipe.snapshots.is_empty(),
        "a saved version's recipe must have its snapshots cleared"
    );
    assert!(
        loaded.versions[0].recipe.version_name.is_none(),
        "a saved version's recipe must have version_name set to None"
    );
}

#[test]
fn a_preset_saved_from_a_recipe_with_versions_carries_none() {
    use rollchrome_core::media::edit::{AdjustmentRecipe, Version};
    use rollchrome_core::tools::presets::{load_preset, save_preset};

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path();

    let recipe = AdjustmentRecipe {
        exposure: 0.8,
        version_name: Some("V1".into()),
        versions: vec![Version {
            name: "Should be dropped".into(),
            recipe: AdjustmentRecipe::default(),
        }],
        ..Default::default()
    };

    save_preset(dir, "clean_preset_versions", &recipe, false).expect("save preset");
    let loaded = load_preset(dir, "clean_preset_versions").expect("load preset");
    assert_eq!(loaded.exposure, 0.8);
    assert!(
        loaded.versions.is_empty(),
        "a preset must carry no versions"
    );
    assert!(
        loaded.version_name.is_none(),
        "a preset must carry no version_name"
    );
}

#[test]
fn a_recipe_with_only_versions_is_still_identity() {
    use rollchrome_core::media::edit::{AdjustmentRecipe, Version};

    let recipe = AdjustmentRecipe {
        version_name: Some("V1".into()),
        versions: vec![Version {
            name: "V2".into(),
            recipe: AdjustmentRecipe {
                exposure: 2.5,
                contrast: 50.0,
                ..Default::default()
            },
        }],
        ..Default::default()
    };

    assert!(
        recipe.is_identity(),
        "a recipe with only versions and version_name must be identity"
    );
}

#[test]
fn a_named_version_exports_with_its_name_in_the_file_name() {
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::export_edited_image;

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("sunset.jpg", 40, 40);
    let out_dir = f.path().join("exports");
    fs::create_dir_all(&out_dir).unwrap();

    let recipe = AdjustmentRecipe {
        exposure: 0.2,
        version_name: Some("B&W Contrast".into()),
        ..Default::default()
    };

    let res = export_edited_image(&img, &recipe, None, &out_dir).unwrap();
    assert_eq!(res.path, out_dir.join("sunset_edit_B-W-Contrast.jpg"));
    assert!(res.path.exists());
}

#[test]
fn an_unnamed_version_exports_exactly_as_before() {
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::export_edited_image;

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("portrait.jpg", 40, 40);
    let out_dir = f.path().join("exports");
    fs::create_dir_all(&out_dir).unwrap();

    let recipe = AdjustmentRecipe {
        exposure: 0.2,
        version_name: None,
        ..Default::default()
    };

    let res = export_edited_image(&img, &recipe, None, &out_dir).unwrap();
    assert_eq!(res.path, out_dir.join("portrait_edit.jpg"));
    assert!(res.path.exists());
}

#[test]
fn a_version_name_cannot_escape_the_output_folder() {
    use rollchrome_core::media::edit::AdjustmentRecipe;
    use rollchrome_core::tools::edit::export_edited_image;

    let f = Fixtures::new();
    let img = f.jpeg_without_exif("landscape.jpg", 40, 40);
    let out_dir = f.path().join("exports");
    fs::create_dir_all(&out_dir).unwrap();

    // Test with traversal characters "../../x"
    let recipe_traversal = AdjustmentRecipe {
        exposure: 0.1,
        version_name: Some("../../x".into()),
        ..Default::default()
    };
    let res_traversal = export_edited_image(&img, &recipe_traversal, None, &out_dir).unwrap();
    assert_eq!(res_traversal.path.parent(), Some(out_dir.as_path()));
    assert_eq!(
        res_traversal.path.file_name().unwrap(),
        "landscape_edit_------x.jpg"
    );
    assert!(res_traversal.path.exists());

    // Test with only separators "///"
    let recipe_separators = AdjustmentRecipe {
        exposure: 0.1,
        version_name: Some("///".into()),
        ..Default::default()
    };
    let res_separators = export_edited_image(&img, &recipe_separators, None, &out_dir).unwrap();
    assert_eq!(res_separators.path.parent(), Some(out_dir.as_path()));
    assert_eq!(
        res_separators.path.file_name().unwrap(),
        "landscape_edit_---.jpg"
    );
    assert!(res_separators.path.exists());
}
