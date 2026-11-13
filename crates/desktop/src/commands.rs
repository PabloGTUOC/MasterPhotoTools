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
use phototools_core::tools::f1_dates::{
    DateRepairAction, DateRepairParams, DateRepairTool, RepairMode, ScanResult,
};
use phototools_core::tools::f3_rename::{
    BatchRenameAction, BatchRenameParams, BatchRenamerTool, RenameOrder,
};
use phototools_core::tools::f4_split::{SplitParams, SplitSettings, SplitTool};
use phototools_core::tools::f5_contact::{ContactSheetParams, ContactSheetTool, SheetStyle};
use phototools_core::tools::f6_transform::{TargetFormat, TransformParams, TransformTool};
use phototools_core::tools::f7_border::{BorderStyle, PrintBorderParams, PrintBorderTool};
use phototools_core::tools::f8_tiff::{TiffToJpegParams, TiffToJpegTool};
use phototools_core::tools::{self, f9_browser, Plan, Tool};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

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

/// Apply the settings and keep them for the next launch.
///
/// Persisting can fail on its own — a locked Keychain, an unwritable
/// configuration directory — and that is reported rather than swallowed (G10).
/// The settings are applied to the running session either way, so a failure to
/// save costs the next launch, not this one.
#[tauri::command]
pub fn set_server_settings(
    settings: ServerSettings,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    state.server.set_settings(settings.clone());
    settings.save().map_err(|e| e.to_string())
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
    #[serde(default)]
    pub recursive: bool,
}

/// What a repair would change, without changing it.
///
/// The desktop's counterpart to `POST /api/tools/dates/plan`.
#[tauri::command]
pub fn plan_dates(
    args: FixDatesArgs,
    state: State<'_, AppState>,
) -> CommandResult<Plan<DateRepairAction>> {
    let config = state.config();
    let paths = resolve_inputs(&config, &args.paths)?;
    DateRepairTool
        .plan(&DateRepairParams {
            paths,
            mode: args.mode,
            recursive: args.recursive,
        })
        .map(|outcome| outcome.data)
        .map_err(describe)
}

