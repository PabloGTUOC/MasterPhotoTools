#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("Configuration error: {0}")]
    Config(String),
    #[error("Access denied: {0}")]
    AccessDenied(String),
    #[error("Job error: {0}")]
    Job(String),
    #[error("Internal error: {0}")]
    Internal(String),
}
