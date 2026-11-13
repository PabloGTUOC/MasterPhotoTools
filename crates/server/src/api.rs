//! HTTP surface (specification §8).
//!
//! Handlers do three things and nothing else (G1): parse, resolve every path
//! against the configured roots (G6), and delegate to `core`.

use crate::auth::Authenticated;
use crate::jobs::JobEvent;
use crate::AppState;
use axum::extract::{Path as UrlPath, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use phototools_core::config::Config;
use phototools_core::error::Error;
use phototools_core::ingest::{self, Card};
use phototools_core::jobs::{InMemoryProgress, Progress};
use phototools_core::tools;
use phototools_core::tools::geotag;
use phototools_core::tools::{f1_dates, f3_rename, f4_split, f5_contact, f6_transform};
use phototools_core::tools::{f7_border, f8_tiff, f9_browser, Tool};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::path::PathBuf;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

pub fn router() -> Router<AppState> {
    Router::new()
        // Archive tools — operate on library paths.
        .route("/api/tools/dates/scan", post(dates_scan))
        .route("/api/tools/dates/plan", post(dates_plan))
        .route("/api/tools/dates/fix", post(dates_fix))
        .route("/api/tools/rename/plan", post(rename_plan))
        .route("/api/tools/rename/apply", post(rename_apply))
        .route("/api/tools/split", post(split))
        .route("/api/tools/split/preview", post(split_preview))
        .route("/api/tools/contact-sheet", post(contact_sheet))
        .route("/api/tools/transform", post(transform))
        .route("/api/tools/border", post(border))
        .route("/api/tools/tiff-to-jpeg", post(tiff_to_jpeg))
        // Geotagging. Beyond the specification; see `docs/geotag-plan.md`.
        .route("/api/tracks", get(tracks_list))
        .route("/api/tracks/preview", post(track_preview))
        .route("/api/tracks/import", post(track_import))
        .route("/api/tracks/:id", axum::routing::delete(track_delete))
        .route("/api/tracks/:id/conflicts", get(track_conflicts))
        // The timeline, as the Timeline tab reads and adds to it
        // (`docs/timeline-plan.md`).
        .route("/api/timeline", post(timeline))
        .route("/api/timeline/place/preview", post(place_preview))
        .route("/api/timeline/place", post(place_commit))
        .route("/api/timeline/export", post(timeline_export))
        // Sync, driven by the desktop (`docs/timeline-sync-plan.md`). The
        // server is passive: it answers what it holds and accepts what it is
        // given. Geopositions only — no card, shot, session or publish row
        // crosses between the two machines.
        .route("/api/timeline/inventory", get(timeline_inventory))
        .route("/api/timeline/tracks/:id", get(timeline_track))
        .route("/api/timeline/tracks", post(timeline_track_receive))
        .route("/api/timeline/deletions", post(timeline_deletions))
        // A phone posting where it was (`docs/owntracks-plan.md`). Its own
        // credential, and a body cap: this is the one route reachable from the
        // public internet without a Firebase session.
        .route(
            "/api/timeline/owntracks",
            post(owntracks).layer(axum::extract::DefaultBodyLimit::max(64 * 1024)),
        )
        .route("/api/tools/geotag/scan", post(geotag_scan))
        .route("/api/tools/geotag/plan", post(geotag_plan))
        .route("/api/tools/geotag/apply", post(geotag_apply))
        .route("/api/storage/ls", get(storage_ls))
        .route("/api/storage/roots", get(storage_roots))
        // Ingest — F11, F12. A card is any directory (build plan §6.3).
        .route("/api/ingest/scan", post(ingest_scan))
        .route("/api/ingest/validate", post(ingest_validate))
        // Jobs.
        .route("/api/jobs/:id", get(job_state))
        .route("/api/jobs/:id/events", get(job_events))
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub code: &'static str,
    pub message: String,
}

impl ApiError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            code: "bad_request",
            message: message.into(),
        }
    }

    fn status(&self) -> StatusCode {
        match self.code {
            "path_not_allowed" | "refused" => StatusCode::FORBIDDEN,
            "not_found" => StatusCode::NOT_FOUND,
            "bad_request" => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        match e {
            // G6 rejections are reported as forbidden, and never leak whether
            // the path exists outside the roots.
            Error::AccessDenied(_) => Self {
                code: "path_not_allowed",
                message: "Path is outside the configured library roots".into(),
            },
            // A refusal that is not about a path says why it refused.
            Error::Refused(m) => Self {
                code: "refused",
                message: m,
            },
            Error::Config(m) => Self {
                code: "bad_request",
                message: m,
            },
            other => Self {
                code: "internal",
                message: other.to_string(),
            },
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status(), Json(self)).into_response()
    }
}

// ---------------------------------------------------------------------------
// G6 — path resolution
// ---------------------------------------------------------------------------

/// Resolve an input path against the configured roots.
fn resolve_input(config: &Config, path: &str) -> Result<PathBuf, ApiError> {
    Ok(config.resolve(std::path::Path::new(path))?)
}

fn resolve_inputs(config: &Config, paths: &[String]) -> Result<Vec<PathBuf>, ApiError> {
    if paths.is_empty() {
        return Err(ApiError::bad_request("No paths supplied"));
    }
    paths.iter().map(|p| resolve_input(config, p)).collect()
}

