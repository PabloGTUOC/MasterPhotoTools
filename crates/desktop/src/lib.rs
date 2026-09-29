//! `phototools-desktop` — the macOS application.
//!
//! Binary crates hold only transport, platform integration and process
//! lifecycle (G1). Everything this application does lives in `phototools-core`.

pub mod commands;
pub mod credentials;
pub mod detection;
pub mod jobs;
pub mod server;

use phototools_core::config::Config;
use phototools_core::error::Error;
use phototools_core::jobs::JobRunner;
use phototools_core::ledger::Ledger;
use phototools_core::media::edit::lut::Lut;
use phototools_core::media::edit::pipeline::AdjustmentRecipe;
use phototools_core::media::edit::preview::{PreviewSession, PreviewStage, RgbaFrame};
use server::ServerConnection;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

static NEXT_SESSION_ID: AtomicU64 = AtomicU64::new(1);

pub type PreviewDimensions = (u32, u32);

/// Session identifier, proxy dimensions, orientation, and card safety status returned on preview open.
#[derive(Debug, Clone)]
pub struct PreviewSessionInfo {
    pub session_id: String,
    pub drag: PreviewDimensions,
    pub settle: PreviewDimensions,
    pub orientation: u32,
    pub read_only: bool,
}

/// A currently active interactive preview session and its token.
pub struct ActivePreview {
    pub session_id: String,
    pub path: PathBuf,
    pub session: PreviewSession,
}

pub struct AppState {
    config: RwLock<Arc<Config>>,
    pub jobs: JobRunner,
    pub server: ServerConnection,
    /// Held for its lifetime, not read: dropping it stops the watch (F10).
    card_watcher: Mutex<Option<detection::VolumeWatcher>>,
    /// Active interactive preview session (ED-4, ED-6).
    ///
    /// Memory: a preview session holds about 65 MB of pre-downscaled, linearised
    /// proxy buffers (720p and 1440p). To prevent runaway memory usage, at most one
    /// open session is kept at any time: opening a second preview session automatically
    /// closes and frees the first. The session is also closed when explicitly dismissed
    /// or when the window closes (via `on_window_event` in `main.rs` calling `close_any_preview`).
    active_preview: Mutex<Option<ActivePreview>>,
}

impl AppState {
    pub fn new(
        config: Config,
        ledger: Ledger,
        sink: Arc<dyn phototools_core::jobs::JobEventSink>,
    ) -> Self {
        // What was saved last time, not the default: an address typed once
        // should not have to be typed again at every launch.
        let server = ServerConnection::new(server::ServerSettings::load());
        Self {
            config: RwLock::new(Arc::new(config)),
            jobs: JobRunner::new(ledger, sink),
            server,
            card_watcher: Mutex::new(None),
            active_preview: Mutex::new(None),
        }
    }

    /// Keep a card watcher alive for the life of the application (F10).
    pub fn set_card_watcher(&self, watcher: detection::VolumeWatcher) {
        if let Ok(mut slot) = self.card_watcher.lock() {
            *slot = Some(watcher);
        }
    }

    pub fn config(&self) -> Arc<Config> {
        self.config
            .read()
            .map(|c| Arc::clone(&c))
            .unwrap_or_else(|_| Arc::new(Config::default()))
    }

    pub fn set_config(&self, next: Config) {
        if let Ok(mut current) = self.config.write() {
            *current = Arc::new(next);
        }
    }

    /// Opens a new interactive preview session for `path`, replacing and freeing any
    /// previous preview session.
    pub fn open_preview(&self, path: PathBuf) -> Result<PreviewSessionInfo, Error> {
        let session = PreviewSession::open(&path)?;
        let drag = session.drag_dimensions();
        let settle = session.settle_dimensions();
        let orientation = phototools_core::media::meta::read_meta(&path)
            .map(|m| m.orientation as u32)
            .unwrap_or(1);
        let read_only = phototools_core::tools::edit::is_card_volume(&path);
        let session_id = format!(
            "preview_{}",
            NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
        );
        let mut slot = self.active_preview.lock().unwrap();
        *slot = Some(ActivePreview {
            session_id: session_id.clone(),
            path,
            session,
        });
        Ok(PreviewSessionInfo {
            session_id,
            drag,
            settle,
            orientation,
            read_only,
        })
    }

    /// Explicitly closes the preview session matching `session_id`, releasing memory.
    pub fn close_preview(&self, session_id: &str) {
        let mut slot = self.active_preview.lock().unwrap();
        if let Some(active) = slot.as_ref() {
            if active.session_id == session_id {
                *slot = None;
            }
        }
    }

    /// Closes whatever preview session is open, if any (e.g. on window close).
    pub fn close_any_preview(&self) {
        let mut slot = self.active_preview.lock().unwrap();
        *slot = None;
    }

    /// Renders an RGBA8 frame from the active preview session.
    ///
    /// Returns an error if the session was closed or replaced by another preview.
    pub fn render_preview(
        &self,
        session_id: &str,
        recipe: &AdjustmentRecipe,
        lut: Option<&Lut>,
        stage: PreviewStage,
    ) -> Result<RgbaFrame, Error> {
        let slot = self.active_preview.lock().unwrap();
        let active = slot
            .as_ref()
            .ok_or_else(|| Error::Refused("No active preview session is open".into()))?;
        if active.session_id != session_id {
            return Err(Error::Refused(
                "The preview session has been closed or replaced by another preview".into(),
            ));
        }
        active.session.render(recipe, lut, stage)
    }
}
