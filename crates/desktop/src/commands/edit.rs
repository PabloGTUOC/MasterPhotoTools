//! Editing and LUT management desktop commands (ED-6).
//!
//! Transport layer only (G1): path resolution against configured roots (G6),
//! input/output validation including Publishing folder refusal (MV-16.7),
//! and delegation to `phototools-core`.

use super::{describe, resolve_input, resolve_inputs, resolve_output, CommandResult};
use crate::AppState;
use phototools_core::media::edit::pipeline::AdjustmentRecipe;
use phototools_core::media::edit::preview::PreviewStage;
use phototools_core::tools::edit::ExportResult;
use phototools_core::tools::lut::{BulkLutParams, BulkLutTool};
use phototools_core::tools::lut_library::{
    find_lut_by_name_or_sha256, import_lut as core_import_lut, list_luts as core_list_luts,
    resolve_recipe_lut, LutEntry, LutLibraryList,
};
use phototools_core::tools::{Skip, Tool};
use serde::{Deserialize, Serialize};
use tauri::ipc::Response;
use tauri::State;

/// Result summary of a bulk LUT dry run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkLutPlanSummary {
    pub actions_count: usize,
    pub skipped: Vec<Skip>,
    pub lut_sha256: String,
    pub sample_frames: Vec<String>,
}

/// Metadata, proxy dimensions, orientation, and card safety status returned when opening a preview session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenPreviewResult {
    pub session_id: String,
    pub drag: (u32, u32),
    pub settle: (u32, u32),
    pub orientation: u32,
    pub read_only: bool,
}

// ---------------------------------------------------------------------------
// 1. Recipes
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn load_recipe(
    path: String,
    state: State<'_, AppState>,
) -> CommandResult<Option<AdjustmentRecipe>> {
    load_recipe_impl(&state, path)
}

pub fn load_recipe_impl(state: &AppState, path: String) -> CommandResult<Option<AdjustmentRecipe>> {
    let config = state.config();
    let resolved = resolve_input(&config, &path)?;
    phototools_core::tools::edit::load_recipe_for_image(&resolved).map_err(describe)
}

#[tauri::command]
pub fn save_recipe(
    path: String,
    recipe: AdjustmentRecipe,
    state: State<'_, AppState>,
) -> CommandResult<String> {
    save_recipe_impl(&state, path, recipe)
}

pub fn save_recipe_impl(
    state: &AppState,
    path: String,
    recipe: AdjustmentRecipe,
) -> CommandResult<String> {
    let config = state.config();
    let resolved = resolve_input(&config, &path)?;
    let saved = phototools_core::tools::edit::save_recipe(&resolved, &recipe).map_err(describe)?;
    Ok(saved.display().to_string())
}

// ---------------------------------------------------------------------------
// 2. Export
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn export_edited_image(
    path: String,
    recipe: AdjustmentRecipe,
    out_dir: String,
    state: State<'_, AppState>,
) -> CommandResult<ExportResult> {
    export_edited_image_impl(&state, path, recipe, out_dir)
}

pub fn export_edited_image_impl(
    state: &AppState,
    path: String,
    recipe: AdjustmentRecipe,
    out_dir: String,
) -> CommandResult<ExportResult> {
    let config = state.config();
    let resolved_path = resolve_input(&config, &path)?;
    // Enforces G6 roots AND Publishing refusal (MV-16.7)
    let resolved_out = resolve_output(&config, &out_dir)?;

    let lut_dir = config.lut_dir();
    let lut_opt = match &recipe.lut {
        Some(lut_ref) => {
            let (_, lut) = resolve_recipe_lut(&lut_dir, lut_ref).map_err(describe)?;
            Some(lut)
        }
        None => None,
    };

    phototools_core::tools::edit::export_edited_image(
        &resolved_path,
        &recipe,
        lut_opt.as_ref(),
        &resolved_out,
    )
    .map_err(describe)
}

// ---------------------------------------------------------------------------
// 3. Bulk LUT
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn plan_bulk_lut(
    inputs: Vec<String>,
    lut: String,
    intensity: f32,
    out_dir: String,
    state: State<'_, AppState>,
) -> CommandResult<BulkLutPlanSummary> {
    plan_bulk_lut_impl(&state, inputs, lut, intensity, out_dir)
}

pub fn plan_bulk_lut_impl(
    state: &AppState,
    inputs: Vec<String>,
    lut: String,
    intensity: f32,
    out_dir: String,
) -> CommandResult<BulkLutPlanSummary> {
    let config = state.config();
    let resolved_inputs = resolve_inputs(&config, &inputs)?;
    // Enforces G6 roots AND Publishing refusal (MV-16.7)
    let resolved_out = resolve_output(&config, &out_dir)?;

    let lut_dir = config.lut_dir();
    let (lut_path, lut_obj) = find_lut_by_name_or_sha256(&lut_dir, &lut)
        .map_err(describe)?
        .ok_or_else(|| format!("LUT '{lut}' was not found in the library"))?;

    let params = BulkLutParams::new(resolved_inputs, lut_path, intensity, resolved_out);
    let plan = BulkLutTool.plan(&params).map_err(describe)?.data;
    let sample_frames = phototools_core::tools::lut::sample_frames(&plan, 3)
        .into_iter()
        .map(|p| p.display().to_string())
        .collect();
    let lut_sha256 = plan
        .actions
        .first()
        .map(|a| a.lut_sha256.clone())
        .unwrap_or(lut_obj.sha256);

    Ok(BulkLutPlanSummary {
        actions_count: plan.actions.len(),
        skipped: plan.skipped,
        lut_sha256,
        sample_frames,
    })
}

