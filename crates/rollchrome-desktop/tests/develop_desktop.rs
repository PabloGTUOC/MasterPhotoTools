//! Desktop command tests for Develop (DV-2b).

#[allow(dead_code)]
#[path = "../../rollchrome-core/tests/support/dng.rs"]
mod dng;

use phototools_core::config::{Config, Thresholds};
use phototools_core::jobs::NoEvents;
use phototools_core::ledger::Ledger;
use rollchrome_core::media::develop::OutputFormat;
use rollchrome_core::tools::develop::DevelopItem;
use rollchrome_desktop::commands::develop::plan_develop_impl;
use std::path::PathBuf;
use std::sync::Arc;

struct Fixture {
    _temp: tempfile::TempDir,
    config: Config,
    root: PathBuf,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("library");
    std::fs::create_dir(&root).unwrap();

    let config = Config {
        roots: vec![root.canonicalize().unwrap()],
        staging_dir: temp.path().join("staging"),
        publishing_dir: None,
        thresholds: Thresholds::default(),
        database: temp.path().join("ledger.sqlite3"),
    };
    let root = root.canonicalize().unwrap();
    Fixture {
        _temp: temp,
        config,
        root,
    }
}

fn make_state(f: &Fixture) -> rollchrome_desktop::AppState {
    let ledger = Ledger::open(&f.config.database).unwrap();
    rollchrome_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents))
}

#[test]
fn plan_develop_refuses_a_folder_outside_the_roots() {
    let f = fixture();
    let state = make_state(&f);

    let outside = f._temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();

    let items = vec![DevelopItem {
        name: "DSCF0001.dng".to_string(),
        look: Some("Identity.cube".to_string()),
        exposure_ev: 0.0,
    }];

    let err = plan_develop_impl(
        &state,
        outside.to_string_lossy().to_string(),
        items,
        OutputFormat::Jpeg { quality: 90 },
    )
    .unwrap_err();

    assert!(
        err.contains("outside the folders this application may touch"),
        "expected G6 refusal, got: {err}"
    );
}

#[test]
fn plan_develop_plans_a_folder_inside_the_roots() {
    let f = fixture();
    let state = make_state(&f);

    // A temp root with two handmade DNGs and a text file: two actions, one skip
    let pixels = vec![1000u16; 16 * 16];
    let dng1 = dng::DngBuilder::new_bayer(16, 16).build(&pixels);
    let dng2 = dng::DngBuilder::new_bayer(16, 16).build(&pixels);
    std::fs::write(f.root.join("DSCF0001.dng"), dng1).unwrap();
    std::fs::write(f.root.join("DSCF0002.dng"), dng2).unwrap();
    std::fs::write(f.root.join("notes.txt"), b"some notes").unwrap();

    let items = vec![
        DevelopItem {
            name: "DSCF0001.dng".to_string(),
            look: Some("Identity.cube".to_string()),
            exposure_ev: 0.0,
        },
        DevelopItem {
            name: "DSCF0002.dng".to_string(),
            look: Some("Identity.cube".to_string()),
            exposure_ev: 0.5,
        },
        DevelopItem {
            name: "notes.txt".to_string(),
            look: Some("Identity.cube".to_string()),
            exposure_ev: 0.0,
        },
    ];

    let plan = plan_develop_impl(
        &state,
        f.root.to_string_lossy().to_string(),
        items,
        OutputFormat::Jpeg { quality: 90 },
    )
    .expect("plan develop");

    assert_eq!(plan.actions.len(), 2);
    assert_eq!(plan.skipped.len(), 1);

    assert_eq!(plan.actions[0].source, "DSCF0001.dng");
    assert_eq!(plan.actions[0].output, "DSCF0001.jpg");
    assert_eq!(plan.actions[1].source, "DSCF0002.dng");
    assert_eq!(plan.actions[1].output, "DSCF0002.jpg");

    assert_eq!(plan.skipped[0].file, "notes.txt");
    assert_eq!(plan.skipped[0].reason, "not a RAW file Develop can read");
}

#[test]
fn preview_refuses_a_name_that_leaves_the_folder() {
    let f = fixture();
    let state = make_state(&f);
    let folder = f.root.to_string_lossy().to_string();

    let err1 = rollchrome_desktop::commands::develop::preview_develop_impl(
        &state,
        folder.clone(),
        "../escape.dng".to_string(),
        None,
        0.0,
        false,
    )
    .unwrap_err();
    assert!(err1.contains("path separators or '..'"));

    let err2 = rollchrome_desktop::commands::develop::preview_develop_impl(
        &state,
        folder.clone(),
        "..".to_string(),
        None,
        0.0,
        false,
    )
    .unwrap_err();
    assert!(err2.contains("path separators or '..'"));

    let err3 = rollchrome_desktop::commands::develop::preview_develop_impl(
        &state,
        folder,
        "sub/file.dng".to_string(),
        None,
        0.0,
        false,
    )
    .unwrap_err();
    assert!(err3.contains("path separators or '..'"));
}

