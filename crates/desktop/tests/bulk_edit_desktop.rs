//! Desktop command tests for ED-17 (presets) and ED-18 (batch recipes).

use phototools_core::config::{Config, Thresholds};
use phototools_core::jobs::NoEvents;
use phototools_core::ledger::Ledger;
use phototools_core::media::edit::AdjustmentRecipe;
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

fn make_state(f: &Fixture) -> phototools_desktop::AppState {
    let ledger = Ledger::open(&f.config.database).unwrap();
    phototools_desktop::AppState::new(f.config.clone(), ledger, Arc::new(NoEvents))
}

// ---------------------------------------------------------------------------
// ED-17: Presets
// ---------------------------------------------------------------------------

#[test]
fn preset_saved_listed_loaded_and_deleted_round_trip() {
    let f = fixture();
    let state = make_state(&f);

    let recipe = AdjustmentRecipe {
        exposure: 0.75,
        contrast: 0.25,
        ..Default::default()
    };

    // Save
    phototools_desktop::commands::presets::save_preset_impl(
        &state,
        "WarmFilm".into(),
        recipe.clone(),
        false,
    )
    .unwrap();

    // List
    let list = phototools_desktop::commands::presets::list_presets_impl(&state).unwrap();
    assert_eq!(list.presets.len(), 1);
    assert_eq!(list.presets[0].name, "WarmFilm");

    // Load
    let loaded =
        phototools_desktop::commands::presets::load_preset_impl(&state, "WarmFilm".into()).unwrap();
    assert_eq!(loaded.exposure, 0.75);
    assert_eq!(loaded.contrast, 0.25);

    // Delete
    phototools_desktop::commands::presets::delete_preset_impl(&state, "WarmFilm".into()).unwrap();

    let after = phototools_desktop::commands::presets::list_presets_impl(&state).unwrap();
    assert_eq!(after.presets.len(), 0);
}

// ---------------------------------------------------------------------------
// ED-18: Batch edit
// ---------------------------------------------------------------------------

#[test]
fn batch_planned_from_a_preset_name() {
    let f = fixture();
    let state = make_state(&f);

    // Create a JPEG in the library
    let img = f.root.join("photo.jpg");
    image::RgbImage::new(40, 40).save(&img).unwrap();

    // Save a preset
    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..Default::default()
    };
    phototools_desktop::commands::presets::save_preset_impl(
        &state,
        "Brighten".into(),
        recipe.clone(),
        false,
    )
    .unwrap();

    let out_dir = f.root.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    // Plan from preset name
    let source = phototools_desktop::commands::bulk_edit::RecipeSource::Preset {
        name: "Brighten".into(),
    };
    let plan = phototools_desktop::commands::bulk_edit::plan_bulk_edit_impl(
        &state,
        vec![img.to_string_lossy().to_string()],
        source,
        out_dir.to_string_lossy().to_string(),
        false,
    )
    .unwrap();

    assert_eq!(plan.actions_count, 1);
    assert!(!plan.recipe_sha256.is_empty());
}

#[test]
fn batch_apply_with_stale_reviewed_hash_is_refused() {
    let f = fixture();
    let state = make_state(&f);

    let img = f.root.join("photo.jpg");
    image::RgbImage::new(40, 40).save(&img).unwrap();

    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..Default::default()
    };
    phototools_desktop::commands::presets::save_preset_impl(
        &state,
        "Brighten".into(),
        recipe.clone(),
        false,
    )
    .unwrap();

    let out_dir = f.root.join("out");
    std::fs::create_dir_all(&out_dir).unwrap();

    // Plan
    let source = phototools_desktop::commands::bulk_edit::RecipeSource::Preset {
        name: "Brighten".into(),
    };
    let plan = phototools_desktop::commands::bulk_edit::plan_bulk_edit_impl(
        &state,
        vec![img.to_string_lossy().to_string()],
        source.clone(),
        out_dir.to_string_lossy().to_string(),
        false,
    )
    .unwrap();
    let reviewed_hash = plan.recipe_sha256;

    // Overwrite the preset with a different recipe (changes the hash)
    let changed = AdjustmentRecipe {
        exposure: 0.9,
        ..Default::default()
    };
    phototools_desktop::commands::presets::save_preset_impl(
        &state,
        "Brighten".into(),
        changed,
        true,
    )
    .unwrap();

    // Apply with the old reviewed hash: refused with an error, so no job id exists to follow.
    let err = phototools_desktop::commands::bulk_edit::apply_bulk_edit_impl(
        &state,
        vec![img.to_string_lossy().to_string()],
        source,
        out_dir.to_string_lossy().to_string(),
        reviewed_hash,
        false,
    )
    .unwrap_err();
    assert!(
        err.contains("recipe changed since the dry run"),
        "expected stale-hash refusal, got: {err}"
    );
    assert_eq!(std::fs::read_dir(&out_dir).unwrap().count(), 0);
}

#[test]
fn output_directory_outside_roots_is_refused() {
    let f = fixture();
    let state = make_state(&f);

    let img = f.root.join("photo.jpg");
    image::RgbImage::new(40, 40).save(&img).unwrap();

    let recipe = AdjustmentRecipe {
        exposure: 0.5,
        ..Default::default()
    };
    let source = phototools_desktop::commands::bulk_edit::RecipeSource::Recipe {
        recipe: Box::new(recipe),
    };

    let outside = f._temp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();

    let err = phototools_desktop::commands::bulk_edit::plan_bulk_edit_impl(
        &state,
        vec![img.to_string_lossy().to_string()],
        source,
        outside.to_string_lossy().to_string(),
        false,
    )
    .unwrap_err();

    assert!(
        err.contains("outside") || err.contains("publishing folder"),
        "expected G6 refusal for outside path, got: {err}"
    );
}