#[tauri::command]
pub fn apply_bulk_lut(
    inputs: Vec<String>,
    lut: String,
    intensity: f32,
    out_dir: String,
    reviewed_lut_sha256: String,
    state: State<'_, AppState>,
) -> CommandResult<String> {
    apply_bulk_lut_impl(&state, inputs, lut, intensity, out_dir, reviewed_lut_sha256)
}

pub fn apply_bulk_lut_impl(
    state: &AppState,
    inputs: Vec<String>,
    lut: String,
    intensity: f32,
    out_dir: String,
    reviewed_lut_sha256: String,
) -> CommandResult<String> {
    let config = state.config();
    let resolved_inputs = resolve_inputs(&config, &inputs)?;
    let resolved_out = resolve_output(&config, &out_dir)?;

    let lut_dir = config.lut_dir();
    let (lut_path, _) = find_lut_by_name_or_sha256(&lut_dir, &lut)
        .map_err(describe)?
        .ok_or_else(|| format!("LUT '{lut}' was not found in the library"))?;

    // The reviewed-hash invariant check:
    // If the file at lut_path changed on disk since the plan the user reviewed,
    // refuse before running or spawning.
    let current_lut =
        phototools_core::media::edit::lut::Lut::from_file(&lut_path).map_err(describe)?;
    if current_lut.sha256 != reviewed_lut_sha256 {
        return Err(format!(
            "The LUT file changed on disk since the reviewed plan (expected {}, found {}); please run another dry run",
            reviewed_lut_sha256, current_lut.sha256
        ));
    }

    let total = resolved_inputs.len() as u64;
    let params = BulkLutParams::new(resolved_inputs, lut_path, intensity, resolved_out);
    let expected_hash = reviewed_lut_sha256;

    state
        .jobs
        .spawn("bulk_lut", total, move |progress| {
            let plan = BulkLutTool.plan(&params)?.data;
            if let Some(first) = plan.actions.first() {
                if first.lut_sha256 != expected_hash {
                    return Err(phototools_core::error::Error::Refused(format!(
                        "The LUT file changed on disk since the reviewed plan (expected {}, found {}); please run another dry run",
                        expected_hash, first.lut_sha256
                    )));
                }
            }
            let skipped = plan.skipped.clone();
            let summary = BulkLutTool.apply(plan, progress)?.data;
            let skipped = [skipped, summary.metadata_skipped.clone()].concat();
            Ok(phototools_core::tools::summarise(
                summary.written.len(),
                "graded",
                summary.failures.len(),
                &skipped,
                &phototools_core::tools::lut::ACCEPTED,
            ))
        })
        .map_err(describe)
}

// ---------------------------------------------------------------------------
// 4. Preview
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn open_preview(path: String, state: State<'_, AppState>) -> CommandResult<OpenPreviewResult> {
    open_preview_impl(&state, path)
}

pub fn open_preview_impl(state: &AppState, path: String) -> CommandResult<OpenPreviewResult> {
    let config = state.config();
    let resolved = resolve_input(&config, &path)?;
    let info = state.open_preview(resolved).map_err(describe)?;
    Ok(OpenPreviewResult {
        session_id: info.session_id,
        drag: info.drag,
        settle: info.settle,
        orientation: info.orientation,
        read_only: info.read_only,
    })
}

#[tauri::command]
pub fn render_preview(
    session_id: String,
    recipe: AdjustmentRecipe,
    stage: Option<PreviewStage>,
    state: State<'_, AppState>,
) -> Result<Response, String> {
    let bytes = render_preview_impl(&state, session_id, recipe, stage)?;
    Ok(Response::new(bytes))
}

pub fn render_preview_impl(
    state: &AppState,
    session_id: String,
    recipe: AdjustmentRecipe,
    stage: Option<PreviewStage>,
) -> Result<Vec<u8>, String> {
    let lut_dir = state.config().lut_dir();
    let lut_opt = match &recipe.lut {
        Some(lut_ref) => {
            let (_, lut) = resolve_recipe_lut(&lut_dir, lut_ref).map_err(describe)?;
            Some(lut)
        }
        None => None,
    };
    let stage = stage.unwrap_or(PreviewStage::Drag);
    let frame = state
        .render_preview(&session_id, &recipe, lut_opt.as_ref(), stage)
        .map_err(describe)?;

    let mut payload = Vec::with_capacity(8 + frame.bytes.len());
    payload.extend_from_slice(&frame.width.to_be_bytes());
    payload.extend_from_slice(&frame.height.to_be_bytes());
    payload.extend_from_slice(&frame.bytes);
    Ok(payload)
}

#[tauri::command]
pub fn close_preview(session_id: String, state: State<'_, AppState>) -> CommandResult<()> {
    close_preview_impl(&state, session_id)
}

pub fn close_preview_impl(state: &AppState, session_id: String) -> CommandResult<()> {
    state.close_preview(&session_id);
    Ok(())
}

// ---------------------------------------------------------------------------
// 5. Managed LUT Library
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_luts(state: State<'_, AppState>) -> CommandResult<LutLibraryList> {
    list_luts_impl(&state)
}

pub fn list_luts_impl(state: &AppState) -> CommandResult<LutLibraryList> {
    let lut_dir = state.config().lut_dir();
    core_list_luts(&lut_dir).map_err(describe)
}

#[tauri::command]
pub fn import_lut(path: String, state: State<'_, AppState>) -> CommandResult<LutEntry> {
    import_lut_impl(&state, path)
}

pub fn import_lut_impl(state: &AppState, path: String) -> CommandResult<LutEntry> {
    let config = state.config();
    let resolved = resolve_input(&config, &path)?;
    let lut_dir = config.lut_dir();
    core_import_lut(&lut_dir, &resolved).map_err(describe)
}
