//! Preset library (ED-17).
//!
//! A preset is an `AdjustmentRecipe` saved under a name, so it can be applied to other
//! photographs. The name is the file stem; the file holds only the recipe JSON.

use crate::error::Error;
use crate::media::edit::AdjustmentRecipe;
use crate::tools::{load_recipe, CURRENT_RECIPE_VERSION};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

pub const PRESET_EXTENSION: &str = "photopreset";

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetEntry {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetError {
    pub name: String,
    pub error: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetList {
    pub presets: Vec<PresetEntry>,
    pub errors: Vec<PresetError>,
}

/// Validate a preset name.
///
/// Names must be:
/// - Non-empty
/// - No longer than 64 characters
/// - Not contain '/', '\', ':', control characters
/// - Not start with '.'
fn validate_name(name: &str) -> Result<String, Error> {
    let trimmed = name.trim();

    if trimmed.is_empty() {
        return Err(Error::Refused("A preset name cannot be empty.".into()));
    }

    if trimmed.chars().count() > 64 {
        return Err(Error::Refused(
            "A preset name cannot be longer than 64 characters.".into(),
        ));
    }

    if trimmed
        .chars()
        .any(|c| matches!(c, '/' | '\\' | ':' | '\0'..='\x1f'))
    {
        return Err(Error::Refused(
            "A preset name cannot contain '/', '\\', ':', or control characters.".into(),
        ));
    }

    if trimmed.starts_with('.') {
        return Err(Error::Refused(
            "A preset name cannot start with '.'.".into(),
        ));
    }

    Ok(trimmed.to_string())
}

/// List presets in a directory.
pub fn list_presets(dir: &Path) -> Result<PresetList, Error> {
    let mut presets = Vec::new();
    let mut errors = Vec::new();

    if !dir.exists() {
        return Ok(PresetList { presets, errors });
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        // Only process files with the correct extension
        if let Some(ext) = path.extension() {
            if ext == PRESET_EXTENSION {
                let name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_string())
                    .ok_or_else(|| Error::Refused("Invalid file name for preset.".into()))?;

                // Validate the name
                let validated_name = match validate_name(&name) {
                    Ok(n) => n,
                    Err(e) => {
                        errors.push(PresetError {
                            name,
                            error: e.to_string(),
                        });
                        continue;
                    }
                };

                // Try to load the preset file
                match load_recipe(&path) {
                    Ok(_) => {
                        presets.push(PresetEntry {
                            name: validated_name,
                        });
                    }
                    Err(e) => {
                        errors.push(PresetError {
                            name: validated_name,
                            error: e.to_string(),
                        });
                    }
                }
            }
        }
    }

    // Sort presets case-insensitively
    presets.sort_by_key(|p| p.name.to_lowercase());

    Ok(PresetList { presets, errors })
}

/// Load a preset from a directory.
pub fn load_preset(dir: &Path, name: &str) -> Result<AdjustmentRecipe, Error> {
    let validated_name = validate_name(name)?;

    let path = dir.join(format!("{}.{}", validated_name, PRESET_EXTENSION));

    if !path.exists() {
        return Err(Error::Refused(format!(
            "No preset called {}.",
            validated_name
        )));
    }

    // Load the recipe using the existing load_recipe function
    load_recipe(&path)
}

/// Save a preset to a directory.
pub fn save_preset(
    dir: &Path,
    name: &str,
    recipe: &AdjustmentRecipe,
    overwrite: bool,
) -> Result<(), Error> {
    let validated_name = validate_name(name)?;

    let path = dir.join(format!("{}.{}", validated_name, PRESET_EXTENSION));

    // Create the directory if it doesn't exist
    fs::create_dir_all(dir)?;

    // A preset is a look, applied to other photographs. The source hash names the
    // photograph it was made on, and the geometry is that photograph's framing: a
    // crop or a straighten carried into a preset would reframe every photograph it
    // touched, in batch as well as one at a time (ED-18 applies a recipe's geometry).
    let mut recipe = recipe.clone();
    recipe.source_sha256 = String::new();
    recipe.geometry = None;
    recipe.version = CURRENT_RECIPE_VERSION;

    // Serialize to JSON
    let json_data = serde_json::to_string_pretty(&recipe)
        .map_err(|e| Error::Internal(format!("Failed to serialize preset: {}", e)))?;

    // Write to a temporary file first
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp_path = dir.join(format!(
        ".{}_{:x}_{}.tmp",
        PRESET_EXTENSION,
        std::process::id(),
        counter
    ));

    // Use create_new to ensure we don't accidentally overwrite existing files
    let mut temp_file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .map_err(|e| {
            let _ = fs::remove_file(&temp_path);
            Error::Internal(format!("Failed to create temporary file: {}", e))
        })?;

    temp_file.write_all(json_data.as_bytes()).map_err(|e| {
        let _ = fs::remove_file(&temp_path);
        Error::Internal(format!("Failed to write to temporary file: {}", e))
    })?;

    // Commit the file
    if overwrite {
        fs::rename(&temp_path, &path).map_err(|e| {
            let _ = fs::remove_file(&temp_path);
            Error::Internal(format!("Failed to commit preset: {}", e))
        })?;
    } else {
        // Use hard link to ensure atomicity - if the target exists, this will fail
        fs::hard_link(&temp_path, &path).map_err(|e| {
            // Clean up temp file on failure
            let _ = fs::remove_file(&temp_path);
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::Refused(format!(
                    "A preset called {} already exists.",
                    validated_name
                ))
            } else {
                Error::Internal(format!("Failed to create preset: {}", e))
            }
        })?;

        // Remove temp file
        fs::remove_file(&temp_path)
            .map_err(|e| Error::Internal(format!("Failed to clean up temporary file: {}", e)))?;
    }

    Ok(())
}

