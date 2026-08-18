//! Job execution for the web front end (F17).
//!
//! A thin adapter over `core`'s [`JobRunner`]: the running, persistence and
//! recovery all live in `core` because the desktop needs them too (G1). What is
//! server-specific is the transport — turning each update into a Server-Sent
//! Event a browser can subscribe to.

use phototools_core::error::Error;
use phototools_core::jobs::{Job, JobEventSink, JobRunner, JobUpdate, Progress};
use phototools_core::ledger::Ledger;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;

/// How many updates to buffer per job before a slow subscriber misses some.
const EVENT_BUFFER: usize = 256;

/// The SSE payload. Mirrors [`JobUpdate`] with the state rendered as a string,
/// which is what a JavaScript client wants to switch on.
#[derive(Debug, Clone, serde::Serialize)]
pub struct JobEvent {
    pub id: String,
    pub kind: String,
    pub state: String,
    pub progress: u64,
    pub total: u64,
    pub message: String,
    pub terminal: bool,
}

impl From<&JobUpdate> for JobEvent {
    fn from(update: &JobUpdate) -> Self {
        Self {
            id: update.id.clone(),
            kind: update.kind.clone(),
            state: update.state.as_str().to_string(),
            progress: update.progress,
            total: update.total,
            message: update.message.clone(),
            terminal: update.terminal,
        }
    }
}

/// Fans job updates out to whichever browsers are watching.
type Channels = Mutex<HashMap<String, broadcast::Sender<JobEvent>>>;

struct BroadcastSink {
    channels: Arc<Channels>,
}

impl JobEventSink for BroadcastSink {
    fn emit(&self, update: &JobUpdate) {
        let Ok(channels) = self.channels.lock() else {
            return;
        };
        if let Some(tx) = channels.get(&update.id) {
            // A send with no subscribers is not an error; the job runs whether
            // or not anyone is watching.
            let _ = tx.send(JobEvent::from(update));
        }
    }
}

pub struct JobManager {
    runner: JobRunner,
    channels: Arc<Channels>,
}

impl JobManager {
    pub fn new(ledger: Ledger) -> Self {
        let channels: Arc<Channels> = Arc::new(Mutex::new(HashMap::new()));
        let sink = Arc::new(BroadcastSink {
            channels: Arc::clone(&channels),
        });
        Self {
            runner: JobRunner::new(ledger, sink),
            channels,
        }
    }

    pub fn recover(&self) -> Result<Vec<Job>, Error> {
        self.runner.recover()
    }

    /// A handle on the same ledger jobs are written through.
    ///
    /// Shared rather than reopened: two connections to one SQLite file contend
    /// for the write lock, and the ingest handlers read the ledger while jobs
    /// are writing to it.
    pub fn ledger(&self) -> Arc<Mutex<phototools_core::ledger::Ledger>> {
        self.runner.ledger()
    }

    pub fn get(&self, id: &str) -> Result<Option<Job>, Error> {
        self.runner.get(id)
    }

    /// Subscribe to a running job's events, if it is still running.
    pub fn subscribe(&self, id: &str) -> Option<broadcast::Receiver<JobEvent>> {
        self.channels.lock().ok()?.get(id).map(|tx| tx.subscribe())
    }

    /// Start a job and return its id immediately. Never waits for the work.
    pub fn spawn<F>(&self, kind: &str, total: u64, work: F) -> Result<String, Error>
    where
        F: FnOnce(&dyn Progress) -> Result<String, Error> + Send + 'static,
    {
        // The id is chosen here so the channel is registered before the job can
        // emit anything into it. Letting the runner pick would leave a window in
        // which the first updates had nowhere to go.
        let id = uuid::Uuid::new_v4().to_string();

        let (tx, _) = broadcast::channel(EVENT_BUFFER);
        if let Ok(mut channels) = self.channels.lock() {
            channels.insert(id.clone(), tx);
        }

        match self.runner.spawn_with_id(id.clone(), kind, total, work) {
            Ok(id) => Ok(id),
            Err(e) => {
                self.release(&id);
                Err(e)
            }
        }
    }

    /// Drop the channel for a finished job.
    pub fn release(&self, id: &str) {
        if let Ok(mut channels) = self.channels.lock() {
            channels.remove(id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use phototools_core::jobs::JobStatus;
    use std::time::Duration;

    fn manager() -> JobManager {
        JobManager::new(Ledger::open_in_memory().unwrap())
    }

    async fn settle(m: &JobManager, id: &str) -> Job {
        for _ in 0..200 {
            if let Some(job) = m.get(id).unwrap() {
                if job.status.is_terminal() {
                    return job;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("job {id} never reached a terminal state");
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

        let job = settle(&m, &id).await;
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

        let job = settle(&m, &id).await;
        assert_eq!(job.status, JobStatus::Failed);
        assert!(job.error.unwrap().contains("disk caught fire"));
    }

    #[tokio::test]
    async fn a_subscriber_receives_progress_and_a_terminal_event() {
        let m = manager();

        let id = m
            .spawn("watched", 2, |p| {
                std::thread::sleep(Duration::from_millis(200));
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
