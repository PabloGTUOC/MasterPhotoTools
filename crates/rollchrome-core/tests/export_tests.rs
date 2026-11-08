//! Acceptance tests for RP-9: Export options.

use phototools_core::error::Error;
use phototools_core::media::meta::read_meta;
use phototools_core::media::ExifWriter;
use phototools_core::tools::geotag::{exif, TrackPoint};
use rollchrome_core::media::edit::geometry::{Geometry, NormalizedCrop};
use rollchrome_core::media::edit::AdjustmentRecipe;
use rollchrome_core::tools::edit::{
    export_edited_image, export_edited_image_with_options, ExportFormat, ExportOptions,
    ExportResize, MetadataPolicy, OutputSharpening,
};
use std::fs::File;
use std::path::Path;
use tempfile::TempDir;

#[path = "../../core/tests/fixtures/mod.rs"]
mod fixtures;
use fixtures::Fixtures;

fn create_test_tiff16(path: &Path, w: u32, h: u32) {
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let r = ((x * 65535) / w.max(1)) as u16;
            let g = ((y * 65535) / h.max(1)) as u16;
            let b = (((x + y) * 32767) / (w + h).max(1)) as u16;
            data.push(r);
            data.push(g);
            data.push(b);
        }
    }
    let img = image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_raw(w, h, data).unwrap();
    let dyn_img = image::DynamicImage::ImageRgb16(img);
    let mut file = File::create(path).unwrap();
    dyn_img
        .write_to(&mut file, image::ImageFormat::Tiff)
        .unwrap();
}

fn create_test_rgb8(path: &Path, w: u32, h: u32) {
    let mut img = image::RgbImage::new(w, h);
    for (x, y, pixel) in img.enumerate_pixels_mut() {
        let r = ((x * 255) / w.max(1)) as u8;
        let g = ((y * 255) / h.max(1)) as u8;
        let b = 128u8;
        *pixel = image::Rgb([r, g, b]);
    }
    img.save(path).unwrap();
}

#[test]
fn the_default_options_export_exactly_as_before() {
    let f = Fixtures::new();
    let src = f.jpeg_without_exif("source.jpg", 120, 80);
    let out_dir = f.path().join("export_defaults");
    std::fs::create_dir_all(&out_dir).unwrap();

    // 1. Identity recipe -> byte-identical copy
    let identity_recipe = AdjustmentRecipe::default();
    let res_old = export_edited_image(&src, &identity_recipe, None, &out_dir).unwrap();
    let res_new = export_edited_image_with_options(
        &src,
        &identity_recipe,
        None,
        &out_dir,
        &ExportOptions::default(),
    )
    .unwrap();

    let bytes_src = std::fs::read(&src).unwrap();
    let bytes_old = std::fs::read(&res_old.path).unwrap();
    let bytes_new = std::fs::read(&res_new.path).unwrap();

    assert_eq!(
        bytes_old, bytes_src,
        "identity export must be byte-copy of source"
    );
    assert_eq!(
        bytes_new, bytes_src,
        "default options export must be byte-copy of source"
    );

    // 2. Non-identity recipe -> byte identical between export_edited_image and default options
    let out_dir_non_id = f.path().join("export_defaults_non_id");
    std::fs::create_dir_all(&out_dir_non_id).unwrap();
    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        contrast: 10.0,
        ..Default::default()
    };
    let res_non_id_old = export_edited_image(&src, &recipe, None, &out_dir_non_id).unwrap();
    let out_dir_non_id2 = f.path().join("export_defaults_non_id2");
    std::fs::create_dir_all(&out_dir_non_id2).unwrap();
    let res_non_id_new = export_edited_image_with_options(
        &src,
        &recipe,
        None,
        &out_dir_non_id2,
        &ExportOptions::default(),
    )
    .unwrap();

    let bytes_non_id_old = std::fs::read(&res_non_id_old.path).unwrap();
    let bytes_non_id_new = std::fs::read(&res_non_id_new.path).unwrap();
    assert_eq!(bytes_non_id_old, bytes_non_id_new);
}

