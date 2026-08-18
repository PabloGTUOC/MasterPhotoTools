//! OAuth 2.0 for Google Photos (specification §6.2).
//!
//! **This is not Firebase.** §5.1 is emphatic that the two are independent:
//! Firebase says who may use PhotoTools, and this says whether PhotoTools may
//! add photographs to one Google account. Firebase's sign-in returns no refresh
//! token for Google API scopes, so a service that uploads unattended needs its
//! own authorization-code flow with `access_type=offline` — which is what this
//! is.
//!
//! > **The seven-day trap.** A Google Cloud project whose consent screen is left
//! > in *Testing* issues refresh tokens that expire after seven days, and no
//! > client configuration avoids it. Google documents only that case, so the
//! > reconnect path here runs regardless of why a grant died.

use crate::error::Error;
use crate::ledger::Ledger;
use crate::publish::crypto::TokenCipher;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// The only scope that still works. Broader read and sharing scopes were
/// withdrawn on 31 March 2025 (§6.1).
pub const SCOPE: &str = "https://www.googleapis.com/auth/photoslibrary.appendonly";

pub const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";

/// The provider key in the `oauth` table.
pub const PROVIDER: &str = "google";

/// Where the pending CSRF nonce is kept between the consent redirect and the
/// callback.
const STATE_SETTING: &str = "google_oauth_state";

/// Refresh this long before the access token actually expires.
///
/// A token that expires during a 500-photograph run would fail one request for
/// no reason; renewing early costs one extra refresh a day.
const REFRESH_MARGIN_SECS: i64 = 120;

/// Credentials for a **Web application** client (§6.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

impl OAuthConfig {
    pub fn from_env() -> Result<Self, Error> {
        let get = |name: &str| -> Result<String, Error> {
            std::env::var(name)
                .ok()
                .filter(|v| !v.trim().is_empty())
                .ok_or_else(|| {
                    Error::Config(format!(
                        "{name} is not set. Google Photos publishing needs a Web \
                         application OAuth client (specification §6.2)."
                    ))
                })
        };

        Ok(Self {
            client_id: get("GOOGLE_OAUTH_CLIENT_ID")?,
            client_secret: get("GOOGLE_OAUTH_CLIENT_SECRET")?,
            redirect_uri: get("GOOGLE_OAUTH_REDIRECT_URI")?,
        })
    }

    /// The consent URL to send somebody to (§6.2 step 1).
    ///
    /// `access_type=offline` is what asks for a refresh token at all, and
    /// `prompt=consent` is what makes Google issue a *new* one rather than
    /// silently reusing a grant it already has — without it, a reconnect after
    /// an expired token returns no refresh token and the reconnect does nothing.
    pub fn consent_url(&self, state: &str) -> String {
        let params = [
            ("client_id", self.client_id.as_str()),
            ("redirect_uri", self.redirect_uri.as_str()),
            ("response_type", "code"),
            ("scope", SCOPE),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("state", state),
        ];

        let query = params
            .iter()
            .map(|(k, v)| format!("{k}={}", percent_encode(v)))
            .collect::<Vec<_>>()
            .join("&");

        format!("{AUTH_ENDPOINT}?{query}")
    }
}

/// Percent-encode a query parameter value.
///
/// Hand-rolled rather than pulled in: the alphabet is small, the rule is
/// RFC 3986's unreserved set, and a dependency for twelve lines is not a trade
/// worth making.
fn percent_encode(value: &str) -> String {
    use std::fmt::Write;
    value.bytes().fold(String::new(), |mut out, b| {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            other => {
                let _ = write!(out, "%{other:02X}");
            }
        }
        out
    })
}

/// What Google's token endpoint returns.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    /// Present on the initial exchange, absent on a refresh.
    #[serde(default)]
    pub refresh_token: Option<String>,
    #[serde(default)]
    pub expires_in: i64,
    #[serde(default)]
    pub scope: Option<String>,
}

/// Why a token request failed.
///
/// `InvalidGrant` is separated from every other failure because it is the only
/// one that means *stop*: the grant is gone and no amount of retrying brings it
/// back. Everything else is worth trying again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    /// The refresh token is dead — revoked, expired, or seven days old on a
    /// project still in Testing. Requires a human to re-authorise.
    InvalidGrant(String),
    /// The request never got an answer.
    Transport(String),
    /// An answer, but not a good one.
    Refused { status: u16, detail: String },
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TokenError::InvalidGrant(d) => write!(
                f,
                "the Google account's authorisation is no longer valid ({d}). \
                 Reconnect it to publish again."
            ),
            TokenError::Transport(d) => write!(f, "could not reach Google: {d}"),
            TokenError::Refused { status, detail } => {
                write!(f, "Google refused the token request ({status}): {detail}")
            }
        }
    }
}

