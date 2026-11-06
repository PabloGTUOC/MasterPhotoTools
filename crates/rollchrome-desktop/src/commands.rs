//! The Tauri command layer.
//!
//! Commands parse, resolve paths against the configured roots (G6), and
//! delegate to `core`. No functionality lives here (G1).

use crate::AppState;
use phototools_core::config::Config;
use phototools_core::error::Error;
use phototools_core::jobs::Job;
use phototools_core::tools::f9_browser;
use std::path::PathBuf;
use tauri::State;

pub mod auto_masks;
pub use auto_masks::*;
pub mod bulk_edit;
pub use bulk_edit::*;
pub mod develop;
pub use develop::*;
pub mod edit;
pub use edit::*;
pub mod presets;
pub use presets::*;

/// Commands answer with a message, not a `Result<_, Error>`, because Tauri
/// serialises the error side as an opaque string. This keeps the reason visible.
pub(crate) type CommandResult<T> = Result<T, String>;

pub(crate) fn describe(e: Error) -> String {
    match e {
        Error::AccessDenied(_) => {
            "That path is outside the folders this application may touch.".to_string()
        }
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// G6 — path resolution
// ---------------------------------------------------------------------------

pub(crate) fn resolve_input(config: &Config, path: &str) -> CommandResult<PathBuf> {
    config.resolve(std::path::Path::new(path)).map_err(describe)
}

pub(crate) fn resolve_inputs(config: &Config, paths: &[String]) -> CommandResult<Vec<PathBuf>> {
    if paths.is_empty() {
        return Err("No paths were supplied.".into());
    }
    paths.iter().map(|p| resolve_input(config, p)).collect()
}

pub(crate) fn resolve_output(config: &Config, path: &str) -> CommandResult<PathBuf> {
    config
        .resolve_for_create(std::path::Path::new(path))
        .map_err(describe)
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_job(id: String, state: State<'_, AppState>) -> CommandResult<Option<Job>> {
    state.jobs.get(&id).map_err(describe)
}

#[tauri::command]
pub fn cancel_job(id: String, state: State<'_, AppState>) -> CommandResult<bool> {
    cancel_job_impl(&state, id)
}

pub fn cancel_job_impl(state: &AppState, id: String) -> CommandResult<bool> {
    Ok(state.jobs.cancel(&id))
}

// ---------------------------------------------------------------------------
// F9 — library browser
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn list_directory(
    path: String,
    state: State<'_, AppState>,
) -> CommandResult<Vec<f9_browser::BrowserEntry>> {
    let config = state.config();
    f9_browser::list_directory(&config, std::path::Path::new(&path)).map_err(describe)
}

/// The directories a browser may start from.
///
/// The desktop's counterpart to `GET /api/storage/roots`. G6 refuses anything
/// outside a configured root, `/` included, so a folder picker cannot discover
/// the top by listing it — the configuration is the only thing that knows.
#[tauri::command]
pub fn list_roots(state: State<'_, AppState>) -> CommandResult<Vec<PathBuf>> {
    Ok(state.config().roots.clone())
}
