//! The Google Photos API surface, and the one real client for it (§6.1).
//!
//! Two calls, because that is all the API offers that still works:
//! `POST /v1/uploads` returns an upload token, and
//! `POST /v1/mediaItems:batchCreate` turns up to fifty tokens into media items.
//!
//! **Uploads are safe to retry; creates are not.** An upload token that is never
//! used costs nothing and Google discards it. `batchCreate` is not idempotent —
//! §6.3 — and the API cannot delete, so a create whose answer never arrived is
//! not something to try again hopefully. That asymmetry is why [`ApiError`]
//! distinguishes an answer that never came from a request that never left.

use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;

/// The most items `batchCreate` accepts in one call (§6.1).
pub const MAX_BATCH: usize = 50;

/// The floor under a `429` retry, from §6.1: *"`429` responses require at least
/// 30 seconds before retrying"*.
pub const RATE_LIMIT_FLOOR: Duration = Duration::from_secs(30);

/// How many times to wait out a `429` before giving up on a request.
pub const MAX_RATE_LIMIT_RETRIES: u32 = 5;

pub const UPLOAD_ENDPOINT: &str = "https://photoslibrary.googleapis.com/v1/uploads";
pub const BATCH_CREATE_ENDPOINT: &str =
    "https://photoslibrary.googleapis.com/v1/mediaItems:batchCreate";

/// One item to create, once its bytes are uploaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMediaItem {
    pub upload_token: String,
    /// Shown in Google Photos. The stem, so a photograph can be found by the
    /// name the camera gave it.
    pub file_name: String,
}

/// What became of one item in a `batchCreate`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateResult {
    Created { media_item_id: String },
    Failed { detail: String },
}

/// Why a call to Google failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    /// `429`. Wait at least [`RATE_LIMIT_FLOOR`], then try again.
    RateLimited { retry_after: Option<Duration> },
    /// `401`. The access token is spent; refresh and retry.
    Unauthorized,
    /// The request provably never reached Google — a connection that was never
    /// established. Safe to retry, whatever the call.
    NotSent(String),
    /// The request left, and no answer came back. **Never safe to retry a
    /// create**: Google may have made the items.
    NoAnswer(String),
    /// Google answered, and the answer was no.
    Refused { status: u16, detail: String },
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApiError::RateLimited { .. } => f.write_str("Google is rate-limiting this account"),
            ApiError::Unauthorized => f.write_str("the access token was refused"),
            ApiError::NotSent(d) => write!(f, "could not reach Google: {d}"),
            ApiError::NoAnswer(d) => write!(f, "Google did not answer: {d}"),
            ApiError::Refused { status, detail } => {
                write!(f, "Google refused the request ({status}): {detail}")
            }
        }
    }
}

impl ApiError {
    /// Whether retrying could create a second copy of a photograph.
    pub fn may_have_been_applied(&self) -> bool {
        matches!(self, ApiError::NoAnswer(_))
    }
}

/// The API, as a seam.
///
/// Every test in this crate uses a fake or a local mock. Nothing reaches
/// Google, which the build plan requires in as many words.
pub trait PhotosApi: Send + Sync {
    fn upload(&self, access_token: &str, path: &Path, file_name: &str) -> Result<String, ApiError>;

    fn batch_create(
        &self,
        access_token: &str,
        items: &[NewMediaItem],
    ) -> Result<Vec<CreateResult>, ApiError>;
}

/// Waiting, as a seam.
///
/// A test that proved the 30-second floor by actually sleeping for 30 seconds
/// would be a test nobody runs. The claim worth asserting is that the code
/// *asked* to wait long enough, which is what this makes visible.
pub trait Sleeper: Send + Sync {
    fn sleep(&self, duration: Duration);
}

pub struct RealSleeper;

impl Sleeper for RealSleeper {
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// How long to wait after the `n`th consecutive `429` (§6.1).
///
/// Thirty seconds is a floor, not a starting point: a `Retry-After` shorter than
/// it is ignored, because the API's stated requirement outranks the header.
pub fn rate_limit_delay(attempt: u32, retry_after: Option<Duration>) -> Duration {
    let exponential = RATE_LIMIT_FLOOR * 2u32.saturating_pow(attempt.min(4));
    retry_after.unwrap_or(exponential).max(RATE_LIMIT_FLOOR)
}

// ---------------------------------------------------------------------------
// The real client
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct BatchCreateRequest<'a> {
    #[serde(rename = "newMediaItems")]
    new_media_items: Vec<NewMediaItemPayload<'a>>,
}