impl From<TokenError> for Error {
    fn from(e: TokenError) -> Self {
        match e {
            TokenError::InvalidGrant(_) => Error::Config(e.to_string()),
            other => Error::Internal(other.to_string()),
        }
    }
}

/// The token endpoint, as a seam.
///
/// Every test in this crate uses a fake. Nothing here reaches Google, which is
/// this phase's stated requirement and not merely a convenience.
pub trait TokenEndpoint: Send + Sync {
    fn exchange_code(&self, config: &OAuthConfig, code: &str) -> Result<TokenResponse, TokenError>;
    fn refresh(
        &self,
        config: &OAuthConfig,
        refresh_token: &str,
    ) -> Result<TokenResponse, TokenError>;
}

/// What the UI needs to show about the connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorStatus {
    pub connected: bool,
    pub scope: Option<String>,
    /// When the *refresh* grant was last renewed, as Unix seconds.
    pub connected_at: Option<i64>,
    /// True when somebody must visit the consent screen again.
    pub needs_reauthorisation: bool,
    /// Words a person can act on.
    pub detail: Option<String>,
}

/// The stored connector states.
const CONNECTED: &str = "connected";
const DISCONNECTED: &str = "disconnected";

/// The Google Photos connector: one account's standing permission to append.
pub struct Connector<'a> {
    ledger: &'a Ledger,
    config: OAuthConfig,
    cipher: TokenCipher,
    endpoint: &'a dyn TokenEndpoint,
    /// The access token, held only in memory. It lives about an hour and there
    /// is nothing to gain by writing it to disk beside the thing it is derived
    /// from.
    cached: Mutex<Option<CachedToken>>,
}

#[derive(Debug, Clone)]
struct CachedToken {
    access_token: String,
    expires_at: i64,
}

impl<'a> Connector<'a> {
    pub fn new(
        ledger: &'a Ledger,
        config: OAuthConfig,
        cipher: TokenCipher,
        endpoint: &'a dyn TokenEndpoint,
    ) -> Self {
        Self {
            ledger,
            config,
            cipher,
            endpoint,
            cached: Mutex::new(None),
        }
    }

    /// Begin the flow: a consent URL, and the nonce that ties the callback to it.
    ///
    /// The `state` nonce is not decoration. Without it, anyone who can make the
    /// photographer's browser hit the callback can bind **their** Google account
    /// to this server, and every photograph published afterwards goes to a
    /// stranger's library.
    pub fn begin(&self) -> Result<String, Error> {
        let state = random_state();
        self.ledger
            .set_setting(STATE_SETTING, &state)
            .map_err(|e| Error::Internal(e.to_string()))?;
        Ok(self.config.consent_url(&state))
    }

    /// Finish the flow: check the nonce, exchange the code, store the grant
    /// (§6.2 steps 2–4).
    pub fn complete(&self, code: &str, state: &str) -> Result<(), Error> {
        let expected = self
            .ledger
            .get_setting(STATE_SETTING)
            .map_err(|e| Error::Internal(e.to_string()))?
            .ok_or_else(|| {
                Error::Config(
                    "there is no authorisation in progress. Start the connection again.".into(),
                )
            })?;

        // Constant-time is overkill for a nonce that is used once and then
        // deleted, but the comparison must still happen before anything else.
        if state != expected {
            return Err(Error::AccessDenied(
                "the authorisation response did not match the request that started it".into(),
            ));
        }

        // Single-use, whatever happens next: a nonce that survives a failed
        // exchange is a nonce that can be replayed.
        self.ledger
            .set_setting(STATE_SETTING, "")
            .map_err(|e| Error::Internal(e.to_string()))?;

        let token = self.endpoint.exchange_code(&self.config, code)?;

        let refresh = token.refresh_token.ok_or_else(|| {
            Error::Config(
                "Google returned no refresh token. This happens when the consent \
                 screen was skipped because a grant already existed; the request \
                 sends prompt=consent to avoid it. Revoke PhotoTools' access in \
                 the Google account and connect again."
                    .into(),
            )
        })?;

        self.store_grant(&refresh, token.scope.as_deref())?;
        self.cache(&token.access_token, token.expires_in);
        Ok(())
    }

