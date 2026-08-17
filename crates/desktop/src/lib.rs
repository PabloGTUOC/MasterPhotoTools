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
use phototools_core::jobs::JobRunner;
use phototools_core::ledger::Ledger;
use server::ServerConnection;
use std::sync::{Arc, Mutex, RwLock};

pub struct AppState {
    config: RwLock<Arc<Config>>,
    pub jobs: JobRunner,
    pub server: ServerConnection,
    /// Held for its lifetime, not read: dropping it stops the watch (F10).
    card_watcher: Mutex<Option<detection::VolumeWatcher>>,
}

impl AppState {
    pub fn new(
        config: Config,
        ledger: Ledger,
        sink: Arc<dyn phototools_core::jobs::JobEventSink>,
    ) -> Self {
        let server = ServerConnection::new(server::ServerSettings::default());
        Self {
            config: RwLock::new(Arc::new(config)),
            jobs: JobRunner::new(ledger, sink),
            server,
            card_watcher: Mutex::new(None),
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
}
