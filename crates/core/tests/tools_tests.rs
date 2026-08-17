use chrono::NaiveDateTime;
use phototools_core::jobs::InMemoryProgress;
use phototools_core::tools::f1_dates::{DateRepairParams, DateRepairTool, RepairMode};
use phototools_core::tools::f2_takeout::find_takeout_date;
use phototools_core::tools::f3_rename::{BatchRenameParams, BatchRenamerTool, RenameOrder};
use phototools_core::tools::Tool;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_f3_rename_plan_dry_run_and_collisions() {
    let dir = tempdir().unwrap();
    let file1 = dir.path().join("a.jpg");
    let file2 = dir.path().join("b.jpg");
    let file3 = dir.path().join("c.jpg");
    let collision_file = dir.path().join("20240101-Trip-01.jpg");

    fs::write(&file1, "1").unwrap();
    fs::write(&file2, "2").unwrap();
    fs::write(&file3, "3").unwrap();
    fs::write(&collision_file, "4").unwrap();

    let tool = BatchRenamerTool;
    let params = BatchRenameParams {
        paths: vec![file1.clone(), file2.clone(), file3.clone()],
        date: Some("20240101".to_string()),
        subject: Some("Trip".to_string()),
        camera: None,
        film: None,
        order: RenameOrder::Capture,
    };

    let plan = tool.plan(&params).unwrap().data;

    // We expect a.jpg to be skipped because 20240101-Trip-01.jpg already exists.
    println!("PLAN ACTIONS: {:#?}", plan.actions);
    println!("PLAN SKIPPED: {:#?}", plan.skipped);
    assert_eq!(plan.skipped.len(), 1);
    assert_eq!(plan.actions.len(), 2);

    let progress = InMemoryProgress::new();
    let _ = tool.apply(plan, &progress).unwrap();

    // The collision file should still exist and not be overwritten
    assert!(collision_file.exists());
    assert_eq!(fs::read_to_string(&collision_file).unwrap(), "4");

    // The successfully renamed files
    assert!(!file2.exists());
    assert!(dir.path().join("20240101-Trip-02.jpg").exists());
}

#[test]
fn test_f2_takeout_sidecar_parsing() {
    let dir = tempdir().unwrap();
    let img = dir.path().join("photo(1).jpg");
    let sidecar = dir.path().join("photo.jpg(1).json");

    fs::write(&img, "img").unwrap();
    let json_content = r#"{
        "photoTakenTime": {
            "timestamp": "1704067200"
        }
    }"#;
    fs::write(&sidecar, json_content).unwrap();

    let dt = find_takeout_date(&img).expect("Should find sidecar and parse date");
    assert_eq!(
        dt,
        NaiveDateTime::parse_from_str("2024-01-01 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
    );
}

#[test]
fn test_f1_scan_and_repair_plan_only() {
    let dir = tempdir().unwrap();
    let img = dir.path().join("test.jpg");
    fs::write(&img, "dummy").unwrap();

    let tool = DateRepairTool;
    let params = DateRepairParams {
        paths: vec![img.clone()],
        mode: RepairMode::Manual(
            NaiveDateTime::parse_from_str("2024-01-01 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
        ),
    };

    let plan = tool.plan(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].path, img);

    // Dry run guarantee: file should remain identical
    assert_eq!(fs::read_to_string(&img).unwrap(), "dummy");
}

#[test]
fn test_f6_transform_plan() {
    use image::ImageFormat;
    use phototools_core::tools::f6_transform::{TransformParams, TransformTool};

    let dir = tempdir().unwrap();
    let img = dir.path().join("test.jpg");
    fs::write(&img, "dummy").unwrap();

    let tool = TransformTool;
    let params = TransformParams {
        paths: vec![img.clone()],
        rotate_deg: Some(90.0),
        max_long_edge: Some(1024),
        format: Some(ImageFormat::WebP),
        quality: 80,
        out_dir: dir.path().join("out"),
    };

    let plan = tool.plan(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].target, dir.path().join("out/test.webp"));
}

#[test]
fn test_f8_tiff_to_jpeg_plan() {
    use phototools_core::tools::f8_tiff::{TiffToJpegParams, TiffToJpegTool};

    let dir = tempdir().unwrap();
    let img = dir.path().join("scan.tiff");
    fs::write(&img, "dummy").unwrap();

    let tool = TiffToJpegTool;
    let params = TiffToJpegParams {
        paths: vec![img.clone()],
        max_long_edge: 2048,
        quality: 90,
        out_dir: dir.path().join("out"),
    };

    let plan = tool.plan(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].target_base, dir.path().join("out/scan"));
}

#[test]
fn test_f5_contact_sheet_plan() {
    use phototools_core::tools::f5_contact::{ContactSheetParams, ContactSheetTool};

    let dir = tempdir().unwrap();
    let img = dir.path().join("1.jpg");
    fs::write(&img, "dummy").unwrap();

    let tool = ContactSheetTool;
    let params = ContactSheetParams {
        paths: vec![img.clone()],
        cols: 3,
        cell_size: 200,
        spacing: 10,
        margin: 20,
        out_path: dir.path().join("contact.jpg"),
    };

    let plan = tool.plan(&params).unwrap().data;
    assert_eq!(plan.actions.len(), 1);
    assert_eq!(plan.actions[0].paths.len(), 1);
}