/// Resolve a destination, which need not exist yet.
fn resolve_output(config: &Config, path: &str) -> Result<PathBuf, ApiError> {
    Ok(config.resolve_for_create(std::path::Path::new(path))?)
}

// ---------------------------------------------------------------------------
// Job responses
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct JobAccepted {
    pub job_id: String,
}

/// Start a job and answer with its id. Never waits for the work (F17).
fn accept<F>(
    state: &AppState,
    auth: &Authenticated,
    kind: &str,
    total: u64,
    work: F,
) -> Result<Response, ApiError>
where
    F: FnOnce(&dyn Progress) -> Result<String, Error> + Send + 'static,
{
    let job_id = state.jobs.spawn(kind, total, work)?;
    // Who asked for what, for the audit trail.
    tracing::info!(uid = auth.uid(), kind, job_id, "job started");
    Ok((StatusCode::ACCEPTED, Json(JobAccepted { job_id })).into_response())
}

// ---------------------------------------------------------------------------
// F1 — dates
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DatesScanRequest {
    pub path: String,
    #[serde(default)]
    pub recursive: bool,
}

async fn dates_scan(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<DatesScanRequest>,
) -> Result<Response, ApiError> {
    let root = resolve_input(&state.config, &request.path)?;

    // Answered directly rather than as a job. A scan writes nothing and its
    // whole value is the table; handing back an id meant the rows were
    // computed, counted and then dropped, and no client could ever show which
    // files needed attention. §9.1 budgets 500 files under five seconds, which
    // is what makes a synchronous answer honest here.
    let results = f1_dates::scan_dates(&root, request.recursive)?;
    Ok(Json(results).into_response())
}

#[derive(Debug, Deserialize)]
pub struct DatesFixRequest {
    pub paths: Vec<String>,
    pub mode: f1_dates::RepairMode,
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub recursive: bool,
}

/// What a repair would change, without changing it.
///
/// A plan rather than a job, as `rename/plan` is: it writes nothing, and the
/// per-file detail is the whole point. Returning counts meant a person could
/// see that four dates would move but never to what — which is exactly the
/// judgement MV-9.3 asks them to make before applying a clock offset.
async fn dates_plan(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<DatesFixRequest>,
) -> Result<Response, ApiError> {
    let paths = resolve_inputs(&state.config, &request.paths)?;
    let plan = f1_dates::DateRepairTool
        .plan(&f1_dates::DateRepairParams {
            paths,
            mode: request.mode,
            recursive: request.recursive,
        })?
        .data;
    Ok(Json(plan).into_response())
}

async fn dates_fix(
    auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<DatesFixRequest>,
) -> Result<Response, ApiError> {
    let paths = resolve_inputs(&state.config, &request.paths)?;
    let total = paths.len() as u64;
    let params = f1_dates::DateRepairParams {
        paths,
        mode: request.mode,
        recursive: request.recursive,
    };
    let dry_run = request.dry_run;

    accept(&state, &auth, "dates_fix", total, move |progress| {
        let plan = f1_dates::DateRepairTool.plan(&params)?.data;

        if dry_run {
            return Ok(format!(
                "dry run: {} files would be redated, {} skipped",
                plan.actions.len(),
                plan.skipped.len()
            ));
        }

        // The dry-run branch above reports its skips; this one did not, so
        // "0 verified, 0 failed" was all a person saw when every file had been
        // skipped for want of a readable date.
        let skipped = plan.skipped.clone();
        let summary = f1_dates::DateRepairTool.apply(plan, progress)?.data;
        Ok(f1_dates::report(&summary, &skipped))
    })
}

// ---------------------------------------------------------------------------
// F3 — rename
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RenameRequest {
    pub paths: Vec<String>,
    pub date: Option<String>,
    pub subject: Option<String>,
    pub camera: Option<String>,
    pub film: Option<String>,
    #[serde(default = "capture_order")]
    pub order: f3_rename::RenameOrder,
    /// RS-3: name each photograph after its folder instead of the blocks.
    #[serde(default)]
    pub name_from_folder: bool,
}

fn capture_order() -> f3_rename::RenameOrder {
    f3_rename::RenameOrder::Capture
}