#[tauri::command]
pub fn fix_dates(args: FixDatesArgs, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let paths = resolve_inputs(&config, &args.paths)?;
    let total = paths.len() as u64;
    let params = DateRepairParams {
        paths,
        mode: args.mode,
        recursive: args.recursive,
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
            // The dry-run branch above reports its skips; this one did not,
            // so "0 verified, 0 failed" was all a person saw when every file
            // had been skipped for want of a readable date.
            let skipped = plan.skipped.clone();
            let summary = DateRepairTool.apply(plan, progress)?.data;
            Ok(phototools_core::tools::f1_dates::report(&summary, &skipped))
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
    /// RS-3: name each photograph after its folder instead of the blocks.
    #[serde(default)]
    pub name_from_folder: bool,
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
            name_from_folder: self.name_from_folder,
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
            // The plan is moved into `apply`, so its skips are taken first.
            let skipped = plan.skipped.clone();
            let summary = BatchRenamerTool.apply(plan, progress)?.data;
            Ok(tools::summarise(
                summary.renamed.len(),
                "renamed",
                summary.failures.len(),
                &skipped,
                // F3 renames whatever it is given; it has no accepted list.
                &[],
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

/// A preview as the front end receives it: images as data URLs it can show.
#[derive(Debug, Serialize)]
pub struct SplitPreviewResult {
    /// The file this preview is of — the caller may have named a folder.
    pub source: String,
    pub divider_x: u32,
    pub divider_fraction: f32,
    pub cropped: PreviewImage,
    pub a: PreviewImage,
    pub b: PreviewImage,
}

#[derive(Debug, Serialize)]
pub struct PreviewImage {
    pub src: String,
    pub width: u32,
    pub height: u32,
}

impl From<phototools_core::tools::f4_split::SplitPreviewImage> for PreviewImage {
    fn from(image: phototools_core::tools::f4_split::SplitPreviewImage) -> Self {
        use base64::Engine as _;
        Self {
            src: format!(
                "data:image/jpeg;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&image.jpeg)
            ),
            width: image.width,
            height: image.height,
        }
    }
}

/// §F4's preview: the border-cropped whole image and both halves, writing
/// nothing.
///
/// Specified since F4 was written and never reachable from either build, which
/// left the one operation whose thresholds most need judging by eye runnable
/// only blind.
#[tauri::command]
pub fn split_preview(
    inputs: Vec<String>,
    recursive: bool,
    settings: Option<SplitSettings>,
    state: State<'_, AppState>,
) -> CommandResult<SplitPreviewResult> {
    let config = state.config();
    // The same inputs the apply takes, expanded the same way.
    let inputs = resolve_inputs(&config, &inputs)?;
    let thumbs = phototools_core::tools::f4_split::preview_first(
        &inputs,
        recursive,
        &settings.unwrap_or_default(),
        phototools_core::tools::f4_split::PREVIEW_MAX_EDGE,
    )
    .map_err(describe)?;

    Ok(SplitPreviewResult {
        source: thumbs.source.display().to_string(),
        divider_x: thumbs.divider_x,
        divider_fraction: thumbs.divider_fraction,
        cropped: thumbs.cropped.into(),
        a: thumbs.a.into(),
        b: thumbs.b.into(),
    })
}

/// F8's size and quality. Absent means §F8's default: 2048 px, quality 90.
#[derive(Debug, Deserialize)]
pub struct TiffArgs {
    #[serde(flatten)]
    pub tool: ImageToolArgs,
    #[serde(default)]
    pub max_long_edge: Option<u32>,
    #[serde(default)]
    pub quality: Option<u8>,
}

/// F7's one parameter. Everything else about the border is fixed by §F7.
#[derive(Debug, Deserialize)]
pub struct BorderArgs {
    #[serde(flatten)]
    pub tool: ImageToolArgs,
    #[serde(default = "yes")]
    pub trim_dark_edges: bool,
    /// How the canvas looks. Absent means §F7's fixed appearance.
    #[serde(default)]
    pub style: Option<BorderStyle>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct SplitArgs {
    #[serde(flatten)]
    pub tool: ImageToolArgs,
    #[serde(default)]
    pub settings: Option<SplitSettings>,
}

#[tauri::command]
pub fn split(args: SplitArgs, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let settings = args.settings.unwrap_or_default();
    let r = args.tool.resolve(&config)?;
    let mut params = SplitParams::new(r.inputs, r.out_dir);
    // The same settings the preview was judged with, or the defaults.
    params.settings = settings;

    state
        .jobs
        .spawn("split", r.total, move |progress| {
            let plan = SplitTool.plan(&params)?.data;
            // The plan is moved into `apply`, so its skips are taken first.
            let skipped = plan.skipped.clone();
            let summary = SplitTool.apply(plan, progress)?.data;
            // A file whose metadata could not be carried is reported beside
            // the ones that were never converted: both are things the run did
            // not fully do.
            let skipped = [skipped, summary.metadata_skipped.clone()].concat();
            Ok(tools::summarise(
                summary.written.len(),
                "halves written",
                summary.failures.len(),
                &skipped,
                &phototools_core::tools::f4_split::ACCEPTED,
            ))
        })
        .map_err(describe)
}

#[tauri::command]
pub fn border(args: BorderArgs, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let trim = args.trim_dark_edges;
    let style = args.style;
    let r = args.tool.resolve(&config)?;
    let mut params = PrintBorderParams::new(r.inputs, r.out_dir);
    params.trim_dark_edges = trim;
    if let Some(style) = style {
        params.style = style;
    }

    state
        .jobs
        .spawn("border", r.total, move |progress| {
            let plan = PrintBorderTool.plan(&params)?.data;
            // The plan is moved into `apply`, so its skips are taken first.
            let skipped = plan.skipped.clone();
            let summary = PrintBorderTool.apply(plan, progress)?.data;
            // A file whose metadata could not be carried is reported beside
            // the ones that were never converted: both are things the run did
            // not fully do.
            let skipped = [skipped, summary.metadata_skipped.clone()].concat();
            Ok(tools::summarise(
                summary.written.len(),
                "bordered",
                summary.failures.len(),
                &skipped,
                &phototools_core::tools::f7_border::ACCEPTED,
            ))
        })
        .map_err(describe)
}

#[tauri::command]
pub fn tiff_to_jpeg(args: TiffArgs, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let max_long_edge = args.max_long_edge;
    let quality = args.quality;
    let r = args.tool.resolve(&config)?;
    let mut params = TiffToJpegParams::new(r.inputs, r.out_dir);
    if let Some(edge) = max_long_edge {
        params.max_long_edge = edge;
    }
    if let Some(q) = quality {
        params.quality = q;
    }

    state
        .jobs
        .spawn("tiff_to_jpeg", r.total, move |progress| {
            let plan = TiffToJpegTool.plan(&params)?.data;
            // The plan is moved into `apply`, so its skips are taken first.
            let skipped = plan.skipped.clone();
            let summary = TiffToJpegTool.apply(plan, progress)?.data;
            // A file whose metadata could not be carried is reported beside
            // the ones that were never converted: both are things the run did
            // not fully do.
            let skipped = [skipped, summary.metadata_skipped.clone()].concat();
            Ok(tools::summarise(
                summary.written.len(),
                "pages written",
                summary.failures.len(),
                &skipped,
                &phototools_core::tools::f8_tiff::ACCEPTED,
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
    #[serde(default)]
    pub style: SheetStyle,
}

#[tauri::command]
pub fn contact_sheet(args: ContactSheetArgs, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let inputs = resolve_inputs(&config, &args.inputs)?;
    let out_path = resolve_output(&config, &args.out_path)?;
    let total = inputs.len() as u64;

    let mut params = ContactSheetParams::new(inputs, out_path);
    params.recursive = args.recursive;
    params.style = args.style;

    state
        .jobs
        .spawn("contact_sheet", total, move |progress| {
            let plan = ContactSheetTool.plan(&params)?.data;
            // The plan is moved into `apply`, so its skips are taken first.
            let skipped = plan.skipped.clone();
            let summary = ContactSheetTool.apply(plan, progress)?.data;
            // The descriptive line is better than the generic one when there
            // is a sheet; when there is not, only the generic one explains why.
            if summary.cells == 0 {
                return Ok(tools::summarise(
                    0,
                    "images",
                    0,
                    &skipped,
                    &phototools_core::tools::f5_contact::ACCEPTED,
                ));
            }
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
            // The plan is moved into `apply`, so its skips are taken first.
            let skipped = plan.skipped.clone();
            let summary = TransformTool.apply(plan, progress)?.data;
            // A file whose metadata could not be carried is reported beside
            // the ones that were never converted: both are things the run did
            // not fully do.
            let skipped = [skipped, summary.metadata_skipped.clone()].concat();
            Ok(tools::summarise(
                summary.written.len(),
                "transformed",
                summary.failures.len(),
                &skipped,
                &phototools_core::tools::f6_transform::ACCEPTED,
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
#[derive(Debug, Deserialize)]
pub struct DeliverArgs {
    /// The card, or any folder being read.
    pub path: String,
    /// Where the frames that passed should land.
    pub destination: String,
    /// Copy only the shots that passed validation, rather than everything.
    #[serde(default = "yes")]
    pub only_passing: bool,
}

/// Copy the frames that passed to a folder, keeping the camera's names.
///
/// PB-3 of `docs/publish-folder-plan.md`: where ingest ends once it no longer
/// hands a session to the server. The tools run on what lands here.
///
/// **The card is never written to (G5)** — `deliver_all` refuses a destination
/// inside the folder being read before a byte is copied, which matters because
/// this is the one operation whose destination somebody types, and the card is
/// the folder they are looking at.
#[tauri::command]
pub fn deliver_card(args: DeliverArgs, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let root = resolve_input(&config, &args.path)?;
    let destination = resolve_output(&config, &args.destination)?;
    let card = Card::at(&root).map_err(describe)?;

    // Refused here as well as inside the job, so a mistyped destination is a
    // red line under the field rather than a job that starts and then fails.
    ingest::check_destination(&root, &destination).map_err(describe)?;

    let only_passing = args.only_passing;
    let thresholds = config.thresholds.clone();

    state
        .jobs
        .spawn("card_deliver", 0, move |progress| {
            let scan = ingest::scan_card(&card, progress)?;

            let assets: Vec<_> = if only_passing {
                let validation =
                    ingest::validate(&scan.shots, chrono::Utc::now().naive_utc(), &thresholds);
                let passing: std::collections::HashSet<&str> = validation
                    .shots
                    .iter()
                    .filter(|s| !s.status().is_fail())
                    .map(|s| s.stem.as_str())
                    .collect();

                scan.shots
                    .iter()
                    .filter(|shot| passing.contains(shot.stem.as_str()))
                    .map(|shot| shot.candidate().clone())
                    .collect()
            } else {
                scan.candidates().cloned().collect()
            };

            let result = ingest::deliver_all(&assets, &root, &destination, progress)?;
            Ok(result.describe())
        })
        .map_err(describe)
}

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

// ---------------------------------------------------------------------------
// Validation — F12
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct CheckRow {
    pub rule: String,
    pub status: String,
    pub failure: Option<String>,
    pub detail: String,
}

#[derive(Debug, Serialize)]
pub struct ShotValidationRow {
    pub stem: String,
    pub status: String,
    pub checks: Vec<CheckRow>,
}

/// One failure class, with what F13 offers for it and how many shots share it.
///
/// This is what the bulk action bar renders (Phase 13): a card of four hundred
/// frames becomes a handful of rows, not four hundred prompts.
#[derive(Debug, Serialize)]
pub struct FailureGroup {
    pub failure: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct ClockOffsetRow {
    pub median: String,
    pub spread_days: i64,
    pub median_age_days: i64,
    pub shift: String,
    pub affected: usize,
}

#[derive(Debug, Serialize)]
pub struct ValidationResult {
    pub shots: Vec<ShotValidationRow>,
    pub groups: Vec<FailureGroup>,
    pub clock_offset: Option<ClockOffsetRow>,
    pub passing: usize,
    pub failing: usize,
}

/// Validate a card's shots against the three rules (F12).
/// Per-session overrides for F12's two ceilings.
///
/// Absent means the configured default. `max_megapixels: Some(0)` means no
/// resolution ceiling — publishing is limited by file size, and a frame inside
/// the byte cap is worth keeping whole. Held here rather than in `Config`
/// because it is a decision about one card, not about the installation.
#[derive(Debug, Default, Deserialize)]
pub struct ThresholdOverrides {
    #[serde(default)]
    pub max_megapixels: Option<u32>,
    #[serde(default)]
    pub max_output_bytes: Option<u64>,
}

impl ThresholdOverrides {
    fn over(
        &self,
        base: &phototools_core::config::Thresholds,
    ) -> phototools_core::config::Thresholds {
        let mut t = base.clone();
        if let Some(mp) = self.max_megapixels {
            t.max_megapixels = mp;
        }
        if let Some(bytes) = self.max_output_bytes {
            t.max_output_bytes = bytes;
        }
        t
    }
}

/// The thresholds for this request: the configuration, with any override.
fn session_thresholds(
    config: &Config,
    overrides: &Option<ThresholdOverrides>,
) -> phototools_core::config::Thresholds {
    match overrides {
        Some(o) => o.over(&config.thresholds),
        None => config.thresholds.clone(),
    }
}

#[tauri::command]
pub fn validate_card(
    path: String,
    thresholds: Option<ThresholdOverrides>,
    state: State<'_, AppState>,
) -> CommandResult<ValidationResult> {
    let config = state.config();
    let root = resolve_input(&config, &path)?;
    let card = Card::at(&root).map_err(describe)?;

    let scan = ingest::scan_card(&card, &InMemoryProgress::new()).map_err(describe)?;
    let validation = ingest::validate(
        &scan.shots,
        chrono::Utc::now().naive_utc(),
        &session_thresholds(&config, &thresholds),
    );

    let groups = validation
        .by_failure()
        .into_iter()
        .map(|(failure, indices)| FailureGroup {
            failure: failure.as_str().to_string(),
            count: indices.len(),
        })
        .collect();

    Ok(ValidationResult {
        passing: validation.passing(),
        failing: validation.failing(),
        clock_offset: validation.clock_offset.as_ref().map(|o| ClockOffsetRow {
            median: o.median.to_string(),
            spread_days: o.spread_days,
            median_age_days: o.median_age_days,
            shift: o.shift.clone(),
            affected: o.affected,
        }),
        groups,
        shots: validation
            .shots
            .iter()
            .map(|shot| ShotValidationRow {
                stem: shot.stem.clone(),
                status: shot.status().as_str().to_string(),
                checks: shot
                    .checks
                    .iter()
                    .map(|c| CheckRow {
                        rule: format!("{:?}", c.rule).to_lowercase(),
                        status: c.status.as_str().to_string(),
                        failure: c.failure.map(|f| f.as_str().to_string()),
                        detail: c.detail.clone(),
                    })
                    .collect(),
            })
            .collect(),
    })
}

// ---------------------------------------------------------------------------
// Launch at login
// ---------------------------------------------------------------------------
//
// Platform integration, which is what a binary crate is for (G1). The plugin
// writes a macOS LaunchAgent; nothing about it belongs in `core`, which has no
// platform assumptions at all.

/// Whether the application is registered to start at login.
#[tauri::command]
pub fn get_launch_at_login(app: tauri::AppHandle) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

/// Register or unregister the login item.
///
/// Reports the state it could confirm rather than the state it was asked for
/// (§9.2 invariant 6): the write can fail on its own, and answering "on"
/// because "on" was requested would be a UI that lies after a reboot.
#[tauri::command]
pub fn set_launch_at_login(app: tauri::AppHandle, enabled: bool) -> Result<bool, String> {
    use tauri_plugin_autostart::ManagerExt;

    let launcher = app.autolaunch();
    let outcome = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    outcome.map_err(|e| e.to_string())?;

    launcher.is_enabled().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Geotagging — the track library and the matching tool
//
// Beyond the specification, which mentions neither GPS nor GPX; the reasoning
// is in `docs/geotag-plan.md` and the deviation in `docs/known-gaps.md` (G9).
//
// The desktop's counterpart to `/api/tracks` and `/api/tools/geotag/*`. The
// two sides keep their own timelines, because they keep their own ledgers: the
// tracks on this Mac are the ones fed to this Mac.
// ---------------------------------------------------------------------------

use phototools_core::tools::geotag;

#[tauri::command]
pub fn list_tracks(state: State<'_, AppState>) -> CommandResult<Vec<geotag::TrackSummary>> {
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    Ok(guard
        .tracks()
        .map_err(|e| describe(e.into()))?
        .into_iter()
        .map(geotag::TrackSummary::from)
        .collect())
}

/// What importing this file would do. Writes nothing.
#[tauri::command]
pub fn preview_track_import(
    path: String,
    state: State<'_, AppState>,
) -> CommandResult<geotag::library::TrackImportPreview> {
    let config = state.config();
    let resolved = resolve_input(&config, &path)?;
    let file = geotag::library::read_track(&resolved).map_err(describe)?;

    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    geotag::library::preview_import(&guard, &file).map_err(describe)
}

#[derive(Debug, Deserialize)]
pub struct ImportTrackArgs {
    pub path: String,
    pub resolution: geotag::library::Resolution,
    #[serde(default)]
    pub overrides: Vec<geotag::library::Decision>,
}

/// Import the file, applying the decisions. One transaction.
#[tauri::command]
pub fn import_track(
    args: ImportTrackArgs,
    state: State<'_, AppState>,
) -> CommandResult<geotag::library::TrackImportResult> {
    let config = state.config();
    let resolved = resolve_input(&config, &args.path)?;
    let file = geotag::library::read_track(&resolved).map_err(describe)?;

    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    geotag::library::commit_import(
        &guard,
        &file,
        args.resolution,
        &args.overrides,
        chrono::Utc::now().timestamp(),
    )
    .map_err(describe)
}

#[derive(Debug, Deserialize)]
pub struct TimelineArgs {
    /// Unix seconds, inclusive. A window is required: a decade of fixes is a
    /// million rows and no map can draw them.
    pub from: i64,
    pub to: i64,
    /// Minutes east of UTC, for grouping the coverage into the days the person
    /// was living rather than the days UTC was having.
    #[serde(default)]
    pub offset_minutes: i32,
    /// Whether to return the fixes themselves, or only the day counts.
    ///
    /// The coverage strip spans a month and needs counts; the map draws one day
    /// and needs the fixes. Asking for a month of fixes to draw a strip of
    /// thirty dots would be megabytes for a number.
    #[serde(default = "yes")]
    pub include_points: bool,
}

#[derive(Debug, Serialize)]
pub struct TimelineView {
    pub points: Vec<geotag::SourcedPoint>,
    pub coverage: Vec<geotag::DayCoverage>,
    pub extent: Option<(i64, i64)>,
    /// Fixes reported by a phone that are not yet a track
    /// (`docs/owntracks-plan.md`). Always zero here: a phone posts to the
    /// server, not to this Mac.
    pub pending: usize,
}

/// The timeline a map draws: the fixes in a window, and the days that hold any.
#[tauri::command]
pub fn timeline(args: TimelineArgs, state: State<'_, AppState>) -> CommandResult<TimelineView> {
    let offset = i64::from(args.offset_minutes) * 60;
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    Ok(TimelineView {
        points: if args.include_points {
            guard
                .points_with_source(args.from, args.to)
                .map_err(|e| describe(e.into()))?
        } else {
            Vec::new()
        },
        coverage: guard
            .coverage(args.from, args.to, offset)
            .map_err(|e| describe(e.into()))?,
        extent: guard.timeline_extent().map_err(|e| describe(e.into()))?,
        pending: guard
            .device_fixes_between(args.from, args.to)
            .map_err(|e| describe(e.into()))?
            .len(),
    })
}

/// What placing these points would do. Writes nothing.
///
/// A pin takes the road a `.gpx` takes — written, parsed, diffed — so an instant
/// the library already holds is put to the user rather than overwritten.
#[tauri::command]
pub fn preview_placed_points(
    stops: Vec<geotag::gpx::PlacedStop>,
    state: State<'_, AppState>,
) -> CommandResult<geotag::library::TrackImportPreview> {
    let file = geotag::place::build(&stops).map_err(describe)?;
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    geotag::library::preview_import(&guard, &file).map_err(describe)
}

#[derive(Debug, Deserialize)]
pub struct PlacePointsArgs {
    pub stops: Vec<geotag::gpx::PlacedStop>,
    pub resolution: geotag::library::Resolution,
    #[serde(default)]
    pub overrides: Vec<geotag::library::Decision>,
}

/// Store the placed points, applying the decisions. One transaction.
#[tauri::command]
pub fn place_points(
    args: PlacePointsArgs,
    state: State<'_, AppState>,
) -> CommandResult<geotag::library::TrackImportResult> {
    let file = geotag::place::build(&args.stops).map_err(describe)?;
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    geotag::library::commit_import(
        &guard,
        &file,
        args.resolution,
        &args.overrides,
        chrono::Utc::now().timestamp(),
    )
    .map_err(describe)
}

#[derive(Debug, Deserialize)]
pub struct ExportTimelineArgs {
    pub from: i64,
    pub to: i64,
    pub path: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct ExportedTimeline {
    pub path: String,
    pub points: usize,
}

/// Write a window of the timeline out as a `.gpx`.
///
/// A library that cannot be got out of is a trap, and the file is the form every
/// other tool in the world reads.
#[tauri::command]
pub fn export_timeline(
    args: ExportTimelineArgs,
    state: State<'_, AppState>,
) -> CommandResult<ExportedTimeline> {
    let config = state.config();
    let resolved = resolve_output(&config, &args.path)?;
    let ledger = state.jobs.ledger();
    let points = {
        let guard = ledger.lock().map_err(|_| poisoned())?;
        guard
            .points_between(args.from, args.to)
            .map_err(|e| describe(e.into()))?
    };
    if points.is_empty() {
        return Err("there are no fixes in that window to export".into());
    }
    let text = geotag::gpx::write(&args.name, geotag::gpx::CREATOR, &points).map_err(describe)?;
    std::fs::write(&resolved, text).map_err(|e| describe(e.into()))?;
    Ok(ExportedTimeline {
        path: resolved.display().to_string(),
        points: points.len(),
    })
}

#[derive(Debug, Serialize)]
pub struct SyncReport {
    /// What happened, in one sentence for the status line.
    pub summary: String,
    pub pulled: usize,
    pub pushed: usize,
    pub deleted_here: usize,
    pub deleted_there: usize,
    pub conflicts: usize,
    pub skipped: Vec<String>,
}

/// Sync this Mac's timeline with the server's
/// (`docs/timeline-sync-plan.md`).
///
/// **Geopositions only.** Tracks, their fixes, the decisions recorded about
/// them, and deletions. No card, shot, session or publish row crosses between
/// the two machines: those are records of what one machine did.
///
/// The Mac drives, because a NAS cannot open a connection to a laptop that is
/// asleep or on somebody else's network. The work is decided in `core`; this
/// holds the ledger and the HTTP client together and reports what happened.
#[tauri::command]
pub fn sync_timeline(state: State<'_, AppState>) -> CommandResult<SyncReport> {
    let remote = state.server.session_client();

    // A long job opens its own connection to the ledger rather than holding the
    // shared mutex: `SinkProgress` locks the same non-reentrant mutex, and a
    // sync of a hundred tracks is not something to hold a lock across.
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;

    let outcome =
        geotag::sync::run(&guard, &remote, chrono::Utc::now().timestamp()).map_err(describe)?;

    Ok(SyncReport {
        summary: outcome.summary(),
        pulled: outcome.pulled,
        pushed: outcome.pushed,
        deleted_here: outcome.deleted_here,
        deleted_there: outcome.deleted_there,
        conflicts: outcome.conflicts,
        skipped: outcome.skipped,
    })
}

#[tauri::command]
pub fn delete_track(id: String, state: State<'_, AppState>) -> CommandResult<usize> {
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    guard
        .delete_track(&id, chrono::Utc::now().timestamp())
        .map_err(|e| describe(e.into()))
}

/// Every disagreement recorded against a track, and what was decided.
///
/// The audit rows exist so that a library which says something one of its own
/// stored files does not can explain itself. Written and never read, they would
/// have been reachable only by opening the database with `sqlite3`.
#[tauri::command]
pub fn track_conflicts(
    id: String,
    state: State<'_, AppState>,
) -> CommandResult<Vec<geotag::RecordedConflict>> {
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    Ok(guard
        .conflicts_for_track(&id)
        .map_err(|e| describe(e.into()))?
        .into_iter()
        .map(geotag::RecordedConflict::from)
        .collect())
}

/// The inventory. Synchronous, like the date scan: it writes nothing and the
/// table is the answer.
#[tauri::command]
pub fn scan_geo(
    path: String,
    recursive: bool,
    state: State<'_, AppState>,
) -> CommandResult<Vec<geotag::scan::GeoScanRow>> {
    let config = state.config();
    let root = resolve_input(&config, &path)?;
    geotag::scan::scan(&root, recursive).map_err(describe)
}

#[derive(Debug, Deserialize)]
pub struct GeotagArgs {
    pub paths: Vec<String>,
    #[serde(default)]
    pub recursive: bool,
    pub utc_offset_minutes: Option<i32>,
    #[serde(default)]
    pub clock_correction_seconds: i64,
    #[serde(default)]
    pub limits: geotag::join::Limits,
    #[serde(default)]
    pub overwrite_existing: bool,
    #[serde(default = "write_altitude_by_default")]
    pub write_altitude: bool,
}

/// Altitude is written unless somebody says otherwise: it is in the track, and
/// leaving out a measurement the phone took is the deviation, not keeping it.
fn write_altitude_by_default() -> bool {
    true
}

impl GeotagArgs {
    fn into_params(self, config: &Config) -> CommandResult<geotag::tool::GeotagParams> {
        Ok(geotag::tool::GeotagParams {
            paths: resolve_inputs(config, &self.paths)?,
            recursive: self.recursive,
            database: config.database.clone(),
            utc_offset_minutes: self.utc_offset_minutes,
            clock_correction_seconds: self.clock_correction_seconds,
            limits: self.limits,
            overwrite_existing: self.overwrite_existing,
            write_altitude: self.write_altitude,
        })
    }
}

/// The dry run, with the offset the photographs themselves suggest.
#[tauri::command]
pub fn plan_geotag(
    args: GeotagArgs,
    state: State<'_, AppState>,
) -> CommandResult<geotag::tool::GeotagPreview> {
    let config = state.config();
    let params = args.into_params(&config)?;
    geotag::preview(&params).map_err(describe)
}

#[tauri::command]
pub fn apply_geotag(args: GeotagArgs, state: State<'_, AppState>) -> CommandResult<String> {
    let config = state.config();
    let params = args.into_params(&config)?;
    let total = params.paths.len() as u64;

    state
        .jobs
        .spawn("geotag", total, move |progress| {
            let plan = geotag::tool::GeotagTool.plan(&params)?.data;
            Ok(geotag::tool::GeotagTool
                .apply(plan, progress)?
                .data
                .describe())
        })
        .map_err(describe)
}

/// A poisoned ledger lock: an earlier panic left it unusable.
fn poisoned() -> String {
    "The ledger lock was poisoned by an earlier failure".to_string()
}