#[test]
fn a_sixteen_bit_source_exports_a_jpeg_and_a_higher_quality_is_larger() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("source16.tiff");
    create_test_tiff16(&src, 200, 200);

    let out_dir = tmp.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let recipe = AdjustmentRecipe::default();
    let opts_q50 = ExportOptions {
        format: ExportFormat::Jpeg { quality: 50 },
        ..Default::default()
    };
    let res_q50 =
        export_edited_image_with_options(&src, &recipe, None, &out_dir, &opts_q50).unwrap();
    assert_eq!(
        res_q50.path.extension().and_then(|s| s.to_str()),
        Some("jpg")
    );

    let opts_q95 = ExportOptions {
        format: ExportFormat::Jpeg { quality: 95 },
        ..Default::default()
    };
    let res_q95 =
        export_edited_image_with_options(&src, &recipe, None, &out_dir, &opts_q95).unwrap();

    let size_50 = std::fs::metadata(&res_q50.path).unwrap().len();
    let size_95 = std::fs::metadata(&res_q95.path).unwrap().len();
    assert!(
        size_95 > size_50,
        "Quality 95 ({size_95} bytes) must be larger than quality 50 ({size_50} bytes)"
    );
}

#[test]
fn an_eight_bit_source_exports_a_sixteen_bit_tiff() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("source8.jpg");
    create_test_rgb8(&src, 100, 100);

    let out_dir = tmp.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let opts = ExportOptions {
        format: ExportFormat::Tiff { bits: 16 },
        ..Default::default()
    };
    let res =
        export_edited_image_with_options(&src, &AdjustmentRecipe::default(), None, &out_dir, &opts)
            .unwrap();

    assert_eq!(res.path.extension().and_then(|s| s.to_str()), Some("tiff"));
    let decoded = image::open(&res.path).unwrap();
    match decoded {
        image::DynamicImage::ImageRgb16(img) => {
            assert_eq!(img.width(), 100);
            assert_eq!(img.height(), 100);
        }
        other => panic!("expected 16-bit RGB TIFF, got {other:?}"),
    }
}

#[test]
fn png_and_webp_open_again_with_the_same_pixels() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("test_pixels.tiff");
    // Write 60x40 with known exact pixels
    let mut orig = image::RgbImage::new(60, 40);
    for (x, y, px) in orig.enumerate_pixels_mut() {
        *px = image::Rgb([(x * 4) as u8, (y * 6) as u8, ((x + y) * 2) as u8]);
    }
    let mut file = File::create(&src).unwrap();
    image::DynamicImage::ImageRgb8(orig.clone())
        .write_to(&mut file, image::ImageFormat::Tiff)
        .unwrap();

    let out_dir = tmp.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let opts_png = ExportOptions {
        format: ExportFormat::Png { bits: 8 },
        ..Default::default()
    };
    let res_png = export_edited_image_with_options(
        &src,
        &AdjustmentRecipe::default(),
        None,
        &out_dir,
        &opts_png,
    )
    .unwrap();
    let decoded_png = image::open(&res_png.path).unwrap().to_rgb8();
    assert_eq!(decoded_png.as_raw(), orig.as_raw(), "PNG must be lossless");

    let opts_webp = ExportOptions {
        format: ExportFormat::Webp,
        ..Default::default()
    };
    let res_webp = export_edited_image_with_options(
        &src,
        &AdjustmentRecipe::default(),
        None,
        &out_dir,
        &opts_webp,
    )
    .unwrap();
    let decoded_webp = image::open(&res_webp.path).unwrap().to_rgb8();
    assert_eq!(
        decoded_webp.as_raw(),
        orig.as_raw(),
        "WebP must be lossless"
    );
}

