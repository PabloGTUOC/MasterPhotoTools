//! Automatic subject and sky masks (ED-23).
//!
//! Thin wrappers over `phototools_core::media::segment`: which models are on disk,
//! downloading one as a job the person can follow and cancel, and making a mask from the
//! photograph open in the editor. The mask is made on a blocking thread so the window keeps
//! drawing during the few seconds it takes.

use super::{describe, CommandResult};
use crate::AppState;
use rollchrome_core::media::edit::{AutoTarget, ObjectClick, StoredRaster};
use rollchrome_core::media::segment;
use serde::Serialize;
use std::path::Path;
use std::sync::Mutex;
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
    let dir = rollchrome_core::config::models_dir(&state.config());
    [AutoTarget::Subject, AutoTarget::People, AutoTarget::Sky]
        .into_iter()
        .map(|target| MaskModel {
            target,
            name: segment::display_name(&dir, target),
            present: segment::target_present(&dir, target),
            bytes: segment::missing_bytes(&dir, target),
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
    let dir = rollchrome_core::config::models_dir(&state.config());
    let missing = segment::missing_bytes(&dir, target);
    let name = segment::display_name(&dir, target);
    state
        .jobs
        .spawn("download_mask_model", missing, move |progress| {
            segment::download_target(&dir, target, progress)?;
            Ok(format!("Downloaded {name}"))
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
    let dir = rollchrome_core::config::models_dir(&state.config());
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
    segment::auto_mask(
        &input,
        orientation,
        target,
        &rollchrome_core::config::models_dir(&state.config()),
    )
    .map_err(describe)
}

/// The object segmentation model pair, as the Masks panel shows it (RP-11c).
#[derive(Debug, Clone, Serialize)]
pub struct ObjectModelStatus {
    pub name: String,
    pub present: bool,
    pub bytes: u64,
}

#[tauri::command]
pub fn object_model_status(state: State<'_, AppState>) -> CommandResult<ObjectModelStatus> {
    Ok(object_model_status_impl(&state))
}

pub fn object_model_status_impl(state: &AppState) -> ObjectModelStatus {
    let dir = rollchrome_core::config::models_dir(&state.config());
    ObjectModelStatus {
        name: segment::OBJECT_MODEL_NAME.to_string(),
        present: segment::object_model_present(&dir),
        bytes: segment::OBJECT_MODEL_BYTES,
    }
}

/// Downloads the object segmentation model pair as a single job.
#[tauri::command]
pub fn download_object_model(state: State<'_, AppState>) -> CommandResult<String> {
    download_object_model_impl(&state)
}

pub fn download_object_model_impl(state: &AppState) -> CommandResult<String> {
    let dir = rollchrome_core::config::models_dir(&state.config());
    state
        .jobs
        .spawn(
            "download_object_model",
            segment::OBJECT_MODEL_BYTES,
            move |progress| {
                segment::download_object_model(&dir, progress)?;
                Ok(format!("Downloaded {}", segment::OBJECT_MODEL_NAME))
            },
        )
        .map_err(describe)
}

/// Runs one selection, building the photograph's `ObjectSelector` first if this is the first
/// click on it. `input` is absent only when the selector was already seen to be built.
fn run_object_selection(
    slot: &Mutex<Option<segment::ObjectSelector>>,
    input: Option<segment::RgbImage>,
    orientation: u32,
    dir: &Path,
    clicks: &[ObjectClick],
) -> Result<StoredRaster, phototools_core::error::Error> {
    let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        let input = input.ok_or_else(|| {
            phototools_core::error::Error::Internal("object selector has no pixels".into())
        })?;
        *guard = Some(segment::ObjectSelector::new(&input, orientation, dir)?);
    }
    guard
        .as_mut()
        .ok_or_else(|| phototools_core::error::Error::Internal("object selector missing".into()))?
        .select(clicks)
}

/// Selects an object in preview session `session_id` from `clicks`, building the
/// photograph's `ObjectSelector` on the first click and reusing it after.
#[tauri::command]
pub async fn select_object(
    session_id: String,
    clicks: Vec<ObjectClick>,
    state: State<'_, AppState>,
) -> CommandResult<StoredRaster> {
    let (slot, input, orientation) = state
        .object_selection_input(&session_id)
        .map_err(describe)?;
    let dir = rollchrome_core::config::models_dir(&state.config());
    tauri::async_runtime::spawn_blocking(move || {
        run_object_selection(&slot, input, orientation, &dir, &clicks)
    })
    .await
    .map_err(|e| format!("the object could not be selected: {e}"))?
    .map_err(describe)
}

/// `select_object` without the async runtime, for tests.
pub fn select_object_impl(
    state: &AppState,
    session_id: &str,
    clicks: &[ObjectClick],
) -> CommandResult<StoredRaster> {
    let (slot, input, orientation) = state.object_selection_input(session_id).map_err(describe)?;
    let dir = rollchrome_core::config::models_dir(&state.config());
    run_object_selection(&slot, input, orientation, &dir, clicks).map_err(describe)
}
