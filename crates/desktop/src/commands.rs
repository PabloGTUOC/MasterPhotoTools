//! The Tauri command layer.
//!
//! Commands parse, resolve paths against the configured roots (G6), and
//! delegate to `core`. No functionality lives here (G1).

use crate::server::{ServerSettings, ServerStatus};
use crate::AppState;
use phototools_core::config::Config;
use phototools_core::error::Error;
use phototools_core::jobs::Job;
use phototools_core::tools::f1_dates::{DateRepairParams, DateRepairTool, RepairMode, ScanResult};
use phototools_core::tools::f3_rename::{
    BatchRenameAction, BatchRenameParams, BatchRenamerTool, RenameOrder,
};
use phototools_core::tools::f4_split::{SplitParams, SplitTool};
use phototools_core::tools::f5_contact::{ContactSheetParams, ContactSheetTool};
use phototools_core::tools::f6_transform::{TargetFormat, TransformParams, TransformTool};
use phototools_core::tools::f7_border::{PrintBorderParams, PrintBorderTool};
use phototools_core::tools::f8_tiff::{TiffToJpegParams, TiffToJpegTool};
use phototools_core::tools::{f9_browser, Plan, Tool};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

/// Commands answer with a message, not a `Result<_, Error>`, because Tauri
/// serialises the error side as an opaque string. This keeps the reason visible.
type CommandResult<T> = Result<T, String>;

