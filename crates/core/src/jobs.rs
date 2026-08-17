//! Long-running work and progress reporting (F17)

use crate::error::Error;
use serde::{Deserialize, Serialize};

pub trait Progress: Send + Sync {
    fn report(&self, done: u64, total: u64, message: &str);
    fn cancelled(&self) -> bool;
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobState {
    pub id: String,
    pub kind: String,
    pub state: String, // pending, running, completed, failed
    pub progress: u64,
    pub total: u64,
}

pub struct InMemoryProgress {
    cancelled: std::sync::atomic::AtomicBool,
}

impl InMemoryProgress {
    pub fn new() -> Self {
        Self {
            cancelled: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Default for InMemoryProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl Progress for InMemoryProgress {
    fn report(&self, _done: u64, _total: u64, _message: &str) {
        // No-op for tests
    }

    fn cancelled(&self) -> bool {
        self.cancelled.load(std::sync::atomic::Ordering::Relaxed)
    }
}

pub type ToolResult<T> = Result<Outcome<T>, Error>;

pub struct Outcome<T> {
    pub data: T,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_job_recovery_simulation() {
        let job = JobState {
            id: "job1".to_string(),
            kind: "scan".to_string(),
            state: "running".to_string(),
            progress: 50,
            total: 100,
        };
        let serialized = serde_json::to_string(&job).unwrap();

        // Simulate restart
        let recovered: JobState = serde_json::from_str(&serialized).unwrap();
        assert_eq!(recovered, job);
    }
}