#[test]
fn preview_refuses_a_folder_outside_the_roots() {
    let f = fixture();
    let state = make_state(&f);
    let outside = f._temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();

    let err = rollchrome_desktop::commands::develop::preview_develop_impl(
        &state,
        outside.to_string_lossy().to_string(),
        "sample.dng".to_string(),
        None,
        0.0,
        false,
    )
    .unwrap_err();

    assert!(err.contains("outside the folders this application may touch"));
}

#[test]
fn preview_develop_returns_a_jpeg_for_a_synthetic_raw() {
    let f = fixture();
    let state = make_state(&f);

    let pixels = vec![2500u16; 64 * 48];
    let dng = dng::DngBuilder::new_bayer(64, 48).build(&pixels);
    std::fs::write(f.root.join("sample.dng"), dng).unwrap();

    let jpeg_bytes = rollchrome_desktop::commands::develop::preview_develop_impl(
        &state,
        f.root.to_string_lossy().to_string(),
        "sample.dng".to_string(),
        Some("builtin:fujifilm-classic-chrome".to_string()),
        0.0,
        false,
    )
    .expect("preview develop");

    assert!(
        jpeg_bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        "output must be a valid JPEG file"
    );
}

#[test]
fn a_second_look_on_the_same_photo_does_not_develop_it_again() {
    let f = fixture();
    let state = make_state(&f);

    let pixels = vec![3000u16; 64 * 48];
    let dng = dng::DngBuilder::new_bayer(64, 48).build(&pixels);
    std::fs::write(f.root.join("photo.dng"), dng).unwrap();

    assert_eq!(state.develop_preview_runs(), 0);

    // First look: develops the RAW
    let _ = rollchrome_desktop::commands::develop::preview_develop_impl(
        &state,
        f.root.to_string_lossy().to_string(),
        "photo.dng".to_string(),
        Some("builtin:fujifilm-classic-chrome".to_string()),
        0.0,
        false,
    )
    .expect("first look");

    assert_eq!(state.develop_preview_runs(), 1);

    // Second look: re-uses the cached DevelopPreview, developing count stays 1
    let _ = rollchrome_desktop::commands::develop::preview_develop_impl(
        &state,
        f.root.to_string_lossy().to_string(),
        "photo.dng".to_string(),
        Some("builtin:ricoh-standard".to_string()),
        0.0,
        false,
    )
    .expect("second look");

    assert_eq!(state.develop_preview_runs(), 1);
}

#[test]
fn changing_exposure_does_not_develop_the_photo_again() {
    let f = fixture();
    let state = make_state(&f);

    let pixels = vec![3000u16; 64 * 48];
    let dng = dng::DngBuilder::new_bayer(64, 48).build(&pixels);
    std::fs::write(f.root.join("photo.dng"), dng).unwrap();

    assert_eq!(state.develop_preview_runs(), 0);

    let _ = rollchrome_desktop::commands::develop::preview_develop_impl(
        &state,
        f.root.to_string_lossy().to_string(),
        "photo.dng".to_string(),
        None,
        0.0,
        false,
    )
    .expect("first render");

    assert_eq!(state.develop_preview_runs(), 1);

    let _ = rollchrome_desktop::commands::develop::preview_develop_impl(
        &state,
        f.root.to_string_lossy().to_string(),
        "photo.dng".to_string(),
        None,
        1.0,
        false,
    )
    .expect("second render with new exposure");

    assert_eq!(state.develop_preview_runs(), 1);
}

#[test]
fn going_back_to_a_recent_photo_does_not_develop_it_again() {
    let f = fixture();
    let state = make_state(&f);

    let pixels = vec![3000u16; 64 * 48];
    let dng1 = dng::DngBuilder::new_bayer(64, 48).build(&pixels);
    let dng2 = dng::DngBuilder::new_bayer(64, 48).build(&pixels);
    std::fs::write(f.root.join("photo1.dng"), dng1).unwrap();
    std::fs::write(f.root.join("photo2.dng"), dng2).unwrap();

    assert_eq!(state.develop_preview_runs(), 0);

    // Develop photo 1
    let _ = rollchrome_desktop::commands::develop::preview_develop_impl(
        &state,
        f.root.to_string_lossy().to_string(),
        "photo1.dng".to_string(),
        None,
        0.0,
        false,
    )
    .expect("photo 1");
    assert_eq!(state.develop_preview_runs(), 1);

    // Develop photo 2
    let _ = rollchrome_desktop::commands::develop::preview_develop_impl(
        &state,
        f.root.to_string_lossy().to_string(),
        "photo2.dng".to_string(),
        None,
        0.0,
        false,
    )
    .expect("photo 2");
    assert_eq!(state.develop_preview_runs(), 2);

    // Go back to photo 1
    let _ = rollchrome_desktop::commands::develop::preview_develop_impl(
        &state,
        f.root.to_string_lossy().to_string(),
        "photo1.dng".to_string(),
        None,
        0.0,
        false,
    )
    .expect("photo 1 again");
    assert_eq!(state.develop_preview_runs(), 2);
}