#[test]
fn avif_opens_again_close_to_the_source() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("avif_test.tiff");
    let mut orig = image::RgbImage::new(80, 80);
    for (x, y, px) in orig.enumerate_pixels_mut() {
        *px = image::Rgb([(x * 3) as u8, (y * 3) as u8, 100]);
    }
    let mut file = File::create(&src).unwrap();
    image::DynamicImage::ImageRgb8(orig.clone())
        .write_to(&mut file, image::ImageFormat::Tiff)
        .unwrap();

    let out_dir = tmp.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let opts_avif = ExportOptions {
        format: ExportFormat::Avif { quality: 80 },
        ..Default::default()
    };
    let res = export_edited_image_with_options(
        &src,
        &AdjustmentRecipe::default(),
        None,
        &out_dir,
        &opts_avif,
    )
    .unwrap();

    // Decode exported AVIF via sips conversion to PNG (macOS native AVIF support)
    let png_conv = tmp.path().join("avif_decoded.png");
    let status = std::process::Command::new("sips")
        .args([
            "-s",
            "format",
            "png",
            res.path.to_str().unwrap(),
            "--out",
            png_conv.to_str().unwrap(),
        ])
        .output()
        .expect("sips must convert AVIF to PNG");
    assert!(
        status.status.success(),
        "sips conversion must succeed: {:?}",
        String::from_utf8_lossy(&status.stderr)
    );

    let decoded = image::open(&png_conv).unwrap().to_rgb8();
    let mut total_diff = 0.0f64;
    let count = (80 * 80 * 3) as f64;
    for (p1, p2) in orig.as_raw().iter().zip(decoded.as_raw().iter()) {
        total_diff += (*p1 as f64 - *p2 as f64).abs();
    }
    let mean_diff = total_diff / count;
    assert!(
        mean_diff < 8.0,
        "AVIF mean difference {mean_diff} must be within 8 levels"
    );
}

#[test]
fn long_edge_and_megapixels_resize_and_never_enlarge() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("frame_400x200.jpg");
    create_test_rgb8(&src, 400, 200);

    let out_dir = tmp.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    // 1. Long edge downscale: 400 -> 200 => result 200x100
    let opts_down = ExportOptions {
        resize: ExportResize::LongEdge(200),
        ..Default::default()
    };
    let res_down = export_edited_image_with_options(
        &src,
        &AdjustmentRecipe::default(),
        None,
        &out_dir,
        &opts_down,
    )
    .unwrap();
    let img_down = image::open(&res_down.path).unwrap();
    assert_eq!((img_down.width(), img_down.height()), (200, 100));

    // 2. Long edge never enlarges: max 800 on 400x200 => keeps 400x200
    let opts_up = ExportOptions {
        resize: ExportResize::LongEdge(800),
        ..Default::default()
    };
    let res_up = export_edited_image_with_options(
        &src,
        &AdjustmentRecipe::default(),
        None,
        &out_dir,
        &opts_up,
    )
    .unwrap();
    let img_up = image::open(&res_up.path).unwrap();
    assert_eq!((img_up.width(), img_up.height()), (400, 200));

    // 3. Megapixels ceiling above image size: never enlarges
    let opts_mp_high = ExportOptions {
        resize: ExportResize::Megapixels(1), // 1 MP = 1,000,000 px, 400x200 = 80,000 px
        ..Default::default()
    };
    let res_mp_high = export_edited_image_with_options(
        &src,
        &AdjustmentRecipe::default(),
        None,
        &out_dir,
        &opts_mp_high,
    )
    .unwrap();
    let img_mp_high = image::open(&res_mp_high.path).unwrap();
    assert_eq!((img_mp_high.width(), img_mp_high.height()), (400, 200));

    // 4. Megapixels downscale on large image: 2000x1000 = 2 MP down to 1 MP
    let src_large = tmp.path().join("large_2000x1000.jpg");
    create_test_rgb8(&src_large, 2000, 1000);
    let opts_mp_down = ExportOptions {
        resize: ExportResize::Megapixels(1),
        ..Default::default()
    };
    let res_mp_down = export_edited_image_with_options(
        &src_large,
        &AdjustmentRecipe::default(),
        None,
        &out_dir,
        &opts_mp_down,
    )
    .unwrap();
    let img_mp_down = image::open(&res_mp_down.path).unwrap();
    assert!(img_mp_down.width() * img_mp_down.height() <= 1_000_000);
    assert!(img_mp_down.width() < 2000 && img_mp_down.height() < 1000);
}