/// Rename a preset.
pub fn rename_preset(dir: &Path, from: &str, to: &str) -> Result<(), Error> {
    let validated_from = validate_name(from)?;
    let validated_to = validate_name(to)?;

    let from_path = dir.join(format!("{}.{}", validated_from, PRESET_EXTENSION));
    let to_path = dir.join(format!("{}.{}", validated_to, PRESET_EXTENSION));

    if !from_path.exists() {
        return Err(Error::Refused(format!(
            "No preset called {}.",
            validated_from
        )));
    }

    // Check if the target already exists
    // Special case: if only case differs, we allow it (macOS case insensitive)
    let case_only = validated_from != validated_to
        && validated_from.to_lowercase() == validated_to.to_lowercase();
    if !case_only && to_path.exists() {
        return Err(Error::Refused(format!(
            "A preset called {} already exists.",
            validated_to
        )));
    }

    // Special case: if only case differs, use rename directly
    if case_only {
        fs::rename(&from_path, &to_path)
            .map_err(|e| Error::Internal(format!("Failed to rename preset: {}", e)))?;
    } else {
        // Use hard link then remove the old file
        fs::hard_link(&from_path, &to_path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error::Refused(format!("A preset called {} already exists.", validated_to))
            } else {
                Error::Internal(format!("Failed to create hard link for preset: {}", e))
            }
        })?;

        fs::remove_file(&from_path).map_err(|e| {
            // If we can't remove the old file, we should try to remove the new one
            let _ = fs::remove_file(&to_path);
            Error::Internal(format!("Failed to remove old preset file: {}", e))
        })?;
    }

    Ok(())
}

