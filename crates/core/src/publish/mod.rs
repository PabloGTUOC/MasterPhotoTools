//! Google Photos client (F15, specification §6).
//!
//! Four pieces, each answering one of §6's constraints:
//!
//! - [`crypto`] — the refresh token is encrypted at rest (§6.2 step 4).
//! - [`auth`] — the authorization-code flow, and the reconnect path that
//!   catches a dead grant instead of failing silently.
//! - [`api`] — the two calls that still work, and the `429` floor (§6.1).
//! - [`publisher`] — the state machine that makes a non-idempotent batch API
//!   safe to retry (§6.3).

pub mod api;
pub mod auth;
pub mod crypto;
pub mod publisher;

pub use api::{
    rate_limit_delay, ApiError, CreateResult, HttpPhotosApi, NewMediaItem, PhotosApi, RealSleeper,
    Sleeper, MAX_BATCH, RATE_LIMIT_FLOOR,
};
pub use auth::{
    Connector, ConnectorStatus, HttpTokenEndpoint, OAuthConfig, TokenEndpoint, TokenError,
    TokenResponse, SCOPE,
};
pub use crypto::TokenCipher;
pub use publisher::{
    batch_count, dry_run, plan_publish, publishable, AccessTokens, PublishItem, PublishOutcome,
    PublishPlan, Publisher, ResumeCounts, Skipped,
};
