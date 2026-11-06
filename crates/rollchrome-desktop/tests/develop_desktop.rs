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