#[test]
fn a_crop_is_resized_after_it_is_applied() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("crop_src.jpg");
    create_test_rgb8(&src, 400, 400);

    let out_dir = tmp.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    // Crop to 200x100 (x: 0, y: 0, w: 0.5, h: 0.25)
    let recipe = AdjustmentRecipe {
        geometry: Some(Geometry {
            crop: Some(NormalizedCrop {
                x: 0.0,
                y: 0.0,
                width: 0.5,
                height: 0.25,
            }),
            ..Geometry::default()
        }),
        ..Default::default()
    };

    // Resize long edge to 100: post-crop long edge is 200, so it resizes to 100x50
    let opts = ExportOptions {
        resize: ExportResize::LongEdge(100),
        ..Default::default()
    };
    let res = export_edited_image_with_options(&src, &recipe, None, &out_dir, &opts).unwrap();
    let decoded = image::open(&res.path).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (100, 50));
}

#[test]
fn output_sharpening_raises_edge_contrast_at_the_final_size() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("edge.tiff");

    // Create sharp step edge at x = 50: left is 60, right is 180
    let mut img = image::RgbImage::new(100, 40);
    for (x, _y, px) in img.enumerate_pixels_mut() {
        let val = if x < 50 { 60u8 } else { 180u8 };
        *px = image::Rgb([val, val, val]);
    }
    let mut file = File::create(&src).unwrap();
    image::DynamicImage::ImageRgb8(img)
        .write_to(&mut file, image::ImageFormat::Tiff)
        .unwrap();

    let out_dir = tmp.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let opts_off = ExportOptions {
        format: ExportFormat::Png { bits: 8 },
        output_sharpening: OutputSharpening::Off,
        ..Default::default()
    };
    let res_off = export_edited_image_with_options(
        &src,
        &AdjustmentRecipe::default(),
        None,
        &out_dir,
        &opts_off,
    )
    .unwrap();

    let opts_screen = ExportOptions {
        format: ExportFormat::Png { bits: 8 },
        output_sharpening: OutputSharpening::Screen,
        ..Default::default()
    };
    let res_screen = export_edited_image_with_options(
        &src,
        &AdjustmentRecipe::default(),
        None,
        &out_dir,
        &opts_screen,
    )
    .unwrap();

    let dec_off = image::open(&res_off.path).unwrap().to_rgb8();
    let dec_screen = image::open(&res_screen.path).unwrap().to_rgb8();

    // In dec_screen, pixel at x=49 should undershoot (< 60) and pixel at x=50 should overshoot (> 180)
    let p_left_off = dec_off.get_pixel(49, 20)[0] as f32;
    let p_right_off = dec_off.get_pixel(50, 20)[0] as f32;
    let contrast_off = p_right_off - p_left_off;

    let p_left_screen = dec_screen.get_pixel(49, 20)[0] as f32;
    let p_right_screen = dec_screen.get_pixel(50, 20)[0] as f32;
    let contrast_screen = p_right_screen - p_left_screen;

    assert!(
        contrast_screen > contrast_off,
        "Sharpened edge contrast ({contrast_screen}) must exceed unsharpened ({contrast_off})"
    );
}

