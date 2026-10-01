//! Batch recipe application desktop commands (ED-18).
//!
//! Mirrors `plan_bulk_lut` / `apply_bulk_lut` in shape, naming and error
//! wording. A `source` is either a preset name or a full recipe.

use super::{describe, resolve_inputs, resolve_output, CommandResult};
use crate::AppState;
use phototools_core::media::edit::AdjustmentRecipe;
use phototools_core::tools::bulk_edit::{BulkEditParams, BulkEditTool};
use phototools_core::tools::presets::load_preset as core_load_preset;
use phototools_core::tools::{Skip, Tool};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::State;

/// The source of the recipe for a batch run.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind")]
pub enum RecipeSource {
    Recipe { recipe: Box<AdjustmentRecipe> },
    Preset { name: String },
}

/// Summary returned by the dry-run command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkEditPlanSummary {
    pub actions_count: usize,
    pub skipped: Vec<Skip>,
    pub recipe_sha256: String,
    pub sample_frames: Vec<String>,
}

#[tauri::command]
pub fn plan_bulk_edit(
    inputs: Vec<String>,
    source: RecipeSource,
    out_dir: String,
    recursive: Option<bool>,
    state: State<'_, AppState>,
) -> CommandResult<BulkEditPlanSummary> {
    plan_bulk_edit_impl(&state, inputs, source, out_dir, recursive.unwrap_or(false))
}

pub fn plan_bulk_edit_impl(
    state: &AppState,
    inputs: Vec<String>,
    source: RecipeSource,
    out_dir: String,
    recursive: bool,
) -> CommandResult<BulkEditPlanSummary> {
    let config = state.config();
    let resolved_inputs = resolve_inputs(&config, &inputs)?;
    let resolved_out = resolve_output(&config, &out_dir)?;

    let recipe = recipe_from_source(&config.presets_dir(), &source)?;
    let lut_dir = config.lut_dir();

    let mut params = BulkEditParams::new(resolved_inputs, recipe, lut_dir, resolved_out);
    params.recursive = recursive;

    let plan = BulkEditTool.plan(&params).map_err(describe)?.data;
    let sample_frames = phototools_core::tools::bulk_edit::sample_frames(&plan, 3)
        .into_iter()
        .map(|p| p.display().to_string())
        .collect();
    let recipe_sha256 = plan
        .actions
        .first()
        .map(|a| a.recipe_sha256.clone())
        .unwrap_or_else(|| phototools_core::tools::bulk_edit::recipe_sha256(&params.recipe));

    Ok(BulkEditPlanSummary {
        actions_count: plan.actions.len(),
        skipped: plan.skipped,
        recipe_sha256,
        sample_frames,
    })
}

#[tauri::command]
pub fn apply_bulk_edit(
    inputs: Vec<String>,
    source: RecipeSource,
    out_dir: String,
    reviewed_recipe_sha256: String,
    recursive: Option<bool>,
    state: State<'_, AppState>,
) -> CommandResult<String> {
    apply_bulk_edit_impl(
        &state,
        inputs,
        source,
        out_dir,
        reviewed_recipe_sha256,
        recursive.unwrap_or(false),
    )
}

pub fn apply_bulk_edit_impl(
    state: &AppState,
    inputs: Vec<String>,
    source: RecipeSource,
    out_dir: String,
    reviewed_recipe_sha256: String,
    recursive: bool,
) -> CommandResult<String> {
    let config = state.config();
    let resolved_inputs = resolve_inputs(&config, &inputs)?;
    let resolved_out = resolve_output(&config, &out_dir)?;

    let recipe = recipe_from_source(&config.presets_dir(), &source)?;
    let lut_dir = config.lut_dir();

    let total = resolved_inputs.len() as u64;
    let mut params = BulkEditParams::new(resolved_inputs, recipe, lut_dir, resolved_out);
    params.recursive = recursive;
    let expected_hash = reviewed_recipe_sha256;

    state
        .jobs
        .spawn("bulk_edit", total, move |progress| {
            let plan = BulkEditTool.plan(&params)?.data;
            let skipped = plan.skipped.clone();
            let summary = BulkEditTool
                .apply_with(plan, &expected_hash, progress, None)?
                .data;
            let skipped = [skipped, summary.metadata_skipped.clone()].concat();
            Ok(phototools_core::tools::summarise(
                summary.written.len(),
                "edited",
                summary.failures.len(),
                &skipped,
                &phototools_core::tools::lut::ACCEPTED,
            ))
        })
        .map_err(describe)
}

/// Resolve a `RecipeSource` into a concrete `AdjustmentRecipe`.
fn recipe_from_source(
    presets_dir: &Path,
    source: &RecipeSource,
) -> CommandResult<AdjustmentRecipe> {
    match source {
        RecipeSource::Recipe { recipe } => Ok(*recipe.clone()),
        RecipeSource::Preset { name } => core_load_preset(presets_dir, name).map_err(describe),
    }
}
