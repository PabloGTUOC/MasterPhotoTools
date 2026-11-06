use phototools_core::config::{Config, Thresholds};
use phototools_core::jobs::NoEvents;
use phototools_core::ledger::Ledger;
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

/// Helper to create a valid 2x2x2 cube LUT on disk.
fn create_test_cube(dir: &std::path::Path) -> PathBuf {
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
    std::fs::write(&p, content).unwrap();
    p
}

#[test]
fn export_edited_image_refuses_destination_inside_publishing_folder() {
    let mut f = fixture();
    let pub_dir = f._temp.path().join("publishing");
    std::fs::create_dir_all(&pub_dir).unwrap();
    f.config.publishing_dir = Some(pub_dir.canonicalize().unwrap());

    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = rollchrome_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img_path = f.root.join("source.jpg");
    image::RgbImage::new(60, 40).save(&img_path).unwrap();

    let recipe = rollchrome_core::media::edit::AdjustmentRecipe::default();
    let err = rollchrome_desktop::commands::edit::export_edited_image_impl(
        &state,
        img_path.to_string_lossy().to_string(),
        recipe,
        pub_dir.to_string_lossy().to_string(),
    )
    .unwrap_err();

    assert!(
        err.contains("publishing folder"),
        "must refuse writing inside Publishing folder (MV-16.7); got: {err}"
    );
}

#[test]
fn bulk_lut_refuses_output_directory_inside_publishing_folder() {
    let mut f = fixture();
    let pub_dir = f._temp.path().join("publishing");
    std::fs::create_dir_all(&pub_dir).unwrap();
    f.config.publishing_dir = Some(pub_dir.canonicalize().unwrap());

    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = rollchrome_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img_path = f.root.join("photo.jpg");
    image::RgbImage::new(60, 40).save(&img_path).unwrap();

    // Import a test LUT into the managed library
    let cube = create_test_cube(&f.root);
    rollchrome_desktop::commands::edit::import_lut_impl(&state, cube.to_string_lossy().to_string())
        .unwrap();

    let err = rollchrome_desktop::commands::edit::plan_bulk_lut_impl(
        &state,
        vec![img_path.to_string_lossy().to_string()],
        "test.cube".into(),
        1.0,
        pub_dir.to_string_lossy().to_string(),
        false,
    )
    .unwrap_err();

    assert!(
        err.contains("publishing folder"),
        "must refuse output inside Publishing folder (MV-16.7); got: {err}"
    );
}

#[test]
fn apply_bulk_lut_refuses_when_the_lut_changed_after_the_reviewed_plan() {
    let f = fixture();
    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = rollchrome_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img_path = f.root.join("source.jpg");
    image::RgbImage::new(60, 40).save(&img_path).unwrap();

    let cube = create_test_cube(&f.root);
    rollchrome_desktop::commands::edit::import_lut_impl(&state, cube.to_string_lossy().to_string())
        .unwrap();

    let out_dir = f.root.join("graded_out");
    std::fs::create_dir_all(&out_dir).unwrap();

    // 1. User plans and reviews the dry run
    let plan = rollchrome_desktop::commands::edit::plan_bulk_lut_impl(
        &state,
        vec![img_path.to_string_lossy().to_string()],
        "test.cube".into(),
        1.0,
        out_dir.to_string_lossy().to_string(),
        false,
    )
    .unwrap();
    let reviewed_lut_sha256 = plan.lut_sha256;

    // 2. The LUT file on disk is modified after review
    let lut_in_lib = f.config.lut_dir().join("test.cube");
    let modified_content = "\
TITLE \"Modified Test Cube\"
LUT_3D_SIZE 2
0.2 0.2 0.2
1.0 0.0 0.0
0.0 1.0 0.0
1.0 1.0 0.0
0.0 0.0 1.0
1.0 0.0 1.0
0.0 1.0 1.0
1.0 1.0 1.0
";
    std::fs::write(&lut_in_lib, modified_content).unwrap();

    // 3. User clicks Run with the reviewed hash
    let err = rollchrome_desktop::commands::edit::apply_bulk_lut_impl(
        &state,
        vec![img_path.to_string_lossy().to_string()],
        "test.cube".into(),
        1.0,
        out_dir.to_string_lossy().to_string(),
        reviewed_lut_sha256,
        false,
    )
    .unwrap_err();

    assert!(
        err.contains("changed on disk since the reviewed plan"),
        "must refuse execution when LUT changed after reviewed plan; got: {err}"
    );
}

