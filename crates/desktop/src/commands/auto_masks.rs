//! Automatic subject and sky masks (ED-23).
//!
//! Thin wrappers over `phototools_core::media::segment`: which models are on disk,
//! downloading one as a job the person can follow and cancel, and making a mask from the
//! photograph open in the editor. The mask is made on a blocking thread so the window keeps
//! drawing during the few seconds it takes.

use super::{describe, CommandResult};
use crate::AppState;
use phototools_core::media::edit::{AutoTarget, StoredRaster};
use phototools_core::media::segment;
use serde::Serialize;
use tauri::State;

/// One model, as the Masks panel shows it.
#[derive(Debug, Clone, Serialize)]
pub struct MaskModel {
    pub target: AutoTarget,
    pub name: String,
    pub present: bool,
    pub bytes: u64,
}

#[tauri::command]
pub fn auto_mask_models(state: State<'_, AppState>) -> CommandResult<Vec<MaskModel>> {
    Ok(auto_mask_models_impl(&state))
}

pub fn auto_mask_models_impl(state: &AppState) -> Vec<MaskModel> {
    let dir = state.config().models_dir();
    [AutoTarget::Subject, AutoTarget::Sky]
        .into_iter()
        .map(|target| {
            let spec = segment::spec(target);
            MaskModel {
                target,
                name: spec.name.to_string(),
                present: segment::model_present(&dir, spec),
                bytes: spec.bytes,
            }
        })
        .collect()
}

/// Downloads one model as a job: progress in bytes, cancellable, kept only if it verifies.
#[tauri::command]
pub fn download_mask_model(
    target: AutoTarget,
    state: State<'_, AppState>,
) -> CommandResult<String> {
    download_mask_model_impl(&state, target)
}

pub fn download_mask_model_impl(state: &AppState, target: AutoTarget) -> CommandResult<String> {
    let dir = state.config().models_dir();
    let spec = segment::spec(target);
    state
        .jobs
        .spawn("download_mask_model", spec.bytes, move |progress| {
            segment::download_model(&dir, spec, progress)?;
            Ok(format!("Downloaded {}", spec.name))
        })
        .map_err(describe)
}

/// Makes the automatic mask of the photograph open in preview session `session_id`.
#[tauri::command]
pub async fn make_auto_mask(
    session_id: String,
    target: AutoTarget,
    state: State<'_, AppState>,
) -> CommandResult<StoredRaster> {
    let (input, orientation) = state.segmentation_input(&session_id).map_err(describe)?;
    let dir = state.config().models_dir();
    tauri::async_runtime::spawn_blocking(move || {
        segment::auto_mask(&input, orientation, target, &dir)
    })
    .await
    .map_err(|e| format!("the mask could not be made: {e}"))?
    .map_err(describe)
}

/// `make_auto_mask` without the runtime, for tests.
pub fn make_auto_mask_impl(
    state: &AppState,
    session_id: &str,
    target: AutoTarget,
) -> CommandResult<StoredRaster> {
    let (input, orientation) = state.segmentation_input(session_id).map_err(describe)?;
    segment::auto_mask(&input, orientation, target, &state.config().models_dir()).map_err(describe)
}
