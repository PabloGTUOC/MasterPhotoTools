//! `phototools-server` — the HTTP backend for the web front end.
//!
//! Binary crates contain only transport, platform integration and process
//! lifecycle (G1). Everything this serves lives in `phototools-core`.

pub mod api;
pub mod auth;
pub mod jobs;

use axum::extract::FromRef;
use axum::response::IntoResponse;
use axum::{
    routing::{any, get},
    Json, Router,
};
use phototools_core::config::Config;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tower_http::services::{ServeDir, ServeFile};

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
    build_router_with_web(state, web_root())
}

/// `build_router` with the front end's location supplied rather than read.
///
/// The location is an environment variable in production, and an environment
/// variable is process-wide state that tests cannot set without racing each
/// other. Taking it as an argument is what lets the two behaviours below be
/// asserted at all.
pub fn build_router_with_web(state: AppState, web: Option<PathBuf>) -> Router {
    let router = Router::new()
        .route("/api/health", get(health))
        .merge(api::router())
        .with_state(state);

    match web {
        // The wildcard is what keeps the two halves apart. Without it the
        // fallback catches everything that did not match, so a typo'd or
        // withdrawn endpoint would answer a client with 200 and a page of HTML
        // instead of a 404 it can act on. A static segment outranks a wildcard
        // in the router, so the real /api routes still win.
        Some(root) => router
            .route("/api/*path", any(api_not_found))
            .fallback_service(web_service(&root)),
        None => router,
    }
}

/// Answer an unknown `/api` path as the rest of the API answers things.
async fn api_not_found() -> impl IntoResponse {
    (
        axum::http::StatusCode::NOT_FOUND,
        Json(serde_json::json!({
            "code": "not_found",
            "message": "No such endpoint.",
        })),
    )
}

/// Where the built web front end lives, if it was shipped alongside the binary.
///
/// Specification §2.2 has the web UI "served by `phototools-server`". It is a
/// deployment detail rather than a behaviour, so it is configuration: the
/// container image sets `WEB_ROOT`, and a development run leaves it unset and
/// uses Vite's dev server instead, which is what `frontend/web/vite.config.ts`
/// proxies `/api` back here for.
fn web_root() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var("WEB_ROOT").ok()?);

    // A misspelled WEB_ROOT would otherwise serve 404s for every asset with
    // nothing said about why, which is a bad half-hour for whoever deploys it.
    if !root.join("index.html").is_file() {
        tracing::error!(
            path = %root.display(),
            "WEB_ROOT holds no index.html, so the web front end will not be served"
        );
        return None;
    }

    tracing::info!(path = %root.display(), "serving the web front end");
    Some(root)
}

/// Serve the built front end, falling back to `index.html`.
///
/// The application is a single page with client-side routing, so a browser
/// asked to open `/publish` directly requests a path that is not a file. It has
/// to be answered with the document rather than a 404, or every route but `/`
/// breaks on reload.
///
/// The cost of that is unconditional: *any* unmatched path under the front end
/// gets `index.html`, including a missing asset, which reaches the browser as a
/// MIME-type complaint rather than a 404. The alternative is to exempt the
/// asset directory, and that trades a clear error on a case that only happens
/// when a deployment is half-copied for a rule that has to be kept in step with
/// whatever Vite names its output directory.
fn web_service(root: &Path) -> ServeDir<ServeFile> {
    // `fallback`, not `not_found_service`: the latter forces 404 onto the
    // response, and a client-side route is a real page rather than a missing
    // one. A browser would render it either way; a link checker, a crawler
    // and anything reading the status would not.
    ServeDir::new(root).fallback(ServeFile::new(root.join("index.html")))
}
