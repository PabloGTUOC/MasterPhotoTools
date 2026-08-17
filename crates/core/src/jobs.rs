//! Long-running work and progress reporting (F17)

use crate::error::Error;
use crate::ledger::Ledger;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

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

/// One frame of a job's progress, as delivered to a watcher.
///
/// Transport-free on purpose: the server turns these into Server-Sent Events
/// and the desktop into Tauri events (F17), and neither shape belongs here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct JobUpdate {
    pub id: String,
    pub kind: String,
    pub state: JobStatus,
    pub progress: u64,
    pub total: u64,
    pub message: String,
    /// True for the last update a watcher will receive.
    pub terminal: bool,
}

/// Where a running job's updates go.
pub trait JobEventSink: Send + Sync + 'static {
    fn emit(&self, update: &JobUpdate);
}

/// A sink that drops everything, for callers that only want persistence.
pub struct NoEvents;

impl JobEventSink for NoEvents {
    fn emit(&self, _update: &JobUpdate) {}
}

/// Runs long operations as persisted, observable jobs (F17).
///
/// Both binaries need this, so it lives here rather than being written twice
/// (G1). The runner owns persistence and threading; each binary supplies a sink
/// that turns updates into its own transport.
pub struct JobRunner {
    /// `rusqlite::Connection` is not `Sync`, so the ledger sits behind a mutex.
    /// Job writes are small and rare next to the work itself.
    ledger: Arc<Mutex<Ledger>>,
    sink: Arc<dyn JobEventSink>,
}

impl JobRunner {
    pub fn new(ledger: Ledger, sink: Arc<dyn JobEventSink>) -> Self {
        Self {
            ledger: Arc::new(Mutex::new(ledger)),
            sink,
        }
    }

    /// A handle on the same ledger the runner writes jobs through.
    ///
    /// Shared rather than reopened: two connections to one SQLite file would
    /// contend for the write lock, and card detection reads the ledger on every
    /// mount while jobs may be writing to it.
    pub fn ledger(&self) -> Arc<Mutex<Ledger>> {
        Arc::clone(&self.ledger)
    }

    /// Mark jobs orphaned by a previous process. Call once at startup.
    pub fn recover(&self) -> Result<Vec<Job>, Error> {
        self.with_ledger(|l| l.recover_interrupted_jobs())
    }

    pub fn get(&self, id: &str) -> Result<Option<Job>, Error> {
        self.with_ledger(|l| l.get_job(id))
    }

    fn with_ledger<T, F>(&self, f: F) -> Result<T, Error>
    where
        F: FnOnce(&Ledger) -> rusqlite::Result<T>,
    {
        let guard = self
            .ledger
            .lock()
            .map_err(|_| Error::Job("ledger lock poisoned".into()))?;
        f(&guard).map_err(|e| Error::Internal(e.to_string()))
    }

    /// Start `work` on its own thread and return the job id immediately.
    ///
    /// **No caller blocks until the operation completes** (F17). The row is
    /// written before the thread starts, so a crash mid-operation still leaves a
    /// recoverable record.
    pub fn spawn<F>(&self, kind: &str, total: u64, work: F) -> Result<String, Error>
    where
        F: FnOnce(&dyn Progress) -> Result<String, Error> + Send + 'static,
    {
        self.spawn_with_id(next_job_id(), kind, total, work)
    }

    /// As [`spawn`](Self::spawn), with an id the caller chose.
    ///
    /// A caller that must register a listener *before* the job can emit needs to
    /// know the id first; generating it here would leave a window in which
    /// updates have nowhere to go.
    pub fn spawn_with_id<F>(
        &self,
        id: impl Into<String>,
        kind: &str,
        total: u64,
        work: F,
    ) -> Result<String, Error>
    where
        F: FnOnce(&dyn Progress) -> Result<String, Error> + Send + 'static,
    {
        let job = Job::new(id, kind, total);
        let id = job.id.clone();
        self.with_ledger(|l| l.insert_job(&job))?;

        let reporter = SinkProgress {
            id: id.clone(),
            kind: kind.to_string(),
            total,
            ledger: Arc::clone(&self.ledger),
            sink: Arc::clone(&self.sink),
        };

        let ledger = Arc::clone(&self.ledger);
        let sink = Arc::clone(&self.sink);
        let finished_id = id.clone();
        let finished_kind = kind.to_string();

        std::thread::Builder::new()
            .name(format!("job-{kind}"))
            .spawn(move || {
                let (status, error, message) = match work(&reporter) {
                    Ok(summary) => (JobStatus::Completed, None, summary),
                    Err(e) => (JobStatus::Failed, Some(e.to_string()), e.to_string()),
                };

                if let Ok(guard) = ledger.lock() {
                    let _ = guard.finish_job(&finished_id, status, error.as_deref());
                }

                // A terminal update, so a watcher knows the stream has ended
                // rather than waiting on something that will never speak again.
                sink.emit(&JobUpdate {
                    id: finished_id,
                    kind: finished_kind,
                    state: status,
                    progress: total,
                    total,
                    message,
                    terminal: true,
                });
            })
            .map_err(|e| Error::Job(format!("could not start job thread: {e}")))?;

        Ok(id)
    }
}

/// Monotonic-enough job identifier without pulling in a UUID dependency.
fn next_job_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:016x}{seq:08x}")
}

/// A [`Progress`] that persists to the ledger and forwards to a sink.
struct SinkProgress {
    id: String,
    kind: String,
    total: u64,
    ledger: Arc<Mutex<Ledger>>,
    sink: Arc<dyn JobEventSink>,
}

impl Progress for SinkProgress {
    fn report(&self, done: u64, total: u64, message: &str) {
        let total = if total == 0 { self.total } else { total };

        if let Ok(guard) = self.ledger.lock() {
            let _ = guard.update_job_progress(&self.id, done, total);
        }

        self.sink.emit(&JobUpdate {
            id: self.id.clone(),
            kind: self.kind.clone(),
            state: JobStatus::Running,
            progress: done,
            total,
            message: message.to_string(),
            terminal: false,
        });
    }

    fn cancelled(&self) -> bool {
        false
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
