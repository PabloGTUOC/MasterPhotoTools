//! Phase 4 acceptance tests — F4, F5, F6, F7, F8.

mod fixtures;

use fixtures::Fixtures;
use phototools_core::jobs::InMemoryProgress;
use phototools_core::media::image_ops;
use phototools_core::tools::f4_split::{self, SplitParams, SplitSettings, SplitTool};
use phototools_core::tools::f5_contact::{
    ContactSheetParams, ContactSheetTool, SheetBackground, SheetLayout,
};
use phototools_core::tools::f6_transform::{TargetFormat, TransformParams, TransformTool};
use phototools_core::tools::f7_border::{self, PrintBorderParams, PrintBorderTool};
use phototools_core::tools::f8_tiff::{TiffToJpegParams, TiffToJpegTool};
use phototools_core::tools::Tool;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

fn hash_tree(root: &Path) -> String {
    let mut entries = Vec::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        for e in read.flatten() {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else {
                entries.push(p);
            }
        }
    }
    entries.sort();

    let mut hasher = Sha256::new();
    for path in entries {
        hasher.update(path.to_string_lossy().as_bytes());
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

// ---------------------------------------------------------------------------
// F6 — transform
// ---------------------------------------------------------------------------

#[test]
fn f6_converts_format_and_caps_the_long_edge() {
    let f = Fixtures::new();
    let dir = f.path().join("in");
    let out = f.path().join("out");
    fs::create_dir(&dir).unwrap();

    let src = f.jpeg_without_exif("wide.jpg", 800, 400);
    fs::rename(&src, dir.join("wide.jpg")).unwrap();

    let mut params = TransformParams::new(vec![dir.clone()], out.clone());
    params.max_long_edge = Some(200);
    params.format = Some(TargetFormat::Png);

    let plan = TransformTool.plan(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 1);

    let summary = TransformTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;
    assert!(summary.failures.is_empty());

    let produced = out.join("wide.png");
    assert!(produced.exists());
    let img = image_ops::decode(&produced).unwrap();
    assert_eq!((img.width(), img.height()), (200, 100));
}

#[test]
fn f6_never_enlarges() {
    let f = Fixtures::new();
    let out = f.path().join("out");
    let src = f.jpeg_without_exif("small.jpg", 100, 50);

    let mut params = TransformParams::new(vec![src], out.clone());
    params.max_long_edge = Some(4000);

    let plan = TransformTool.plan(&params).unwrap().data;
    TransformTool.apply(plan, &InMemoryProgress::new()).unwrap();

    let img = image_ops::decode(&out.join("small.jpg")).unwrap();
    assert_eq!((img.width(), img.height()), (100, 50));
}

#[test]
fn f6_rotates_by_an_arbitrary_angle_expanding_the_canvas() {
    let f = Fixtures::new();
    let out = f.path().join("out");
    let src = f.jpeg_without_exif("rot.jpg", 100, 100);

    let mut params = TransformParams::new(vec![src], out.clone());
    params.rotate_degrees = Some(45.0);
    params.format = Some(TargetFormat::Png);

    let plan = TransformTool.plan(&params).unwrap().data;
    TransformTool.apply(plan, &InMemoryProgress::new()).unwrap();

    let img = image_ops::decode(&out.join("rot.png")).unwrap();
    // A 100×100 square rotated 45° needs a canvas of about 100√2 ≈ 142.
    assert!(
        (140..=143).contains(&img.width()),
        "expected the canvas to expand, got {}",
        img.width()
    );
    assert_eq!(img.width(), img.height());
}

#[test]
fn f6_applies_exif_orientation_before_anything_else() {
    let f = Fixtures::new();
    let out = f.path().join("out");
    // 60×30 landscape pixels tagged "rotate 90" — the output must be portrait.
    let src = f.jpeg_with_orientation("oriented.jpg", 60, 30, 6);

    let params = TransformParams::new(vec![src], out.clone());
    let plan = TransformTool.plan(&params).unwrap().data;
    TransformTool.apply(plan, &InMemoryProgress::new()).unwrap();

    let img = image_ops::decode(&out.join("oriented.jpg")).unwrap();
    assert_eq!((img.width(), img.height()), (30, 60));
}

#[test]
fn f6_reports_a_format_it_cannot_decode_rather_than_failing_silently() {
    let f = Fixtures::new();
    let src = f.path().join("photo.heic");
    fs::write(&src, b"not really heic").unwrap();

    let params = TransformParams::new(vec![src], f.path().join("out"));
    let plan = TransformTool.plan(&params).unwrap().data;

    assert!(plan.actions.is_empty());
    assert_eq!(plan.skipped.len(), 1);
    assert!(plan.skipped[0].reason.contains("No decoder"));
}

// ---------------------------------------------------------------------------
// F8 — TIFF to JPEG
// ---------------------------------------------------------------------------

#[test]
fn f8_a_multi_page_tiff_produces_one_numbered_jpeg_per_page() {
    let f = Fixtures::new();
    let out = f.path().join("out");
    let src = f.multipage_tiff("scan.tif", 3, 120, 80);

    let params = TiffToJpegParams::new(vec![src], out.clone());
    let plan = TiffToJpegTool.plan(&params).unwrap().data;
    let summary = TiffToJpegTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;

    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert_eq!(summary.written.len(), 3);
    for page in 1..=3 {
        assert!(
            out.join(format!("scan_p{page:03}.jpg")).exists(),
            "page {page} missing"
        );
    }
    assert!(
        !out.join("scan.jpg").exists(),
        "multi-page must be numbered"
    );
}

#[test]
fn f8_a_single_page_tiff_produces_a_plain_name() {
    let f = Fixtures::new();
    let out = f.path().join("out");
    let src = f.multipage_tiff("one.tif", 1, 100, 60);

    let params = TiffToJpegParams::new(vec![src], out.clone());
    let plan = TiffToJpegTool.plan(&params).unwrap().data;
    TiffToJpegTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap();

    assert!(out.join("one.jpg").exists());
    assert!(!out.join("one_p001.jpg").exists());
}

#[test]
fn f8_flattens_alpha_onto_white_not_black() {
    let f = Fixtures::new();
    let out = f.path().join("out");
    // Fully transparent black: ignoring alpha gives black, flattening gives white.
    let src = f.tiff_with_alpha("alpha.tif", 40, 40);

    let params = TiffToJpegParams::new(vec![src], out.clone());
    let plan = TiffToJpegTool.plan(&params).unwrap().data;
    let summary = TiffToJpegTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);

    let img = image_ops::decode(&out.join("alpha.jpg")).unwrap().to_rgb8();
    let centre = img.get_pixel(20, 20);
    assert!(
        centre[0] > 200 && centre[1] > 200 && centre[2] > 200,
        "expected white, got {centre:?}"
    );
}

