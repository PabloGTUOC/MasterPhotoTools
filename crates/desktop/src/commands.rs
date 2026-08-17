//! The Tauri command layer.
//!
//! Commands parse, resolve paths against the configured roots (G6), and
//! delegate to `core`. No functionality lives here (G1).

use crate::server::{ServerSettings, ServerStatus};
use crate::AppState;
use phototools_core::config::Config;
use phototools_core::error::Error;
use phototools_core::ingest::{self, Card, ScanProblem};
use phototools_core::jobs::{InMemoryProgress, Job};
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

// ---------------------------------------------------------------------------
// Ingest — F10, F11
// ---------------------------------------------------------------------------

/// What the review screen needs about one photograph.
#[derive(Debug, Serialize)]
pub struct ShotRow {
    pub stem: String,
    pub candidate_kind: String,
    pub candidate_path: String,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
    pub megapixels: f64,
    pub capture: Option<String>,
    pub camera: Option<String>,
    pub asset_count: usize,
    pub needs_derivation: bool,
}

#[derive(Debug, Serialize)]
pub struct CardScanResult {
    pub card_id: String,
    pub label: Option<String>,
    pub shots: Vec<ShotRow>,
    pub awaiting_derivation: usize,
    pub problems: Vec<ScanProblem>,
}

#[derive(Debug, Serialize)]
pub struct CardSummaryResult {
    pub path: String,
    pub card_id: String,
    pub label: Option<String>,
    pub shots: usize,
    pub new_shots: usize,
    pub seen_before: bool,
    pub looks_like_a_card: bool,
}

/// A quick look at a directory being offered as a card (F10).
///
/// Cheap by construction — directory entries and the ledger, no photographs —
/// so the UI can offer it the moment someone picks a folder.
#[tauri::command]
pub fn summarise_card(
    path: String,
    state: State<'_, AppState>,
) -> CommandResult<CardSummaryResult> {
    let config = state.config();
    let root = resolve_input(&config, &path)?;
    let card = Card::at(&root).map_err(describe)?;

    let ledger = state.jobs.ledger();
    let guard = ledger
        .lock()
        .map_err(|_| "The ledger is unavailable.".to_string())?;
    let summary = ingest::summarise_card(&card, &guard).map_err(describe)?;

    Ok(CardSummaryResult {
        path: card.root().to_string_lossy().to_string(),
        card_id: summary.card_id,
        label: summary.label,
        shots: summary.shots,
        new_shots: summary.new_shots,
        seen_before: summary.seen_before,
        looks_like_a_card: card.looks_like_a_card(),
    })
}

/// Scan a card and record it (F11).
///
/// Any directory is accepted, which is build plan §6.3's simulated card mode:
/// the command resolves a path against the configured roots (G6) and hands it
/// to `core`, which cannot tell whether a card was mounted.
#[tauri::command]
pub fn scan_card(path: String, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let root = resolve_input(&config, &path)?;
    let card = Card::at(&root).map_err(describe)?;
    let ledger = state.jobs.ledger();

    state
        .jobs
        .spawn("card_scan", 0, move |progress| {
            let scan = ingest::scan_card(&card, progress)?;
            {
                let guard = ledger
                    .lock()
                    .map_err(|_| Error::Internal("ledger lock poisoned".into()))?;
                ingest::record_scan(&scan, &guard)?;
            }
            Ok(format!(
                "{} shots, {} awaiting derivation, {} unreadable",
                scan.shot_count(),
                scan.awaiting_derivation(),
                scan.problems.len()
            ))
        })
        .map_err(describe)
}

/// Copy a card's candidates into the staging directory, verified by hash.
///
/// **Never writes to the card** (G5); the staging directory is the only
/// destination.
#[tauri::command]
pub fn stage_card(path: String, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let root = resolve_input(&config, &path)?;
    let card = Card::at(&root).map_err(describe)?;
    let staging = config.staging_dir.clone();

    state
        .jobs
        .spawn("card_stage", 0, move |progress| {
            let scan = ingest::scan_card(&card, progress)?;
            let candidates: Vec<_> = scan.candidates().cloned().collect();
            let result = ingest::stage_all(&candidates, &staging, progress);

            if result.all_verified() {
                Ok(format!("{} copied and verified", result.staged.len()))
            } else {
                Ok(format!(
                    "{} copied, {} could not be verified",
                    result.staged.len(),
                    result.failed.len()
                ))
            }
        })
        .map_err(describe)
}

/// A scan the UI can render immediately, without waiting for a job.
///
/// Used by the review screen once a scan job has finished.
#[tauri::command]
pub fn read_card(path: String, state: State<'_, AppState>) -> CommandResult<CardScanResult> {
    let config = state.config();
    let root = resolve_input(&config, &path)?;
    let card = Card::at(&root).map_err(describe)?;

    let scan = ingest::scan_card(&card, &InMemoryProgress::new()).map_err(describe)?;

    Ok(CardScanResult {
        card_id: scan.card_id.clone(),
        label: scan.label.clone(),
        awaiting_derivation: scan.awaiting_derivation(),
        problems: scan.problems.clone(),
        shots: scan
            .shots
            .iter()
            .map(|shot| {
                let candidate = shot.candidate();
                ShotRow {
                    stem: shot.stem.clone(),
                    candidate_kind: candidate.kind.as_str().to_string(),
                    candidate_path: candidate.rel_path.clone(),
                    bytes: candidate.bytes,
                    width: candidate.width,
                    height: candidate.height,
                    megapixels: candidate.megapixels(),
                    capture: candidate.capture.map(|c| c.to_string()),
                    camera: candidate.camera.clone(),
                    asset_count: shot.assets.len(),
                    needs_derivation: shot.needs_derivation,
                }
            })
            .collect(),
    })
}