/// Delete a preset.
pub fn delete_preset(dir: &Path, name: &str) -> Result<(), Error> {
    let validated_name = validate_name(name)?;

    let path = dir.join(format!("{}.{}", validated_name, PRESET_EXTENSION));

    if !path.exists() {
        return Err(Error::Refused(format!(
            "No preset called {}.",
            validated_name
        )));
    }

    fs::remove_file(&path)
        .map_err(|e| Error::Internal(format!("Failed to delete preset: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn a_preset_carries_the_look_not_the_framing() {
        let temp_dir = tempdir().unwrap();
        let dir = temp_dir.path();

        let mut recipe = AdjustmentRecipe {
            exposure: 0.7,
            source_sha256: "abc".into(),
            ..Default::default()
        };
        recipe.geometry = Some(crate::media::edit::geometry::Geometry {
            straighten: 3.5,
            rotate: 90,
            ..Default::default()
        });

        save_preset(dir, "framed", &recipe, false).unwrap();
        let loaded = load_preset(dir, "framed").unwrap();

        assert_eq!(loaded.exposure, 0.7);
        assert_eq!(loaded.geometry, None);
        assert!(loaded.source_sha256.is_empty());
    }

    #[test]
    fn preset_round_trips_to_disk_and_appears_in_preset_library() {
        let temp_dir = tempdir().unwrap();
        let dir = temp_dir.path();

        // Create a test recipe
        let recipe = AdjustmentRecipe {
            exposure: 0.5,
            temperature: 10.0,
            ..Default::default()
        };

        // Save the preset
        save_preset(dir, "test", &recipe, false).unwrap();

        // Load it back
        let loaded = load_preset(dir, "test").unwrap();

        // Verify it's the same
        assert_eq!(loaded, recipe);

        // List presets
        let list = list_presets(dir).unwrap();
        assert_eq!(list.presets.len(), 1);
        assert_eq!(list.presets[0].name, "test");
        assert_eq!(list.errors.len(), 0);
    }

    #[test]
    fn preset_deletion_and_rename_are_safe_and_atomic() {
        let temp_dir = tempdir().unwrap();
        let dir = temp_dir.path();

        // Create a test recipe
        let recipe = AdjustmentRecipe::default();

        // Save the preset
        save_preset(dir, "test", &recipe, false).unwrap();

        // Rename it
        rename_preset(dir, "test", "renamed").unwrap();

        // Verify old name is gone
        assert!(load_preset(dir, "test").is_err());

        // Verify new name works
        let loaded = load_preset(dir, "renamed").unwrap();
        assert_eq!(loaded, recipe);

        // Try to rename to existing name (should fail)
        save_preset(dir, "another", &recipe, false).unwrap();
        assert!(rename_preset(dir, "renamed", "another").is_err());

        // Verify both names still exist after failed rename
        let loaded_renamed = load_preset(dir, "renamed").unwrap();
        let loaded_another = load_preset(dir, "another").unwrap();
        assert_eq!(loaded_renamed, recipe);
        assert_eq!(loaded_another, recipe);

        // Try to delete it
        delete_preset(dir, "renamed").unwrap();
        assert!(load_preset(dir, "renamed").is_err());

        // Try to delete non-existent (should fail)
        assert!(delete_preset(dir, "nonexistent").is_err());
    }

    #[test]
    fn a_preset_does_not_carry_the_source_photographs_hash() {
        let temp_dir = tempdir().unwrap();
        let dir = temp_dir.path();

        // Create a recipe with a source hash
        let recipe = AdjustmentRecipe {
            source_sha256: "some_hash".to_string(),
            ..Default::default()
        };

        // Save the preset
        save_preset(dir, "test", &recipe, false).unwrap();

        // Load it back
        let loaded = load_preset(dir, "test").unwrap();

        // Verify the source hash is cleared
        assert_eq!(loaded.source_sha256, "");
        assert_eq!(loaded.version, CURRENT_RECIPE_VERSION);
    }

    #[test]
    fn saving_over_an_existing_preset_is_refused_unless_asked() {
        let temp_dir = tempdir().unwrap();
        let dir = temp_dir.path();

        // Create a test recipe
        let recipe = AdjustmentRecipe::default();

        // Save the preset
        save_preset(dir, "test", &recipe, false).unwrap();

        // Try to save again without overwrite (should fail)
        assert!(save_preset(dir, "test", &recipe, false).is_err());

        // Try to save with overwrite (should succeed)
        save_preset(dir, "test", &recipe, true).unwrap();
    }

    #[test]
    fn a_name_that_could_escape_the_folder_is_refused() {
        // Test invalid names
        let long = "a".repeat(65);
        for name in ["../x", "a/b", ".hidden", "", long.as_str()] {
            assert!(validate_name(name).is_err(), "{name:?} should be refused");
        }

        // Test valid names
        let valid_names = vec!["valid_name", "test123", "Test_Name"];
        for name in valid_names {
            assert!(validate_name(name).is_ok());
        }
    }

    #[test]
    fn a_malformed_preset_is_reported_not_hidden() {
        let temp_dir = tempdir().unwrap();
        let dir = temp_dir.path();

        // Create a malformed preset file
        let bad_path = dir.join("bad.photopreset");
        fs::write(&bad_path, "this is not valid json").unwrap();

        // Create a good preset file
        let recipe = AdjustmentRecipe::default();
        save_preset(dir, "good", &recipe, false).unwrap();

        // List presets
        let list = list_presets(dir).unwrap();

        // Should have one good preset and one error
        assert_eq!(list.presets.len(), 1);
        assert_eq!(list.presets[0].name, "good");
        assert_eq!(list.errors.len(), 1);
        assert_eq!(list.errors[0].name, "bad");
    }

    #[test]
    fn a_preset_can_be_renamed_to_a_different_case_of_its_own_name() {
        let temp_dir = tempdir().unwrap();
        let dir = temp_dir.path();

        // Create a test recipe
        let recipe = AdjustmentRecipe::default();

        // Save the preset
        save_preset(dir, "Test", &recipe, false).unwrap();

        // Rename to different case (should work)
        rename_preset(dir, "Test", "test").unwrap();

        // Verify it's accessible with new name
        let loaded = load_preset(dir, "test").unwrap();
        assert_eq!(loaded, recipe);
    }
}
