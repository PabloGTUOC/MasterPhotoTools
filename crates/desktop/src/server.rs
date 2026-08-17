//! The server connection.
//!
//! **Specification §8:** the desktop's HTTP calls to the server are made from
//! the Rust side with `reqwest`, not from the webview's JavaScript. That avoids
//! CORS entirely, avoids mixed-content restrictions, and means plain HTTP over
//! the local network needs no certificate.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Duration;

/// How long to wait before deciding the NAS is not answering.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerSettings {
    /// Base URL of `phototools-server`, e.g. `http://nas.local:3000`.
    pub base_url: String,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:3000".to_string(),
        }
    }
}

/// What the UI needs to know to disable server-backed features clearly.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerStatus {
    pub reachable: bool,
    pub base_url: String,
    pub version: Option<String>,
    /// Why it is unreachable, in words a person can act on.
    pub detail: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Health {
    #[allow(dead_code)]
    status: String,
    version: String,
}

pub struct ServerConnection {
    settings: Mutex<ServerSettings>,
    client: reqwest::Client,
}

impl ServerConnection {
    pub fn new(settings: ServerSettings) -> Self {
        Self {
            settings: Mutex::new(settings),
            client: reqwest::Client::builder()
                .timeout(PROBE_TIMEOUT)
                .build()
                .unwrap_or_default(),
        }
    }

    pub fn settings(&self) -> ServerSettings {
        self.settings.lock().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn set_settings(&self, next: ServerSettings) {
        if let Ok(mut current) = self.settings.lock() {
            *current = next;
        }
    }

    /// Ask the server whether it is there.
    ///
    /// Never returns an error: an unreachable server is a *state* the UI shows,
    /// not a failure that breaks the application (task 6).
    pub async fn status(&self) -> ServerStatus {
        let base_url = self.settings().base_url;
        let url = format!("{}/api/health", base_url.trim_end_matches('/'));

        match self.client.get(&url).send().await {
            Ok(response) if response.status().is_success() => match response.json::<Health>().await
            {
                Ok(health) => ServerStatus {
                    reachable: true,
                    base_url,
                    version: Some(health.version),
                    detail: None,
                },
                Err(e) => ServerStatus {
                    reachable: false,
                    base_url,
                    version: None,
                    detail: Some(format!("The server answered but not with health: {e}")),
                },
            },
            Ok(response) => ServerStatus {
                reachable: false,
                base_url,
                version: None,
                detail: Some(format!("The server answered {}", response.status())),
            },
            Err(e) if e.is_timeout() => ServerStatus {
                reachable: false,
                base_url,
                version: None,
                detail: Some("The server did not answer in time.".into()),
            },
            Err(_) => ServerStatus {
                reachable: false,
                base_url,
                version: None,
                detail: Some(
                    "Could not reach the server. Check it is running and the address is right."
                        .into(),
                ),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_points_at_a_local_server() {
        assert_eq!(ServerSettings::default().base_url, "http://127.0.0.1:3000");
    }

    #[test]
    fn settings_can_be_changed_at_runtime() {
        let connection = ServerConnection::new(ServerSettings::default());
        connection.set_settings(ServerSettings {
            base_url: "http://nas.local:3000".into(),
        });
        assert_eq!(connection.settings().base_url, "http://nas.local:3000");
    }

    #[tokio::test]
    async fn an_unreachable_server_is_a_status_not_an_error() {
        // Port 1 is not listening; this must report rather than fail.
        let connection = ServerConnection::new(ServerSettings {
            base_url: "http://127.0.0.1:1".into(),
        });

        let status = connection.status().await;
        assert!(!status.reachable);
        assert!(status.version.is_none());
        assert!(
            status.detail.is_some(),
            "the UI needs something to show the user"
        );
    }
}