#[test]
fn an_identity_recipe_with_a_resize_is_rendered_not_copied() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("identity_resize.jpg");
    create_test_rgb8(&src, 400, 200);
    let src_bytes = std::fs::read(&src).unwrap();

    let out_dir = tmp.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let opts = ExportOptions {
        resize: ExportResize::LongEdge(200),
        ..Default::default()
    };
    let res =
        export_edited_image_with_options(&src, &AdjustmentRecipe::default(), None, &out_dir, &opts)
            .unwrap();

    let out_bytes = std::fs::read(&res.path).unwrap();
    assert_ne!(
        out_bytes, src_bytes,
        "identity recipe with resize must be rendered, not byte-copied"
    );

    let decoded = image::open(&res.path).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (200, 100));
}

#[test]
fn the_name_template_fills_its_tokens() {
    let f = Fixtures::new();
    let src = f.jpeg_with_exif("myphoto.jpg", 80, 80, "2024:08:15 10:30:00", "TESTCAM");
    let out_dir = f.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let recipe = AdjustmentRecipe {
        version_name: Some("bw-print".into()),
        ..Default::default()
    };
    let opts = ExportOptions {
        name_template: Some("{date}_{name}_{version}_{edit}".into()),
        ..Default::default()
    };
    let res = export_edited_image_with_options(&src, &recipe, None, &out_dir, &opts).unwrap();

    let fname = res.path.file_name().unwrap().to_string_lossy();
    assert_eq!(fname, "2024-08-15_myphoto_bw-print__edit_bw-print.jpg");
}

#[test]
fn a_name_template_cannot_leave_the_folder() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("sample.jpg");
    create_test_rgb8(&src, 50, 50);

    let out_dir = tmp.path().join("subfolder");
    std::fs::create_dir_all(&out_dir).unwrap();

    let recipe = AdjustmentRecipe::default();

    // 1. Path traversal: ../../x
    let opts1 = ExportOptions {
        name_template: Some("../../escape".into()),
        ..Default::default()
    };
    let res1 = export_edited_image_with_options(&src, &recipe, None, &out_dir, &opts1).unwrap();
    assert!(
        res1.path.starts_with(&out_dir),
        "File must be inside out_dir"
    );
    assert_eq!(
        res1.path.file_name().unwrap().to_string_lossy(),
        "..-..-escape.jpg"
    );

    // 2. Absolute path: /etc/x
    let opts2 = ExportOptions {
        name_template: Some("/etc/escape".into()),
        ..Default::default()
    };
    let res2 = export_edited_image_with_options(&src, &recipe, None, &out_dir, &opts2).unwrap();
    assert!(
        res2.path.starts_with(&out_dir),
        "File must be inside out_dir"
    );
    assert_eq!(
        res2.path.file_name().unwrap().to_string_lossy(),
        "-etc-escape.jpg"
    );

    // 3. Template resolving to empty or only dots
    let opts_empty = ExportOptions {
        name_template: Some("".into()),
        ..Default::default()
    };
    let err_empty = export_edited_image_with_options(&src, &recipe, None, &out_dir, &opts_empty);
    assert!(matches!(err_empty, Err(Error::Refused(_))));

    let opts_dots = ExportOptions {
        name_template: Some("...".into()),
        ..Default::default()
    };
    let err_dots = export_edited_image_with_options(&src, &recipe, None, &out_dir, &opts_dots);
    assert!(matches!(err_dots, Err(Error::Refused(_))));

    let opts_ver_none = ExportOptions {
        name_template: Some("{version}".into()), // recipe has no version_name -> resolves to empty
        ..Default::default()
    };
    let err_ver_none =
        export_edited_image_with_options(&src, &recipe, None, &out_dir, &opts_ver_none);
    assert!(matches!(err_ver_none, Err(Error::Refused(_))));
}

