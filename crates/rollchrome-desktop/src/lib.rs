//! `rollchrome-desktop` — the Rollchrome macOS application.
//!
//! Binary crates hold only transport, platform integration and process
//! lifecycle (G1). Everything Rollchrome does lives in `rollchrome-core` and
//! `phototools-core`.

pub mod commands;
pub mod jobs;

use phototools_core::config::Config;
use phototools_core::error::Error;
use phototools_core::jobs::JobRunner;
use phototools_core::ledger::Ledger;
use rollchrome_core::media::edit::lut::Lut;
use rollchrome_core::media::edit::pipeline::AdjustmentRecipe;
use rollchrome_core::media::edit::preview::{PreviewSession, PreviewStage, RgbaFrame};
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
    pub source_sha256: String,
}

/// A currently active interactive preview session and its token.
pub struct ActivePreview {
    pub session_id: String,
    pub path: PathBuf,
    pub session: PreviewSession,
}

struct CachedDevelopPreview {
    path: PathBuf,
    exposure_bits: u32,
    preview: Arc<rollchrome_core::media::develop::DevelopPreview>,
}

pub struct AppState {
    config: RwLock<Arc<Config>>,
    pub jobs: JobRunner,
    /// Active interactive preview session (ED-4, ED-6).
    ///
    /// Memory: a preview session holds about 65 MB of pre-downscaled, linearised
    /// proxy buffers (720p and 1440p). To prevent runaway memory usage, at most one
    /// open session is kept at any time: opening a second preview session automatically
    /// closes and frees the first. The session is also closed when explicitly dismissed
    /// or when the window closes (via `on_window_event` in `main.rs` calling `close_any_preview`).
    active_preview: Mutex<Option<ActivePreview>>,
    /// Cached develop preview (RP-1).
    ///
    /// Memory: at most one DevelopPreview is kept, keyed by resolved path and exposure.
    /// Changing a look re-renders without re-developing the RAW.
    develop_preview: Mutex<Option<CachedDevelopPreview>>,
    develop_preview_runs: AtomicU64,
}

impl AppState {
    pub fn new(
        config: Config,
        ledger: Ledger,
        sink: Arc<dyn phototools_core::jobs::JobEventSink>,
    ) -> Self {
        Self {
            config: RwLock::new(Arc::new(config)),
            jobs: JobRunner::new(ledger, sink),
            active_preview: Mutex::new(None),
            develop_preview: Mutex::new(None),
            develop_preview_runs: AtomicU64::new(0),
        }
    }

    pub fn config(&self) -> Arc<Config> {
        self.config
            .read()
            .map(|c| Arc::clone(&c))
            .unwrap_or_else(|_| Arc::new(Config::default()))
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
        let read_only = rollchrome_core::tools::edit::is_card_volume(&path);
        let session_id = format!(
            "preview_{}",
            NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed)
        );
        let session_source_sha256 = session.source_sha256.clone();
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
            source_sha256: session_source_sha256,
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

    /// The open photograph's stored pixels and orientation, for an automatic mask (ED-23).
    /// The lock is held only to copy them, not while the model runs.
    pub fn segmentation_input(
        &self,
        session_id: &str,
    ) -> Result<(rollchrome_core::media::segment::RgbImage, u32), Error> {
        let slot = self.active_preview.lock().unwrap();
        let active = slot
            .as_ref()
            .ok_or_else(|| Error::Refused("No active preview session is open".into()))?;
        if active.session_id != session_id {
            return Err(Error::Refused(
                "The preview session has been closed or replaced by another preview".into(),
            ));
        }
        Ok((
            active.session.segmentation_input(),
            active.session.orientation,
        ))
    }

    /// Coverage of one mask from the active preview session, for the mask overlay (ED-20).
    pub fn mask_coverage(
        &self,
        session_id: &str,
        recipe: &AdjustmentRecipe,
        mask_id: &str,
        stage: PreviewStage,
    ) -> Result<(u32, u32, Vec<u8>), Error> {
        let slot = self.active_preview.lock().unwrap();
        let active = slot
            .as_ref()
            .ok_or_else(|| Error::Refused("No active preview session is open".into()))?;
        if active.session_id != session_id {
            return Err(Error::Refused(
                "The preview session has been closed or replaced by another preview".into(),
            ));
        }
        active.session.mask_coverage(recipe, mask_id, stage)
    }

    /// Number of actual RAW developments performed for develop preview (RP-1).
    pub fn develop_preview_runs(&self) -> u64 {
        self.develop_preview_runs.load(Ordering::SeqCst)
    }

    /// Returns a cached DevelopPreview or creates and caches one (RP-1).
    ///
    /// The lock is NOT held during the slow RAW development.
    pub fn get_or_create_develop_preview(
        &self,
        path: &std::path::Path,
        exposure_ev: f32,
    ) -> Result<Arc<rollchrome_core::media::develop::DevelopPreview>, Error> {
        let exp_bits = exposure_ev.to_bits();
        // 1. Check cache with lock held briefly
        {
            let slot = self.develop_preview.lock().unwrap();
            if let Some(cached) = slot.as_ref() {
                if cached.path == path && cached.exposure_bits == exp_bits {
                    return Ok(cached.preview.clone());
                }
            }
        }

        // 2. Lock is dropped. Develop RAW without holding the lock!
        let raw = std::fs::read(path).map_err(Error::Io)?;
        self.develop_preview_runs.fetch_add(1, Ordering::SeqCst);
        let preview = Arc::new(rollchrome_core::media::develop::DevelopPreview::new(
            &raw,
            exposure_ev,
            rollchrome_core::media::develop::PREVIEW_MAX_EDGE,
        )?);

        // 3. Re-acquire lock and store the single cached preview
        {
            let mut slot = self.develop_preview.lock().unwrap();
            *slot = Some(CachedDevelopPreview {
                path: path.to_path_buf(),
                exposure_bits: exp_bits,
                preview: preview.clone(),
            });
        }
        Ok(preview)
    }
}