    /// A usable access token, refreshing if the held one is spent.
    ///
    /// **A disconnected connector short-circuits here**, without a request. That
    /// is what stops a batch of 400 photographs turning one dead grant into 400
    /// identical failed calls to Google.
    pub fn access_token(&self) -> Result<String, Error> {
        if let Some(cached) = self.usable_cached_token() {
            return Ok(cached);
        }

        let stored = self.stored_grant()?;
        if stored.state == DISCONNECTED {
            return Err(Error::Config(
                "the Google account is disconnected. Reconnect it to publish.".into(),
            ));
        }

        let refresh = self.cipher.decrypt(&stored.encrypted_refresh_token)?;

        match self.endpoint.refresh(&self.config, &refresh) {
            Ok(token) => {
                self.cache(&token.access_token, token.expires_in);
                // Google may hand back a rotated refresh token; keeping the old
                // one would work until it did not.
                if let Some(rotated) = token.refresh_token {
                    self.store_grant(&rotated, token.scope.as_deref())?;
                }
                Ok(token.access_token)
            }
            Err(TokenError::InvalidGrant(detail)) => {
                // §6.2: catch invalid_grant, mark disconnected, prompt to
                // re-authorise. Marking it here is what makes the next call
                // short-circuit rather than ask again.
                self.mark_disconnected()?;
                Err(Error::Config(TokenError::InvalidGrant(detail).to_string()))
            }
            Err(other) => Err(other.into()),
        }
    }

    pub fn status(&self) -> Result<ConnectorStatus, Error> {
        let Some(stored) = self.stored_grant_opt()? else {
            return Ok(ConnectorStatus {
                connected: false,
                scope: None,
                connected_at: None,
                needs_reauthorisation: false,
                detail: Some("No Google account is connected.".into()),
            });
        };

        let disconnected = stored.state == DISCONNECTED;
        Ok(ConnectorStatus {
            connected: !disconnected,
            scope: Some(stored.scope),
            connected_at: Some(stored.expires_at),
            needs_reauthorisation: disconnected,
            detail: disconnected.then(|| {
                "The Google account's authorisation expired or was revoked. \
                 Reconnect it to publish again."
                    .to_string()
            }),
        })
    }

    /// Forget the grant entirely.
    ///
    /// The row is deleted rather than flagged: a disconnect is somebody saying
    /// "stop", and a refresh token nobody intends to use should not sit in the
    /// database waiting to be stolen.
    pub fn disconnect(&self) -> Result<(), Error> {
        self.ledger
            .delete_oauth(PROVIDER)
            .map_err(|e| Error::Internal(e.to_string()))?;
        if let Ok(mut cached) = self.cached.lock() {
            *cached = None;
        }
        Ok(())
    }

    fn usable_cached_token(&self) -> Option<String> {
        let cached = self.cached.lock().ok()?;
        let held = cached.as_ref()?;
        (held.expires_at > chrono::Utc::now().timestamp() + REFRESH_MARGIN_SECS)
            .then(|| held.access_token.clone())
    }

    fn cache(&self, access_token: &str, expires_in: i64) {
        if let Ok(mut cached) = self.cached.lock() {
            *cached = Some(CachedToken {
                access_token: access_token.to_string(),
                expires_at: chrono::Utc::now().timestamp() + expires_in,
            });
        }
    }

    fn store_grant(&self, refresh_token: &str, scope: Option<&str>) -> Result<(), Error> {
        let encrypted = self.cipher.encrypt(refresh_token)?;
        self.ledger
            .set_oauth_grant(
                PROVIDER,
                &encrypted,
                scope.unwrap_or(SCOPE),
                chrono::Utc::now().timestamp(),
                CONNECTED,
            )
            .map_err(|e| Error::Internal(e.to_string()))
    }

    fn mark_disconnected(&self) -> Result<(), Error> {
        if let Ok(mut cached) = self.cached.lock() {
            *cached = None;
        }
        self.ledger
            .set_oauth_state(PROVIDER, DISCONNECTED)
            .map_err(|e| Error::Internal(e.to_string()))
    }

    fn stored_grant(&self) -> Result<crate::ledger::OAuthGrant, Error> {
        self.stored_grant_opt()?.ok_or_else(|| {
            Error::Config("No Google account is connected. Connect one to publish.".into())
        })
    }

    fn stored_grant_opt(&self) -> Result<Option<crate::ledger::OAuthGrant>, Error> {
        self.ledger
            .oauth_grant(PROVIDER)
            .map_err(|e| Error::Internal(e.to_string()))
    }
}