#[derive(Serialize)]
struct NewMediaItemPayload<'a> {
    #[serde(rename = "simpleMediaItem")]
    simple_media_item: SimpleMediaItem<'a>,
}

#[derive(Serialize)]
struct SimpleMediaItem<'a> {
    #[serde(rename = "uploadToken")]
    upload_token: &'a str,
    #[serde(rename = "fileName")]
    file_name: &'a str,
}

#[derive(Deserialize)]
struct BatchCreateResponse {
    #[serde(rename = "newMediaItemResults", default)]
    results: Vec<NewMediaItemResult>,
}

#[derive(Deserialize)]
struct NewMediaItemResult {
    #[serde(default)]
    status: Option<GoogleStatus>,
    #[serde(rename = "mediaItem", default)]
    media_item: Option<MediaItem>,
}

#[derive(Deserialize)]
struct GoogleStatus {
    #[serde(default)]
    code: i32,
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct MediaItem {
    #[serde(default)]
    id: String,
}

/// The client that actually talks to Google.
///
/// The base URL is configurable so a local mock can stand in for
/// `photoslibrary.googleapis.com` and the wire format — headers, JSON shape,
/// response parsing — is exercised for real without a single packet leaving the
/// machine.
pub struct HttpPhotosApi {
    client: reqwest::blocking::Client,
    upload_endpoint: String,
    batch_create_endpoint: String,
}

impl Default for HttpPhotosApi {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpPhotosApi {
    pub fn new() -> Self {
        Self::with_endpoints(UPLOAD_ENDPOINT, BATCH_CREATE_ENDPOINT)
    }

    pub fn with_endpoints(upload: &str, batch_create: &str) -> Self {
        Self {
            client: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .unwrap_or_default(),
            upload_endpoint: upload.to_string(),
            batch_create_endpoint: batch_create.to_string(),
        }
    }

    /// Classify a transport failure by whether Google can have seen it.
    ///
    /// A connection that was never established provably delivered nothing. Any
    /// other failure — a timeout above all — may have delivered the request and
    /// lost the answer, and for a create that difference is a duplicate
    /// photograph.
    fn classify(e: &reqwest::Error) -> ApiError {
        if e.is_connect() {
            ApiError::NotSent(e.to_string())
        } else {
            ApiError::NoAnswer(e.to_string())
        }
    }

    fn status_error(
        status: reqwest::StatusCode,
        retry_after: Option<Duration>,
        body: String,
    ) -> ApiError {
        match status.as_u16() {
            429 => ApiError::RateLimited { retry_after },
            401 => ApiError::Unauthorized,
            other => ApiError::Refused {
                status: other,
                detail: body,
            },
        }
    }

    fn retry_after_of(response: &reqwest::blocking::Response) -> Option<Duration> {
        response
            .headers()
            .get("retry-after")?
            .to_str()
            .ok()?
            .trim()
            .parse::<u64>()
            .ok()
            .map(Duration::from_secs)
    }
}

impl PhotosApi for HttpPhotosApi {
    fn upload(&self, access_token: &str, path: &Path, file_name: &str) -> Result<String, ApiError> {
        let bytes = std::fs::read(path)
            .map_err(|e| ApiError::NotSent(format!("{}: {e}", path.display())))?;

        let response = self
            .client
            .post(&self.upload_endpoint)
            .bearer_auth(access_token)
            .header("Content-Type", "application/octet-stream")
            .header("X-Goog-Upload-Content-Type", "image/jpeg")
            .header("X-Goog-Upload-Protocol", "raw")
            .header("X-Goog-File-Name", file_name)
            .body(bytes)
            .send()
            .map_err(|e| Self::classify(&e))?;

        let status = response.status();
        let retry_after = Self::retry_after_of(&response);
        let body = response.text().unwrap_or_default();

        if !status.is_success() {
            return Err(Self::status_error(status, retry_after, body));
        }

        let token = body.trim().to_string();
        if token.is_empty() {
            // §9.2 invariant 6: an upload whose token did not arrive is not a
            // successful upload, whatever the status code said.
            return Err(ApiError::Refused {
                status: status.as_u16(),
                detail: "the upload returned no token".into(),
            });
        }
        Ok(token)
    }

