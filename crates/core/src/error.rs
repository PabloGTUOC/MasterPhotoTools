//! The single crate-wide error type.

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Configuration error: {0}")]
    Config(String),
    /// A path outside the configured roots (G6).
    ///
    /// Reported to callers with a fixed message that never says whether the
    /// path exists, so the variant carries its detail for the log alone.
    #[error("Access denied: {0}")]
    AccessDenied(String),
    /// A refusal that is **not** about a path, and whose reason is safe to
    /// repeat back.
    ///
    /// Exists because `AccessDenied` is answered with "Path is outside the
    /// configured library roots" whatever produced it — so an OAuth callback
    /// whose `state` did not match was told its problem was a filesystem
    /// permission. A refusal that explains itself wrongly is worse than one
    /// that says nothing.
    #[error("{0}")]
    Refused(String),
    #[error("Job error: {0}")]
    Job(String),
    #[error("Internal error: {0}")]
    Internal(String),
}
