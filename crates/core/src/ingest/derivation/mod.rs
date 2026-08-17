//! Derivative generation for ingest: staged, publishable copies of each shot.

pub mod f14;
pub mod worker;
pub use f14::{
    derive_batch, derive_batch_with, requests_for, DerivationRequest, DerivationSummary,
    DerivedShot,
};
pub use worker::{DeriveJob, Derived, WorkerPool};
