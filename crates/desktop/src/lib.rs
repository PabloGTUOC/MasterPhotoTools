//! `phototools-desktop` — the macOS application.
//!
//! Binary crates hold only transport, platform integration and process
//! lifecycle (G1). Everything this application does lives in `phototools-core`.

pub mod commands;
pub mod credentials;
pub mod jobs;
pub mod server;

use phototools_core::config::Config;
use phototools_core::jobs::JobRunner;
use phototools_core::ledger::Ledger;
use server::ServerConnection;
use std::sync::{Arc, RwLock};

pub struct AppState {
    config: RwLock<Arc<Config>>,
    pub jobs: JobRunner,
    pub server: ServerConnection,
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
