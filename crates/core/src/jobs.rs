//! Long-running work and progress reporting (F17)

use crate::error::Error;
use serde::{Deserialize, Serialize};

/// Reported by every long-running operation as it works.
pub trait Progress: Send + Sync {
    fn report(&self, done: u64, total: u64, message: &str);
    fn cancelled(&self) -> bool;
}

/// Where a job is in its lifecycle.
///
/// `Interrupted` is not reachable from within a running process: it is applied
/// on startup to jobs that were still `Pending` or `Running` when the previous
/// process stopped. F17 requires that such a job resumes or reports failure and
/// never silently disappears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Interrupted,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Pending => "pending",
            JobStatus::Running => "running",
            JobStatus::Completed => "completed",
            JobStatus::Failed => "failed",
            JobStatus::Interrupted => "interrupted",
        }
    }

    /// True once the job will not change again without being re-run.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Interrupted
        )
    }
}

impl std::fmt::Display for JobStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for JobStatus {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(JobStatus::Pending),
            "running" => Ok(JobStatus::Running),
            "completed" => Ok(JobStatus::Completed),
            "failed" => Ok(JobStatus::Failed),
            "interrupted" => Ok(JobStatus::Interrupted),
            other => Err(Error::Job(format!("unknown job status {other:?}"))),
        }
    }
}

/// A persisted job — one row of the `jobs` table (specification §7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub kind: String,
    pub status: JobStatus,
    pub progress: u64,
    pub total: u64,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub error: Option<String>,
}

impl Job {
    /// A new job, pending, started now.
    pub fn new(id: impl Into<String>, kind: impl Into<String>, total: u64) -> Self {
        Self {
            id: id.into(),
            kind: kind.into(),
            status: JobStatus::Pending,
            progress: 0,
            total,
            started_at: chrono::Utc::now().timestamp(),
            finished_at: None,
            error: None,
        }
    }
}

/// Progress snapshot pushed to clients over SSE or Tauri events.
///
/// Distinct from [`Job`], which is the persisted row. Phase 5 should collapse
/// the two once the real job runner exists.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct JobState {
    pub id: String,
    pub kind: String,
    pub state: String,
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
    fn job_status_round_trips_through_its_string_form() {
        for status in [
            JobStatus::Pending,
            JobStatus::Running,
            JobStatus::Completed,
            JobStatus::Failed,
            JobStatus::Interrupted,
        ] {
            let parsed: JobStatus = status.as_str().parse().unwrap();
            assert_eq!(parsed, status);
        }
        assert!("nonsense".parse::<JobStatus>().is_err());
    }

    #[test]
    fn only_finished_statuses_are_terminal() {
        assert!(!JobStatus::Pending.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
        assert!(JobStatus::Completed.is_terminal());
        assert!(JobStatus::Failed.is_terminal());
        assert!(JobStatus::Interrupted.is_terminal());
    }

    #[test]
    fn a_new_job_starts_pending_with_no_progress() {
        let job = Job::new("job1", "date_scan", 400);
        assert_eq!(job.status, JobStatus::Pending);
        assert_eq!(job.progress, 0);
        assert_eq!(job.total, 400);
        assert!(job.finished_at.is_none());
        assert!(job.error.is_none());
    }
}
