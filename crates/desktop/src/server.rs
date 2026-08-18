//! The server connection.
//!
//! **Specification §8:** the desktop's HTTP calls to the server are made from
//! the Rust side with `reqwest`, not from the webview's JavaScript. That avoids
//! CORS entirely, avoids mixed-content restrictions, and means plain HTTP over
//! the local network needs no certificate.

use phototools_core::error::Error;
use phototools_core::ingest::{ArrivalReport, Manifest, SessionClient, SessionPlan};
use phototools_core::jobs::{Job, JobStatus};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Duration;

/// How long to wait before deciding the NAS is not answering.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerSettings {
    /// Base URL of `phototools-server`, e.g. `http://nas.local:3000`.
    pub base_url: String,
    /// The bearer token sent with every authenticated request (§5.2).
    ///
    /// `None` until somebody supplies one. The desktop has a Keychain
    /// (`credentials`) but no Firebase sign-in yet, so today this carries the
    /// configured administrative token — §5.3's documented break-glass path,
    /// which is exactly the case of a machine on the local network that cannot
    /// reach Firebase.
    #[serde(default)]
    pub auth_token: Option<String>,
}

impl Default for ServerSettings {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:3000".to_string(),
            auth_token: None,
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

    /// A client for the handoff, bound to the settings as they stand now.
    ///
    /// Separate from the probe client on purpose: [`PROBE_TIMEOUT`] is three
    /// seconds, which is right for "is the NAS there?" and wrong for a request
    /// that may be copying a gigabyte's worth of answer out of a manifest.
    pub fn session_client(&self) -> HandoffClient {
        HandoffClient::new(self.settings())
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

/// How long a handoff request may take before the NAS is presumed gone.
const HANDOFF_TIMEOUT: Duration = Duration::from_secs(60);

/// How often to ask whether the verification job has finished.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// How long to wait for verification before giving up on it.
///
/// Hashing a card's worth of derivatives off the NAS's own disks is minutes,
/// not seconds, and a bounded wait is what stops a wedged job from hanging the
/// desktop until somebody notices.
const VERIFY_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Debug, Deserialize)]
struct JobAccepted {
    job_id: String,
}

/// The desktop's half of the handoff: HTTP, and nothing else.
///
/// Every decision in the protocol — what to copy, what a mismatch means, how
/// many times to try — lives in `core`'s [`run_handoff`]. This type exists only
/// to put those two calls on the wire (G1).
///
/// [`run_handoff`]: phototools_core::ingest::run_handoff
pub struct HandoffClient {
    settings: ServerSettings,
    client: reqwest::blocking::Client,
}

impl HandoffClient {
    pub fn new(settings: ServerSettings) -> Self {
        let client = reqwest::blocking::Client::builder()
            .timeout(HANDOFF_TIMEOUT)
            .build()
            .unwrap_or_default();
        Self { settings, client }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.settings.base_url.trim_end_matches('/'))
    }

    /// Attach the bearer token, if there is one.
    ///
    /// A missing token is not caught here: the server answers `401` and says
    /// why, which is a better message than anything this side could invent.
    fn authorise(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> reqwest::blocking::RequestBuilder {
        match &self.settings.auth_token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    fn read<T: serde::de::DeserializeOwned>(
        response: reqwest::blocking::Response,
        what: &str,
    ) -> Result<T, Error> {
        let status = response.status();
        if !status.is_success() {
            let detail = response.text().unwrap_or_default();
            return Err(Error::Internal(format!(
                "the server answered {status} to {what}: {detail}"
            )));
        }
        response
            .json()
            .map_err(|e| Error::Internal(format!("could not read the server's {what}: {e}")))
    }

    /// Poll a job until it is terminal, and fail loudly if it did not succeed.
    fn await_job(&self, job_id: &str) -> Result<(), Error> {
        let deadline = std::time::Instant::now() + VERIFY_TIMEOUT;

        loop {
            let response = self
                .authorise(self.client.get(self.url(&format!("/api/jobs/{job_id}"))))
                .send()
                .map_err(|e| Error::Internal(format!("could not reach the server: {e}")))?;
            let job: Job = Self::read(response, "job state")?;

            if job.status.is_terminal() {
                return match job.status {
                    JobStatus::Completed => Ok(()),
                    other => Err(Error::Internal(format!(
                        "the server's verification {other}: {}",
                        job.error.unwrap_or_else(|| "no reason given".into())
                    ))),
                };
            }

            if std::time::Instant::now() >= deadline {
                return Err(Error::Internal(format!(
                    "the server was still verifying after {} minutes",
                    VERIFY_TIMEOUT.as_secs() / 60
                )));
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    }
}

impl SessionClient for HandoffClient {
    fn open_session(&self, manifest: &Manifest) -> Result<SessionPlan, Error> {
        let response = self
            .authorise(self.client.post(self.url("/api/ingest/sessions")))
            .json(manifest)
            .send()
            .map_err(|e| Error::Internal(format!("could not reach the server: {e}")))?;

        Self::read(response, "session plan")
    }

    fn mark_ready(&self, session_id: &str) -> Result<ArrivalReport, Error> {
        let response = self
            .authorise(
                self.client
                    .post(self.url(&format!("/api/ingest/sessions/{session_id}/ready"))),
            )
            .send()
            .map_err(|e| Error::Internal(format!("could not reach the server: {e}")))?;

        // Verification is a job (F17), so this returns an id, not an answer.
        let accepted: JobAccepted = Self::read(response, "verification job")?;
        self.await_job(&accepted.job_id)?;

        let response = self
            .authorise(
                self.client
                    .get(self.url(&format!("/api/ingest/sessions/{session_id}/shots"))),
            )
            .send()
            .map_err(|e| Error::Internal(format!("could not reach the server: {e}")))?;

        let shots: SessionShots = Self::read(response, "arrival report")?;
        shots.report.ok_or_else(|| {
            Error::Internal(
                "the server finished verifying but produced no report; \
                 nothing was recopied because nothing is known to be wrong"
                    .into(),
            )
        })
    }
}

/// Only the field this side needs. The rest of the response is for the review
/// grid, which is the web UI's business.
#[derive(Debug, Deserialize)]
struct SessionShots {
    report: Option<ArrivalReport>,
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
            auth_token: None,
        });
        assert_eq!(connection.settings().base_url, "http://nas.local:3000");
    }

    #[tokio::test]
    async fn an_unreachable_server_is_a_status_not_an_error() {
        // Port 1 is not listening; this must report rather than fail.
        let connection = ServerConnection::new(ServerSettings {
            base_url: "http://127.0.0.1:1".into(),
            auth_token: None,
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