impl RenameRequest {
    fn into_params(self, config: &Config) -> Result<f3_rename::BatchRenameParams, ApiError> {
        Ok(f3_rename::BatchRenameParams {
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

/// The dry run is synchronous: it reads metadata and touches nothing.
async fn rename_plan(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<RenameRequest>,
) -> Result<Response, ApiError> {
    let params = request.into_params(&state.config)?;
    let plan = f3_rename::BatchRenamerTool.plan(&params)?.data;
    Ok(Json(plan).into_response())
}

async fn rename_apply(
    auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<RenameRequest>,
) -> Result<Response, ApiError> {
    let params = request.into_params(&state.config)?;
    let total = params.paths.len() as u64;

    accept(&state, &auth, "rename_apply", total, move |progress| {
        let plan = f3_rename::BatchRenamerTool.plan(&params)?.data;
        // The plan is moved into `apply`, so its skips are taken first.
        let skipped = plan.skipped.clone();
        let summary = f3_rename::BatchRenamerTool.apply(plan, progress)?.data;
        Ok(tools::summarise(
            summary.renamed.len(),
            "renamed",
            summary.failures.len(),
            &skipped,
            // F3 renames whatever it is given; it has no accepted list.
            &[],
        ))
    })
}

// ---------------------------------------------------------------------------
// F4–F8 — the image tools
// ---------------------------------------------------------------------------

/// The shape every image tool's request shares.
#[derive(Debug, Deserialize)]
pub struct ImageToolRequest {
    pub inputs: Vec<String>,
    #[serde(default)]
    pub recursive: bool,
    pub out_dir: String,
}

struct ResolvedInputs {
    inputs: Vec<PathBuf>,
    out_dir: PathBuf,
    total: u64,
}

impl ImageToolRequest {
    fn resolve(self, config: &Config) -> Result<ResolvedInputs, ApiError> {
        let inputs = resolve_inputs(config, &self.inputs)?;
        let out_dir = resolve_output(config, &self.out_dir)?;
        let total = inputs.len() as u64;
        Ok(ResolvedInputs {
            inputs,
            out_dir,
            total,
        })
    }
}

async fn split(
    auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<SplitRequest>,
) -> Result<Response, ApiError> {
    let settings = request.settings.unwrap_or_default();
    let r = request.tool.resolve(&state.config)?;
    let mut params = f4_split::SplitParams::new(r.inputs, r.out_dir);
    // The same settings the preview was judged with, or the defaults.
    params.settings = settings;

    accept(&state, &auth, "split", r.total, move |progress| {
        let plan = f4_split::SplitTool.plan(&params)?.data;
        // The plan is moved into `apply`, so its skips are taken first.
        let skipped = plan.skipped.clone();
        let summary = f4_split::SplitTool.apply(plan, progress)?.data;
        // A file whose metadata could not be carried is reported beside
        // the ones that were never converted: both are things the run did
        // not fully do.
        let skipped = [skipped, summary.metadata_skipped.clone()].concat();
        Ok(tools::summarise(
            summary.written.len(),
            "halves written",
            summary.failures.len(),
            &skipped,
            &f4_split::ACCEPTED,
        ))
    })
}

#[derive(Debug, Deserialize)]
pub struct ContactSheetRequest {
    pub inputs: Vec<String>,
    #[serde(default)]
    pub recursive: bool,
    pub out_path: String,
    #[serde(default)]
    pub style: f5_contact::SheetStyle,
}

/// A preview as the front ends receive it.
///
/// The images are data URLs rather than bytes: both builds put them straight
/// into an `<img src>`, and a preview that needs a second request per image
/// would be three more round trips for something the person is looking at
/// once.
#[derive(Debug, Serialize)]
pub struct SplitPreviewResponse {
    /// The file this preview is of — the caller may have named a folder.
    pub source: String,
    pub divider_x: u32,
    pub divider_fraction: f32,
    pub cropped: PreviewImageResponse,
    pub a: PreviewImageResponse,
    pub b: PreviewImageResponse,
}

#[derive(Debug, Serialize)]
pub struct PreviewImageResponse {
    /// `data:image/jpeg;base64,...`
    pub src: String,
    pub width: u32,
    pub height: u32,
}

impl From<f4_split::SplitPreviewImage> for PreviewImageResponse {
    fn from(image: f4_split::SplitPreviewImage) -> Self {
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

impl From<f4_split::SplitPreviewThumbs> for SplitPreviewResponse {
    fn from(thumbs: f4_split::SplitPreviewThumbs) -> Self {
        Self {
            source: thumbs.source.display().to_string(),
            divider_x: thumbs.divider_x,
            divider_fraction: thumbs.divider_fraction,
            cropped: thumbs.cropped.into(),
            a: thumbs.a.into(),
            b: thumbs.b.into(),
        }
    }
}

/// §F4's preview: the border-cropped whole image and both halves, writing
/// nothing.
///
/// Specified since F4 was written and never reachable — `preview()` existed in
/// `core` and only `apply` called it, so the one operation whose thresholds
/// most need judging by eye could only be run blind.
async fn split_preview(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<SplitPreviewRequest>,
) -> Result<Response, ApiError> {
    // The same inputs the apply takes, expanded the same way: a preview of
    // something the run would not touch is worse than no preview.
    let inputs = resolve_inputs(&state.config, &request.inputs)?;
    let thumbs = f4_split::preview_first(
        &inputs,
        request.recursive,
        &request.settings.unwrap_or_default(),
        f4_split::PREVIEW_MAX_EDGE,
    )?;
    Ok(Json(SplitPreviewResponse::from(thumbs)).into_response())
}

/// An image-tool request carrying F8's size and quality.
///
/// §F8 fixes 2048 px and quality 90 for scanner output being sent somewhere,
/// and both were unreachable — so a 36 MP camera TIFF lost 92% of its pixels
/// with no way to ask otherwise. Absent means the specification's default.
#[derive(Debug, Deserialize)]
pub struct TiffRequest {
    #[serde(flatten)]
    pub tool: ImageToolRequest,
    #[serde(default)]
    pub max_long_edge: Option<u32>,
    #[serde(default)]
    pub quality: Option<u8>,
}

/// An image-tool request carrying F7's one parameter.
///
/// Everything else about the border is fixed by §F7 — the 3000 px canvas, the
/// 50 px margin, the 2% radius — and deliberately so. This is the only choice
/// there is, and it was not reachable: a genuinely dark photograph mistaken
/// for a scan border could not be rescued by turning the trim off, which is
/// exactly what MV-4.2 asks somebody to watch for.
#[derive(Debug, Deserialize)]
pub struct BorderRequest {
    #[serde(flatten)]
    pub tool: ImageToolRequest,
    /// Trim dark scan edges before placing the image. Defaults to on, as F7 does.
    #[serde(default = "default_true")]
    pub trim_dark_edges: bool,
    /// How the canvas looks. Absent means §F7's fixed appearance.
    #[serde(default)]
    pub style: Option<f7_border::BorderStyle>,
}

fn default_true() -> bool {
    true
}

/// An image-tool request carrying F4's thresholds.
///
/// Flattened so the shape stays the one every image tool shares, with the
/// split's own settings alongside rather than inside it.
#[derive(Debug, Deserialize)]
pub struct SplitRequest {
    #[serde(flatten)]
    pub tool: ImageToolRequest,
    #[serde(default)]
    pub settings: Option<f4_split::SplitSettings>,
}

#[derive(Debug, Deserialize)]
pub struct SplitPreviewRequest {
    pub inputs: Vec<String>,
    #[serde(default)]
    pub recursive: bool,
    #[serde(default)]
    pub settings: Option<f4_split::SplitSettings>,
}

async fn contact_sheet(
    auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<ContactSheetRequest>,
) -> Result<Response, ApiError> {
    let inputs = resolve_inputs(&state.config, &request.inputs)?;
    let out_path = resolve_output(&state.config, &request.out_path)?;
    let total = inputs.len() as u64;

    let mut params = f5_contact::ContactSheetParams::new(inputs, out_path);
    params.recursive = request.recursive;
    params.style = request.style;

    accept(&state, &auth, "contact_sheet", total, move |progress| {
        let plan = f5_contact::ContactSheetTool.plan(&params)?.data;
        // The plan is moved into `apply`, so its skips are taken first.
        let skipped = plan.skipped.clone();
        let summary = f5_contact::ContactSheetTool.apply(plan, progress)?.data;
        // The descriptive line is better than the generic one when there is
        // a sheet; when there is not, only the generic one explains why.
        if summary.cells == 0 {
            return Ok(tools::summarise(
                0,
                "images",
                0,
                &skipped,
                &f5_contact::ACCEPTED,
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
}

#[derive(Debug, Deserialize)]
pub struct TransformRequest {
    #[serde(flatten)]
    pub common: ImageToolRequest,
    pub rotate_degrees: Option<f32>,
    pub max_long_edge: Option<u32>,
    pub format: Option<f6_transform::TargetFormat>,
    pub quality: Option<u8>,
}

async fn transform(
    auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<TransformRequest>,
) -> Result<Response, ApiError> {
    let recursive = request.common.recursive;
    let r = request.common.resolve(&state.config)?;

    let mut params = f6_transform::TransformParams::new(r.inputs, r.out_dir);
    params.recursive = recursive;
    params.rotate_degrees = request.rotate_degrees;
    params.max_long_edge = request.max_long_edge;
    params.format = request.format;
    if let Some(q) = request.quality {
        params.quality = q;
    }

    accept(&state, &auth, "transform", r.total, move |progress| {
        let plan = f6_transform::TransformTool.plan(&params)?.data;
        // The plan is moved into `apply`, so its skips are taken first.
        let skipped = plan.skipped.clone();
        let summary = f6_transform::TransformTool.apply(plan, progress)?.data;
        // A file whose metadata could not be carried is reported beside
        // the ones that were never converted: both are things the run did
        // not fully do.
        let skipped = [skipped, summary.metadata_skipped.clone()].concat();
        Ok(tools::summarise(
            summary.written.len(),
            "transformed",
            summary.failures.len(),
            &skipped,
            &f6_transform::ACCEPTED,
        ))
    })
}

async fn border(
    auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<BorderRequest>,
) -> Result<Response, ApiError> {
    let recursive = request.tool.recursive;
    let trim = request.trim_dark_edges;
    let style = request.style;
    let r = request.tool.resolve(&state.config)?;

    let mut params = f7_border::PrintBorderParams::new(r.inputs, r.out_dir);
    params.recursive = recursive;
    params.trim_dark_edges = trim;
    if let Some(style) = style {
        params.style = style;
    }

    accept(&state, &auth, "border", r.total, move |progress| {
        let plan = f7_border::PrintBorderTool.plan(&params)?.data;
        // The plan is moved into `apply`, so its skips are taken first.
        let skipped = plan.skipped.clone();
        let summary = f7_border::PrintBorderTool.apply(plan, progress)?.data;
        // A file whose metadata could not be carried is reported beside
        // the ones that were never converted: both are things the run did
        // not fully do.
        let skipped = [skipped, summary.metadata_skipped.clone()].concat();
        Ok(tools::summarise(
            summary.written.len(),
            "bordered",
            summary.failures.len(),
            &skipped,
            &f7_border::ACCEPTED,
        ))
    })
}

async fn tiff_to_jpeg(
    auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<TiffRequest>,
) -> Result<Response, ApiError> {
    let recursive = request.tool.recursive;
    let max_long_edge = request.max_long_edge;
    let quality = request.quality;
    let r = request.tool.resolve(&state.config)?;

    let mut params = f8_tiff::TiffToJpegParams::new(r.inputs, r.out_dir);
    params.recursive = recursive;
    if let Some(edge) = max_long_edge {
        params.max_long_edge = edge;
    }
    if let Some(q) = quality {
        params.quality = q;
    }

    accept(&state, &auth, "tiff_to_jpeg", r.total, move |progress| {
        let plan = f8_tiff::TiffToJpegTool.plan(&params)?.data;
        // The plan is moved into `apply`, so its skips are taken first.
        let skipped = plan.skipped.clone();
        let summary = f8_tiff::TiffToJpegTool.apply(plan, progress)?.data;
        // A file whose metadata could not be carried is reported beside the
        // ones that were never converted: both are things the run did not
        // fully do.
        let skipped = [skipped, summary.metadata_skipped.clone()].concat();
        Ok(tools::summarise(
            summary.written.len(),
            "pages written",
            summary.failures.len(),
            &skipped,
            &f8_tiff::ACCEPTED,
        ))
    })
}

// ---------------------------------------------------------------------------
// Geotagging — the track library and the matching tool
//
// Beyond the specification, which mentions neither GPS nor GPX; the reasoning
// is in `docs/geotag-plan.md` and the deviation in `docs/known-gaps.md` (G9).
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct TrackPathRequest {
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct TrackImportRequest {
    pub path: String,
    pub resolution: geotag::library::Resolution,
    #[serde(default)]
    pub overrides: Vec<geotag::library::Decision>,
}

async fn tracks_list(
    _auth: Authenticated,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    let rows: Vec<geotag::TrackSummary> = guard
        .tracks()
        .map_err(Error::Sqlite)?
        .into_iter()
        .map(geotag::TrackSummary::from)
        .collect();
    Ok(Json(rows).into_response())
}

/// What importing this file would do. Writes nothing.
async fn track_preview(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<TrackPathRequest>,
) -> Result<Response, ApiError> {
    let path = resolve_input(&state.config, &request.path)?;
    let file = geotag::library::read_track(&path)?;

    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    Ok(Json(geotag::library::preview_import(&guard, &file)?).into_response())
}

/// Import the file, applying the decisions. One transaction.
async fn track_import(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<TrackImportRequest>,
) -> Result<Response, ApiError> {
    let path = resolve_input(&state.config, &request.path)?;
    let file = geotag::library::read_track(&path)?;

    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    let result = geotag::library::commit_import(
        &guard,
        &file,
        request.resolution,
        &request.overrides,
        chrono::Utc::now().timestamp(),
    )?;
    Ok(Json(result).into_response())
}

#[derive(Debug, Deserialize)]
pub struct TimelineRequest {
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

fn yes() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct PlaceRequest {
    pub stops: Vec<geotag::gpx::PlacedStop>,
    pub resolution: geotag::library::Resolution,
    #[serde(default)]
    pub overrides: Vec<geotag::library::Decision>,
}

#[derive(Debug, Deserialize)]
pub struct TimelineExportRequest {
    pub from: i64,
    pub to: i64,
    /// Where to write it. Canonicalised against the roots like any other path
    /// this server writes to (G6).
    pub path: String,
    pub name: String,
}

/// The timeline a map draws: the fixes in a window, and the days around it that
/// hold any.
async fn timeline(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<TimelineRequest>,
) -> Result<Response, ApiError> {
    let offset = i64::from(request.offset_minutes) * 60;
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    Ok(Json(serde_json::json!({
        "points": if request.include_points {
            guard
                .points_with_source(request.from, request.to)
                .map_err(Error::Sqlite)?
        } else {
            Vec::new()
        },
        "coverage": guard
            .coverage(request.from, request.to, offset)
            .map_err(Error::Sqlite)?,
        "extent": guard.timeline_extent().map_err(Error::Sqlite)?,
        // Fixes the phone has reported that are not yet a track, for the day
        // being looked at. Without this a phone can be reporting perfectly and
        // the screen shows nothing until the day ends — which is
        // indistinguishable from a phone that is not reporting at all.
        "pending": guard
            .device_fixes_between(request.from, request.to)
            .map_err(Error::Sqlite)?
            .len(),
    }))
    .into_response())
}

/// What placing these points would do. Writes nothing.
///
/// A pin takes the road a `.gpx` takes — written, parsed, diffed — so that an
/// instant the library already holds is put to the user rather than overwritten
/// by whoever clicked last.
async fn place_preview(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(stops): Json<Vec<geotag::gpx::PlacedStop>>,
) -> Result<Response, ApiError> {
    let file = geotag::place::build(&stops)?;
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    Ok(Json(geotag::library::preview_import(&guard, &file)?).into_response())
}

/// Store the placed points, applying the decisions. One transaction.
async fn place_commit(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<PlaceRequest>,
) -> Result<Response, ApiError> {
    let file = geotag::place::build(&request.stops)?;
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    let result = geotag::library::commit_import(
        &guard,
        &file,
        request.resolution,
        &request.overrides,
        chrono::Utc::now().timestamp(),
    )?;
    Ok(Json(result).into_response())
}

/// Write a window of the timeline out as a `.gpx`.
///
/// A library that cannot be got out of is a trap, and the file is the form
/// every other tool in the world reads.
async fn timeline_export(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<TimelineExportRequest>,
) -> Result<Response, ApiError> {
    let path = resolve_output(&state.config, &request.path)?;
    let ledger = state.jobs.ledger();
    let points = {
        let guard = ledger.lock().map_err(|_| poisoned())?;
        guard
            .points_between(request.from, request.to)
            .map_err(Error::Sqlite)?
    };
    if points.is_empty() {
        return Err(ApiError {
            code: "empty",
            message: "there are no fixes in that window to export".into(),
        });
    }
    let text = geotag::gpx::write(&request.name, geotag::gpx::CREATOR, &points)?;
    std::fs::write(&path, text).map_err(Error::Io)?;
    Ok(Json(serde_json::json!({
        "path": path.display().to_string(),
        "points": points.len(),
    }))
    .into_response())
}

/// What this side holds, for the machine that drives the sync
/// (`docs/timeline-sync-plan.md`).
///
/// Cheap by design: stamps and tombstones, no points and no GPX text. The text
/// is fetched only for the tracks that turn out to be missing.
async fn timeline_inventory(
    _auth: Authenticated,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    Ok(Json(geotag::sync::inventory(&guard)?).into_response())
}

/// One track, with its text and the decisions recorded against it.
async fn timeline_track(
    _auth: Authenticated,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Response, ApiError> {
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    Ok(Json(geotag::sync::transfer_of(&guard, &id)?).into_response())
}

/// Accept a track from the other machine.
///
/// By `commit_import`, like every other track: an instant already held is a
/// disagreement to record, not something to overwrite because it arrived later.
async fn timeline_track_receive(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(transfer): Json<geotag::sync::TrackTransfer>,
) -> Result<Response, ApiError> {
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    let result = geotag::sync::receive(&guard, &transfer, chrono::Utc::now().timestamp())?;
    Ok(Json(result).into_response())
}

/// Accept deletions from the other machine.
///
/// The tombstone's own date is written, not this server's clock: it is the
/// moment somebody deleted the track, and it is what a later re-import has to
/// beat.
async fn timeline_deletions(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(tombstones): Json<Vec<geotag::sync::Tombstone>>,
) -> Result<Response, ApiError> {
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    let mut removed = 0usize;
    for stone in &tombstones {
        removed += guard
            .delete_track(&stone.id, stone.deleted_at)
            .map_err(Error::Sqlite)?;
    }
    Ok(Json(serde_json::json!({
        "tracks": tombstones.len(),
        "points_removed": removed,
    }))
    .into_response())
}

/// One or many reports. OwnTracks posts one; a catch-up may batch them.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OwnTracksBody {
    One(geotag::owntracks::Report),
    Many(Vec<geotag::owntracks::Report>),
}

/// Where a phone posts its positions (`docs/owntracks-plan.md`).
///
/// **The second route Firebase does not guard**, `/api/health` being the first,
/// because a phone has no Firebase session. So it carries its own credential
/// and a deliberately tiny blast radius: it reads six fields out of a capped
/// body, writes them to a staging table, and can do nothing else. No path, no
/// filesystem, no query, nothing to read back. The worst a stolen token buys is
/// false positions in the timeline.
///
/// Answers `[]`, which is what OwnTracks expects and means "no commands".
async fn owntracks(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let Some(credential) = state.auth.device.as_ref() else {
        // Not configured is not "allow": the route is reachable from the public
        // internet and a feature nobody set up must be one nobody can use.
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "code": "not_configured" })),
        )
            .into_response();
    };

    let offered = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    if !credential.matches(offered) {
        // Says nothing about which half was wrong.
        tracing::warn!("a position report was refused: the credential did not match");
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "code": "unauthorized" })),
        )
            .into_response();
    }

    let reports = match serde_json::from_slice::<OwnTracksBody>(&body) {
        Ok(OwnTracksBody::One(report)) => vec![report],
        Ok(OwnTracksBody::Many(reports)) => reports,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "code": "bad_request", "message": e.to_string() })),
            )
                .into_response()
        }
    };