fn describe(e: Error) -> String {
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

fn resolve_input(config: &Config, path: &str) -> CommandResult<PathBuf> {
    config.resolve(std::path::Path::new(path)).map_err(describe)
}

fn resolve_inputs(config: &Config, paths: &[String]) -> CommandResult<Vec<PathBuf>> {
    if paths.is_empty() {
        return Err("No paths were supplied.".into());
    }
    paths.iter().map(|p| resolve_input(config, p)).collect()
}

fn resolve_output(config: &Config, path: &str) -> CommandResult<PathBuf> {
    config
        .resolve_for_create(std::path::Path::new(path))
        .map_err(describe)
}

// ---------------------------------------------------------------------------
// Configuration and server connection
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_config(state: State<'_, AppState>) -> CommandResult<Config> {
    Ok(state.config().as_ref().clone())
}

#[tauri::command]
pub fn save_config(new_config: Config, state: State<'_, AppState>) -> CommandResult<()> {
    new_config.save().map_err(describe)?;
    state.set_config(new_config);
    Ok(())
}

#[tauri::command]
pub fn get_server_settings(state: State<'_, AppState>) -> ServerSettings {
    state.server.settings()
}

#[tauri::command]
pub fn set_server_settings(settings: ServerSettings, state: State<'_, AppState>) {
    state.server.set_settings(settings);
}

/// Whether the NAS is answering.
///
/// Server-backed features disable on the strength of this; local tools do not
/// care and keep working (task 6).
#[tauri::command]
pub async fn server_status(state: State<'_, AppState>) -> CommandResult<ServerStatus> {
    Ok(state.server.status().await)
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_job(id: String, state: State<'_, AppState>) -> CommandResult<Option<Job>> {
    state.jobs.get(&id).map_err(describe)
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

// ---------------------------------------------------------------------------
// F1 — dates
// ---------------------------------------------------------------------------

/// Synchronous because the desktop scans a local folder and wants the table
/// back, not a job to poll.
#[tauri::command]
pub fn scan_dates(
    path: String,
    recursive: bool,
    state: State<'_, AppState>,
) -> CommandResult<Vec<ScanResult>> {
    let config = state.config();
    let root = resolve_input(&config, &path)?;
    phototools_core::tools::f1_dates::scan_dates(&root, recursive).map_err(describe)
}

#[derive(Debug, Deserialize)]
pub struct FixDatesArgs {
    pub paths: Vec<String>,
    pub mode: RepairMode,
    #[serde(default)]
    pub dry_run: bool,
}

#[tauri::command]
pub fn fix_dates(args: FixDatesArgs, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let paths = resolve_inputs(&config, &args.paths)?;
    let total = paths.len() as u64;
    let params = DateRepairParams {
        paths,
        mode: args.mode,
    };
    let dry_run = args.dry_run;

    state
        .jobs
        .spawn("dates_fix", total, move |progress| {
            let plan = DateRepairTool.plan(&params)?.data;
            if dry_run {
                return Ok(format!(
                    "dry run: {} files would be redated, {} skipped",
                    plan.actions.len(),
                    plan.skipped.len()
                ));
            }
            let summary = DateRepairTool.apply(plan, progress)?.data;
            Ok(format!(
                "{} verified, {} failed",
                summary.verified_count(),
                summary.failures.len()
            ))
        })
        .map_err(describe)
}

// ---------------------------------------------------------------------------
// F3 — rename
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RenameArgs {
    pub paths: Vec<String>,
    pub date: Option<String>,
    pub subject: Option<String>,
    pub camera: Option<String>,
    pub film: Option<String>,
    pub order: RenameOrder,
}

impl RenameArgs {
    fn into_params(self, config: &Config) -> CommandResult<BatchRenameParams> {
        Ok(BatchRenameParams {
            paths: resolve_inputs(config, &self.paths)?,
            date: self.date,
            subject: self.subject,
            camera: self.camera,
            film: self.film,
            order: self.order,
        })
    }
}

#[tauri::command]
pub fn plan_rename(
    args: RenameArgs,
    state: State<'_, AppState>,
) -> CommandResult<Plan<BatchRenameAction>> {
    let config = state.config();
    let params = args.into_params(&config)?;
    BatchRenamerTool
        .plan(&params)
        .map(|outcome| outcome.data)
        .map_err(describe)
}

#[tauri::command]
pub fn apply_rename(args: RenameArgs, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let params = args.into_params(&config)?;
    let total = params.paths.len() as u64;

    state
        .jobs
        .spawn("rename_apply", total, move |progress| {
            let plan = BatchRenamerTool.plan(&params)?.data;
            let summary = BatchRenamerTool.apply(plan, progress)?.data;
            Ok(format!(
                "{} renamed, {} failed",
                summary.renamed.len(),
                summary.failures.len()
            ))
        })
        .map_err(describe)
}

// ---------------------------------------------------------------------------
// F4–F8 — the image tools
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ImageToolArgs {
    pub inputs: Vec<String>,
    #[serde(default)]
    pub recursive: bool,
    pub out_dir: String,
}

struct Resolved {
    inputs: Vec<PathBuf>,
    out_dir: PathBuf,
    total: u64,
}

impl ImageToolArgs {
    fn resolve(self, config: &Config) -> CommandResult<Resolved> {
        let inputs = resolve_inputs(config, &self.inputs)?;
        let out_dir = resolve_output(config, &self.out_dir)?;
        let total = inputs.len() as u64;
        Ok(Resolved {
            inputs,
            out_dir,
            total,
        })
    }
}

#[tauri::command]
pub fn split(args: ImageToolArgs, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let r = args.resolve(&config)?;
    let params = SplitParams::new(r.inputs, r.out_dir);

    state
        .jobs
        .spawn("split", r.total, move |progress| {
            let plan = SplitTool.plan(&params)?.data;
            let summary = SplitTool.apply(plan, progress)?.data;
            Ok(format!(
                "{} halves written, {} failed",
                summary.written.len(),
                summary.failures.len()
            ))
        })
        .map_err(describe)
}

#[tauri::command]
pub fn border(args: ImageToolArgs, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let r = args.resolve(&config)?;
    let params = PrintBorderParams::new(r.inputs, r.out_dir);

    state
        .jobs
        .spawn("border", r.total, move |progress| {
            let plan = PrintBorderTool.plan(&params)?.data;
            let summary = PrintBorderTool.apply(plan, progress)?.data;
            Ok(format!(
                "{} bordered, {} failed",
                summary.written.len(),
                summary.failures.len()
            ))
        })
        .map_err(describe)
}

#[tauri::command]
pub fn tiff_to_jpeg(args: ImageToolArgs, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let r = args.resolve(&config)?;
    let params = TiffToJpegParams::new(r.inputs, r.out_dir);

    state
        .jobs
        .spawn("tiff_to_jpeg", r.total, move |progress| {
            let plan = TiffToJpegTool.plan(&params)?.data;
            let summary = TiffToJpegTool.apply(plan, progress)?.data;
            Ok(format!(
                "{} pages written, {} failed",
                summary.written.len(),
                summary.failures.len()
            ))
        })
        .map_err(describe)
}

#[derive(Debug, Deserialize)]
pub struct ContactSheetArgs {
    pub inputs: Vec<String>,
    #[serde(default)]
    pub recursive: bool,
    pub out_path: String,
}

#[tauri::command]
pub fn contact_sheet(args: ContactSheetArgs, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let inputs = resolve_inputs(&config, &args.inputs)?;
    let out_path = resolve_output(&config, &args.out_path)?;
    let total = inputs.len() as u64;

    let mut params = ContactSheetParams::new(inputs, out_path);
    params.recursive = args.recursive;

    state
        .jobs
        .spawn("contact_sheet", total, move |progress| {
            let plan = ContactSheetTool.plan(&params)?.data;
            let summary = ContactSheetTool.apply(plan, progress)?.data;
            Ok(format!(
                "sheet {}x{} from {} images, {} unreadable",
                summary.width,
                summary.height,
                summary.cells,
                summary.unreadable.len()
            ))
        })
        .map_err(describe)
}

#[derive(Debug, Deserialize)]
pub struct TransformArgs {
    pub inputs: Vec<String>,
    #[serde(default)]
    pub recursive: bool,
    pub out_dir: String,
    pub rotate_degrees: Option<f32>,
    pub max_long_edge: Option<u32>,
    pub format: Option<TargetFormat>,
    pub quality: Option<u8>,
}

#[tauri::command]
pub fn transform(args: TransformArgs, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let inputs = resolve_inputs(&config, &args.inputs)?;
    let out_dir = resolve_output(&config, &args.out_dir)?;
    let total = inputs.len() as u64;

    let mut params = TransformParams::new(inputs, out_dir);
    params.recursive = args.recursive;
    params.rotate_degrees = args.rotate_degrees;
    params.max_long_edge = args.max_long_edge;
    params.format = args.format;
    if let Some(q) = args.quality {
        params.quality = q;
    }

    state
        .jobs
        .spawn("transform", total, move |progress| {
            let plan = TransformTool.plan(&params)?.data;
            let summary = TransformTool.apply(plan, progress)?.data;
            Ok(format!(
                "{} written, {} failed",
                summary.written.len(),
                summary.failures.len()
            ))
        })
        .map_err(describe)
}

/// What the front end needs to render the shell before anything else loads.
#[derive(Debug, Serialize)]
pub struct Bootstrap {
    pub roots: Vec<String>,
    pub server: ServerSettings,
}

#[tauri::command]
pub fn bootstrap(state: State<'_, AppState>) -> Bootstrap {
    Bootstrap {
        roots: state
            .config()
            .roots
            .iter()
            .map(|r| r.to_string_lossy().to_string())
            .collect(),
        server: state.server.settings(),
    }
}
