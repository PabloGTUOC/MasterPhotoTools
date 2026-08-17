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
use phototools_core::tools::{f1_dates, f3_rename, f4_split, f5_contact, f6_transform};
use phototools_core::tools::{f7_border, f8_tiff, f9_browser, Tool};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::path::PathBuf;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::{Stream, StreamExt};

pub fn router() -> Router<AppState> {
    Router::new()
        // Archive tools — operate on library paths.
        .route("/api/tools/dates/scan", post(dates_scan))
        .route("/api/tools/dates/fix", post(dates_fix))
        .route("/api/tools/rename/plan", post(rename_plan))
        .route("/api/tools/rename/apply", post(rename_apply))
        .route("/api/tools/split", post(split))
        .route("/api/tools/contact-sheet", post(contact_sheet))
        .route("/api/tools/transform", post(transform))
        .route("/api/tools/border", post(border))
        .route("/api/tools/tiff-to-jpeg", post(tiff_to_jpeg))
        .route("/api/storage/ls", get(storage_ls))
        // Ingest — F11, F12, F13. A card is any directory (build plan §6.3).
        .route("/api/ingest/scan", post(ingest_scan))
        .route("/api/ingest/validate", post(ingest_validate))
        .route("/api/ingest/remediate", post(ingest_remediate))
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
            "path_not_allowed" => StatusCode::FORBIDDEN,
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
    auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<DatesScanRequest>,
) -> Result<Response, ApiError> {
    let root = resolve_input(&state.config, &request.path)?;
    let recursive = request.recursive;

    accept(&state, &auth, "dates_scan", 0, move |progress| {
        let results = f1_dates::scan_dates(&root, recursive)?;
        progress.report(results.len() as u64, results.len() as u64, "scanned");

        let mismatched = results
            .iter()
            .filter(|r| r.status != f1_dates::DateStatus::Ok)
            .count();
        Ok(format!(
            "{} files scanned, {mismatched} needing attention",
            results.len()
        ))
    })
}

#[derive(Debug, Deserialize)]
pub struct DatesFixRequest {
    pub paths: Vec<String>,
    pub mode: f1_dates::RepairMode,
    #[serde(default)]
    pub dry_run: bool,
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

