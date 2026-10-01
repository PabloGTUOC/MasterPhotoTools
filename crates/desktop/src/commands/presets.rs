//! Preset management desktop commands (ED-17).
//!
//! Thin wrappers over `phototools_core::tools::presets`. Names are validated by
//! `core`; the presets folder is application-owned, so paths are not resolved
//! against the roots.

use super::{describe, CommandResult};
use crate::AppState;
use phototools_core::media::edit::AdjustmentRecipe;
use phototools_core::tools::presets::{
    delete_preset as core_delete_preset, list_presets as core_list_presets,
    load_preset as core_load_preset, rename_preset as core_rename_preset,
    save_preset as core_save_preset, PresetList,
};
use tauri::State;

#[tauri::command]
pub fn list_presets(state: State<'_, AppState>) -> CommandResult<PresetList> {
    list_presets_impl(&state)
}

pub fn list_presets_impl(state: &AppState) -> CommandResult<PresetList> {
    let presets_dir = state.config().presets_dir();
    core_list_presets(&presets_dir).map_err(describe)
}

#[tauri::command]
pub fn load_preset(name: String, state: State<'_, AppState>) -> CommandResult<AdjustmentRecipe> {
    load_preset_impl(&state, name)
}

pub fn load_preset_impl(state: &AppState, name: String) -> CommandResult<AdjustmentRecipe> {
    let presets_dir = state.config().presets_dir();
    core_load_preset(&presets_dir, &name).map_err(describe)
}

#[tauri::command]
pub fn save_preset(
    name: String,
    recipe: AdjustmentRecipe,
    overwrite: Option<bool>,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    save_preset_impl(&state, name, recipe, overwrite.unwrap_or(false))
}

pub fn save_preset_impl(
    state: &AppState,
    name: String,
    recipe: AdjustmentRecipe,
    overwrite: bool,
) -> CommandResult<()> {
    let presets_dir = state.config().presets_dir();
    core_save_preset(&presets_dir, &name, &recipe, overwrite).map_err(describe)
}

#[tauri::command]
pub fn rename_preset(from: String, to: String, state: State<'_, AppState>) -> CommandResult<()> {
    rename_preset_impl(&state, from, to)
}

pub fn rename_preset_impl(state: &AppState, from: String, to: String) -> CommandResult<()> {
    let presets_dir = state.config().presets_dir();
    core_rename_preset(&presets_dir, &from, &to).map_err(describe)
}

#[tauri::command]
pub fn delete_preset(name: String, state: State<'_, AppState>) -> CommandResult<()> {
    delete_preset_impl(&state, name)
}

pub fn delete_preset_impl(state: &AppState, name: String) -> CommandResult<()> {
    let presets_dir = state.config().presets_dir();
    core_delete_preset(&presets_dir, &name).map_err(describe)
}