    let now = chrono::Utc::now().timestamp();
    let ledger = state.jobs.ledger();
    let Ok(guard) = ledger.lock() else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "code": "poisoned" })),
        )
            .into_response();
    };

    for report in &reports {
        match geotag::owntracks::read(report) {
            Ok(geotag::owntracks::Accepted::Fix { point, device }) => {
                match guard.record_device_fix(&point, device.as_deref(), now) {
                    // At info, and without the coordinates: somebody testing a
                    // phone needs to see that a fix arrived, and a server log is
                    // a worse place to keep a position than the database is.
                    Ok(stored) => tracing::info!(
                        at = point.at,
                        device = device.as_deref().unwrap_or("unnamed"),
                        new = stored,
                        "a position was reported"
                    ),
                    Err(e) => tracing::error!(error = %e, "could not store a reported position"),
                }
            }
            // Kept out of the logs at warn level: a phone reports geofence
            // crossings and last-will messages as a matter of course, and a
            // warning per message would bury the ones that matter.
            Ok(geotag::owntracks::Accepted::Ignored { kind }) => {
                tracing::debug!(kind = %kind, "a report that is not a location");
            }
            Err(e) => tracing::warn!(error = %e, "a location report could not be read"),
        }
    }

    // OwnTracks reads the reply as a list of commands. An empty one is "carry
    // on", and anything else would be this server telling a phone what to do.
    Json(serde_json::json!([])).into_response()
}

