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

/// What is written to disk: the address, and deliberately not the token.
#[derive(Debug, Serialize, Deserialize)]
struct StoredSettings {
    base_url: String,
}

/// Where the address is kept — beside `config.json`, in the same directory the
/// rest of this application's configuration already uses.
fn settings_path() -> Option<std::path::PathBuf> {
    phototools_core::config::Config::config_path()
        .parent()
        .map(|dir| dir.join("server.json"))
}

impl ServerSettings {
    /// Read what was last saved, falling back to the default.
    ///
    /// **The address comes from a file and the token from the Keychain.** They
    /// are split because they are different kinds of thing: an address is
    /// configuration, and a bearer token is a credential that §9.2 rule 4 keeps
    /// off disk in the clear.
    ///
    /// Every failure here degrades to the default rather than propagating. A
    /// machine with no credential store, or a settings file somebody has
    /// mangled, should still open a window — the address is re-typable, and
    /// refusing to start over it would be worse than starting at the default.
    pub fn load() -> Self {
        let base_url = settings_path()
            .map(|path| Self::base_url_from(&path))
            .unwrap_or_else(|| Self::default().base_url);

        let auth_token = crate::credentials::Credential::server_auth_token()
            .and_then(|c| c.read())
            .unwrap_or(None);

        Self {
            base_url,
            auth_token,
        }
    }

    /// The saved address, or the default when there is not a usable one.
    ///
    /// A file that is missing, unreadable or malformed all mean the same thing
    /// to a person opening the application: start at the default and let them
    /// retype it. None of them is worth refusing to open a window over.
    fn base_url_from(path: &std::path::Path) -> String {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str::<StoredSettings>(&raw).ok())
            .map(|stored| stored.base_url)
            .unwrap_or_else(|| Self::default().base_url)
    }

    /// Write the address, and only the address.
    fn write_base_url(&self, path: &std::path::Path) -> Result<(), String> {
        let stored = StoredSettings {
            base_url: self.base_url.clone(),
        };
        let json = serde_json::to_string_pretty(&stored)
            .map_err(|e| format!("Could not encode the server settings: {e}"))?;
        std::fs::write(path, json).map_err(|e| format!("Could not write {}: {e}", path.display()))
    }

    /// Persist, so the next launch does not start at the default again.
    ///
    /// Reports what it could not do rather than failing silently (G10): a
    /// Keychain that refused the token is the difference between a handoff that
    /// works tomorrow and one that answers 401 for no visible reason.
    pub fn save(&self) -> Result<(), String> {
        let path = settings_path().ok_or("There is no configuration directory to save into.")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Could not create {}: {e}", parent.display()))?;
        }

        self.write_base_url(&path)?;

        let credential = crate::credentials::Credential::server_auth_token()?;
        match self.auth_token.as_deref().filter(|t| !t.trim().is_empty()) {
            Some(token) => credential.store(token),
            // Clearing matters as much as storing: a token removed from the
            // field has to leave the Keychain, or the next launch restores it.
            None => credential.clear(),
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
                // Something is there and it answered 200, but not with the
                // health document. Another service on the same port.
                Err(_) => ServerStatus {
                    reachable: false,
                    base_url: base_url.clone(),
                    version: None,
                    detail: Some(format!(
                        "Something is listening at {base_url}, but it did not answer as \
                         PhotoTools. This is usually another application on that port."
                    )),
                },
            },
            // The distinction that matters: an answer means the address reaches
            // *a* server, so the fix is the address or the port — not starting
            // something that is already running.
            Ok(response) => ServerStatus {
                reachable: false,
                base_url: base_url.clone(),
                version: None,
                detail: Some(format!(
                    "Something is listening at {base_url}, but it answered {} and is not \
                     PhotoTools. Check the address and port.",
                    response.status()
                )),
            },
            Err(e) if e.is_timeout() => ServerStatus {
                reachable: false,
                base_url: base_url.clone(),
                version: None,
                detail: Some(format!(
                    "{base_url} did not answer within {} seconds.",
                    PROBE_TIMEOUT.as_secs()
                )),
            },
            Err(_) => ServerStatus {
                reachable: false,
                base_url: base_url.clone(),
                version: None,
                detail: Some(format!(
                    "Nothing answered at {base_url}. Check the server is running and the \
                     address is right."
                )),
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

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    fn an_address_survives_a_round_trip_to_disk() {
        // The whole point: an address typed once is not typed again.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.json");

        let settings = ServerSettings {
            base_url: "http://nas.local:3000".into(),
            auth_token: Some("not written here".into()),
        };
        settings.write_base_url(&path).unwrap();

        assert_eq!(
            ServerSettings::base_url_from(&path),
            "http://nas.local:3000"
        );
    }

    #[test]
    fn the_token_is_never_written_beside_the_address() {
        // §9.2 rule 4. The Keychain holds the credential; this file must not,
        // and a regression here would be invisible until somebody read the file.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.json");

        ServerSettings {
            base_url: "http://nas.local:3000".into(),
            auth_token: Some("super-secret-admin-token".into()),
        }
        .write_base_url(&path)
        .unwrap();

        let written = std::fs::read_to_string(&path).unwrap();
        assert!(
            !written.contains("super-secret-admin-token"),
            "the token reached the settings file: {written}"
        );
    }

    #[test]
    fn a_missing_or_mangled_file_falls_back_to_the_default() {
        // Neither is worth refusing to open a window over: the address is
        // re-typable, and an application that will not start is not.
        let dir = tempfile::tempdir().unwrap();

        let missing = dir.path().join("absent.json");
        assert_eq!(
            ServerSettings::base_url_from(&missing),
            ServerSettings::default().base_url
        );

        let mangled = dir.path().join("mangled.json");
        std::fs::write(&mangled, "{ this is not json").unwrap();
        assert_eq!(
            ServerSettings::base_url_from(&mangled),
            ServerSettings::default().base_url
        );
    }
}
