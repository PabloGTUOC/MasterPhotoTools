//! Develop desktop commands (DV-2b, RP-1).
//!
//! Transport only (G1); follows `commands/bulk_edit.rs`.

use super::{describe, resolve_input, resolve_output, CommandResult};
use crate::AppState;
use rollchrome_core::media::develop::OutputFormat;
use rollchrome_core::media::looks::builtin::BuiltinLookInfo;
use rollchrome_core::tools::develop::{
    plan_develop as core_plan_develop, DevelopItem, DevelopPlan,
};
use rollchrome_core::tools::develop_folder::FolderStore;
use tauri::{Manager, State};

fn validate_filename(name: &str) -> Result<(), phototools_core::error::Error> {
    if name.is_empty()
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.contains(std::path::MAIN_SEPARATOR)
    {
        return Err(phototools_core::error::Error::Refused(format!(
            "filename '{name}' contains path separators or '..'"
        )));
    }
    Ok(())
}

/// Lists all shipped built-in film looks.
#[tauri::command]
pub fn list_builtin_looks() -> CommandResult<Vec<BuiltinLookInfo>> {
    Ok(list_builtin_looks_impl())
}

pub fn list_builtin_looks_impl() -> Vec<BuiltinLookInfo> {
    rollchrome_core::media::looks::builtin::builtin_look_list()
}

/// Lists non-recursive RAW files in a folder, sorted by name.
#[tauri::command]
pub fn list_develop_folder(
    folder: String,
    state: State<'_, AppState>,
) -> CommandResult<Vec<String>> {
    list_develop_folder_impl(&state, folder)
}

pub fn list_develop_folder_impl(state: &AppState, folder: String) -> CommandResult<Vec<String>> {
    let config = state.config();
    let resolved_folder = resolve_input(&config, &folder)?;
    rollchrome_core::tools::develop_folder::raw_names(&resolved_folder).map_err(describe)
}

/// Generates an upright thumbnail JPEG (long edge 360) for a RAW file.
#[tauri::command]
pub async fn develop_thumbnail(
    folder: String,
    name: String,
    app: tauri::AppHandle,
) -> Result<tauri::ipc::Response, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        develop_thumbnail_impl(&state, folder, name)
    })
    .await
    .map_err(|e| format!("thumbnail generation panicked: {e}"))?
    .map(tauri::ipc::Response::new)
}

pub fn develop_thumbnail_impl(
    state: &AppState,
    folder: String,
    name: String,
) -> Result<Vec<u8>, String> {
    validate_filename(&name).map_err(describe)?;
    let config = state.config();
    let resolved_folder = resolve_input(&config, &folder)?;
    let path = resolved_folder.join(name);
    rollchrome_core::tools::develop_folder::thumbnail(&path, 360).map_err(describe)
}

/// Generates or renders an upright develop preview JPEG for display.
#[tauri::command]
pub async fn preview_develop(
    folder: String,
    name: String,
    look: Option<String>,
    exposure_ev: f32,
    draft: bool,
    app: tauri::AppHandle,
) -> Result<tauri::ipc::Response, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        preview_develop_impl(&state, folder, name, look, exposure_ev, draft)
    })
    .await
    .map_err(|e| format!("preview develop panicked: {e}"))?
    .map(tauri::ipc::Response::new)
}

pub fn preview_develop_impl(
    state: &AppState,
    folder: String,
    name: String,
    look: Option<String>,
    exposure_ev: f32,
    draft: bool,
) -> Result<Vec<u8>, String> {
    validate_filename(&name).map_err(describe)?;
    let config = state.config();
    let resolved_folder = resolve_input(&config, &folder)?;
    let path = resolved_folder.join(name);

    let look_obj = if let Some(look_id) = look {
        let lut_dir = rollchrome_core::config::lut_dir(&config);
        let store = FolderStore::new(resolved_folder, lut_dir);
        let l =
            rollchrome_core::tools::develop::resolve_look(&look_id, &store).map_err(describe)?;
        Some(l)
    } else {
        None
    };

    let preview = state
        .get_or_create_develop_preview(&path)
        .map_err(describe)?;
    let rendered = preview
        .render(look_obj.as_ref(), exposure_ev, draft)
        .map_err(describe)?;
    Ok(rendered.bytes)
}

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
    rollchrome_core::tools::develop_folder::refuse_card_folder(
        &resolved_folder,
        rollchrome_core::tools::edit::find_volume_root,
    )
    .map_err(describe)?;
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
    rollchrome_core::tools::develop_folder::refuse_card_folder(
        &resolved_folder,
        rollchrome_core::tools::edit::find_volume_root,
    )
    .map_err(describe)?;
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
