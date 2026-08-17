use axum::{
    extract::{Path, State},
    routing::{get, post},
    Router, Json,
    response::{IntoResponse, sse::{Event, Sse}},
};
use phototools_core::jobs::JobState;
use phototools_core::tools::f1_dates::{DateRepairTool, DateRepairParams, DateRepairAction};
use phototools_core::tools::Tool;
use crate::AppState;
use crate::auth::ClaimsExtracted;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use futures::stream::Stream;
use tokio_stream::wrappers::BroadcastStream;
use tokio::sync::broadcast;
use std::sync::Arc;
use tokio_stream::StreamExt;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/tools/f1/plan", post(f1_plan))
        .route("/api/tools/f1/apply", post(f1_apply))
        .route("/api/tools/f2/plan", post(f2_plan))
        .route("/api/tools/f3/plan", post(f3_plan))
        .route("/api/tools/f4/plan", post(f4_plan))
        .route("/api/tools/f5/plan", post(f5_plan))
        .route("/api/tools/f6/plan", post(f6_plan))
        .route("/api/tools/f7/plan", post(f7_plan))
        .route("/api/tools/f8/plan", post(f8_plan))
        .route("/api/tools/f9/plan", post(f9_plan))
        .route("/api/jobs/:id/events", get(job_events))
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

// Dummy progress implementation for API
struct ApiProgress {
    sender: broadcast::Sender<JobState>,
}

impl phototools_core::jobs::Progress for ApiProgress {
    fn report(&self, done: u64, total: u64, message: &str) {
        let _ = self.sender.send(JobState {
            id: "fake_id".into(),
            kind: "f1_repair".into(),
            state: "running".into(),
            progress: done,
            total,
        });
    }

    fn cancelled(&self) -> bool {
        false
    }
}

async fn f1_plan(
    _claims: ClaimsExtracted,
    State(state): State<AppState>,
    Json(params): Json<DateRepairParams>,
) -> impl IntoResponse {
    let tool = DateRepairTool;
    // In reality we should sanitize params.paths using config.roots
    match tool.plan(&params) {
        Ok(outcome) => (axum::http::StatusCode::OK, Json(outcome.data)).into_response(),
        Err(e) => (axum::http::StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

async fn f1_apply(
    _claims: ClaimsExtracted,
    State(state): State<AppState>,
    Json(plan): Json<phototools_core::tools::Plan<DateRepairAction>>,
) -> impl IntoResponse {
    let tool = DateRepairTool;
    let (tx, _rx) = broadcast::channel(100);
    let progress = ApiProgress { sender: tx };
    
    match tool.apply(plan, &progress) {
        Ok(outcome) => (axum::http::StatusCode::OK, Json(outcome.data)).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(ErrorResponse { error: e.to_string() })).into_response(),
    }
}

// Stub handlers for the rest
async fn f2_plan(_claims: ClaimsExtracted) -> impl IntoResponse { axum::http::StatusCode::NOT_IMPLEMENTED }
async fn f3_plan(_claims: ClaimsExtracted) -> impl IntoResponse { axum::http::StatusCode::NOT_IMPLEMENTED }
async fn f4_plan(_claims: ClaimsExtracted) -> impl IntoResponse { axum::http::StatusCode::NOT_IMPLEMENTED }
async fn f5_plan(_claims: ClaimsExtracted) -> impl IntoResponse { axum::http::StatusCode::NOT_IMPLEMENTED }
async fn f6_plan(_claims: ClaimsExtracted) -> impl IntoResponse { axum::http::StatusCode::NOT_IMPLEMENTED }
async fn f7_plan(_claims: ClaimsExtracted) -> impl IntoResponse { axum::http::StatusCode::NOT_IMPLEMENTED }
async fn f8_plan(_claims: ClaimsExtracted) -> impl IntoResponse { axum::http::StatusCode::NOT_IMPLEMENTED }
async fn f9_plan(_claims: ClaimsExtracted) -> impl IntoResponse { axum::http::StatusCode::NOT_IMPLEMENTED }

// SSE Endpoint for job events
async fn job_events(
    Path(id): Path<String>,
    _claims: ClaimsExtracted,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    // In reality, look up the job's broadcast channel from State
    let (tx, rx) = broadcast::channel(100);
    
    // Create a stream that just yields a single "connected" event for now
    let _ = tx.send(JobState {
        id: id.clone(),
        kind: "unknown".into(),
        state: "connected".into(),
        progress: 0,
        total: 100,
    });

    let stream = tokio_stream::wrappers::BroadcastStream::new(rx)
        .map(|msg| {
            let status = msg.unwrap();
            Event::default().json_data(status).map_err(|_| unreachable!())
        });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(1))
            .text("keep-alive-text"),
    )
}
