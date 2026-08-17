//! `phototools-core` — all PhotoTools functionality.
//!
//! No web framework, no UI, no platform assumptions. Compiled into both
//! `phototools-server` and `phototools-desktop`; see specification §2.2.

pub mod config;
pub mod error;
pub mod ingest;
pub mod jobs;
pub mod ledger;
pub mod media;
pub mod publish;
pub mod tools;