        let summary = f1_dates::DateRepairTool.apply(plan, progress)?.data;
        Ok(format!(
            "{} verified, {} failed",
            summary.verified_count(),
            summary.failures.len()
        ))
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
        let summary = f3_rename::BatchRenamerTool.apply(plan, progress)?.data;
        Ok(format!(
            "{} renamed, {} failed",
            summary.renamed.len(),
            summary.failures.len()
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
    Json(request): Json<ImageToolRequest>,
) -> Result<Response, ApiError> {
    let r = request.resolve(&state.config)?;
    let params = f4_split::SplitParams::new(r.inputs, r.out_dir);

    accept(&state, &auth, "split", r.total, move |progress| {
        let plan = f4_split::SplitTool.plan(&params)?.data;
        let summary = f4_split::SplitTool.apply(plan, progress)?.data;
        Ok(format!(
            "{} halves written, {} failed",
            summary.written.len(),
            summary.failures.len()
        ))
    })
}

#[derive(Debug, Deserialize)]
pub struct ContactSheetRequest {
    pub inputs: Vec<String>,
    #[serde(default)]
    pub recursive: bool,
    pub out_path: String,
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

    accept(&state, &auth, "contact_sheet", total, move |progress| {
        let plan = f5_contact::ContactSheetTool.plan(&params)?.data;
        let summary = f5_contact::ContactSheetTool.apply(plan, progress)?.data;
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
        let summary = f6_transform::TransformTool.apply(plan, progress)?.data;
        Ok(format!(
            "{} written, {} failed",
            summary.written.len(),
            summary.failures.len()
        ))
    })
}

async fn border(
    auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<ImageToolRequest>,
) -> Result<Response, ApiError> {
    let recursive = request.recursive;
    let r = request.resolve(&state.config)?;

    let mut params = f7_border::PrintBorderParams::new(r.inputs, r.out_dir);
    params.recursive = recursive;

    accept(&state, &auth, "border", r.total, move |progress| {
        let plan = f7_border::PrintBorderTool.plan(&params)?.data;
        let summary = f7_border::PrintBorderTool.apply(plan, progress)?.data;
        Ok(format!(
            "{} bordered, {} failed",
            summary.written.len(),
            summary.failures.len()
        ))
    })
}

async fn tiff_to_jpeg(
    auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<ImageToolRequest>,
) -> Result<Response, ApiError> {
    let recursive = request.recursive;
    let r = request.resolve(&state.config)?;

    let mut params = f8_tiff::TiffToJpegParams::new(r.inputs, r.out_dir);
    params.recursive = recursive;

    accept(&state, &auth, "tiff_to_jpeg", r.total, move |progress| {
        let plan = f8_tiff::TiffToJpegTool.plan(&params)?.data;
        let summary = f8_tiff::TiffToJpegTool.apply(plan, progress)?.data;
        Ok(format!(
            "{} pages written, {} failed",
            summary.written.len(),
            summary.failures.len()
        ))
    })
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

// ---------------------------------------------------------------------------
// Ingest — F11, F12, F13
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct CardRequest {
    pub path: String,
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
    pub actions: Vec<String>,
    pub default_action: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ClockOffsetResponse {
    pub median: String,
    pub spread_days: i64,
    pub median_age_days: i64,
    pub shift: String,
    pub affected: usize,
}

/// Validate a card against the three rules (F12).
///
/// Synchronous: validation reads no pixels, so it is fast even on a full card.
async fn ingest_validate(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<CardRequest>,
) -> Result<Response, ApiError> {
    let root = resolve_input(&state.config, &request.path)?;
    let card = Card::at(&root)?;

    let scan = ingest::scan_card(&card, &InMemoryProgress::new())?;
    let validation = ingest::validate(
        &scan.shots,
        chrono::Utc::now().naive_utc(),
        &state.config.thresholds,
    );

    let groups = validation
        .by_failure()
        .into_iter()
        .map(|(failure, indices)| FailureGroup {
            failure: failure.as_str().to_string(),
            count: indices.len(),
            actions: ingest::actions_for(failure)
                .into_iter()
                .map(|a| a.as_str().to_string())
                .collect(),
            default_action: ingest::default_action(failure).map(|a| a.as_str().to_string()),
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

#[derive(Debug, Deserialize)]
pub struct RemediateRequest {
    pub path: String,
    pub failure: String,
    pub action: String,
    pub date: Option<String>,
    pub out_dir: String,
    /// Specification §9.2 rule 3: every destructive operation supports a dry run.
    #[serde(default)]
    pub dry_run: bool,
}

#[derive(Debug, Serialize)]
pub struct RemediationPreview {
    pub stem: String,
    pub action: String,
    pub target_dimensions: Option<(u32, u32)>,
    pub new_date: Option<String>,
    pub destination: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RemediationPlanResponse {
    pub actions: Vec<RemediationPreview>,
    pub skipped: Vec<SkipResponse>,
    pub applied: bool,
}

#[derive(Debug, Serialize)]
pub struct SkipResponse {
    pub file: String,
    pub reason: String,
}

/// Apply one action to every shot sharing one failure (F13).
///
/// With `dry_run` the plan comes back and nothing is written.
async fn ingest_remediate(
    _auth: Authenticated,
    State(state): State<AppState>,
    Json(request): Json<RemediateRequest>,
) -> Result<Response, ApiError> {
    let root = resolve_input(&state.config, &request.path)?;
    let out_dir = resolve_output(&state.config, &request.out_dir)?;
    let card = Card::at(&root)?;

    let failure = parse_failure(&request.failure)?;
    let action = parse_action(&request.action)?;
    let date = match &request.date {
        Some(raw) => Some(
            chrono::NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S").map_err(|_| {
                ApiError::bad_request(format!(
                    "{raw} is not a date of the form YYYY-MM-DDTHH:MM:SS"
                ))
            })?,
        ),
        None => None,
    };

    let scan = ingest::scan_card(&card, &InMemoryProgress::new())?;
    let validation = ingest::validate(
        &scan.shots,
        chrono::Utc::now().naive_utc(),
        &state.config.thresholds,
    );

    let params = ingest::RemediationParams {
        shots: &scan.shots,
        validation: &validation,
        thresholds: state.config.thresholds.clone(),
        request: ingest::BulkRequest {
            failure,
            action,
            date,
            output_dir: out_dir,
        },
    };

    let plan = ingest::plan_bulk(&params)?.data;

    let body = RemediationPlanResponse {
        applied: !request.dry_run,
        actions: plan
            .actions
            .iter()
            .map(|a| RemediationPreview {
                stem: a.stem.clone(),
                action: a.action.as_str().to_string(),
                target_dimensions: a.target_dimensions,
                new_date: a.new_date.map(|d| d.to_string()),
                destination: a
                    .destination
                    .as_ref()
                    .map(|d| d.to_string_lossy().to_string()),
            })
            .collect(),
        skipped: plan
            .skipped
            .iter()
            .map(|s| SkipResponse {
                file: s.file.clone(),
                reason: s.reason.clone(),
            })
            .collect(),
    };

    if !request.dry_run {
        ingest::apply_bulk(plan, &InMemoryProgress::new())?;
    }

    Ok(Json(body).into_response())
}

fn parse_failure(raw: &str) -> Result<ingest::FailureClass, ApiError> {
    use ingest::FailureClass::*;
    match raw {
        "no_date" => Ok(NoDate),
        "date_out_of_range" => Ok(DateOutOfRangeIsolated),
        "date_out_of_range_batch" => Ok(DateOutOfRangeBatch),
        "too_many_pixels" => Ok(TooManyPixels),
        "too_large" => Ok(TooLarge),
        other => Err(ApiError::bad_request(format!(
            "{other} is not a failure class"
        ))),
    }
}

fn parse_action(raw: &str) -> Result<ingest::ActionKind, ApiError> {
    use ingest::ActionKind::*;
    match raw {
        "enter_date_manually" => Ok(EnterDateManually),
        "derive_from_batch_median" => Ok(DeriveFromBatchMedian),
        "use_file_modification_time" => Ok(UseFileModificationTime),
        "redate_manually" => Ok(RedateManually),
        "bulk_shift" => Ok(BulkShift),
        "publish_anyway" => Ok(PublishAnyway),
        "resize" => Ok(Resize),
        "reencode_lower" => Ok(ReencodeLower),
        "skip" => Ok(Skip),
        other => Err(ApiError::bad_request(format!(
            "{other} is not a remediation action"
        ))),
    }
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
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
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
            message: job.error.clone().unwrap_or_else(|| "done".into()),
            terminal: true,
        })
    } else {
        None
    };

    let live = state.jobs.subscribe(&id);

    let stream = async_stream::stream! {
        if let Some(event) = replay {
            yield Ok(sse_event(&event));
            return;
        }

        if let Some(rx) = live {
            let mut stream = BroadcastStream::new(rx);
            while let Some(item) = stream.next().await {
                let Ok(event) = item else { continue };
                let terminal = event.terminal;
                yield Ok(sse_event(&event));
                if terminal {
                    break;
                }
            }
        }
    };

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
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