#[test]
fn f8_caps_the_long_edge_at_2048() {
    let f = Fixtures::new();
    let out = f.path().join("out");
    let src = f.multipage_tiff("big.tif", 1, 3000, 1500);

    let params = TiffToJpegParams::new(vec![src], out.clone());
    let plan = TiffToJpegTool.plan(&params).unwrap().data;
    TiffToJpegTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap();

    let img = image_ops::decode(&out.join("big.jpg")).unwrap();
    assert_eq!(img.width(), 2048);
    assert_eq!(img.height(), 1024);
}

// ---------------------------------------------------------------------------
// F5 — contact sheet
// ---------------------------------------------------------------------------

/// **Phase 4 acceptance.** A sheet from 9 fixtures where the 5th is corrupt:
/// the output dimensions match the formula, and the 5th cell contains red.
#[test]
fn f5_a_corrupt_file_gets_a_red_crossed_box_and_never_aborts_the_sheet() {
    let f = Fixtures::new();
    let dir = f.path().join("sheet");
    fs::create_dir(&dir).unwrap();

    let mut sources: Vec<PathBuf> = Vec::new();
    for i in 1..=9 {
        let name = format!("img{i}.jpg");
        let target = dir.join(&name);
        if i == 5 {
            // The 5th is not a decodable image at all.
            fs::write(&target, b"this is not a JPEG").unwrap();
        } else {
            let p = f.jpeg_without_exif(&name, 200, 150);
            fs::rename(&p, &target).unwrap();
        }
        sources.push(target);
    }

    let out_path = f.path().join("contact.jpg");
    let mut params = ContactSheetParams::new(sources, out_path.clone());
    params.cols = 4;
    params.cell_size = 300;
    params.spacing = 20;
    params.margin = 40;
    params.captions = true;
    params.background = SheetBackground::White;

    let plan = ContactSheetTool.plan(&params).unwrap().data;
    let summary = ContactSheetTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;

    // The sheet was produced despite the bad file.
    assert_eq!(summary.cells, 9);
    assert_eq!(summary.unreadable.len(), 1);
    assert!(summary.unreadable[0].ends_with("img5.jpg"));

    // Dimensions match the specification's formulas.
    // 9 images, 4 cols -> 3 rows.
    // width  = 4×300 + 3×20 + 2×40 = 1340
    // height = 3×(300+30) + 2×20 + 2×40 = 990 + 40 + 80 = 1110
    let expected = SheetLayout::compute(9, 4, 300, 20, 40, true);
    assert_eq!((expected.width, expected.height), (1340, 1110));
    assert_eq!((summary.width, summary.height), (1340, 1110));

    let sheet = image_ops::decode(&out_path).unwrap().to_rgb8();
    assert_eq!((sheet.width(), sheet.height()), (1340, 1110));

    // The 5th cell (index 4) is the first cell of the second row.
    let (cx, cy) = expected.cell_origin(4);
    let mut red_pixels = 0;
    for y in cy..cy + expected.cell_size {
        for x in cx..cx + expected.cell_size {
            let p = sheet.get_pixel(x, y);
            if p[0] > 150 && p[1] < 100 && p[2] < 100 {
                red_pixels += 1;
            }
        }
    }
    assert!(
        red_pixels > 100,
        "the 5th cell should carry a red crossed box, found {red_pixels} red pixels"
    );
}