    fn batch_create(
        &self,
        access_token: &str,
        items: &[NewMediaItem],
    ) -> Result<Vec<CreateResult>, ApiError> {
        if items.len() > MAX_BATCH {
            return Err(ApiError::NotSent(format!(
                "{} items in one batchCreate; the API takes at most {MAX_BATCH}",
                items.len()
            )));
        }

        let request = BatchCreateRequest {
            new_media_items: items
                .iter()
                .map(|item| NewMediaItemPayload {
                    simple_media_item: SimpleMediaItem {
                        upload_token: &item.upload_token,
                        file_name: &item.file_name,
                    },
                })
                .collect(),
        };

        let response = self
            .client
            .post(&self.batch_create_endpoint)
            .bearer_auth(access_token)
            .json(&request)
            .send()
            .map_err(|e| Self::classify(&e))?;

        let status = response.status();
        let retry_after = Self::retry_after_of(&response);
        let body = response.text().unwrap_or_default();

        if !status.is_success() {
            return Err(Self::status_error(status, retry_after, body));
        }

        let parsed: BatchCreateResponse = serde_json::from_str(&body).map_err(|e| {
            // The items may well have been created; we simply cannot say which.
            ApiError::NoAnswer(format!("could not read the batchCreate response: {e}"))
        })?;

        if parsed.results.len() != items.len() {
            return Err(ApiError::NoAnswer(format!(
                "batchCreate answered for {} of {} items",
                parsed.results.len(),
                items.len()
            )));
        }

        Ok(parsed
            .results
            .into_iter()
            .map(|result| {
                let ok = result.status.as_ref().map(|s| s.code == 0).unwrap_or(true);
                match result.media_item {
                    Some(item) if ok && !item.id.is_empty() => CreateResult::Created {
                        media_item_id: item.id,
                    },
                    _ => CreateResult::Failed {
                        detail: result
                            .status
                            .map(|s| s.message)
                            .filter(|m| !m.is_empty())
                            .unwrap_or_else(|| "Google created no item and gave no reason".into()),
                    },
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_rate_limit_wait_is_the_documented_floor() {
        assert_eq!(rate_limit_delay(0, None), RATE_LIMIT_FLOOR);
        assert!(rate_limit_delay(0, None) >= Duration::from_secs(30));
    }

    #[test]
    fn rate_limit_waits_grow_from_the_floor() {
        assert_eq!(rate_limit_delay(1, None), Duration::from_secs(60));
        assert_eq!(rate_limit_delay(2, None), Duration::from_secs(120));
        assert!(rate_limit_delay(3, None) > rate_limit_delay(2, None));
    }

    #[test]
    fn a_retry_after_shorter_than_the_floor_is_ignored() {
        // §6.1 states a minimum of thirty seconds. A header asking for five does
        // not override the API's own rule.
        let delay = rate_limit_delay(0, Some(Duration::from_secs(5)));
        assert_eq!(delay, RATE_LIMIT_FLOOR);
    }

    #[test]
    fn a_retry_after_longer_than_the_floor_is_honoured() {
        let delay = rate_limit_delay(0, Some(Duration::from_secs(90)));
        assert_eq!(delay, Duration::from_secs(90));
    }

    #[test]
    fn the_backoff_does_not_overflow_on_a_long_run_of_refusals() {
        // `Duration * 2^attempt` is an easy way to panic in release. Capped.
        let delay = rate_limit_delay(u32::MAX, None);
        assert!(delay >= RATE_LIMIT_FLOOR);
    }

    #[test]
    fn only_a_lost_answer_is_treated_as_possibly_applied() {
        assert!(ApiError::NoAnswer("timeout".into()).may_have_been_applied());
        assert!(!ApiError::NotSent("refused".into()).may_have_been_applied());
        assert!(!ApiError::Unauthorized.may_have_been_applied());
        assert!(!ApiError::RateLimited { retry_after: None }.may_have_been_applied());
        assert!(!ApiError::Refused {
            status: 400,
            detail: String::new()
        }
        .may_have_been_applied());
    }

    #[test]
    fn a_batch_larger_than_the_api_allows_is_refused_before_it_is_sent() {
        let api = HttpPhotosApi::new();
        let items: Vec<NewMediaItem> = (0..MAX_BATCH + 1)
            .map(|i| NewMediaItem {
                upload_token: format!("t{i}"),
                file_name: format!("IMG_{i}.jpg"),
            })
            .collect();

        let err = api.batch_create("token", &items).unwrap_err();
        assert!(matches!(err, ApiError::NotSent(_)), "got {err}");
        assert!(!err.may_have_been_applied());
    }
}
