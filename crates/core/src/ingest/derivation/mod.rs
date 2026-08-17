//! Derivative generation for ingest: staged, publishable copies of each shot.

pub mod worker;
pub use worker::{DeriveJob, Derived, WorkerPool};