#[test]
fn a_second_export_of_the_same_name_gets_a_number() {
    let tmp = TempDir::new().unwrap();
    let src = tmp.path().join("photo.jpg");
    create_test_rgb8(&src, 50, 50);

    let out_dir = tmp.path().join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    let recipe = AdjustmentRecipe {
        exposure: 0.1,
        ..Default::default()
    };
    let opts = ExportOptions {
        name_template: Some("export_name".into()),
        ..Default::default()
    };

    let res1 = export_edited_image_with_options(&src, &recipe, None, &out_dir, &opts).unwrap();
    assert_eq!(
        res1.path.file_name().unwrap().to_string_lossy(),
        "export_name.jpg"
    );

    let res2 = export_edited_image_with_options(&src, &recipe, None, &out_dir, &opts).unwrap();
    assert_eq!(
        res2.path.file_name().unwrap().to_string_lossy(),
        "export_name_1.jpg"
    );

    let res3 = export_edited_image_with_options(&src, &recipe, None, &out_dir, &opts).unwrap();
    assert_eq!(
        res3.path.file_name().unwrap().to_string_lossy(),
        "export_name_2.jpg"
    );
}

#[test]
fn the_metadata_policy_keeps_removes_or_drops() {
    let f = Fixtures::new();
    let path = f.jpeg_with_exif("gps_source.jpg", 60, 60, "2024:07:04 12:00:00", "CAM_GPS");

    // Add real GPS fix via ExifWriter
    let fix = TrackPoint {
        at: 1_720_094_400,
        lat: 48.8584,
        lon: 2.2945,
        ele: Some(35.0),
    };
    let mut writer = ExifWriter::start().unwrap();
    writer
        .set_tags(&path, &exif::render(&fix, true).args())
        .unwrap();
    writer.close().unwrap();

    let recipe = AdjustmentRecipe {
        exposure: 0.1,
        ..Default::default()
    };

    // 1. MetadataPolicy::All -> GPS present and camera tag kept
    let out_all = f.path().join("meta_all");
    std::fs::create_dir_all(&out_all).unwrap();
    let opts_all = ExportOptions {
        metadata: MetadataPolicy::All,
        ..Default::default()
    };
    let res_all =
        export_edited_image_with_options(&path, &recipe, None, &out_all, &opts_all).unwrap();
    let meta_all = read_meta(&res_all.path).unwrap();
    assert_eq!(meta_all.camera.as_deref(), Some("CAM_GPS"));
    let gps_all = meta_all
        .gps
        .expect("GPS must be kept under MetadataPolicy::All");
    assert!((gps_all.lat - 48.8584).abs() < 1e-4);
    assert!((gps_all.lon - 2.2945).abs() < 1e-4);

    // 2. MetadataPolicy::NoLocation -> GPS gone, other tags kept
    let out_noloc = f.path().join("meta_noloc");
    std::fs::create_dir_all(&out_noloc).unwrap();
    let opts_noloc = ExportOptions {
        metadata: MetadataPolicy::NoLocation,
        ..Default::default()
    };
    let res_noloc =
        export_edited_image_with_options(&path, &recipe, None, &out_noloc, &opts_noloc).unwrap();
    let meta_noloc = read_meta(&res_noloc.path).unwrap();
    assert_eq!(meta_noloc.camera.as_deref(), Some("CAM_GPS"));
    assert!(
        meta_noloc.gps.is_none(),
        "GPS must be removed under MetadataPolicy::NoLocation"
    );

    // 3. MetadataPolicy::None -> Nothing carried
    let out_none = f.path().join("meta_none");
    std::fs::create_dir_all(&out_none).unwrap();
    let opts_none = ExportOptions {
        metadata: MetadataPolicy::None,
        ..Default::default()
    };
    let res_none =
        export_edited_image_with_options(&path, &recipe, None, &out_none, &opts_none).unwrap();
    let meta_none = read_meta(&res_none.path).unwrap();
    assert!(
        meta_none.camera.is_none(),
        "Camera tag must not be carried under MetadataPolicy::None"
    );
    assert!(
        meta_none.gps.is_none(),
        "GPS must not be carried under MetadataPolicy::None"
    );
}