async fn track_delete(
    _auth: Authenticated,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Response, ApiError> {
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    let removed = guard
        .delete_track(&id, chrono::Utc::now().timestamp())
        .map_err(Error::Sqlite)?;
    Ok(Json(serde_json::json!({ "points_removed": removed })).into_response())
}

/// Every disagreement recorded against a track, and what was decided.
///
/// The audit rows exist so that a library which says something one of its own
/// stored files does not can explain itself. Written and never read, they would
/// have been reachable only by opening the database with `sqlite3`.
async fn track_conflicts(
    _auth: Authenticated,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Response, ApiError> {
    let ledger = state.jobs.ledger();
    let guard = ledger.lock().map_err(|_| poisoned())?;
    let rows: Vec<geotag::RecordedConflict> = guard
        .conflicts_for_track(&id)
        .map_err(Error::Sqlite)?
        .into_iter()
        .map(geotag::RecordedConflict::from)
        .collect();
    Ok(Json(rows).into_response())
}

/// The inventory. Reads metadata, writes nothing, and answers with the table
/// rather than a job id — the same reasoning as the date scan.
async fn geotag_scan(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<DatesScanRequest>,
) -> Result<Response, ApiError> {
    let root = resolve_input(&state.config, &request.path)?;
    Ok(Json(geotag::scan::scan(&root, request.recursive)?).into_response())
}

#[derive(Debug, Deserialize)]
pub struct GeotagRequest {
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

impl GeotagRequest {
    fn into_params(self, config: &Config) -> Result<geotag::tool::GeotagParams, ApiError> {
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
///
/// The suggestion travels with the plan rather than living behind a button of
/// its own: the moment somebody needs it is the moment they are looking at a
/// table of refusals, and a number they have to go and ask for separately is a
/// number they will not ask for.
async fn geotag_plan(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<GeotagRequest>,
) -> Result<Response, ApiError> {
    let params = request.into_params(&state.config)?;
    Ok(Json(geotag::preview(&params)?).into_response())
}

async fn geotag_apply(
    auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<GeotagRequest>,
) -> Result<Response, ApiError> {
    let params = request.into_params(&state.config)?;
    let total = params.paths.len() as u64;

    accept(&state, &auth, "geotag", total, move |progress| {
        let plan = geotag::tool::GeotagTool.plan(&params)?.data;
        Ok(geotag::tool::GeotagTool
            .apply(plan, progress)?
            .data
            .describe())
    })
}

/// A poisoned ledger lock: an earlier panic left it unusable.
fn poisoned() -> ApiError {
    ApiError {
        code: "internal",
        message: "The ledger lock was poisoned by an earlier failure".into(),
    }
}

// ---------------------------------------------------------------------------
// F9 — storage listing
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ListQuery {
    pub path: String,
}

async fn storage_ls(
    _auth: Authenticated,
    State(state): State<AppState>,
    Query(query): Query<ListQuery>,
) -> Result<Response, ApiError> {
    // f9_browser resolves internally; the listing is cheap and synchronous.
    let entries = f9_browser::list_directory(&state.config, std::path::Path::new(&query.path))?;
    Ok(Json(entries).into_response())
}

/// The directories a browser may start from.
///
/// Without this a folder picker has nowhere to begin: G6 refuses any path
/// outside a configured root, `/` included, so "list the top" is not a question
/// the filesystem can answer — only the configuration can. Returning them is
/// not a disclosure: a caller is already authenticated, and every path it could
/// reach is reachable by walking `ls` from a root it guessed.
async fn storage_roots(
    _auth: Authenticated,
    State(state): State<AppState>,
) -> Result<Response, ApiError> {
    let roots: Vec<&std::path::Path> = state.config.roots.iter().map(|r| r.as_path()).collect();
    Ok(Json(roots).into_response())
}

// ---------------------------------------------------------------------------
// Ingest — F11, F12, F13
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CardRequest {
    pub path: String,
}

/// Validation takes the ceilings with it; a scan has none to take.
#[derive(Debug, Deserialize)]
pub struct ValidateRequest {
    pub path: String,
    #[serde(default)]
    pub thresholds: Option<ThresholdOverrides>,
}

/// Scan a card (F11). Any directory is accepted — build plan §6.3.
async fn ingest_scan(
    auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<CardRequest>,
) -> Result<Response, ApiError> {
    let root = resolve_input(&state.config, &request.path)?;
    let card = Card::at(&root)?;
    let ledger_path = state.config.database.clone();

    accept(&state, &auth, "card_scan", 0, move |progress| {
        let scan = ingest::scan_card(&card, progress)?;
        let ledger = phototools_core::ledger::Ledger::open(&ledger_path)
            .map_err(|e| Error::Internal(e.to_string()))?;
        ingest::record_scan(&scan, &ledger)?;
        Ok(format!(
            "{} shots, {} awaiting derivation",
            scan.shot_count(),
            scan.awaiting_derivation()
        ))
    })
}

#[derive(Debug, Serialize)]
pub struct ValidationResponse {
    pub shots: Vec<ShotVerdict>,
    pub groups: Vec<FailureGroup>,
    pub clock_offset: Option<ClockOffsetResponse>,
    pub passing: usize,
    pub failing: usize,
}

#[derive(Debug, Serialize)]
pub struct ShotVerdict {
    pub stem: String,
    pub status: String,
    pub checks: Vec<CheckResponse>,
}

#[derive(Debug, Serialize)]
pub struct CheckResponse {
    pub rule: String,
    pub status: String,
    pub failure: Option<String>,
    pub detail: String,
}

/// One failure class and what F13 offers for it.
#[derive(Debug, Serialize)]
pub struct FailureGroup {
    pub failure: String,
    pub count: usize,
}

#[derive(Debug, Serialize)]
pub struct ClockOffsetResponse {
    pub median: String,
    pub spread_days: i64,
    pub median_age_days: i64,
    pub shift: String,
    pub affected: usize,
}

/// Per-session overrides for F12's two ceilings.
///
/// Absent means the configured default. `max_megapixels: Some(0)` means no
/// resolution ceiling — publishing is limited by file size, and a frame inside
/// the byte cap is worth keeping whole. A decision about one card, not about
/// the installation, which is why it travels with the request.
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

fn session_thresholds(
    config: &Config,
    overrides: &Option<ThresholdOverrides>,
) -> phototools_core::config::Thresholds {
    match overrides {
        Some(o) => o.over(&config.thresholds),
        None => config.thresholds.clone(),
    }
}

/// Validate a card against the three rules (F12).
///
/// Synchronous: validation reads no pixels, so it is fast even on a full card.
async fn ingest_validate(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<ValidateRequest>,
) -> Result<Response, ApiError> {
    let root = resolve_input(&state.config, &request.path)?;
    let card = Card::at(&root)?;

    let scan = ingest::scan_card(&card, &InMemoryProgress::new())?;
    let validation = ingest::validate(
        &scan.shots,
        chrono::Utc::now().naive_utc(),
        &session_thresholds(&state.config, &request.thresholds),
    );

    let groups = validation
        .by_failure()
        .into_iter()
        .map(|(failure, indices)| FailureGroup {
            failure: failure.as_str().to_string(),
            count: indices.len(),
        })
        .collect();

    let body = ValidationResponse {
        passing: validation.passing(),
        failing: validation.failing(),
        clock_offset: validation
            .clock_offset
            .as_ref()
            .map(|o| ClockOffsetResponse {
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
            .map(|shot| ShotVerdict {
                stem: shot.stem.clone(),
                status: shot.status().as_str().to_string(),
                checks: shot
                    .checks
                    .iter()
                    .map(|c| CheckResponse {
                        rule: format!("{:?}", c.rule).to_lowercase(),
                        status: c.status.as_str().to_string(),
                        failure: c.failure.map(|f| f.as_str().to_string()),
                        detail: c.detail.clone(),
                    })
                    .collect(),
            })
            .collect(),
    };

    Ok(Json(body).into_response())
}

// ---------------------------------------------------------------------------
// Jobs
// ---------------------------------------------------------------------------

async fn job_state(
    _auth: Authenticated,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<Response, ApiError> {
    match state.jobs.get(&id)? {
        Some(job) => Ok(Json(job).into_response()),
        None => Err(ApiError {
            code: "not_found",
            message: format!("No job {id}"),
        }),
    }
}

async fn job_events(
    _auth: Authenticated,
    State(state): State<AppState>,
    UrlPath(id): UrlPath<String>,
) -> Result<impl IntoResponse, ApiError> {
    let job = state.jobs.get(&id)?.ok_or(ApiError {
        code: "not_found",
        message: format!("No job {id}"),
    })?;

    // A job that already finished gets one terminal event rather than a stream
    // that will never speak.
    let replay = if job.status.is_terminal() {
        Some(JobEvent {
            id: job.id.clone(),
            kind: job.kind.clone(),
            state: job.status.as_str().to_string(),
            progress: job.progress,
            total: job.total,
            // What the job actually reported, not the word "done": a client
            // that subscribed late is the one that most needs telling.
            message: job
                .summary
                .clone()
                .or_else(|| job.error.clone())
                .unwrap_or_else(|| "done".into()),
            terminal: true,
        })
    } else {
        None
    };

    let live = state.jobs.subscribe(&id);

    let stream = async_stream::stream! {
        if let Some(event) = replay {
            yield Ok::<Event, Infallible>(sse_event(&event));
            return;
        }

        if let Some(rx) = live {
            let mut stream = BroadcastStream::new(rx);
            while let Some(item) = stream.next().await {
                let Ok(event) = item else { continue };
                let terminal = event.terminal;
                yield Ok::<Event, Infallible>(sse_event(&event));
                if terminal {
                    break;
                }
            }
        }
    };

    // Behind nginx — nginx-proxy-manager, in front of a Cloudflare tunnel, is how
    // this server is reached from outside — a response is buffered by default,
    // so progress arrived all at once when the job ended. This header is nginx's
    // own per-response switch; sent from here, no proxy needs configuring for it.
    Ok((
        [("x-accel-buffering", "no")],
        Sse::new(stream).keep_alive(KeepAlive::default()),
    ))
}

fn sse_event(event: &JobEvent) -> Event {
    Event::default()
        .event(if event.terminal {
            "terminal"
        } else {
            "progress"
        })
        .json_data(event)
        .unwrap_or_else(|_| Event::default().data("serialisation failed"))
}
