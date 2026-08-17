//! Job progress on the desktop.
//!
//! The running, persistence and recovery live in `core` (G1). What is
//! desktop-specific is the transport: each update becomes a Tauri event the
//! webview listens for, which is F17's "Tauri events on the desktop".

use phototools_core::jobs::{JobEventSink, JobUpdate};
use tauri::{AppHandle, Emitter};

/// The event name the front end subscribes to.
pub const JOB_EVENT: &str = "phototools://job";

pub struct TauriSink {
    app: AppHandle,
}

impl TauriSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl JobEventSink for TauriSink {
    fn emit(&self, update: &JobUpdate) {
        // A failed emit means the window has gone; the job continues regardless.
        let _ = self.app.emit(JOB_EVENT, update);
    }
}
