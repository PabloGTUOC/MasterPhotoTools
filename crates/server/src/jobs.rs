//! Job execution and progress streaming (F17).
//!
//! Every long-running operation is a job with a persisted row and a progress
//! stream. **No request blocks until its operation completes**: a request starts
//! a job and returns its identifier immediately.

use phototools_core::error::Error;
use phototools_core::jobs::{Job, JobStatus, Progress};
use phototools_core::ledger::Ledger;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// How many progress events to buffer per job before a slow subscriber misses some.
const EVENT_BUFFER: usize = 256;

/// One frame of a job's progress stream.
#[derive(Debug, Clone, Serialize)]
pub struct JobEvent {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub progress: u64,
    pub total: u64,
    pub message: String,
    /// True for the last event a subscriber will receive.
    pub terminal: bool,
}

/// Runs jobs and keeps their state durable.
pub struct JobManager {
    /// `rusqlite::Connection` is not `Sync`, so the ledger is behind a mutex.
    /// Job writes are small and infrequent relative to the work itself.
    ledger: Arc<Mutex<Ledger>>,
    channels: Mutex<std::collections::HashMap<String, broadcast::Sender<JobEvent>>>,
}

impl JobManager {
    pub fn new(ledger: Ledger) -> Self {
        Self {
            ledger: Arc::new(Mutex::new(ledger)),
            channels: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Mark jobs orphaned by a previous process, at startup.
    ///
    /// F17 — an interrupted job resumes or reports failure, and never silently
    /// disappears.
    pub fn recover(&self) -> Result<Vec<Job>, Error> {
        self.ledger
            .lock()
            .expect("ledger mutex poisoned")
            .recover_interrupted_jobs()
            .map_err(|e| Error::Internal(e.to_string()))
    }

    pub fn get(&self, id: &str) -> Result<Option<Job>, Error> {
        self.ledger
            .lock()
            .expect("ledger mutex poisoned")
            .get_job(id)
            .map_err(|e| Error::Internal(e.to_string()))
    }

    /// Subscribe to a running job's events, if it is still running.
    pub fn subscribe(&self, id: &str) -> Option<broadcast::Receiver<JobEvent>> {
        self.channels
            .lock()
            .expect("channel map poisoned")
            .get(id)
            .map(|tx| tx.subscribe())
    }

    /// Start `work` as a job and return its id immediately.
    ///
    /// The closure runs on the blocking pool: the archive tools are synchronous
    /// and CPU-bound, and running them on an async worker would stall the
    /// runtime for every other request.
    pub fn spawn<F>(&self, kind: &str, total: u64, work: F) -> Result<String, Error>
    where
        F: FnOnce(&dyn Progress) -> Result<String, Error> + Send + 'static,
    {
        let id = uuid::Uuid::new_v4().to_string();
        let job = Job::new(id.clone(), kind, total);

        // Persisted before any work starts, so a crash mid-operation still
        // leaves a recoverable record.
        self.ledger
            .lock()
            .expect("ledger mutex poisoned")
            .insert_job(&job)
            .map_err(|e| Error::Internal(e.to_string()))?;

        let (tx, _) = broadcast::channel(EVENT_BUFFER);
        self.channels
            .lock()
            .expect("channel map poisoned")
            .insert(id.clone(), tx.clone());

        let reporter = LedgerProgress {
            id: id.clone(),
            kind: kind.to_string(),
            total,
            ledger: Arc::clone(&self.ledger),
            events: tx.clone(),
        };

        let ledger = Arc::clone(&self.ledger);
        let finished_id = id.clone();
        let finished_kind = kind.to_string();

        tokio::task::spawn_blocking(move || {
            let outcome = work(&reporter);

            let (status, error, message) = match &outcome {
                Ok(summary) => (JobStatus::Completed, None, summary.clone()),
                Err(e) => (JobStatus::Failed, Some(e.to_string()), e.to_string()),
            };

            if let Ok(guard) = ledger.lock() {
                let _ = guard.finish_job(&finished_id, status, error.as_deref());
            }

            // Terminal event, so a subscriber knows the stream has ended rather
            // than waiting on a connection that will never speak again.
            let _ = tx.send(JobEvent {
                id: finished_id,
                kind: finished_kind,
                state: status.as_str().to_string(),
                progress: total,
                total,
                message,
                terminal: true,
            });
        });

        Ok(id)
    }

    /// Drop the channel for a finished job. Called once its stream closes.
    pub fn release(&self, id: &str) {
        self.channels
            .lock()
            .expect("channel map poisoned")
            .remove(id);
    }
}

/// A [`Progress`] that persists to the ledger and fans out to subscribers.
struct LedgerProgress {
    id: String,
    kind: String,
    total: u64,
    ledger: Arc<Mutex<Ledger>>,
    events: broadcast::Sender<JobEvent>,
}

impl Progress for LedgerProgress {
    fn report(&self, done: u64, total: u64, message: &str) {
        let total = if total == 0 { self.total } else { total };

        if let Ok(guard) = self.ledger.lock() {
            let _ = guard.update_job_progress(&self.id, done, total);
        }

        // A send with no subscribers is not an error; the job runs regardless of
        // whether anyone is watching.
        let _ = self.events.send(JobEvent {
            id: self.id.clone(),
            kind: self.kind.clone(),
            state: JobStatus::Running.as_str().to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn manager() -> JobManager {
        JobManager::new(Ledger::open_in_memory().unwrap())
    }

    #[tokio::test]
    async fn spawning_returns_immediately_and_the_job_exists_at_once() {
        let m = manager();

        let started = std::time::Instant::now();
        let id = m
            .spawn("slow", 1, |_p| {
                std::thread::sleep(Duration::from_millis(400));
                Ok("done".into())
            })
            .unwrap();
        let elapsed = started.elapsed();

        assert!(
            elapsed < Duration::from_millis(150),
            "F17: the request must not block on the work, took {elapsed:?}"
        );

        // The record is durable before the work finishes.
        let job = m.get(&id).unwrap().expect("job row should exist already");
        assert_eq!(job.kind, "slow");
        assert!(!job.status.is_terminal());
    }

    #[tokio::test]
    async fn a_completed_job_reaches_a_terminal_state() {
        let m = manager();
        let id = m
            .spawn("quick", 3, |p| {
                p.report(1, 3, "one");
                p.report(3, 3, "three");
                Ok("finished".into())
            })
            .unwrap();

        // Wait for the blocking task to land.
        for _ in 0..100 {
            if m.get(&id).unwrap().is_some_and(|j| j.status.is_terminal()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        let job = m.get(&id).unwrap().unwrap();
        assert_eq!(job.status, JobStatus::Completed);
        assert!(job.finished_at.is_some());
        assert!(job.error.is_none());
    }

    #[tokio::test]
    async fn a_failing_job_records_its_error_rather_than_vanishing() {
        let m = manager();
        let id = m
            .spawn("doomed", 1, |_p| {
                Err(Error::Internal("the disk caught fire".into()))
            })
            .unwrap();

        for _ in 0..100 {
            if m.get(&id).unwrap().is_some_and(|j| j.status.is_terminal()) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        let job = m.get(&id).unwrap().unwrap();
        assert_eq!(job.status, JobStatus::Failed);
        assert!(job.error.unwrap().contains("disk caught fire"));
    }

    #[tokio::test]
    async fn a_subscriber_receives_progress_and_a_terminal_event() {
        let m = manager();

        // Subscribe before the work can finish.
        let id = m
            .spawn("watched", 2, |p| {
                std::thread::sleep(Duration::from_millis(150));
                p.report(1, 2, "half");
                std::thread::sleep(Duration::from_millis(50));
                Ok("all done".into())
            })
            .unwrap();

        let mut rx = m.subscribe(&id).expect("a running job can be watched");

        let mut progress_events = 0;
        let mut saw_terminal = false;
        while let Ok(event) = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
            let Ok(event) = event else { break };
            if event.terminal {
                saw_terminal = true;
                assert_eq!(event.state, "completed");
                break;
            }
            progress_events += 1;
        }

        assert!(progress_events >= 1, "expected at least one progress event");
        assert!(saw_terminal, "the stream must end with a terminal event");
    }

    #[tokio::test]
    async fn an_unknown_job_is_none_and_has_no_stream() {
        let m = manager();
        assert!(m.get("nope").unwrap().is_none());
        assert!(m.subscribe("nope").is_none());
    }
}