impl crate::publish::publisher::AccessTokens for Connector<'_> {
    fn access_token(&self) -> Result<String, Error> {
        Connector::access_token(self)
    }

    /// Drop the held access token so the next call refreshes.
    ///
    /// Called when Google refuses a token that had not yet expired by our
    /// reckoning — a clock difference, or a revocation part way through a run.
    fn invalidate(&self) {
        if let Ok(mut cached) = self.cached.lock() {
            *cached = None;
        }
    }
}

/// A one-time nonce for the OAuth `state` parameter.
fn random_state() -> String {
    use chacha20poly1305::aead::{AeadCore, OsRng};
    use chacha20poly1305::ChaCha20Poly1305;

    // Two nonces' worth of OS randomness, hex-encoded. Reaching for the AEAD's
    // generator rather than adding `rand` for one call.
    let a = ChaCha20Poly1305::generate_nonce(&mut OsRng);
    let b = ChaCha20Poly1305::generate_nonce(&mut OsRng);

    use std::fmt::Write;
    a.iter().chain(b.iter()).fold(String::new(), |mut s, byte| {
        let _ = write!(s, "{byte:02x}");
        s
    })
}

// ---------------------------------------------------------------------------
// The real token endpoint
// ---------------------------------------------------------------------------

/// Google's OAuth token endpoint over HTTP.
///
/// The base URL is configurable so a local mock can stand in for
/// `oauth2.googleapis.com` — every test in this crate points it at `127.0.0.1`
/// or replaces it with a fake, and nothing reaches Google.
pub struct HttpTokenEndpoint {
    client: reqwest::blocking::Client,
    endpoint: String,
}

impl Default for HttpTokenEndpoint {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpTokenEndpoint {
    pub fn new() -> Self {
        Self::at(TOKEN_ENDPOINT)
    }

    pub fn at(endpoint: &str) -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            endpoint: endpoint.to_string(),
        }
    }

    fn post(&self, form: &[(&str, &str)]) -> Result<TokenResponse, TokenError> {
        // The body is built here rather than through `reqwest`'s form helper,
        // which needs a feature this crate does not otherwise want. The encoding
        // rule is the same one the consent URL uses.
        let body = form
            .iter()
            .map(|(k, v)| format!("{k}={}", percent_encode(v)))
            .collect::<Vec<_>>()
            .join("&");

        let response = self
            .client
            .post(&self.endpoint)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .map_err(|e: reqwest::Error| TokenError::Transport(e.to_string()))?;

        let status = response.status();
        let body = response.text().unwrap_or_default();

        if status.is_success() {
            return serde_json::from_str(&body)
                .map_err(|e| TokenError::Transport(format!("unreadable token response: {e}")));
        }

        // `invalid_grant` is the one answer that means stop rather than retry:
        // the grant is revoked, or seven days old on a project still in Testing
        // (§6.2). It arrives as a 400 with a named error.
        #[derive(Deserialize, Default)]
        struct OAuthError {
            #[serde(default)]
            error: String,
            #[serde(default)]
            error_description: String,
        }

        let parsed: OAuthError = serde_json::from_str(&body).unwrap_or_default();
        if parsed.error == "invalid_grant" {
            let detail = if parsed.error_description.is_empty() {
                "invalid_grant".to_string()
            } else {
                parsed.error_description
            };
            return Err(TokenError::InvalidGrant(detail));
        }

        Err(TokenError::Refused {
            status: status.as_u16(),
            detail: if parsed.error.is_empty() {
                body
            } else {
                format!("{}: {}", parsed.error, parsed.error_description)
            },
        })
    }
}

impl TokenEndpoint for HttpTokenEndpoint {
    fn exchange_code(&self, config: &OAuthConfig, code: &str) -> Result<TokenResponse, TokenError> {
        self.post(&[
            ("code", code),
            ("client_id", &config.client_id),
            ("client_secret", &config.client_secret),
            ("redirect_uri", &config.redirect_uri),
            ("grant_type", "authorization_code"),
        ])
    }

    fn refresh(
        &self,
        config: &OAuthConfig,
        refresh_token: &str,
    ) -> Result<TokenResponse, TokenError> {
        self.post(&[
            ("refresh_token", refresh_token),
            ("client_id", &config.client_id),
            ("client_secret", &config.client_secret),
            ("grant_type", "refresh_token"),
        ])
    }
}