#[test]
fn render_preview_returns_width_height_and_rgba_of_that_size() {
    let f = fixture();
    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = rollchrome_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img_path = f.root.join("photo.jpg");
    image::RgbImage::new(60, 40).save(&img_path).unwrap();

    let open_res = rollchrome_desktop::commands::edit::open_preview_impl(
        &state,
        img_path.to_string_lossy().to_string(),
    )
    .unwrap();

    assert_eq!(open_res.drag, (60, 40));
    assert_eq!(open_res.settle, (60, 40));
    assert_eq!(open_res.orientation, 1);
    assert!(!open_res.read_only);

    let bytes = rollchrome_desktop::commands::edit::render_preview_impl(
        &state,
        open_res.session_id,
        rollchrome_core::media::edit::AdjustmentRecipe::default(),
        Some(rollchrome_core::media::edit::preview::PreviewStage::Drag),
    )
    .unwrap();

    let width = u32::from_be_bytes(bytes[0..4].try_into().unwrap());
    let height = u32::from_be_bytes(bytes[4..8].try_into().unwrap());
    let orientation = u32::from_be_bytes(bytes[8..12].try_into().unwrap());
    assert_eq!(width, 60);
    assert_eq!(height, 40);
    assert_eq!(orientation, 1);
    assert_eq!(
        bytes.len(),
        rollchrome_core::media::edit::PREVIEW_FRAME_HEADER_LEN + (60 * 40 * 4),
        "payload must contain the header and exactly width*height*4 RGBA bytes"
    );
    // A black frame: every pixel is counted, and every one is shadow-clipped.
    let pixels = u32::from_be_bytes(bytes[12..16].try_into().unwrap());
    let shadow_clipped = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    assert_eq!(pixels, 60 * 40);
    assert_eq!(shadow_clipped, 60 * 40);
}

/// The overlay's coverage comes from the open session, frame-sized, and names a missing
/// mask rather than returning nothing (ED-20, G10).
#[test]
fn mask_coverage_is_served_for_the_open_preview() {
    use rollchrome_core::media::edit::{AdjustmentRecipe, LocalAdjustments, Mask, MaskKind};

    let f = fixture();
    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = rollchrome_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img_path = f.root.join("photo.jpg");
    image::RgbImage::new(60, 40).save(&img_path).unwrap();
    let session = rollchrome_desktop::commands::edit::open_preview_impl(
        &state,
        img_path.to_string_lossy().to_string(),
    )
    .unwrap()
    .session_id;

    let recipe = AdjustmentRecipe {
        masks: vec![Mask {
            id: "top".into(),
            name: "Top".into(),
            kind: MaskKind::Linear {
                start: [0.5, 0.0],
                end: [0.5, 0.5],
            },
            invert: false,
            opacity: 1.0,
            enabled: true,
            // A new mask with nothing set yet is still shown.
            adjustments: LocalAdjustments::default(),
            strokes: Vec::new(),
        }],
        ..Default::default()
    };

    let bytes = rollchrome_desktop::commands::edit::render_mask_coverage_impl(
        &state,
        session.clone(),
        recipe.clone(),
        "top".into(),
        None,
    )
    .unwrap();
    let w = u32::from_be_bytes(bytes[0..4].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[4..8].try_into().unwrap());
    assert_eq!((w, h), (60, 40));
    assert_eq!(bytes.len(), 8 + 60 * 40);
    let row = |y: usize| bytes[8 + y * 60 + 30];
    assert_eq!(row(0), 255, "full coverage at the start line");
    assert_eq!(row(39), 0, "none beyond the end line");

    let err = rollchrome_desktop::commands::edit::render_mask_coverage_impl(
        &state,
        session,
        recipe,
        "nope".into(),
        None,
    )
    .unwrap_err();
    assert!(err.contains("No mask nope"), "got: {err}");
}

