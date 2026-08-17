//! `phototools-server` — the HTTP backend for the web front end.
//!
//! Binary crates contain only transport, platform integration and process
//! lifecycle (G1). Everything this serves lives in `phototools-core`.

pub mod api;
pub mod auth;
pub mod jobs;

use axum::extract::FromRef;
use axum::response::IntoResponse;
use axum::{routing::get, Json, Router};
use phototools_core::config::Config;
use serde::Serialize;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub auth: Arc<auth::AuthConfig>,
    pub jobs: Arc<jobs::JobManager>,
}

impl FromRef<AppState> for Arc<auth::AuthConfig> {
    fn from_ref(state: &AppState) -> Self {
        Arc::clone(&state.auth)
    }
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
}

/// The one unauthenticated route (specification §8).
pub async fn health() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok",
        version: env!("CARGO_PKG_VERSION"),
    })
}

pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(health))
        .merge(api::router())
        .with_state(state)
}
