//! Develop desktop commands (DV-2b).
//!
//! Transport only (G1); follows `commands/bulk_edit.rs`.

use super::{describe, resolve_input, resolve_output, CommandResult};
use crate::AppState;
use rollchrome_core::media::develop::OutputFormat;
use rollchrome_core::tools::develop::{
    plan_develop as core_plan_develop, DevelopItem, DevelopPlan,
};
use rollchrome_core::tools::develop_folder::FolderStore;
use tauri::State;

#[tauri::command]
pub fn plan_develop(
    folder: String,
    items: Vec<DevelopItem>,
    format: OutputFormat,
    state: State<'_, AppState>,
) -> CommandResult<DevelopPlan> {
    plan_develop_impl(&state, folder, items, format)
}

pub fn plan_develop_impl(
    state: &AppState,
    folder: String,
    items: Vec<DevelopItem>,
    format: OutputFormat,
) -> CommandResult<DevelopPlan> {
    let config = state.config();
    let resolved_folder = resolve_input(&config, &folder)?;
    let dev_folder = resolved_folder.join("Developed");
    resolve_output(&config, &dev_folder.to_string_lossy())?;

    let lut_dir = rollchrome_core::config::lut_dir(&config);
    let store = FolderStore::new(resolved_folder, lut_dir);
    let existing = store.existing_outputs().map_err(describe)?;
    Ok(core_plan_develop(&items, format, &existing))
}

#[tauri::command]
pub fn apply_develop(
    folder: String,
    items: Vec<DevelopItem>,
    format: OutputFormat,
    state: State<'_, AppState>,
) -> CommandResult<String> {
    apply_develop_impl(&state, folder, items, format)
}

pub fn apply_develop_impl(
    state: &AppState,
    folder: String,
    items: Vec<DevelopItem>,
    format: OutputFormat,
) -> CommandResult<String> {
    let config = state.config();
    let resolved_folder = resolve_input(&config, &folder)?;
    let dev_folder = resolved_folder.join("Developed");
    resolve_output(&config, &dev_folder.to_string_lossy())?;

    let lut_dir = rollchrome_core::config::lut_dir(&config);
    let total = items.len() as u64;

    state
        .jobs
        .spawn("develop", total, move |progress| {
            let mut store = FolderStore::new(resolved_folder, lut_dir);
            let existing = store.existing_outputs()?;
            let plan = core_plan_develop(&items, format, &existing);
            let skipped = plan.skipped.clone();
            let summary =
                rollchrome_core::tools::develop::execute_develop(plan, &mut store, progress);
            let all_skipped = [skipped, summary.metadata_skipped].concat();
            Ok(phototools_core::tools::summarise(
                summary.developed.len(),
                "developed",
                summary.failures.len(),
                &all_skipped,
                phototools_core::media::raw::RAW_EXTENSIONS,
            ))
        })
        .map_err(describe)
}