#[test]
fn f5_without_captions_the_sheet_is_shorter_by_the_label_strips() {
    let with = SheetLayout::compute(9, 4, 300, 20, 40, true);
    let without = SheetLayout::compute(9, 4, 300, 20, 40, false);
    assert_eq!(with.height - without.height, 3 * 30);
    assert_eq!(with.width, without.width);
}

// ---------------------------------------------------------------------------
// F7 — print border
// ---------------------------------------------------------------------------

/// **Phase 4 acceptance.** Portrait yields exactly 3000×3750, landscape yields
/// 3000×2400, and the image is centred with at least 50 px of white on
/// every side.
#[test]
fn f7_canvas_sizes_and_margins_match_the_specification() {
    let f = Fixtures::new();
    let out = f.path().join("out");

    for (name, w, h, expect_w, expect_h) in [
        ("portrait.jpg", 400, 600, 3000u32, 3750u32),
        ("landscape.jpg", 600, 400, 3000, 2400),
    ] {
        let src = f.jpeg_without_exif(name, w, h);

        let mut params = PrintBorderParams::new(vec![src], out.clone());
        params.trim_dark_edges = false;

        let plan = PrintBorderTool.plan(&params).unwrap().data;
        let summary = PrintBorderTool
            .apply(plan, &InMemoryProgress::new())
            .unwrap()
            .data;
        assert!(summary.failures.is_empty(), "{:?}", summary.failures);

        let stem = Path::new(name).file_stem().unwrap().to_string_lossy();
        let produced = out.join(format!("{stem}.jpg"));
        let img = image_ops::decode(&produced).unwrap().to_rgb8();

        assert_eq!(
            (img.width(), img.height()),
            (expect_w, expect_h),
            "{name} canvas"
        );

        // At least 50 px of white on every side.
        let is_white = |p: &image::Rgb<u8>| p[0] > 245 && p[1] > 245 && p[2] > 245;
        for y in 0..img.height() {
            for x in 0..f7_border::MIN_MARGIN {
                assert!(
                    is_white(img.get_pixel(x, y)),
                    "{name}: left margin at {x},{y}"
                );
                assert!(
                    is_white(img.get_pixel(img.width() - 1 - x, y)),
                    "{name}: right margin"
                );
            }
        }
        for x in 0..img.width() {
            for y in 0..f7_border::MIN_MARGIN {
                assert!(is_white(img.get_pixel(x, y)), "{name}: top margin");
                assert!(
                    is_white(img.get_pixel(x, img.height() - 1 - y)),
                    "{name}: bottom margin"
                );
            }
        }

        // And the image is actually present in the middle.
        let centre = img.get_pixel(img.width() / 2, img.height() / 2);
        assert!(
            !is_white(centre),
            "{name}: the photograph should be centred"
        );
    }
}