/// Without the models on disk the panel is told so, and asking for a mask says what is
/// missing rather than failing obscurely (ED-23, G10).
#[test]
fn an_automatic_mask_without_its_model_is_refused_by_name() {
    use rollchrome_core::media::edit::AutoTarget;

    let f = fixture();
    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = rollchrome_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let models = rollchrome_desktop::commands::auto_masks::auto_mask_models_impl(&state);
    assert_eq!(models.len(), 2);
    assert!(models.iter().all(|m| !m.present));
    assert!(models
        .iter()
        .any(|m| m.target == AutoTarget::Subject && m.bytes == 224_005_088));

    let img_path = f.root.join("photo.jpg");
    image::RgbImage::new(64, 48).save(&img_path).unwrap();
    let session = rollchrome_desktop::commands::edit::open_preview_impl(
        &state,
        img_path.to_string_lossy().to_string(),
    )
    .unwrap()
    .session_id;

    let err = rollchrome_desktop::commands::auto_masks::make_auto_mask_impl(
        &state,
        &session,
        AutoTarget::Sky,
    )
    .unwrap_err();
    assert!(err.contains("not downloaded"), "got: {err}");

    let err = rollchrome_desktop::commands::auto_masks::make_auto_mask_impl(
        &state,
        "not-a-session",
        AutoTarget::Sky,
    )
    .unwrap_err();
    assert!(err.contains("closed or replaced"), "got: {err}");
}

#[test]
fn opening_a_second_preview_closes_the_first() {
    let f = fixture();
    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = rollchrome_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img1 = f.root.join("first.jpg");
    image::RgbImage::new(40, 40).save(&img1).unwrap();
    let img2 = f.root.join("second.jpg");
    image::RgbImage::new(80, 80).save(&img2).unwrap();

    let session1 = rollchrome_desktop::commands::edit::open_preview_impl(
        &state,
        img1.to_string_lossy().to_string(),
    )
    .unwrap()
    .session_id;

    let session2 = rollchrome_desktop::commands::edit::open_preview_impl(
        &state,
        img2.to_string_lossy().to_string(),
    )
    .unwrap()
    .session_id;

    assert_ne!(session1, session2);

    // Session 1 is now closed and must be refused
    let err = rollchrome_desktop::commands::edit::render_preview_impl(
        &state,
        session1,
        rollchrome_core::media::edit::AdjustmentRecipe::default(),
        None,
    )
    .unwrap_err();
    assert!(
        err.contains("closed or replaced"),
        "session 1 must be closed after opening session 2; got: {err}"
    );

    // Session 2 is active and succeeds
    let res = rollchrome_desktop::commands::edit::render_preview_impl(
        &state,
        session2,
        rollchrome_core::media::edit::AdjustmentRecipe::default(),
        None,
    );
    assert!(res.is_ok(), "session 2 must be active and renderable");
}

#[test]
fn a_saved_recipe_is_loaded_again_when_the_photo_is_reopened() {
    let f = fixture();
    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = rollchrome_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img = f.root.join("reopen.jpg");
    image::RgbImage::new(40, 40).save(&img).unwrap();

    let recipe = rollchrome_core::media::edit::AdjustmentRecipe {
        exposure: 1.5,
        temperature: -10.0,
        contrast: 25.0,
        ..Default::default()
    };

    let path_str = img.to_string_lossy().to_string();
    rollchrome_desktop::commands::edit::save_recipe_impl(&state, path_str.clone(), recipe).unwrap();

    // Reopen and load
    let loaded = rollchrome_desktop::commands::edit::load_recipe_impl(&state, path_str)
        .unwrap()
        .expect("saved recipe must be loaded");
    assert_eq!(loaded.exposure, 1.5);
    assert_eq!(loaded.temperature, -10.0);
    assert_eq!(loaded.contrast, 25.0);
}

#[test]
fn an_unreadable_sidecar_is_reported_not_treated_as_no_edits() {
    let f = fixture();
    let ledger = Ledger::open(&f.config.database).unwrap();
    let state = rollchrome_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents));

    let img = f.root.join("corrupt.jpg");
    image::RgbImage::new(40, 40).save(&img).unwrap();

    // Write invalid JSON to companion sidecar
    let sidecar = f.root.join("corrupt.jpg.photoedit");
    std::fs::write(&sidecar, b"not valid json").unwrap();

    let path_str = img.to_string_lossy().to_string();
    let err = rollchrome_desktop::commands::edit::load_recipe_impl(&state, path_str)
        .expect_err("unreadable sidecar must error, never return None");
    assert!(
        err.contains("JSON") || err.contains("syntax") || err.contains("corrupt"),
        "error must report the corrupted sidecar; got: {err}"
    );
}