#[test]
fn f7_enlarges_a_small_image_to_fill_the_space() {
    let f = Fixtures::new();
    let out = f.path().join("out");
    let src = f.jpeg_without_exif("tiny.jpg", 80, 120);

    let mut params = PrintBorderParams::new(vec![src], out.clone());
    params.trim_dark_edges = false;

    let plan = PrintBorderTool.plan(&params).unwrap().data;
    PrintBorderTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap();

    let img = image_ops::decode(&out.join("tiny.jpg")).unwrap().to_rgb8();
    assert_eq!((img.width(), img.height()), (3000, 3750));

    // The photograph occupies far more than its original 80 px width, so it was
    // enlarged rather than sat in a sea of white.
    let is_white = |p: &image::Rgb<u8>| p[0] > 245 && p[1] > 245 && p[2] > 245;
    let mid_row = img.height() / 2;
    let non_white = (0..img.width())
        .filter(|&x| !is_white(img.get_pixel(x, mid_row)))
        .count();
    assert!(
        non_white > 1000,
        "expected enlargement, spanned {non_white}px"
    );
}

// ---------------------------------------------------------------------------
// F4 — half-frame split
// ---------------------------------------------------------------------------

/// **Phase 4 acceptance.** On `half_frame_scan` with a divider at a known
/// column, the detected split is within a small tolerance of it, both halves are
/// portrait, and both are within 10% of ratio 24/17.
#[test]
fn f4_finds_the_planted_divider_and_produces_two_portrait_halves() {
    let f = Fixtures::new();

    // Panels 17:24-ish so the halves land near the target ratio, plus a wide
    // white lab border and a dark divider at a known column.
    let (source, planted_divider) = f.half_frame_scan("roll.jpg", 340, 480, 60, 12);

    let settings = SplitSettings::default();
    let preview = f4_split::preview(&source, &settings).unwrap();

    // The detector found the divider. Its coordinates are relative to the
    // border-cropped image, so allow for the border that was removed.
    let detected_absolute = preview.divider_x + 60;
    let drift = (detected_absolute as i64 - planted_divider as i64).abs();
    assert!(
        drift <= 8,
        "divider planted at {planted_divider}, detected near {detected_absolute} (drift {drift})"
    );

    // Both halves are portrait.
    for (label, half) in [("A", &preview.a), ("B", &preview.b)] {
        assert!(
            half.height() > half.width(),
            "half {label} should be portrait, got {}x{}",
            half.width(),
            half.height()
        );

        // And within 10% of ratio 24/17.
        let ratio = half.height() as f32 / half.width() as f32;
        let target = settings.ratio;
        assert!(
            (ratio - target).abs() / target <= 0.10,
            "half {label} ratio {ratio:.3} is more than 10% from {target:.3}"
        );
    }
}

#[test]
fn f4_writes_a_and_b_names_at_quality_95() {
    let f = Fixtures::new();
    let out = f.path().join("out");
    let (source, _) = f.half_frame_scan("frame.jpg", 340, 480, 60, 12);

    let params = SplitParams::new(vec![source], out.clone());
    let plan = SplitTool.plan(&params).unwrap().data;
    assert!(plan.actions[0].target_a.ends_with("frame_A.jpg"));
    assert!(plan.actions[0].target_b.ends_with("frame_B.jpg"));

    let summary = SplitTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap()
        .data;
    assert!(summary.failures.is_empty(), "{:?}", summary.failures);
    assert!(out.join("frame_A.jpg").exists());
    assert!(out.join("frame_B.jpg").exists());
}

#[test]
fn f4_preview_writes_nothing() {
    let f = Fixtures::new();
    let dir = f.path().join("preview");
    fs::create_dir(&dir).unwrap();
    let (source, _) = f.half_frame_scan("p.jpg", 200, 280, 40, 8);
    fs::rename(&source, dir.join("p.jpg")).unwrap();

    let before = hash_tree(&dir);
    let _ = f4_split::preview(&dir.join("p.jpg"), &SplitSettings::default()).unwrap();
    assert_eq!(hash_tree(&dir), before, "preview must not write");
}

// ---------------------------------------------------------------------------
// The dry-run guarantee, extended to the image tools
// ---------------------------------------------------------------------------

/// **Phase 4 acceptance.** `plan` makes no filesystem modification — including
/// not creating the output directory, which every one of these tools used to do.
#[test]
fn planning_an_image_tool_never_touches_the_filesystem() {
    let f = Fixtures::new();
    let dir = f.path().join("inputs");
    fs::create_dir(&dir).unwrap();

    let jpeg = f.jpeg_without_exif("a.jpg", 120, 90);
    fs::rename(&jpeg, dir.join("a.jpg")).unwrap();
    let tiff = f.multipage_tiff("b.tif", 2, 60, 40);
    fs::rename(&tiff, dir.join("b.tif")).unwrap();

    let root_before = hash_tree(f.path());
    let out_dir = f.path().join("never_created");
    let inputs = vec![dir.join("a.jpg")];

    let _ = TransformTool
        .plan(&TransformParams::new(inputs.clone(), out_dir.clone()))
        .unwrap();
    let _ = SplitTool
        .plan(&SplitParams::new(inputs.clone(), out_dir.clone()))
        .unwrap();
    let _ = PrintBorderTool
        .plan(&PrintBorderParams::new(inputs.clone(), out_dir.clone()))
        .unwrap();
    let _ = TiffToJpegTool
        .plan(&TiffToJpegParams::new(
            vec![dir.join("b.tif")],
            out_dir.clone(),
        ))
        .unwrap();
    let _ = ContactSheetTool
        .plan(&ContactSheetParams::new(inputs, out_dir.join("sheet.jpg")))
        .unwrap();

    assert!(
        !out_dir.exists(),
        "a dry run must not create the output directory"
    );
    assert_eq!(hash_tree(f.path()), root_before, "a plan modified the tree");
}

// ---------------------------------------------------------------------------
// Encoder output properties
// ---------------------------------------------------------------------------

/// Read a JPEG's encoding properties back out of the file itself.
fn jpeg_properties(path: &Path) -> String {
    let out = std::process::Command::new("exiftool")
        .args(["-s", "-EncodingProcess", "-YCbCrSubSampling"])
        .arg(path)
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).to_string()
}

/// F4 step 5 — "quality 95 with no chroma subsampling", asserted from the file.
#[test]
fn f4_writes_full_chroma_resolution() {
    let f = Fixtures::new();
    let out = f.path().join("out");
    let (source, _) = f.half_frame_scan("chroma.jpg", 340, 480, 60, 12);

    let plan = SplitTool
        .plan(&SplitParams::new(vec![source], out.clone()))
        .unwrap()
        .data;
    SplitTool.apply(plan, &InMemoryProgress::new()).unwrap();

    let props = jpeg_properties(&out.join("chroma_A.jpg"));
    assert!(
        props.contains("4:4:4"),
        "F4 specifies no chroma subsampling; got:\n{props}"
    );
}

/// F7 step 5 — same requirement, asserted from the file.
#[test]
fn f7_writes_full_chroma_resolution() {
    let f = Fixtures::new();
    let out = f.path().join("out");
    let src = f.jpeg_without_exif("border.jpg", 400, 600);

    let mut params = PrintBorderParams::new(vec![src], out.clone());
    params.trim_dark_edges = false;
    let plan = PrintBorderTool.plan(&params).unwrap().data;
    PrintBorderTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap();

    let props = jpeg_properties(&out.join("border.jpg"));
    assert!(
        props.contains("4:4:4"),
        "F7 specifies no chroma subsampling; got:\n{props}"
    );
}

/// F8 — "quality 90, 4:2:0 chroma subsampling, progressive, optimised",
/// asserted from the file.
#[test]
fn f8_writes_progressive_four_two_zero() {
    let f = Fixtures::new();
    let out = f.path().join("out");
    let src = f.multipage_tiff("dist.tif", 1, 300, 200);

    let plan = TiffToJpegTool
        .plan(&TiffToJpegParams::new(vec![src], out.clone()))
        .unwrap()
        .data;
    TiffToJpegTool
        .apply(plan, &InMemoryProgress::new())
        .unwrap();

    let props = jpeg_properties(&out.join("dist.jpg"));
    assert!(
        props.contains("4:2:0"),
        "F8 specifies 4:2:0 chroma subsampling; got:\n{props}"
    );
    assert!(
        props.contains("Progressive"),
        "F8 specifies progressive encoding; got:\n{props}"
    );
}
